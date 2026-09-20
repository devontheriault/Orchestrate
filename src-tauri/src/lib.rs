pub mod commands;
pub mod error;
pub mod git;
pub mod model;
pub mod models;
pub mod paths;
pub mod runtime;
pub mod storage;
pub mod usage;
pub mod worktree;

#[cfg(test)]
pub(crate) mod test_util;

use serde::Serialize;
use tauri::{Emitter, Manager};

use commands::AppState;
use model::{Agent, AgentEvent};
use runtime::{AgentRuntime, RuntimeEvent};

/// Payload for the `agent-event` Tauri event: one stream-json line from a
/// running Agent, timestamped and tagged with which Agent produced it.
#[derive(Serialize, Clone)]
struct AgentEventPayload {
    agent_id: String,
    event: AgentEvent,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let startup_orphans = runtime::adopt_orphans_on_launch().unwrap_or_default();
    let (rt, rx) = AgentRuntime::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let mut rx = rx;
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        RuntimeEvent::AgentEvent { agent_id, event } => {
                            let _ =
                                handle.emit("agent-event", AgentEventPayload { agent_id, event });
                        }
                        RuntimeEvent::StateChanged { agent, .. } => {
                            let _ = handle.emit::<Agent>("agent-state-changed", *agent);
                        }
                    }
                }
            });
            Ok(())
        })
        .manage(AppState {
            runtime: rt,
            startup_orphans,
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Best-effort: mark live Agents as Orphaned before the process
                // dies. If this race is lost, adopt_orphans_on_launch catches
                // them on the next start.
                let state = window.state::<AppState>();
                let runtime = state.runtime.clone();
                tauri::async_runtime::block_on(async move {
                    runtime.shutdown().await;
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::add_project,
            commands::remove_project,
            commands::list_agents,
            commands::spawn_agent,
            commands::resume_agent,
            commands::stop_agent,
            commands::reap_agent,
            commands::agent_events,
            commands::agent_diff,
            commands::agent_commit,
            commands::startup_orphans,
            commands::list_models,
            commands::usage_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
