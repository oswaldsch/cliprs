use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use cliprs_ipc::{ClipMeta, Settings, give_to_invoking_user};

use crate::encode::{Encoder, Sample};
use crate::kms::Frame;
use crate::muxer::write_mp4;
use crate::vulkan::{VulkanDevice, vk_format};

pub struct Recording<'a> {
    vk: &'a VulkanDevice,
    settings: Settings,
    width: Option<u32>,
    height: Option<u32>,
    encoder: Option<Encoder<'a>>,
    samples: VecDeque<Sample>,
    max_frames: usize,
}

impl<'a> Recording<'a> {
    pub fn new(vk: &'a VulkanDevice, settings: Settings) -> Result<Self, Box<dyn Error>> {
        let max_frames = (settings.clip_seconds * settings.fps) as usize;
        Ok(Recording {
            vk,
            settings,
            width: None,
            height: None,
            encoder: None,
            samples: VecDeque::new(),
            max_frames,
        })
    }

    pub fn add_frame(&mut self, frame: &Frame) -> Result<(), Box<dyn Error>> {
        if self.width.is_some() && self.height.is_some() {
            if self.width.unwrap() != frame.width || self.height.unwrap() != frame.height {
                panic!("Resolution changed mid-record, bailing.")
            }
        } else {
            self.width = Some(frame.width);
            self.height = Some(frame.height);
            let format = vk_format(frame.fourcc)
                .ok_or_else(|| format!("unsupported plane format {}", frame.fourcc))?;
            self.encoder = Some(Encoder::new(
                self.vk,
                frame.width,
                frame.height,
                &self.settings,
                format,
            )?);
        }

        let encoder = self
            .encoder
            .as_mut()
            .expect("encoder is created with the first frame");

        self.samples.push_back(encoder.encode_frame(frame)?);
        drop_old_gops(&mut self.samples, self.max_frames);
        Ok(())
    }

    pub fn save(&mut self, clips_dir: impl AsRef<Path>, id: &str) -> Result<(), Box<dyn Error>> {
        let (width, height) = self.width.zip(self.height).ok_or("no frames recorded")?;

        let clips_dir = clips_dir.as_ref();
        fs::create_dir_all(clips_dir)?;
        give_to_invoking_user(clips_dir)?;
        let samples = VecDeque::make_contiguous(&mut self.samples);
        let clip_path = clips_dir.join(format!("{id}.mp4"));
        let part_path = clips_dir.join(format!("{id}.mp4.part"));
        write_mp4(&part_path, &samples, width, height, self.settings.fps)?;
        give_to_invoking_user(&part_path)?;

        let sidecar_path = clips_dir.join(format!("{id}.json"));
        ClipMeta {
            title: None,
            saved_at_unix_secs: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            duration_secs: samples.len() as f64 / f64::from(self.settings.fps),
            fps: self.settings.fps,
            width,
            height,
        }
        .save(&sidecar_path)?;

        // Rename last so a visible .mp4 always has its sidecar.
        fs::rename(part_path, clip_path)?;
        Ok(())
    }
}

// A clip is only decodable from an IDR frame, so the front is trimmed one GOP at a time.
fn drop_old_gops(samples: &mut VecDeque<Sample>, max_frames: usize) {
    while let Some(next_idr) = samples.iter().skip(1).position(|s| s.is_idr).map(|p| p + 1)
        && samples.len() - next_idr >= max_frames
    {
        samples.drain(..next_idr);
    }
}
