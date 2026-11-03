use evdev::{Device, EventSummary, KeyCode};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

const INPUT_DIR: &str = "/dev/input";
const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

type Watched = Arc<Mutex<HashSet<PathBuf>>>;

pub fn hotkey_presses(key: KeyCode) -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    let watched = Watched::default();
    thread::spawn(move || {
        loop {
            if let Err(error) = watch_new_devices(key, &tx, &watched) {
                log::warn!("input scan failed: {error}");
            }
            thread::sleep(RESCAN_INTERVAL);
        }
    });
    rx
}

fn watch_new_devices(key: KeyCode, tx: &Sender<()>, watched: &Watched) -> io::Result<()> {
    for entry in fs::read_dir(INPUT_DIR)? {
        let path = entry?.path();
        let is_event_node = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("event"));
        if !is_event_node || lock(watched).contains(&path) {
            continue;
        }
        let Ok(device) = Device::open(&path) else {
            continue;
        };
        if !device
            .supported_keys()
            .is_some_and(|keys| keys.contains(key))
        {
            continue;
        }
        log::info!("watching {} for the hotkey", path.display());
        lock(watched).insert(path.clone());
        let tx = tx.clone();
        let watched = Arc::clone(watched);
        thread::spawn(move || {
            read_presses(device, key, &tx);
            lock(&watched).remove(&path);
        });
    }
    Ok(())
}

fn read_presses(mut device: Device, key: KeyCode, tx: &Sender<()>) {
    while let Ok(events) = device.fetch_events() {
        for event in events {
            if let EventSummary::Key(_, code, 1) = event.destructure()
                && code == key
                && tx.send(()).is_err()
            {
                return;
            }
        }
    }
}

fn lock(watched: &Watched) -> MutexGuard<'_, HashSet<PathBuf>> {
    watched.lock().unwrap_or_else(PoisonError::into_inner)
}
