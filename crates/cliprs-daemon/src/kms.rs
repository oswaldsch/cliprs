use drm::control::{Device as ControlDevice, plane};
use drm::{ClientCapability, Device};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

const PLANE_TYPE_PRIMARY: u64 = 1;

pub struct Card {
    file: File,
    primary_planes: Vec<plane::Handle>,
}

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}
impl Device for Card {}
impl ControlDevice for Card {}

pub struct Frame {
    pub dmabuf: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub fourcc: drm_fourcc::DrmFourcc,
    pub modifier: u64,
    pub planes: Vec<(u32, u32)>,
}

impl Card {
    pub fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut card = Card {
            file,
            primary_planes: Vec::new(),
        };
        card.set_client_capability(ClientCapability::UniversalPlanes, true)?;
        for p in card.plane_handles()? {
            if card.is_primary(p)? {
                card.primary_planes.push(p);
            }
        }
        Ok(card)
    }

    fn is_primary(&self, plane: plane::Handle) -> Result<bool, Box<dyn Error>> {
        for (&prop, &value) in self.get_properties(plane)?.iter() {
            if self.get_property(prop)?.name().to_bytes() == b"type" {
                return Ok(value == PLANE_TYPE_PRIMARY);
            }
        }
        Ok(false)
    }

    pub fn active_primary_planes(&self) -> Result<Vec<plane::Handle>, Box<dyn Error>> {
        let mut out = Vec::new();
        for &p in &self.primary_planes {
            if self.get_plane(p)?.framebuffer().is_some() {
                out.push(p);
            }
        }
        Ok(out)
    }

    pub fn grab(&self, plane: plane::Handle) -> Result<Frame, Box<dyn Error>> {
        let fb = self
            .get_plane(plane)?
            .framebuffer()
            .ok_or("plane has no framebuffer")?;
        let info = self.get_planar_framebuffer(fb)?;
        let handle = info.buffers()[0].ok_or("no buffer handle, run as root")?;

        let dmabuf = self.buffer_to_prime_fd(handle, 0)?;
        self.close_buffer(handle)?;

        let planes = info
            .buffers()
            .iter()
            .enumerate()
            .filter(|(_, b)| b.is_some())
            .map(|(i, _)| (info.offsets()[i], info.pitches()[i]))
            .collect();

        let (width, height) = info.size();
        Ok(Frame {
            dmabuf,
            width,
            height,
            fourcc: info.pixel_format(),
            modifier: info.modifier().ok_or("no modifier")?.into(),
            planes,
        })
    }
}
