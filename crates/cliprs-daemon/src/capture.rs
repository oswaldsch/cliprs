use ash::vk;
use std::error::Error;
use std::path::Path;

use crate::kms::{Card, Frame};
use crate::readback::FrameReader;
use crate::vulkan::{VulkanDevice, vk_format};

const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

pub struct Capture<'a> {
    vk: &'a VulkanDevice,
    reader: FrameReader<'a>,
}

impl<'a> Capture<'a> {
    pub fn new(vk: &'a VulkanDevice, first: &Frame) -> Result<Self, Box<dyn Error>> {
        if vk_format(first.fourcc) != Some(FORMAT) {
            return Err(format!("PNG capture needs AB24 or XB24, got {}", first.fourcc).into());
        }
        let mods = vk.supported_modifiers(FORMAT);
        if !mods.iter().any(|m| m.drm_format_modifier == first.modifier) {
            return Err("modifier not supported by Vulkan".into());
        }
        let reader = FrameReader::new(vk, first.width, first.height, 1)?;
        Ok(Self { vk, reader })
    }

    pub fn capture<R>(
        &mut self,
        frame: &Frame,
        f: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, Box<dyn Error>> {
        let (image, memory) =
            self.vk
                .import_frame(frame, vk::ImageUsageFlags::TRANSFER_SRC, None)?;
        let result = self.reader.capture_blocking(image, f);
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
