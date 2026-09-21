//! Tauri command surface. Each function is invocable from the frontend via
//! `@tauri-apps/api/core#invoke`. Errors are stringified so the frontend gets
//! a plain `Error` object with a readable message.

use std::path::PathBuf;

use tauri::State;
use time::OffsetDateTime;

use crate::git::{self, Branches, Commit, Merged, WorktreeDiff};
use crate::model::{new_id, Agent, AgentEvent, AgentState, Project};
use crate::models::ModelInfo;
use crate::runtime::AgentRuntime;
use crate::usage::UsageSummary;
use crate::{paths, storage, worktree};

/// App-level shared state, managed by Tauri.
pub struct AppState {
    pub runtime: AgentRuntime,
    /// Orphans adopted at launch time. Cached so the UI can surface them
    /// without re-scanning the filesystem.
    pub startup_orphans: Vec<Agent>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    storage::Registry::load().map(|r| r.projects).map_err(err)
}

#[tauri::command]
pub async fn add_project(name: String, path: PathBuf) -> Result<Project, String> {
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
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
) -> Result<Agent, String> {
    let reg = storage::Registry::load().map_err(err)?;
    let project = reg
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    state
        .runtime
        .spawn(project, prompt, model, effort, permission_mode)
        .await
        .map_err(err)
}

/// Continue a conversation with an Agent that has stopped working: a follow-up
/// prompt, answered in the Agent's existing Worktree with its Session resumed,
/// on the model, effort and Permission Mode the caller names for this Turn.
#[tauri::command]
pub async fn resume_agent(
    state: State<'_, AppState>,
    agent_id: String,
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
) -> Result<Agent, String> {
    state
        .runtime
        .resume(&agent_id, prompt, model, effort, permission_mode)
        .await
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
    if let Some(project) = reg.projects.iter().find(|p| p.id == agent.project_id) {
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
    let project_path = reg
        .projects
        .iter()
        .find(|p| p.id == agent.project_id)
        .map(|p| p.path.clone());

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
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    git::branches(&project.path).await.map_err(err)
}

/// Merge a Committed Agent's branch into a branch of the Project. The one thing
/// the app writes to the Project, and only ever because the user asked.
///
/// Refused while the Agent is running — the merge would capture a tree the Agent
/// is still writing. The Agent survives a Merge: only Reap destroys anything, so
/// the Merge is recorded on the Agent and nothing is cleaned up.
#[tauri::command]
pub async fn agent_merge(agent_id: String, target: String) -> Result<Merged, String> {
    let mut agent = storage::load_agent(&agent_id).map_err(err)?;
    if agent.state == AgentState::Running {
        return Err("cannot merge while the agent is running; stop it first".into());
    }

    let reg = storage::Registry::load().map_err(err)?;
    let project = reg
        .projects
        .iter()
        .find(|p| p.id == agent.project_id)
        .ok_or_else(|| {
            format!(
                "cannot merge: the project this agent belongs to is no longer registered ({})",
                agent.project_id
            )
        })?;

    // Git's own merge subject, with the Agent's name under it so the history
    // says which piece of work merged rather than only which branch.
    let what = agent.title.clone().unwrap_or_else(|| {
        agent
            .task
            .prompt
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned()
    });
    let mut message = format!("Merge branch '{}' into {target}", agent.branch);
    if !what.is_empty() {
        message.push_str("\n\n");
        message.push_str(&what);
    }

    let merged = git::merge(
        &project.path,
        &agent.worktree_path,
        &agent.branch,
        &target,
        &message,
    )
    .await
    .map_err(err)?;

    agent.merged_branch = Some(merged.target.clone());
    agent.merged_at = Some(OffsetDateTime::now_utc());
    storage::save_agent(&agent).map_err(err)?;

    Ok(merged)
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

#[tauri::command]
pub async fn startup_orphans(state: State<'_, AppState>) -> Result<Vec<Agent>, String> {
    Ok(state.startup_orphans.clone())
}
