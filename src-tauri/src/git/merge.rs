//! Merging an Agent's branch into one of the Project's own.

use std::path::Path;

use serde::Serialize;

use super::{is_ancestor, run_with_index, stdout};
use crate::error::{Error, Result};
use crate::paths;

/// The branches a Merge can target in a Project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Branches {
    /// The Project's checked-out branch, or `None` on a detached HEAD. Merging
    /// into this one merges in place; anything else borrows a worktree.
    pub current: Option<String>,
    /// Local branches worth merging into: the current one first, then the rest
    /// alphabetically, with every Agent branch left out.
    pub names: Vec<String>,
}

/// A completed Merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Merged {
    pub target: String,
    /// The merge commit now at the tip of `target`.
    pub sha: String,
}

/// The Project's local branches, as the Merge picker offers them.
pub async fn branches(project_path: &Path) -> Result<Branches> {
    let current = stdout(project_path, &["branch", "--show-current"]).await?;
    let current = match current.trim() {
        "" => None,
        name => Some(name.to_owned()),
    };

    let listed = stdout(
        project_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;

    let mut names: Vec<String> = listed
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .filter(|n| !n.starts_with(crate::domain::AGENT_BRANCH_PREFIX))
        .map(str::to_owned)
        .collect();
    names.sort();

    // The branch you are on is the one you usually mean, so it leads.
    if let Some(cur) = &current {
        if let Some(i) = names.iter().position(|n| n == cur) {
            let cur = names.remove(i);
            names.insert(0, cur);
        }
    }

    Ok(Branches { current, names })
}

/// Merge `agent_branch` into `target`, with `--no-ff` so the Agent's work stays
/// one identifiable unit in history.
///
/// When `target` is not the Project's checked-out branch the merge runs in a
/// throwaway worktree, so a Merge never moves the user off their own branch.
/// When it is, the merge runs in place and the Project's tree must be clean.
///
/// A conflict aborts: the Project is left byte-identical to how it started and
/// the colliding paths are reported. This app has no merge tool, so a
/// half-merged tree is somewhere it could not get the Project back out of.
pub async fn merge(
    project_path: &Path,
    worktree_path: &Path,
    agent_branch: &str,
    target: &str,
    message: &str,
) -> Result<Merged> {
    if !worktree_path.exists() {
        return Err(Error::WorktreeMissing {
            path: worktree_path.to_owned(),
        });
    }

    // Only committed work merges. Anything still loose in the Worktree would be
    // silently left behind, so say so instead of merging half the work.
    if !stdout(worktree_path, &["status", "--porcelain"])
        .await?
        .trim()
        .is_empty()
    {
        return Err(Error::WorktreeDirty {
            path: worktree_path.to_owned(),
        });
    }

    // Already an ancestor means there is nothing to merge; `git merge` would
    // report "Already up to date" and exit zero, which reads as a success that
    // did nothing.
    if is_ancestor(project_path, agent_branch, target).await? {
        return Err(Error::NothingToMerge {
            branch: agent_branch.to_owned(),
            target: target.to_owned(),
        });
    }

    let current = stdout(project_path, &["branch", "--show-current"]).await?;
    let in_place = current.trim() == target;

    if in_place {
        if !stdout(project_path, &["status", "--porcelain"])
            .await?
            .trim()
            .is_empty()
        {
            return Err(Error::ProjectDirty {
                branch: target.to_owned(),
            });
        }
        return merge_into(project_path, agent_branch, target, message).await;
    }

    // Borrow the target branch into a worktree of our own, merge there, and
    // hand it back. The user's checkout is never involved.
    let borrowed = paths::worktrees_dir()?.join(format!("merge-{}", crate::domain::new_id()));
    crate::worktree::add_existing(project_path, &borrowed, target).await?;
    let merged = merge_into(&borrowed, agent_branch, target, message).await;
    crate::worktree::release(project_path, &borrowed).await?;
    merged
}

/// Run the merge in whichever worktree has `target` checked out, aborting and
/// reporting the conflicted paths if it doesn't apply cleanly.
async fn merge_into(dir: &Path, agent_branch: &str, target: &str, message: &str) -> Result<Merged> {
    let merge = run_with_index(
        dir,
        &["merge", "--no-ff", "-m", message, agent_branch],
        None,
    )
    .await?;

    if !merge.status.success() {
        // Read the conflicts before the abort clears them.
        let conflicted = stdout(dir, &["diff", "--name-only", "--diff-filter=U"])
            .await
            .unwrap_or_default();
        let files: Vec<String> = conflicted
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();

        let _ = run_with_index(dir, &["merge", "--abort"], None).await;

        if files.is_empty() {
            // Not a conflict — something else went wrong, and the merge never
            // started, so the tree is untouched either way.
            return Err(Error::Git {
                command: format!("merge --no-ff {agent_branch}"),
                stderr: String::from_utf8_lossy(&merge.stderr).trim().to_owned(),
            });
        }
        return Err(Error::MergeConflict {
            target: target.to_owned(),
            files,
        });
    }

    let sha = stdout(dir, &["rev-parse", "HEAD"]).await?;
    Ok(Merged {
        target: target.to_owned(),
        sha: sha.trim().to_owned(),
    })
}
