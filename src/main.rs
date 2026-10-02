mod capture;
mod encode;
mod kms;
mod muxer;
mod readback;
mod stitch;
mod vulkan;

use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

const SINGLE_FRAME_DEBUG: bool = false;
const TARGET_FPS: u32 = 60;

fn recording_loop(
    duration: u32,
    card: &kms::Card,
    vk: &vulkan::VulkanDevice,
) -> Result<(), Box<dyn Error>> {
    let total_frames = duration * TARGET_FPS;
    let mut recording = stitch::Recording::new(vk, TARGET_FPS, "clip.mp4")?;
    let interval = Duration::from_secs_f64(1.0 / TARGET_FPS as f64);
    let start = Instant::now();

    for i in 0..total_frames {
        let frame = capture::grab_frame(card).map_err(|e| format!("grab failed: {e}"))?;
        recording.add_frame(&frame)?;
        thread::sleep((start + interval * (i + 1)).saturating_duration_since(Instant::now()));
    }
    recording.finish()
}

fn main() -> Result<(), Box<dyn Error>> {
    let card = kms::Card::open("/dev/dri/card2")?;
    let vk = vulkan::VulkanDevice::new()?;

    if SINGLE_FRAME_DEBUG {
        let frame = capture::grab_frame(&card)?;
        let (width, height) = (frame.width, frame.height);
        let mut capture = capture::Capture::new(&vk, &frame)?;

        capture.capture(&frame, |rgba| {
            capture::save_png("frame.png", rgba, width, height)
        })??;
        return Ok(());
    }

    recording_loop(3, &card, &vk)
}
