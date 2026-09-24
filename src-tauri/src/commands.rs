//! Tauri command surface. Each function is invocable from the frontend via
//! `@tauri-apps/api/core#invoke`. Errors are stringified so the frontend gets
//! a plain `Error` object with a readable message.
//!
//! Almost everything the webview asks is a question for the Host, and goes
//! through [`host`] untouched (the calls are listed in `host/calls.rs`). What
//! stays here is what belongs to the window's own machine: the clipboard, and
//! files the user drops on the window.

use std::path::PathBuf;

use serde_json::Value;
use tauri::State;

use crate::host::client::{HostLink, Status};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Pass one call to the Host and hand back its answer.
#[tauri::command]
pub async fn host(link: State<'_, HostLink>, method: String, args: Value) -> Result<Value, String> {
    link.call(&method, args).await
}

/// Where the window stands with its Host right now; changes after this arrive
/// as `host-status` events.
#[tauri::command]
pub fn host_status(link: State<'_, HostLink>) -> Status {
    link.status()
}

/// Send files the user attached to the Host, and return the paths of the
/// Host's copies, which are what the Agent is handed. Read here, on the
/// window's side, because the files are on this machine and the Host may not be.
#[tauri::command]
pub async fn send_attachments(
    link: State<'_, HostLink>,
    paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    use base64::Engine;
    let mut sent = Vec::with_capacity(paths.len());
    for path in paths {
        let (name, bytes) =
            tauri::async_runtime::spawn_blocking(move || crate::attachments::read_for_host(&path))
                .await
                .map_err(err)?
                .map_err(err)?;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let stored = link
            .call(
                "store_attachment",
                serde_json::json!({ "name": name, "data": data }),
            )
            .await?;
        sent.push(serde_json::from_value(stored).map_err(err)?);
    }
    Ok(sent)
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

/// Save the image on the OS clipboard, for a paste whose event carried none —
/// all a paste on Linux ever carries, since WebKitGTK withholds clipboard
/// images from the page. `None` when there's no image there either.
#[tauri::command]
pub async fn save_clipboard_image(name: String) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::attachments::save_clipboard_image(&name))
        .await
        .map_err(err)?
        .map_err(err)
}

/// An attached image's bytes, for the thumbnail on its chip. Returned raw
/// rather than as JSON, for the same reason [`save_attachment`] takes them so.
#[tauri::command]
pub async fn attachment_preview(path: PathBuf) -> Result<tauri::ipc::Response, String> {
    crate::attachments::preview(&path)
        .map(tauri::ipc::Response::new)
        .map_err(err)
}
