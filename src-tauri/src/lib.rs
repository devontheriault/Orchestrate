pub mod commands;
pub mod error;
pub mod git;
pub mod model;
pub mod models;
pub mod paths;
pub mod runtime;
pub mod storage;
pub mod title;
pub mod usage;
pub mod worktree;

#[cfg(test)]
pub(crate) mod test_util;

use serde::Serialize;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

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

/// Build the one window the app has.
///
/// It lives here rather than in `tauri.conf.json` because its frame differs by
/// platform and the config is a single static description.
///
/// The rule is to wear whatever frame the desktop hands out, and only draw our
/// own where it hands out none. macOS keeps its decorations so the traffic
/// lights stay native, and only hides the bar's chrome so the app's own header
/// can draw behind them. Linux keeps them too: tao's window is a plain
/// `GtkWindow`, so GTK negotiates for us over KDE's `server-decoration`
/// protocol and stands its own titlebar down wherever the compositor answers
/// `Server` — Hyprland, sway and KWin all do, and under a tiling layout that
/// resolves to no titlebar at all, just the compositor's border. GNOME, which
/// offers no server-side mode, gets GTK's own bar instead. Either way the
/// window is dressed like every other window on that desktop, and minimize and
/// maximize mean whatever the desktop means by them rather than being buttons
/// we draw over a compositor that has no such states.
///
/// Windows is the one platform with nothing to negotiate, so it runs
/// undecorated and the app's header supplies the controls itself. `platform.ts`
/// mirrors this split on the UI side.
fn build_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Claude Wrapper")
        .inner_size(1280.0, 800.0)
        .min_inner_size(560.0, 420.0);

    #[cfg(target_os = "macos")]
    let win = win
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    #[cfg(target_os = "windows")]
    let win = win.decorations(false);

    win.build()?;
    Ok(())
}

/// WebKitGTK's DMA-BUF renderer crashes under Wayland on some drivers (notably
/// NVIDIA), killing the process at startup with
/// `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.`
/// Fall back to the plain renderer unless the user set the variable themselves.
#[cfg(target_os = "linux")]
fn apply_linux_webkit_workarounds() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    apply_linux_webkit_workarounds();

    let startup_orphans = runtime::adopt_orphans_on_launch().unwrap_or_default();
    let (rt, rx) = AgentRuntime::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            build_main_window(app.handle())?;

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
            commands::agent_merge,
            commands::agents_holding_work,
            commands::project_branches,
            commands::startup_orphans,
            commands::list_models,
            commands::usage_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
