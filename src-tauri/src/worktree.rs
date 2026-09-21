use std::path::Path;

use tokio::process::Command;

use crate::error::{Error, Result};

/// Create a new git worktree at `worktree_path` in Project at `project_path`,
/// branched off `HEAD` as `branch`.
///
/// Runs `git -C <project> worktree add -b <branch> <worktree_path> HEAD`.
pub async fn create(project_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_owned(),
            source,
        })?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(branch)
        .arg(worktree_path)
        .arg("HEAD")
        .output()
        .await
        .map_err(|source| Error::Io {
            path: project_path.to_owned(),
            source,
        })?;

    if !output.status.success() {
        return Err(Error::Git {
            command: format!("worktree add -b {branch} {}", worktree_path.display()),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Check an *existing* branch out into a new worktree at `worktree_path`.
///
/// Unlike [`create`], which cuts a fresh branch for an Agent, this borrows a
/// branch that already exists. It is how a Merge reaches a branch that is
/// not the one checked out: git can only merge into a checked-out branch, and
/// borrowing it here means the user is never moved off their own.
///
/// Fails if the branch is already checked out in another worktree — git allows
/// a branch in only one at a time.
pub async fn add_existing(project_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_owned(),
            source,
        })?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("add")
        .arg(worktree_path)
        .arg(branch)
        .output()
        .await
        .map_err(|source| Error::Io {
            path: project_path.to_owned(),
            source,
        })?;

    if !output.status.success() {
        return Err(Error::Git {
            command: format!("worktree add {} {branch}", worktree_path.display()),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Remove a worktree and its directory, leaving its branch alone.
///
/// This is [`reap`] without the destruction: it is what a Merge does with the
/// throwaway worktree it borrowed a branch into, where deleting the branch
/// would throw away the merge that just happened on it. Best-effort throughout.
pub async fn release(project_path: &Path, worktree_path: &Path) -> Result<()> {
    let _ = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_path)
        .output()
        .await;

    // Fallback: if git didn't clean up, remove the directory ourselves.
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    // Prune stale worktree metadata.
    let _ = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("prune")
        .output()
        .await;

    Ok(())
}

/// Reap a worktree: remove it via git, delete its directory, and delete its
/// branch. Errors from any individual step are swallowed — reap is best-effort.
pub async fn reap(project_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    let _ = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("remove")
        .arg("--force")
        .arg(worktree_path)
        .output()
        .await;

    // Fallback: if git didn't clean up, remove the directory ourselves.
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    let _ = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("branch")
        .arg("-D")
        .arg(branch)
        .output()
        .await;

    // Prune stale worktree metadata.
    let _ = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("prune")
        .output()
        .await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        for args in [
            vec!["init", "--initial-branch=main"],
            vec!["config", "user.email", "t@t.t"],
            vec!["config", "user.name", "t"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(&args)
                .output()
                .await
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        dir
    }

    #[tokio::test]
    async fn create_then_reap() {
        let repo = init_repo().await;
        let wt = TempDir::new().unwrap();
        let wt_path = wt.path().join("agent-abcd");
        let branch = "cw/agent-abcd";

        create(repo.path(), &wt_path, branch).await.unwrap();
        assert!(wt_path.exists(), "worktree dir should exist");
        assert!(
            wt_path.join(".git").exists(),
            "worktree should have a .git file"
        );

        reap(repo.path(), &wt_path, branch).await.unwrap();
        assert!(!wt_path.exists(), "worktree dir should be gone");
    }
}
