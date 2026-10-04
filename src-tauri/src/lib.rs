pub mod attachments;
pub mod commands;
pub mod domain;
pub mod error;
pub mod git;
pub mod handoff;
pub mod host;
pub mod mail;
pub mod merging;
pub mod models;
pub mod paths;
pub mod plugins;
pub mod process;
pub mod runtime;
pub mod slash;
pub mod storage;
pub mod title;
pub mod usage;
pub mod worktree;

#[cfg(test)]
pub(crate) mod test_util;

use std::sync::Arc;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use host::hosts::Hosts;

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
/// leave the controls, resize edges and drag handle off (see `platform.ts`).
///
/// It gets no minimum size there either. A tile is whatever size the layout
/// makes it, and one smaller than the minimum doesn't grow to fit — GTK draws
/// the window at its minimum anyway and the compositor crops the rest, which
/// cuts off the composer and the settings button at the bottom. The UI copes
/// with any size it's given.
///
/// The window is always transparent, because a window can only be made so
/// when it is built and the Glass theme can be picked at any time. Every other
/// theme paints the page opaque, so it looks the same as a window that isn't;
/// Glass paints only a veil, and the desktop shows through.
#[cfg(desktop)]
fn build_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Orchestrate")
        .inner_size(1280.0, 800.0)
        .transparent(true);

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

/// On a phone the window is the whole screen and the OS draws around it. The
/// UI is told so before its first frame (see `platform.ts`): it has no frame
/// to draw, and no Host on this device to ask — only other machines'.
#[cfg(mobile)]
fn build_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .initialization_script("window.__MOBILE__ = true;")
        .build()?;
    Ok(())
}

/// iOS puts a bar of ↑ ↓ ✓ above the keyboard for any field in a web page.
/// It's for stepping through a form, which the composer isn't, and it takes a
/// row of the screen. The page can't turn it off: the bar comes from WebKit's
/// content view, so that view is made to answer that it has none. Capacitor's
/// Keyboard plugin makes the same swap.
#[cfg(target_os = "ios")]
fn hide_form_bar() {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::sel;

    extern "C-unwind" fn no_bar(_this: *mut AnyObject, _cmd: Sel) -> *mut AnyObject {
        std::ptr::null_mut()
    }

    // A private WebKit class: if a later iOS renames it, the bar just stays.
    let Some(view) = AnyClass::get(c"WKContentView") else {
        return;
    };
    // SAFETY: `inputAccessoryView` takes no arguments and returns an object or
    // nil (type encoding `@@:`), which is exactly `no_bar`'s shape.
    unsafe {
        let imp: Imp = std::mem::transmute(
            no_bar as extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject,
        );
        objc2::ffi::class_replaceMethod(
            view as *const AnyClass as *mut AnyClass,
            sel!(inputAccessoryView),
            imp,
            c"@@:".as_ptr(),
        );
    }
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
/// It's the GPU buffers it hands the compositor that do it, so keep the
/// renderer but have it hand over shared memory instead.
///
/// Turning the renderer off, as this used to, is worse. Its fallback in
/// WebKitGTK 2.50, which the AppImage bundles, never clears a frame before
/// drawing the next one into a transparent window, so anything see-through
/// piles up: Glass darkens with every repaint, and a closed menu or old text
/// stays on screen. It is also slower, dropping frames whenever the theme
/// changes. And 2.50 turns the renderer off by itself under NVIDIA's driver,
/// so there it has to be forced back on.
///
/// Left alone if the user set any of these themselves.
#[cfg(target_os = "linux")]
fn apply_linux_webkit_workarounds() {
    let nvidia = std::path::Path::new("/sys/module/nvidia").exists();
    for (key, value) in webkit_renderer_env(|k| std::env::var_os(k).is_some(), nvidia) {
        std::env::set_var(key, value);
    }
}

#[cfg(any(target_os = "linux", test))]
fn webkit_renderer_env(
    is_set: impl Fn(&str) -> bool,
    nvidia: bool,
) -> Vec<(&'static str, &'static str)> {
    const KEYS: [&str; 3] = [
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_FORCE_DMABUF_RENDERER",
        "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
    ];
    if KEYS.iter().any(|k| is_set(k)) {
        return vec![];
    }
    let mut env = vec![("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1")];
    if nvidia {
        env.push(("WEBKIT_FORCE_DMABUF_RENDERER", "1"));
    }
    env
}

/// The app's entry point: this machine's Host with `--host`, a window
/// otherwise. The window owns no Agents; it asks the Host for everything, and
/// closing it leaves every Agent working (ADR 0009).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if std::env::args().nth(1).as_deref() == Some("--host") {
        return host::run();
    }

    #[cfg(target_os = "linux")]
    apply_linux_webkit_workarounds();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            #[cfg(mobile)]
            {
                let dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&dir)?;
                paths::use_app_data_dir(dir);
            }

            build_main_window(app.handle())?;
            #[cfg(target_os = "ios")]
            hide_form_bar();

            // A phone can't run `claude`, so it has no Host of its own.
            #[cfg(desktop)]
            let local = Some(host::client::Target::Local {
                socket: paths::host_socket()?,
                start: Arc::new(host::service::start),
            });
            #[cfg(mobile)]
            let local = None;

            let handle = app.handle().clone();
            let hosts = Hosts::new(
                local,
                Arc::new(move |name: &str, payload| {
                    let _ = handle.emit(name, payload);
                }),
            );
            let connecting = hosts.clone();
            tauri::async_runtime::spawn(async move { connecting.connect_all() });
            app.manage(hosts);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::host,
            commands::hosts,
            commands::add_host,
            commands::tailnet_machines,
            commands::remove_host,
            commands::send_attachments,
            commands::save_attachment,
            commands::save_clipboard_image,
            commands::attachment_preview,
            commands::notification_icon,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{is_tiling_compositor, webkit_renderer_env};

    #[test]
    fn webkit_renders_to_shared_memory() {
        assert_eq!(
            webkit_renderer_env(|_| false, false),
            [("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1")]
        );
    }

    #[test]
    fn nvidia_forces_the_renderer_back_on() {
        assert_eq!(
            webkit_renderer_env(|_| false, true),
            [
                ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1")
            ]
        );
    }

    #[test]
    fn the_users_own_renderer_choice_stands() {
        for key in [
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_FORCE_DMABUF_RENDERER",
            "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
        ] {
            assert!(webkit_renderer_env(|k| k == key, true).is_empty());
        }
    }

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
