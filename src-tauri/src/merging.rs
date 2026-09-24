//! Merging an Agent's work into the Project, and finishing a Merge that
//! conflicted by way of a Resolver.
//!
//! A conflicting Merge still aborts — the Project is never left half-merged.
//! What the user can do instead is Resolve: spawn a Resolver, a new Agent whose
//! branch is cut from the conflicted Agent's, and have it merge the target in
//! and settle the conflicts inside its own Worktree. When its Turn Completes
//! the app runs the Merge the user already asked for, with the Resolver's
//! branch in place of the conflicted one, and records it on both Agents.

use std::path::Path;

use time::OffsetDateTime;

use crate::domain::{Agent, Resolution};
use crate::error::{Error, Result};
use crate::git::{self, Merged};
use crate::storage;

/// Merge `agent`'s branch into `target` and record the Merge on it.
///
/// Where `target` tracks a remote, it is caught up with it first and pushed
/// after (ADR 0011). A push that fails leaves the Merge in place and records
/// why on the Agent, so it can be pushed again.
///
/// When `agent` is a Resolver, the Agent it resolves is recorded as Merged too
/// — but only if its branch tip really is in `target` now, so a conflicted
/// Agent that kept working while the Resolver ran is not filed as delivered
/// with work the Project has not got. That Agent, if recorded, is returned so
/// the caller can tell the UI.
pub async fn merge(
    project_path: &Path,
    agent: &mut Agent,
    target: &str,
) -> Result<(Merged, Option<Agent>)> {
    git::catch_up(project_path, target).await?;
    let merged = git::merge(
        project_path,
        &agent.worktree_path,
        &agent.branch,
        target,
        &message(agent, target),
    )
    .await?;
    let pushed = git::push(project_path, &merged.target).await;

    let now = OffsetDateTime::now_utc();
    agent.merged_branch = Some(merged.target.clone());
    agent.merged_at = Some(now);
    agent.push_error = push_error(pushed);
    storage::save_agent(agent)?;

    let mut resolved = None;
    if let Some(Resolution { agent_id, .. }) = &agent.resolves {
        if let Ok(mut original) = storage::load_agent(agent_id) {
            let delivered = git::is_ancestor(project_path, &original.branch, &merged.target)
                .await
                .unwrap_or(false);
            if delivered {
                original.merged_branch = Some(merged.target.clone());
                original.merged_at = Some(now);
                storage::save_agent(&original)?;
                resolved = Some(original);
            }
        }
    }
    Ok((merged, resolved))
}

/// Push the branch `agent` was last Merged into, again: the retry for a Merge
/// whose push failed. Returns why it still didn't go, if it didn't.
pub async fn push_again(project_path: &Path, agent: &Agent) -> Result<Option<String>> {
    let target = agent.merged_branch.as_deref().ok_or_else(|| Error::Git {
        command: "push".into(),
        stderr: "this agent hasn't been merged anywhere".into(),
    })?;
    Ok(push_error(git::push(project_path, target).await))
}

fn push_error(pushed: git::Pushed) -> Option<String> {
    match pushed {
        git::Pushed::No(why) => Some(why),
        git::Pushed::Yes | git::Pushed::NoRemote => None,
    }
}

/// Git's own merge subject, with the Agent's name under it so the history says
/// which piece of work merged rather than only which branch.
fn message(agent: &Agent, target: &str) -> String {
    let mut message = format!("Merge branch '{}' into {target}", agent.branch);
    let what = name(agent);
    if !what.is_empty() {
        message.push_str("\n\n");
        message.push_str(&what);
    }
    message
}

/// What to call an Agent in text we write: its Title, else its Task's first line.
fn name(agent: &Agent) -> String {
    agent.title.clone().unwrap_or_else(|| {
        agent
            .task
            .prompt
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned()
    })
}

/// The Task handed to a Resolver. Its first line doubles as the Resolver's name
/// in the sidebar until it has a Title of its own.
pub fn resolver_prompt(conflicted: &Agent, target: &str, files: &[String]) -> String {
    let files = files
        .iter()
        .map(|f| format!("- `{f}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let what = name(conflicted);
    format!(
        "Resolve the merge conflicts between `{branch}` and `{target}`.\n\
         \n\
         This worktree is on a new branch cut from `{branch}`, which holds another \
         agent's work (\"{what}\"). Merging it into `{target}` conflicted in:\n\
         \n\
         {files}\n\
         \n\
         That agent's task was:\n\
         \n\
         > {task}\n\
         \n\
         1. Run `git merge {target}` here.\n\
         2. Resolve every conflict, keeping the intent of both sides: the work on this \
         branch and whatever reached `{target}` since it was cut.\n\
         3. Check the result still builds and its tests pass, if the project has them.\n\
         4. Stage everything and conclude the merge with `git commit --no-edit`.\n\
         \n\
         Stay on this branch: don't rebase, reset, or touch `{target}` or any other \
         branch. When you finish with a clean worktree, the app merges this branch \
         into `{target}`.",
        branch = conflicted.branch,
        task = conflicted.task.prompt.trim().replace('\n', "\n> "),
    )
}

/// Finish the Merge a Resolver was spawned for, once its Turn has Completed.
///
/// Refused — leaving the Resolver Completed for the user to Resume or Merge by
/// hand — when `target` isn't in its branch yet, since that means the Resolver
/// didn't get as far as merging it; otherwise whatever [`merge`] refuses, such
/// as a dirty Worktree or `target` having moved on and conflicting again.
pub async fn finish_resolution(
    resolver: &mut Agent,
    target: &str,
) -> Result<(Merged, Option<Agent>)> {
    let unresolved = |why: String| Error::Unresolved {
        target: target.to_owned(),
        why,
    };
    let reg = storage::Registry::load()?;
    let project_path = reg
        .project(&resolver.project_id)
        .map(|p| p.path.clone())
        .ok_or_else(|| unresolved("its project is no longer registered".into()))?;

    if !git::is_ancestor(&resolver.worktree_path, target, "HEAD").await? {
        return Err(unresolved(format!(
            "`{target}` has not been merged into this branch"
        )));
    }
    merge(&project_path, resolver, target).await
}
