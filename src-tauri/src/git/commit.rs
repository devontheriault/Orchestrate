//! Committing the work in an Agent's Worktree onto its own branch.

use std::path::Path;

use serde::Serialize;

use super::stdout;
use crate::error::{Error, Result};

/// A commit the Agent made on its own branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

/// Stage everything in the Worktree and commit it. Project hooks run as usual —
/// a rejecting pre-commit hook surfaces as [`Error::Git`] with its stderr.
pub async fn commit(worktree_path: &Path, message: &str) -> Result<Commit> {
    if message.trim().is_empty() {
        return Err(Error::EmptyCommitMessage);
    }
    if !worktree_path.exists() {
        return Err(Error::WorktreeMissing {
            path: worktree_path.to_owned(),
        });
    }

    stdout(worktree_path, &["add", "-A"]).await?;

    let staged = stdout(worktree_path, &["status", "--porcelain", "-z"]).await?;
    if !staged.split('\0').any(|f| !f.trim().is_empty()) {
        return Err(Error::NothingToCommit {
            path: worktree_path.to_owned(),
        });
    }

    stdout(worktree_path, &["commit", "-m", message]).await?;

    let out = stdout(worktree_path, &["log", "-1", "--format=%h%x1f%s"]).await?;
    parse_log(&out).pop().ok_or_else(|| Error::Git {
        command: "log -1".to_owned(),
        stderr: "commit succeeded but could not be read back".to_owned(),
    })
}

/// Parse `git log --format=%h%x1f%s` into oldest-first order.
pub(super) fn parse_log(out: &str) -> Vec<Commit> {
    let mut commits: Vec<Commit> = out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let (sha, subject) = line.split_once('\u{1f}')?;
            Some(Commit {
                sha: sha.to_owned(),
                subject: subject.to_owned(),
            })
        })
        .collect();
    commits.reverse();
    commits
}
