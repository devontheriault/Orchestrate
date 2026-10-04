//! The Mail Space (ADR 0017): the user's own mailbox, read on the Host over
//! IMAP. The server is the source of truth. The Host keeps only a cache it can
//! throw away (`cache`), and the account's credentials in a file only the user
//! can read (`account`).
//!
//! Mail starts narrow: reading, searching, and handing a thread to an Agent.
//! Archiving and marking read are allowed, since they are changes to the
//! user's own mailbox that the user can see and undo. Nothing here sends mail:
//! anything that leaves the machine is the user's to send, and that comes later.
//!
//! The provider sits behind [`MailStore`], so JMAP can be a second
//! implementation beside [`imap`] without the rest of the module noticing.
//! What a window sees of a message is decided here and in [`parse`]: its HTML
//! is untrusted, so only [`sanitize`]d HTML ever leaves the Host.

mod account;
mod cache;
mod imap;
mod oauth;
mod parse;
pub mod sanitize;
mod thread;

#[cfg(test)]
mod tests;

use std::sync::{Arc, LazyLock, Mutex};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, RwLock};

pub use account::Setup;

/// What a mail call fails with: a sentence for the user.
pub type Result<T> = std::result::Result<T, String>;

/// What a folder is for, from the server's special-use attributes (RFC 6154)
/// or, on a server that has none, from its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Inbox,
    /// Gmail's Important, which only Gmail has.
    Important,
    Flagged,
    Drafts,
    Sent,
    Archive,
    /// Every message in the account: Gmail's All Mail.
    All,
    Junk,
    Trash,
}

/// One folder, or on Gmail one label, as the folder column lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    /// The server's name for it, which every other call names it by.
    pub path: String,
    /// Its own name, decoded, without its parents'.
    pub name: String,
    pub role: Option<Role>,
    /// How many folders up it sits, for indenting.
    pub depth: u32,
    pub unread: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

/// What the message list knows of a message without opening it. Kept in the
/// cache too: a message's headers never change, only its flags do.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub uid: u32,
    pub folder: String,
    pub message_id: Option<String>,
    /// The ids it answers, oldest first: References, then In-Reply-To.
    #[serde(default)]
    pub references: Vec<String>,
    /// Gmail's own thread id, where the server is Gmail.
    #[serde(default)]
    pub gm_thread: Option<u64>,
    pub subject: String,
    pub from: Option<Address>,
    #[serde(default)]
    pub to: Vec<Address>,
    /// Unix seconds.
    pub date: i64,
    pub snippet: String,
    pub unread: bool,
    pub flagged: bool,
    /// It carries files, as far as its structure says before it is opened.
    pub attachments: bool,
    /// Gmail's labels on it, other than the folder it is listed in.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// A conversation: the messages in one folder that answer each other.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Thread {
    pub id: String,
    pub subject: String,
    /// Oldest first.
    pub messages: Vec<Summary>,
    pub unread: usize,
    /// Its newest message's.
    pub date: i64,
}

/// A file attached to a message, listed for the user to save. Never opened.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AttachmentInfo {
    /// Its place among the message's attachments, for asking for it.
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
}

/// An opened message, as the reading pane shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Message {
    pub uid: u32,
    pub folder: String,
    pub subject: String,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub date: i64,
    /// Sanitized HTML, for a sandboxed frame. Never the sender's own.
    pub html: Option<String>,
    /// The plain-text body, or the HTML's text where it has none.
    pub text: String,
    /// Remote images left out of `html`. The window offers to load them.
    pub remote_images: usize,
    pub attachments: Vec<AttachmentInfo>,
}

/// One attachment's bytes, base64, for the window to save where the user says.
#[derive(Debug, Clone, Serialize)]
pub struct Download {
    pub name: String,
    pub mime: String,
    pub data: String,
}

/// The account, as the window may know it: never its secrets.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AccountInfo {
    pub email: String,
    /// `"google"` for Gmail signed in with OAuth, `"password"` otherwise.
    pub auth: String,
    pub server: String,
}

/// Where Mail stands, for the window to show.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    pub account: Option<AccountInfo>,
    /// `"none"`, `"signing_in"` (waiting on the browser), `"connecting"`,
    /// `"connected"` or `"error"`.
    pub state: &'static str,
    pub error: Option<String>,
}

/// What setting an account up needs next.
#[derive(Debug, Serialize)]
#[serde(tag = "next", rename_all = "snake_case")]
pub enum SetUpOutcome {
    Connected(Status),
    /// The user signs in at `url` in their browser; Mail reports the outcome
    /// with a `mail-changed` event.
    Browser {
        url: String,
    },
}

pub type Fut<'a, T> = BoxFuture<'a, Result<T>>;

/// A mail provider. IMAP is the one there is; JMAP would be another.
pub trait MailStore: Send + Sync {
    fn folders(&self) -> Fut<'_, Vec<Folder>>;
    /// The newest `limit` messages in `folder`, newest first.
    fn recent<'a>(&'a self, folder: &'a str, limit: usize) -> Fut<'a, Vec<Summary>>;
    /// Messages in `folder` the server finds for `query`, newest first.
    fn search<'a>(&'a self, folder: &'a str, query: &'a str, limit: usize)
        -> Fut<'a, Vec<Summary>>;
    /// Messages in `folder` that Gmail files under thread `thread`. Empty on
    /// any other server.
    fn gmail_thread<'a>(&'a self, folder: &'a str, thread: u64) -> Fut<'a, Vec<Summary>>;
    /// A message as the server holds it, RFC 5322 bytes.
    fn raw<'a>(&'a self, folder: &'a str, uid: u32) -> Fut<'a, Vec<u8>>;
    fn set_seen<'a>(&'a self, folder: &'a str, uids: &'a [u32], seen: bool) -> Fut<'a, ()>;
    /// Move messages to the folder with `role`, making an Archive if the
    /// server has none.
    fn move_to<'a>(&'a self, folder: &'a str, uids: &'a [u32], role: Role) -> Fut<'a, ()>;
    /// Watch for new mail until dropped, calling `changed` with the folder.
    fn watch(&self, changed: Arc<dyn Fn(&str) + Send + Sync>) -> tokio::task::JoinHandle<()>;
}

/// The one mail account a Host holds, and what is happening with it.
struct Mail {
    store: RwLock<Option<Arc<dyn MailStore>>>,
    status: Mutex<Status>,
    watcher: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// `mail-changed` events, for the Host to pass to every window.
    events: broadcast::Sender<Value>,
}

static MAIL: LazyLock<Mail> = LazyLock::new(|| Mail {
    store: RwLock::new(None),
    status: Mutex::new(Status {
        account: None,
        state: "none",
        error: None,
    }),
    watcher: Mutex::new(None),
    events: broadcast::channel(64).0,
});

/// The events Mail raises, as `(name, payload)` for the Host to relay: just
/// `mail-changed`, with the folder that changed or `null` for the account.
pub fn events() -> broadcast::Receiver<Value> {
    MAIL.events.subscribe()
}

fn announce(folder: Option<&str>) {
    let _ = MAIL.events.send(json!({ "folder": folder }));
}

fn set_status(state: &'static str, error: Option<String>) {
    let mut status = MAIL.status.lock().unwrap_or_else(|e| e.into_inner());
    status.state = state;
    status.error = error;
}

/// Connect the saved account, if there is one. Called as the Host starts, so
/// new mail is watched for whether or not a window is open.
pub fn start() {
    tokio::spawn(async {
        if let Err(e) = connect_saved().await {
            eprintln!("mail: {e}");
        }
    });
}

async fn connect_saved() -> Result<()> {
    let Some(saved) = account::load()? else {
        return Ok(());
    };
    {
        let mut status = MAIL.status.lock().unwrap_or_else(|e| e.into_inner());
        status.account = Some(saved.info());
        status.state = "connecting";
    }
    let store: Arc<dyn MailStore> = Arc::new(imap::Imap::new(saved));
    // The folder list is the cheapest call that proves the account works.
    match store.folders().await {
        Ok(_) => set_status("connected", None),
        Err(e) => {
            set_status("error", Some(e.clone()));
            announce(None);
            // Keep the store anyway: the next call retries, and the watcher
            // keeps trying, so a laptop that wakes offline recovers by itself.
        }
    }
    let changed: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|folder| announce(Some(folder)));
    let watcher = store.watch(changed);
    if let Some(old) = MAIL
        .watcher
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(watcher)
    {
        old.abort();
    }
    *MAIL.store.write().await = Some(store);
    announce(None);
    Ok(())
}

async fn store() -> Result<Arc<dyn MailStore>> {
    MAIL.store
        .read()
        .await
        .clone()
        .ok_or_else(|| "no mail account is set up".to_string())
}

/// Run a call against the store, noting on the way whether the account works.
async fn with_store<T, F>(f: impl FnOnce(Arc<dyn MailStore>) -> F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    let store = store().await?;
    let outcome = f(store).await;
    let state = MAIL.status.lock().unwrap_or_else(|e| e.into_inner()).state;
    match &outcome {
        Ok(_) if state != "connected" => {
            set_status("connected", None);
            announce(None);
        }
        Err(e) if imap::is_connection_error(e) && state != "error" => {
            set_status("error", Some(e.clone()));
            announce(None);
        }
        _ => {}
    }
    outcome
}

pub fn status() -> Status {
    MAIL.status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Set the account up. A password account is tried before it is saved, so a
/// typo is said now. Gmail with OAuth answers with the page to sign in on.
pub async fn set_up(setup: Setup) -> Result<SetUpOutcome> {
    match setup {
        Setup::Password(p) => {
            let saved = account::Saved::password(p)?;
            imap::Imap::new(saved.clone()).folders().await?;
            account::save(&saved)?;
            cache::clear()?;
            connect_saved().await?;
            Ok(SetUpOutcome::Connected(status()))
        }
        Setup::Google(g) => {
            let url = oauth::begin(g, |outcome| async move {
                match outcome {
                    Ok(saved) => {
                        let saved_ok = account::save(&saved).and_then(|()| cache::clear());
                        if let Err(e) = saved_ok {
                            set_status("error", Some(e));
                            announce(None);
                        } else if let Err(e) = connect_saved().await {
                            set_status("error", Some(e));
                            announce(None);
                        }
                    }
                    Err(e) => {
                        set_status(
                            if account_info().is_some() {
                                "error"
                            } else {
                                "none"
                            },
                            Some(e),
                        );
                        announce(None);
                    }
                }
            })
            .await?;
            set_status("signing_in", None);
            Ok(SetUpOutcome::Browser { url })
        }
    }
}

fn account_info() -> Option<AccountInfo> {
    MAIL.status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .account
        .clone()
}

/// Forget the account: its credentials and everything cached from it. The
/// mailbox itself is untouched.
pub async fn sign_out() -> Result<()> {
    if let Some(w) = MAIL
        .watcher
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        w.abort();
    }
    *MAIL.store.write().await = None;
    account::remove()?;
    cache::clear()?;
    *MAIL.status.lock().unwrap_or_else(|e| e.into_inner()) = Status {
        account: None,
        state: "none",
        error: None,
    };
    announce(None);
    Ok(())
}

pub async fn folders() -> Result<Vec<Folder>> {
    with_store(|s| async move { s.folders().await }).await
}

/// How many messages a folder's list reads, newest first. Older mail is
/// reached by search.
const LIST_LIMIT: usize = 300;

/// How many messages a search returns.
const SEARCH_LIMIT: usize = 150;

pub async fn threads(folder: &str) -> Result<Vec<Thread>> {
    with_store(|s| async move { Ok(thread::group(s.recent(folder, LIST_LIMIT).await?)) }).await
}

/// The server's own search, in `folder`, or with none everywhere it can look:
/// Gmail's All Mail, or else the Inbox, the Archive and Sent.
pub async fn search(query: &str, folder: Option<&str>) -> Result<Vec<Thread>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    with_store(|s| async move {
        let scope = match folder {
            Some(f) => vec![f.to_string()],
            None => everywhere(&s.folders().await?),
        };
        let mut found = Vec::new();
        for f in &scope {
            found.extend(s.search(f, query, SEARCH_LIMIT).await?);
        }
        found.sort_by(|a, b| b.date.cmp(&a.date));
        found.truncate(SEARCH_LIMIT);
        Ok(thread::group(found))
    })
    .await
}

/// The folders a search with no folder looks in.
fn everywhere(folders: &[Folder]) -> Vec<String> {
    if let Some(all) = folders.iter().find(|f| f.role == Some(Role::All)) {
        return vec![all.path.clone()];
    }
    [Role::Inbox, Role::Archive, Role::Sent]
        .iter()
        .filter_map(|r| folders.iter().find(|f| f.role == Some(*r)))
        .map(|f| f.path.clone())
        .collect()
}

/// The unread conversations in the Inbox, newest first.
pub async fn unread() -> Result<Vec<Thread>> {
    let inbox = with_store(|s| async move { inbox(&s.folders().await?) }).await?;
    Ok(threads(&inbox)
        .await?
        .into_iter()
        .filter(|t| t.unread > 0)
        .collect())
}

fn inbox(folders: &[Folder]) -> Result<String> {
    folders
        .iter()
        .find(|f| f.role == Some(Role::Inbox))
        .map(|f| f.path.clone())
        .ok_or_else(|| "this account has no Inbox".to_string())
}

/// Open a message, marking it read. Its HTML is sanitized, with remote images
/// left out unless `images`.
pub async fn message(folder: &str, uid: u32, images: bool) -> Result<Message> {
    with_store(|s| async move {
        let raw = s.raw(folder, uid).await?;
        let message = parse::message(&raw, folder, uid, images)
            .ok_or_else(|| "this message can't be read".to_string())?;
        s.set_seen(folder, &[uid], true).await?;
        Ok(message)
    })
    .await
}

pub async fn set_seen(folder: &str, uids: &[u32], seen: bool) -> Result<()> {
    with_store(|s| async move { s.set_seen(folder, uids, seen).await }).await
}

pub async fn archive(folder: &str, uids: &[u32]) -> Result<()> {
    with_store(|s| async move { s.move_to(folder, uids, Role::Archive).await }).await
}

pub async fn trash(folder: &str, uids: &[u32]) -> Result<()> {
    with_store(|s| async move { s.move_to(folder, uids, Role::Trash).await }).await
}

pub async fn attachment(folder: &str, uid: u32, index: usize) -> Result<Download> {
    with_store(|s| async move {
        let raw = s.raw(folder, uid).await?;
        parse::attachment(&raw, index).ok_or_else(|| "that attachment isn't there".to_string())
    })
    .await
}

/// A conversation as plain text, oldest message first, each quoted: what an
/// Agent is handed, as its Task or through MCP. Not marked read, since nobody
/// has read it yet. On Gmail the whole conversation is read from All Mail, so
/// the user's own replies are in it too.
pub async fn thread_text(folder: &str, uids: &[u32]) -> Result<String> {
    with_store(|s| async move {
        let mut messages: Vec<(String, u32)> =
            uids.iter().map(|u| (folder.to_string(), *u)).collect();
        let folders = s.folders().await?;
        if let Some(all) = folders.iter().find(|f| f.role == Some(Role::All)) {
            let first = s.recent(folder, LIST_LIMIT).await?;
            let thread = first
                .iter()
                .find(|m| uids.contains(&m.uid))
                .and_then(|m| m.gm_thread);
            if let Some(thread) = thread {
                let whole = s.gmail_thread(&all.path, thread).await?;
                if !whole.is_empty() {
                    messages = whole.iter().map(|m| (all.path.clone(), m.uid)).collect();
                }
            }
        }
        let mut parsed = Vec::new();
        for (f, uid) in &messages {
            let raw = s.raw(f, *uid).await?;
            if let Some(m) = parse::message(&raw, f, *uid, false) {
                parsed.push(m);
            }
        }
        parsed.sort_by_key(|m| m.date);
        Ok(parse::quote_thread(&parsed))
    })
    .await
}

/// A conversation by its id, as [`threads`] or [`search`] gave it, for a
/// caller that holds only the id: an Agent over MCP.
pub async fn thread_text_by_id(folder: &str, id: &str) -> Result<String> {
    let found = threads(folder).await?.into_iter().find(|t| t.id == id);
    let Some(thread) = found else {
        return Err(format!("no conversation {id} in {folder}"));
    };
    let uids: Vec<u32> = thread.messages.iter().map(|m| m.uid).collect();
    thread_text(folder, &uids).await
}
