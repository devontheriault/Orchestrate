//! Outlook.com and Microsoft 365, through Microsoft Graph: neither offers
//! CalDAV. Signing in is the same OAuth 2.0 for installed apps as Google's
//! (see [`super::oauth`]), from Microsoft's common endpoint, so a personal
//! account and a work or school one both work. The app is a public client:
//! it has an ID and no secret.
//!
//! Microsoft writes repeats in its own shape, not as RRULEs, so rather than
//! translate them the Host lets Graph expand them: each calendar is synced as
//! a window of time (`calendarView/delta`), a year back and two ahead, every
//! occurrence its own Calendar event that knows its series. Writing turns the
//! app's RRULE into Microsoft's pattern. Graph emails the people invited
//! itself.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Utc};
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

use super::oauth::{escape, Client, OAuth, Session, Tokens};
use super::write::{Answer, Draft, Found, Scope};
use super::{AccountCache, Attendee, CachedCalendar, Item, Provider, Resource, SyncError, When};

/// Reading and writing the user's calendars, and their name and address;
/// `offline_access` is what keeps the Host signed in.
pub const SCOPE: &str = "offline_access https://graph.microsoft.com/User.Read https://graph.microsoft.com/Calendars.ReadWrite";

/// How far back and ahead each calendar is synced.
const BACK_DAYS: i64 = 365;
const AHEAD_DAYS: i64 = 730;
/// How stale the window may get before it is moved along with a full sync.
const SLIDE_DAYS: i64 = 30;

/// Where Microsoft is. Only tests point these anywhere else.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub api: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            auth: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
            token: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
            api: "https://graph.microsoft.com/v1.0".into(),
        }
    }
}

impl Endpoints {
    pub fn oauth(&self) -> OAuth {
        OAuth {
            name: "Microsoft",
            auth: self.auth.clone(),
            token: self.token.clone(),
            scope: SCOPE.into(),
            extra: vec![("prompt", "select_account"), ("response_mode", "query")],
            loopback: "localhost",
            scope_on_refresh: true,
        }
    }
}

/// One Microsoft account, signed in.
pub struct Graph<'a> {
    http: &'a reqwest::Client,
    api: String,
    oauth: OAuth,
    client: Client,
    tokens: &'a mut Tokens,
    /// The user's address, read once per sync, to find them among attendees.
    me: Option<String>,
}

fn failure(status: StatusCode, body: &Value) -> SyncError {
    let code = body["error"]["code"].as_str().unwrap_or_default();
    let why = body["error"]["message"].as_str().unwrap_or_default();
    match status.as_u16() {
        401 => SyncError::Auth(format!("Microsoft wants you to sign in again ({why})")),
        403 if code == "ErrorAccessDenied" => {
            SyncError::Other("Microsoft won't let you change that calendar".into())
        }
        404 => SyncError::Other("it isn't in your Microsoft calendar any more".into()),
        410 => SyncError::Other("gone".into()),
        412 => SyncError::Other("it changed in Outlook since; sync and try again".into()),
        _ if code.contains("SyncState") || code == "resyncRequired" => {
            SyncError::Other("gone".into())
        }
        _ => SyncError::Other(format!("Microsoft answered {status}: {why}")),
    }
}

impl<'a> Graph<'a> {
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
            me: None,
        }
    }

    async fn call(
        &mut self,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<Value, SyncError> {
        let reply = Session::new(self.http, &self.oauth, &self.client, self.tokens)
            .send(
                method,
                url,
                body,
                // Times in UTC, so a repeat Graph expanded keeps its instants
                // whatever zone it was made in.
                &[("prefer", "outlook.timezone=\"UTC\", odata.maxpagesize=200")],
            )
            .await?;
        if reply.status.is_success() {
            Ok(reply.body)
        } else {
            Err(failure(reply.status, &reply.body))
        }
    }

    /// The account's address.
    pub async fn address(&mut self) -> Result<String, SyncError> {
        let url = format!("{}/me?$select=mail,userPrincipalName", self.api);
        let me = self.call(Method::GET, &url, None).await?;
        Ok(me["mail"]
            .as_str()
            .or(me["userPrincipalName"].as_str())
            .unwrap_or("Microsoft")
            .to_string())
    }

    async fn calendars(&mut self) -> Result<Vec<Value>, SyncError> {
        let mut out = Vec::new();
        let mut url = format!("{}/me/calendars?$top=100", self.api);
        loop {
            let body = self.call(Method::GET, &url, None).await?;
            out.extend(body["value"].as_array().cloned().unwrap_or_default());
            match body["@odata.nextLink"].as_str() {
                Some(next) => url = next.to_string(),
                None => return Ok(out),
            }
        }
    }

    async fn sync_calendar(&mut self, cal: &mut CachedCalendar) -> Result<bool, SyncError> {
        let today = Utc::now().date_naive();
        let from = today - Duration::days(BACK_DAYS);
        let stale = cal
            .window_start
            .as_deref()
            .and_then(|w| NaiveDate::parse_from_str(w, "%Y-%m-%d").ok())
            .is_none_or(|w| (from - w).num_days() > SLIDE_DAYS);
        if stale {
            cal.sync_token = None;
        }
        let full = cal.sync_token.is_none();
        let mut url = match &cal.sync_token {
            Some(link) => link.clone(),
            None => format!(
                "{}/me/calendars/{}/calendarView/delta?startDateTime={}T00:00:00Z&endDateTime={}T00:00:00Z",
                self.api,
                escape(&cal.id),
                from,
                today + Duration::days(AHEAD_DAYS)
            ),
        };
        let mut fetched = Vec::new();
        let delta = loop {
            let body = match self.call(Method::GET, &url, None).await {
                Ok(b) => b,
                Err(SyncError::Other(m)) if m == "gone" && cal.sync_token.is_some() => {
                    cal.sync_token = None;
                    cal.window_start = None;
                    return Box::pin(self.sync_calendar(cal)).await.map(|_| true);
                }
                Err(e) => return Err(e),
            };
            fetched.extend(body["value"].as_array().cloned().unwrap_or_default());
            if let Some(next) = body["@odata.nextLink"].as_str() {
                url = next.to_string();
                continue;
            }
            break body["@odata.deltaLink"].as_str().map(str::to_string);
        };
        let mut changed = false;
        if full {
            changed = !cal.resources.is_empty();
            cal.resources.clear();
            cal.window_start = Some(from.to_string());
        }
        for raw in &fetched {
            let Some(id) = raw["id"].as_str() else {
                continue;
            };
            if raw.get("@removed").is_some() {
                changed |= cal.resources.remove(id).is_some();
                continue;
            }
            let Some(item) = item(raw, self.me.as_deref()) else {
                continue;
            };
            let resource = Resource {
                etag: raw["@odata.etag"].as_str().map(str::to_string),
                items: vec![item],
                raw: None,
            };
            if cal.resources.get(id) != Some(&resource) {
                cal.resources.insert(id.to_string(), resource);
                changed = true;
            }
        }
        cal.sync_token = delta;
        Ok(changed)
    }

    /// Which event at Microsoft a change for `scope` goes to: one
    /// occurrence, or its whole series.
    fn target(found: &Found, scope: Scope) -> Result<String, SyncError> {
        let missing = || SyncError::Other("no such Calendar event".into());
        if found.repeats() && scope == Scope::All {
            return Ok(found.uid.clone());
        }
        found
            .key(Scope::This)
            .map(str::to_string)
            .ok_or_else(missing)
    }
}

/// Microsoft's colour names, for a calendar with no colour of its own.
fn named_color(name: &str) -> Option<&'static str> {
    Some(match name {
        "lightBlue" => "#4f9fe8",
        "lightGreen" => "#5fb85f",
        "lightOrange" => "#f2994a",
        "lightGray" => "#9aa0a6",
        "lightYellow" => "#e8c547",
        "lightTeal" => "#3fb6b2",
        "lightPink" => "#e66fa8",
        "lightBrown" => "#a57952",
        "lightRed" => "#e5534b",
        "maxColor" | "auto" => return None,
        _ => return None,
    })
}

fn answer_word(r: &str) -> Option<String> {
    Some(
        match r {
            "accepted" => "accepted",
            "declined" => "declined",
            "tentativelyAccepted" => "tentative",
            "notResponded" | "none" => "needs-action",
            _ => return None,
        }
        .to_string(),
    )
}

/// A Graph time: `2026-10-05T13:00:00.0000000` in `timeZone`.
fn when(v: &Value, all_day: bool) -> Option<When> {
    let s = v["dateTime"].as_str()?;
    let s = s.split('.').next().unwrap_or(s);
    let at = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()?;
    let tz = v["timeZone"].as_str().unwrap_or("UTC");
    if all_day {
        // An all-day one is midnight to midnight in its own zone; read in
        // UTC it can land either side of one, so take the nearest.
        let date = if at.hour() >= 12 {
            at.date() + Duration::days(1)
        } else {
            at.date()
        };
        return Some(When::Date { date });
    }
    let tz = super::zone(tz)
        .map(|z| z.name().to_string())
        .unwrap_or_else(|| "UTC".into());
    Some(When::Time { at, tz: Some(tz) })
}

fn person(v: &Value) -> Attendee {
    Attendee {
        name: v["emailAddress"]["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .map(str::to_string),
        email: v["emailAddress"]["address"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        response: v["status"]["response"].as_str().and_then(answer_word),
        is_self: false,
        organizer: false,
    }
}

/// A Graph event as an [`Item`]. Each occurrence of a series is its own,
/// grouped by the series' id, which a change to the whole series names.
pub fn item(v: &Value, me: Option<&str>) -> Option<Item> {
    let id = v["id"].as_str()?;
    let all_day = v["isAllDay"].as_bool().unwrap_or(false);
    let start = when(&v["start"], all_day)?;
    let kind = v["type"].as_str().unwrap_or("singleInstance");
    if kind == "seriesMaster" {
        // Its occurrences come on their own.
        return None;
    }
    let uid = v["seriesMasterId"].as_str().unwrap_or(id).to_string();
    let is_me = |e: &str| me.is_some_and(|m| m.eq_ignore_ascii_case(e));
    let organizer = v
        .get("organizer")
        .filter(|o| o.is_object())
        .map(|o| Attendee {
            organizer: true,
            is_self: v["isOrganizer"].as_bool().unwrap_or(false),
            ..person(o)
        });
    let mut attendees: Vec<Attendee> = v["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["type"] != "resource")
        .map(|a| {
            let mut p = person(a);
            p.is_self = is_me(&p.email);
            p
        })
        .collect();
    // Microsoft keeps the user's own answer apart from the list.
    let mine = v["responseStatus"]["response"]
        .as_str()
        .and_then(answer_word);
    let invited = !v["isOrganizer"].as_bool().unwrap_or(false) && !attendees.is_empty();
    match attendees.iter_mut().find(|a| a.is_self) {
        Some(a) => a.response = mine.or(a.response.take()),
        None if invited => {
            if let (Some(email), Some(response)) = (me, mine) {
                attendees.push(Attendee {
                    name: None,
                    email: email.to_string(),
                    response: Some(response),
                    is_self: true,
                    organizer: false,
                });
            }
        }
        None => {}
    }
    let html = v["body"]["contentType"] == "html";
    let content = v["body"]["content"].as_str().unwrap_or_default();
    let description = if html {
        super::google::plain(content)
    } else {
        content.to_string()
    };
    let location = v["location"]["displayName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let meeting = v["onlineMeeting"]["joinUrl"]
        .as_str()
        .or(v["onlineMeetingUrl"].as_str())
        .map(str::to_string)
        .or_else(|| super::meeting_link([location.as_deref(), Some(content)]));
    Some(Item {
        uid,
        recurrence_id: (kind != "singleInstance").then(|| start.clone()),
        cancelled: v["isCancelled"].as_bool().unwrap_or(false),
        tentative: v["showAs"] == "tentative",
        title: v["subject"].as_str().unwrap_or_default().to_string(),
        description: Some(description.trim().to_string()).filter(|s| !s.is_empty()),
        location,
        end: when(&v["end"], all_day),
        start,
        rrule: Vec::new(),
        rdate: Vec::new(),
        exdate: Vec::new(),
        attendees,
        organizer,
        meeting_url: meeting,
        link: v["webLink"].as_str().map(str::to_string),
        transparent: matches!(v["showAs"].as_str(), Some("free" | "workingElsewhere")),
    })
}

/// An RRULE as Microsoft's recurrence pattern and range, for a series
/// starting on `start` in zone `tz`.
pub fn pattern(rule: &str, start: NaiveDate, tz: chrono_tz::Tz) -> Result<Value, String> {
    let mut freq = "";
    let mut interval = 1;
    let mut byday: Vec<String> = Vec::new();
    let mut bymonthday: Option<u32> = None;
    let mut until: Option<NaiveDate> = None;
    let mut count: Option<u32> = None;
    for part in rule.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = if v.eq_ignore_ascii_case("DAILY") {
                    "daily"
                } else if v.eq_ignore_ascii_case("WEEKLY") {
                    "weekly"
                } else if v.eq_ignore_ascii_case("MONTHLY") {
                    "monthly"
                } else if v.eq_ignore_ascii_case("YEARLY") {
                    "yearly"
                } else {
                    return Err(format!("Microsoft can't repeat by {v}"));
                }
            }
            "INTERVAL" => interval = v.parse().unwrap_or(1),
            "BYDAY" => byday = v.split(',').map(str::to_string).collect(),
            "BYMONTHDAY" => bymonthday = v.parse().ok(),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => {
                until = if v.len() == 8 {
                    NaiveDate::parse_from_str(v, "%Y%m%d").ok()
                } else {
                    NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
                        .ok()
                        .map(|t| chrono::TimeZone::from_utc_datetime(&tz, &t).date_naive())
                }
            }
            _ => {}
        }
    }
    let day = |code: &str| -> Option<&'static str> {
        Some(match &code[code.len().saturating_sub(2)..] {
            "MO" => "monday",
            "TU" => "tuesday",
            "WE" => "wednesday",
            "TH" => "thursday",
            "FR" => "friday",
            "SA" => "saturday",
            "SU" => "sunday",
            _ => return None,
        })
    };
    let weekday = day(super::write::byday(start.weekday())).unwrap_or("monday");
    let index = |code: &str| -> &'static str {
        match code.trim_end_matches(char::is_alphabetic) {
            "1" | "+1" => "first",
            "2" | "+2" => "second",
            "3" | "+3" => "third",
            "4" | "+4" => "fourth",
            "-1" => "last",
            _ => "first",
        }
    };
    let pattern = match freq {
        "daily" if !byday.is_empty() => json!({
            "type": "weekly", "interval": interval,
            "daysOfWeek": byday.iter().filter_map(|d| day(d)).collect::<Vec<_>>(),
        }),
        "daily" => json!({ "type": "daily", "interval": interval }),
        "weekly" => {
            let days: Vec<&str> = byday.iter().filter_map(|d| day(d)).collect();
            json!({
                "type": "weekly", "interval": interval,
                "daysOfWeek": if days.is_empty() { vec![weekday] } else { days },
                "firstDayOfWeek": "sunday",
            })
        }
        "monthly" => match byday.first() {
            Some(d) if d.len() > 2 => json!({
                "type": "relativeMonthly", "interval": interval,
                "daysOfWeek": [day(d)], "index": index(d),
            }),
            _ => json!({
                "type": "absoluteMonthly", "interval": interval,
                "dayOfMonth": bymonthday.unwrap_or(start.day()),
            }),
        },
        "yearly" => json!({
            "type": "absoluteYearly", "interval": interval,
            "month": start.month(), "dayOfMonth": start.day(),
        }),
        _ => return Err("a repeat rule needs a FREQ".into()),
    };
    let range = match (until, count) {
        (Some(u), _) => {
            json!({ "type": "endDate", "startDate": start.to_string(), "endDate": u.to_string() })
        }
        (None, Some(n)) => {
            json!({ "type": "numbered", "startDate": start.to_string(), "numberOfOccurrences": n })
        }
        (None, None) => json!({ "type": "noEnd", "startDate": start.to_string() }),
    };
    Ok(json!({ "pattern": pattern, "range": range }))
}

/// A draft as a Graph event. `whole` says whether the repeat belongs in it.
fn body(draft: &Draft, whole: bool) -> Result<Value, SyncError> {
    let tz = draft.zone().map_err(SyncError::Other)?;
    let time = |s: &str| {
        let s = if draft.all_day {
            format!("{s}T00:00:00")
        } else if s.len() == 16 {
            format!("{s}:00")
        } else {
            s.to_string()
        };
        json!({ "dateTime": s, "timeZone": draft.time_zone })
    };
    let mut b = json!({
        "subject": draft.title.trim(),
        "body": { "contentType": "text", "content": draft.description.as_deref().unwrap_or("").trim() },
        "start": time(&draft.start),
        "end": time(&draft.end),
        "isAllDay": draft.all_day,
        "location": { "displayName": draft.location.as_deref().unwrap_or("").trim() },
        "showAs": if draft.busy { "busy" } else { "free" },
        "attendees": draft.attendees.iter().map(|g| json!({
            "type": "required",
            "emailAddress": { "address": g.email.trim(), "name": g.name.as_deref().unwrap_or(g.email.trim()) },
        })).collect::<Vec<_>>(),
    });
    if whole {
        let start = super::write::start_date(draft)
            .ok_or_else(|| SyncError::Other("the Calendar event has no start date".into()))?;
        b["recurrence"] = match draft.rrule().map_err(SyncError::Other)? {
            Some(rule) => pattern(&rule, start, tz).map_err(SyncError::Other)?,
            None => Value::Null,
        };
    }
    Ok(b)
}

impl Provider for Graph<'_> {
    async fn sync(&mut self, cache: &mut AccountCache) -> Result<bool, SyncError> {
        if self.me.is_none() {
            self.me = self.address().await.ok();
        }
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
                    selected: true,
                    ..Default::default()
                });
            let name = entry["name"].as_str().unwrap_or("Calendar").to_string();
            let color = entry["hexColor"]
                .as_str()
                .filter(|c| c.starts_with('#'))
                .map(str::to_string)
                .or_else(|| {
                    entry["color"]
                        .as_str()
                        .and_then(named_color)
                        .map(str::to_string)
                });
            let primary = entry["isDefaultCalendar"].as_bool().unwrap_or(false);
            let writable = entry["canEdit"].as_bool().unwrap_or(true);
            if (&cal.name, &cal.color, cal.primary, cal.writable)
                != (&name, &color, primary, writable)
            {
                (cal.name, cal.color, cal.primary, cal.writable) = (name, color, primary, writable);
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
        let url = format!("{}/me/calendars/{}/events", self.api, escape(&cal.id));
        let b = body(draft, true)?;
        self.call(Method::POST, &url, Some(&b)).await.map(|_| ())
    }

    async fn update(
        &mut self,
        _cal: &CachedCalendar,
        found: &Found,
        draft: &Draft,
        scope: Scope,
    ) -> Result<(), SyncError> {
        let id = Self::target(found, scope)?;
        let whole = !(found.repeats() && scope == Scope::This);
        let b = body(draft, whole)?;
        let url = format!("{}/me/events/{}", self.api, escape(&id));
        self.call(Method::PATCH, &url, Some(&b)).await.map(|_| ())
    }

    async fn delete(
        &mut self,
        _cal: &CachedCalendar,
        found: &Found,
        scope: Scope,
    ) -> Result<(), SyncError> {
        let id = Self::target(found, scope)?;
        let url = format!("{}/me/events/{}", self.api, escape(&id));
        self.call(Method::DELETE, &url, None).await.map(|_| ())
    }

    async fn respond(
        &mut self,
        _cal: &CachedCalendar,
        found: &Found,
        answer: Answer,
        scope: Scope,
    ) -> Result<(), SyncError> {
        let id = Self::target(found, scope)?;
        let verb = match answer {
            Answer::Accepted => "accept",
            Answer::Tentative => "tentativelyAccept",
            Answer::Declined => "decline",
        };
        let url = format!("{}/me/events/{}/{verb}", self.api, escape(&id));
        self.call(Method::POST, &url, Some(&json!({ "sendResponse": true })))
            .await
            .map(|_| ())
    }
}
