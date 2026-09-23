//! Tauri command surface. Each function is invocable from the frontend via
//! `@tauri-apps/api/core#invoke`. Errors are stringified so the frontend gets
//! a plain `Error` object with a readable message.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use time::OffsetDateTime;

use crate::domain::{new_id, Agent, AgentEvent, AgentState, Project};
use crate::error::Error;
use crate::git::{self, Branches, Commit, Merged, WorktreeDiff};
use crate::models::ModelInfo;
use crate::runtime::AgentRuntime;
use crate::usage::UsageSummary;
use crate::{merging, paths, storage, worktree};

/// App-level shared state, managed by Tauri.
pub struct AppState {
    pub runtime: AgentRuntime,
    /// Ids of the Orphans adopted at launch time, so the UI can surface them
    /// without re-scanning the filesystem. Emptied once the user dismisses the
    /// banner, so a reload of the window doesn't raise it again.
    pub startup_orphans: Mutex<Vec<String>>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    storage::Registry::load().map(|r| r.projects).map_err(err)
}

/// Whether the folder at `path` needs [`git::set_up`] before Agents can work
/// in it. Asked when the user picks a folder, so the trust dialog can say so.
#[tauri::command]
pub async fn project_needs_setup(path: PathBuf) -> bool {
    !git::has_commits(&path).await
}

/// Register the folder at `path` as a Project. `set_up` is the user's go-ahead
/// to [`git::set_up`] a folder that isn't ready; without it, such a folder is
/// refused here rather than failing later, at the first Spawn.
#[tauri::command]
pub async fn add_project(name: String, path: PathBuf, set_up: bool) -> Result<Project, String> {
    if !git::has_commits(&path).await {
        if !set_up {
            return Err(err(Error::NotSetUp { path }));
        }
        git::set_up(&path).await.map_err(err)?;
    }
    let mut reg = storage::Registry::load().map_err(err)?;
    let project = Project {
        id: new_id(),
        name,
        path,
        added_at: OffsetDateTime::now_utc(),
    };
    reg.projects.push(project.clone());
    reg.save().map_err(err)?;
    Ok(project)
}

#[tauri::command]
pub async fn remove_project(id: String) -> Result<(), String> {
    let mut reg = storage::Registry::load().map_err(err)?;
    reg.projects.retain(|p| p.id != id);
    reg.save().map_err(err)
}

#[tauri::command]
pub async fn list_agents() -> Result<Vec<Agent>, String> {
    storage::list_agents().map_err(err)
}

#[tauri::command]
pub async fn spawn_agent(
    state: State<'_, AppState>,
    project_id: String,
    prompt: String,
    attachments: Vec<PathBuf>,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
) -> Result<Agent, String> {
    let reg = storage::Registry::load().map_err(err)?;
    let project = reg
        .project(&project_id)
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    state
        .runtime
        .spawn(project, prompt, attachments, model, effort, permission_mode)
        .await
        .map_err(err)
}

/// Continue a conversation with an Agent that has stopped working: a follow-up
/// prompt and any files attached to it, answered in the Agent's existing Worktree with its Session resumed,
/// on the model, effort and Permission Mode the caller names for this Turn.
#[tauri::command]
pub async fn resume_agent(
    state: State<'_, AppState>,
    agent_id: String,
    prompt: String,
    attachments: Vec<PathBuf>,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
) -> Result<Agent, String> {
    state
        .runtime
        .resume(
            &agent_id,
            prompt,
            attachments,
            model,
            effort,
            permission_mode,
        )
        .await
        .map_err(err)
}

/// Write a pasted file to the state directory, so it can be attached by path
/// like any other. The bytes arrive as the raw request body rather than as
/// JSON — a screenshot as a JSON array of numbers is several times its size —
/// with the name it was pasted under in the `x-name` header.
#[tauri::command]
pub async fn save_attachment(request: tauri::ipc::Request<'_>) -> Result<PathBuf, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the file's bytes as the request body".into());
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    crate::attachments::save(name, bytes).map_err(err)
}

/// An attached image's bytes, for the thumbnail on its chip. Returned raw
/// rather than as JSON, for the same reason [`save_attachment`] takes them so.
#[tauri::command]
pub async fn attachment_preview(path: PathBuf) -> Result<tauri::ipc::Response, String> {
    crate::attachments::preview(&path)
        .map(tauri::ipc::Response::new)
        .map_err(err)
}

#[tauri::command]
pub async fn stop_agent(state: State<'_, AppState>, agent_id: String) -> Result<(), String> {
    state.runtime.stop(&agent_id).await.map_err(err)
}

#[tauri::command]
pub async fn reap_agent(agent_id: String) -> Result<(), String> {
    let agent = storage::load_agent(&agent_id).map_err(err)?;
    if agent.state == AgentState::Running {
        return Err("cannot reap a running agent; stop it first".into());
    }

    // Best-effort worktree removal. If the Project has been unregistered we
    // fall back to a plain directory delete.
    let reg = storage::Registry::load().map_err(err)?;
    if let Some(project) = reg.project(&agent.project_id) {
        let _ = worktree::reap(&project.path, &agent.worktree_path, &agent.branch).await;
    } else if agent.worktree_path.exists() {
        let _ = std::fs::remove_dir_all(&agent.worktree_path);
    }

    // Delete the meta file so the Agent disappears from list_agents.
    let meta = paths::agent_meta_path(&agent_id).map_err(err)?;
    if meta.exists() {
        std::fs::remove_file(&meta).map_err(err)?;
    }
    Ok(())
}

/// Replay of the Agent's output, read back from its log on disk. Lets the UI
/// rebuild the output pane after a reload without disturbing a running Agent —
/// live events keep arriving on the `agent-event` channel either way.
#[tauri::command]
pub async fn agent_events(agent_id: String) -> Result<Vec<AgentEvent>, String> {
    storage::read_events(&agent_id).map_err(err)
}

/// Everything the Agent has produced in its Worktree, relative to the commit it
/// branched from. Safe to call in any state, including while the Agent runs.
#[tauri::command]
pub async fn agent_diff(agent_id: String) -> Result<WorktreeDiff, String> {
    let agent = storage::load_agent(&agent_id).map_err(err)?;
    let reg = storage::Registry::load().map_err(err)?;
    let project_path = reg.project(&agent.project_id).map(|p| p.path.clone());

    git::diff(
        project_path.as_deref(),
        &agent.worktree_path,
        &agent.branch,
        agent.base_commit.as_deref(),
    )
    .await
    .map_err(err)
}

/// Stage and commit everything in the Agent's Worktree, on the Agent's own
/// branch. Refused while the Agent is running, since it would race the Agent's
/// own writes and capture a half-finished tree.
#[tauri::command]
pub async fn agent_commit(agent_id: String, message: String) -> Result<Commit, String> {
    let agent = storage::load_agent(&agent_id).map_err(err)?;
    if agent.state == AgentState::Running {
        return Err("cannot commit while the agent is running; stop it first".into());
    }
    git::commit(&agent.worktree_path, &message)
        .await
        .map_err(err)
}

/// The Project's local branches, for the Merge picker. Agent branches are left
/// out, and the checked-out branch comes first.
#[tauri::command]
pub async fn project_branches(project_id: String) -> Result<Branches, String> {
    let reg = storage::Registry::load().map_err(err)?;
    let project = reg
        .project(&project_id)
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    git::branches(&project.path).await.map_err(err)
}

/// How a Merge the user asked for came out. A conflict is an outcome rather
/// than an error: the Project is untouched, and the UI offers to Resolve it.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MergeOutcome {
    Merged(Merged),
    Conflict { target: String, files: Vec<String> },
}

/// Merge a Committed Agent's branch into a branch of the Project. The one thing
/// the app writes to the Project, and only ever because the user asked.
///
/// Refused while the Agent is running — the merge would capture a tree the Agent
/// is still writing. The Agent survives a Merge: only Reap destroys anything, so
/// the Merge is recorded on the Agent and nothing is cleaned up.
#[tauri::command]
pub async fn agent_merge(agent_id: String, target: String) -> Result<MergeOutcome, String> {
    let mut agent = storage::load_agent(&agent_id).map_err(err)?;
    if agent.state == AgentState::Running {
        return Err("cannot merge while the agent is running; stop it first".into());
    }
    let project = project_of(&agent, "merge")?;

    match merging::merge(&project.path, &mut agent, &target).await {
        Ok((merged, _)) => Ok(MergeOutcome::Merged(merged)),
        Err(Error::MergeConflict { target, files }) => Ok(MergeOutcome::Conflict { target, files }),
        Err(e) => Err(err(e)),
    }
}

/// Resolve a Merge that conflicted: spawn a Resolver on a branch cut from the
/// Agent's, told to merge `target` in and settle `files`. When the Resolver's
/// Turn Completes the Merge is finished for the user and recorded on both.
///
/// Refused while the Agent is running, as a Merge is — the Resolver would be
/// cut from a branch still moving.
#[tauri::command]
pub async fn resolve_conflict(
    state: State<'_, AppState>,
    agent_id: String,
    target: String,
    files: Vec<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Result<Agent, String> {
    let agent = storage::load_agent(&agent_id).map_err(err)?;
    if agent.state == AgentState::Running {
        return Err("cannot resolve while the agent is running; stop it first".into());
    }
    let project = project_of(&agent, "resolve")?;
    state
        .runtime
        .spawn_resolver(&project, &agent, &target, &files, model, effort)
        .await
        .map_err(err)
}

/// The registered Project an Agent belongs to, or an error naming `action` for
/// an Agent whose Project has since been removed.
fn project_of(agent: &Agent, action: &str) -> Result<Project, String> {
    let reg = storage::Registry::load().map_err(err)?;
    reg.project(&agent.project_id).cloned().ok_or_else(|| {
        format!(
            "cannot {action}: the project this agent belongs to is no longer registered ({})",
            agent.project_id
        )
    })
}

/// The Merged Agents whose Worktree still holds work the Project does not
/// have — left dirty, or committed again after the Merge.
///
/// The sidebar buckets on the merge record alone, which would leave an Agent
/// that was Resumed and did more work sitting in Delivered as though you were
/// done with it. This is the one extra fact that reading needs, and it is asked
/// only of Agents that have a merge record at all, so the git cost stays
/// bounded by how many Agents you have already Merged rather than by how many
/// you have. Running Agents are skipped: their Worktree is being written as we
/// look, and Running outranks the merge record anyway.
#[tauri::command]
pub async fn agents_holding_work() -> Result<Vec<String>, String> {
    let agents = storage::list_agents().map_err(err)?;
    let mut holding = Vec::new();
    for agent in agents {
        if agent.merged_at.is_none() || agent.state == AgentState::Running {
            continue;
        }
        if git::holds_unmerged_work(&agent.worktree_path).await {
            holding.push(agent.id);
        }
    }
    Ok(holding)
}

/// The models this user's account can run, newest first, for the model picker.
/// Asked of Anthropic rather than hardcoded, so the list matches the account
/// and picks up models released after this build.
#[tauri::command]
pub async fn list_models() -> Result<Vec<ModelInfo>, String> {
    crate::models::list().await.map_err(err)
}

/// What every Agent has spent — tokens and money per Model — plus the account's
/// rate-limit windows as last reported. Read from the logs on each call rather
/// than tallied as events arrive, so the numbers are right after a restart and
/// cover Agents this window never opened.
#[tauri::command]
pub async fn usage_summary() -> Result<UsageSummary, String> {
    crate::usage::summary().map_err(err)
}

/// The launch-time Orphans that are still Orphaned. Read fresh from disk, so
/// one the user has since Resumed or Reaped drops out: a reload of the window
/// asks again, but this process kept its launch-time list.
#[tauri::command]
pub async fn startup_orphans(state: State<'_, AppState>) -> Result<Vec<Agent>, String> {
    let ids = state.startup_orphans.lock().map_err(err)?.clone();
    Ok(ids
        .iter()
        .filter_map(|id| storage::load_agent(id).ok())
        .filter(|a| a.state == AgentState::Orphaned)
        .collect())
}

/// The user has seen the launch-time Orphans; stop reporting them.
#[tauri::command]
pub async fn dismiss_orphans(state: State<'_, AppState>) -> Result<(), String> {
    state.startup_orphans.lock().map_err(err)?.clear();
    Ok(())
}
