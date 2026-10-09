use std::error::Error;
use std::path::Path;

use image::imageops::{self, FilterType};
use image::{DynamicImage, RgbaImage};

use crate::capture::Capture;
use crate::kms::Frame;

const THUMBNAIL_WIDTH: u32 = 320;

pub fn save_thumbnail(
    capture: &mut Capture<'_>,
    frame: &Frame,
    path: impl AsRef<Path>,
) -> Result<(), Box<dyn Error>> {
    let (width, height) = (frame.width, frame.height);
    capture.capture(frame, |rgba| write_jpeg(rgba, width, height, path.as_ref()))?
}

fn write_jpeg(rgba: &[u8], width: u32, height: u32, path: &Path) -> Result<(), Box<dyn Error>> {
    let source = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or("frame buffer does not match its dimensions")?;
    let thumb_height = (height * THUMBNAIL_WIDTH / width).max(1);
    let resized = imageops::resize(&source, THUMBNAIL_WIDTH, thumb_height, FilterType::Triangle);
    DynamicImage::ImageRgba8(resized).to_rgb8().save(path)?;
    Ok(())
}
