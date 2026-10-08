use std::env;
use std::ffi::{CStr, OsStr};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, chown, fchown};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "cliprs";
const CLIPS_SUBDIR: &str = "Clips";
const THUMBNAILS_SUBDIR: &str = "thumbnails";
const USER_DIRS_FILE: &str = "user-dirs.dirs";
const VIDEOS_DIR_KEY: &str = "XDG_VIDEOS_DIR=";
const DEFAULT_VIDEOS_SUBDIR: &str = "Videos";
const SETTINGS_FILE: &str = "settings.json";
const CAPABILITIES_FILE: &str = "capabilities.json";
const OVERLAY_SOCKET_FILE: &str = "cliprs-overlay.sock";
const DAEMON_LOCK_FILE: &str = "cliprs-daemon.lock";
const OVERLAY_LOCK_FILE: &str = "cliprs-overlay.lock";
const MAX_NOTIFICATION_BYTES: usize = 64 * 1024;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipMeta {
    #[serde(default)]
    pub title: Option<String>,
    pub saved_at_unix_secs: u64,
    pub duration_secs: f64,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
}

impl ClipMeta {
    pub fn load(id: &str) -> io::Result<Option<ClipMeta>> {
        read_json(&clip_meta_path(id)?)
    }

    pub fn save(&self, id: &str) -> io::Result<()> {
        write_json(&clip_meta_path(id)?, self)
    }
}

pub fn clips_dir() -> io::Result<PathBuf> {
    Ok(videos_dir()?.join(CLIPS_SUBDIR))
}

pub fn thumbnail_path(id: &str) -> io::Result<PathBuf> {
    Ok(xdg_base_dir("XDG_CACHE_HOME", ".cache")?
        .join(APP_DIR)
        .join(THUMBNAILS_SUBDIR)
        .join(format!("{id}.jpg")))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Notification {
    ClipSaved { id: String },
    Error { description: String },
}

pub fn notify(notification: &Notification) -> io::Result<()> {
    send_notification(notification, &overlay_socket_path()?)
}

pub fn notify_error(description: impl Into<String>) {
    let notification = Notification::Error {
        description: description.into(),
    };
    if let Err(error) = notify(&notification) {
        log::warn!("overlay unreachable: {error}, dropped {notification:?}");
    }
}

fn send_notification(notification: &Notification, socket_path: &Path) -> io::Result<()> {
    let socket = UnixDatagram::unbound()?;
    // The daemon sends from its capture loop, which must not stall on a full overlay queue.
    socket.set_nonblocking(true)?;
    socket.send_to(&serde_json::to_vec(notification)?, socket_path)?;
    Ok(())
}

pub struct NotificationReceiver {
    socket: UnixDatagram,
}

impl NotificationReceiver {
    pub fn bind() -> io::Result<Self> {
        Self::bind_at(&overlay_socket_path()?)
    }

    fn bind_at(socket_path: &Path) -> io::Result<Self> {
        // A socket file left behind by a previous overlay makes bind fail with AddrInUse.
        match fs::remove_file(socket_path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        Ok(NotificationReceiver {
            socket: UnixDatagram::bind(socket_path)?,
        })
    }

    pub fn receive(&self) -> io::Result<Notification> {
        let mut buffer = vec![0; MAX_NOTIFICATION_BYTES];
        let length = self.socket.recv(&mut buffer)?;
        Ok(serde_json::from_slice(&buffer[..length])?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Process {
    Daemon,
    Overlay,
}

impl Process {
    pub fn running_pid(self) -> io::Result<Option<u32>> {
        lock_holder_pid(&self.lock_path()?)
    }

    pub fn sample_resources(self) -> io::Result<Option<ResourceSample>> {
        match self.running_pid()? {
            Some(pid) => ResourceSample::take(pid),
            None => Ok(None),
        }
    }

    fn lock_path(self) -> io::Result<PathBuf> {
        let file = match self {
            Process::Daemon => DAEMON_LOCK_FILE,
            Process::Overlay => OVERLAY_LOCK_FILE,
        };
        Ok(runtime_dir()?.join(file))
    }
}

pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(process: Process) -> io::Result<Self> {
        Self::acquire_at(&process.lock_path()?)
    }

    fn acquire_at(lock_path: &Path) -> io::Result<Self> {
        // The daemon opens this as root in a user-writable dir, where a planted symlink would redirect the fchown.
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(libc::O_NOFOLLOW)
            .open(lock_path)?;
        if let (Some(uid), Some(gid)) = (sudo_id("SUDO_UID"), sudo_id("SUDO_GID")) {
            fchown(&file, Some(uid), Some(gid))?;
        }
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &whole_file_write_lock()) } == -1 {
            let error = io::Error::last_os_error();
            return Err(match error.raw_os_error() {
                Some(libc::EAGAIN | libc::EACCES) => io::Error::other("already running"),
                _ => error,
            });
        }
        Ok(InstanceLock { _file: file })
    }
}

fn whole_file_write_lock() -> libc::flock {
    let mut lock: libc::flock = unsafe { mem::zeroed() };
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock
}

fn lock_holder_pid(lock_path: &Path) -> io::Result<Option<u32>> {
    let file = match File::open(lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut lock = whole_file_write_lock();
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((lock.l_type != libc::F_UNLCK as libc::c_short).then_some(lock.l_pid as u32))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceSample {
    pub pid: u32,
    pub cpu_time: Duration,
    pub resident_bytes: u64,
    taken_at: Instant,
}

impl ResourceSample {
    fn take(pid: u32) -> io::Result<Option<ResourceSample>> {
        let taken_at = Instant::now();
        let (Some(stat), Some(statm)) =
            (read_proc_file(pid, "stat")?, read_proc_file(pid, "statm")?)
        else {
            return Ok(None);
        };
        let cpu_ticks = parse_cpu_ticks(&stat)
            .ok_or_else(|| io::Error::other(format!("unexpected /proc/{pid}/stat: {stat}")))?;
        let resident_pages = parse_resident_pages(&statm)
            .ok_or_else(|| io::Error::other(format!("unexpected /proc/{pid}/statm: {statm}")))?;
        Ok(Some(ResourceSample {
            pid,
            cpu_time: Duration::from_secs_f64(
                cpu_ticks as f64 / sysconf(libc::_SC_CLK_TCK)? as f64,
            ),
            resident_bytes: resident_pages * sysconf(libc::_SC_PAGESIZE)?,
            taken_at,
        }))
    }

    // 100 is one fully used core, so a multithreaded process can exceed it.
    pub fn cpu_percent_since(&self, previous: &ResourceSample) -> Option<f32> {
        if self.pid != previous.pid {
            return None;
        }
        let elapsed = self.taken_at.checked_duration_since(previous.taken_at)?;
        let used = self.cpu_time.checked_sub(previous.cpu_time)?;
        (!elapsed.is_zero()).then(|| 100.0 * used.as_secs_f32() / elapsed.as_secs_f32())
    }
}

fn read_proc_file(pid: u32, name: &str) -> io::Result<Option<String>> {
    match fs::read_to_string(format!("/proc/{pid}/{name}")) {
        Ok(contents) => Ok(Some(contents)),
        // A process that exits between open and read fails the read with ESRCH.
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(libc::ESRCH) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn parse_cpu_ticks(stat: &str) -> Option<u64> {
    // The comm field may itself contain spaces and parentheses, so fields are counted after its closing one.
    let mut fields = stat.rsplit_once(')')?.1.split_ascii_whitespace().skip(11);
    let user_ticks: u64 = fields.next()?.parse().ok()?;
    let system_ticks: u64 = fields.next()?.parse().ok()?;
    Some(user_ticks + system_ticks)
}

fn parse_resident_pages(statm: &str) -> Option<u64> {
    statm.split_ascii_whitespace().nth(1)?.parse().ok()
}

pub fn total_memory_bytes() -> io::Result<u64> {
    Ok(sysconf(libc::_SC_PHYS_PAGES)? * sysconf(libc::_SC_PAGESIZE)?)
}

fn sysconf(name: libc::c_int) -> io::Result<u64> {
    match unsafe { libc::sysconf(name) } {
        value if value > 0 => Ok(value as u64),
        _ => Err(io::Error::last_os_error()),
    }
}

// The daemon runs as root for DRM and evdev access, so files it creates would otherwise be root-owned.
pub fn give_to_invoking_user(path: &Path) -> io::Result<()> {
    let (Some(uid), Some(gid)) = (sudo_id("SUDO_UID"), sudo_id("SUDO_GID")) else {
        return Ok(());
    };
    chown(path, Some(uid), Some(gid))
}

pub fn create_user_dir(dir: &Path) -> io::Result<()> {
    if dir.exists() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        create_user_dir(parent)?;
    }
    fs::create_dir(dir)?;
    give_to_invoking_user(dir)
}

fn sudo_id(name: &str) -> Option<u32> {
    env::var(name).ok()?.parse().ok()
}

fn invoking_user_home(uid: u32) -> io::Result<PathBuf> {
    let mut passwd = MaybeUninit::<libc::passwd>::uninit();
    let mut strings = [0 as libc::c_char; 4096];
    let mut entry = ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            passwd.as_mut_ptr(),
            strings.as_mut_ptr(),
            strings.len(),
            &mut entry,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if entry.is_null() {
        return Err(io::Error::other(format!("no passwd entry for uid {uid}")));
    }
    let home = unsafe { CStr::from_ptr((*entry).pw_dir) };
    Ok(PathBuf::from(OsStr::from_bytes(home.to_bytes())))
}

fn home_dir() -> io::Result<PathBuf> {
    // sudo resets HOME to /root, which would split the daemon's files from the GUI's.
    if let Some(uid) = sudo_id("SUDO_UID") {
        return invoking_user_home(uid);
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("HOME is not set"))
}

fn xdg_base_dir(variable: &str, home_fallback: &str) -> io::Result<PathBuf> {
    // sudo drops the XDG variables, so under sudo only the default location is known.
    if sudo_id("SUDO_UID").is_none()
        && let Some(dir) = env::var_os(variable)
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    Ok(home_dir()?.join(home_fallback))
}

fn config_dir() -> io::Result<PathBuf> {
    Ok(xdg_base_dir("XDG_CONFIG_HOME", ".config")?.join(APP_DIR))
}

fn clip_meta_path(id: &str) -> io::Result<PathBuf> {
    Ok(xdg_base_dir("XDG_DATA_HOME", ".local/share")?
        .join(APP_DIR)
        .join(format!("{id}.json")))
}

fn videos_dir() -> io::Result<PathBuf> {
    let home = home_dir()?;
    let user_dirs = xdg_base_dir("XDG_CONFIG_HOME", ".config")?.join(USER_DIRS_FILE);
    let configured = fs::read_to_string(user_dirs)
        .ok()
        .and_then(|user_dirs| parse_videos_dir(&user_dirs, &home));
    Ok(configured.unwrap_or_else(|| home.join(DEFAULT_VIDEOS_SUBDIR)))
}

fn parse_videos_dir(user_dirs: &str, home: &Path) -> Option<PathBuf> {
    let value = user_dirs
        .lines()
        .find_map(|line| line.trim().strip_prefix(VIDEOS_DIR_KEY))?
        .trim_matches('"');
    let dir = match value.strip_prefix("$HOME") {
        Some(relative) => home.join(relative.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    dir.is_absolute().then_some(dir)
}

fn settings_path() -> io::Result<PathBuf> {
    Ok(config_dir()?.join(SETTINGS_FILE))
}

fn capabilities_path() -> io::Result<PathBuf> {
    Ok(config_dir()?.join(CAPABILITIES_FILE))
}

fn runtime_dir() -> io::Result<PathBuf> {
    // sudo drops XDG_RUNTIME_DIR, and the overlay binds in the invoking user's runtime dir.
    if let Some(uid) = sudo_id("SUDO_UID") {
        return Ok(PathBuf::from(format!("/run/user/{uid}")));
    }
    match env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Err(io::Error::other("XDG_RUNTIME_DIR is not set")),
    }
}

fn overlay_socket_path() -> io::Result<PathBuf> {
    Ok(runtime_dir()?.join(OVERLAY_SOCKET_FILE))
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
        create_user_dir(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, serde_json::to_vec_pretty(value)?)?;
    give_to_invoking_user(&temp_path)?;
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

    #[test]
    fn videos_dir_from_user_dirs() {
        let home = Path::new("/home/someone");
        let parse = |user_dirs| parse_videos_dir(user_dirs, home);
        assert_eq!(
            parse("XDG_MUSIC_DIR=\"$HOME/Musik\"\nXDG_VIDEOS_DIR=\"$HOME/Filme\"\n"),
            Some(home.join("Filme"))
        );
        assert_eq!(
            parse("XDG_VIDEOS_DIR=\"/mnt/media\""),
            Some(PathBuf::from("/mnt/media"))
        );
        assert_eq!(parse("XDG_VIDEOS_DIR=\"media\""), None);
        assert_eq!(parse("XDG_MUSIC_DIR=\"$HOME/Musik\""), None);
    }

    #[test]
    fn notification_roundtrip_and_stale_socket() {
        let dir = env::temp_dir().join(format!("cliprs-ipc-socket-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let socket_path = dir.join(OVERLAY_SOCKET_FILE);
        let error = Notification::Error {
            description: "encode failed".to_string(),
        };
        assert!(send_notification(&error, &socket_path).is_err());

        drop(NotificationReceiver::bind_at(&socket_path).unwrap());
        let receiver = NotificationReceiver::bind_at(&socket_path).unwrap();
        send_notification(&error, &socket_path).unwrap();
        assert_eq!(receiver.receive().unwrap(), error);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn instance_lock_reports_holder_until_it_exits() {
        let dir = env::temp_dir().join(format!("cliprs-ipc-lock-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock_path = dir.join(DAEMON_LOCK_FILE);
        assert_eq!(lock_holder_pid(&lock_path).unwrap(), None);

        let mut locked = [0; 2];
        assert_eq!(unsafe { libc::pipe(locked.as_mut_ptr()) }, 0);
        let child = unsafe { libc::fork() };
        if child == 0 {
            let lock = InstanceLock::acquire_at(&lock_path);
            unsafe {
                libc::write(locked[1], [lock.is_ok() as u8].as_ptr().cast(), 1);
                libc::pause();
                libc::_exit(0);
            }
        }
        let mut acquired = [0u8];
        assert_eq!(
            unsafe { libc::read(locked[0], acquired.as_mut_ptr().cast(), 1) },
            1
        );
        assert_eq!(acquired, [1]);

        assert_eq!(lock_holder_pid(&lock_path).unwrap(), Some(child as u32));
        assert!(InstanceLock::acquire_at(&lock_path).is_err());

        unsafe {
            libc::kill(child, libc::SIGKILL);
            libc::waitpid(child, ptr::null_mut(), 0);
        }
        assert_eq!(lock_holder_pid(&lock_path).unwrap(), None);
        drop(InstanceLock::acquire_at(&lock_path).unwrap());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resource_sample_parsing_and_own_process() {
        let stat = "4242 (a) weird (name) S 1 4242 4242 0 -1 4194560 900 0 3 0 250 50 0 0 20 0 9 0 81234 1 2";
        assert_eq!(parse_cpu_ticks(stat), Some(300));
        assert_eq!(parse_cpu_ticks("4242 (truncated) S 1"), None);
        assert_eq!(parse_resident_pages("5000 1200 300 10 0 900 0"), Some(1200));

        let earlier = ResourceSample::take(std::process::id()).unwrap().unwrap();
        assert!(earlier.resident_bytes > 0);
        let mut later = earlier;
        later.cpu_time += Duration::from_millis(500);
        later.taken_at += Duration::from_secs(1);
        assert_eq!(later.cpu_percent_since(&earlier), Some(50.0));
        assert_eq!(earlier.cpu_percent_since(&later), None);
        later.pid += 1;
        assert_eq!(later.cpu_percent_since(&earlier), None);

        assert_eq!(ResourceSample::take(u32::MAX).unwrap(), None);
    }
}
