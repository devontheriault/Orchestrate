//! Starting the programs the app runs: `git`, `claude`, `tailscale`.

use std::ffi::OsStr;

use tokio::process::Command;

/// A [`Command`] for `program`. On Windows the program gets no console window.
/// Neither the window nor the Host has a console, so without this Windows gives
/// each console program it starts a window of its own, which flashes open and
/// shut.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Take back what an AppImage put into `cmd`'s environment. The AppImage's
/// launcher points `LD_LIBRARY_PATH`, `PYTHONHOME` and the like into its own
/// mount for the app's sake, and a program started from it inherits them:
/// the system's `git` then loads the AppImage's libraries and can't fetch over
/// https. Does nothing outside an AppImage.
pub fn outside_appimage(cmd: &mut Command) {
    let Some(appdir) = std::env::var("APPDIR").ok().filter(|a| !a.is_empty()) else {
        return;
    };
    for (key, value) in scrubbed(std::env::vars(), &appdir) {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
}

/// Every variable that mentions `appdir`, with the entries under it dropped:
/// `None` where nothing is left of it.
fn scrubbed(
    vars: impl IntoIterator<Item = (String, String)>,
    appdir: &str,
) -> Vec<(String, Option<String>)> {
    vars.into_iter()
        .filter(|(_, v)| v.contains(appdir))
        .map(|(k, v)| {
            let kept: Vec<&str> = v
                .split(':')
                .filter(|entry| !entry.is_empty() && !entry.starts_with(appdir))
                .collect();
            (k, (!kept.is_empty()).then(|| kept.join(":")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_appimages_entries_come_out_and_the_rest_stay() {
        let vars = [
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_x/usr/lib/:/tmp/.mount_x/lib/:",
            ),
            ("PATH", "/tmp/.mount_x/usr/bin/:/usr/bin:/home/u/.local/bin"),
            ("HOME", "/home/u"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let mut got = scrubbed(vars, "/tmp/.mount_x");
        got.sort();
        assert_eq!(
            got,
            vec![
                ("LD_LIBRARY_PATH".to_string(), None),
                (
                    "PATH".to_string(),
                    Some("/usr/bin:/home/u/.local/bin".to_string())
                ),
            ]
        );
    }
}
