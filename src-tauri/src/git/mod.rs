//! Inspection of an Agent's Worktree, plus the writes the user can make from
//! the app: committing the Agent's work, merging it into a branch of the
//! Project, and setting a folder up as a Project in the first place. None ever
//! happens on an Agent's own initiative.
//!
//! Everything here is expressed relative to the Agent's *base* — the commit its
//! branch was cut from at Spawn. That makes one diff answer the question the
//! user actually has after an Agent Completes: what did this Agent do? Work the
//! Agent committed itself and work it left dirty both show up in the same view.
//!
//! Split by what the user is doing: `diff.rs` reads a Worktree, `commit.rs`
//! captures its work, `merge.rs` brings that work into the Project, and
//! `setup.rs` makes a folder something Agents can branch from, and `remote.rs`
//! keeps a Project in step with its remote. What they share —
//! running git and resolving revisions — lives here.

mod commit;
mod diff;
mod merge;
mod remote;
mod setup;

#[cfg(test)]
mod tests;

use std::path::Path;

use crate::error::{Error, Result};
use crate::process::command;

pub use commit::{commit, Commit};
pub use diff::{diff, holds_unmerged_work, resolve_base, ChangedFile, WorktreeDiff};
pub use merge::{branches, merge, Branches, Merged};
pub use remote::{
    catch_up, clone, fetch_published, handoff_branch, publish, push, remote_url, spawn_start,
    unpublish, Pushed, Start,
};
pub use setup::{has_commits, set_up};

/// Run git in `dir`, optionally against a scratch index instead of the
/// repository's own. See `diff::ScratchIndex`.
async fn run_with_index(
    dir: &Path,
    args: &[&str],
    index: Option<&Path>,
) -> Result<std::process::Output> {
    let mut cmd = command("git");
    cmd.arg("-C").arg(dir).args(args);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    cmd.output().await.map_err(|source| Error::Io {
        path: dir.to_owned(),
        source,
    })
}

/// Run git and return stdout, turning a non-zero exit into [`Error::Git`].
async fn stdout(dir: &Path, args: &[&str]) -> Result<String> {
    stdout_with_index(dir, args, None).await
}

async fn stdout_with_index(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<String> {
    let output = run_with_index(dir, args, index).await?;
    if !output.status.success() {
        return Err(Error::Git {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The commit a repository's HEAD currently points at.
pub async fn head_commit(repo: &Path) -> Result<String> {
    rev_parse(repo, "HEAD").await
}

/// The commit `rev` names in `repo` — a branch, `HEAD`, or a sha.
pub async fn rev_parse(repo: &Path, rev: &str) -> Result<String> {
    let out = stdout(
        repo,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )
    .await?;
    Ok(out.trim().to_owned())
}

/// Whether `ancestor` is already in `rev`'s history. `Err` is git failing to
/// answer — an unknown revision, say — which is not the same as "no".
pub async fn is_ancestor(repo: &Path, ancestor: &str, rev: &str) -> Result<bool> {
    let out = run_with_index(repo, &["merge-base", "--is-ancestor", ancestor, rev], None).await?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Error::Git {
            command: format!("merge-base --is-ancestor {ancestor} {rev}"),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        }),
    }
}

/// Whether a Worktree holds anything uncommitted, untracked files included.
pub async fn is_dirty(worktree_path: &Path) -> Result<bool> {
    Ok(!stdout(worktree_path, &["status", "--porcelain"])
        .await?
        .trim()
        .is_empty())
}
