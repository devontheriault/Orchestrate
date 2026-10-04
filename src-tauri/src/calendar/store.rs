//! The Calendar Space's files. The two holding secrets — the accounts and the
//! Google OAuth client — are made readable by the user alone, and written
//! whole to a temporary file first, so a crash mid-write can't leave half of
//! one, or leave it readable by others for a moment.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Account, AccountCache};

fn accounts_path(dir: &Path) -> PathBuf {
    dir.join("accounts.json")
}

pub fn google_client_path(dir: &Path) -> PathBuf {
    dir.join("google-client.json")
}

fn cache_path(dir: &Path, account: &str) -> PathBuf {
    dir.join("cache").join(format!("{account}.json"))
}

/// Write `contents` to `path`, readable only by the user when `secret`.
fn write(path: &Path, contents: &[u8], secret: bool) -> Result<(), String> {
    let parent = path.parent().ok_or("no directory to write into")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let _ = secret;
    let mut file = options
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    // A file left from before with wider permissions keeps them through
    // `mode`, which only applies on creation.
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
    }
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Serialize, Deserialize, Default)]
struct AccountsFile {
    accounts: Vec<Account>,
}

pub fn load_accounts(dir: &Path) -> Result<Vec<Account>, String> {
    match std::fs::read_to_string(accounts_path(dir)) {
        Ok(text) => serde_json::from_str::<AccountsFile>(&text)
            .map(|f| f.accounts)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save_accounts(dir: &Path, accounts: &[Account]) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(&AccountsFile {
        accounts: accounts.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    write(&accounts_path(dir), &text, true)
}

/// A cache that's missing or unreadable is an empty one: the next sync
/// fetches everything again.
pub fn load_cache(dir: &Path, account: &str) -> AccountCache {
    std::fs::read_to_string(cache_path(dir, account))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_cache(dir: &Path, account: &str, cache: &AccountCache) -> Result<(), String> {
    let text = serde_json::to_vec(cache).map_err(|e| e.to_string())?;
    // Calendar events can name people and places: no wider than the secrets.
    write(&cache_path(dir, account), &text, true)
}

pub fn remove_cache(dir: &Path, account: &str) {
    let _ = std::fs::remove_file(cache_path(dir, account));
}

/// The OAuth client the user made in their own Google Cloud project, which
/// is what the app signs in to Google as.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoogleClient {
    pub client_id: String,
    pub client_secret: String,
}

/// The client, from the file the app writes or the one Google's console
/// downloads (`{"installed": {...}}`), saved under the same name.
pub fn load_google_client(dir: &Path) -> Option<GoogleClient> {
    #[derive(Deserialize)]
    struct Downloaded {
        installed: GoogleClient,
    }
    let text = std::fs::read_to_string(google_client_path(dir)).ok()?;
    serde_json::from_str::<GoogleClient>(&text)
        .or_else(|_| serde_json::from_str::<Downloaded>(&text).map(|d| d.installed))
        .ok()
        .filter(|c| !c.client_id.is_empty() && !c.client_secret.is_empty())
}

pub fn save_google_client(dir: &Path, client: &GoogleClient) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(client).map_err(|e| e.to_string())?;
    write(&google_client_path(dir), &text, true)
}
