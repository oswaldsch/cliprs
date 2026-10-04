use std::env;
use std::io;
use std::os::unix::fs::chown;
use std::path::Path;

// The daemon runs as root for DRM and evdev access, so files it creates would otherwise be root-owned.
pub fn give_to_invoking_user(path: &Path) -> io::Result<()> {
    let (Some(uid), Some(gid)) = (read_id("SUDO_UID"), read_id("SUDO_GID")) else {
        return Ok(());
    };
    chown(path, Some(uid), Some(gid))
}

fn read_id(name: &str) -> Option<u32> {
    env::var(name).ok()?.parse().ok()
}
