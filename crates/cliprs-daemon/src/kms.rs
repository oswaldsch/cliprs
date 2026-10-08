use drm::control::{Device as ControlDevice, connector, crtc, plane};
use drm::{ClientCapability, Device};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

const PLANE_TYPE_PRIMARY: u64 = 1;

pub struct Card {
    file: File,
    primary_planes: Vec<plane::Handle>,
    connector: Option<connector::Handle>,
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

pub fn open_monitor(wanted: Option<&str>) -> Result<Card, Box<dyn Error>> {
    let mut fallback = None;
    for path in cliprs_ipc::drm_card_paths()? {
        let card = match Card::probe(&path, wanted) {
            Ok(card) => card,
            Err(error) => {
                log::warn!("skipped {}: {error}", path.display());
                continue;
            }
        };
        if card.connector.is_some() {
            log::info!(
                "recording {} on {}",
                wanted.unwrap_or_default(),
                path.display()
            );
            return Ok(card);
        }
        if fallback.is_none() && !card.active_primary_planes()?.is_empty() {
            log::info!("recording the first active output on {}", path.display());
            fallback = Some(card);
        }
    }
    if let Some(wanted) = wanted {
        log::warn!("monitor {wanted} is not connected, recording another output");
    }
    fallback.ok_or_else(|| "no card with an active output".into())
}

impl Card {
    fn probe(path: &Path, wanted: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let mut card = Card::open(path)?;
        if let Some(wanted) = wanted {
            card.connector = card.connected_connector(wanted)?;
        }
        Ok(card)
    }

    fn open(path: &Path) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut card = Card {
            file,
            primary_planes: Vec::new(),
            connector: None,
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

    fn connected_connector(&self, id: &str) -> Result<Option<connector::Handle>, Box<dyn Error>> {
        for &handle in self.resource_handles()?.connectors() {
            let connector = self.get_connector(handle, false)?;
            if connector.state() == connector::State::Connected
                && cliprs_ipc::connector_id(&connector) == id
            {
                return Ok(Some(handle));
            }
        }
        Ok(None)
    }

    fn connector_crtc(
        &self,
        connector: connector::Handle,
    ) -> Result<Option<crtc::Handle>, Box<dyn Error>> {
        let Some(encoder) = self.get_connector(connector, false)?.current_encoder() else {
            return Ok(None);
        };
        Ok(self.get_encoder(encoder)?.crtc())
    }

    pub fn active_primary_planes(&self) -> Result<Vec<plane::Handle>, Box<dyn Error>> {
        let wanted_crtc = match self.connector {
            Some(connector) => match self.connector_crtc(connector)? {
                Some(crtc) => Some(crtc),
                None => return Ok(Vec::new()),
            },
            None => None,
        };
        let mut out = Vec::new();
        for &p in &self.primary_planes {
            let plane = self.get_plane(p)?;
            let on_wanted_crtc = wanted_crtc.is_none() || plane.crtc() == wanted_crtc;
            if plane.framebuffer().is_some() && on_wanted_crtc {
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
