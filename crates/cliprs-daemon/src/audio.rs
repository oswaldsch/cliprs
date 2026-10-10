use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

pub const PIPEWIRE_CAPTURE_ARGUMENT: &str = "--pipewire-capture";
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;
pub const SAMPLES_PER_PACKET: usize = (SAMPLE_RATE as usize / 50) * CHANNELS as usize;
// libopus lookahead at 48 kHz for OPUS_APPLICATION_AUDIO
pub const ENCODER_DELAY_FRAMES: u16 = 312;
pub const OPUS_MAX_FRAME_BYTES: usize = 1275;
const PACKET_NANOS: i64 = 20_000_000;
// above the largest PipeWire quantum, packets arrive in bursts of one quantum
const MAX_LATENESS_NANOS: i64 = 250_000_000;
const RUNNING_EXECUTABLE: &str = "/proc/self/exe";
const MIN_RESPAWN_DELAY: Duration = Duration::from_secs(5);
const MAX_RESPAWN_DELAY: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct Packet {
    pub arrived_at: Instant,
    pub data: Vec<u8>,
}

pub struct Block<'a> {
    pub start_ms: u64,
    pub data: &'a [u8],
}

#[derive(Clone, Default)]
pub struct Ring(Arc<Mutex<VecDeque<Packet>>>);

impl Ring {
    pub fn packets(&self) -> Vec<Packet> {
        self.lock().iter().cloned().collect()
    }

    pub fn drop_before(&self, oldest: Instant) {
        let mut packets = self.lock();
        let stale = packets.partition_point(|packet| packet.arrived_at < oldest);
        packets.drain(..stale);
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<Packet>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn start() -> Ring {
    let ring = Ring::default();
    let filled = ring.clone();
    thread::spawn(move || {
        let mut delay = MIN_RESPAWN_DELAY;
        loop {
            let started = Instant::now();
            match run_helper(&filled) {
                Ok(error) => log::warn!("audio helper stopped: {error}"),
                Err(error) => {
                    log::warn!("audio is disabled, helper did not start: {error}");
                    return;
                }
            }
            delay = if started.elapsed() > MAX_RESPAWN_DELAY {
                MIN_RESPAWN_DELAY
            } else {
                (delay * 2).min(MAX_RESPAWN_DELAY)
            };
            thread::sleep(delay);
        }
    });
    ring
}

fn run_helper(ring: &Ring) -> io::Result<io::Error> {
    let mut helper = cliprs_ipc::unprivileged_command(RUNNING_EXECUTABLE)?
        .arg(PIPEWIRE_CAPTURE_ARGUMENT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut output = helper.stdout.take().expect("helper stdout is piped");
    let error = read_packets(&mut output, ring);
    let _ = helper.kill();
    helper.wait()?;
    Ok(error)
}

fn read_packets(source: &mut impl Read, ring: &Ring) -> io::Error {
    loop {
        match read_packet(source) {
            Ok(data) => ring.lock().push_back(Packet {
                arrived_at: Instant::now(),
                data,
            }),
            Err(error) => return error,
        }
    }
}

fn read_packet(source: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 2];
    source.read_exact(&mut length)?;
    let length = usize::from(u16::from_le_bytes(length));
    if length == 0 || length > OPUS_MAX_FRAME_BYTES {
        return Err(io::Error::other(format!(
            "helper sent a {length} byte packet"
        )));
    }
    let mut data = vec![0; length];
    source.read_exact(&mut data)?;
    Ok(data)
}

pub fn blocks(packets: &[Packet], origin: Instant) -> Vec<Block<'_>> {
    let arrivals: Vec<i64> = packets
        .iter()
        .map(|packet| signed_nanos(packet.arrived_at, origin))
        .collect();
    let mut blocks = Vec::with_capacity(packets.len());
    let mut run_start = 0;
    while run_start < packets.len() {
        let (run_len, first_arrival) = leading_run(&arrivals[run_start..]);
        for (position, packet) in packets[run_start..run_start + run_len].iter().enumerate() {
            // a packet arrives once its last sample was captured
            let start = first_arrival + (position as i64 - 1) * PACKET_NANOS;
            if let Ok(start) = u64::try_from(start) {
                blocks.push(Block {
                    start_ms: start / 1_000_000,
                    data: &packet.data,
                });
            }
        }
        run_start += run_len;
    }
    blocks
}

// the audio clock is steadier than pipe delivery, so a run is timed from its least delayed packet
fn leading_run(arrivals: &[i64]) -> (usize, i64) {
    let mut first_arrival = arrivals[0];
    for (position, arrival) in arrivals.iter().enumerate().skip(1) {
        let implied = arrival - position as i64 * PACKET_NANOS;
        if implied - first_arrival > MAX_LATENESS_NANOS {
            return (position, first_arrival);
        }
        first_arrival = first_arrival.min(implied);
    }
    (arrivals.len(), first_arrival)
}

fn signed_nanos(at: Instant, origin: Instant) -> i64 {
    match at.checked_duration_since(origin) {
        Some(after) => after.as_nanos() as i64,
        None => -(origin.duration_since(at).as_nanos() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(origin: Instant, arrival_ms: u64) -> Packet {
        Packet {
            arrived_at: origin + Duration::from_millis(arrival_ms),
            data: vec![0],
        }
    }

    fn start_times(arrivals_ms: &[u64], origin_ms: u64) -> Vec<u64> {
        let base = Instant::now();
        let packets: Vec<Packet> = arrivals_ms.iter().map(|ms| packet(base, *ms)).collect();
        blocks(&packets, base + Duration::from_millis(origin_ms))
            .iter()
            .map(|block| block.start_ms)
            .collect()
    }

    #[test]
    fn bursts_are_spread_onto_the_packet_grid() {
        assert_eq!(start_times(&[40, 40, 80, 80], 0), [0, 20, 40, 60]);
    }

    #[test]
    fn packets_before_the_origin_are_dropped() {
        assert_eq!(start_times(&[20, 40, 60, 80], 30), [10, 30]);
    }

    #[test]
    fn a_pause_starts_a_new_run() {
        assert_eq!(start_times(&[20, 40, 1000, 1020], 0), [0, 20, 980, 1000]);
    }

    #[test]
    fn oversized_and_empty_packets_are_rejected() {
        let oversized = (OPUS_MAX_FRAME_BYTES as u16 + 1).to_le_bytes();
        assert!(read_packet(&mut oversized.as_slice()).is_err());
        assert!(read_packet(&mut [0, 0].as_slice()).is_err());
        assert_eq!(read_packet(&mut [1, 0, 7].as_slice()).unwrap(), [7]);
    }
}
