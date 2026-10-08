use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{InvokingUser, env_id, user_entry, xdg_base_dir};

pub const INSTALL_ARGUMENT: &str = "--install";
pub(crate) const SERVICE_UID_VARIABLE: &str = "CLIPRS_UID";
const DAEMON_BINARY: &str = "cliprs-daemon";
const OVERLAY_BINARY: &str = "cliprs-overlay";
const DAEMON_INSTALL_DIR: &str = "/usr/local/bin";
const PACKAGED_PREFIX: &str = "/usr";
const SYSTEM_UNIT_DIR: &str = "/etc/systemd/system";
const POLKIT_RULES_DIR: &str = "/etc/polkit-1/rules.d";
const USER_UNIT_SUBDIR: &str = "systemd/user";
const DAEMON_UNIT_TEMPLATE: &str = "cliprs-daemon@.service";
const OVERLAY_UNIT: &str = "cliprs-overlay.service";
const RUNNING_EXECUTABLE: &str = "/proc/self/exe";
const EXECUTABLE_MODE: u32 = 0o755;
const UNIT_MODE: u32 = 0o644;
const GROUP_OR_OTHER_WRITABLE: u32 = 0o022;
const PKEXEC_DISMISSED: i32 = 126;
const PKEXEC_NOT_AUTHORIZED: i32 = 127;
const UNIT_SPECIAL_CHARACTERS: &[char] = &['%', '$', '"', '\'', '\\', ';'];

pub fn install() -> io::Result<()> {
    install_daemon()?;
    install_overlay()
}

fn install_daemon() -> io::Result<()> {
    let daemon = sibling_binary(DAEMON_BINARY)?;
    let output = Command::new("pkexec")
        .arg(daemon)
        .arg(INSTALL_ARGUMENT)
        .output()
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => io::Error::other("pkexec is not installed"),
            _ => error,
        })?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(PKEXEC_DISMISSED) => Err(io::Error::other("authentication was cancelled")),
        Some(PKEXEC_NOT_AUTHORIZED) => Err(io::Error::other(
            "authorization failed, is a polkit authentication agent running?",
        )),
        _ => Err(io::Error::other(format!(
            "daemon install failed: {}",
            last_line(&output.stderr)
        ))),
    }
}

fn install_overlay() -> io::Result<()> {
    let overlay = sibling_binary(OVERLAY_BINARY)?;
    let unit_path = xdg_base_dir("XDG_CONFIG_HOME", ".config")?
        .join(USER_UNIT_SUBDIR)
        .join(OVERLAY_UNIT);
    write_atomically(&unit_path, UNIT_MODE, overlay_unit(&overlay)?.as_bytes())?;
    systemctl(&["--user", "daemon-reload"])?;
    systemctl(&["--user", "enable", OVERLAY_UNIT])?;
    systemctl(&["--user", "restart", OVERLAY_UNIT])
}

pub fn restart_daemon() -> io::Result<()> {
    systemctl(&["restart", &daemon_instance(unsafe { libc::geteuid() })])
}

fn daemon_instance(uid: u32) -> String {
    DAEMON_UNIT_TEMPLATE.replace('@', &format!("@{uid}"))
}

pub fn install_daemon_as_root() -> io::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(io::Error::other("installing the daemon needs root"));
    }
    let uid = env_id("PKEXEC_UID")
        .or_else(|| env_id("SUDO_UID"))
        .filter(|uid| *uid != 0)
        .ok_or_else(|| io::Error::other("run the setup as your desktop user, not as root"))?;
    let user = user_entry(uid)?;

    let daemon = install_daemon_binary()?;
    write_atomically(
        &Path::new(SYSTEM_UNIT_DIR).join(DAEMON_UNIT_TEMPLATE),
        UNIT_MODE,
        daemon_unit(&daemon)?.as_bytes(),
    )?;
    // Without the rule everything still works, restarts from the GUI just ask for the password.
    if let Err(error) = install_restart_rule(&user) {
        log::warn!("could not install polkit restart rule: {error}");
    }
    let instance = daemon_instance(uid);
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", &instance])?;
    systemctl(&["restart", &instance])
}

fn install_daemon_binary() -> io::Result<PathBuf> {
    // The magic link keeps pointing at the authorized binary even if its path is replaced meanwhile.
    let running = File::open(RUNNING_EXECUTABLE)?;
    let metadata = running.metadata()?;
    let path = env::current_exe()?;
    let root_controlled = metadata.uid() == 0 && metadata.mode() & GROUP_OR_OTHER_WRITABLE == 0;
    if root_controlled && path.starts_with(PACKAGED_PREFIX) {
        return Ok(path);
    }
    // A unit running a user-writable binary would hand root to anything that can replace it.
    let installed = Path::new(DAEMON_INSTALL_DIR).join(DAEMON_BINARY);
    write_atomically(&installed, EXECUTABLE_MODE, running)?;
    Ok(installed)
}

fn install_restart_rule(user: &InvokingUser) -> io::Result<()> {
    let rule_path = Path::new(POLKIT_RULES_DIR).join(format!("50-cliprs-{}.rules", user.uid));
    write_atomically(&rule_path, UNIT_MODE, restart_rule(user)?.as_bytes())
}

fn restart_rule(user: &InvokingUser) -> io::Result<String> {
    let name = &user.name;
    let plain = |character: char| character.is_ascii_alphanumeric() || "_-.".contains(character);
    if name.is_empty() || !name.chars().all(plain) {
        return Err(io::Error::other(format!(
            "user name {name} cannot be written into a polkit rule"
        )));
    }
    let instance = daemon_instance(user.uid);
    Ok(format!(
        r#"polkit.addRule(function(action, subject) {{
    if (action.id == "org.freedesktop.systemd1.manage-units" &&
        action.lookup("unit") == "{instance}" &&
        action.lookup("verb") == "restart" &&
        subject.user == "{name}") {{
        return polkit.Result.YES;
    }}
}});
"#
    ))
}

fn daemon_unit(daemon: &Path) -> io::Result<String> {
    let daemon = unit_safe(daemon)?;
    Ok(format!(
        "[Unit]
Description=cliprs replay buffer for uid %i
BindsTo=user@%i.service
After=user@%i.service
StartLimitIntervalSec=60
StartLimitBurst=3

[Service]
Environment={SERVICE_UID_VARIABLE}=%i
ExecStart={daemon}
Restart=on-failure
RestartSec=5
NoNewPrivileges=yes
ProtectSystem=full

[Install]
WantedBy=user@%i.service
"
    ))
}

fn overlay_unit(overlay: &Path) -> io::Result<String> {
    let overlay = unit_safe(overlay)?;
    Ok(format!(
        "[Unit]
Description=cliprs notification overlay
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart={overlay}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
"
    ))
}

fn unit_safe(path: &Path) -> io::Result<&str> {
    path.to_str()
        .filter(|path| {
            !path.contains(char::is_whitespace) && !path.contains(UNIT_SPECIAL_CHARACTERS)
        })
        .ok_or_else(|| {
            io::Error::other(format!(
                "{} contains characters a systemd unit cannot hold",
                path.display()
            ))
        })
}

fn sibling_binary(name: &str) -> io::Result<PathBuf> {
    let path = env::current_exe()?.with_file_name(name);
    if !path.is_file() {
        return Err(io::Error::other(format!("{} is missing", path.display())));
    }
    Ok(path)
}

// Rename also replaces a binary that is currently executing, which a plain overwrite cannot.
fn write_atomically(path: &Path, mode: u32, mut contents: impl Read) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temp_path)?;
    io::copy(&mut contents, &mut file)?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()?;
    fs::rename(&temp_path, path)
}

fn systemctl(arguments: &[&str]) -> io::Result<()> {
    let output = Command::new("systemctl").args(arguments).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "systemctl {} failed: {}",
        arguments.join(" "),
        last_line(&output.stderr)
    )))
}

fn last_line(output: &[u8]) -> String {
    String::from_utf8_lossy(output)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no error output")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_paths_reject_special_characters() {
        assert!(unit_safe(Path::new("/usr/bin/cliprs-daemon")).is_ok());
        assert!(unit_safe(Path::new("/home/some one/cliprs-daemon")).is_err());
        assert!(unit_safe(Path::new("/home/100%/cliprs-daemon")).is_err());
    }

    #[test]
    fn restart_rule_names_instance_and_rejects_odd_names() {
        let user = |name: &str| InvokingUser {
            uid: 1000,
            name: name.to_owned(),
            gid: 1000,
            home: PathBuf::from("/home/someone"),
        };
        let rule = restart_rule(&user("some.one-1")).unwrap();
        assert!(rule.contains(r#"action.lookup("unit") == "cliprs-daemon@1000.service""#));
        assert!(rule.contains(r#"subject.user == "some.one-1""#));
        assert!(restart_rule(&user(r#"a" || true || ""#)).is_err());
        assert!(restart_rule(&user("")).is_err());
    }

    #[test]
    fn atomic_write_replaces_and_sets_mode() {
        let dir = env::temp_dir().join(format!("cliprs-install-test-{}", std::process::id()));
        let path = dir.join("nested").join(DAEMON_BINARY);
        write_atomically(&path, EXECUTABLE_MODE, &b"old"[..]).unwrap();
        write_atomically(&path, EXECUTABLE_MODE, &b"new"[..]).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, EXECUTABLE_MODE);
        fs::remove_dir_all(&dir).unwrap();
    }
}
