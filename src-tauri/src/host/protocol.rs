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
pub const PROTOCOL: u32 = 2;

/// Who the Host is, sent as the first line of every connection.
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
}

impl Hello {
    pub fn new(instance: String) -> Self {
        Self {
            protocol: PROTOCOL,
            version: env!("CARGO_PKG_VERSION").to_string(),
            build: build_id(),
            instance,
        }
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
