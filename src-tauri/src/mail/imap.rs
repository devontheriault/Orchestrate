//! The account over IMAP: one connection for the user's calls, kept open and
//! taken in turn, and a second that waits in IDLE on the Inbox so new mail
//! shows up without polling.
//!
//! Most fetches are written out by hand and read back response by response
//! (see [`fetch`]) rather than through async-imap's `Fetch`, which has no way
//! to read Gmail's thread id.

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use async_imap::extensions::idle::IdleResponse;
use async_imap::imap_proto::{
    AttributeValue, MailboxDatum, MessageSection, NameAttribute, Response, SectionPath, Status,
};
use async_imap::{Authenticator, Client, Session};
use base64::Engine;
use futures_util::TryStreamExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::MutexGuard;
use tokio_rustls::rustls;

use super::account::{Saved, Secret, Security};
use super::cache::{self, FolderCache};
use super::parse::{self, Listed};
use super::{oauth, Folder, Fut, MailStore, Result, Role, Summary};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// A connection unused for this long is checked with NOOP before it is
/// trusted: servers drop idle connections, often without a word.
const STALE_AFTER: Duration = Duration::from_secs(60);
/// How long one IDLE lasts before it is renewed. Under the 29 minutes RFC
/// 2177 allows, and under what home routers let a silent connection live.
const IDLE_FOR: Duration = Duration::from_secs(10 * 60);
/// Without IDLE, how often the Inbox is checked.
const POLL_EVERY: Duration = Duration::from_secs(120);
/// How much of each body the list reads, for its snippet.
const SNIPPET_BYTES: u32 = 4096;
/// On a server that can't do partial fetches, the largest message whose
/// whole text is read for its snippet.
const WHOLE_TEXT_MAX: u32 = 64 * 1024;
/// Messages fetched per command.
const BATCH: usize = 100;
/// Folders asked for their unread count. Gmail accounts can have hundreds
/// of labels, and each is a round trip.
const COUNTED_FOLDERS: usize = 80;

/// What every error that means "the server can't be reached" starts with, so
/// the Mail Space can tell being offline from being refused.
const OFFLINE: &str = "can't reach the mail server";

/// Said of an answer the IMAP parser couldn't read. Some servers get the
/// syntax of partial fetches wrong (GreenMail leaves out the space in
/// `BODY[TEXT]<0> {n}`), so a call that hits one is tried once more without
/// them.
const UNREADABLE: &str = "it sent an answer this app can't read";

pub fn is_connection_error(e: &str) -> bool {
    e.starts_with(OFFLINE)
}

fn offline(e: impl std::fmt::Display) -> String {
    format!("{OFFLINE}: {e}")
}

/// An error from a command: the connection failing, or the server saying no.
fn failed(e: async_imap::error::Error) -> String {
    use async_imap::error::Error as E;
    match e {
        E::Io(_) | E::ConnectionLost => offline(e),
        E::No(why) | E::Bad(why) => format!("the mail server refused: {why}"),
        other => format!("the mail server's answer made no sense: {other}"),
    }
}

/// The connection, with TLS or (to this machine only) without.
#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Gmail's sign-in over IMAP: the address and an access token.
struct XOAuth2 {
    response: Option<String>,
}

impl Authenticator for XOAuth2 {
    type Response = String;
    /// The token the first time. If the server answers with an error challenge
    /// instead of accepting, an empty reply ends the exchange so the error
    /// comes back as the command's result.
    fn process(&mut self, _challenge: &[u8]) -> String {
        self.response.take().unwrap_or_default()
    }
}

/// One signed-in connection, and what it knows about the server.
struct Conn {
    session: Session<Stream>,
    gmail: bool,
    has_move: bool,
    uidplus: bool,
    idle: bool,
    /// Whether the server can be trusted with partial fetches.
    partial: bool,
    /// The folder selected, and its UIDVALIDITY.
    selected: Option<(String, u32)>,
    used: Instant,
}

async fn connect(account: &Saved) -> Result<Conn> {
    let tcp = tokio::time::timeout(
        CONNECT_TIMEOUT,
        TcpStream::connect((account.server.as_str(), account.port)),
    )
    .await
    .map_err(|_| offline(format!("{} didn't answer", account.server)))?
    .map_err(offline)?;
    let stream = match account.security {
        Security::Plain => Stream::Plain(tcp),
        Security::Tls => Stream::Tls(Box::new(tls(&account.server, tcp).await?)),
    };
    let mut client = Client::new(stream);
    let sasl_ir = match client.read_response().await {
        Ok(Some(greeting)) => !lacks_sasl_ir(greeting.parsed()),
        Ok(None) => return Err(offline("it hung up before saying hello")),
        Err(e) => return Err(offline(e)),
    };
    let mut session = match &account.secret {
        Secret::Password { password } => client
            .login(&account.username, password)
            .await
            .map_err(|(e, _)| refused_login(account, e))?,
        Secret::OAuth { .. } => {
            let token = oauth::access_token(account).await?;
            let response = format!("user={}\x01auth=Bearer {token}\x01\x01", account.username);
            // With SASL-IR the token rides on the command itself, which some
            // servers insist on; without it, it answers the server's "+".
            let (command, auth) = if sasl_ir {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&response);
                (format!("XOAUTH2 {encoded}"), XOAuth2 { response: None })
            } else {
                (
                    "XOAUTH2".to_string(),
                    XOAuth2 {
                        response: Some(response),
                    },
                )
            };
            client
                .authenticate(command, auth)
                .await
                .map_err(|(e, _)| refused_login(account, e))?
        }
    };
    let caps = session.capabilities().await.map_err(failed)?;
    Ok(Conn {
        gmail: caps.has_str("X-GM-EXT-1"),
        has_move: caps.has_str("MOVE"),
        uidplus: caps.has_str("UIDPLUS"),
        idle: caps.has_str("IDLE"),
        partial: true,
        session,
        selected: None,
        used: Instant::now(),
    })
}

fn refused_login(account: &Saved, e: async_imap::error::Error) -> String {
    use async_imap::error::Error as E;
    match e {
        E::Io(_) | E::ConnectionLost => offline(e),
        E::No(why) | E::Bad(why) => {
            let password = matches!(account.secret, Secret::Password { .. });
            let hint = login_hint(&account.server, password);
            format!(
                "{} refused the sign-in for {}: {why}.{hint}",
                account.server, account.username
            )
        }
        other => format!("could not sign in to {}: {other}", account.server),
    }
}

/// Whether a server's greeting lists its capabilities without SASL-IR, the
/// one case where a sign-in can't send its first answer with the command.
fn lacks_sasl_ir(greeting: &Response<'_>) -> bool {
    use async_imap::imap_proto::{Capability, ResponseCode};
    let Response::Data { outcome, .. } = greeting else {
        return false;
    };
    match &outcome.code {
        Some(ResponseCode::Capabilities(caps)) => !caps
            .iter()
            .any(|c| matches!(c, Capability::Atom(a) if a.eq_ignore_ascii_case("SASL-IR"))),
        _ => false,
    }
}

/// What to try when a provider refuses a sign-in.
pub fn login_hint(server: &str, password: bool) -> &'static str {
    let server = server.to_ascii_lowercase();
    let microsoft = server.ends_with("office365.com") || server.ends_with("outlook.com");
    match (microsoft, password) {
        (true, true) => {
            " Microsoft no longer lets other apps sign in to Outlook with a password; \
             sign in with Microsoft instead."
        }
        (true, false) => {
            " Check that IMAP is on for this mailbox: in Outlook.com, Settings → Mail → \
             Forwarding and IMAP; for a work account, your admin must allow it."
        }
        (false, true) if server.ends_with("gmail.com") => {
            " Gmail takes an app password here, not your Google password."
        }
        _ => "",
    }
}

async fn tls(server: &str, tcp: TcpStream) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_platform_verifier())
        .map_err(|e| format!("could not set up TLS: {e}"))?
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(server.to_string())
        .map_err(|_| format!("{server} isn't a server name"))?;
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| offline(format!("TLS with {server} failed: {e}")))
}

/// One message as a FETCH described it.
#[derive(Debug, Default)]
struct Fetched {
    uid: u32,
    flags: Vec<String>,
    header: Option<Vec<u8>>,
    text: Option<Vec<u8>>,
    body: Option<Vec<u8>>,
    arrived: Option<i64>,
    size: u32,
    gm_thread: Option<u64>,
    labels: Option<Vec<String>>,
}

/// Run a FETCH (or UID FETCH) and read back every message it describes.
async fn fetch(session: &mut Session<Stream>, command: &str) -> Result<Vec<Fetched>> {
    let tag = session.run_command(command).await.map_err(failed)?;
    let mut out = Vec::new();
    loop {
        let response = match session.read_response().await {
            Ok(Some(r)) => r,
            Ok(None) => return Err(offline("the connection closed")),
            Err(e) if e.to_string().contains("during parsing") => {
                // The stream can't be trusted past this; the connection goes.
                return Err(offline(format!("{UNREADABLE} ({command})")));
            }
            Err(e) => return Err(offline(e)),
        };
        match response.parsed() {
            Response::Fetch(_, attrs) => {
                let mut f = Fetched::default();
                for attr in attrs {
                    match attr {
                        AttributeValue::Uid(u) => f.uid = *u,
                        AttributeValue::Flags(flags) => {
                            f.flags = flags.iter().map(|s| s.to_string()).collect()
                        }
                        AttributeValue::BodySection {
                            section,
                            data: Some(data),
                            ..
                        } => {
                            let data = data.to_vec();
                            match section {
                                Some(SectionPath::Full(MessageSection::Header)) => {
                                    f.header = Some(data)
                                }
                                Some(SectionPath::Full(MessageSection::Text)) => {
                                    f.text = Some(data)
                                }
                                None => f.body = Some(data),
                                _ => {}
                            }
                        }
                        AttributeValue::InternalDate(d) => f.arrived = internal_date(d),
                        AttributeValue::Rfc822Size(n) => f.size = *n,
                        AttributeValue::GmailThrId(t) => f.gm_thread = Some(*t),
                        AttributeValue::GmailLabels(l) => {
                            f.labels = Some(l.iter().map(|s| s.to_string()).collect())
                        }
                        _ => {}
                    }
                }
                // A FETCH the server sends on its own, about flags, carries no
                // UID unless asked; it isn't one of ours.
                if f.uid != 0 {
                    out.push(f);
                }
            }
            Response::Done {
                tag: t,
                status,
                outcome,
            } if *t == tag => {
                return match status {
                    Status::Ok => Ok(out),
                    _ => Err(format!(
                        "the mail server refused: {}",
                        outcome.information.as_deref().unwrap_or("no reason given")
                    )),
                };
            }
            _ => {}
        }
    }
}

/// IMAP's `INTERNALDATE`, `"04-Oct-2026 13:41:52 +0000"`, as Unix seconds.
pub fn internal_date(s: &str) -> Option<i64> {
    let mut parts = s.split_whitespace();
    let (date, clock, zone) = (parts.next()?, parts.next()?, parts.next()?);
    let mut d = date.split('-');
    let day: u8 = d.next()?.parse().ok()?;
    let month = match d.next()?.to_ascii_lowercase().as_str() {
        "jan" => time::Month::January,
        "feb" => time::Month::February,
        "mar" => time::Month::March,
        "apr" => time::Month::April,
        "may" => time::Month::May,
        "jun" => time::Month::June,
        "jul" => time::Month::July,
        "aug" => time::Month::August,
        "sep" => time::Month::September,
        "oct" => time::Month::October,
        "nov" => time::Month::November,
        "dec" => time::Month::December,
        _ => return None,
    };
    let year: i32 = d.next()?.parse().ok()?;
    let mut c = clock.split(':').map(|n| n.parse::<u8>().ok());
    let (h, m, sec) = (c.next()??, c.next()??, c.next()??);
    let sign = match zone.get(..1)? {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let zh: i64 = zone.get(1..3)?.parse().ok()?;
    let zm: i64 = zone.get(3..5)?.parse().ok()?;
    let local = time::Date::from_calendar_date(year, month, day)
        .ok()?
        .with_hms(h, m, sec)
        .ok()?
        .assume_utc()
        .unix_timestamp();
    Some(local - sign * (zh * 3600 + zm * 60))
}

/// UIDs in IMAP's set syntax, runs folded: `1:4,7,9:10`.
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        out.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        i += 1;
    }
    out.join(",")
}

/// A string as an IMAP quoted string. Line breaks and other control
/// characters are dropped rather than escaped: IMAP has no escape for them,
/// and a line break would end the command and start another.
pub fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars().filter(|c| !c.is_control()) {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A search the user typed, as IMAP SEARCH criteria. Words must all appear;
/// `from:`, `to:` and `subject:` narrow a word to that header. Gmail is
/// handed the text whole, in its own search syntax.
pub fn search_criteria(query: &str, gmail: bool) -> String {
    let utf8 = if query.is_ascii() {
        ""
    } else {
        "CHARSET UTF-8 "
    };
    if gmail {
        return format!("{utf8}X-GM-RAW {}", quoted(query));
    }
    let terms: Vec<String> = words(query)
        .into_iter()
        .map(|w| {
            let lower = w.to_ascii_lowercase();
            for (prefix, key) in [("from:", "FROM"), ("to:", "TO"), ("subject:", "SUBJECT")] {
                if lower.starts_with(prefix) && w.len() > prefix.len() {
                    return format!("{key} {}", quoted(&w[prefix.len()..]));
                }
            }
            format!("TEXT {}", quoted(&w))
        })
        .collect();
    if terms.is_empty() {
        return "ALL".into();
    }
    format!("{utf8}{}", terms.join(" "))
}

/// Words, keeping a "quoted phrase" as one.
fn words(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in query.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// A folder name as IMAP sends it, in its modified UTF-7 (RFC 3501 5.1.3),
/// decoded: `Entw&APw-rfe` is `Entwürfe`.
pub fn decode_folder_name(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(dash) = after.find('-') else {
            out.push_str(&rest[amp..]);
            return out;
        };
        let encoded = &after[..dash];
        if encoded.is_empty() {
            out.push('&');
        } else {
            let b64 = encoded.replace(',', "/");
            let decoded = base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.as_bytes());
            match decoded {
                Ok(bytes) if bytes.len() % 2 == 0 => {
                    let units: Vec<u16> = bytes
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                _ => out.push_str(&rest[amp..amp + 2 + dash]),
            }
        }
        rest = &after[dash + 1..];
    }
    out.push_str(rest);
    out
}

/// Each folder's role: the server's special-use flag, or for a server with
/// none, the usual names.
pub fn role_of(path: &str, attrs: &[String]) -> Option<Role> {
    if path.eq_ignore_ascii_case("INBOX") {
        return Some(Role::Inbox);
    }
    for a in attrs {
        let role = match a.to_ascii_lowercase().as_str() {
            "\\all" => Role::All,
            "\\archive" => Role::Archive,
            "\\drafts" => Role::Drafts,
            "\\flagged" => Role::Flagged,
            "\\junk" => Role::Junk,
            "\\sent" => Role::Sent,
            "\\trash" => Role::Trash,
            "\\important" => Role::Important,
            _ => continue,
        };
        return Some(role);
    }
    None
}

fn role_by_name(name: &str) -> Option<Role> {
    match name.to_ascii_lowercase().as_str() {
        "sent" | "sent items" | "sent messages" | "sent mail" => Some(Role::Sent),
        "drafts" => Some(Role::Drafts),
        "trash" | "deleted items" | "deleted messages" | "bin" => Some(Role::Trash),
        "junk" | "spam" | "junk e-mail" | "junk email" => Some(Role::Junk),
        "archive" | "archives" => Some(Role::Archive),
        _ => None,
    }
}

/// Folders as the server listed them: path, delimiter, attributes.
pub fn folders_from(list: Vec<(String, Option<String>, Vec<String>)>) -> Vec<Folder> {
    let mut folders: Vec<Folder> = list
        .iter()
        .filter(|(_, _, attrs)| {
            !attrs.iter().any(|a| {
                a.eq_ignore_ascii_case("\\Noselect") || a.eq_ignore_ascii_case("\\NonExistent")
            })
        })
        .map(|(path, delim, attrs)| {
            let segments: Vec<&str> = match delim {
                Some(d) if !d.is_empty() => path.split(d.as_str()).collect(),
                _ => vec![path.as_str()],
            };
            let name = if path.eq_ignore_ascii_case("INBOX") {
                "Inbox".to_string()
            } else {
                decode_folder_name(segments.last().copied().unwrap_or(path))
            };
            let role = role_of(path, attrs);
            Folder {
                path: path.clone(),
                name,
                // Gmail's own folders sit under "[Gmail]", which is no folder.
                depth: if role.is_some() {
                    0
                } else {
                    segments.len() as u32 - 1
                },
                role,
                unread: 0,
                total: 0,
            }
        })
        .collect();
    // Name the usual folders on a server that marks none of them.
    for f in folders.iter_mut() {
        if f.role.is_none() && f.depth == 0 {
            f.role = role_by_name(&f.name);
        }
    }
    let mut taken = HashSet::new();
    for f in folders.iter_mut() {
        if let Some(r) = f.role {
            if !taken.insert(r) {
                f.role = None;
            }
        }
    }
    sort_folders(&mut folders);
    folders
}

/// The folders with a role first, in the order mail apps use, then the rest
/// by path, so a folder's children follow it.
pub fn sort_folders(folders: &mut [Folder]) {
    const ORDER: [Role; 9] = [
        Role::Inbox,
        Role::Important,
        Role::Flagged,
        Role::Drafts,
        Role::Sent,
        Role::Archive,
        Role::All,
        Role::Junk,
        Role::Trash,
    ];
    folders.sort_by(|a, b| {
        let rank = |f: &Folder| {
            f.role
                .and_then(|r| ORDER.iter().position(|x| *x == r))
                .unwrap_or(ORDER.len())
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
}

/// Run `$body` with `$c` as the connection, taking turns on it with other
/// calls, and drop the connection if the call shows it is dead. A macro
/// rather than a function taking a closure, since an async closure borrowing
/// the connection can't yet be proved `Send`.
macro_rules! on_conn {
    ($imap:ident, |$c:ident| $body:block) => {{
        let mut guard = $imap.conn().await?;
        let $c = guard.as_mut().expect("connected above");
        let out = match tokio::time::timeout(COMMAND_TIMEOUT, async $body).await {
            Ok(out) => out,
            Err(_) => Err(offline("it stopped answering")),
        };
        settle(&mut guard, out)
    }};
}

/// Keep the connection for the next call, or drop it if `out` shows it died.
fn settle<T>(guard: &mut MutexGuard<'_, Option<Conn>>, out: Result<T>) -> Result<T> {
    match &out {
        Err(e) if is_connection_error(e) => **guard = None,
        _ => {
            if let Some(c) = guard.as_mut() {
                c.used = Instant::now();
            }
        }
    }
    out
}

pub struct Imap {
    account: Saved,
    conn: tokio::sync::Mutex<Option<Conn>>,
    /// The folders last listed, for finding the Archive or the Trash without
    /// asking every folder its count again.
    folders: Mutex<Vec<Folder>>,
    /// Set once the server has garbled a partial fetch.
    no_partial: AtomicBool,
    /// The last call made its own connection, rather than reusing one.
    fresh: AtomicBool,
}

impl Imap {
    pub fn new(account: Saved) -> Self {
        Self {
            account,
            conn: tokio::sync::Mutex::new(None),
            folders: Mutex::new(Vec::new()),
            no_partial: AtomicBool::new(false),
            fresh: AtomicBool::new(false),
        }
    }

    /// The connection, made or checked. The guard is held for the whole call,
    /// so calls take turns on the one connection.
    async fn conn(&self) -> Result<MutexGuard<'_, Option<Conn>>> {
        let mut guard = self.conn.lock().await;
        if let Some(c) = guard.as_mut() {
            if c.used.elapsed() > STALE_AFTER {
                let alive = tokio::time::timeout(CONNECT_TIMEOUT, c.session.noop()).await;
                if !matches!(alive, Ok(Ok(()))) {
                    *guard = None;
                }
            }
        }
        self.fresh.store(guard.is_none(), Ordering::Relaxed);
        if guard.is_none() {
            let mut c = connect(&self.account).await?;
            c.partial = !self.no_partial.load(Ordering::Relaxed);
            *guard = Some(c);
        }
        Ok(guard)
    }

    /// Run `call`, and once more on a new connection if the one it used
    /// turned out dead: servers drop quiet connections without a word, and
    /// that is no reason to tell the user. A connection that fails as it is
    /// made is not tried again, since the server is plainly unreachable. If
    /// the server's answer couldn't be read, the second try goes without
    /// partial fetches.
    async fn tolerant<T, F, Fut>(&self, call: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        match call().await {
            Err(e) if e.contains(UNREADABLE) && !self.no_partial.swap(true, Ordering::Relaxed) => {
                eprintln!("mail: {e}; trying again without partial fetches");
                call().await
            }
            Err(e) if is_connection_error(&e) && !self.fresh.load(Ordering::Relaxed) => {
                call().await
            }
            other => other,
        }
    }

    /// The folders as last listed, listing them if they never were.
    async fn known_folders(&self) -> Result<Vec<Folder>> {
        let known = self
            .folders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if !known.is_empty() {
            return Ok(known);
        }
        self.list_folders().await
    }

    async fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut folders = on_conn!(self, |c| {
            let names: Vec<_> = c
                .session
                .list(Some(""), Some("*"))
                .await
                .map_err(failed)?
                .try_collect()
                .await
                .map_err(failed)?;
            let list = names
                .iter()
                .map(|n| {
                    let attrs = n.attributes().iter().map(attribute_name).collect();
                    (
                        n.name().to_string(),
                        n.delimiter().map(str::to_string),
                        attrs,
                    )
                })
                .collect();
            let mut folders = folders_from(list);
            for f in folders.iter_mut().take(COUNTED_FOLDERS) {
                // A folder that won't say is listed without a count.
                if let Ok(mb) = c.session.status(&f.path, "(MESSAGES UNSEEN)").await {
                    f.total = mb.exists;
                    f.unread = mb.unseen.unwrap_or(0);
                }
            }
            Ok(folders)
        })?;

        sort_folders(&mut folders);
        *self.folders.lock().unwrap_or_else(|e| e.into_inner()) = folders.clone();
        Ok(folders)
    }
}

fn attribute_name(a: &NameAttribute<'_>) -> String {
    match a {
        NameAttribute::NoInferiors => "\\Noinferiors".into(),
        NameAttribute::NoSelect => "\\Noselect".into(),
        NameAttribute::Marked => "\\Marked".into(),
        NameAttribute::Unmarked => "\\Unmarked".into(),
        NameAttribute::All => "\\All".into(),
        NameAttribute::Archive => "\\Archive".into(),
        NameAttribute::Drafts => "\\Drafts".into(),
        NameAttribute::Flagged => "\\Flagged".into(),
        NameAttribute::Junk => "\\Junk".into(),
        NameAttribute::Sent => "\\Sent".into(),
        NameAttribute::Trash => "\\Trash".into(),
        NameAttribute::Extension(s) => s.to_string(),
        _ => String::new(),
    }
}

/// Select `folder` unless it is selected already, and say its UIDVALIDITY.
/// `fresh` selects it again anyway, which is how a client hears of mail that
/// arrived since.
async fn select(c: &mut Conn, folder: &str, fresh: bool) -> Result<(u32, u32)> {
    if !fresh {
        if let Some((f, validity)) = &c.selected {
            if f == folder {
                return Ok((*validity, 0));
            }
        }
    }
    c.selected = None;
    let mb = c.session.select(folder).await.map_err(failed)?;
    let validity = mb.uid_validity.unwrap_or(0);
    c.selected = Some((folder.to_string(), validity));
    Ok((validity, mb.exists))
}

/// Summaries for `uids` in the selected folder, from the cache where it has
/// them and the server where it doesn't. `listed` carries the flags and
/// labels just read, which are newer than the cache's.
async fn summaries(
    c: &mut Conn,
    folder: &str,
    cache: &mut FolderCache,
    listed: &[Fetched],
) -> Result<Vec<Summary>> {
    let missing: Vec<u32> = listed
        .iter()
        .map(|f| f.uid)
        .filter(|u| !cache.messages.contains_key(u))
        .collect();
    let gmail = if c.gmail {
        " X-GM-THRID X-GM-LABELS"
    } else {
        ""
    };
    let mut commands = Vec::new();
    for batch in missing.chunks(BATCH) {
        let head = "UID FLAGS INTERNALDATE BODY.PEEK[HEADER]";
        if c.partial {
            commands.push(format!(
                "UID FETCH {} ({head} BODY.PEEK[TEXT]<0.{SNIPPET_BYTES}>{gmail})",
                uid_set(batch)
            ));
            continue;
        }
        // Without partial fetches, only small messages bring their text.
        let sizes = fetch(
            &mut c.session,
            &format!("UID FETCH {} (UID RFC822.SIZE)", uid_set(batch)),
        )
        .await?;
        let (small, large): (Vec<_>, Vec<_>) = sizes.iter().partition(|f| f.size <= WHOLE_TEXT_MAX);
        let set = |fs: Vec<&Fetched>| uid_set(&fs.iter().map(|f| f.uid).collect::<Vec<_>>());
        if !small.is_empty() {
            commands.push(format!(
                "UID FETCH {} ({head} BODY.PEEK[TEXT]{gmail})",
                set(small)
            ));
        }
        if !large.is_empty() {
            commands.push(format!("UID FETCH {} ({head}{gmail})", set(large)));
        }
    }
    for command in commands {
        for f in fetch(&mut c.session, &command).await? {
            let summary = parse::summary(Listed {
                uid: f.uid,
                folder,
                header: f.header.as_deref().unwrap_or_default(),
                text_start: f.text.as_deref().unwrap_or_default(),
                flags: &f.flags,
                labels: labels(folder, f.labels.unwrap_or_default()),
                gm_thread: f.gm_thread,
                arrived: f.arrived,
            });
            cache.messages.insert(f.uid, summary);
        }
    }
    let mut out = Vec::with_capacity(listed.len());
    for f in listed {
        let deleted = f.flags.iter().any(|x| x.eq_ignore_ascii_case("\\Deleted"));
        if let Some(s) = cache.messages.get_mut(&f.uid) {
            s.unread = !f.flags.iter().any(|x| x.eq_ignore_ascii_case("\\Seen"));
            s.flagged = f.flags.iter().any(|x| x.eq_ignore_ascii_case("\\Flagged"));
            if let Some(l) = &f.labels {
                s.labels = labels(folder, l.clone());
            }
            if !deleted {
                out.push(s.clone());
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.uid));
    Ok(out)
}

/// Gmail's labels worth showing on a message: not its system ones, and not
/// the folder it is being listed in.
fn labels(folder: &str, mut labels: Vec<String>) -> Vec<String> {
    labels.retain(|l| !l.starts_with('\\') && l != folder);
    labels.iter_mut().for_each(|l| *l = decode_folder_name(l));
    labels
}

impl MailStore for Imap {
    fn folders(&self) -> Fut<'_, Vec<Folder>> {
        Box::pin(self.tolerant(move || self.list_folders()))
    }

    fn recent<'a>(&'a self, folder: &'a str, limit: usize) -> Fut<'a, Vec<Summary>> {
        Box::pin(self.tolerant(move || async move {
            on_conn!(self, |c| {
                let (validity, exists) = select(c, folder, true).await?;
                let mut cache = cache::load_folder(folder);
                if cache.validity != validity {
                    cache = FolderCache {
                        validity,
                        ..Default::default()
                    };
                }
                if exists == 0 {
                    cache.messages.clear();
                    cache::save_folder(folder, &mut cache);
                    return Ok(Vec::new());
                }
                let start = exists.saturating_sub(limit as u32 - 1).max(1);
                let gmail = if c.gmail { " X-GM-LABELS" } else { "" };
                let listed = fetch(
                    &mut c.session,
                    &format!("FETCH {start}:* (UID FLAGS{gmail})"),
                )
                .await?;
                // Anything cached from this stretch of the folder that the server
                // no longer lists was moved or deleted elsewhere.
                if let Some(oldest) = listed.iter().map(|f| f.uid).min() {
                    let present: HashSet<u32> = listed.iter().map(|f| f.uid).collect();
                    cache
                        .messages
                        .retain(|uid, _| *uid < oldest || present.contains(uid));
                }
                let out = summaries(c, folder, &mut cache, &listed).await?;
                cache::save_folder(folder, &mut cache);
                Ok(out)
            })
        }))
    }

    fn search<'a>(
        &'a self,
        folder: &'a str,
        query: &'a str,
        limit: usize,
    ) -> Fut<'a, Vec<Summary>> {
        Box::pin(self.tolerant(move || async move {
            on_conn!(self, |c| {
                let (validity, _) = select(c, folder, true).await?;
                let criteria = search_criteria(query, c.gmail);
                let found = c.session.uid_search(&criteria).await.map_err(failed)?;
                let mut uids: Vec<u32> = found.into_iter().collect();
                uids.sort_unstable_by(|a, b| b.cmp(a));
                uids.truncate(limit);
                listed_summaries(c, folder, validity, &uids).await
            })
        }))
    }

    fn gmail_thread<'a>(&'a self, folder: &'a str, thread: u64) -> Fut<'a, Vec<Summary>> {
        Box::pin(self.tolerant(move || async move {
            on_conn!(self, |c| {
                if !c.gmail {
                    return Ok(Vec::new());
                }
                let (validity, _) = select(c, folder, true).await?;
                let found = c
                    .session
                    .uid_search(format!("X-GM-THRID {thread}"))
                    .await
                    .map_err(failed)?;
                let uids: Vec<u32> = found.into_iter().collect();
                listed_summaries(c, folder, validity, &uids).await
            })
        }))
    }

    fn raw<'a>(&'a self, folder: &'a str, uid: u32) -> Fut<'a, Vec<u8>> {
        Box::pin(self.tolerant(move || async move {
            on_conn!(self, |c| {
                let (validity, _) = select(c, folder, false).await?;
                if let Some(bytes) = cache::load_body(folder, validity, uid) {
                    return Ok(bytes);
                }
                let fetched = fetch(
                    &mut c.session,
                    &format!("UID FETCH {uid} (UID BODY.PEEK[])"),
                )
                .await?;
                let body = fetched
                    .into_iter()
                    .find(|f| f.uid == uid)
                    .and_then(|f| f.body)
                    .ok_or_else(|| "that message is no longer in this folder".to_string())?;
                cache::save_body(folder, validity, uid, &body);
                Ok(body)
            })
        }))
    }

    fn set_seen<'a>(&'a self, folder: &'a str, uids: &'a [u32], seen: bool) -> Fut<'a, ()> {
        Box::pin(self.tolerant(move || async move {
            on_conn!(self, |c| {
                if uids.is_empty() {
                    return Ok(());
                }
                select(c, folder, false).await?;
                let change = if seen {
                    "+FLAGS.SILENT (\\Seen)"
                } else {
                    "-FLAGS.SILENT (\\Seen)"
                };
                let _: Vec<_> = c
                    .session
                    .uid_store(uid_set(uids), change)
                    .await
                    .map_err(failed)?
                    .try_collect()
                    .await
                    .map_err(failed)?;
                let mut cache = cache::load_folder(folder);
                for uid in uids {
                    if let Some(s) = cache.messages.get_mut(uid) {
                        s.unread = !seen;
                    }
                }
                cache::save_folder(folder, &mut cache);
                Ok(())
            })
        }))
    }

    fn move_to<'a>(&'a self, folder: &'a str, uids: &'a [u32], role: Role) -> Fut<'a, ()> {
        Box::pin(async move {
            if uids.is_empty() {
                return Ok(());
            }
            let folders = self.known_folders().await?;
            let target = destination(&folders, role);
            let target = match target {
                Some(t) => t,
                None => {
                    // A server with no Archive gets one, as other mail apps do.
                    let name = if role == Role::Trash {
                        "Trash"
                    } else {
                        "Archive"
                    };
                    on_conn!(self, |c| { c.session.create(name).await.map_err(failed) })?;

                    self.folders
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clear();
                    name.to_string()
                }
            };
            if target == folder {
                return Err(match role {
                    Role::Trash => "these are in the Trash already".into(),
                    _ => "these are archived already".into(),
                });
            }
            on_conn!(self, |c| {
                select(c, folder, false).await?;
                let set = uid_set(uids);
                if c.has_move {
                    c.session.uid_mv(&set, &target).await.map_err(failed)?;
                } else {
                    c.session.uid_copy(&set, &target).await.map_err(failed)?;
                    let _: Vec<_> = c
                        .session
                        .uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                        .await
                        .map_err(failed)?
                        .try_collect()
                        .await
                        .map_err(failed)?;
                    // Only the moved messages are expunged. Without UIDPLUS that
                    // can't be said, and a plain EXPUNGE would also take any
                    // other message the user marked deleted, so they stay
                    // marked, which this app's list leaves out.
                    if c.uidplus {
                        let _: Vec<_> = c
                            .session
                            .uid_expunge(&set)
                            .await
                            .map_err(failed)?
                            .try_collect()
                            .await
                            .map_err(failed)?;
                    }
                }
                let mut cache = cache::load_folder(folder);
                for uid in uids {
                    cache.messages.remove(uid);
                }
                cache::save_folder(folder, &mut cache);
                Ok(())
            })
        })
    }

    fn watch(&self, changed: Arc<dyn Fn(&str) + Send + Sync>) -> tokio::task::JoinHandle<()> {
        let account = self.account.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(5);
            loop {
                let started = Instant::now();
                if let Err(e) = watch_inbox(&account, &changed).await {
                    eprintln!("mail: watching the Inbox: {e}");
                }
                if started.elapsed() > Duration::from_secs(120) {
                    backoff = Duration::from_secs(5);
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
            }
        })
    }
}

/// Where archiving or deleting puts mail: Gmail archives to All Mail, which
/// takes the Inbox label off and nothing else.
pub fn destination(folders: &[Folder], role: Role) -> Option<String> {
    let find = |r: Role| {
        folders
            .iter()
            .find(|f| f.role == Some(r))
            .map(|f| f.path.clone())
    };
    match role {
        Role::Archive => find(Role::Archive).or_else(|| find(Role::All)),
        r => find(r),
    }
}

async fn listed_summaries(
    c: &mut Conn,
    folder: &str,
    validity: u32,
    uids: &[u32],
) -> Result<Vec<Summary>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let mut cache = cache::load_folder(folder);
    if cache.validity != validity {
        cache = FolderCache {
            validity,
            ..Default::default()
        };
    }
    let gmail = if c.gmail { " X-GM-LABELS" } else { "" };
    let mut listed = Vec::new();
    for batch in uids.chunks(BATCH * 5) {
        listed.extend(
            fetch(
                &mut c.session,
                &format!("UID FETCH {} (UID FLAGS{gmail})", uid_set(batch)),
            )
            .await?,
        );
    }
    let out = summaries(c, folder, &mut cache, &listed).await?;
    cache::save_folder(folder, &mut cache);
    Ok(out)
}

/// Wait in IDLE on the Inbox, saying whenever it changes, until the
/// connection fails.
async fn watch_inbox(account: &Saved, changed: &Arc<dyn Fn(&str) + Send + Sync>) -> Result<()> {
    let mut conn = connect(account).await?;
    let inbox = "INBOX";
    if !conn.idle {
        let mut last = None;
        loop {
            let mb = conn
                .session
                .status(inbox, "(MESSAGES UIDNEXT UNSEEN)")
                .await
                .map_err(failed)?;
            let now = (mb.exists, mb.uid_next, mb.unseen);
            if last.is_some_and(|l| l != now) {
                changed(inbox);
            }
            last = Some(now);
            tokio::time::sleep(POLL_EVERY).await;
        }
    }
    conn.session.select(inbox).await.map_err(failed)?;
    let mut session = conn.session;
    loop {
        let mut idle = session.idle();
        idle.init().await.map_err(failed)?;
        let response = {
            let (waiting, _stop) = idle.wait_with_timeout(IDLE_FOR);
            waiting.await.map_err(failed)?
        };
        session = idle.done().await.map_err(failed)?;
        if let IdleResponse::NewData(data) = response {
            let worth_saying = matches!(
                data.parsed(),
                Response::MailboxData(MailboxDatum::Exists(_))
                    | Response::MailboxData(MailboxDatum::Recent(_))
                    | Response::Expunge(_)
                    | Response::Fetch(..)
                    | Response::Vanished { .. }
            );
            if worth_saying {
                // A burst of changes (a filter moving ten messages) is one.
                tokio::time::sleep(Duration::from_millis(500)).await;
                changed(inbox);
            }
        }
    }
}
