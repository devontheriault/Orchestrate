//! Tauri command surface. Each function is invocable from the frontend via
//! `@tauri-apps/api/core#invoke`. Errors are stringified so the frontend gets
//! a plain `Error` object with a readable message.

use std::path::PathBuf;

use tauri::State;
use time::OffsetDateTime;

use crate::model::{new_id, Agent, AgentState, Project};
use crate::runtime::AgentRuntime;
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
) -> Result<Agent, String> {
    let reg = storage::Registry::load().map_err(err)?;
    let project = reg
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    state.runtime.spawn(project, prompt).await.map_err(err)
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

#[tauri::command]
pub async fn startup_orphans(state: State<'_, AppState>) -> Result<Vec<Agent>, String> {
    Ok(state.startup_orphans.clone())
}
