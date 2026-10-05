//! The Calendar Space (ADR 0017): the user's calendars, read from their
//! provider and kept here only as a cache the Host can throw away.
//!
//! An account is either Google, through its Calendar API, or any CalDAV
//! server. Both sit behind [`Provider`], whose one job is to bring a cache
//! level with the server as cheaply as the server allows: Google's sync
//! tokens, CalDAV's ctag and etags. Everything after that — repeating Calendar
//! events, time zones, search, free/busy — works on the cache alone, so it is
//! the same for both (see [`expand`]).
//!
//! A calendar entry is a [`CalendarEvent`], never an "event", which already
//! means a stream event in this codebase.
//!
//! What lives on disk, under the state directory's `calendar/`:
//! - `accounts.json`: the accounts and their secrets (tokens, passwords),
//!   readable only by the user, for the same reason the Host's socket is.
//! - `google-client.json`: the OAuth client the user made for themselves (see
//!   `docs/google-calendar.md`), also readable only by the user.
//! - `cache/<account>.json`: what the provider last said, rebuilt from it if
//!   it's lost.

pub mod caldav;
pub mod expand;
pub mod google;
pub mod ical;
pub mod mcp;
mod store;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::new_id;
pub use store::GoogleClient;

/// How often every account is synced while nothing else asks for it.
pub const SYNC_EVERY: Duration = Duration::from_secs(5 * 60);

/// Bounded so a server that takes the connection and never answers fails the
/// sync, rather than holding it, and every one after it, forever.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// A date or a time, as a provider gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum When {
    /// A whole day: an all-day Calendar event. It has no zone, so it is the
    /// same day wherever it is read.
    Date { date: NaiveDate },
    /// A wall-clock time in the IANA zone `tz` (`"UTC"` for UTC). A time with
    /// no zone ("floating") is read in the Host's own.
    Time {
        at: NaiveDateTime,
        tz: Option<String>,
    },
}

impl When {
    /// This moment moved on by `secs`: whole days for a date, so a duration
    /// written as hours still lands on a day.
    pub fn plus_seconds(&self, secs: i64) -> When {
        match self {
            When::Date { date } => When::Date {
                date: *date + chrono::Duration::days((secs as f64 / 86_400.0).ceil() as i64),
            },
            When::Time { at, tz } => When::Time {
                at: *at + chrono::Duration::seconds(secs),
                tz: tz.clone(),
            },
        }
    }

    /// The zone this time is in, or `home` for a floating time or a date.
    pub fn zone(&self, home: Tz) -> Tz {
        match self {
            When::Time { tz: Some(name), .. } => zone(name).unwrap_or(home),
            _ => home,
        }
    }

    /// The instant this is, reading a floating time and a date's midnight in
    /// `home`.
    pub fn instant(&self, home: Tz) -> DateTime<Utc> {
        let (naive, tz) = match self {
            When::Date { date } => (date.and_time(ical::midnight()), home),
            When::Time { at, .. } => (*at, self.zone(home)),
        };
        local_instant(tz, naive)
    }
}

/// A wall-clock time in `tz` as an instant. A time that happens twice, as the
/// clocks go back, is the first; one that never happens, as they go forward,
/// is read as if they hadn't yet, which lands it an hour later — what every
/// calendar app does with a 02:30 meeting on the night the clocks spring.
pub fn local_instant(tz: Tz, naive: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(first, _) => first.with_timezone(&Utc),
        chrono::LocalResult::None => {
            let before = tz.offset_from_utc_datetime(&(naive - chrono::Duration::hours(12)));
            let utc = naive
                - chrono::Duration::seconds(i64::from(
                    chrono::Offset::fix(&before).local_minus_utc(),
                ));
            Utc.from_utc_datetime(&utc)
        }
    }
}

/// An IANA zone by name, also taking the forms servers wrap them in — a
/// `/mozilla.org/…/Europe/Paris` prefix, or a Windows name from Exchange.
pub fn zone(name: &str) -> Option<Tz> {
    let name = name.trim();
    if let Ok(tz) = name.parse::<Tz>() {
        return Some(tz);
    }
    // A prefixed path: try its last two segments, then its last.
    let parts: Vec<&str> = name.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        if let Ok(tz) = parts[parts.len() - 2..].join("/").parse::<Tz>() {
            return Some(tz);
        }
    }
    let windows = match name {
        "UTC" | "Coordinated Universal Time" | "GMT" => "UTC",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "US Mountain Standard Time" => "America/Phoenix",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Alaskan Standard Time" => "America/Anchorage",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Atlantic Standard Time" => "America/Halifax",
        "Newfoundland Standard Time" => "America/St_Johns",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Romance Standard Time" => "Europe/Paris",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "GTB Standard Time" => "Europe/Bucharest",
        "Russian Standard Time" => "Europe/Moscow",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "Singapore Standard Time" => "Asia/Singapore",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        _ => return None,
    };
    windows.parse().ok()
}

/// The Host's own zone, for floating times and all-day free/busy.
pub fn home_zone() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| zone(&name))
        .unwrap_or(Tz::UTC)
}

/// Someone on a Calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attendee {
    pub name: Option<String>,
    pub email: String,
    /// `accepted`, `declined`, `tentative` or `needs-action`, when known.
    pub response: Option<String>,
    /// The user themselves.
    #[serde(default, rename = "self")]
    pub is_self: bool,
    #[serde(default)]
    pub organizer: bool,
}

/// One Calendar event as its provider stores it: a single one, the master of
/// a repeating one, or one occurrence of a repeating one the user changed
/// (an exception, which has a `recurrence_id`). Exceptions share their
/// master's `uid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub uid: String,
    /// Which occurrence of the master this one replaces.
    pub recurrence_id: Option<When>,
    /// Gone: a deleted occurrence of a repeating Calendar event.
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub tentative: bool,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: When,
    /// Exclusive. None means it ends as it starts, or for a date, that day.
    pub end: Option<When>,
    /// `RRULE` values, as written (`FREQ=WEEKLY;BYDAY=MO`).
    #[serde(default)]
    pub rrule: Vec<String>,
    #[serde(default)]
    pub rdate: Vec<When>,
    #[serde(default)]
    pub exdate: Vec<When>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    pub organizer: Option<Attendee>,
    pub meeting_url: Option<String>,
    /// The Calendar event on the provider's own site, where it has one.
    pub link: Option<String>,
    /// Shown as free rather than busy.
    #[serde(default)]
    pub transparent: bool,
}

impl Item {
    pub fn repeats(&self) -> bool {
        !self.rrule.is_empty() || !self.rdate.is_empty()
    }
}

/// One occurrence of a Calendar event, as a window or an Agent sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Unique among every occurrence of every Calendar event in the range.
    pub id: String,
    pub account_id: String,
    pub calendar_id: String,
    pub title: String,
    /// An all-day Calendar event's first day, `YYYY-MM-DD`; anything else's
    /// start as an RFC 3339 instant in UTC.
    pub start: String,
    /// Exclusive: the day after the last, or the instant it ends.
    pub end: String,
    pub all_day: bool,
    /// The zone it was made in, where that isn't UTC or the Host's own.
    pub time_zone: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Vec<Attendee>,
    pub organizer: Option<Attendee>,
    pub meeting_url: Option<String>,
    pub link: Option<String>,
    /// One occurrence of a repeating Calendar event.
    pub recurring: bool,
    pub tentative: bool,
    /// Shown as busy, for free/busy.
    pub busy: bool,
}

/// Find a video-call link in the given texts: the meeting a Calendar event's
/// detail offers to join.
pub fn meeting_link<'a>(texts: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    const HOSTS: [&str; 8] = [
        "meet.google.com/",
        "zoom.us/j/",
        "zoom.us/my/",
        "teams.microsoft.com/l/meetup-join",
        "teams.live.com/meet",
        "webex.com/",
        "whereby.com/",
        "meet.jit.si/",
    ];
    for text in texts.into_iter().flatten() {
        for word in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'')) {
            let word = word.trim_end_matches([')', ',', '.', ';']);
            if word.starts_with("https://") && HOSTS.iter().any(|h| word.contains(h)) {
                return Some(word.to_string());
            }
        }
    }
    None
}

/// What one calendar holds, as the provider last said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedCalendar {
    /// The provider's id: Google's calendar id, or the CalDAV collection's URL.
    pub id: String,
    pub name: String,
    /// `#rrggbb`, as the provider colours it.
    pub color: Option<String>,
    #[serde(default)]
    pub primary: bool,
    /// Whether the provider shows it by default.
    #[serde(default = "yes")]
    pub selected: bool,
    /// Google's `nextSyncToken`, or CalDAV's `getctag`: how to ask the server
    /// only what changed since.
    pub sync_token: Option<String>,
    /// What the server holds, by resource: a Google event id, or a CalDAV
    /// object's href. A CalDAV object holds a master and its exceptions.
    #[serde(default)]
    pub resources: BTreeMap<String, Resource>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub etag: Option<String>,
    pub items: Vec<Item>,
}

/// One account's cache.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountCache {
    pub calendars: Vec<CachedCalendar>,
}

/// Why a sync failed.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncError {
    /// The provider no longer accepts the credentials: the user has to
    /// reconnect the account.
    Auth(String),
    /// Anything else, likely to pass: the network, the server.
    Other(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Auth(m) | SyncError::Other(m) => f.write_str(m),
        }
    }
}

/// Where the Calendar Space's data comes from. The provider is the source of
/// truth; the cache is only ever brought level with it.
pub trait Provider: Send {
    /// Bring `cache` level with the server, fetching only what changed since
    /// the last sync where the server can tell. True if anything changed.
    fn sync<'a>(
        &'a mut self,
        cache: &'a mut AccountCache,
    ) -> impl Future<Output = Result<bool, SyncError>> + Send + 'a;
}

/// An account and its secrets, as kept in `accounts.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// The address or user name it signs in as.
    pub name: String,
    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Google(google::Tokens),
    Caldav(caldav::Login),
}

impl Source {
    fn kind(&self) -> &'static str {
        match self {
            Source::Google(_) => "google",
            Source::Caldav(_) => "caldav",
        }
    }
}

/// Where an account stands, for the window to show.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Never synced since the Host started; the cache is what it shows.
    Idle,
    Syncing,
    Ok {
        at: String,
    },
    /// The last sync failed; it is tried again on the next.
    Error {
        message: String,
    },
    /// The provider refused the credentials; only reconnecting helps.
    Reconnect {
        message: String,
    },
}

/// An account as a window sees it: no secrets.
#[derive(Debug, Clone, Serialize)]
pub struct AccountInfo {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub status: Status,
    pub calendars: Vec<CalendarInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub primary: bool,
    /// Whether to show it until the user says otherwise.
    pub selected: bool,
}

/// Everything the Calendar Space opens on.
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub accounts: Vec<AccountInfo>,
    /// Whether the user has given this Host a Google OAuth client to sign in
    /// with. Without one, Google can't be connected.
    pub google_client: bool,
    /// Where that client goes, for the setup steps to name.
    pub google_client_path: String,
}

/// A stretch of time, as free/busy answers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreeBusy {
    pub busy: Vec<Span>,
    pub free: Vec<Span>,
}

/// How a calendar change reaches every window: an event by `name`.
pub type Notify = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// The Calendar Space on a Host: its accounts, their caches, and the syncing
/// that keeps those level with each provider.
pub struct Calendars {
    dir: PathBuf,
    http: reqwest::Client,
    pub(crate) google: google::Endpoints,
    accounts: Mutex<Vec<Account>>,
    caches: Mutex<HashMap<String, AccountCache>>,
    status: Mutex<HashMap<String, Status>>,
    /// Held for a whole sync, so two never run over one cache at once.
    syncing: tokio::sync::Mutex<()>,
    /// A Google sign-in waiting for its browser, to abandon if another starts.
    signing_in: Mutex<Option<tokio::task::JoinHandle<()>>>,
    notify: Notify,
    home: Tz,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// A time a window or an Agent sent: RFC 3339, or a bare date read as its
/// midnight in UTC.
pub fn parse_time(s: &str) -> Result<DateTime<FixedOffset>, String> {
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t);
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| {
            Utc.from_utc_datetime(&d.and_time(ical::midnight()))
                .fixed_offset()
        })
        .map_err(|_| format!("not a time: {s} (expected RFC 3339, like 2026-10-04T09:00:00-03:00)"))
}

impl Calendars {
    /// The Calendar Space kept in `dir`, telling windows of changes through
    /// `notify`.
    pub fn new(dir: PathBuf, notify: Notify) -> Arc<Self> {
        Self::with_endpoints(dir, notify, google::Endpoints::default())
    }

    pub(crate) fn with_endpoints(
        dir: PathBuf,
        notify: Notify,
        google: google::Endpoints,
    ) -> Arc<Self> {
        let accounts = store::load_accounts(&dir).unwrap_or_else(|e| {
            eprintln!("calendar: could not read the accounts: {e}");
            Vec::new()
        });
        let caches = accounts
            .iter()
            .map(|a| (a.id.clone(), store::load_cache(&dir, &a.id)))
            .collect();
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_default();
        Arc::new(Self {
            dir,
            http,
            google,
            accounts: Mutex::new(accounts),
            caches: Mutex::new(caches),
            status: Mutex::new(HashMap::new()),
            syncing: tokio::sync::Mutex::new(()),
            signing_in: Mutex::new(None),
            notify,
            home: home_zone(),
        })
    }

    /// The default: under the state directory.
    pub fn dir() -> Result<PathBuf, String> {
        Ok(crate::paths::state_dir().map_err(err)?.join("calendar"))
    }

    /// Sync every account now and every [`SYNC_EVERY`] after, for as long as
    /// the Host runs.
    pub fn keep_syncing(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                me.sync_all().await;
                tokio::time::sleep(SYNC_EVERY).await;
            }
        });
    }

    fn changed(&self) {
        // An object, so the window can stamp it with the Host it came from.
        (self.notify)("calendar-changed", serde_json::json!({}));
    }

    pub fn overview(&self) -> Overview {
        let accounts = self.accounts.lock().unwrap().clone();
        let caches = self.caches.lock().unwrap();
        let status = self.status.lock().unwrap();
        Overview {
            accounts: accounts
                .iter()
                .map(|a| AccountInfo {
                    id: a.id.clone(),
                    kind: a.source.kind(),
                    name: a.name.clone(),
                    status: status.get(&a.id).cloned().unwrap_or(Status::Idle),
                    calendars: caches
                        .get(&a.id)
                        .map(|c| {
                            c.calendars
                                .iter()
                                .map(|cal| CalendarInfo {
                                    id: cal.id.clone(),
                                    name: cal.name.clone(),
                                    color: cal.color.clone(),
                                    primary: cal.primary,
                                    selected: cal.selected,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect(),
            google_client: store::load_google_client(&self.dir).is_some(),
            google_client_path: store::google_client_path(&self.dir).display().to_string(),
        }
    }

    /// Every occurrence of every Calendar event that overlaps `from`..`to`,
    /// soonest first. All-day ones are matched by date in `from`'s offset,
    /// which is the asking window's own.
    pub fn events(&self, from: &str, to: &str) -> Result<Vec<CalendarEvent>, String> {
        let (from, to) = (parse_time(from)?, parse_time(to)?);
        if to <= from {
            return Err("the range ends before it starts".into());
        }
        let accounts = self.accounts.lock().unwrap().clone();
        let caches = self.caches.lock().unwrap();
        let mut out = Vec::new();
        for account in &accounts {
            let Some(cache) = caches.get(&account.id) else {
                continue;
            };
            let me = self_email(account);
            for cal in &cache.calendars {
                let items: Vec<&Item> = cal.resources.values().flat_map(|r| &r.items).collect();
                for occ in expand::expand(&items, from, to, self.home) {
                    out.push(expand::to_event(
                        &occ,
                        &account.id,
                        &cal.id,
                        me.as_deref(),
                        self.home,
                    ));
                }
            }
        }
        out.sort_by_key(sort_key);
        Ok(out)
    }

    /// Calendar events in `from`..`to` whose title, place, notes or people
    /// mention `query`, at most `limit`, keeping those nearest now.
    pub fn search(
        &self,
        query: &str,
        from: Option<&str>,
        to: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<CalendarEvent>, String> {
        let now = Utc::now().fixed_offset();
        let from = from
            .map(str::to_string)
            .unwrap_or_else(|| (now - chrono::Duration::days(365)).to_rfc3339());
        let to = to
            .map(str::to_string)
            .unwrap_or_else(|| (now + chrono::Duration::days(365)).to_rfc3339());
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut found: Vec<CalendarEvent> = self
            .events(&from, &to)?
            .into_iter()
            .filter(|e| {
                let hay = haystack(e);
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        let limit = limit.unwrap_or(50);
        if found.len() > limit {
            let home = self.home;
            found.sort_by_key(|e| {
                (event_instant(&e.start, home) - now.to_utc())
                    .num_seconds()
                    .abs()
            });
            found.truncate(limit);
            found.sort_by_key(sort_key);
        }
        Ok(found)
    }

    /// When the user is busy in `from`..`to`, and the gaps between. Only
    /// timed Calendar events count, and not those marked free or declined:
    /// an all-day one is a birthday or a holiday far more often than a day
    /// the user can't be reached.
    pub fn free_busy(&self, from: &str, to: &str) -> Result<FreeBusy, String> {
        let (start, end) = (parse_time(from)?.to_utc(), parse_time(to)?.to_utc());
        let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = self
            .events(from, to)?
            .into_iter()
            .filter(|e| !e.all_day && e.busy)
            .filter_map(|e| {
                let s = DateTime::parse_from_rfc3339(&e.start)
                    .ok()?
                    .to_utc()
                    .max(start);
                let t = DateTime::parse_from_rfc3339(&e.end).ok()?.to_utc().min(end);
                (t > s).then_some((s, t))
            })
            .collect();
        spans.sort();
        let mut busy: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
        for (s, t) in spans {
            match busy.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(t),
                _ => busy.push((s, t)),
            }
        }
        let mut free = Vec::new();
        let mut cursor = start;
        for (s, t) in &busy {
            if *s > cursor {
                free.push((cursor, *s));
            }
            cursor = cursor.max(*t);
        }
        if cursor < end {
            free.push((cursor, end));
        }
        let span = |(s, t): (DateTime<Utc>, DateTime<Utc>)| Span {
            start: s.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            end: t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        Ok(FreeBusy {
            busy: busy.into_iter().map(span).collect(),
            free: free.into_iter().map(span).collect(),
        })
    }

    /// Sync every account, one after another, and tell windows what changed.
    pub async fn sync_all(&self) {
        let ids: Vec<String> = self
            .accounts
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.id.clone())
            .collect();
        for id in ids {
            self.sync_account(&id).await;
        }
    }

    fn set_status(&self, id: &str, status: Status) {
        self.status.lock().unwrap().insert(id.to_string(), status);
    }

    /// Sync one account. Its cache and its secrets are copied out, brought
    /// level without any lock held over the network, and put back.
    pub async fn sync_account(&self, id: &str) {
        let _one_at_a_time = self.syncing.lock().await;
        let Some(mut account) = self
            .accounts
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.id == id)
            .cloned()
        else {
            return;
        };
        let mut cache = self
            .caches
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default();
        self.set_status(id, Status::Syncing);
        self.changed();

        let before = account.clone();
        let outcome = match &mut account.source {
            Source::Google(tokens) => match store::load_google_client(&self.dir) {
                Some(client) => {
                    google::Google::new(&self.http, &self.google, &client, tokens)
                        .sync(&mut cache)
                        .await
                }
                None => Err(SyncError::Auth(format!(
                    "this Host has no Google OAuth client; put one in {}",
                    store::google_client_path(&self.dir).display()
                ))),
            },
            Source::Caldav(login) => {
                caldav::CalDav::new(&self.http, login)
                    .sync(&mut cache)
                    .await
            }
        };

        // The account may have been removed while it synced.
        let still_here = {
            let mut accounts = self.accounts.lock().unwrap();
            match accounts.iter_mut().find(|a| a.id == id) {
                Some(a) => {
                    if account != before {
                        *a = account.clone();
                    }
                    true
                }
                None => false,
            }
        };
        if !still_here {
            return;
        }
        if account != before {
            if let Err(e) = self.save_accounts() {
                eprintln!("calendar: could not save the accounts: {e}");
            }
        }
        let status = match outcome {
            Ok(changed) => {
                if changed {
                    if let Err(e) = store::save_cache(&self.dir, id, &cache) {
                        eprintln!("calendar: could not save the cache: {e}");
                    }
                    self.caches.lock().unwrap().insert(id.to_string(), cache);
                }
                Status::Ok {
                    at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                }
            }
            Err(SyncError::Auth(message)) => Status::Reconnect { message },
            Err(SyncError::Other(message)) => Status::Error { message },
        };
        self.set_status(id, status);
        self.changed();
    }

    fn save_accounts(&self) -> Result<(), String> {
        let accounts = self.accounts.lock().unwrap().clone();
        store::save_accounts(&self.dir, &accounts)
    }

    /// Add an account, replacing one of the same kind and name, sync it, and
    /// say so.
    pub(crate) async fn add(&self, account: Account) -> Result<AccountInfo, String> {
        let id = {
            let mut accounts = self.accounts.lock().unwrap();
            let id = match accounts
                .iter_mut()
                .find(|a| a.name == account.name && a.source.kind() == account.source.kind())
            {
                // Reconnecting: keep its id, and so its cache and the
                // window's choices about which calendars to show.
                Some(existing) => {
                    existing.source = account.source;
                    existing.id.clone()
                }
                None => {
                    let id = account.id.clone();
                    accounts.push(account);
                    id
                }
            };
            id
        };
        self.save_accounts()?;
        self.caches.lock().unwrap().entry(id.clone()).or_default();
        self.sync_account(&id).await;
        self.overview()
            .accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| "the account was removed as it was added".into())
    }

    /// Connect a CalDAV account, after checking the server takes the login.
    pub async fn add_caldav(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<AccountInfo, String> {
        let mut login = caldav::Login {
            url: url.trim().to_string(),
            username: username.trim().to_string(),
            password: password.to_string(),
            home: None,
        };
        caldav::CalDav::new(&self.http, &mut login)
            .discover()
            .await
            .map_err(|e| e.to_string())?;
        let name = if login.username.is_empty() {
            reqwest::Url::parse(&login.url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| login.url.clone())
        } else {
            login.username.clone()
        };
        self.add(Account {
            id: new_id(),
            name,
            source: Source::Caldav(login),
        })
        .await
    }

    /// Forget an account: its secrets and its cache. The provider keeps
    /// everything; nothing there is touched.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.accounts.lock().unwrap().retain(|a| a.id != id);
        self.caches.lock().unwrap().remove(id);
        self.status.lock().unwrap().remove(id);
        self.save_accounts()?;
        store::remove_cache(&self.dir, id);
        self.changed();
        Ok(())
    }

    /// Keep the Google OAuth client the user made, as the setup doc has them
    /// paste it.
    pub fn set_google_client(&self, client_id: &str, client_secret: &str) -> Result<(), String> {
        let client = GoogleClient {
            client_id: client_id.trim().to_string(),
            client_secret: client_secret.trim().to_string(),
        };
        if client.client_id.is_empty() || client.client_secret.is_empty() {
            return Err("both the client ID and the client secret are needed".into());
        }
        store::save_google_client(&self.dir, &client)?;
        self.changed();
        Ok(())
    }

    /// Start signing in to Google: the URL for the user's browser. The Host
    /// waits for the browser to come back to it on this machine, then adds
    /// the account and tells every window.
    pub async fn begin_google(self: &Arc<Self>) -> Result<String, String> {
        let client = store::load_google_client(&self.dir).ok_or_else(|| {
            format!(
                "this Host has no Google OAuth client yet; see docs/google-calendar.md, or put one in {}",
                store::google_client_path(&self.dir).display()
            )
        })?;
        let pending = google::SignIn::start(&self.google, &client).await?;
        let url = pending.url.clone();
        let me = self.clone();
        let task = tokio::spawn(async move {
            let outcome = match pending.finish(&me.http, &me.google, &client).await {
                Ok((name, tokens)) => me
                    .add(Account {
                        id: new_id(),
                        name,
                        source: Source::Google(tokens),
                    })
                    .await
                    .map(|_| ()),
                Err(e) => Err(e),
            };
            if let Err(e) = outcome {
                eprintln!("calendar: Google sign-in failed: {e}");
                (me.notify)(
                    "calendar-sign-in-failed",
                    serde_json::json!({ "message": e }),
                );
            }
        });
        if let Some(old) = self.signing_in.lock().unwrap().replace(task) {
            old.abort();
        }
        Ok(url)
    }
}

/// The user's own address on an account, to tell which attendee is them.
fn self_email(account: &Account) -> Option<String> {
    account
        .name
        .contains('@')
        .then(|| account.name.to_lowercase())
}

/// Sorts all-day Calendar events before timed ones on the same day.
fn sort_key(e: &CalendarEvent) -> (String, bool, String) {
    (
        e.start.chars().take(10).collect(),
        !e.all_day,
        e.start.clone(),
    )
}

fn event_instant(s: &str, home: Tz) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.to_utc())
        .unwrap_or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| When::Date { date: d }.instant(home))
                .unwrap_or_default()
        })
}

fn haystack(e: &CalendarEvent) -> String {
    let mut s = e.title.to_lowercase();
    for part in [&e.location, &e.description].into_iter().flatten() {
        s.push('\n');
        s.push_str(&part.to_lowercase());
    }
    for a in &e.attendees {
        s.push('\n');
        s.push_str(&a.email.to_lowercase());
        if let Some(n) = &a.name {
            s.push(' ');
            s.push_str(&n.to_lowercase());
        }
    }
    s
}
