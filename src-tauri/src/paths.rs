use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Root state directory. Defaults to `dirs::state_dir()/orchestrate` (usually
/// `~/.local/state/orchestrate` on Linux), overridable via `ORCHESTRATE_STATE_DIR`
/// for tests and portable installations.
pub fn state_dir() -> Result<PathBuf> {
    if let Ok(override_) = std::env::var("ORCHESTRATE_STATE_DIR") {
        return Ok(PathBuf::from(override_));
    }
    let root = dirs::state_dir().ok_or(Error::NoStateDir)?;
    let dir = root.join("orchestrate");
    // Installs from before the app was renamed keep their old directory: its
    // worktrees are recorded by absolute path, in git and in Claude's sessions.
    let legacy = root.join("claudewrapper");
    Ok(if !dir.exists() && legacy.exists() {
        legacy
    } else {
        dir
    })
}

/// Where Claude Code keeps a transcript of every session it runs, whoever
/// started it. Follows `CLAUDE_CONFIG_DIR` the way `claude` does.
pub fn claude_projects_dir() -> Option<PathBuf> {
    let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()?.join(".claude"),
    };
    Some(config.join("projects"))
}

pub fn state_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("state.json"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("logs"))
}

/// Where a Host keeps the checkouts it clones itself, for Projects registered
/// only on other machines.
pub fn checkouts_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("checkouts"))
}

pub fn worktrees_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("worktrees"))
}

/// Where pasted attachments are written — they have no path of their own for
/// `claude` to read them from.
pub fn attachments_dir() -> Result<PathBuf> {
    Ok(state_dir()?.join("attachments"))
}

/// The socket this state directory's Host listens on. Windows find their
/// Host here, so a state directory and its Host are always a pair.
pub fn host_socket() -> Result<PathBuf> {
    Ok(state_dir()?.join("host.sock"))
}

/// Held locked by the running Host, so a second one for the same state
/// directory knows to leave.
pub fn host_lock() -> Result<PathBuf> {
    Ok(state_dir()?.join("host.lock"))
}

/// Where a Host started without systemd writes what it has to say.
pub fn host_log() -> Result<PathBuf> {
    Ok(state_dir()?.join("host.log"))
}

/// The Agents a stopping Host Orphaned, left for the next Host to report.
pub fn orphans_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("orphans.json"))
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
