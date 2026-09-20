//! Test-only helpers. Shared across all `#[cfg(test)]` modules so that any
//! test touching `CLAUDEWRAPPER_STATE_DIR` serialises against every other one
//! — env vars are process-wide, so a per-module mutex would let tests from
//! different modules clobber each other's tempdir override.

use std::sync::{Mutex, MutexGuard};

use tempfile::TempDir;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// Scope a test to a fresh `CLAUDEWRAPPER_STATE_DIR` tempdir. Drops both the
/// env var and the lock when it goes out of scope.
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
        }
    }
}
