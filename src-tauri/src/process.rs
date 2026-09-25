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
