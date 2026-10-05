//! What the Host keeps of the user's mail: headers it has listed, and the
//! bodies of messages recently opened. Only ever a cache. The server is the
//! source of truth, so any of it can be deleted at any time, and a folder's
//! cache is thrown away whenever the server says its UIDs were renumbered.
//!
//! It lives under the private `mail` directory, beside the account, since it
//! is the user's mail.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{account, Result, Summary};

/// Bodies kept, most recently opened first.
const BODIES_KEPT: usize = 200;

/// Bigger bodies are fetched afresh each time rather than kept.
const BODY_MAX_BYTES: usize = 10 * 1024 * 1024;

/// Headers kept per folder, newest first.
pub const SUMMARIES_KEPT: usize = 3000;

/// One folder's headers, valid while the server's UIDVALIDITY stays the same.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FolderCache {
    pub validity: u32,
    pub messages: BTreeMap<u32, Summary>,
}

fn root() -> Result<PathBuf> {
    Ok(account::dir()?.join("cache"))
}

/// A folder's name, made safe for a file name and kept short.
fn key(folder: &str) -> String {
    let digest = Sha256::digest(folder.as_bytes());
    crate::domain::hex(&digest[..8])
}

fn folder_file(folder: &str) -> Result<PathBuf> {
    Ok(root()?
        .join("folders")
        .join(format!("{}.json", key(folder))))
}

fn body_file(folder: &str, validity: u32, uid: u32) -> Result<PathBuf> {
    Ok(root()?
        .join("bodies")
        .join(format!("{}-{validity}-{uid}.eml", key(folder))))
}

pub fn load_folder(folder: &str) -> FolderCache {
    folder_file(folder)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Write a folder's headers, keeping the newest. Failing to is not worth
/// failing the call over: the cache only saves fetching them again.
pub fn save_folder(folder: &str, cache: &mut FolderCache) {
    while cache.messages.len() > SUMMARIES_KEPT {
        cache.messages.pop_first();
    }
    let Ok(path) = folder_file(folder) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec(cache) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load_body(folder: &str, validity: u32, uid: u32) -> Option<Vec<u8>> {
    let path = body_file(folder, validity, uid).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    // Touched, so pruning keeps what was read lately.
    let _ = std::fs::File::options()
        .append(true)
        .open(&path)
        .and_then(|f| f.set_modified(std::time::SystemTime::now()));
    Some(bytes)
}

pub fn save_body(folder: &str, validity: u32, uid: u32, bytes: &[u8]) {
    if bytes.len() > BODY_MAX_BYTES {
        return;
    }
    let Ok(path) = body_file(folder, validity, uid) else {
        return;
    };
    let Some(dir) = path.parent() else { return };
    let _ = std::fs::create_dir_all(dir);
    if std::fs::write(&path, bytes).is_ok() {
        prune(dir);
    }
}

/// Keep only the bodies opened most recently.
fn prune(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= BODIES_KEPT {
        return;
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.into_iter().skip(BODIES_KEPT) {
        let _ = std::fs::remove_file(path);
    }
}

/// Throw the whole cache away: on signing out, or on a new account.
pub fn clear() -> Result<()> {
    match std::fs::remove_dir_all(root()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not clear the mail cache: {e}")),
    }
}
