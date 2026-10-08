mod capture;
mod encode;
mod hotkeys;
mod kms;
mod muxer;
mod readback;
mod stitch;
mod thumbnail;
mod vulkan;

use std::env;
use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use cliprs_ipc::{
    Capabilities, InstanceLock, Notification, Process, Settings, create_user_dir,
    give_to_invoking_user, install, notify, notify_error,
};
use evdev::KeyCode;

const SINGLE_FRAME_DEBUG: bool = false;

fn save_clip(
    recording: &mut stitch::Recording,
    vk: &vulkan::VulkanDevice,
    frame: &kms::Frame,
) -> Result<String, Box<dyn Error>> {
    let id = uuid::Uuid::new_v4().to_string();
    log::info!("attempting to save clip {id}");
    recording.save(&id)?;
    let thumbnail_path = cliprs_ipc::thumbnail_path(&id)?;
    if let Some(thumbnails_dir) = thumbnail_path.parent() {
        create_user_dir(thumbnails_dir)?;
    }
    let mut capture = capture::Capture::new(vk, frame)?;
    thumbnail::save_thumbnail(&mut capture, frame, &thumbnail_path)?;
    give_to_invoking_user(&thumbnail_path)?;
    Ok(id)
}

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
            match save_clip(&mut recording, vk, &frame) {
                Ok(id) => {
                    if let Err(error) = notify(&Notification::ClipSaved { id }) {
                        log::warn!("clip saved notification failed: {error}");
                    }
                }
                Err(error) => {
                    log::error!("clip save failed: {error}");
                    notify_error(format!("Could not save clip: {error}"));
                }
            }
        }
        i += 1;
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let card = kms::Card::open("/dev/dri/card2")?; // TODO: add DRM enumeration
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

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if env::args().nth(1).as_deref() == Some(install::INSTALL_ARGUMENT) {
        if let Err(error) = install::install_daemon_as_root() {
            log::error!("install failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    let _instance_lock = match InstanceLock::acquire(Process::Daemon) {
        Ok(lock) => lock,
        Err(error) => {
            log::error!("could not take daemon lock: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = run() {
        log::error!("daemon stopped: {error}");
        notify_error(format!("cliprs stopped: {error}"));
        std::process::exit(1);
    }
}
