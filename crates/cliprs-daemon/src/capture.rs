use ash::vk;
use std::borrow::Cow;
use std::error::Error;
use std::path::Path;

use crate::kms::{Card, Frame};
use crate::readback::FrameReader;
use crate::vulkan::{VulkanDevice, vk_format};

#[derive(Clone, Copy)]
enum Layout {
    Rgba8,
    Bgra8,
    A2b10g10r10,
    A2r10g10b10,
}

impl Layout {
    fn from_format(format: vk::Format) -> Option<Self> {
        match format {
            vk::Format::R8G8B8A8_UNORM => Some(Self::Rgba8),
            vk::Format::B8G8R8A8_UNORM => Some(Self::Bgra8),
            vk::Format::A2B10G10R10_UNORM_PACK32 => Some(Self::A2b10g10r10),
            vk::Format::A2R10G10B10_UNORM_PACK32 => Some(Self::A2r10g10b10),
            _ => None,
        }
    }

    fn to_rgba8<'b>(self, bytes: &'b [u8]) -> Cow<'b, [u8]> {
        match self {
            Self::Rgba8 => Cow::Borrowed(bytes),
            Self::Bgra8 => {
                let mut rgba = bytes.to_vec();
                rgba.chunks_exact_mut(4).for_each(|pixel| pixel.swap(0, 2));
                Cow::Owned(rgba)
            }
            Self::A2b10g10r10 => Cow::Owned(unpack_10_bit(bytes, 0, 20)),
            Self::A2r10g10b10 => Cow::Owned(unpack_10_bit(bytes, 20, 0)),
        }
    }
}

fn unpack_10_bit(bytes: &[u8], red_shift: u32, blue_shift: u32) -> Vec<u8> {
    let to_8_bit = |pixel: u32, shift: u32| (((pixel >> shift) & 0x3ff) >> 2) as u8;
    let mut rgba = Vec::with_capacity(bytes.len());
    for chunk in bytes.chunks_exact(4) {
        let pixel = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        rgba.extend_from_slice(&[
            to_8_bit(pixel, red_shift),
            to_8_bit(pixel, 10),
            to_8_bit(pixel, blue_shift),
            u8::MAX,
        ]);
    }
    rgba
}

pub struct Capture<'a> {
    vk: &'a VulkanDevice,
    reader: FrameReader<'a>,
    layout: Layout,
}

impl<'a> Capture<'a> {
    pub fn new(vk: &'a VulkanDevice, first: &Frame) -> Result<Self, Box<dyn Error>> {
        let format = vk_format(first.fourcc)
            .ok_or_else(|| format!("unsupported plane format {}", first.fourcc))?;
        let layout = Layout::from_format(format)
            .ok_or_else(|| format!("capture does not support {}", first.fourcc))?;
        let mods = vk.supported_modifiers(format);
        if !mods.iter().any(|m| m.drm_format_modifier == first.modifier) {
            return Err("modifier not supported by Vulkan".into());
        }
        let reader = FrameReader::new(vk, first.width, first.height, 1)?;
        Ok(Self { vk, reader, layout })
    }

    pub fn capture<R>(
        &mut self,
        frame: &Frame,
        f: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, Box<dyn Error>> {
        let (image, memory) =
            self.vk
                .import_frame(frame, vk::ImageUsageFlags::TRANSFER_SRC, None)?;
        let layout = self.layout;
        let result = self
            .reader
            .capture_blocking(image, |bytes| f(&layout.to_rgba8(bytes)));
        unsafe {
            self.vk.device.destroy_image(image, None);
            self.vk.device.free_memory(memory, None);
        }
        result
    }
}

pub fn grab_frame(card: &Card) -> Result<Frame, Box<dyn Error>> {
    let mut seen = Vec::new();
    for plane in card.active_primary_planes()? {
        let frame = card.grab(plane)?;
        if vk_format(frame.fourcc).is_some() {
            return Ok(frame);
        }
        seen.push(frame.fourcc.to_string());
    }
    Err(format!(
        "no primary plane with a supported format, found [{}]",
        seen.join(", ")
    )
    .into())
}

pub fn save_png(
    path: impl AsRef<Path>,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), Box<dyn Error>> {
    image::save_buffer(path, rgba, width, height, image::ColorType::Rgba8)?;
    Ok(())
}
