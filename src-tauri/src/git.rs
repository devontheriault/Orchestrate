//! Inspection of an Agent's Worktree, plus the two writes the user can make
//! from the app: committing the Agent's work, and merging it into a branch of
//! the Project. Neither ever happens on an Agent's own initiative.
//!
//! Everything here is expressed relative to the Agent's *base* — the commit its
//! branch was cut from at Spawn. That makes one diff answer the question the
//! user actually has after an Agent Completes: what did this Agent do? Work the
//! Agent committed itself and work it left dirty both show up in the same view.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use tokio::process::Command;

use crate::error::{Error, Result};
use crate::paths;

/// Cap on the patch text we hand the frontend. A runaway diff would otherwise
/// pin the webview; past this point the user should read the worktree directly.
const MAX_PATCH_BYTES: usize = 512 * 1024;

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

/// A commit the Agent made on its own branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
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
    /// capture, and a Reap right now would destroy work.
    pub uncommitted: bool,
    /// Project branches that already contain the Agent's tip, so a Merge into
    /// them would have nothing left to do. Empty until the work is merged, and
    /// empty again the moment the Agent commits something new.
    pub merged_into: Vec<String>,
}

/// Run git in `dir`, optionally against a scratch index instead of the
/// repository's own. See [`ScratchIndex`].
async fn run_with_index(
    dir: &Path,
    args: &[&str],
    index: Option<&Path>,
) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
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
        let path = std::env::temp_dir().join(format!("cw-index-{}", crate::model::new_id()));
        std::fs::copy(&source, &path).ok()?;
        Some(Self { path })
    }
}

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The commit a repository's HEAD currently points at.
pub async fn head_commit(repo: &Path) -> Result<String> {
    let out = stdout(repo, &["rev-parse", "HEAD"]).await?;
    Ok(out.trim().to_owned())
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
        merged_into: branches_containing_head(worktree_path).await,
    })
}

/// The Project branches that already have the Worktree's tip in their history.
///
/// Agent branches are left out, exactly as in [`branches`], so this lines up
/// with the branches the Merge picker offers. Best-effort: if git fails here
/// the picker simply keeps offering a branch that has nothing to take, which
/// [`merge`] itself still refuses.
async fn branches_containing_head(worktree_path: &Path) -> Vec<String> {
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
    .await
    .unwrap_or_default();

    listed
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .filter(|n| !n.starts_with(crate::model::AGENT_BRANCH_PREFIX))
        .map(str::to_owned)
        .collect()
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

/// Parse `git diff --numstat -z`: NUL-separated `insertions\tdeletions\tpath`
/// records, where a binary file reports `-` for both counts.
fn parse_numstat(out: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
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
fn parse_name_status(out: &str) -> HashMap<String, String> {
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut map = HashMap::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        map.insert(path.to_owned(), status.to_owned());
    }
    map
}

/// Parse `git log --format=%h%x1f%s` into oldest-first order.
fn parse_log(out: &str) -> Vec<Commit> {
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

/// Cut an over-long patch at the last line break that fits.
fn truncate_patch(patch: String) -> (String, bool) {
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
        .filter(|n| !n.starts_with(crate::model::AGENT_BRANCH_PREFIX))
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
    let ancestor = run_with_index(
        project_path,
        &["merge-base", "--is-ancestor", agent_branch, target],
        None,
    )
    .await?;
    if ancestor.status.success() {
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
    let borrowed = paths::worktrees_dir()?.join(format!("merge-{}", crate::model::new_id()));
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
        let files: Vec<&str> = conflicted
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
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
            files: files.join(", "),
        });
    }

    let sha = stdout(dir, &["rev-parse", "HEAD"]).await?;
    Ok(Merged {
        target: target.to_owned(),
        sha: sha.trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A Project repo with one commit, plus an Agent worktree branched off it.
    struct Fixture {
        repo: TempDir,
        wt: TempDir,
    }

    impl Fixture {
        const BRANCH: &'static str = "cw/agent-abcd1234";

        async fn new() -> Self {
            let repo = TempDir::new().unwrap();
            for args in [
                vec!["init", "--initial-branch=main"],
                vec!["config", "user.email", "t@t.t"],
                vec!["config", "user.name", "t"],
                // Don't let the developer's global config break the fixture.
                vec!["config", "commit.gpgsign", "false"],
                vec!["config", "core.hooksPath", "/dev/null"],
            ] {
                stdout(repo.path(), &args).await.unwrap();
            }
            std::fs::write(repo.path().join("tracked.txt"), "base\n").unwrap();
            stdout(repo.path(), &["add", "-A"]).await.unwrap();
            stdout(repo.path(), &["commit", "-m", "init"])
                .await
                .unwrap();

            let wt = TempDir::new().unwrap();
            let wt_path = wt.path().join("agent");
            crate::worktree::create(repo.path(), &wt_path, Self::BRANCH)
                .await
                .unwrap();
            Self { repo, wt }
        }

        fn wt_path(&self) -> std::path::PathBuf {
            self.wt.path().join("agent")
        }

        /// The commit the worktree was cut from: main's tip at fixture time.
        async fn base(&self) -> String {
            head_commit(self.repo.path()).await.unwrap()
        }

        async fn diff(&self) -> WorktreeDiff {
            let base = self.base().await;
            diff(
                Some(self.repo.path()),
                &self.wt_path(),
                Self::BRANCH,
                Some(&base),
            )
            .await
            .unwrap()
        }

        /// Commit inside the worktree as the Agent itself would.
        async fn agent_commits(&self, file: &str, contents: &str, message: &str) {
            std::fs::write(self.wt_path().join(file), contents).unwrap();
            stdout(&self.wt_path(), &["add", "-A"]).await.unwrap();
            stdout(&self.wt_path(), &["commit", "-m", message])
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn diff_covers_committed_and_uncommitted_work() {
        let f = Fixture::new().await;
        f.agent_commits("added.txt", "from the agent\n", "agent: add a file")
            .await;
        // Left dirty, as an Agent that exits without committing would.
        std::fs::write(f.wt_path().join("tracked.txt"), "base\nmore\n").unwrap();
        std::fs::write(f.wt_path().join("untracked.txt"), "never staged\n").unwrap();

        let d = f.diff().await;

        let paths: Vec<&str> = d.files.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.contains(&"added.txt"), "committed work: {paths:?}");
        assert!(paths.contains(&"tracked.txt"), "dirty work: {paths:?}");
        assert!(
            paths.contains(&"untracked.txt"),
            "untracked work must be visible: {paths:?}"
        );

        assert_eq!(d.commits.len(), 1);
        assert_eq!(d.commits[0].subject, "agent: add a file");
        assert!(d.uncommitted, "dirty tree must report uncommitted work");
        assert!(d.patch.contains("+never staged"));
        assert!(!d.truncated);

        let tracked = d.files.iter().find(|c| c.path == "tracked.txt").unwrap();
        assert_eq!(tracked.status, "M");
        assert_eq!(tracked.insertions, Some(1));
        assert_eq!(tracked.deletions, Some(0));
    }

    #[tokio::test]
    async fn clean_worktree_reports_no_uncommitted_work() {
        let f = Fixture::new().await;
        f.agent_commits("added.txt", "all tidy\n", "agent: tidy")
            .await;

        let d = f.diff().await;
        assert!(!d.uncommitted);
        assert_eq!(d.files.len(), 1);
        assert_eq!(d.commits.len(), 1);
    }

    #[tokio::test]
    async fn diff_leaves_the_agents_index_untouched() {
        let f = Fixture::new().await;
        std::fs::write(f.wt_path().join("untracked.txt"), "new\n").unwrap();

        let before = stdout(&f.wt_path(), &["status", "--porcelain"])
            .await
            .unwrap();
        let d = f.diff().await;
        let after = stdout(&f.wt_path(), &["status", "--porcelain"])
            .await
            .unwrap();

        assert!(
            d.files.iter().any(|c| c.path == "untracked.txt"),
            "the untracked file still has to show up in the diff"
        );
        assert_eq!(before.trim(), "?? untracked.txt");
        assert_eq!(
            before, after,
            "diff must not stage anything in the worktree"
        );
    }

    #[tokio::test]
    async fn commit_captures_dirty_and_untracked_work() {
        let f = Fixture::new().await;
        std::fs::write(f.wt_path().join("tracked.txt"), "base\nedited\n").unwrap();
        std::fs::write(f.wt_path().join("brand-new.txt"), "new\n").unwrap();

        let c = commit(&f.wt_path(), "save the agent's work").await.unwrap();
        assert_eq!(c.subject, "save the agent's work");
        assert!(!c.sha.is_empty());

        let d = f.diff().await;
        assert!(!d.uncommitted, "tree should be clean after commit");
        assert_eq!(d.commits.len(), 1);
        assert_eq!(d.commits[0].sha, c.sha);
        // Both files are still in the diff vs base — now as committed work.
        assert_eq!(d.files.len(), 2);
    }

    #[tokio::test]
    async fn commit_refuses_empty_message_and_clean_tree() {
        let f = Fixture::new().await;
        std::fs::write(f.wt_path().join("tracked.txt"), "base\nedited\n").unwrap();

        assert!(matches!(
            commit(&f.wt_path(), "   ").await,
            Err(Error::EmptyCommitMessage)
        ));

        commit(&f.wt_path(), "real commit").await.unwrap();
        assert!(matches!(
            commit(&f.wt_path(), "nothing left").await,
            Err(Error::NothingToCommit { .. })
        ));
    }

    #[tokio::test]
    async fn resolve_base_falls_back_to_merge_base() {
        let f = Fixture::new().await;
        let spawn_head = f.base().await;
        f.agent_commits("added.txt", "work\n", "agent: work").await;
        // The Project moves on after the Agent was spawned.
        std::fs::write(f.repo.path().join("other.txt"), "meanwhile\n").unwrap();
        stdout(f.repo.path(), &["add", "-A"]).await.unwrap();
        stdout(f.repo.path(), &["commit", "-m", "project moves on"])
            .await
            .unwrap();

        let resolved = resolve_base(Some(f.repo.path()), &f.wt_path(), Fixture::BRANCH, None)
            .await
            .unwrap();
        assert_eq!(resolved, spawn_head, "merge-base should be the spawn point");

        // And the diff built on it shows only the Agent's own file.
        let d = diff(Some(f.repo.path()), &f.wt_path(), Fixture::BRANCH, None)
            .await
            .unwrap();
        let paths: Vec<&str> = d.files.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, vec!["added.txt"]);
    }

    #[tokio::test]
    async fn diff_and_commit_report_a_reaped_worktree() {
        let f = Fixture::new().await;
        let path = f.wt_path();
        crate::worktree::reap(f.repo.path(), &path, Fixture::BRANCH)
            .await
            .unwrap();

        assert!(matches!(
            diff(Some(f.repo.path()), &path, Fixture::BRANCH, None).await,
            Err(Error::WorktreeMissing { .. })
        ));
        assert!(matches!(
            commit(&path, "too late").await,
            Err(Error::WorktreeMissing { .. })
        ));
    }

    #[test]
    fn parses_numstat_including_binary_files() {
        let out = "-\t-\tbin.dat\x000\t1\tgone.txt\x003\t4\twith space.txt\x00";
        assert_eq!(
            parse_numstat(out),
            vec![
                ("bin.dat".to_owned(), None, None),
                ("gone.txt".to_owned(), Some(0), Some(1)),
                ("with space.txt".to_owned(), Some(3), Some(4)),
            ]
        );
    }

    #[test]
    fn parses_name_status_pairs() {
        let map = parse_name_status("M\0keep.txt\0D\0gone.txt\0");
        assert_eq!(map.get("keep.txt").unwrap(), "M");
        assert_eq!(map.get("gone.txt").unwrap(), "D");
    }

    #[test]
    fn parses_log_oldest_first() {
        let commits = parse_log("bbb\u{1f}second\naaa\u{1f}first\n");
        assert_eq!(commits[0].subject, "first");
        assert_eq!(commits[1].subject, "second");
    }

    #[test]
    fn truncate_patch_cuts_on_a_line_break() {
        let (short, truncated) = truncate_patch("+a\n+b\n".to_owned());
        assert_eq!(short, "+a\n+b\n");
        assert!(!truncated);

        let long = "+line\n".repeat(MAX_PATCH_BYTES);
        let (cut, truncated) = truncate_patch(long);
        assert!(truncated);
        assert!(cut.len() <= MAX_PATCH_BYTES);
        assert!(cut.ends_with('\n'), "must not cut mid-line");
    }

    // --- Merge -------------------------------------------------------------

    impl Fixture {
        /// Make a branch at the Project's current tip without switching to it.
        async fn branch_at_head(&self, name: &str) {
            stdout(self.repo.path(), &["branch", name]).await.unwrap();
        }

        /// Commit in the Project itself, as the user would.
        async fn user_commits(&self, file: &str, contents: &str, message: &str) {
            std::fs::write(self.repo.path().join(file), contents).unwrap();
            stdout(self.repo.path(), &["add", "-A"]).await.unwrap();
            stdout(self.repo.path(), &["commit", "-m", message])
                .await
                .unwrap();
        }

        async fn tip_of(&self, branch: &str) -> String {
            stdout(self.repo.path(), &["rev-parse", branch])
                .await
                .unwrap()
                .trim()
                .to_owned()
        }

        async fn merge_into_branch(&self, target: &str) -> Result<Merged> {
            merge(
                self.repo.path(),
                &self.wt_path(),
                Self::BRANCH,
                target,
                "merge it",
            )
            .await
        }
    }

    #[tokio::test]
    async fn branches_lead_with_current_and_leave_out_agent_branches() {
        let f = Fixture::new().await;
        f.branch_at_head("zebra").await;
        f.branch_at_head("alpha").await;

        let b = branches(f.repo.path()).await.unwrap();

        assert_eq!(b.current.as_deref(), Some("main"));
        // Current first, the rest alphabetical, the Agent's own branch absent.
        assert_eq!(b.names, vec!["main", "alpha", "zebra"]);
    }

    #[tokio::test]
    async fn merge_in_place_merges_onto_the_checked_out_branch() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.agent_commits("agent.txt", "from the agent\n", "agent work")
            .await;

        let merged = f.merge_into_branch("main").await.unwrap();

        assert_eq!(merged.target, "main");
        assert_eq!(merged.sha, f.tip_of("main").await);
        // The work is in the Project's tree, not just its history.
        assert_eq!(
            std::fs::read_to_string(f.repo.path().join("agent.txt")).unwrap(),
            "from the agent\n"
        );
    }

    #[tokio::test]
    async fn merge_makes_a_merge_commit_even_when_it_could_fast_forward() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.agent_commits("agent.txt", "x\n", "agent work").await;

        f.merge_into_branch("main").await.unwrap();

        // --no-ff: two parents, so the Agent's work stays one identifiable unit.
        let parents = stdout(f.repo.path(), &["rev-list", "--parents", "-n", "1", "HEAD"])
            .await
            .unwrap();
        assert_eq!(
            parents.split_whitespace().count(),
            3,
            "expected commit + 2 parents, got {parents:?}"
        );
    }

    #[tokio::test]
    async fn merging_another_branch_borrows_a_worktree_and_leaves_the_user_put() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.branch_at_head("feature").await;
        f.agent_commits("agent.txt", "x\n", "agent work").await;

        let main_before = f.tip_of("main").await;
        let merged = f.merge_into_branch("feature").await.unwrap();

        assert_eq!(merged.target, "feature");
        assert_eq!(merged.sha, f.tip_of("feature").await);
        // The branch we were on is untouched, and we are still on it.
        assert_eq!(f.tip_of("main").await, main_before);
        assert_eq!(
            branches(f.repo.path()).await.unwrap().current.as_deref(),
            Some("main")
        );
        // The borrowed worktree is handed back, and `feature` is still ours.
        assert!(stdout(f.repo.path(), &["worktree", "list"])
            .await
            .unwrap()
            .matches("merge-")
            .next()
            .is_none());
    }

    #[tokio::test]
    async fn merge_refuses_uncommitted_worktree_work() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.agent_commits("agent.txt", "x\n", "agent work").await;
        // Loose work that a Merge would otherwise leave behind.
        std::fs::write(f.wt_path().join("scratch.txt"), "not committed\n").unwrap();

        let err = f.merge_into_branch("main").await.unwrap_err();

        assert!(
            matches!(err, Error::WorktreeDirty { .. }),
            "expected WorktreeDirty, got {err:?}"
        );
    }

    #[tokio::test]
    async fn merge_in_place_refuses_a_dirty_project() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.agent_commits("agent.txt", "x\n", "agent work").await;
        std::fs::write(f.repo.path().join("tracked.txt"), "my own edit\n").unwrap();

        let err = f.merge_into_branch("main").await.unwrap_err();

        assert!(
            matches!(err, Error::ProjectDirty { .. }),
            "expected ProjectDirty, got {err:?}"
        );
    }

    #[tokio::test]
    async fn a_dirty_project_does_not_block_merging_into_another_branch() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.branch_at_head("feature").await;
        f.agent_commits("agent.txt", "x\n", "agent work").await;
        // Uncommitted work of the user's own, irrelevant to a merge happening
        // in a worktree of ours.
        std::fs::write(f.repo.path().join("tracked.txt"), "my own edit\n").unwrap();

        f.merge_into_branch("feature").await.unwrap();

        assert_eq!(
            std::fs::read_to_string(f.repo.path().join("tracked.txt")).unwrap(),
            "my own edit\n",
            "the user's uncommitted work must survive"
        );
    }

    #[tokio::test]
    async fn merge_refuses_when_there_is_nothing_to_merge() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        // The Agent committed nothing, so its branch is still an ancestor.

        let err = f.merge_into_branch("main").await.unwrap_err();

        assert!(
            matches!(err, Error::NothingToMerge { .. }),
            "expected NothingToMerge, got {err:?}"
        );
    }

    #[tokio::test]
    async fn diff_reports_the_branches_that_already_have_the_work() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.agent_commits("agent.txt", "from the agent\n", "agent work")
            .await;

        // Unmerged work belongs to nobody but the Agent's own branch.
        assert!(f.diff().await.merged_into.is_empty());

        f.merge_into_branch("main").await.unwrap();
        assert_eq!(f.diff().await.merged_into, vec!["main"]);

        // A Merge is a record, not a state: new commits are unmerged again.
        f.agent_commits("agent.txt", "second thoughts\n", "more agent work")
            .await;
        assert!(f.diff().await.merged_into.is_empty());
    }

    #[tokio::test]
    async fn merge_aborts_on_conflict_and_leaves_the_project_byte_identical() {
        let _env = crate::test_util::StateEnv::new();
        let f = Fixture::new().await;
        f.agent_commits("tracked.txt", "the agent's line\n", "agent edit")
            .await;
        f.user_commits("tracked.txt", "my line\n", "my edit").await;

        let before = f.tip_of("main").await;
        let err = f.merge_into_branch("main").await.unwrap_err();

        match err {
            Error::MergeConflict { target, files } => {
                assert_eq!(target, "main");
                assert!(files.contains("tracked.txt"), "got {files:?}");
            }
            other => panic!("expected MergeConflict, got {other:?}"),
        }

        // Byte-identical: same tip, same contents, no merge left in progress.
        assert_eq!(f.tip_of("main").await, before);
        assert_eq!(
            std::fs::read_to_string(f.repo.path().join("tracked.txt")).unwrap(),
            "my line\n"
        );
        assert!(stdout(f.repo.path(), &["status", "--porcelain"])
            .await
            .unwrap()
            .trim()
            .is_empty());
    }
}
