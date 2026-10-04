//! The one mail account a Host holds, and where its credentials live: a file
//! in the state directory that only the user can read (ADR 0017). That file is
//! trusted for the same reason the Host's socket is: its permissions are the
//! trust.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{AccountInfo, Result};
use crate::paths;

/// How the user asked to connect, as the window sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "auth", rename_all = "snake_case")]
pub enum Setup {
    /// Any IMAP server, with a password: for Gmail, iCloud, Fastmail and
    /// Outlook, an app password made in the account's security settings.
    Password(PasswordSetup),
    /// Gmail, signed in with Google in the browser.
    Google(GoogleSetup),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordSetup {
    pub email: String,
    pub server: String,
    pub port: u16,
    pub security: Security,
    /// The login, where it isn't the address.
    #[serde(default)]
    pub username: Option<String>,
    pub password: String,
}

/// The user's own OAuth client: Google hands one out free to anyone, for
/// their own account, from the Cloud console.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSetup {
    pub email: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    /// TLS from the first byte, as port 993 expects.
    Tls,
    /// No encryption at all. Only to this machine, for a test server.
    Plain,
}

/// The account as it is kept on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Saved {
    pub email: String,
    pub server: String,
    pub port: u16,
    pub security: Security,
    pub username: String,
    pub secret: Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Secret {
    Password {
        password: String,
    },
    Google {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
}

/// Gmail's IMAP server, which a Google sign-in always connects to.
pub const GMAIL_SERVER: &str = "imap.gmail.com";

impl Saved {
    pub fn password(p: PasswordSetup) -> Result<Self> {
        let email = p.email.trim().to_string();
        let server = p.server.trim().to_string();
        if email.is_empty() || server.is_empty() || p.password.is_empty() {
            return Err("the address, the server and the password are all needed".into());
        }
        if p.security == Security::Plain && !is_loopback(&server) {
            return Err(format!(
                "{server} would be reached without encryption, so your password would cross \
                 the network readable; only a test server on this machine may be"
            ));
        }
        let username = p
            .username
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| email.clone());
        Ok(Self {
            email,
            server,
            port: p.port,
            security: p.security,
            username,
            secret: Secret::Password {
                password: p.password,
            },
        })
    }

    pub fn google(
        email: String,
        client_id: String,
        client_secret: String,
        refresh_token: String,
    ) -> Self {
        Self {
            username: email.clone(),
            email,
            server: GMAIL_SERVER.into(),
            port: 993,
            security: Security::Tls,
            secret: Secret::Google {
                client_id,
                client_secret,
                refresh_token,
            },
        }
    }

    pub fn info(&self) -> AccountInfo {
        AccountInfo {
            email: self.email.clone(),
            auth: match self.secret {
                Secret::Password { .. } => "password",
                Secret::Google { .. } => "google",
            }
            .into(),
            server: self.server.clone(),
        }
    }
}

/// Whether `server` names this machine, the only place plain IMAP may go.
pub fn is_loopback(server: &str) -> bool {
    let host = server.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Where Mail keeps everything: the account, and the cache under it.
pub fn dir() -> Result<PathBuf> {
    Ok(paths::state_dir().map_err(|e| e.to_string())?.join("mail"))
}

fn file() -> Result<PathBuf> {
    Ok(dir()?.join("account.json"))
}

pub fn load() -> Result<Option<Saved>> {
    let path = file()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    // Narrowed again if something widened it: a copy, a restore from backup.
    private(&path, 0o600)?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{} is not an account this app wrote: {e}", path.display()))
}

/// Write the account where only the user can read it. Written whole to a new
/// file that is private from its first byte, then moved over the old one, so
/// it is never readable by anyone else, even for a moment, and never half
/// written.
pub fn save(saved: &Saved) -> Result<()> {
    let dir = dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    private(&dir, 0o700)?;
    let path = file()?;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let json = serde_json::to_vec_pretty(saved).map_err(|e| e.to_string())?;
    write_private(&tmp, &json).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// On Windows a file in the user's own profile is private to them already.
#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(unix)]
fn private(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.permissions().mode() & 0o777 != mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("could not make {} private: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn private(_: &std::path::Path, _: u32) -> Result<()> {
    Ok(())
}

pub fn remove() -> Result<()> {
    match std::fs::remove_file(file()?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not forget the account: {e}")),
    }
}
