use ash::vk;
use std::error::Error;

use crate::vulkan::VulkanDevice;

pub struct FrameReader<'a> {
    dev: &'a VulkanDevice,
    pool: vk::CommandPool,
    slots: Vec<Slot>,
    next: usize,
    width: u32,
    height: u32,
    size: u64,
}

struct Slot {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *const u8,
    coherent: bool,
    cb: vk::CommandBuffer,
    fence: vk::Fence,
}

impl<'a> FrameReader<'a> {
    pub fn new(
        dev: &'a VulkanDevice,
        width: u32,
        height: u32,
        slot_count: usize,
    ) -> Result<Self, Box<dyn Error>> {
        assert!(slot_count > 0);
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(dev.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let pool = unsafe { dev.device.create_command_pool(&pool_info, None)? };

        let mut reader = Self {
            dev,
            pool,
            slots: Vec::with_capacity(slot_count),
            next: 0,
            width,
            height,
            size: width as u64 * height as u64 * 4,
        };
        for _ in 0..slot_count {
            let slot = reader.create_slot()?;
            reader.slots.push(slot);
        }
        Ok(reader)
    }

    pub fn submit(&mut self, src: vk::Image) -> Result<usize, Box<dyn Error>> {
        let idx = self.next;
        self.next = (self.next + 1) % self.slots.len();

        let d = &self.dev.device;
        let slot = &self.slots[idx];
        unsafe {
            d.wait_for_fences(&[slot.fence], true, u64::MAX)?;
            d.reset_fences(&[slot.fence])?;
            d.reset_command_buffer(slot.cb, vk::CommandBufferResetFlags::empty())?;
        }
        self.record_copy(slot.cb, src, slot.buffer)?;

        let cbs = [slot.cb];
        let submit = vk::SubmitInfo::default().command_buffers(&cbs);
        unsafe { d.queue_submit(self.dev.queue, &[submit], slot.fence)? };
        Ok(idx)
    }

    pub fn read<R>(&self, slot: usize, f: impl FnOnce(&[u8]) -> R) -> Result<R, Box<dyn Error>> {
        let d = &self.dev.device;
        let slot = &self.slots[slot];
        unsafe { d.wait_for_fences(&[slot.fence], true, u64::MAX)? };

        if !slot.coherent {
            let range = vk::MappedMemoryRange::default()
                .memory(slot.memory)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            unsafe { d.invalidate_mapped_memory_ranges(&[range])? };
        }

        let bytes = unsafe { std::slice::from_raw_parts(slot.ptr, self.size as usize) };
        Ok(f(bytes))
    }

    pub fn capture_blocking<R>(
        &mut self,
        src: vk::Image,
        f: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, Box<dyn Error>> {
        let slot = self.submit(src)?;
        self.read(slot, f)
    }

    fn create_slot(&self) -> Result<Slot, Box<dyn Error>> {
        let d = &self.dev.device;

        let buffer_info = vk::BufferCreateInfo::default()
            .size(self.size)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { d.create_buffer(&buffer_info, None)? };

        let reqs = unsafe { d.get_buffer_memory_requirements(buffer) };
        let (memory_type_index, coherent) = find_readback_memory(self.dev, reqs.memory_type_bits)?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(reqs.size)
            .memory_type_index(memory_type_index);
        let memory = unsafe { d.allocate_memory(&alloc, None)? };
        unsafe { d.bind_buffer_memory(buffer, memory, 0)? };

        let ptr = unsafe { d.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())? }
            as *const u8;

        let cb_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let cb = unsafe { d.allocate_command_buffers(&cb_info)? }[0];

        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = unsafe { d.create_fence(&fence_info, None)? };

        Ok(Slot {
            buffer,
            memory,
            ptr,
            coherent,
            cb,
            fence,
        })
    }

    fn record_copy(
        &self,
        cb: vk::CommandBuffer,
        src: vk::Image,
        dst: vk::Buffer,
    ) -> Result<(), Box<dyn Error>> {
        let d = &self.dev.device;
        let qfi = self.dev.queue_family_index;

        let color = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };

        let acquire = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(qfi)
            .image(src)
            .subresource_range(color);

        let release = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::empty())
            .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(qfi)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .image(src)
            .subresource_range(color);

        let to_host = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ);

        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(vk::Extent3D {
                width: self.width,
                height: self.height,
                depth: 1,
            });

        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            d.begin_command_buffer(cb, &begin)?;
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[acquire],
            );
            d.cmd_copy_image_to_buffer(
                cb,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst,
                &[region],
            );
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[to_host],
                &[],
                &[],
            );
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[release],
            );
            d.end_command_buffer(cb)?;
        }
        Ok(())
    }
}

impl Drop for FrameReader<'_> {
    fn drop(&mut self) {
        let d = &self.dev.device;
        unsafe {
            let _ = d.device_wait_idle();
            for s in &self.slots {
                d.destroy_fence(s.fence, None);
                d.destroy_buffer(s.buffer, None);
                d.free_memory(s.memory, None);
            }
            d.destroy_command_pool(self.pool, None);
        }
    }
}

pub(crate) fn find_readback_memory(
    dev: &VulkanDevice,
    type_bits: u32,
) -> Result<(u32, bool), Box<dyn Error>> {
    let props = unsafe { dev.instance.get_physical_device_memory_properties(dev.pdev) };
    let find = |wanted: vk::MemoryPropertyFlags| {
        (0..props.memory_type_count).find(|&i| {
            type_bits & (1 << i) != 0
                && props.memory_types[i as usize]
                    .property_flags
                    .contains(wanted)
        })
    };

    let index = find(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_CACHED)
        .or_else(|| find(vk::MemoryPropertyFlags::HOST_VISIBLE))
        .ok_or("no host-visible memory type")?;

    let coherent = props.memory_types[index as usize]
        .property_flags
        .contains(vk::MemoryPropertyFlags::HOST_COHERENT);
    Ok((index, coherent))
}
