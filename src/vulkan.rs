use ash::vk::TaggedStructure;
use ash::{Device, Entry, Instance, vk};
use drm_fourcc::DrmFourcc;
use std::error::Error;
use std::ffi::c_char;
use std::os::fd::{AsRawFd, IntoRawFd};

pub fn vk_format(fourcc: DrmFourcc) -> Option<vk::Format> {
    match fourcc {
        DrmFourcc::Abgr8888 | DrmFourcc::Xbgr8888 => Some(vk::Format::R8G8B8A8_UNORM),
        DrmFourcc::Argb8888 | DrmFourcc::Xrgb8888 => Some(vk::Format::B8G8R8A8_UNORM),
        DrmFourcc::Abgr2101010 | DrmFourcc::Xbgr2101010 => {
            Some(vk::Format::A2B10G10R10_UNORM_PACK32)
        }
        DrmFourcc::Argb2101010 | DrmFourcc::Xrgb2101010 => {
            Some(vk::Format::A2R10G10B10_UNORM_PACK32)
        }
        _ => None,
    }
}

pub struct VulkanDevice {
    pub entry: Entry,
    pub instance: Instance,
    pub pdev: vk::PhysicalDevice,
    pub device: Device,
    pub queue: vk::Queue,
    pub queue_family_index: u32,
    pub encode_queue: vk::Queue,
    pub encode_queue_family_index: u32,
}

impl VulkanDevice {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let entry = Entry::linked();
        let instance = create_instance(&entry)?;
        let pdev = pick_device(&instance)?;
        let (device, (queue, queue_family_index), (encode_queue, encode_queue_family_index)) =
            create_logical_device(&instance, pdev)?;

        Ok(Self {
            entry,
            instance,
            pdev,
            device,
            queue,
            queue_family_index,
            encode_queue,
            encode_queue_family_index,
        })
    }

    pub fn supported_modifiers(
        &self,
        format: vk::Format,
    ) -> Vec<vk::DrmFormatModifierPropertiesEXT> {
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
        let mut props = vk::FormatProperties2::default().push(&mut list);
        unsafe {
            self.instance
                .get_physical_device_format_properties2(self.pdev, format, &mut props)
        };
        let count = list.drm_format_modifier_count as usize;

        let mut mods = vec![vk::DrmFormatModifierPropertiesEXT::default(); count];
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
            .drm_format_modifier_properties(&mut mods);
        let mut props = vk::FormatProperties2::default().push(&mut list);
        unsafe {
            self.instance
                .get_physical_device_format_properties2(self.pdev, format, &mut props)
        };

        mods
    }

    pub fn import_frame(
        &self,
        frame: &crate::kms::Frame,
        usage: vk::ImageUsageFlags,
        mut profiles: Option<vk::VideoProfileListInfoKHR>,
    ) -> Result<(vk::Image, vk::DeviceMemory), Box<dyn Error>> {
        let format = vk_format(frame.fourcc)
            .ok_or_else(|| format!("unsupported plane format {}", frame.fourcc))?;
        let layouts: Vec<vk::SubresourceLayout> = frame
            .planes
            .iter()
            .map(|&(offset, pitch)| vk::SubresourceLayout {
                offset: offset as u64,
                row_pitch: pitch as u64,
                ..Default::default()
            })
            .collect();

        let mut modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(frame.modifier)
            .plane_layouts(&layouts);
        let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let mut image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: frame.width,
                height: frame.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push(&mut modifier_info)
            .push(&mut external_info);
        if let Some(list) = profiles.as_mut() {
            image_info = image_info.push(list);
        }

        let image = unsafe { self.device.create_image(&image_info, None)? };

        let fd = frame.dmabuf.try_clone()?;
        let fd_loader = ash::khr::external_memory_fd::Device::load(&self.instance, &self.device);
        let mut fd_props = vk::MemoryFdPropertiesKHR::default();
        unsafe {
            fd_loader.get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                fd.as_raw_fd(),
                &mut fd_props,
            )?
        };

        let reqs = unsafe { self.device.get_image_memory_requirements(image) };
        let type_bits = reqs.memory_type_bits & fd_props.memory_type_bits;
        let memory_type_index = type_bits.trailing_zeros();
        if type_bits == 0 {
            return Err("no compatible memory type".into());
        }

        let mut import_info = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(fd.into_raw_fd());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(reqs.size)
            .memory_type_index(memory_type_index)
            .push(&mut import_info)
            .push(&mut dedicated);

        let memory = unsafe { self.device.allocate_memory(&alloc_info, None)? };
        unsafe { self.device.bind_image_memory(image, memory, 0)? };

        Ok((image, memory))
    }
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

fn create_instance(entry: &Entry) -> Result<Instance, Box<dyn Error>> {
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"cliprs")
        .api_version(vk::API_VERSION_1_3);

    let layers: Vec<*const c_char> = if cfg!(debug_assertions) {
        vec![c"VK_LAYER_KHRONOS_validation".as_ptr()]
    } else {
        vec![]
    };

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_layer_names(&layers);

    Ok(unsafe { entry.create_instance(&create_info, None)? })
}

fn pick_device(instance: &Instance) -> Result<vk::PhysicalDevice, Box<dyn Error>> {
    let devices = unsafe { instance.enumerate_physical_devices()? };
    devices
        .into_iter()
        .next()
        .ok_or("no Vulkan device found".into())
}

fn create_logical_device(
    instance: &Instance,
    pdev: vk::PhysicalDevice,
) -> Result<(Device, (vk::Queue, u32), (vk::Queue, u32)), Box<dyn Error>> {
    let queue_families = unsafe { instance.get_physical_device_queue_family_properties(pdev) };
    let find_family = |flag: vk::QueueFlags| {
        queue_families
            .iter()
            .position(|q| q.queue_flags.contains(flag))
            .map(|i| i as u32)
    };
    let queue_family_index =
        find_family(vk::QueueFlags::GRAPHICS).ok_or("no graphics queue family")?;
    let encode_queue_family_index =
        find_family(vk::QueueFlags::VIDEO_ENCODE_KHR).ok_or("no video encode queue family")?;

    let priority = [1.0];
    let queue_info = [
        vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priority),
        vk::DeviceQueueCreateInfo::default()
            .queue_family_index(encode_queue_family_index)
            .queue_priorities(&priority),
    ];

    let extensions = [
        ash::khr::external_memory_fd::NAME.as_ptr(),
        ash::ext::external_memory_dma_buf::NAME.as_ptr(),
        ash::ext::image_drm_format_modifier::NAME.as_ptr(),
        ash::ext::queue_family_foreign::NAME.as_ptr(),
        ash::khr::video_queue::NAME.as_ptr(),
        ash::khr::video_encode_queue::NAME.as_ptr(),
        ash::khr::video_encode_h264::NAME.as_ptr(),
        vk::VALVE_VIDEO_ENCODE_RGB_CONVERSION_NAME.as_ptr(),
    ];

    let mut features13 = vk::PhysicalDeviceVulkan13Features::default().synchronization2(true);
    let mut rgb_features =
        vk::PhysicalDeviceVideoEncodeRgbConversionFeaturesVALVE::default()
            .video_encode_rgb_conversion(true);

    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_info)
        .enabled_extension_names(&extensions)
        .push(&mut features13)
        .push(&mut rgb_features);

    let device = unsafe { instance.create_device(pdev, &device_info, None)? };
    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
    let encode_queue = unsafe { device.get_device_queue(encode_queue_family_index, 0) };

    Ok((
        device,
        (queue, queue_family_index),
        (encode_queue, encode_queue_family_index),
    ))
}
