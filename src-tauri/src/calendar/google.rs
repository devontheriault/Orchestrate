//! Google Calendar, through its own API: Google offers CalDAV too, but not to
//! an app signing in with OAuth the way a desktop app should.
//!
//! Signing in is OAuth 2.0 for installed apps: the user's browser goes to
//! Google with a PKCE challenge, and Google sends it back to a port the Host
//! listens on at 127.0.0.1 — so the browser has to be on the Host's machine.
//! The client it signs in as is the user's own (`docs/google-calendar.md`).
//!
//! Syncing reads each calendar's Calendar events once in full and then only
//! what changed, by Google's sync token. Repeating ones come as their rule
//! and their exceptions, never expanded, so they are expanded the same way a
//! CalDAV server's are.

use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::{
    ical, meeting_link, AccountCache, Attendee, CachedCalendar, GoogleClient, Item, Provider,
    Resource, SyncError, When,
};

/// Read access to every calendar the account can see. Nothing is written.
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";

/// How long a sign-in waits for the browser to come back.
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);

/// Where Google is. Only tests point these anywhere else.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            api: "https://www.googleapis.com/calendar/v3".into(),
        }
    }
}

/// What signing in left the Host with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// When `access_token` stops working, in Unix seconds.
    pub expires_at: i64,
}

/// `s` percent-encoded, for a query value or a path segment.
pub fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `application/x-www-form-urlencoded`, for a query or a form body.
pub fn encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", escape(k), escape(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `key=value` pairs of a query string, decoded.
pub fn query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
}

fn now() -> i64 {
    Utc::now().timestamp()
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

/// Ask Google's token endpoint for tokens, by the `form` given.
async fn token(
    http: &reqwest::Client,
    endpoints: &Endpoints,
    form: &[(&str, &str)],
) -> Result<TokenReply, SyncError> {
    let res = http
        .post(&endpoints.token)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encode(form))
        .send()
        .await
        .map_err(|e| SyncError::Other(format!("could not reach Google: {e}")))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or_default();
    if status.is_success() {
        return serde_json::from_value(body)
            .map_err(|e| SyncError::Other(format!("Google's token reply was unreadable: {e}")));
    }
    let code = body["error"].as_str().unwrap_or_default();
    let why = body["error_description"].as_str().unwrap_or(code);
    // A refresh token the user revoked, or one Google expired (as it does
    // after a week for an app still "in testing"), can't be used again.
    if matches!(
        code,
        "invalid_grant" | "invalid_client" | "unauthorized_client"
    ) {
        Err(SyncError::Auth(format!(
            "Google wants you to sign in again ({why})"
        )))
    } else {
        Err(SyncError::Other(format!(
            "Google refused the sign-in: {status} {why}"
        )))
    }
}

/// A sign-in waiting for the browser to come back.
pub struct SignIn {
    /// Where to send the user's browser.
    pub url: String,
    listener: TcpListener,
    redirect: String,
    verifier: String,
    state: String,
}

fn random(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    (0..len)
        .map(|_| CHARS[rand::random_range(0..CHARS.len())] as char)
        .collect()
}

/// The PKCE challenge for `verifier`: its SHA-256, base64url without padding.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The page the browser lands on when it comes back.
fn page(title: &str, body: &str) -> String {
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title>\
         <body style=\"font:16px system-ui;margin:4rem auto;max-width:32rem;color:#333\">\
         <h1 style=\"font-size:1.3rem\">{title}</h1><p>{body}</p></body>"
    );
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}",
        html.len()
    )
}

impl SignIn {
    /// Listen for Google's redirect, and say where to send the browser.
    pub async fn start(endpoints: &Endpoints, client: &GoogleClient) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("could not listen for Google's answer: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let redirect = format!("http://127.0.0.1:{port}");
        let verifier = random(64);
        let state = random(24);
        let url = format!(
            "{}?{}",
            endpoints.auth,
            encode(&[
                ("client_id", &client.client_id),
                ("redirect_uri", &redirect),
                ("response_type", "code"),
                ("scope", SCOPE),
                ("code_challenge", &challenge(&verifier)),
                ("code_challenge_method", "S256"),
                ("state", &state),
                // A refresh token, every time: without `consent` Google only
                // gives one the first time an account agrees.
                ("access_type", "offline"),
                ("prompt", "consent"),
            ])
        );
        Ok(Self {
            url,
            listener,
            redirect,
            verifier,
            state,
        })
    }

    /// Wait for the browser, trade its code for tokens, and find which
    /// account it signed in as.
    pub async fn finish(
        self,
        http: &reqwest::Client,
        endpoints: &Endpoints,
        client: &GoogleClient,
    ) -> Result<(String, Tokens), String> {
        let code = tokio::time::timeout(SIGN_IN_WAIT, self.wait_for_code())
            .await
            .map_err(|_| "the sign-in wasn't finished in the browser in time".to_string())??;
        let reply = token(
            http,
            endpoints,
            &[
                ("client_id", &client.client_id),
                ("client_secret", &client.client_secret),
                ("code", &code),
                ("code_verifier", &self.verifier),
                ("redirect_uri", &self.redirect),
                ("grant_type", "authorization_code"),
            ],
        )
        .await
        .map_err(|e| e.to_string())?;
        let mut tokens = Tokens {
            access_token: reply.access_token,
            refresh_token: reply
                .refresh_token
                .ok_or("Google gave no refresh token, so the Host couldn't stay signed in")?,
            expires_at: now() + reply.expires_in,
        };
        // The primary calendar's id is the account's address.
        let primary = Google::new(http, endpoints, client, &mut tokens)
            .get(&format!("{}/users/me/calendarList/primary", endpoints.api))
            .await
            .map_err(|e| e.to_string())?;
        let name = primary["id"].as_str().unwrap_or("Google").to_string();
        Ok((name, tokens))
    }

    async fn wait_for_code(&self) -> Result<String, String> {
        loop {
            let (mut stream, _) = self.listener.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let target = request
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            if path != "/" {
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let params = query_pairs(query);
            let get = |k: &str| {
                params
                    .iter()
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.clone())
            };
            if get("state").as_deref() != Some(self.state.as_str()) {
                // Not the browser this sign-in sent; keep waiting for it.
                let _ = stream
                    .write_all(
                        page(
                            "Not this sign-in",
                            "This page belongs to an older sign-in. You can close it.",
                        )
                        .as_bytes(),
                    )
                    .await;
                continue;
            }
            if let Some(error) = get("error") {
                let _ = stream
                    .write_all(
                        page(
                            "Google wasn't connected",
                            "You can close this tab and try again from Orchestrate.",
                        )
                        .as_bytes(),
                    )
                    .await;
                return Err(format!("Google sign-in was not completed ({error})"));
            }
            let Some(code) = get("code") else {
                continue;
            };
            let _ = stream
                .write_all(
                    page(
                        "Google Calendar is connected",
                        "You can close this tab and go back to Orchestrate.",
                    )
                    .as_bytes(),
                )
                .await;
            return Ok(code);
        }
    }
}

/// One Google account, signed in.
pub struct Google<'a> {
    http: &'a reqwest::Client,
    endpoints: &'a Endpoints,
    client: &'a GoogleClient,
    tokens: &'a mut Tokens,
}

impl<'a> Google<'a> {
    pub fn new(
        http: &'a reqwest::Client,
        endpoints: &'a Endpoints,
        client: &'a GoogleClient,
        tokens: &'a mut Tokens,
    ) -> Self {
        Self {
            http,
            endpoints,
            client,
            tokens,
        }
    }

    /// Trade the refresh token for a new access token.
    async fn refresh(&mut self) -> Result<(), SyncError> {
        let reply = token(
            self.http,
            self.endpoints,
            &[
                ("client_id", &self.client.client_id),
                ("client_secret", &self.client.client_secret),
                ("refresh_token", &self.tokens.refresh_token),
                ("grant_type", "refresh_token"),
            ],
        )
        .await?;
        self.tokens.access_token = reply.access_token;
        self.tokens.expires_at = now() + reply.expires_in;
        if let Some(r) = reply.refresh_token {
            self.tokens.refresh_token = r;
        }
        Ok(())
    }

    /// GET `url` as the account, refreshing the access token when it's about
    /// to run out, or when Google says it already has.
    async fn get(&mut self, url: &str) -> Result<Value, SyncError> {
        if self.tokens.expires_at - 60 <= now() {
            self.refresh().await?;
        }
        let mut retried = false;
        loop {
            let res = self
                .http
                .get(url)
                .bearer_auth(&self.tokens.access_token)
                .send()
                .await
                .map_err(|e| SyncError::Other(format!("could not reach Google: {e}")))?;
            let status = res.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && !retried {
                retried = true;
                self.refresh().await?;
                continue;
            }
            let body: Value = res.json().await.unwrap_or_default();
            if status.is_success() {
                return Ok(body);
            }
            let why = body["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            return Err(match status.as_u16() {
                401 => SyncError::Auth(format!("Google wants you to sign in again ({why})")),
                410 => SyncError::Other("gone".into()),
                _ => SyncError::Other(format!("Google answered {status}: {why}")),
            });
        }
    }

    /// Every calendar on the account's list.
    async fn calendars(&mut self) -> Result<Vec<Value>, SyncError> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![("maxResults", "250")];
            if let Some(p) = &page {
                q.push(("pageToken", p));
            }
            let body = self
                .get(&format!(
                    "{}/users/me/calendarList?{}",
                    self.endpoints.api,
                    encode(&q)
                ))
                .await?;
            out.extend(body["items"].as_array().cloned().unwrap_or_default());
            match body["nextPageToken"].as_str() {
                Some(p) => page = Some(p.to_string()),
                None => return Ok(out),
            }
        }
    }

    /// Bring one calendar's Calendar events level: everything since its sync
    /// token, or everything if it has none or Google has forgotten it.
    async fn sync_calendar(&mut self, cal: &mut CachedCalendar) -> Result<bool, SyncError> {
        let full = cal.sync_token.is_none();
        let mut fetched: Vec<Value> = Vec::new();
        let mut page: Option<String> = None;
        let next_token = loop {
            let mut q = vec![("maxResults", "2500"), ("singleEvents", "false")];
            if let Some(t) = &cal.sync_token {
                q.push(("syncToken", t));
            }
            if let Some(p) = &page {
                q.push(("pageToken", p));
            }
            let url = format!(
                "{}/calendars/{}/events?{}",
                self.endpoints.api,
                escape(&cal.id),
                encode(&q)
            );
            let body = match self.get(&url).await {
                Ok(b) => b,
                // The sync token is too old: start again from nothing.
                Err(SyncError::Other(m)) if m == "gone" && cal.sync_token.is_some() => {
                    cal.sync_token = None;
                    cal.resources.clear();
                    return Box::pin(self.sync_calendar(cal)).await.map(|_| true);
                }
                Err(e) => return Err(e),
            };
            fetched.extend(body["items"].as_array().cloned().unwrap_or_default());
            if let Some(p) = body["nextPageToken"].as_str() {
                page = Some(p.to_string());
                continue;
            }
            break body["nextSyncToken"].as_str().map(str::to_string);
        };

        let mut changed = false;
        if full {
            changed = !cal.resources.is_empty();
            cal.resources.clear();
        }
        for raw in &fetched {
            let Some(id) = raw["id"].as_str() else {
                continue;
            };
            let item = item(raw);
            let master_gone = raw["status"] == "cancelled" && raw["recurringEventId"].is_null();
            match item {
                // A deleted one-off or master leaves nothing behind; a deleted
                // occurrence stays, cancelled, to hide that occurrence.
                _ if master_gone => {
                    changed |= cal.resources.remove(id).is_some();
                }
                Some(item) => {
                    let resource = Resource {
                        etag: raw["etag"].as_str().map(str::to_string),
                        items: vec![item],
                    };
                    if cal.resources.get(id) != Some(&resource) {
                        cal.resources.insert(id.to_string(), resource);
                        changed = true;
                    }
                }
                None => {}
            }
        }
        cal.sync_token = next_token;
        Ok(changed)
    }
}

impl Provider for Google<'_> {
    async fn sync(&mut self, cache: &mut AccountCache) -> Result<bool, SyncError> {
        let listed = self.calendars().await?;
        let mut changed = false;
        let mut calendars = Vec::with_capacity(listed.len());
        for entry in &listed {
            let Some(id) = entry["id"].as_str() else {
                continue;
            };
            let mut cal = cache
                .calendars
                .iter()
                .find(|c| c.id == id)
                .cloned()
                .unwrap_or_else(|| CachedCalendar {
                    id: id.to_string(),
                    ..Default::default()
                });
            let name = entry["summaryOverride"]
                .as_str()
                .or(entry["summary"].as_str())
                .unwrap_or(id)
                .to_string();
            let color = entry["backgroundColor"].as_str().map(str::to_string);
            let primary = entry["primary"].as_bool().unwrap_or(false);
            let selected = entry["selected"].as_bool().unwrap_or(primary);
            if (&cal.name, &cal.color, cal.primary, cal.selected)
                != (&name, &color, primary, selected)
            {
                (cal.name, cal.color, cal.primary, cal.selected) = (name, color, primary, selected);
                changed = true;
            }
            changed |= self.sync_calendar(&mut cal).await?;
            calendars.push(cal);
        }
        calendars.sort_by(|a, b| b.primary.cmp(&a.primary).then_with(|| a.name.cmp(&b.name)));
        if calendars.len() != cache.calendars.len() {
            changed = true;
        }
        cache.calendars = calendars;
        Ok(changed)
    }
}

/// A start, end or original start, as Google writes them: a `date`, or a
/// `dateTime` with an offset and usually the zone it was made in.
fn when(v: &Value) -> Option<When> {
    if let Some(d) = v["date"].as_str() {
        return NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .ok()
            .map(|date| When::Date { date });
    }
    let t = DateTime::parse_from_rfc3339(v["dateTime"].as_str()?).ok()?;
    match v["timeZone"].as_str().and_then(super::zone) {
        Some(tz) => Some(When::Time {
            at: t.with_timezone(&tz).naive_local(),
            tz: Some(tz.name().to_string()),
        }),
        None => Some(When::Time {
            at: t.to_utc().naive_utc(),
            tz: Some("UTC".into()),
        }),
    }
}

fn person(v: &Value) -> Attendee {
    Attendee {
        name: v["displayName"].as_str().map(str::to_string),
        email: v["email"].as_str().unwrap_or_default().to_string(),
        response: v["responseStatus"].as_str().map(|r| match r {
            "needsAction" => "needs-action".to_string(),
            other => other.to_string(),
        }),
        is_self: v["self"].as_bool().unwrap_or(false),
        organizer: v["organizer"].as_bool().unwrap_or(false),
    }
}

/// A Google Calendar event as an [`Item`]. Exceptions are grouped with their
/// master by the master's id, which they carry as `recurringEventId` even
/// when cancelled and stripped of everything else.
pub fn item(v: &Value) -> Option<Item> {
    let id = v["id"].as_str()?;
    // Where the user says they're working from, which Google keeps as an
    // all-day Calendar event on every day. It isn't something happening.
    if v["eventType"] == "workingLocation" {
        return None;
    }
    let uid = v["recurringEventId"].as_str().unwrap_or(id).to_string();
    let recurrence_id = when(&v["originalStartTime"]);
    let cancelled = v["status"] == "cancelled";
    let start = when(&v["start"]).or_else(|| recurrence_id.clone())?;
    let html = v["description"].as_str();
    let video = v["conferenceData"]["entryPoints"]
        .as_array()
        .and_then(|eps| eps.iter().find(|e| e["entryPointType"] == "video"))
        .and_then(|e| e["uri"].as_str())
        .or(v["hangoutLink"].as_str())
        .map(str::to_string);
    let location = v["location"].as_str().map(str::to_string);
    let meeting = video.or_else(|| meeting_link([location.as_deref(), html]));

    let mut rrule = Vec::new();
    let mut rdate = Vec::new();
    let mut exdate = Vec::new();
    for line in v["recurrence"].as_array().into_iter().flatten() {
        let Some(line) = line.as_str().and_then(ical::parse_line) else {
            continue;
        };
        let whens = || {
            let date_only = line.params.iter().any(|(k, v)| k == "VALUE" && v == "DATE");
            let tzid = line
                .params
                .iter()
                .find(|(k, _)| k == "TZID")
                .map(|(_, v)| v.as_str());
            line.value
                .split(',')
                .filter_map(|x| ical::parse_when(x, tzid, date_only))
                .collect::<Vec<_>>()
        };
        match line.name.as_str() {
            "RRULE" => rrule.push(line.value.clone()),
            "RDATE" => rdate.extend(whens()),
            "EXDATE" => exdate.extend(whens()),
            _ => {}
        }
    }

    Some(Item {
        uid,
        recurrence_id,
        cancelled,
        tentative: v["status"] == "tentative",
        title: v["summary"].as_str().unwrap_or_default().to_string(),
        description: html.map(plain).filter(|s| !s.is_empty()),
        location,
        end: when(&v["end"]),
        start,
        rrule,
        rdate,
        exdate,
        attendees: v["attendees"]
            .as_array()
            .into_iter()
            .flatten()
            .map(person)
            .collect(),
        organizer: v
            .get("organizer")
            .filter(|o| o.is_object())
            .map(|o| Attendee {
                organizer: true,
                ..person(o)
            }),
        meeting_url: meeting,
        link: v["htmlLink"].as_str().map(str::to_string),
        transparent: v["transparency"] == "transparent",
    })
}

/// Google writes descriptions as a little HTML. Plain text keeps them safe to
/// show and readable to an Agent: line breaks kept, tags dropped, and a
/// link's address kept where its text doesn't already say it.
pub fn plain(html: &str) -> String {
    if !html.contains('<') && !html.contains('&') {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut href: Option<String> = None;
    let mut link_text = String::new();
    while let Some(open) = rest.find('<') {
        let text = &rest[..open];
        if href.is_some() {
            link_text.push_str(text);
        } else {
            out.push_str(text);
        }
        let Some(close) = rest[open..].find('>') else {
            rest = &rest[open..];
            break;
        };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let lower = tag.to_ascii_lowercase();
        let name = lower
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        match name {
            "br" | "br/" => {
                if href.is_some() {
                    link_text.push('\n')
                } else {
                    out.push('\n')
                }
            }
            "p" | "div" | "li" | "ul" | "ol" => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
            }
            "a" if !lower.starts_with('/') => {
                href = attr(tag, "href");
                link_text.clear();
            }
            "a" => {
                let text = link_text.trim().to_string();
                if let Some(url) = href.take() {
                    if text.is_empty() || text == url || url.contains(&text) {
                        out.push_str(&url);
                    } else {
                        out.push_str(&format!("{text} ({url})"));
                    }
                } else {
                    out.push_str(&text);
                }
            }
            _ => {}
        }
    }
    out.push_str(rest);
    entities(out.trim())
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    let (quote, rest) = match rest.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &rest[1..]),
        _ => (None, rest),
    };
    let end = match quote {
        Some(q) => rest.find(q)?,
        None => rest.find(char::is_whitespace).unwrap_or(rest.len()),
    };
    Some(entities(&rest[..end]))
}

fn entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}
