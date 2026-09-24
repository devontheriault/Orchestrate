use std::path::Path;

use tokio::process::Command;

use crate::error::{Error, Result};

/// `git -C <project_path>`, ready for its arguments.
fn git(project_path: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(project_path);
    cmd
}

/// Make sure the directory a new worktree goes in exists.
fn create_parent(worktree_path: &Path) -> Result<()> {
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_owned(),
            source,
        })?;
    }
    Ok(())
}

/// Run a `git worktree add`, turning a non-zero exit into [`Error::Git`]
/// described as `command`.
async fn add(mut cmd: Command, project_path: &Path, command: String) -> Result<()> {
    let output = cmd.output().await.map_err(|source| Error::Io {
        path: project_path.to_owned(),
        source,
    })?;

    if !output.status.success() {
        return Err(Error::Git {
            command,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Create a new git worktree at `worktree_path` in Project at `project_path`,
/// branched off `start` as `branch`. An Agent's work starts from `HEAD`; a
/// Resolver's starts from the conflicted Agent's branch, so it has that work to
/// merge.
///
/// Runs `git -C <project> worktree add -b <branch> <worktree_path> <start>`.
pub async fn create(
    project_path: &Path,
    worktree_path: &Path,
    branch: &str,
    start: &str,
) -> Result<()> {
    create_parent(worktree_path)?;
    let mut cmd = git(project_path);
    cmd.args(["worktree", "add", "-b", branch])
        .arg(worktree_path)
        .arg(start);
    let command = format!("worktree add -b {branch} {}", worktree_path.display());
    add(cmd, project_path, command).await
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
    create_parent(worktree_path)?;
    let mut cmd = git(project_path);
    cmd.args(["worktree", "add"]).arg(worktree_path).arg(branch);
    let command = format!("worktree add {} {branch}", worktree_path.display());
    add(cmd, project_path, command).await
}

/// Remove a worktree via git, deleting its directory ourselves if git didn't.
/// Best-effort.
async fn remove(project_path: &Path, worktree_path: &Path) {
    let _ = git(project_path)
        .args(["worktree", "remove", "--force"])
        .arg(worktree_path)
        .output()
        .await;

    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }
}

/// Prune stale worktree metadata. Best-effort.
async fn prune(project_path: &Path) {
    let _ = git(project_path).args(["worktree", "prune"]).output().await;
}

/// Remove a worktree and its directory, leaving its branch alone.
///
/// This is [`discard`] without the destruction: it is what a Merge does with the
/// throwaway worktree it borrowed a branch into, where deleting the branch
/// would throw away the merge that just happened on it. Best-effort throughout.
pub async fn release(project_path: &Path, worktree_path: &Path) -> Result<()> {
    remove(project_path, worktree_path).await;
    prune(project_path).await;
    Ok(())
}

/// Discard a worktree: remove it via git, delete its directory, and delete its
/// branch. Errors from any individual step are swallowed — discard is best-effort.
pub async fn discard(project_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    remove(project_path, worktree_path).await;
    let _ = git(project_path)
        .args(["branch", "-D", branch])
        .output()
        .await;
    prune(project_path).await;
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
    async fn create_then_discard() {
        let repo = init_repo().await;
        let wt = TempDir::new().unwrap();
        let wt_path = wt.path().join("agent-abcd");
        let branch = "cw/agent-abcd";

        create(repo.path(), &wt_path, branch, "HEAD").await.unwrap();
        assert!(wt_path.exists(), "worktree dir should exist");
        assert!(
            wt_path.join(".git").exists(),
            "worktree should have a .git file"
        );

        discard(repo.path(), &wt_path, branch).await.unwrap();
        assert!(!wt_path.exists(), "worktree dir should be gone");
    }
}
