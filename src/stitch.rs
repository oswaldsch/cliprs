use std::error::Error;
use std::path::{Path, PathBuf};

use crate::encode::{Encoder, Sample};
use crate::kms::Frame;
use crate::muxer::write_mp4;
use crate::vulkan::{VulkanDevice, vk_format};

pub struct Recording<'a> {
    vk: &'a VulkanDevice,
    fps: u32,
    width: Option<u32>,
    height: Option<u32>,
    encoder: Option<Encoder<'a>>,
    samples: Vec<Sample>,
    path: PathBuf,
}

impl<'a> Recording<'a> {
    pub fn new(
        vk: &'a VulkanDevice,
        fps: u32,
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn Error>> {
        Ok(Recording {
            vk,
            fps,
            width: None,
            height: None,
            encoder: None,
            samples: Vec::new(),
            path: path.as_ref().to_path_buf(),
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
                self.fps,
                format,
            )?);
        }

        let encoder = self
            .encoder
            .as_mut()
            .expect("encoder is created with the first frame");

        self.samples.push(encoder.encode_frame(frame)?);
        Ok(())
    }

    pub fn finish(self) -> Result<(), Box<dyn Error>> {
        let (width, height) = self.width.zip(self.height).ok_or("no frames recorded")?;
        write_mp4(&self.path, &self.samples, width, height, self.fps)
    }
}
