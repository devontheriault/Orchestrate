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

use std::sync::Arc;

use crate::host::client::HostLink;
use crate::host::hosts::{HostInfo, Hosts, LOCAL};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The link to Host `id`, or this machine's when none is named.
fn link(hosts: &Hosts, id: Option<&str>) -> Result<HostLink, String> {
    let id = id.unwrap_or(LOCAL);
    hosts
        .get(id)
        .ok_or_else(|| format!("{id} isn't one of this window's Hosts"))
}

/// Pass one call to a Host — this machine's unless `host` names another — and
/// hand back its answer.
#[tauri::command]
pub async fn host(
    hosts: State<'_, Arc<Hosts>>,
    host: Option<String>,
    method: String,
    args: Value,
) -> Result<Value, String> {
    link(&hosts, host.as_deref())?.call(&method, args).await
}

/// Every Host this window knows and where it stands with each; changes after
/// this arrive as `host-status` events, stamped with the Host's id.
#[tauri::command]
pub fn hosts(hosts: State<'_, Arc<Hosts>>) -> Vec<HostInfo> {
    hosts.list()
}

/// Add another machine's Host by its Tailscale name.
#[tauri::command]
pub fn add_host(hosts: State<'_, Arc<Hosts>>, name: String) -> Result<HostInfo, String> {
    hosts.add(&name)
}

/// Forget another machine's Host. Its Agents stay on it, untouched.
#[tauri::command]
pub fn remove_host(hosts: State<'_, Arc<Hosts>>, id: String) -> Result<(), String> {
    hosts.remove(&id)
}

/// Send files the user attached to the Host, and return the paths of the
/// Host's copies, which are what the Agent is handed. Read here, on the
/// window's side, because the files are on this machine and the Host may not be.
#[tauri::command]
pub async fn send_attachments(
    hosts: State<'_, Arc<Hosts>>,
    host: Option<String>,
    paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, String> {
    use base64::Engine;
    let link = link(&hosts, host.as_deref())?;
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
