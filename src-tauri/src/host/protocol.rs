//! What goes over a connection between a window and its Host: one JSON value
//! per line, each way. Plain JSON rather than anything Tauri knows about, so a
//! phone or a browser can speak it later.
//!
//! The Host opens with a [`Hello`]. After that the window sends [`Request`]s
//! and the Host sends [`Frame::Reply`]s to them, interleaved with the
//! [`Frame::Event`]s every connection hears.

use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped whenever a call or an event changes shape. A window and a Host that
/// disagree on it cannot work together, and the window asks the Host to make
/// way instead (see `shutdown_when_idle`, the one call every version keeps).
pub const PROTOCOL: u32 = 4;

/// Who the Host is, sent as the first line of every connection.
///
/// Every build must be able to read every other build's Hello, or a window
/// can't tell an old Host to make way: give any field added later a default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub protocol: u32,
    /// The app version, for telling the user which side to update.
    pub version: String,
    /// Which build of the binary the Host is running. Differs from the
    /// window's after an update, or after a rebuild during development, even
    /// when the version does not.
    pub build: String,
    /// New each time a Host starts, so a window can tell a reconnection to the
    /// same Host from one to its successor.
    pub instance: String,
    /// The machine's own name, for a window to call this Host by. Empty from a
    /// Host older than protocol 3.
    #[serde(default)]
    pub name: String,
}

impl Hello {
    pub fn new(instance: String) -> Self {
        Self {
            protocol: PROTOCOL,
            version: env!("CARGO_PKG_VERSION").to_string(),
            build: build_id(),
            instance,
            name: machine_name(),
        }
    }
}

/// What this machine calls itself.
pub fn machine_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `buf.len()` bytes into a buffer we own.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_owned();
    if ok && !name.is_empty() {
        name
    } else {
        "this machine".into()
    }
}

/// The running binary's modification time. Two processes started from the
/// same file agree on it; a binary replaced by an update or a rebuild does
/// not. Empty when the binary can't be found, which reads as a mismatch.
pub fn build_id() -> String {
    std::env::current_exe()
        .and_then(|exe| exe.metadata())
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default()
}

/// One call from a window. `call` is `{"method": ..., "args": {...}}`, kept as
/// raw JSON here so a call this Host doesn't know can still be answered.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub call: Value,
}

/// Everything the Host sends after its [`Hello`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    Hello(Hello),
    /// Sent instead of a Hello to a connection the Host won't serve, and then
    /// it hangs up: a peer on the tailnet that isn't the user's own.
    Refused(String),
    /// The answer to the [`Request`] with the same id.
    Reply {
        id: u64,
        #[serde(flatten)]
        outcome: Outcome,
    },
    /// Something happened that every window should hear, named as the webview
    /// knows it (`agent-event`, `agent-state-changed`).
    Event {
        name: String,
        payload: Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok(Value),
    Err(String),
}

impl From<Result<Value, String>> for Outcome {
    fn from(r: Result<Value, String>) -> Self {
        match r {
            Ok(v) => Outcome::Ok(v),
            Err(e) => Outcome::Err(e),
        }
    }
}

impl From<Outcome> for Result<Value, String> {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Ok(v) => Ok(v),
            Outcome::Err(e) => Err(e),
        }
    }
}

/// A frame as one line of the stream, newline included.
pub fn line(frame: &Frame) -> String {
    let mut s = serde_json::to_string(frame).expect("frames always serialize");
    s.push('\n');
    s
}
