use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Root state directory. Defaults to `dirs::state_dir()/claudewrapper` (usually
/// `~/.local/state/claudewrapper` on Linux), overridable via `CLAUDEWRAPPER_STATE_DIR`
/// for tests and portable installations.
pub fn state_dir() -> Result<PathBuf> {
    if let Ok(override_) = std::env::var("CLAUDEWRAPPER_STATE_DIR") {
        return Ok(PathBuf::from(override_));
    }
    dirs::state_dir()
        .map(|d| d.join("claudewrapper"))
        .ok_or(Error::NoStateDir)
}

pub fn state_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("state.json"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("logs"))
}

pub fn worktrees_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("worktrees"))
}

pub fn agent_log_path(agent_id: &str) -> Result<PathBuf> {
    Ok(logs_dir()?.join(format!("{agent_id}.jsonl")))
}

pub fn agent_meta_path(agent_id: &str) -> Result<PathBuf> {
    Ok(logs_dir()?.join(format!("{agent_id}.meta.json")))
}

/// Create the state, logs, and worktrees directories if they don't exist.
pub fn ensure_dirs() -> Result<()> {
    for dir in [state_dir()?, logs_dir()?, worktrees_dir()?] {
        create_dir_all(&dir)?;
    }
    Ok(())
}

fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}
