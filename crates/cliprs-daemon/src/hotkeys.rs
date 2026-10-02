use evdev::{EventSummary, KeyCode};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub fn hotkey_presses(key: KeyCode) -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    for (_, mut device) in evdev::enumerate() {
        if !device
            .supported_keys()
            .is_some_and(|keys| keys.contains(key))
        {
            continue;
        }
        let tx = tx.clone();
        thread::spawn(move || {
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
        });
    }
    rx
}
