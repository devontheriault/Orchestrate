pub mod attachments;
pub mod commands;
pub mod domain;
pub mod error;
pub mod git;
pub mod merging;
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
use domain::{Agent, AgentEvent};
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
/// platform and the config is a single static description. macOS keeps its
/// decorations so the traffic lights stay native, and only hides the bar's
/// chrome so the app's own header can draw behind them; everywhere else the
/// decoration goes entirely and the header supplies the controls itself.
///
/// Except under a tiling compositor, which places, sizes and closes every
/// window itself and gives none of them buttons, so drawn controls there are
/// just noise. Keeping the decoration on Linux doesn't get us that for free:
/// GTK3 only asks for server-side decorations over KDE's protocol, Hyprland
/// doesn't speak it, and GTK falls back to drawing its own titlebar. So the
/// window stays undecorated and the UI is told, before its first frame, to
/// leave the controls and resize edges off (see `platform.ts`).
///
/// It gets no minimum size there either. A tile is whatever size the layout
/// makes it, and one smaller than the minimum doesn't grow to fit — GTK draws
/// the window at its minimum anyway and the compositor crops the rest, which
/// cuts off the composer and the settings button at the bottom. The UI copes
/// with any size it's given.
fn build_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Claude Wrapper")
        .inner_size(1280.0, 800.0);

    #[cfg(target_os = "macos")]
    let win = win
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    #[cfg(not(target_os = "macos"))]
    let win = win.decorations(false);

    #[cfg(target_os = "linux")]
    let tiled = is_tiling_compositor(|k| std::env::var(k).ok());
    #[cfg(not(target_os = "linux"))]
    let tiled = false;

    let win = if tiled {
        win.initialization_script("window.__COMPOSITOR_OWNS_FRAME__ = true;")
    } else {
        win.min_inner_size(560.0, 420.0)
    };

    win.build()?;
    Ok(())
}

/// Whether the session is a tiling Wayland compositor or X11 window manager,
/// read off the variables each one exports into the programs it launches.
#[cfg(any(target_os = "linux", test))]
fn is_tiling_compositor(var: impl Fn(&str) -> Option<String>) -> bool {
    const SOCKETS: [&str; 4] = [
        "HYPRLAND_INSTANCE_SIGNATURE",
        "SWAYSOCK",
        "NIRI_SOCKET",
        "I3SOCK",
    ];
    const DESKTOPS: [&str; 5] = ["hyprland", "sway", "niri", "river", "i3"];

    SOCKETS
        .iter()
        .any(|k| var(k).is_some_and(|v| !v.is_empty()))
        || var("XDG_CURRENT_DESKTOP").is_some_and(|v| {
            v.split(':')
                .any(|d| DESKTOPS.contains(&d.to_ascii_lowercase().as_str()))
        })
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

    let startup_orphans = runtime::adopt_orphans_on_launch()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.id)
        .collect();
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
            startup_orphans: std::sync::Mutex::new(startup_orphans),
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
            commands::project_needs_setup,
            commands::add_project,
            commands::remove_project,
            commands::list_agents,
            commands::spawn_agent,
            commands::resume_agent,
            commands::save_attachment,
            commands::save_clipboard_image,
            commands::attachment_preview,
            commands::stop_agent,
            commands::discard_agent,
            commands::agent_events,
            commands::agent_diff,
            commands::agent_commit,
            commands::agent_merge,
            commands::resolve_conflict,
            commands::agents_holding_work,
            commands::project_branches,
            commands::startup_orphans,
            commands::dismiss_orphans,
            commands::list_models,
            commands::usage_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::is_tiling_compositor;

    fn env<'a>(pairs: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn tiling_compositors_are_recognised() {
        assert!(is_tiling_compositor(env(&[(
            "HYPRLAND_INSTANCE_SIGNATURE",
            "abc"
        )])));
        assert!(is_tiling_compositor(env(&[(
            "XDG_CURRENT_DESKTOP",
            "sway"
        )])));
        assert!(is_tiling_compositor(env(&[(
            "XDG_CURRENT_DESKTOP",
            "Hyprland"
        )])));
    }

    #[test]
    fn stacking_desktops_are_not() {
        assert!(!is_tiling_compositor(env(&[])));
        assert!(!is_tiling_compositor(env(&[(
            "XDG_CURRENT_DESKTOP",
            "ubuntu:GNOME"
        )])));
        assert!(!is_tiling_compositor(env(&[
            ("XDG_CURRENT_DESKTOP", "KDE"),
            ("SWAYSOCK", "")
        ])));
    }
}
