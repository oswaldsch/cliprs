use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "cliprs";
const SETTINGS_FILE: &str = "settings.json";
const CAPABILITIES_FILE: &str = "capabilities.json";
const PEAK_TO_AVERAGE_BITRATE: u64 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub fps: u32,
    pub average_bitrate_bps: u64,
    pub clip_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            fps: 60,
            average_bitrate_bps: 30_000_000,
            clip_seconds: 30,
        }
    }
}

impl Settings {
    pub fn peak_bitrate_bps(&self) -> u64 {
        self.average_bitrate_bps * PEAK_TO_AVERAGE_BITRATE
    }

    pub fn load() -> io::Result<Settings> {
        Ok(read_json(&settings_path()?)?.unwrap_or_default())
    }

    pub fn save(&self) -> io::Result<()> {
        write_json(&settings_path()?, self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub max_bitrate_bps: u64,
    pub gop_frames: u32,
}

impl Capabilities {
    pub fn load() -> io::Result<Option<Capabilities>> {
        read_json(&capabilities_path()?)
    }

    pub fn save(&self) -> io::Result<()> {
        write_json(&capabilities_path()?, self)
    }
}

fn config_dir() -> io::Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(
            env::var_os("HOME")
                .ok_or_else(|| io::Error::other("neither XDG_CONFIG_HOME nor HOME is set"))?,
        )
        .join(".config"),
    };
    Ok(base.join(APP_DIR))
}

fn settings_path() -> io::Result<PathBuf> {
    Ok(config_dir()?.join(SETTINGS_FILE))
}

fn capabilities_path() -> io::Result<PathBuf> {
    Ok(config_dir()?.join(CAPABILITIES_FILE))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

// Rename is atomic, so a concurrent reader never sees a half-written file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&temp_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_and_missing_file() {
        let dir = env::temp_dir().join(format!("cliprs-ipc-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(read_json::<Settings>(&path).unwrap(), None);

        let settings = Settings {
            fps: 144,
            average_bitrate_bps: 20_000_000,
            clip_seconds: 15,
        };
        write_json(&path, &settings).unwrap();
        assert_eq!(read_json::<Settings>(&path).unwrap(), Some(settings));

        fs::remove_dir_all(&dir).unwrap();
    }
}
