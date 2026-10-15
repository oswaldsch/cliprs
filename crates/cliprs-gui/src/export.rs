use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use cliprs_ipc::{AUDIO_BITRATE_BPS, clips_dir};

const EXPORTS_SUBDIR: &str = "Exports";
const X264_PRESET: &str = "fast";

#[derive(Debug, Clone)]
pub struct ExportPlan {
    pub video_bps: u64,
    pub height: Option<u32>,
    pub fps: Option<u32>,
}

pub fn exports_dir() -> io::Result<PathBuf> {
    Ok(clips_dir()?.join(EXPORTS_SUBDIR))
}

pub fn output_path(source: &Path) -> io::Result<PathBuf> {
    let stem = source
        .file_stem()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "clip has no file name"))?;
    Ok(exports_dir()?.join(format!("{}.mp4", stem.to_string_lossy())))
}

pub fn run(source: &Path, plan: &ExportPlan, out: &Path) -> io::Result<()> {
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }

    let filters: Vec<String> = plan
        .height
        .map(|height| format!("scale=-2:{height}"))
        .into_iter()
        .chain(plan.fps.map(|fps| format!("fps={fps}")))
        .collect();

    // FIXME: two exports of the same clip both can run in parallel
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(source);
    if !filters.is_empty() {
        command.arg("-vf").arg(filters.join(","));
    }
    command
        .args([
            "-c:v",
            "libx264",
            "-preset",
            X264_PRESET,
            "-pix_fmt",
            "yuv420p",
        ])
        .arg("-b:v")
        .arg(plan.video_bps.to_string())
        .arg("-maxrate")
        .arg(plan.video_bps.to_string())
        .arg("-bufsize")
        .arg((plan.video_bps * 2).to_string())
        .args(["-c:a", "aac", "-b:a"])
        .arg(AUDIO_BITRATE_BPS.to_string())
        .args(["-movflags", "+faststart"])
        .arg(out);

    let output = command.output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(io::ErrorKind::NotFound, "ffmpeg is not installed")
        } else {
            error
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    // FIXME: this also deletes the previous good export when a re-export fails early
    let _ = fs::remove_file(out);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let reason = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("unknown error");
    Err(io::Error::other(format!("ffmpeg failed: {reason}")))
}
