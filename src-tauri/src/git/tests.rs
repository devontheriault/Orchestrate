
use super::commit::parse_log;
use super::diff::{parse_name_status, parse_numstat, truncate_patch, MAX_PATCH_BYTES};
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
        crate::worktree::create(repo.path(), &wt_path, Self::BRANCH, "HEAD")
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
async fn diff_and_commit_report_a_discarded_worktree() {
    let f = Fixture::new().await;
    let path = f.wt_path();
    crate::worktree::discard(f.repo.path(), &path, Fixture::BRANCH)
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

/// The state a Merged Agent is done in: everything it produced is on main,
/// and it has written nothing since. This is the only case that stays in
/// the Delivered bucket.
#[tokio::test]
async fn merged_and_quiet_worktree_holds_nothing() {
    let _env = crate::test_util::StateEnv::new();
    let f = Fixture::new().await;
    f.agent_commits("agent.txt", "from the agent\n", "agent work")
        .await;
    f.merge_into_branch("main").await.unwrap();

    assert!(!holds_unmerged_work(&f.wt_path()).await);
}

#[tokio::test]
async fn work_left_dirty_after_a_merge_is_held() {
    let _env = crate::test_util::StateEnv::new();
    let f = Fixture::new().await;
    f.agent_commits("agent.txt", "from the agent\n", "agent work")
        .await;
    f.merge_into_branch("main").await.unwrap();

    // The Agent was Resumed and wrote something it never committed.
    std::fs::write(f.wt_path().join("later.txt"), "second thoughts\n").unwrap();

    assert!(holds_unmerged_work(&f.wt_path()).await);
}

/// The case `git status` alone misses: a clean tree whose tip no Project
/// branch has, because the Agent committed again after the Merge.
#[tokio::test]
async fn committing_again_after_a_merge_is_held() {
    let _env = crate::test_util::StateEnv::new();
    let f = Fixture::new().await;
    f.agent_commits("agent.txt", "from the agent\n", "agent work")
        .await;
    f.merge_into_branch("main").await.unwrap();
    f.agent_commits("later.txt", "second thoughts\n", "agent: more work")
        .await;

    assert!(holds_unmerged_work(&f.wt_path()).await);
}

/// A Worktree that isn't there any more can't be claiming to hold work:
/// the row keeps whatever its merge record says rather than erroring.
#[tokio::test]
async fn missing_worktree_holds_nothing() {
    let f = Fixture::new().await;

    assert!(!holds_unmerged_work(&f.wt.path().join("gone")).await);
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
            assert_eq!(files, ["tracked.txt"]);
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

/// The files `git ls-files` reports tracked in `repo`.
async fn tracked(repo: &Path) -> Vec<String> {
    stdout(repo, &["ls-files"])
        .await
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn set_up_commits_a_plain_folder_so_an_agent_can_branch_from_it() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.py"), "print('hi')\n").unwrap();
    std::fs::write(dir.path().join(".env"), "SECRET=1\n").unwrap();
    std::fs::create_dir(dir.path().join("node_modules")).unwrap();
    std::fs::write(dir.path().join("node_modules/dep.js"), "\n").unwrap();
    assert!(!has_commits(dir.path()).await);

    set_up(dir.path()).await.unwrap();

    assert!(has_commits(dir.path()).await);
    assert_eq!(tracked(dir.path()).await, [".gitignore", "main.py"]);

    // The point of it all: an Agent's Worktree starts with the user's files.
    let wt = TempDir::new().unwrap();
    let wt_path = wt.path().join("agent");
    crate::worktree::create(dir.path(), &wt_path, "cw/agent-abcd1234", "HEAD")
        .await
        .unwrap();
    assert!(wt_path.join("main.py").exists());
}

#[tokio::test]
async fn set_up_keeps_an_existing_gitignore() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join(".gitignore"), "secret.txt\n").unwrap();
    std::fs::write(dir.path().join("secret.txt"), "shh\n").unwrap();
    std::fs::write(dir.path().join(".env"), "mine to commit\n").unwrap();

    set_up(dir.path()).await.unwrap();

    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "secret.txt\n"
    );
    assert_eq!(tracked(dir.path()).await, [".env", ".gitignore"]);
}

#[tokio::test]
async fn set_up_finishes_a_repository_with_no_commits() {
    let dir = TempDir::new().unwrap();
    stdout(dir.path(), &["init", "--initial-branch=trunk"])
        .await
        .unwrap();
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    assert!(!has_commits(dir.path()).await);

    set_up(dir.path()).await.unwrap();

    assert!(has_commits(dir.path()).await);
    assert_eq!(tracked(dir.path()).await, [".gitignore", "a.txt"]);
    // The repository the user made is kept, not re-created.
    let head = stdout(dir.path(), &["symbolic-ref", "--short", "HEAD"])
        .await
        .unwrap();
    assert_eq!(head.trim(), "trunk");
}

#[tokio::test]
async fn set_up_handles_an_empty_folder() {
    let dir = TempDir::new().unwrap();
    set_up(dir.path()).await.unwrap();
    assert!(has_commits(dir.path()).await);
}
