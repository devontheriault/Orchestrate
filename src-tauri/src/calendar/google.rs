//! Google Calendar, through its own API: Google offers CalDAV too, but not to
//! an app signing in with OAuth the way a desktop app should.
//!
//! Signing in is OAuth 2.0 for installed apps (see [`super::oauth`]), as an
//! OAuth client the build comes with or the user made themselves
//! (`docs/calendar-accounts.md`).
//!
//! Syncing reads each calendar's Calendar events once in full and then only
//! what changed, by Google's sync token. Repeating ones come as their rule
//! and their exceptions, never expanded, so they are expanded the same way a
//! CalDAV server's are. Writing asks Google to email the people invited.

use chrono::{DateTime, NaiveDate};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

use super::oauth::{encode, escape, Client, OAuth, Session, Tokens};
use super::write::{Answer, Draft, Found, Scope};
use super::{
    ical, meeting_link, AccountCache, Attendee, CachedCalendar, Item, Provider, Resource,
    SyncError, When,
};

/// Reading every calendar the account can see, and changing Calendar events
/// on the ones it can write. Nothing else of the account's.
pub const SCOPE: &str =
    "https://www.googleapis.com/auth/calendar.readonly https://www.googleapis.com/auth/calendar.events";

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

impl Endpoints {
    pub fn oauth(&self) -> OAuth {
        OAuth {
            name: "Google",
            auth: self.auth.clone(),
            token: self.token.clone(),
            scope: SCOPE.into(),
            // A refresh token, every time: without `consent` Google only
            // gives one the first time an account agrees.
            extra: vec![("access_type", "offline"), ("prompt", "consent")],
            loopback: "127.0.0.1",
            scope_on_refresh: false,
        }
    }
}

/// One Google account, signed in.
pub struct Google<'a> {
    http: &'a reqwest::Client,
    api: String,
    oauth: OAuth,
    client: Client,
    tokens: &'a mut Tokens,
}

/// What Google said went wrong, from its reply.
fn failure(status: StatusCode, body: &Value) -> SyncError {
    let why = body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let reason = body["error"]["errors"][0]["reason"]
        .as_str()
        .or(body["error"]["status"].as_str())
        .unwrap_or_default();
    match status.as_u16() {
        401 => SyncError::Auth(format!("Google wants you to sign in again ({why})")),
        403 if matches!(
            reason,
            "insufficientPermissions" | "PERMISSION_DENIED" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT"
        ) && why.to_ascii_lowercase().contains("scope") =>
        {
            SyncError::Auth(
                "sign in to Google again, to let Orchestrate change your calendar".into(),
            )
        }
        404 | 410 if reason == "deleted" || status == StatusCode::NOT_FOUND => {
            SyncError::Other("it isn't on Google Calendar any more".into())
        }
        410 => SyncError::Other("gone".into()),
        412 => SyncError::Other("it changed on Google Calendar since; sync and try again".into()),
        _ => SyncError::Other(format!("Google answered {status}: {why}")),
    }
}

impl<'a> Google<'a> {
    pub fn new(
        http: &'a reqwest::Client,
        endpoints: &Endpoints,
        client: Client,
        tokens: &'a mut Tokens,
    ) -> Self {
        Self {
            http,
            api: endpoints.api.clone(),
            oauth: endpoints.oauth(),
            client,
            tokens,
        }
    }

    async fn call(
        &mut self,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Value, SyncError> {
        let reply = Session::new(self.http, &self.oauth, &self.client, self.tokens)
            .send(method, url, body, &[])
            .await?;
        if reply.status.is_success() {
            Ok(reply.body)
        } else {
            Err(failure(reply.status, &reply.body))
        }
    }

    async fn get(&mut self, url: &str) -> Result<Value, SyncError> {
        self.call(Method::GET, url, None).await
    }

    /// The account's address: its primary calendar's id.
    pub async fn address(&mut self) -> Result<String, SyncError> {
        let url = format!("{}/users/me/calendarList/primary", self.api);
        let primary = self.get(&url).await?;
        Ok(primary["id"].as_str().unwrap_or("Google").to_string())
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
            let url = format!("{}/users/me/calendarList?{}", self.api, encode(&q));
            let body = self.get(&url).await?;
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
                self.api,
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
                        raw: None,
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

    /// The URL of one Calendar event, or one occurrence's own, with
    /// `sendUpdates` saying whether to email the people on it.
    fn event_url(&self, cal: &CachedCalendar, id: &str, notify: bool) -> String {
        format!(
            "{}/calendars/{}/events/{}?sendUpdates={}",
            self.api,
            escape(&cal.id),
            escape(id),
            if notify { "all" } else { "none" }
        )
    }

    /// Which event at Google a change for `scope` goes to: the master, or the
    /// occurrence, whose id is the master's and its original start.
    fn target(found: &Found, scope: Scope) -> Result<String, SyncError> {
        if scope == Scope::This && found.repeats() {
            if let Some((id, _)) = &found.exception {
                return Ok(id.clone());
            }
            let (master, _) = found.master.as_ref().ok_or_else(missing)?;
            return Ok(format!("{master}_{}", found.occurrence));
        }
        found.key(scope).map(str::to_string).ok_or_else(missing)
    }
}

fn missing() -> SyncError {
    SyncError::Other("no such Calendar event".into())
}

/// Google's name for an answer.
fn status(response: Option<&str>) -> &'static str {
    match response {
        Some("accepted") => "accepted",
        Some("declined") => "declined",
        Some("tentative") => "tentative",
        _ => "needsAction",
    }
}

/// A draft as Google's event resource. People already invited keep their
/// answers; `whole` says whether the repeat rule belongs in it.
fn body(draft: &Draft, old: &[Attendee], whole: bool) -> Result<Value, SyncError> {
    let time = |s: &str| {
        if draft.all_day {
            json!({ "date": s })
        } else {
            let s = if s.len() == 16 {
                format!("{s}:00")
            } else {
                s.to_string()
            };
            json!({ "dateTime": s, "timeZone": draft.time_zone })
        }
    };
    let attendees: Vec<Value> = draft
        .attendees
        .iter()
        .map(|g| {
            let was = old.iter().find(|a| a.email.eq_ignore_ascii_case(&g.email));
            let mut a = json!({
                "email": g.email.trim(),
                "responseStatus": status(was.and_then(|a| a.response.as_deref())),
            });
            if let Some(n) = g.name.as_deref().filter(|n| !n.trim().is_empty()) {
                a["displayName"] = json!(n.trim());
            }
            a
        })
        .collect();
    let mut b = json!({
        "summary": draft.title.trim(),
        "location": draft.location.as_deref().unwrap_or("").trim(),
        "description": draft.description.as_deref().unwrap_or("").trim(),
        "start": time(&draft.start),
        "end": time(&draft.end),
        "attendees": attendees,
        "transparency": if draft.busy { "opaque" } else { "transparent" },
    });
    if whole {
        let rule = draft.rrule().map_err(SyncError::Other)?;
        b["recurrence"] = json!(rule.map(|r| vec![format!("RRULE:{r}")]).unwrap_or_default());
    }
    Ok(b)
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
            let writable = matches!(
                entry["accessRole"].as_str(),
                Some("owner" | "writer") | None
            );
            if (
                &cal.name,
                &cal.color,
                cal.primary,
                cal.selected,
                cal.writable,
            ) != (&name, &color, primary, selected, writable)
            {
                (cal.name, cal.color, cal.primary, cal.selected, cal.writable) =
                    (name, color, primary, selected, writable);
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

    async fn create(&mut self, cal: &CachedCalendar, draft: &Draft) -> Result<(), SyncError> {
        let url = format!(
            "{}/calendars/{}/events?sendUpdates={}",
            self.api,
            escape(&cal.id),
            if draft.attendees.is_empty() {
                "none"
            } else {
                "all"
            }
        );
        let b = body(draft, &[], true)?;
        self.call(Method::POST, &url, Some(&b)).await.map(|_| ())
    }

    async fn update(
        &mut self,
        cal: &CachedCalendar,
        found: &Found,
        draft: &Draft,
        scope: Scope,
    ) -> Result<(), SyncError> {
        let id = Self::target(found, scope)?;
        let old = found
            .item(scope)
            .map(|i| i.attendees.clone())
            .unwrap_or_default();
        let notify = !draft.attendees.is_empty() || !old.is_empty();
        let whole = !(scope == Scope::This && found.repeats());
        let b = body(draft, &old, whole)?;
        let url = self.event_url(cal, &id, notify);
        self.call(Method::PATCH, &url, Some(&b)).await.map(|_| ())
    }

    async fn delete(
        &mut self,
        cal: &CachedCalendar,
        found: &Found,
        scope: Scope,
    ) -> Result<(), SyncError> {
        let id = Self::target(found, scope)?;
        let notify = found.item(scope).is_some_and(|i| !i.attendees.is_empty());
        let url = self.event_url(cal, &id, notify);
        self.call(Method::DELETE, &url, None).await.map(|_| ())
    }

    async fn respond(
        &mut self,
        cal: &CachedCalendar,
        found: &Found,
        answer: Answer,
        scope: Scope,
    ) -> Result<(), SyncError> {
        let id = Self::target(found, scope)?;
        let item = found.item(scope).ok_or_else(missing)?;
        if !item.attendees.iter().any(|a| a.is_self) {
            return Err(SyncError::Other(
                "you aren't among the people invited to it".into(),
            ));
        }
        let mine = match answer {
            Answer::Accepted => "accepted",
            Answer::Tentative => "tentative",
            Answer::Declined => "declined",
        };
        let attendees: Vec<Value> = item
            .attendees
            .iter()
            .map(|a| {
                json!({
                    "email": a.email,
                    "responseStatus": if a.is_self { mine } else { status(a.response.as_deref()) },
                })
            })
            .collect();
        let url = self.event_url(cal, &id, true);
        self.call(
            Method::PATCH,
            &url,
            Some(&json!({ "attendees": attendees })),
        )
        .await
        .map(|_| ())
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
