use std::error::Error;
use std::io::{self, Write};

use opus::{Application, Bitrate, Channels, Encoder};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::stream::{StreamBox, StreamFlags, StreamState};
use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::pod::Pod;

use crate::audio::{CHANNELS, OPUS_MAX_FRAME_BYTES, SAMPLE_RATE, SAMPLES_PER_PACKET};

const STREAM_NAME: &str = "cliprs";
const BITRATE_BPS: i32 = 160_000;
const BYTES_PER_SAMPLE: usize = size_of::<f32>();

struct Capture {
    encoder: Encoder,
    pending: Vec<f32>,
    packet: [u8; OPUS_MAX_FRAME_BYTES],
    output: io::StdoutLock<'static>,
}

impl Capture {
    fn push(&mut self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let (samples, _) = bytes.as_chunks::<BYTES_PER_SAMPLE>();
        self.pending
            .extend(samples.iter().map(|sample| f32::from_le_bytes(*sample)));
        let mut encoded = 0;
        for samples in self.pending.as_chunks::<SAMPLES_PER_PACKET>().0 {
            let length = self.encoder.encode_float(samples, &mut self.packet)?;
            self.output
                .write_all(&u16::try_from(length)?.to_le_bytes())?;
            self.output.write_all(&self.packet[..length])?;
            encoded += samples.len();
        }
        self.output.flush()?;
        self.pending.drain(..encoded);
        Ok(())
    }
}

pub fn run() -> Result<(), Box<dyn Error>> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, Application::Audio)?;
    encoder.set_bitrate(Bitrate::Bits(BITRATE_BPS))?;
    let capture = Capture {
        encoder,
        pending: Vec::new(),
        packet: [0; OPUS_MAX_FRAME_BYTES],
        output: io::stdout().lock(),
    };

    let stream = StreamBox::new(
        &core,
        STREAM_NAME,
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::STREAM_CAPTURE_SINK => "true",
        },
    )?;

    let on_state = mainloop.clone();
    let on_process = mainloop.clone();
    let _listener = stream
        .add_local_listener_with_user_data(capture)
        .state_changed(move |_, _, _, state| match state {
            StreamState::Error(error) => {
                log::error!("audio stream failed: {error}");
                on_state.quit();
            }
            StreamState::Unconnected => {
                log::error!("audio stream lost its PipeWire connection");
                on_state.quit();
            }
            _ => {}
        })
        .process(move |stream, capture| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let start = data.chunk().offset() as usize;
            let end = start + data.chunk().size() as usize;
            let Some(samples) = data.data().and_then(|bytes| bytes.get(start..end)) else {
                return;
            };
            if let Err(error) = capture.push(samples) {
                log::error!("audio packet was not delivered: {error}");
                on_process.quit();
            }
        })
        .register()?;

    let format = format_pod()?;
    let mut params = [Pod::from_bytes(&format).ok_or("audio format pod is invalid")?];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    mainloop.run();
    Err("audio capture loop ended".into())
}

fn format_pod() -> Result<Vec<u8>, Box<dyn Error>> {
    let mut position = [0; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;

    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(SAMPLE_RATE);
    info.set_channels(CHANNELS);
    info.set_position(position);

    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let (cursor, _) = spa::pod::serialize::PodSerializer::serialize(
        io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )?;
    Ok(cursor.into_inner())
}
