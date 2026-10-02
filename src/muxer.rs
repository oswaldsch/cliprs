use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use bytes::Bytes;
use mp4::{
    AvcConfig, FourCC, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig, TrackType,
};

use crate::encode::Sample;

const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;
const TRACK_ID: u32 = 1;
const BRANDS: [&str; 4] = ["isom", "iso2", "avc1", "mp41"];

pub fn write_mp4(
    path: &Path,
    samples: &[Sample],
    width: u32,
    height: u32,
    fps: u32,
) -> Result<(), Box<dyn Error>> {
    let first = samples.first().ok_or("no samples to mux")?;
    let first_nals = split_nal_units(&first.data);
    let sps = find_nal(&first_nals, NAL_SPS).ok_or("first sample has no SPS")?;
    let pps = find_nal(&first_nals, NAL_PPS).ok_or("first sample has no PPS")?;

    let config = Mp4Config {
        major_brand: "isom".parse::<FourCC>()?,
        minor_version: 512,
        compatible_brands: BRANDS
            .iter()
            .map(|b| b.parse::<FourCC>())
            .collect::<Result<_, _>>()?,
        timescale: 1000,
    };
    let mut writer = Mp4Writer::write_start(BufWriter::new(File::create(path)?), &config)?;

    writer.add_track(&TrackConfig {
        track_type: TrackType::Video,
        timescale: fps,
        language: "und".to_string(),
        media_conf: MediaConfig::AvcConfig(AvcConfig {
            width: width as u16,
            height: height as u16,
            seq_param_set: sps.to_vec(),
            pic_param_set: pps.to_vec(),
        }),
    })?;

    for (i, sample) in samples.iter().enumerate() {
        writer.write_sample(
            TRACK_ID,
            &Mp4Sample {
                start_time: i as u64,
                duration: 1,
                rendering_offset: 0,
                is_sync: sample.is_idr,
                bytes: Bytes::from(to_avcc(&sample.data)),
            },
        )?;
    }

    writer.write_end()?;
    writer.into_writer().flush()?;
    Ok(())
}

fn split_nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }

    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let end = starts.get(n + 1).map_or(data.len(), |&next| next - 3);
            let mut nal = &data[start..end];
            // A NAL never ends in 0x00, so trailing zeros belong to the next 4-byte start code
            while let [rest @ .., 0] = nal {
                nal = rest;
            }
            nal
        })
        .filter(|nal| !nal.is_empty())
        .collect()
}

fn nal_type(nal: &[u8]) -> u8 {
    nal[0] & 0x1F
}

fn find_nal<'a>(nals: &[&'a [u8]], wanted: u8) -> Option<&'a [u8]> {
    nals.iter().copied().find(|nal| nal_type(nal) == wanted)
}

fn to_avcc(annex_b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(annex_b.len());
    for nal in split_nal_units(annex_b) {
        if matches!(nal_type(nal), NAL_SPS | NAL_PPS) {
            continue;
        }
        out.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        out.extend_from_slice(nal);
    }
    out
}
