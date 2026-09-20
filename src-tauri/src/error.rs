use std::io;
use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("no state directory available (no $HOME?)")]
    NoStateDir,

    #[error("git `{command}` failed: {stderr}")]
    Git { command: String, stderr: String },

    #[error("agent not found: {0}")]
    AgentNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;
