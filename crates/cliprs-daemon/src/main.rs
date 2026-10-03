mod capture;
mod encode;
mod hotkeys;
mod kms;
mod muxer;
mod readback;
mod stitch;
mod vulkan;

use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use cliprs_ipc::{Capabilities, Settings};
use evdev::KeyCode;

const SINGLE_FRAME_DEBUG: bool = false;

fn recording_loop(
    card: &kms::Card,
    vk: &vulkan::VulkanDevice,
    settings: Settings,
) -> Result<(), Box<dyn Error>> {
    let interval = Duration::from_secs_f64(1.0 / settings.fps as f64);
    let mut recording = stitch::Recording::new(vk, settings)?;
    let start = Instant::now();
    let record_hotkey = hotkeys::hotkey_presses(KeyCode::KEY_F8);

    let mut i = 0;
    loop {
        let frame = capture::grab_frame(card).map_err(|e| format!("grab failed: {e}"))?;
        recording.add_frame(&frame)?;
        thread::sleep((start + interval * (i + 1)).saturating_duration_since(Instant::now()));
        if record_hotkey.try_recv().is_ok() {
            println!("received keypress f8, saving clip");
            return recording.finish("clips", &uuid::Uuid::new_v4().to_string());
        }
        i += 1;
    }
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

    Capabilities {
        max_bitrate_bps: encode::query_max_bitrate(&vk)?,
        gop_frames: encode::GOP_LENGTH,
    }
    .save()?;
    let settings = Settings::load()?;

    recording_loop(&card, &vk, settings)
}
