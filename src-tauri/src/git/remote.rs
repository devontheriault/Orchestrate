//! Keeping a Project in step with its remote, where it has one (ADR 0011).
//! The remote is where work is meant to end up, and what Agents on other Hosts
//! start from, so a Spawn fetches before it picks its Base and a Merge fetches
//! before it merges — and pushes after, when the user asks (ADR 0014).
//!
//! A branch with no upstream — a Project with no remote, or a branch never
//! pushed — is left entirely alone: everything here answers "nothing to do".
//! And an unreachable remote never stops the user: a Spawn starts from what the
//! Host already has and says so, and a Merge lands locally, recorded as not yet
//! pushed.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use super::{is_ancestor, rev_parse, stdout};
use crate::error::{Error, Result};
use crate::process::command;

/// Longer than any fetch or push of a healthy remote; short enough that a
/// Spawn waiting on a dead one gives up while the user is still looking.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

/// A clone brings the whole history, so it gets far longer.
const CLONE_TIMEOUT: Duration = Duration::from_secs(600);

/// Where a new Agent's branch is cut from, and anything about that choice the
/// user should be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    pub commit: String,
    pub note: Option<String>,
}

/// A branch's upstream: the remote it tracks and the branch there.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Upstream {
    remote: String,
    /// The branch on the remote, as `refs/heads/main`.
    merge_ref: String,
    /// How git names its remote-tracking copy, as `origin/main`.
    tracking: String,
}

impl Upstream {
    fn branch(&self) -> &str {
        self.merge_ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&self.merge_ref)
    }
}

/// The upstream `branch` tracks, if it tracks one.
async fn upstream(repo: &Path, branch: &str) -> Option<Upstream> {
    let remote = stdout(repo, &["config", &format!("branch.{branch}.remote")])
        .await
        .ok()?;
    let merge_ref = stdout(repo, &["config", &format!("branch.{branch}.merge")])
        .await
        .ok()?;
    let tracking = stdout(
        repo,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            &format!("{branch}@{{upstream}}"),
        ],
    )
    .await
    .ok()?;
    let remote = remote.trim().to_owned();
    // `.` is a branch tracking another local branch: there is no remote.
    if remote.is_empty() || remote == "." {
        return None;
    }
    Some(Upstream {
        remote,
        merge_ref: merge_ref.trim().to_owned(),
        tracking: tracking.trim().to_owned(),
    })
}

/// Run a git command that talks to a remote. Never prompts — the Host has no
/// one to ask — and gives up after [`NETWORK_TIMEOUT`].
async fn network(repo: &Path, args: &[&str]) -> std::result::Result<(), String> {
    network_for(repo, args, NETWORK_TIMEOUT).await
}

async fn network_for(
    repo: &Path,
    args: &[&str],
    timeout: Duration,
) -> std::result::Result<(), String> {
    let mut cmd = command("git");
    cmd.arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| "the remote didn't answer in time".to_string())?
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // git's last line is the one that says what went wrong.
        let why = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        Err(why.trim().to_owned())
    }
}

/// Where `repo`'s remote is, as git has it, or `None` for a repository with
/// none. The remote its checked-out branch tracks, else `origin`, else
/// whichever there is. It is what makes checkouts of one repository on
/// several Hosts one Project (ADR 0011).
pub async fn remote_url(repo: &Path) -> Option<String> {
    let name = remote_name(repo).await?;
    let url = stdout(repo, &["remote", "get-url", &name]).await.ok()?;
    Some(url.trim().to_owned()).filter(|u| !u.is_empty())
}

/// Which of `repo`'s remotes is its remote, as [`remote_url`] picks it.
async fn remote_name(repo: &Path) -> Option<String> {
    let branch = stdout(repo, &["branch", "--show-current"]).await.ok()?;
    let tracked = match branch.trim() {
        "" => None,
        b => upstream(repo, b).await.map(|u| u.remote),
    };
    let names = stdout(repo, &["remote"]).await.ok()?;
    let names: Vec<&str> = names
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    let name = tracked
        .or_else(|| {
            names
                .iter()
                .find(|n| **n == "origin")
                .map(|n| n.to_string())
        })
        .or_else(|| names.first().map(|n| n.to_string()))?;
    Some(name)
}

/// Clone `url` into `dest`, for a Host that has no checkout of a Project an
/// Agent is being spawned onto.
pub async fn clone(url: &str, dest: &Path) -> Result<()> {
    let parent = dest.parent().unwrap_or(dest);
    std::fs::create_dir_all(parent).map_err(|source| Error::Io {
        path: parent.to_owned(),
        source,
    })?;
    let dest_str = dest.to_string_lossy();
    network_for(parent, &["clone", "--quiet", url, &dest_str], CLONE_TIMEOUT)
        .await
        .map_err(|stderr| Error::Git {
            command: format!("clone {url}"),
            stderr,
        })
}

/// How many commits `rev` has that `other` doesn't.
async fn count_beyond(repo: &Path, rev: &str, other: &str) -> usize {
    stdout(repo, &["rev-list", "--count", &format!("{other}..{rev}")])
        .await
        .ok()
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

fn commits(n: usize) -> String {
    format!("{n} commit{}", if n == 1 { "" } else { "s" })
}

/// Where a new Agent in `repo` starts: the checked-out branch, brought up to
/// date with its upstream where it can be, without ever moving the checkout.
///
/// Fetches first. Then whichever of the local branch and its upstream contains
/// the other is the newer, and wins: a branch someone pushed to from another
/// machine starts the Agent from what they pushed, and unpushed work of the
/// user's own starts it from that. When they have diverged, the local branch
/// wins — the user at this machine may mean the Agent to have their work — and
/// the note says what that leaves out.
pub async fn spawn_start(repo: &Path) -> Result<Start> {
    let local = rev_parse(repo, "HEAD").await?;
    let start = |note: Option<String>| Start {
        commit: local.clone(),
        note,
    };
    let branch = stdout(repo, &["branch", "--show-current"]).await?;
    let branch = branch.trim();
    if branch.is_empty() {
        return Ok(start(None));
    }
    let Some(up) = upstream(repo, branch).await else {
        return Ok(start(None));
    };

    if let Err(why) = network(repo, &["fetch", "--quiet", &up.remote]).await {
        return Ok(start(Some(format!(
            "Couldn't fetch from `{}` ({why}), so this started from `{branch}` as this \
             machine has it.",
            up.remote
        ))));
    }
    let Ok(remote) = rev_parse(repo, &up.tracking).await else {
        return Ok(start(None));
    };
    if remote == local || is_ancestor(repo, &remote, &local).await? {
        return Ok(start(None));
    }
    if is_ancestor(repo, &local, &remote).await? {
        return Ok(Start {
            commit: remote,
            note: None,
        });
    }
    let missing = count_beyond(repo, &up.tracking, &local).await;
    Ok(start(Some(format!(
        "Started from `{branch}` as this machine has it: it has diverged from `{}`, \
         which has {} it doesn't.",
        up.tracking,
        commits(missing)
    ))))
}

/// Bring `target` level with its upstream before a Merge into it. Fetches,
/// then fast-forwards a target that is only behind, and refuses one that has
/// diverged: merging into it would bury the difference in a merge the user
/// never asked for.
///
/// A target checked out in the Project with local changes can't be moved, and
/// is refused as the Merge itself would refuse it. An unreachable remote isn't
/// a refusal — the Merge lands locally and its push says what happened.
pub async fn catch_up(repo: &Path, target: &str) -> Result<()> {
    let Some(up) = upstream(repo, target).await else {
        return Ok(());
    };
    if network(repo, &["fetch", "--quiet", &up.remote])
        .await
        .is_err()
    {
        return Ok(());
    }
    let local = rev_parse(repo, target).await?;
    let Ok(remote) = rev_parse(repo, &up.tracking).await else {
        return Ok(());
    };
    if remote == local || is_ancestor(repo, &remote, &local).await? {
        return Ok(());
    }
    if !is_ancestor(repo, &local, &remote).await? {
        let behind = count_beyond(repo, &up.tracking, &local).await;
        return Err(Error::TargetDiverged {
            target: target.to_owned(),
            upstream: up.tracking,
            behind,
        });
    }

    let checked_out = stdout(repo, &["branch", "--show-current"]).await?;
    if checked_out.trim() == target {
        let dirty = !stdout(repo, &["status", "--porcelain"])
            .await?
            .trim()
            .is_empty();
        if dirty {
            return Err(Error::ProjectDirty {
                branch: target.to_owned(),
            });
        }
        stdout(repo, &["merge", "--ff-only", "--quiet", &up.tracking]).await?;
    } else {
        // Not checked out here, so only the ref moves. Fetching into it from
        // this same repository refuses anything but a fast-forward, and a
        // branch some other worktree has checked out.
        stdout(
            repo,
            &[
                "fetch",
                "--quiet",
                ".",
                &format!("refs/remotes/{}:refs/heads/{target}", up.tracking),
            ],
        )
        .await?;
    }
    Ok(())
}

/// Whether a Merge reached the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pushed {
    /// `target` has no upstream, so there was nowhere to push it.
    NoRemote,
    Yes,
    /// It stays merged locally; this is why it didn't go.
    No(String),
}

/// Whether `branch` tracks a remote, so a Merge into it has somewhere to push.
pub async fn tracks_remote(repo: &Path, branch: &str) -> bool {
    upstream(repo, branch).await.is_some()
}

/// Push `target` to its upstream, after a Merge into it.
pub async fn push(repo: &Path, target: &str) -> Pushed {
    let Some(up) = upstream(repo, target).await else {
        return Pushed::NoRemote;
    };
    let refspec = format!("refs/heads/{target}:refs/heads/{}", up.branch());
    match network(repo, &["push", "--quiet", &up.remote, &refspec]).await {
        Ok(()) => Pushed::Yes,
        Err(why) => Pushed::No(if why.is_empty() {
            "the push was refused".into()
        } else {
            why
        }),
    }
}

/// Where Handoffs travel through the remote: a ref of their own, so taking one
/// down afterwards can never touch a branch the user pushed.
const HANDOFF_PREFIX: &str = "cw/handoff/";

/// The remote branch a Handoff named `id` travels on.
pub fn handoff_branch(id: &str) -> String {
    format!("{HANDOFF_PREFIX}{id}")
}

/// `repo`'s remote, or the error for a Handoff from or to a repository that
/// has none.
async fn handoff_remote(repo: &Path) -> Result<String> {
    remote_name(repo).await.ok_or_else(|| Error::Git {
        command: "remote".into(),
        stderr: "this project has no remote, so its work can only be picked up on this machine"
            .into(),
    })
}

/// Push `branch` to `repo`'s remote as `to`, for another Host to pick the work
/// up from (ADR 0013). Unlike a Merge's push, an unreachable remote is a
/// refusal: there is no other way for the work to get there.
pub async fn publish(repo: &Path, branch: &str, to: &str) -> Result<()> {
    let remote = handoff_remote(repo).await?;
    let refspec = format!("refs/heads/{branch}:refs/heads/{to}");
    network(repo, &["push", "--quiet", &remote, &refspec])
        .await
        .map_err(|stderr| Error::Git {
            command: format!("push {remote} {refspec}"),
            stderr,
        })
}

/// Fetch the branch `from` off `repo`'s remote and return the commit it is at.
pub async fn fetch_published(repo: &Path, from: &str) -> Result<String> {
    let remote = handoff_remote(repo).await?;
    let tracking = format!("refs/remotes/{remote}/{from}");
    let refspec = format!("+refs/heads/{from}:{tracking}");
    network(repo, &["fetch", "--quiet", &remote, &refspec])
        .await
        .map_err(|stderr| Error::Git {
            command: format!("fetch {remote} {from}"),
            stderr,
        })?;
    rev_parse(repo, &tracking).await
}

/// Take a branch [`publish`] put on the remote down again, once it has been
/// fetched. Best-effort: one left behind is clutter, not harm.
pub async fn unpublish(repo: &Path, branch: &str) {
    if let Some(remote) = remote_name(repo).await {
        let _ = network(repo, &["push", "--quiet", "--delete", &remote, branch]).await;
        let _ = stdout(
            repo,
            &[
                "update-ref",
                "-d",
                &format!("refs/remotes/{remote}/{branch}"),
            ],
        )
        .await;
    }
}
