//! The tailnet: where a Host listens for windows on the user's other machines,
//! and how it knows a connection is the user's own (ADR 0012).
//!
//! A Host listens on its Tailscale address only, never on every interface, and
//! asks Tailscale who each peer is: only a machine logged in as the same
//! Tailscale user as this one gets in. A node shared into the tailnet, or a
//! tagged server, is someone else. All of it goes through the `tailscale` CLI,
//! so a machine without Tailscale simply has no listener — its Host is
//! reachable from this machine alone.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;

/// The port a Host listens on, on its tailnet address. Windows on other
/// machines reach a Host as `<its Tailscale name>:47300`.
pub const DEFAULT_PORT: u16 = 47300;

/// The Host port, overridable with `CLAUDEWRAPPER_HOST_PORT`.
pub fn port() -> u16 {
    std::env::var("CLAUDEWRAPPER_HOST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Decides whether a connection from `peer` may talk to this Host, and if not,
/// why not. [`vet`] in real use; tests stand in their own.
pub type Vet = Arc<
    dyn Fn(SocketAddr) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync,
>;

/// The real [`Vet`]: the peer must be the same Tailscale user as this machine.
pub fn tailscale_vet() -> Vet {
    Arc::new(|peer| Box::pin(vet(peer)))
}

/// This machine's tailnet IPv4 address, or `None` when Tailscale isn't running.
pub async fn address() -> Option<IpAddr> {
    let out = tailscale(&["ip", "-4"]).await.ok()?;
    out.lines().next()?.trim().parse().ok()
}

async fn vet(peer: SocketAddr) -> Result<(), String> {
    let status = tailscale(&["status", "--json"]).await?;
    let me = own_user(&serde_json::from_str(&status).map_err(|e| e.to_string())?)
        .ok_or("Tailscale didn't say who this machine belongs to")?;
    let whois = tailscale(&["whois", "--json", &peer.to_string()])
        .await
        .map_err(|_| format!("Tailscale doesn't know {peer}"))?;
    let (them, login) = peer_user(&serde_json::from_str(&whois).map_err(|e| e.to_string())?)
        .ok_or_else(|| format!("Tailscale didn't say who {peer} belongs to"))?;
    if them == me {
        Ok(())
    } else {
        Err(format!(
            "{login} isn't the Tailscale user this machine belongs to"
        ))
    }
}

/// The user id this machine is logged in as, from `tailscale status --json`.
pub fn own_user(status: &Value) -> Option<u64> {
    status["Self"]["UserID"].as_u64()
}

/// The user id and login a peer belongs to, from `tailscale whois --json`.
pub fn peer_user(whois: &Value) -> Option<(u64, String)> {
    let profile = &whois["UserProfile"];
    Some((
        profile["ID"].as_u64()?,
        profile["LoginName"].as_str().unwrap_or("?").to_owned(),
    ))
}

/// One of the user's other machines on the tailnet, as the Hosts dialog offers
/// it. Whether it runs this app, Tailscale can't say: adding it finds out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Machine {
    /// Its MagicDNS name, the one to add it by.
    pub name: String,
    pub os: String,
    pub online: bool,
}

/// The user's other machines, as `tailscale status --json` on this one lists
/// them. `None` when Tailscale isn't running here.
pub async fn machines() -> Option<Vec<Machine>> {
    let status = tailscale(&["status", "--json"]).await.ok()?;
    Some(own_machines(&serde_json::from_str(&status).ok()?))
}

/// The peers in `status` that are the same Tailscale user as this machine —
/// the only ones whose Host would let this window in — and that could run a
/// Host: not phones. The ones online first.
pub fn own_machines(status: &Value) -> Vec<Machine> {
    let Some(me) = own_user(status) else {
        return Vec::new();
    };
    let Some(peers) = status["Peer"].as_object() else {
        return Vec::new();
    };
    let mut machines: Vec<Machine> = peers
        .values()
        .filter(|p| p["UserID"].as_u64() == Some(me))
        .filter_map(|p| {
            let os = p["OS"].as_str().unwrap_or("").to_owned();
            if matches!(os.as_str(), "iOS" | "android" | "tvOS") {
                return None;
            }
            // The first label of its MagicDNS name, which is what resolves.
            // Its HostName is what the machine calls itself, and may not.
            let name = p["DNSName"]
                .as_str()
                .and_then(|n| n.split('.').next())
                .filter(|n| !n.is_empty())
                .or_else(|| p["HostName"].as_str())?
                .to_ascii_lowercase();
            Some(Machine {
                name,
                os,
                online: p["Online"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    machines.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.cmp(&b.name)));
    machines
}

async fn tailscale(args: &[&str]) -> Result<String, String> {
    let out = Command::new("tailscale")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("couldn't run tailscale: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_who_this_machine_is() {
        let status = json!({
            "Self": { "HostName": "desktop", "UserID": 4242 },
            "User": { "4242": { "ID": 4242, "LoginName": "dev@example.com" } },
        });
        assert_eq!(own_user(&status), Some(4242));
        assert_eq!(own_user(&json!({ "BackendState": "Stopped" })), None);
    }

    #[test]
    fn reads_who_a_peer_is() {
        let whois = json!({
            "Node": { "Name": "laptop.tail1234.ts.net." },
            "UserProfile": { "ID": 4242, "LoginName": "dev@example.com" },
        });
        assert_eq!(peer_user(&whois), Some((4242, "dev@example.com".into())));
        assert_eq!(peer_user(&json!({ "Node": {} })), None);
    }

    #[test]
    fn lists_the_users_own_machines_that_could_run_a_host() {
        let status = json!({
            "Self": { "HostName": "omarchy", "UserID": 4242 },
            "Peer": {
                "a": { "HostName": "Desktop", "DNSName": "desktop.tail1234.ts.net.",
                       "UserID": 4242, "OS": "linux", "Online": false },
                "b": { "HostName": "Devons-MacBook", "DNSName": "laptop.tail1234.ts.net.",
                       "UserID": 4242, "OS": "macOS", "Online": true },
                "c": { "HostName": "localhost", "DNSName": "iphone.tail1234.ts.net.",
                       "UserID": 4242, "OS": "iOS", "Online": true },
                "d": { "HostName": "shared", "DNSName": "shared.other.ts.net.",
                       "UserID": 99, "OS": "linux", "Online": true },
            },
        });
        assert_eq!(
            own_machines(&status),
            vec![
                Machine {
                    name: "laptop".into(),
                    os: "macOS".into(),
                    online: true
                },
                Machine {
                    name: "desktop".into(),
                    os: "linux".into(),
                    online: false
                },
            ]
        );
        // Logged out, or alone on the tailnet.
        assert_eq!(own_machines(&json!({ "BackendState": "Stopped" })), vec![]);
        assert_eq!(
            own_machines(&json!({ "Self": { "UserID": 4242 }, "Peer": null })),
            vec![]
        );
    }
}
