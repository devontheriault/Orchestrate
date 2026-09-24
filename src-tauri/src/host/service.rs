//! The Host as an OS service: starting it when a window finds none
//! listening, and whether it keeps running while its user is logged out.
//!
//! Installed builds hand it to the OS's service manager, so it starts at login
//! and outlives every window. Only systemd is supported so far; another OS adds
//! its own branch to [`start`]. Development builds, and any window with its own
//! `ORCHESTRATE_STATE_DIR`, start it as a plain detached process instead, so
//! a build under test never replaces the user's real service.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::paths;

/// Start a Host for this window's state directory. Returns once it has been
/// launched, not once it is listening; the caller waits for the socket.
pub fn start() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if systemd::manages_host() {
        return systemd::start();
    }
    detached()
}

/// Run `<this binary> --host` in its own session, so it outlives the window
/// that started it, writing what it says to the state directory's host log.
fn detached() -> io::Result<()> {
    paths::ensure_dirs().map_err(io::Error::other)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::host_log().map_err(io::Error::other)?)?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--host")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches nothing but this process.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console, and out of the window's process group, so a Ctrl+C
        // meant for the window doesn't reach it.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = cmd.spawn()?;
    // Reaped here if it exits while the window is still open, rather than
    // left a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Whether this Host keeps running while its user is logged out, or `None`
/// when it can't be made to: it isn't running as a service.
pub fn keeps_running() -> Option<bool> {
    #[cfg(target_os = "linux")]
    return systemd::lingers();
    #[cfg(not(target_os = "linux"))]
    None
}

/// Keep this Host running while its user is logged out, or stop doing so.
pub fn set_keeps_running(on: bool) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    return systemd::set_linger(on);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = on;
        Err(io::Error::other("not supported on this system yet"))
    }
}

/// The systemd user unit that runs the Host.
pub fn unit(exe: &Path, env_file: &Path) -> String {
    format!(
        "\
# Written by Orchestrate, and rewritten whenever it starts its Host.
[Unit]
Description=Orchestrate Host: keeps agents running with no window open

[Service]
ExecStart=\"{exe}\" --host
EnvironmentFile=-{env}
# SIGTERM to the Host alone, which marks running agents Orphaned before ending
# their `claude`s. Anything left is killed after it exits.
KillMode=mixed
TimeoutStopSec=30
Restart=on-failure

[Install]
WantedBy=default.target
",
        exe = escape_specifiers(&exe.to_string_lossy()),
        env = escape_specifiers(&env_file.to_string_lossy()),
    )
}

/// systemd reads `%` as the start of a specifier in these settings.
fn escape_specifiers(s: &str) -> String {
    s.replace('%', "%%")
}

/// Variables a Host needs from the window's environment. A service starts with
/// almost none, and `claude` — and every build tool an Agent runs — is found
/// on the user's own `PATH`, often somewhere only their shell adds.
const KEEP: &[&str] = &[
    "PATH",
    "LANG",
    "LANGUAGE",
    "SSH_AUTH_SOCK",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    // The state directory is found through these, and window and Host must
    // agree on it.
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
];

/// Prefixes of variables that configure Claude Code or the cloud it calls.
const KEEP_PREFIXES: &[&str] = &[
    "LC_",
    "CLAUDE_",
    "ANTHROPIC_",
    "AWS_",
    "GOOGLE_",
    "CLOUD_ML_",
    "VERTEX_",
];

/// The window's environment, cut down to what the Host needs, as a systemd
/// environment file. `appdir` is the AppImage's mount, whose `PATH` entries
/// vanish with the window that mounted it.
pub fn environment(
    vars: impl IntoIterator<Item = (String, String)>,
    appdir: Option<&str>,
) -> String {
    let mut kept: Vec<(String, String)> = vars
        .into_iter()
        .filter(|(k, v)| {
            !v.contains('\n')
                && (KEEP.contains(&k.as_str()) || KEEP_PREFIXES.iter().any(|p| k.starts_with(p)))
        })
        .map(|(k, v)| {
            if k != "PATH" {
                return (k, v);
            }
            let path = v
                .split(':')
                .filter(|dir| !appdir.is_some_and(|a| !a.is_empty() && dir.starts_with(a)))
                .collect::<Vec<_>>()
                .join(":");
            (k, path)
        })
        .collect();
    kept.sort();
    kept.iter()
        .map(|(k, v)| format!("{k}=\"{}\"\n", v.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect()
}

#[cfg(target_os = "linux")]
mod systemd {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use crate::paths;

    const UNIT: &str = "orchestrate-host.service";
    /// The unit's name from before the app was renamed.
    const LEGACY_UNIT: &str = "claudewrapper-host.service";

    /// Whether the user's systemd instance, and so this Host, outlives their
    /// last session. Only asked of a Host that systemd runs: one started any
    /// other way ends with the session whatever this says.
    pub fn lingers() -> Option<bool> {
        let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok()?;
        if !cgroup.contains(UNIT) {
            return None;
        }
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let out = Command::new("loginctl")
            .args([
                "show-user",
                &uid.to_string(),
                "--property=Linger",
                "--value",
            ])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim() == "yes")
    }

    /// Lingering is logind's word for a user whose services run while they
    /// are logged out. Setting it for yourself needs no password on most
    /// systems.
    pub fn set_linger(on: bool) -> io::Result<()> {
        let verb = if on {
            "enable-linger"
        } else {
            "disable-linger"
        };
        let out = Command::new("loginctl").arg(verb).output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    }

    /// Whether this window's Host belongs to systemd: an installed build using
    /// the default state directory, on a machine running a user manager.
    pub fn manages_host() -> bool {
        !cfg!(debug_assertions)
            && std::env::var_os("ORCHESTRATE_STATE_DIR").is_none()
            && Command::new("systemctl")
                .args(["--user", "show-environment"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
    }

    /// Write the unit and its environment as this window would have them, and
    /// start it — enabled, so it comes up at every login from now on.
    pub fn start() -> io::Result<()> {
        // Inside an AppImage the binary lives in a mount that goes away with
        // this window; the AppImage itself is what the unit must run.
        let exe = match std::env::var_os("APPIMAGE") {
            Some(appimage) => PathBuf::from(appimage),
            None => std::env::current_exe()?,
        };
        let env_file = paths::state_dir()
            .map_err(io::Error::other)?
            .join("host.env");
        let appdir = std::env::var("APPDIR").ok();
        write_if_changed(
            &env_file,
            &super::environment(std::env::vars(), appdir.as_deref()),
        )?;

        let unit_dir = dirs::config_dir()
            .ok_or_else(|| io::Error::other("no config directory"))?
            .join("systemd/user");
        // Left enabled, it would race this one for the Host lock at every login.
        let legacy = unit_dir.join(LEGACY_UNIT);
        if legacy.exists() {
            let _ = systemctl(&["disable", LEGACY_UNIT]);
            let _ = std::fs::remove_file(&legacy);
            systemctl(&["daemon-reload"])?;
        }
        let unit_file = unit_dir.join(UNIT);
        if write_if_changed(&unit_file, &super::unit(&exe, &env_file))? {
            systemctl(&["daemon-reload"])?;
        }
        // A Host that crashed too often in a row is held back until reset.
        let _ = systemctl(&["reset-failed", UNIT]);
        systemctl(&["enable", "--now", UNIT])
    }

    fn systemctl(args: &[&str]) -> io::Result<()> {
        let status = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .stdout(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "`systemctl --user {}` failed",
                args.join(" ")
            )))
        }
    }

    /// Write `contents` to `path` unless it already holds exactly that.
    /// Returns whether it wrote.
    fn write_if_changed(path: &Path, contents: &str) -> io::Result<bool> {
        if std::fs::read_to_string(path).is_ok_and(|old| old == contents) {
            return Ok(false);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, contents)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn environment_keeps_what_agents_need_and_nothing_else() {
        let env = environment(
            vars(&[
                ("PATH", "/home/u/.local/bin:/usr/bin"),
                ("ANTHROPIC_API_KEY", "sk-1"),
                ("CLAUDE_CONFIG_DIR", "/home/u/.claude"),
                ("LC_ALL", "C"),
                ("WAYLAND_DISPLAY", "wayland-1"),
                ("ORCHESTRATE_STATE_DIR", "/tmp/x"),
                ("TERM", "xterm"),
            ]),
            None,
        );
        assert_eq!(
            env,
            "ANTHROPIC_API_KEY=\"sk-1\"\n\
             CLAUDE_CONFIG_DIR=\"/home/u/.claude\"\n\
             LC_ALL=\"C\"\n\
             PATH=\"/home/u/.local/bin:/usr/bin\"\n"
        );
    }

    #[test]
    fn environment_drops_the_appimage_mount_from_path() {
        let env = environment(
            vars(&[("PATH", "/tmp/.mount_abc/usr/bin:/usr/bin")]),
            Some("/tmp/.mount_abc"),
        );
        assert_eq!(env, "PATH=\"/usr/bin\"\n");
    }

    #[test]
    fn environment_quotes_values() {
        let env = environment(vars(&[("HTTPS_PROXY", r#"http://a"b\c"#)]), None);
        assert_eq!(env, "HTTPS_PROXY=\"http://a\\\"b\\\\c\"\n");
    }

    #[test]
    fn unit_runs_the_binary_as_a_host_and_escapes_specifiers() {
        let unit = unit(
            Path::new("/opt/Orchestrate/app%1"),
            Path::new("/home/u/.local/state/orchestrate/host.env"),
        );
        assert!(unit.contains("ExecStart=\"/opt/Orchestrate/app%%1\" --host\n"));
        assert!(unit.contains("EnvironmentFile=-/home/u/.local/state/orchestrate/host.env\n"));
        assert!(unit.contains("KillMode=mixed\n"));
        assert!(unit.contains("WantedBy=default.target\n"));
    }
}
