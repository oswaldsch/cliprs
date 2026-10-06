use std::error::Error;
use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::path::Path;

use cliprs_ipc::ClipMeta;
use webm_iterable::matroska_spec::{Master, MatroskaSpec, SimpleBlock};
use webm_iterable::{WebmWriter, WriteOptions};

use crate::encode::Sample;

const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;
const TRACK_NUMBER: u64 = 1;
const TRACK_TYPE_VIDEO: u64 = 1;
const CODEC_ID: &str = "V_MPEG4/ISO/AVC";
const APP_NAME: &str = "cliprs";
const UUID_TAG: &str = "CLIPRS_UUID";
// ffmpeg drops DateUTC when re-encoding but keeps custom tags.
const SAVED_AT_TAG: &str = "CLIPRS_SAVED_AT_UNIX_SECS";
// Matroska dates count nanoseconds from 2001-01-01 UTC.
const MATROSKA_EPOCH_UNIX_SECS: i64 = 978_307_200;
const NANOS_PER_MILLI: u64 = 1_000_000;
const NANOS_PER_SEC: u64 = 1_000_000_000;
const CUES_ID: [u8; 4] = [0x1C, 0x53, 0xBB, 0x6B];
const SEEK_POSITION_ID: u64 = 0x53AC;
// The writer holds the segment header (4-byte ID, 8-byte unknown size) back until the next element.
const SEGMENT_HEADER_LEN: u64 = 12;
const AVCC_VERSION: u8 = 1;
const AVCC_NAL_LENGTH_4_BYTES: u8 = 0xFF;
const AVCC_ONE_SPS: u8 = 0xE1;
const AVCC_ONE_PPS: u8 = 1;

pub fn write_mkv(
    path: &Path,
    samples: &[Sample],
    id: &str,
    meta: &ClipMeta,
) -> Result<(), Box<dyn Error>> {
    let first = samples.first().ok_or("no samples to mux")?;
    let first_nals = split_nal_units(&first.data);
    let sps = find_nal(&first_nals, NAL_SPS).ok_or("first sample has no SPS")?;
    let pps = find_nal(&first_nals, NAL_PPS).ok_or("first sample has no PPS")?;

    let fps = u64::from(meta.fps);
    let timestamp_ms = |frame: usize| frame as u64 * 1000 / fps;

    let mut writer = WebmWriter::new(File::create(path)?);
    writer.write(&MatroskaSpec::Ebml(Master::Full(vec![
        MatroskaSpec::EbmlVersion(1),
        MatroskaSpec::EbmlReadVersion(1),
        MatroskaSpec::EbmlMaxIdLength(4),
        MatroskaSpec::EbmlMaxSizeLength(8),
        MatroskaSpec::DocType("matroska".to_string()),
        MatroskaSpec::DocTypeVersion(4),
        MatroskaSpec::DocTypeReadVersion(2),
    ])))?;

    // An unknown-size segment makes the writer flush each cluster instead of buffering the file.
    writer.write_advanced(
        &MatroskaSpec::Segment(Master::Start),
        WriteOptions::is_unknown_sized_element(),
    )?;
    let segment_start = writer.get_mut().stream_position()? + SEGMENT_HEADER_LEN;
    write_seek_head(&mut writer, 0)?;

    let saved_at_matroska_secs = i64::try_from(meta.saved_at_unix_secs)? - MATROSKA_EPOCH_UNIX_SECS;
    let mut info = vec![
        MatroskaSpec::TimestampScale(NANOS_PER_MILLI),
        MatroskaSpec::Duration(meta.duration_secs * 1000.0),
        MatroskaSpec::DateUTC(saved_at_matroska_secs * NANOS_PER_SEC as i64),
        MatroskaSpec::MuxingApp(APP_NAME.to_string()),
        MatroskaSpec::WritingApp(APP_NAME.to_string()),
    ];
    if let Some(title) = &meta.title {
        info.push(MatroskaSpec::Title(title.clone()));
    }
    writer.write(&MatroskaSpec::Info(Master::Full(info)))?;

    writer.write(&MatroskaSpec::Tracks(Master::Full(vec![
        MatroskaSpec::TrackEntry(Master::Full(vec![
            MatroskaSpec::TrackNumber(TRACK_NUMBER),
            MatroskaSpec::TrackUID(TRACK_NUMBER),
            MatroskaSpec::TrackType(TRACK_TYPE_VIDEO),
            MatroskaSpec::FlagLacing(0),
            MatroskaSpec::CodecID(CODEC_ID.to_string()),
            MatroskaSpec::CodecPrivate(avc_decoder_config(sps, pps)?),
            MatroskaSpec::DefaultDuration(NANOS_PER_SEC / fps),
            MatroskaSpec::Video(Master::Full(vec![
                MatroskaSpec::PixelWidth(u64::from(meta.width)),
                MatroskaSpec::PixelHeight(u64::from(meta.height)),
            ])),
        ])),
    ])))?;

    writer.write(&MatroskaSpec::Tags(Master::Full(vec![MatroskaSpec::Tag(
        Master::Full(vec![
            MatroskaSpec::Targets(Master::Full(vec![])),
            simple_tag(UUID_TAG, id),
            simple_tag(SAVED_AT_TAG, &meta.saved_at_unix_secs.to_string()),
        ]),
    )])))?;

    let mut cue_points = Vec::new();
    let mut frame = 0;
    for gop in samples.chunk_by(|_, next| !next.is_idr) {
        let cluster_ms = timestamp_ms(frame);
        let cluster_position = writer.get_mut().stream_position()? - segment_start;
        cue_points.push(MatroskaSpec::CuePoint(Master::Full(vec![
            MatroskaSpec::CueTime(cluster_ms),
            MatroskaSpec::CueTrackPositions(Master::Full(vec![
                MatroskaSpec::CueTrack(TRACK_NUMBER),
                MatroskaSpec::CueClusterPosition(cluster_position),
            ])),
        ])));

        writer.write(&MatroskaSpec::Cluster(Master::Start))?;
        writer.write(&MatroskaSpec::Timestamp(cluster_ms))?;
        for sample in gop {
            let data = to_avcc(&sample.data);
            let block = SimpleBlock::new_uncheked(
                &data,
                TRACK_NUMBER,
                i16::try_from(timestamp_ms(frame) - cluster_ms)?,
                false,
                None,
                false,
                sample.is_idr,
            );
            writer.write(&MatroskaSpec::from(block))?;
            frame += 1;
        }
        writer.write(&MatroskaSpec::Cluster(Master::End))?;
    }

    let cues_position = writer.get_mut().stream_position()? - segment_start;
    writer.write(&MatroskaSpec::Cues(Master::Full(cue_points)))?;

    writer.get_mut().seek(SeekFrom::Start(segment_start))?;
    write_seek_head(&mut writer, cues_position)?;
    writer.into_inner()?;
    Ok(())
}

// SeekPosition is written raw at a fixed 8 bytes so the placeholder can be overwritten in place.
fn write_seek_head(
    writer: &mut WebmWriter<File>,
    cues_position: u64,
) -> Result<(), Box<dyn Error>> {
    writer.write(&MatroskaSpec::SeekHead(Master::Start))?;
    writer.write(&MatroskaSpec::Seek(Master::Start))?;
    writer.write(&MatroskaSpec::SeekID(CUES_ID.to_vec()))?;
    writer.write_raw(SEEK_POSITION_ID, &cues_position.to_be_bytes())?;
    writer.write(&MatroskaSpec::Seek(Master::End))?;
    writer.write(&MatroskaSpec::SeekHead(Master::End))?;
    Ok(())
}

fn simple_tag(name: &str, value: &str) -> MatroskaSpec {
    MatroskaSpec::SimpleTag(Master::Full(vec![
        MatroskaSpec::TagName(name.to_string()),
        MatroskaSpec::TagString(value.to_string()),
    ]))
}

fn avc_decoder_config(sps: &[u8], pps: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let profile_and_level = sps.get(1..4).ok_or("SPS is too short")?;

    let mut config = vec![AVCC_VERSION];
    config.extend_from_slice(profile_and_level);
    config.extend_from_slice(&[AVCC_NAL_LENGTH_4_BYTES, AVCC_ONE_SPS]);
    config.extend_from_slice(&u16::try_from(sps.len())?.to_be_bytes());
    config.extend_from_slice(sps);
    config.push(AVCC_ONE_PPS);
    config.extend_from_slice(&u16::try_from(pps.len())?.to_be_bytes());
    config.extend_from_slice(pps);
    Ok(config)
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
