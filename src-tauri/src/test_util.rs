//! Test-only helpers. Shared across all `#[cfg(test)]` modules so that any
//! test touching `CLAUDEWRAPPER_STATE_DIR` serialises against every other one
//! — env vars are process-wide, so a per-module mutex would let tests from
//! different modules clobber each other's tempdir override.

use std::sync::{Mutex, MutexGuard};

use tempfile::TempDir;
use tokio::process::Command;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// Scope a test to a fresh `CLAUDEWRAPPER_STATE_DIR` tempdir, with Claude
/// Code's own config dir inside it so nothing reads the developer's real
/// sessions. Drops the env vars and the lock when it goes out of scope.
pub struct StateEnv {
    _dir: TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl StateEnv {
    pub fn new() -> Self {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        // SAFETY: guarded by ENV_MUTEX for the lifetime of this StateEnv.
        unsafe {
            std::env::set_var("CLAUDEWRAPPER_STATE_DIR", dir.path());
            std::env::set_var("CLAUDE_CONFIG_DIR", dir.path().join("claude"));
        }
        Self {
            _dir: dir,
            _guard: guard,
        }
    }
}

impl Drop for StateEnv {
    fn drop(&mut self) {
        // SAFETY: still holding ENV_MUTEX.
        unsafe {
            std::env::remove_var("CLAUDEWRAPPER_STATE_DIR");
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
    }
}

/// A fresh Git repository with one empty commit on `main`, set up so the
/// developer's global config can't break it.
pub async fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    for args in [
        vec!["init", "--initial-branch=main"],
        vec!["config", "user.email", "t@t.t"],
        vec!["config", "user.name", "t"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "core.hooksPath", "/dev/null"],
        vec!["commit", "--allow-empty", "-m", "init"],
    ] {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(&args)
            .output()
            .await
            .unwrap();
        assert!(out.status.success());
    }
    dir
}

/// Write an executable script, for standing in for `claude`. Returns its path.
pub fn write_script(contents: &str) -> String {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = std::env::temp_dir().join(format!("cw-fake-claude-{}.sh", crate::domain::new_id()));
    let mut f = std::fs::File::create(&tmp).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    let mut perms = std::fs::metadata(&tmp).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&tmp, perms).unwrap();
    tmp.to_string_lossy().into_owned()
}
