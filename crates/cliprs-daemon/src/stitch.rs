use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cliprs_ipc::{ClipMeta, Settings, clips_dir, create_user_dir, give_to_invoking_user};

use crate::audio;
use crate::encode::{Encoder, Sample};
use crate::kms::Frame;
use crate::muxer::write_mkv;
use crate::vulkan::{VulkanDevice, vk_format};

pub struct Recording<'a> {
    vk: &'a VulkanDevice,
    settings: Settings,
    size: Option<(u32, u32)>,
    encoder: Option<Encoder<'a>>,
    samples: VecDeque<Sample>,
    max_frames: usize,
    audio: audio::Ring,
}

impl<'a> Recording<'a> {
    pub fn new(vk: &'a VulkanDevice, settings: Settings) -> Result<Self, Box<dyn Error>> {
        let max_frames = (settings.clip_seconds * settings.fps) as usize;
        Ok(Recording {
            vk,
            settings,
            size: None,
            encoder: None,
            samples: VecDeque::new(),
            max_frames,
            audio: audio::start(),
        })
    }

    pub fn add_frame(&mut self, frame: &Frame) -> Result<(), Box<dyn Error>> {
        let size = (frame.width, frame.height);
        if self.size != Some(size) {
            if self.size.is_some() {
                log::warn!(
                    "resolution changed to {}x{}, replay buffer restarted",
                    frame.width,
                    frame.height
                );
                self.samples.clear();
            }
            let format = vk_format(frame.fourcc)
                .ok_or_else(|| format!("unsupported plane format {}", frame.fourcc))?;
            self.encoder = None;
            self.size = None;
            self.encoder = Some(Encoder::new(
                self.vk,
                frame.width,
                frame.height,
                &self.settings,
                format,
            )?);
            self.size = Some(size);
        }

        let encoder = self
            .encoder
            .as_mut()
            .expect("encoder is created with the first frame");

        self.samples.push_back(encoder.encode_frame(frame)?);
        drop_old_gops(&mut self.samples, self.max_frames);
        if let Some(oldest) = self.samples.front() {
            self.audio.drop_before(oldest.captured_at);
        }
        Ok(())
    }

    pub fn save(&mut self, id: &str) -> Result<(), Box<dyn Error>> {
        let (width, height) = self.size.ok_or("no frames recorded")?;

        let clips_dir = clips_dir()?;
        create_user_dir(&clips_dir)?;
        let samples = VecDeque::make_contiguous(&mut self.samples);
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            return Err("no frames recorded".into());
        };
        let frame_interval = Duration::from_secs_f64(1.0 / f64::from(self.settings.fps));
        let duration = last.captured_at.duration_since(first.captured_at) + frame_interval;
        let audio_packets = self.audio.packets();
        let audio_blocks = audio::blocks(&audio_packets, first.captured_at);
        let clip_path = clips_dir.join(format!("{id}.mkv"));
        let part_path = clips_dir.join(format!("{id}.mkv.part"));
        let meta = ClipMeta {
            title: None,
            saved_at_unix_secs: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            duration_secs: duration.as_secs_f64(),
            fps: self.settings.fps,
            width,
            height,
        };
        write_mkv(&part_path, samples, &audio_blocks, id, &meta)?;
        give_to_invoking_user(&part_path)?;

        meta.save(id)?;

        fs::rename(part_path, clip_path)?;
        Ok(())
    }
}

fn drop_old_gops(samples: &mut VecDeque<Sample>, max_frames: usize) {
    while let Some(next_idr) = samples.iter().skip(1).position(|s| s.is_idr).map(|p| p + 1)
        && samples.len() - next_idr >= max_frames
    {
        samples.drain(..next_idr);
    }
}
