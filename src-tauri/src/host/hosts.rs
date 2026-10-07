//! The Hosts a window talks to: its own machine's, and any the user has added
//! by Tailscale name. One [`HostLink`] each, all connected at once, so the
//! window shows every machine's Agents together. A phone runs no Host of its
//! own, so a window there has only the ones added.
//!
//! What the window hears from each is stamped with that Host's id — the
//! `host` field on every event payload — so the webview knows where each
//! Agent lives and which Host to ask about it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

use super::client::{HostLink, Notify, Status, Target};
use super::tailnet;
use crate::paths;

/// The id of the window's own machine's Host.
pub const LOCAL: &str = "local";

/// A Host as the webview lists it.
#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    /// `local`, or the Tailscale name it was added by.
    pub id: String,
    pub local: bool,
    pub status: Status,
}

/// The remote Hosts the user has added, kept in the state directory.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    hosts: Vec<String>,
}

fn saved_path() -> crate::error::Result<PathBuf> {
    Ok(paths::state_dir()?.join("hosts.json"))
}

fn load() -> Vec<String> {
    saved_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Saved>(&s).ok())
        .map(|s| s.hosts)
        .unwrap_or_default()
}

struct Entry {
    link: HostLink,
    stop: watch::Sender<bool>,
}

pub struct Hosts {
    /// This machine's Host, or `None` on a phone, which has none.
    local: Option<Target>,
    /// Hands an event to the webview. Every Host's go through it, stamped.
    emit: Notify,
    links: Mutex<BTreeMap<String, Entry>>,
}

impl Hosts {
    /// The window's Hosts, not yet connected: [`connect_all`] does that, from
    /// inside the async runtime.
    ///
    /// [`connect_all`]: Self::connect_all
    pub fn new(local: Option<Target>, emit: Notify) -> Arc<Self> {
        Arc::new(Self {
            local,
            emit,
            links: Mutex::default(),
        })
    }

    /// Connect to this machine's Host and every saved remote one.
    pub fn connect_all(&self) {
        if let Some(local) = &self.local {
            self.launch(LOCAL, local.clone());
        }
        for name in load() {
            self.launch(&name, remote_target(&name));
        }
    }

    fn launch(&self, id: &str, target: Target) {
        let emit = self.emit.clone();
        let stamp = id.to_owned();
        let notify: Notify = Arc::new(move |name, payload| emit(name, stamped(payload, &stamp)));
        let link = HostLink::new(target, notify);
        let (stop, stopped) = watch::channel(false);
        tauri::async_runtime::spawn(link.clone().run(stopped));
        self.links
            .lock()
            .unwrap()
            .insert(id.to_owned(), Entry { link, stop });
    }

    /// The link to one Host, by id.
    pub fn get(&self, id: &str) -> Option<HostLink> {
        self.links.lock().unwrap().get(id).map(|e| e.link.clone())
    }

    /// Have every link check its Host is still there, and reconnect now if
    /// not. See [`HostLink::wake`].
    pub fn wake(&self) {
        for e in self.links.lock().unwrap().values() {
            e.link.wake();
        }
    }

    pub fn list(&self) -> Vec<HostInfo> {
        self.links
            .lock()
            .unwrap()
            .iter()
            .map(|(id, e)| HostInfo {
                id: id.clone(),
                local: id == LOCAL,
                status: e.link.status(),
            })
            .collect()
    }

    /// Add another machine's Host by its Tailscale name, and start talking to
    /// it. Kept for next time whether or not it answers now.
    pub fn add(&self, name: &str) -> Result<HostInfo, String> {
        let id = host_id(name)?;
        if self.links.lock().unwrap().contains_key(&id) {
            return Err(format!("{id} is already one of this window's Hosts"));
        }
        self.launch(&id, remote_target(&id));
        self.save()?;
        Ok(HostInfo {
            id: id.clone(),
            local: false,
            status: self.get(&id).map(|l| l.status()).expect("just launched"),
        })
    }

    /// Stop talking to a remote Host and forget it. Its Agents are untouched:
    /// they are that machine's.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        if id == LOCAL {
            return Err("this machine's Host can't be removed".into());
        }
        let entry = self.links.lock().unwrap().remove(id);
        match entry {
            Some(e) => {
                e.stop.send_replace(true);
                self.save()
            }
            None => Err(format!("{id} isn't one of this window's Hosts")),
        }
    }

    fn save(&self) -> Result<(), String> {
        let hosts = self
            .links
            .lock()
            .unwrap()
            .keys()
            .filter(|id| *id != LOCAL)
            .cloned()
            .collect();
        let path = saved_path().map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(&Saved { hosts }).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("couldn't save {}: {e}", path.display()))
    }
}

/// What the user typed, as the id a Host is kept under: its Tailscale name,
/// trimmed and lower-cased, as MagicDNS treats it.
pub fn host_id(name: &str) -> Result<String, String> {
    let id = name.trim().trim_end_matches('.').to_ascii_lowercase();
    if id.is_empty() {
        return Err("give the machine's Tailscale name".into());
    }
    if id == LOCAL || id.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(format!("{name:?} isn't a machine name"));
    }
    Ok(id)
}

/// Where a Host added as `id` is: its Tailscale name on the Host port, unless
/// the name already says which port.
fn remote_target(id: &str) -> Target {
    let address = if id.contains(':') {
        id.to_owned()
    } else {
        format!("{id}:{}", tailnet::port())
    };
    Target::Remote { address }
}

/// `payload` with the Host it came from written into it.
pub fn stamped(mut payload: Value, host: &str) -> Value {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("host".into(), Value::String(host.to_owned()));
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_host_is_kept_by_its_tailscale_name() {
        assert_eq!(host_id(" Desktop.\n").unwrap(), "desktop");
        assert_eq!(
            host_id("desktop.tail1234.ts.net").unwrap(),
            "desktop.tail1234.ts.net"
        );
        assert!(host_id("  ").is_err());
        assert!(host_id("local").is_err());
        assert!(host_id("my desktop").is_err());
    }

    #[test]
    fn a_remote_host_is_reached_on_the_host_port_unless_told_otherwise() {
        let Target::Remote { address } = remote_target("desktop") else {
            panic!()
        };
        assert_eq!(address, format!("desktop:{}", tailnet::DEFAULT_PORT));
        let Target::Remote { address } = remote_target("127.0.0.1:47301") else {
            panic!()
        };
        assert_eq!(address, "127.0.0.1:47301");
    }

    #[test]
    fn events_are_stamped_with_their_host() {
        assert_eq!(
            stamped(json!({ "id": "a1" }), "desktop"),
            json!({ "id": "a1", "host": "desktop" })
        );
        // Nothing to stamp on a payload that isn't an object.
        assert_eq!(stamped(json!(null), "desktop"), json!(null));
    }
}
