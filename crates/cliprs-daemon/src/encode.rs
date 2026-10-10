use ash::khr;
use ash::vk::{self, TaggedStructure, native};
use cliprs_ipc::Settings;
use std::error::Error;
use std::time::Instant;

use crate::kms::Frame;
use crate::readback::find_readback_memory;
use crate::vulkan::{VulkanDevice, vk_format};

const ENCODABLE_FORMATS: [vk::Format; 2] = [vk::Format::R8G8B8A8_UNORM, vk::Format::B8G8R8A8_UNORM];
const DPB_FORMAT: vk::Format = vk::Format::G8_B8R8_2PLANE_420_UNORM;
const DPB_SLOTS: usize = 2;
pub const GOP_LENGTH: u32 = 120;
const RATE_WINDOW_MS: u32 = 1000;
const BITSTREAM_CAPACITY: u64 = 8 * 1024 * 1024;
const NO_REFERENCE: u8 = 0xFF;

const LEVELS: [(native::StdVideoH264LevelIdc, u64, u64); 5] = [
    (
        native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_0,
        245_760,
        8_192,
    ),
    (
        native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_2,
        522_240,
        8_704,
    ),
    (
        native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_0,
        589_824,
        22_080,
    ),
    (
        native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_1,
        983_040,
        36_864,
    ),
    (
        native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_2,
        2_073_600,
        36_864,
    ),
];

pub fn with_encode_profile<R>(f: impl FnOnce(&vk::VideoProfileInfoKHR<'_>) -> R) -> R {
    let mut h264 = vk::VideoEncodeH264ProfileInfoKHR::default()
        .std_profile_idc(native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN);
    let mut rgb =
        vk::VideoEncodeProfileRgbConversionInfoVALVE::default().perform_encode_rgb_conversion(true);
    let profile = vk::VideoProfileInfoKHR::default()
        .video_codec_operation(vk::VideoCodecOperationFlagsKHR::ENCODE_H264)
        .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
        .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
        .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
        .push(&mut h264)
        .push(&mut rgb);
    f(&profile)
}

fn with_profile_list<R>(f: impl FnOnce(&vk::VideoProfileListInfoKHR<'_>) -> R) -> R {
    with_encode_profile(|profile| {
        let profiles = [*profile];
        let list = vk::VideoProfileListInfoKHR::default().profiles(&profiles);
        f(&list)
    })
}

pub struct Sample {
    pub data: Vec<u8>,
    pub is_idr: bool,
    pub captured_at: Instant,
}

struct DpbSlot {
    image: vk::Image,
    view: vk::ImageView,
    memory: vk::DeviceMemory,
    reference: Option<native::StdVideoEncodeH264ReferenceInfo>,
}

struct Staging {
    image: vk::Image,
    memory: vk::DeviceMemory,
    pool: vk::CommandPool,
    cb: vk::CommandBuffer,
    layout: vk::ImageLayout,
}

impl Staging {
    fn new(
        vk: &VulkanDevice,
        extent: vk::Extent2D,
        format: vk::Format,
    ) -> Result<Self, Box<dyn Error>> {
        let d = &vk.device;
        let (image, memory) = create_image(
            vk,
            extent,
            format,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR,
            true,
        )?;
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(vk.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let pool = unsafe { d.create_command_pool(&pool_info, None)? };
        let cb_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let cb = unsafe { d.allocate_command_buffers(&cb_info)? }[0];
        Ok(Self {
            image,
            memory,
            pool,
            cb,
            layout: vk::ImageLayout::UNDEFINED,
        })
    }

    fn blit_from(
        &mut self,
        vk: &VulkanDevice,
        src: vk::Image,
        extent: vk::Extent2D,
    ) -> Result<(), Box<dyn Error>> {
        let d = &vk.device;
        let qfi = vk.queue_family_index;
        let (previous_owner, owner) = if self.layout == vk::ImageLayout::UNDEFINED {
            (vk::QUEUE_FAMILY_IGNORED, vk::QUEUE_FAMILY_IGNORED)
        } else {
            (vk::QUEUE_FAMILY_FOREIGN_EXT, qfi)
        };

        let acquire = [
            vk::ImageMemoryBarrier::default()
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .dst_queue_family_index(qfi)
                .image(src)
                .subresource_range(color_range()),
            vk::ImageMemoryBarrier::default()
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(self.layout)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(previous_owner)
                .dst_queue_family_index(owner)
                .image(self.image)
                .subresource_range(color_range()),
        ];
        let release = [
            vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(qfi)
                .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .image(src)
                .subresource_range(color_range()),
            vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(qfi)
                .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .image(self.image)
                .subresource_range(color_range()),
        ];

        let layers = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        let corners = [
            vk::Offset3D::default(),
            vk::Offset3D {
                x: extent.width as i32,
                y: extent.height as i32,
                z: 1,
            },
        ];
        let region = vk::ImageBlit::default()
            .src_subresource(layers)
            .src_offsets(corners)
            .dst_subresource(layers)
            .dst_offsets(corners);

        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            d.reset_command_buffer(self.cb, vk::CommandBufferResetFlags::empty())?;
            d.begin_command_buffer(self.cb, &begin)?;
            d.cmd_pipeline_barrier(
                self.cb,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &acquire,
            );
            d.cmd_blit_image(
                self.cb,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
                vk::Filter::NEAREST,
            );
            d.cmd_pipeline_barrier(
                self.cb,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &release,
            );
            d.end_command_buffer(self.cb)?;
            let cbs = [self.cb];
            let submit = vk::SubmitInfo::default().command_buffers(&cbs);
            d.queue_submit(vk.queue, &[submit], vk::Fence::null())?;
            d.queue_wait_idle(vk.queue)?;
        }
        self.layout = vk::ImageLayout::GENERAL;
        Ok(())
    }
}

pub struct Encoder<'a> {
    vk: &'a VulkanDevice,
    video_queue: khr::video_queue::Device,
    video_encode_queue: khr::video_encode_queue::Device,
    extent: vk::Extent2D,
    fps: u32,
    average_bitrate: u64,
    peak_bitrate: u64,
    picture_format: vk::Format,
    staging: Option<Staging>,
    session: vk::VideoSessionKHR,
    session_memory: Vec<vk::DeviceMemory>,
    parameters: vk::VideoSessionParametersKHR,
    headers: Vec<u8>,
    dpb: Vec<DpbSlot>,
    bitstream: vk::Buffer,
    bitstream_memory: vk::DeviceMemory,
    bitstream_ptr: *const u8,
    bitstream_coherent: bool,
    bitstream_range: u64,
    query_pool: vk::QueryPool,
    pool: vk::CommandPool,
    cb: vk::CommandBuffer,
    fence: vk::Fence,
    idr_count: u16,
    frame_in_gop: u32,
}

impl<'a> Encoder<'a> {
    pub fn new(
        vk: &'a VulkanDevice,
        width: u32,
        height: u32,
        settings: &Settings,
        source_format: vk::Format,
    ) -> Result<Self, Box<dyn Error>> {
        let fps = settings.fps;
        let extent = vk::Extent2D { width, height };
        let picture_format = if ENCODABLE_FORMATS.contains(&source_format) {
            source_format
        } else {
            ENCODABLE_FORMATS[0]
        };
        let video_queue = khr::video_queue::Device::load(&vk.instance, &vk.device);
        let video_encode_queue = khr::video_encode_queue::Device::load(&vk.instance, &vk.device);

        let caps = query_capabilities(vk, extent, settings.peak_bitrate_bps())?;
        let level = pick_level(width, height, fps, caps.max_level_idc);

        let (session, session_memory) = create_session(
            vk,
            &video_queue,
            extent,
            picture_format,
            caps.max_active_references,
            &caps.std_header_version,
        )?;

        let vui = build_vui(fps);
        let mut sps = build_sps(extent, level);
        sps.flags.set_vui_parameters_present_flag(1);
        sps.pSequenceParameterSetVui = &vui;
        let pps = build_pps();
        let parameters = create_parameters(&video_queue, session, &sps, &pps)?;
        let headers = fetch_headers(&video_encode_queue, parameters)?;

        let mut encoder = Self {
            vk,
            video_queue,
            video_encode_queue,
            extent,
            fps,
            average_bitrate: settings.average_bitrate_bps,
            peak_bitrate: settings.peak_bitrate_bps(),
            picture_format,
            staging: None,
            session,
            session_memory,
            parameters,
            headers,
            dpb: Vec::new(),
            bitstream: vk::Buffer::null(),
            bitstream_memory: vk::DeviceMemory::null(),
            bitstream_ptr: std::ptr::null(),
            bitstream_coherent: true,
            bitstream_range: align_up(BITSTREAM_CAPACITY, caps.size_alignment),
            query_pool: vk::QueryPool::null(),
            pool: vk::CommandPool::null(),
            cb: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            idr_count: 0,
            frame_in_gop: 0,
        };
        encoder.create_resources()?;
        encoder.initialize_session()?;
        Ok(encoder)
    }

    pub fn encode_frame(&mut self, frame: &Frame) -> Result<Sample, Box<dyn Error>> {
        let direct = vk_format(frame.fourcc) == Some(self.picture_format);
        let (image, memory) = if direct {
            with_profile_list(|list| {
                self.vk.import_frame(
                    frame,
                    vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR,
                    Some(*list),
                )
            })?
        } else {
            self.vk
                .import_frame(frame, vk::ImageUsageFlags::TRANSFER_SRC, None)?
        };
        let result = if direct {
            self.encode_image(image)
        } else {
            self.encode_converted(image)
        };
        unsafe {
            self.vk.device.destroy_image(image, None);
            self.vk.device.free_memory(memory, None);
        }
        result
    }

    fn encode_converted(&mut self, src: vk::Image) -> Result<Sample, Box<dyn Error>> {
        if self.staging.is_none() {
            self.staging = Some(Staging::new(self.vk, self.extent, self.picture_format)?);
        }
        let staging = self.staging.as_mut().expect("staging was just created");
        staging.blit_from(self.vk, src, self.extent)?;
        let image = staging.image;
        self.encode_image(image)
    }

    pub fn encode_image(&mut self, src: vk::Image) -> Result<Sample, Box<dyn Error>> {
        let captured_at = Instant::now();
        let idr = self.frame_in_gop == 0;
        if idr {
            self.idr_count = self.idr_count.wrapping_add(1);
        }
        let setup_index = self.frame_in_gop as usize % DPB_SLOTS;
        let ref_index = (!idr).then(|| (self.frame_in_gop as usize + DPB_SLOTS - 1) % DPB_SLOTS);

        let src_view = self.create_src_view(src)?;
        let bytes = self
            .record(src, src_view, idr, setup_index, ref_index)
            .and_then(|()| self.submit_and_read(idr));
        unsafe { self.vk.device.destroy_image_view(src_view, None) };
        let bytes = bytes?;

        self.frame_in_gop = (self.frame_in_gop + 1) % GOP_LENGTH;

        let sample = Sample {
            data: bytes,
            is_idr: idr,
            captured_at,
        };

        Ok(sample)
    }

    fn initialize_session(&self) -> Result<(), Box<dyn Error>> {
        let d = &self.vk.device;
        let barriers: Vec<_> = self
            .dpb
            .iter()
            .map(|slot| {
                vk::ImageMemoryBarrier2::default()
                    .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
                    .dst_access_mask(
                        vk::AccessFlags2::VIDEO_ENCODE_READ_KHR
                            | vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR,
                    )
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::VIDEO_ENCODE_DPB_KHR)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(slot.image)
                    .subresource_range(color_range())
            })
            .collect();
        let deps = vk::DependencyInfo::default().image_memory_barriers(&barriers);

        let begin_info = vk::VideoBeginCodingInfoKHR::default()
            .video_session(self.session)
            .video_session_parameters(self.parameters);
        let layers = rate_control_layers(self.fps, self.average_bitrate, self.peak_bitrate);
        let mut rate_control = rate_control(&layers);
        let mut h264_rate_control = h264_rate_control();
        let control_info = vk::VideoCodingControlInfoKHR::default()
            .flags(
                vk::VideoCodingControlFlagsKHR::RESET
                    | vk::VideoCodingControlFlagsKHR::ENCODE_RATE_CONTROL,
            )
            .push(&mut rate_control)
            .push(&mut h264_rate_control);
        let end_info = vk::VideoEndCodingInfoKHR::default();
        let begin_cb = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        unsafe {
            d.begin_command_buffer(self.cb, &begin_cb)?;
            d.cmd_pipeline_barrier2(self.cb, &deps);
            self.video_queue
                .cmd_begin_video_coding(self.cb, &begin_info);
            self.video_queue
                .cmd_control_video_coding(self.cb, &control_info);
            self.video_queue.cmd_end_video_coding(self.cb, &end_info);
            d.end_command_buffer(self.cb)?;
            d.reset_fences(&[self.fence])?;
            let cbs = [self.cb];
            let submit = vk::SubmitInfo::default().command_buffers(&cbs);
            d.queue_submit(self.vk.encode_queue, &[submit], self.fence)?;
        }
        Ok(())
    }

    fn create_src_view(&self, src: vk::Image) -> Result<vk::ImageView, Box<dyn Error>> {
        let mut usage = vk::ImageViewUsageCreateInfo::default()
            .usage(vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR);
        let info = vk::ImageViewCreateInfo::default()
            .image(src)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(self.picture_format)
            .subresource_range(color_range())
            .push(&mut usage);
        Ok(unsafe { self.vk.device.create_image_view(&info, None)? })
    }

    fn record(
        &mut self,
        src: vk::Image,
        src_view: vk::ImageView,
        idr: bool,
        setup_index: usize,
        ref_index: Option<usize>,
    ) -> Result<(), Box<dyn Error>> {
        let d = &self.vk.device;
        let qfi = self.vk.encode_queue_family_index;

        let frame_num = self.frame_in_gop;
        let poc = 2 * self.frame_in_gop as i32;
        let picture_type = if idr {
            native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_IDR
        } else {
            native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_P
        };

        let mut setup_reference: native::StdVideoEncodeH264ReferenceInfo =
            unsafe { std::mem::zeroed() };
        setup_reference.primary_pic_type = picture_type;
        setup_reference.FrameNum = frame_num;
        setup_reference.PicOrderCnt = poc;

        let mut ref_lists: native::StdVideoEncodeH264ReferenceListsInfo =
            unsafe { std::mem::zeroed() };
        ref_lists.RefPicList0 = [NO_REFERENCE; 32];
        ref_lists.RefPicList1 = [NO_REFERENCE; 32];
        if let Some(r) = ref_index {
            ref_lists.RefPicList0[0] = r as u8;
        }

        let mut picture: native::StdVideoEncodeH264PictureInfo = unsafe { std::mem::zeroed() };
        picture.flags.set_IdrPicFlag(idr as u32);
        picture.flags.set_is_reference(1);
        picture.primary_pic_type = picture_type;
        picture.frame_num = frame_num;
        picture.PicOrderCnt = poc;
        picture.idr_pic_id = self.idr_count;
        picture.pRefLists = &ref_lists;

        let mut slice_header: native::StdVideoEncodeH264SliceHeader = unsafe { std::mem::zeroed() };
        slice_header.slice_type = if idr {
            native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_I
        } else {
            native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_P
        };
        slice_header.cabac_init_idc =
            native::StdVideoH264CabacInitIdc_STD_VIDEO_H264_CABAC_INIT_IDC_0;
        slice_header.disable_deblocking_filter_idc =
            native::StdVideoH264DisableDeblockingFilterIdc_STD_VIDEO_H264_DISABLE_DEBLOCKING_FILTER_IDC_DISABLED;

        let slices =
            [vk::VideoEncodeH264NaluSliceInfoKHR::default().std_slice_header(&slice_header)];
        let mut h264_picture = vk::VideoEncodeH264PictureInfoKHR::default()
            .nalu_slice_entries(&slices)
            .std_picture_info(&picture);

        let full = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: self.extent,
        };
        let resource = |view: vk::ImageView| {
            vk::VideoPictureResourceInfoKHR::default()
                .coded_offset(full.offset)
                .coded_extent(full.extent)
                .base_array_layer(0)
                .image_view_binding(view)
        };

        let setup_resource = resource(self.dpb[setup_index].view);
        let ref_resource = ref_index.map(|r| resource(self.dpb[r].view));
        let src_resource = resource(src_view);

        let ref_std =
            ref_index.map(|r| self.dpb[r].reference.expect("reference slot is populated"));
        let mut ref_dpb_info = ref_std
            .as_ref()
            .map(|s| vk::VideoEncodeH264DpbSlotInfoKHR::default().std_reference_info(s));
        let mut setup_dpb_info =
            vk::VideoEncodeH264DpbSlotInfoKHR::default().std_reference_info(&setup_reference);
        let mut begin_setup_dpb_info =
            vk::VideoEncodeH264DpbSlotInfoKHR::default().std_reference_info(&setup_reference);

        let mut ref_slot = ref_resource.as_ref().map(|res| {
            let slot = vk::VideoReferenceSlotInfoKHR::default()
                .slot_index(ref_index.unwrap() as i32)
                .picture_resource(res);
            slot.push(ref_dpb_info.as_mut().unwrap())
        });
        let encode_setup_slot = vk::VideoReferenceSlotInfoKHR::default()
            .slot_index(setup_index as i32)
            .picture_resource(&setup_resource)
            .push(&mut setup_dpb_info);

        let mut begin_slots = Vec::with_capacity(2);
        if let Some(slot) = ref_slot.take() {
            begin_slots.push(slot);
        }
        begin_slots.push(
            vk::VideoReferenceSlotInfoKHR::default()
                .slot_index(-1)
                .picture_resource(&setup_resource)
                .push(&mut begin_setup_dpb_info),
        );
        let encode_ref_slots: Vec<_> = begin_slots
            .iter()
            .take(ref_index.is_some() as usize)
            .copied()
            .collect();

        let encode_info = vk::VideoEncodeInfoKHR::default()
            .dst_buffer(self.bitstream)
            .dst_buffer_offset(0)
            .dst_buffer_range(self.bitstream_range)
            .src_picture_resource(src_resource)
            .setup_reference_slot(&encode_setup_slot)
            .reference_slots(&encode_ref_slots)
            .push(&mut h264_picture);

        let layers = rate_control_layers(self.fps, self.average_bitrate, self.peak_bitrate);
        let mut rate_control = rate_control(&layers);
        let mut h264_rate_control = h264_rate_control();
        let begin_info = vk::VideoBeginCodingInfoKHR::default()
            .video_session(self.session)
            .video_session_parameters(self.parameters)
            .reference_slots(&begin_slots)
            .push(&mut rate_control)
            .push(&mut h264_rate_control);

        let acquire = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::NONE)
            .src_access_mask(vk::AccessFlags2::NONE)
            .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .dst_access_mask(vk::AccessFlags2::VIDEO_ENCODE_READ_KHR)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::VIDEO_ENCODE_SRC_KHR)
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(qfi)
            .image(src)
            .subresource_range(color_range());
        let release = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .src_access_mask(vk::AccessFlags2::NONE)
            .dst_stage_mask(vk::PipelineStageFlags2::NONE)
            .dst_access_mask(vk::AccessFlags2::NONE)
            .old_layout(vk::ImageLayout::VIDEO_ENCODE_SRC_KHR)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(qfi)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .image(src)
            .subresource_range(color_range());

        let barriers = [acquire];
        let acquire_deps = vk::DependencyInfo::default().image_memory_barriers(&barriers);
        let release_barriers = [release];
        let release_deps = vk::DependencyInfo::default().image_memory_barriers(&release_barriers);

        let end_info = vk::VideoEndCodingInfoKHR::default();
        let begin_cb = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        unsafe {
            d.wait_for_fences(&[self.fence], true, u64::MAX)?;
            d.reset_fences(&[self.fence])?;
            d.reset_command_buffer(self.cb, vk::CommandBufferResetFlags::empty())?;
            d.begin_command_buffer(self.cb, &begin_cb)?;
            d.cmd_pipeline_barrier2(self.cb, &acquire_deps);
            d.cmd_reset_query_pool(self.cb, self.query_pool, 0, 1);
            self.video_queue
                .cmd_begin_video_coding(self.cb, &begin_info);
            d.cmd_begin_query(self.cb, self.query_pool, 0, vk::QueryControlFlags::empty());
            self.video_encode_queue
                .cmd_encode_video(self.cb, &encode_info);
            d.cmd_end_query(self.cb, self.query_pool, 0);
            self.video_queue.cmd_end_video_coding(self.cb, &end_info);
            d.cmd_pipeline_barrier2(self.cb, &release_deps);
            d.end_command_buffer(self.cb)?;
        }

        self.dpb[setup_index].reference = Some(setup_reference);
        Ok(())
    }

    fn submit_and_read(&mut self, idr: bool) -> Result<Vec<u8>, Box<dyn Error>> {
        let d = &self.vk.device;
        let cbs = [self.cb];
        let submit = vk::SubmitInfo::default().command_buffers(&cbs);
        unsafe {
            d.queue_submit(self.vk.encode_queue, &[submit], self.fence)?;
            d.wait_for_fences(&[self.fence], true, u64::MAX)?;
        }

        let mut feedback = [[0u64; 3]; 1];
        unsafe {
            d.get_query_pool_results(
                self.query_pool,
                0,
                &mut feedback,
                vk::QueryResultFlags::WAIT
                    | vk::QueryResultFlags::TYPE_64
                    | vk::QueryResultFlags::WITH_STATUS_KHR,
            )?;
        }
        let [offset, written, status] = feedback[0];
        if (status as i64) <= 0 {
            return Err(format!("encode failed with query status {}", status as i64).into());
        }

        if !self.bitstream_coherent {
            let range = vk::MappedMemoryRange::default()
                .memory(self.bitstream_memory)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            unsafe { d.invalidate_mapped_memory_ranges(&[range])? };
        }
        let data = unsafe {
            std::slice::from_raw_parts(self.bitstream_ptr.add(offset as usize), written as usize)
        };

        let mut out = Vec::with_capacity(self.headers.len() + data.len());
        if idr {
            out.extend_from_slice(&self.headers);
        }
        out.extend_from_slice(data);
        Ok(out)
    }

    fn create_resources(&mut self) -> Result<(), Box<dyn Error>> {
        let d = &self.vk.device;

        for _ in 0..DPB_SLOTS {
            let slot = self.create_dpb_slot()?;
            self.dpb.push(slot);
        }

        let buffer = with_profile_list(|list| -> Result<_, Box<dyn Error>> {
            let mut list = *list;
            let info = vk::BufferCreateInfo::default()
                .size(self.bitstream_range)
                .usage(vk::BufferUsageFlags::VIDEO_ENCODE_DST_KHR)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .push(&mut list);
            Ok(unsafe { d.create_buffer(&info, None)? })
        })?;
        self.bitstream = buffer;
        let reqs = unsafe { d.get_buffer_memory_requirements(buffer) };
        let (type_index, coherent) = find_readback_memory(self.vk, reqs.memory_type_bits)?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(reqs.size)
            .memory_type_index(type_index);
        self.bitstream_memory = unsafe { d.allocate_memory(&alloc, None)? };
        self.bitstream_coherent = coherent;
        unsafe { d.bind_buffer_memory(buffer, self.bitstream_memory, 0)? };
        self.bitstream_ptr = unsafe {
            d.map_memory(
                self.bitstream_memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?
        } as *const u8;

        let mut feedback_info = vk::QueryPoolVideoEncodeFeedbackCreateInfoKHR::default()
            .encode_feedback_flags(
                vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BUFFER_OFFSET
                    | vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BYTES_WRITTEN,
            );
        self.query_pool = with_encode_profile(|profile| -> Result<_, Box<dyn Error>> {
            let mut profile = *profile;
            let info = vk::QueryPoolCreateInfo::default()
                .query_type(vk::QueryType::VIDEO_ENCODE_FEEDBACK_KHR)
                .query_count(1)
                .push(&mut feedback_info);
            let info = unsafe { info.extend(&mut profile) };
            Ok(unsafe { d.create_query_pool(&info, None)? })
        })?;

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(self.vk.encode_queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        self.pool = unsafe { d.create_command_pool(&pool_info, None)? };
        let cb_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        self.cb = unsafe { d.allocate_command_buffers(&cb_info)? }[0];
        let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        self.fence = unsafe { d.create_fence(&fence_info, None)? };
        Ok(())
    }

    fn create_dpb_slot(&self) -> Result<DpbSlot, Box<dyn Error>> {
        let d = &self.vk.device;
        let image = with_profile_list(|list| -> Result<_, Box<dyn Error>> {
            let mut list = *list;
            let info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(DPB_FORMAT)
                .extent(vk::Extent3D {
                    width: self.extent.width,
                    height: self.extent.height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .push(&mut list);
            Ok(unsafe { d.create_image(&info, None)? })
        })?;

        let reqs = unsafe { d.get_image_memory_requirements(image) };
        let type_index = find_device_local(self.vk, reqs.memory_type_bits)?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(reqs.size)
            .memory_type_index(type_index);
        let memory = unsafe { d.allocate_memory(&alloc, None)? };
        unsafe { d.bind_image_memory(image, memory, 0)? };

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(DPB_FORMAT)
            .subresource_range(color_range());
        let view = unsafe { d.create_image_view(&view_info, None)? };
        Ok(DpbSlot {
            image,
            view,
            memory,
            reference: None,
        })
    }
}

impl Drop for Encoder<'_> {
    fn drop(&mut self) {
        let d = &self.vk.device;
        unsafe {
            let _ = d.device_wait_idle();
            d.destroy_fence(self.fence, None);
            d.destroy_command_pool(self.pool, None);
            d.destroy_query_pool(self.query_pool, None);
            d.destroy_buffer(self.bitstream, None);
            d.free_memory(self.bitstream_memory, None);
            if let Some(staging) = &self.staging {
                d.destroy_command_pool(staging.pool, None);
                d.destroy_image(staging.image, None);
                d.free_memory(staging.memory, None);
            }
            for slot in &self.dpb {
                d.destroy_image_view(slot.view, None);
                d.destroy_image(slot.image, None);
                d.free_memory(slot.memory, None);
            }
            self.video_queue
                .destroy_video_session_parameters(self.parameters, None);
            self.video_queue.destroy_video_session(self.session, None);
            for &memory in &self.session_memory {
                d.free_memory(memory, None);
            }
        }
    }
}

pub fn query_max_bitrate(vk: &VulkanDevice) -> Result<u64, Box<dyn Error>> {
    let video_queue = khr::video_queue::Instance::load(&vk.entry, &vk.instance);
    with_encode_profile(|profile| {
        let mut rgb = vk::VideoEncodeRgbConversionCapabilitiesVALVE::default();
        let mut h264 = vk::VideoEncodeH264CapabilitiesKHR::default();
        let mut encode = vk::VideoEncodeCapabilitiesKHR::default();
        let mut caps = vk::VideoCapabilitiesKHR::default()
            .push(&mut encode)
            .push(&mut h264)
            .push(&mut rgb);
        unsafe { video_queue.get_physical_device_video_capabilities(vk.pdev, profile, &mut caps)? };
        Ok(encode.max_bitrate)
    })
}

struct Capabilities {
    max_level_idc: native::StdVideoH264LevelIdc,
    max_active_references: u32,
    size_alignment: u64,
    std_header_version: vk::ExtensionProperties,
}

fn query_capabilities(
    vk: &VulkanDevice,
    extent: vk::Extent2D,
    peak_bitrate: u64,
) -> Result<Capabilities, Box<dyn Error>> {
    let video_queue = khr::video_queue::Instance::load(&vk.entry, &vk.instance);
    with_encode_profile(|profile| {
        let mut rgb = vk::VideoEncodeRgbConversionCapabilitiesVALVE::default();
        let mut h264 = vk::VideoEncodeH264CapabilitiesKHR::default();
        let mut encode = vk::VideoEncodeCapabilitiesKHR::default();
        let mut caps = vk::VideoCapabilitiesKHR::default()
            .push(&mut encode)
            .push(&mut h264)
            .push(&mut rgb);
        unsafe { video_queue.get_physical_device_video_capabilities(vk.pdev, profile, &mut caps)? };

        let (min, max) = (caps.min_coded_extent, caps.max_coded_extent);
        let max_active_references = caps.max_active_reference_pictures.min(1);
        let size_alignment = caps.min_bitstream_buffer_size_alignment;
        let std_header_version = caps.std_header_version;
        if extent.width < min.width
            || extent.height < min.height
            || extent.width > max.width
            || extent.height > max.height
        {
            return Err(format!(
                "{}x{} outside encoder range {}x{} to {}x{}",
                extent.width, extent.height, min.width, min.height, max.width, max.height
            )
            .into());
        }
        if !rgb
            .rgb_models
            .contains(vk::VideoEncodeRgbModelConversionFlagsVALVE::YCBCR_709)
            || !rgb
                .rgb_ranges
                .contains(vk::VideoEncodeRgbRangeCompressionFlagsVALVE::NARROW_RANGE)
        {
            return Err("encoder lacks BT.709 narrow range RGB conversion".into());
        }
        if !encode
            .rate_control_modes
            .contains(vk::VideoEncodeRateControlModeFlagsKHR::VBR)
            || encode.max_bitrate < peak_bitrate
        {
            return Err(format!(
                "encoder lacks VBR rate control up to {peak_bitrate} bps (supports {})",
                encode.max_bitrate
            )
            .into());
        }

        Ok(Capabilities {
            max_level_idc: h264.max_level_idc,
            max_active_references,
            size_alignment,
            std_header_version,
        })
    })
}

fn pick_level(
    width: u32,
    height: u32,
    fps: u32,
    device_max: native::StdVideoH264LevelIdc,
) -> native::StdVideoH264LevelIdc {
    let macroblocks = (width as u64).div_ceil(16) * (height as u64).div_ceil(16);
    let per_second = macroblocks * fps as u64;
    LEVELS
        .iter()
        .find(|&&(_, max_rate, max_frame)| per_second <= max_rate && macroblocks <= max_frame)
        .map(|&(level, _, _)| level)
        .unwrap_or(LEVELS[LEVELS.len() - 1].0)
        .min(device_max)
}

fn create_session(
    vk: &VulkanDevice,
    video_queue: &khr::video_queue::Device,
    extent: vk::Extent2D,
    picture_format: vk::Format,
    max_active_references: u32,
    std_header_version: &vk::ExtensionProperties,
) -> Result<(vk::VideoSessionKHR, Vec<vk::DeviceMemory>), Box<dyn Error>> {
    let d = &vk.device;
    let mut rgb = vk::VideoEncodeSessionRgbConversionCreateInfoVALVE::default()
        .rgb_model(vk::VideoEncodeRgbModelConversionFlagsVALVE::YCBCR_709)
        .rgb_range(vk::VideoEncodeRgbRangeCompressionFlagsVALVE::NARROW_RANGE)
        .x_chroma_offset(vk::VideoEncodeRgbChromaOffsetFlagsVALVE::COSITED_EVEN)
        .y_chroma_offset(vk::VideoEncodeRgbChromaOffsetFlagsVALVE::MIDPOINT);

    let session = with_encode_profile(|profile| -> Result<_, Box<dyn Error>> {
        let info = vk::VideoSessionCreateInfoKHR::default()
            .queue_family_index(vk.encode_queue_family_index)
            .video_profile(profile)
            .picture_format(picture_format)
            .max_coded_extent(extent)
            .reference_picture_format(DPB_FORMAT)
            .max_dpb_slots(DPB_SLOTS as u32)
            .max_active_reference_pictures(max_active_references)
            .std_header_version(std_header_version)
            .push(&mut rgb);
        Ok(unsafe { video_queue.create_video_session(&info, None)? })
    })?;

    let count = unsafe { video_queue.get_video_session_memory_requirements_len(session)? };
    let mut reqs = vec![vk::VideoSessionMemoryRequirementsKHR::default(); count];
    unsafe { video_queue.get_video_session_memory_requirements(session, &mut reqs)? };

    let mut memories = Vec::with_capacity(count);
    let mut binds = Vec::with_capacity(count);
    for r in &reqs {
        let type_index = find_device_local(vk, r.memory_requirements.memory_type_bits)?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(r.memory_requirements.size)
            .memory_type_index(type_index);
        let memory = unsafe { d.allocate_memory(&alloc, None)? };
        memories.push(memory);
        binds.push(
            vk::BindVideoSessionMemoryInfoKHR::default()
                .memory_bind_index(r.memory_bind_index)
                .memory(memory)
                .memory_offset(0)
                .memory_size(r.memory_requirements.size),
        );
    }
    unsafe { video_queue.bind_video_session_memory(session, &binds)? };
    Ok((session, memories))
}

fn build_sps(
    extent: vk::Extent2D,
    level: native::StdVideoH264LevelIdc,
) -> native::StdVideoH264SequenceParameterSet {
    let width_mbs = extent.width.div_ceil(16);
    let height_mbs = extent.height.div_ceil(16);

    let mut sps: native::StdVideoH264SequenceParameterSet = unsafe { std::mem::zeroed() };
    sps.flags.set_frame_mbs_only_flag(1);
    sps.flags.set_direct_8x8_inference_flag(1);
    sps.profile_idc = native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN;
    sps.level_idc = level;
    sps.chroma_format_idc =
        native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_420;
    sps.log2_max_frame_num_minus4 = 4;
    sps.pic_order_cnt_type = native::StdVideoH264PocType_STD_VIDEO_H264_POC_TYPE_0;
    sps.log2_max_pic_order_cnt_lsb_minus4 = 4;
    sps.max_num_ref_frames = 1;
    sps.pic_width_in_mbs_minus1 = width_mbs - 1;
    sps.pic_height_in_map_units_minus1 = height_mbs - 1;

    let crop_right = (width_mbs * 16 - extent.width) / 2;
    let crop_bottom = (height_mbs * 16 - extent.height) / 2;
    if crop_right > 0 || crop_bottom > 0 {
        sps.flags.set_frame_cropping_flag(1);
        sps.frame_crop_right_offset = crop_right;
        sps.frame_crop_bottom_offset = crop_bottom;
    }
    sps
}

fn build_vui(fps: u32) -> native::StdVideoH264SequenceParameterSetVui {
    let mut vui: native::StdVideoH264SequenceParameterSetVui = unsafe { std::mem::zeroed() };
    vui.flags.set_video_signal_type_present_flag(1);
    vui.flags.set_color_description_present_flag(1);
    vui.flags.set_timing_info_present_flag(1);
    vui.flags.set_fixed_frame_rate_flag(1);
    vui.video_format = 5;
    vui.colour_primaries = 1;
    vui.transfer_characteristics = 1;
    vui.matrix_coefficients = 1;
    vui.num_units_in_tick = 1;
    vui.time_scale = 2 * fps;
    vui.aspect_ratio_idc =
        native::StdVideoH264AspectRatioIdc_STD_VIDEO_H264_ASPECT_RATIO_IDC_UNSPECIFIED;
    vui
}

fn build_pps() -> native::StdVideoH264PictureParameterSet {
    let mut pps: native::StdVideoH264PictureParameterSet = unsafe { std::mem::zeroed() };
    pps.flags.set_deblocking_filter_control_present_flag(1);
    pps.weighted_bipred_idc =
        native::StdVideoH264WeightedBipredIdc_STD_VIDEO_H264_WEIGHTED_BIPRED_IDC_DEFAULT;
    pps
}

fn create_parameters(
    video_queue: &khr::video_queue::Device,
    session: vk::VideoSessionKHR,
    sps: &native::StdVideoH264SequenceParameterSet,
    pps: &native::StdVideoH264PictureParameterSet,
) -> Result<vk::VideoSessionParametersKHR, Box<dyn Error>> {
    let sps = [*sps];
    let pps = [*pps];
    let add_info = vk::VideoEncodeH264SessionParametersAddInfoKHR::default()
        .std_sp_ss(&sps)
        .std_pp_ss(&pps);
    let mut h264 = vk::VideoEncodeH264SessionParametersCreateInfoKHR::default()
        .max_std_sps_count(1)
        .max_std_pps_count(1)
        .parameters_add_info(&add_info);
    let info = vk::VideoSessionParametersCreateInfoKHR::default()
        .video_session(session)
        .push(&mut h264);
    Ok(unsafe { video_queue.create_video_session_parameters(&info, None)? })
}

fn fetch_headers(
    video_encode_queue: &khr::video_encode_queue::Device,
    parameters: vk::VideoSessionParametersKHR,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut h264 = vk::VideoEncodeH264SessionParametersGetInfoKHR::default()
        .write_std_sps(true)
        .write_std_pps(true);
    let info = vk::VideoEncodeSessionParametersGetInfoKHR::default()
        .video_session_parameters(parameters)
        .push(&mut h264);
    let len = unsafe { video_encode_queue.get_encoded_video_session_parameters_len(&info, None)? };
    let mut buffer = vec![std::mem::MaybeUninit::<u8>::uninit(); len];
    unsafe { video_encode_queue.get_encoded_video_session_parameters(&info, None, &mut buffer)? };
    Ok(buffer
        .into_iter()
        .map(|b| unsafe { b.assume_init() })
        .collect())
}

fn create_image(
    vk: &VulkanDevice,
    extent: vk::Extent2D,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    for_encode: bool,
) -> Result<(vk::Image, vk::DeviceMemory), Box<dyn Error>> {
    let d = &vk.device;
    let image = with_profile_list(|list| -> Result<_, Box<dyn Error>> {
        let mut list = *list;
        let mut info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        if for_encode {
            info = info.push(&mut list);
        }
        Ok(unsafe { d.create_image(&info, None)? })
    })?;
    let reqs = unsafe { d.get_image_memory_requirements(image) };
    let alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(reqs.size)
        .memory_type_index(find_device_local(vk, reqs.memory_type_bits)?);
    let memory = unsafe { d.allocate_memory(&alloc, None)? };
    unsafe { d.bind_image_memory(image, memory, 0)? };
    Ok((image, memory))
}

fn find_device_local(vk: &VulkanDevice, type_bits: u32) -> Result<u32, Box<dyn Error>> {
    let props = unsafe { vk.instance.get_physical_device_memory_properties(vk.pdev) };
    (0..props.memory_type_count)
        .find(|&i| {
            type_bits & (1 << i) != 0
                && props.memory_types[i as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .ok_or("no device-local memory type".into())
}

fn rate_control_layers(
    fps: u32,
    average_bitrate: u64,
    peak_bitrate: u64,
) -> [vk::VideoEncodeRateControlLayerInfoKHR<'static>; 1] {
    [vk::VideoEncodeRateControlLayerInfoKHR::default()
        .average_bitrate(average_bitrate)
        .max_bitrate(peak_bitrate)
        .frame_rate_numerator(fps)
        .frame_rate_denominator(1)]
}

fn rate_control<'a>(
    layers: &'a [vk::VideoEncodeRateControlLayerInfoKHR<'a>],
) -> vk::VideoEncodeRateControlInfoKHR<'a> {
    vk::VideoEncodeRateControlInfoKHR::default()
        .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::VBR)
        .layers(layers)
        .virtual_buffer_size_in_ms(RATE_WINDOW_MS)
        .initial_virtual_buffer_size_in_ms(RATE_WINDOW_MS / 2)
}

fn h264_rate_control() -> vk::VideoEncodeH264RateControlInfoKHR<'static> {
    vk::VideoEncodeH264RateControlInfoKHR::default()
        .flags(
            vk::VideoEncodeH264RateControlFlagsKHR::REGULAR_GOP
                | vk::VideoEncodeH264RateControlFlagsKHR::REFERENCE_PATTERN_FLAT,
        )
        .gop_frame_count(GOP_LENGTH)
        .idr_period(GOP_LENGTH)
        .temporal_layer_count(1)
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

fn align_up(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment.max(1)) * alignment.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const OUT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target");

    fn fill_and_release(
        vk: &VulkanDevice,
        cb: vk::CommandBuffer,
        image: vk::Image,
        rgba: [f32; 4],
        old: vk::ImageLayout,
    ) {
        let d = &vk.device;
        let qfi = vk.queue_family_index;
        let to_dst = vk::ImageMemoryBarrier::default()
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(old)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(if old == vk::ImageLayout::UNDEFINED {
                vk::QUEUE_FAMILY_IGNORED
            } else {
                vk::QUEUE_FAMILY_FOREIGN_EXT
            })
            .dst_queue_family_index(if old == vk::ImageLayout::UNDEFINED {
                vk::QUEUE_FAMILY_IGNORED
            } else {
                qfi
            })
            .image(image)
            .subresource_range(color_range());
        let to_foreign = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(qfi)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .image(image)
            .subresource_range(color_range());
        unsafe {
            d.begin_command_buffer(
                cb,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .unwrap();
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_dst],
            );
            d.cmd_clear_color_image(
                cb,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue { float32: rgba },
                &[color_range()],
            );
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_foreign],
            );
            d.end_command_buffer(cb).unwrap();
            let cbs = [cb];
            d.queue_submit(
                vk.queue,
                &[vk::SubmitInfo::default().command_buffers(&cbs)],
                vk::Fence::null(),
            )
            .unwrap();
            d.queue_wait_idle(vk.queue).unwrap();
        }
    }

    #[test]
    fn encodes_synthetic_frames() {
        let vk = VulkanDevice::new().unwrap();
        let extent = vk::Extent2D {
            width: 1920,
            height: 1080,
        };
        let frames = 130u32;
        let out_path = std::env::var("CLIPRS_TEST_OUT").unwrap_or(format!("{OUT_DIR}/test.h264"));
        let mut out = std::fs::File::create(&out_path).unwrap();

        let format = vk::Format::R8G8B8A8_UNORM;
        let mut encoder = Encoder::new(
            &vk,
            extent.width,
            extent.height,
            &Settings::default(),
            format,
        )
        .unwrap();
        let (image, memory) = create_image(
            &vk,
            extent,
            format,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR,
            true,
        )
        .unwrap();

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(vk.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let pool = unsafe { vk.device.create_command_pool(&pool_info, None).unwrap() };
        let cb_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .command_buffer_count(1);
        let cb = unsafe { vk.device.allocate_command_buffers(&cb_info).unwrap() }[0];

        let mut samples = Vec::new();
        let mut old = vk::ImageLayout::UNDEFINED;
        let start = Instant::now();
        for i in 0..frames {
            let t = i as f32 / frames as f32;
            fill_and_release(&vk, cb, image, [t, 0.2, 1.0 - t, 1.0], old);
            old = vk::ImageLayout::GENERAL;
            let mut sample = encoder.encode_image(image).unwrap();
            // frames encode faster than real time, the muxer needs them one interval apart
            sample.captured_at = start + std::time::Duration::from_secs_f64(f64::from(i) / 60.0);
            assert!(!sample.data.is_empty());
            out.write_all(&sample.data).unwrap();
            samples.push(sample);
        }
        crate::muxer::write_mkv(
            std::path::Path::new(&format!("{OUT_DIR}/test.mkv")),
            &samples,
            &[],
            "synthetic",
            &cliprs_ipc::ClipMeta {
                title: Some("synthetic frames".to_string()),
                saved_at_unix_secs: 1_700_000_000,
                duration_secs: samples.len() as f64 / 60.0,
                fps: 60,
                width: extent.width,
                height: extent.height,
            },
        )
        .unwrap();

        unsafe {
            vk.device.destroy_command_pool(pool, None);
            vk.device.destroy_image(image, None);
            vk.device.free_memory(memory, None);
        }
    }

    fn encode_solid_red(format: vk::Format) {
        let vk = VulkanDevice::new().unwrap();
        let extent = vk::Extent2D {
            width: 1920,
            height: 1080,
        };
        let direct = ENCODABLE_FORMATS.contains(&format);
        let mut encoder = Encoder::new(
            &vk,
            extent.width,
            extent.height,
            &Settings::default(),
            format,
        )
        .unwrap();
        let (usage, for_encode) = if direct {
            (vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR, true)
        } else {
            (vk::ImageUsageFlags::TRANSFER_SRC, false)
        };
        let (image, memory) = create_image(
            &vk,
            extent,
            format,
            vk::ImageUsageFlags::TRANSFER_DST | usage,
            for_encode,
        )
        .unwrap();

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(vk.queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let pool = unsafe { vk.device.create_command_pool(&pool_info, None).unwrap() };
        let cb_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .command_buffer_count(1);
        let cb = unsafe { vk.device.allocate_command_buffers(&cb_info).unwrap() }[0];

        let mut out = std::fs::File::create(format!("{OUT_DIR}/test_{format:?}.h264")).unwrap();
        let mut old = vk::ImageLayout::UNDEFINED;
        for _ in 0..3 {
            fill_and_release(&vk, cb, image, [1.0, 0.0, 0.0, 1.0], old);
            old = vk::ImageLayout::GENERAL;
            let sample = if direct {
                encoder.encode_image(image).unwrap()
            } else {
                encoder.encode_converted(image).unwrap()
            };
            assert!(!sample.data.is_empty());
            out.write_all(&sample.data).unwrap();
        }

        unsafe {
            vk.device.destroy_command_pool(pool, None);
            vk.device.destroy_image(image, None);
            vk.device.free_memory(memory, None);
        }
    }

    #[test]
    fn encodes_every_plane_format() {
        for format in [
            vk::Format::R8G8B8A8_UNORM,
            vk::Format::B8G8R8A8_UNORM,
            vk::Format::A2B10G10R10_UNORM_PACK32,
            vk::Format::A2R10G10B10_UNORM_PACK32,
        ] {
            encode_solid_red(format);
        }
    }
}
