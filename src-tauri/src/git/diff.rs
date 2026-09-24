//! What an Agent has produced: its Worktree diffed against its Base.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use super::commit::{parse_log, Commit};
use super::{run_with_index, stdout, stdout_with_index};
use crate::error::{Error, Result};

/// Cap on the patch text we hand the frontend. A runaway diff would otherwise
/// pin the webview; past this point the user should read the worktree directly.
pub(super) const MAX_PATCH_BYTES: usize = 512 * 1024;

/// One file changed between the base and the Worktree's current contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    /// Git's single-letter status vs the base: `A`, `M`, `D`, `T`, …
    pub status: String,
    /// `None` for binary files, which git reports as `-`.
    pub insertions: Option<u32>,
    pub deletions: Option<u32>,
}

/// Everything an Agent produced, as of now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorktreeDiff {
    /// The commit the Agent's branch was cut from.
    pub base: String,
    pub files: Vec<ChangedFile>,
    /// Commits on the Agent's branch, oldest first.
    pub commits: Vec<Commit>,
    pub patch: String,
    /// The patch was cut off at [`MAX_PATCH_BYTES`].
    pub truncated: bool,
    /// There are changes not yet in a commit — i.e. [`commit`] has something to
    /// capture, and a Discard right now would destroy work.
    pub uncommitted: bool,
    /// Project branches that already contain the Agent's tip, so a Merge into
    /// them would have nothing left to do. Empty until the work is merged, and
    /// empty again the moment the Agent commits something new.
    pub merged_into: Vec<String>,
}

/// A throwaway copy of a Worktree's index.
///
/// Untracked files are invisible to `git diff` until the index knows they
/// exist, so the diff needs a `git add --intent-to-add` first — but doing that
/// to the real index would be writing to a repository an Agent may still be
/// working in. Copying the index first keeps [`diff`] read-only.
struct ScratchIndex {
    path: std::path::PathBuf,
}

impl ScratchIndex {
    /// Copy the Worktree's index aside. Returns `None` if the index can't be
    /// located or copied, in which case the caller works without one.
    async fn copy_of(worktree_path: &Path) -> Option<Self> {
        let located = stdout(worktree_path, &["rev-parse", "--git-path", "index"])
            .await
            .ok()?;
        let source = worktree_path.join(located.trim());
        if !source.exists() {
            return None;
        }
        let path = std::env::temp_dir().join(format!("cw-index-{}", crate::domain::new_id()));
        std::fs::copy(&source, &path).ok()?;
        Some(Self { path })
    }
}

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Resolve the commit an Agent's branch was cut from.
///
/// Agents spawned since base tracking landed carry the commit on their record.
/// For older Agents we fall back to the merge-base of the Agent's branch and
/// the Project's current HEAD, which is the same commit unless the Project's
/// branch has been rewound since Spawn.
pub async fn resolve_base(
    project_path: Option<&Path>,
    worktree_path: &Path,
    branch: &str,
    recorded: Option<&str>,
) -> Result<String> {
    if let Some(sha) = recorded {
        let verified = stdout(
            worktree_path,
            &["rev-parse", "--verify", &format!("{sha}^{{commit}}")],
        )
        .await?;
        return Ok(verified.trim().to_owned());
    }
    let project_path = project_path.ok_or_else(|| Error::NoBaseCommit {
        branch: branch.to_owned(),
    })?;
    let out = stdout(project_path, &["merge-base", branch, "HEAD"]).await?;
    Ok(out.trim().to_owned())
}

/// Diff an Agent's Worktree against its base. Read-only: untracked files are
/// staged intent-to-add against a [`ScratchIndex`], never the Agent's own.
pub async fn diff(
    project_path: Option<&Path>,
    worktree_path: &Path,
    branch: &str,
    recorded_base: Option<&str>,
) -> Result<WorktreeDiff> {
    if !worktree_path.exists() {
        return Err(Error::WorktreeMissing {
            path: worktree_path.to_owned(),
        });
    }
    let base = resolve_base(project_path, worktree_path, branch, recorded_base).await?;

    let scratch = ScratchIndex::copy_of(worktree_path).await;
    let index = scratch.as_ref().map(|s| s.path.as_path());

    // Best-effort: if this fails, new files are simply missing from the patch.
    let _ = run_with_index(worktree_path, &["add", "-A", "--intent-to-add"], index).await;

    let numstat = stdout_with_index(
        worktree_path,
        &["diff", "--numstat", "-z", "--no-renames", &base],
        index,
    )
    .await?;
    let name_status = stdout_with_index(
        worktree_path,
        &["diff", "--name-status", "-z", "--no-renames", &base],
        index,
    )
    .await?;
    let patch = stdout_with_index(worktree_path, &["diff", "--no-renames", &base], index).await?;
    let log = stdout(
        worktree_path,
        &["log", "--format=%h%x1f%s", &format!("{base}..HEAD")],
    )
    .await?;
    let status = stdout_with_index(worktree_path, &["status", "--porcelain", "-z"], index).await?;

    let statuses = parse_name_status(&name_status);
    let files = parse_numstat(&numstat)
        .into_iter()
        .map(|(path, insertions, deletions)| ChangedFile {
            status: statuses
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "M".to_owned()),
            path,
            insertions,
            deletions,
        })
        .collect();

    let (patch, truncated) = truncate_patch(patch);

    Ok(WorktreeDiff {
        base,
        files,
        commits: parse_log(&log),
        patch,
        truncated,
        uncommitted: status.split('\0').any(|f| !f.trim().is_empty()),
        // Best-effort here: if git can't say, the picker simply keeps offering
        // a branch that has nothing to take, which [`merge`] itself refuses.
        merged_into: branches_containing_head(worktree_path)
            .await
            .unwrap_or_default(),
    })
}

/// Whether a Worktree holds work the Project does not have — something
/// uncommitted, or commits no Project branch contains.
///
/// This is what tells a Merged Agent that is done from one that has been
/// Resumed and produced something since. Cheap enough to ask about every
/// Merged Agent, unlike [`diff`]: it builds no patch and copies no index,
/// because `git status` reports untracked files on its own and only `git diff`
/// needs the intent-to-add.
///
/// A Worktree that has gone missing, or a git that fails, reads as holding
/// nothing: the Agent then keeps the reading its merge record gives it rather
/// than a sidebar row claiming work that may not exist.
pub async fn holds_unmerged_work(worktree_path: &Path) -> bool {
    if !worktree_path.exists() {
        return false;
    }
    let Ok(status) = stdout(worktree_path, &["status", "--porcelain", "-z"]).await else {
        return false;
    };
    if status.split('\0').any(|f| !f.trim().is_empty()) {
        return true;
    }
    // No branch having the tip means the Agent has committed since it merged.
    // An `Err` is git declining to answer, which is not the same thing.
    matches!(branches_containing_head(worktree_path).await, Ok(b) if b.is_empty())
}

/// The Project branches that already have the Worktree's tip in their history.
///
/// Agent branches are left out, exactly as in [`branches`], so this lines up
/// with the branches the Merge picker offers. `Err` is git failing to answer,
/// which callers must keep apart from no branch having the tip — the second
/// means the Agent holds unmerged commits, the first means we don't know.
async fn branches_containing_head(worktree_path: &Path) -> Result<Vec<String>> {
    let listed = stdout(
        worktree_path,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "--contains",
            "HEAD",
            "refs/heads",
        ],
    )
    .await?;

    Ok(listed
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .filter(|n| !n.starts_with(crate::domain::AGENT_BRANCH_PREFIX))
        .map(str::to_owned)
        .collect())
}

/// Parse `git diff --numstat -z`: NUL-separated `insertions\tdeletions\tpath`
/// records, where a binary file reports `-` for both counts.
pub(super) fn parse_numstat(out: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
    out.split('\0')
        .filter(|rec| !rec.is_empty())
        .filter_map(|rec| {
            let mut parts = rec.splitn(3, '\t');
            let insertions = parts.next()?;
            let deletions = parts.next()?;
            let path = parts.next()?;
            Some((
                path.to_owned(),
                insertions.parse().ok(),
                deletions.parse().ok(),
            ))
        })
        .collect()
}

/// Parse `git diff --name-status -z`: NUL-separated fields alternating between
/// a status letter and the path it applies to.
pub(super) fn parse_name_status(out: &str) -> HashMap<String, String> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut map = HashMap::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        map.insert(path.to_owned(), status.to_owned());
    }
    map
}

/// Cut an over-long patch at the last line break that fits.
pub(super) fn truncate_patch(patch: String) -> (String, bool) {
    if patch.len() <= MAX_PATCH_BYTES {
        return (patch, false);
    }
    let mut cut = MAX_PATCH_BYTES;
    while cut > 0 && !patch.is_char_boundary(cut) {
        cut -= 1;
    }
    let cut = patch[..cut].rfind('\n').map_or(cut, |i| i + 1);
    (patch[..cut].to_owned(), true)
}
