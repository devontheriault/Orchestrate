//! The Calendar Space's MCP tools (see `crate::mcp` for how tools work). They
//! read the Host's cache of the user's calendars, and may draft a Calendar
//! event. Nothing here sends: a Calendar event, an invitation or an answer to
//! one is the user's to send, from the Space (ADR 0017).

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{CalendarEvent, FreeBusy};
use crate::mcp::{Host, Tool};

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::reads(
            "calendar_list_events",
            "List the Calendar events in the user's calendars between two times, soonest \
             first: title, when, where, who and the meeting link. Repeating ones come as \
             each occurrence. Times are in the user's own zone.",
            list_events,
        ),
        Tool::reads(
            "calendar_search",
            "Find Calendar events whose title, place, notes or people mention every word \
             of a query, within a year either side of today unless a range is given.",
            search,
        ),
        Tool::reads(
            "calendar_free_busy",
            "When the user is busy and when they're free between two times, from their \
             calendars: timed Calendar events they haven't declined or marked as free. \
             Use it to suggest a time.",
            free_busy,
        ),
        Tool::writes(
            "calendar_draft_event",
            "Draft a Calendar event for the user to look over: it appears in their \
             Calendar Space, marked as a draft, and nothing is saved to their calendar or \
             sent to anyone until they send it themselves. Use it to suggest a meeting, \
             a reminder or a block of time; check calendar_free_busy first.",
            draft_event,
        ),
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct DraftArgs {
    /// What it's called.
    pub(super) title: String,
    /// When it starts: RFC 3339 (2026-10-05T14:00:00-03:00), a local time
    /// (2026-10-05T14:00) in the user's zone, or a date for an all-day one.
    pub(super) start: String,
    /// When it ends, the same way, exclusive. For an all-day one, the day after
    /// the last.
    pub(super) end: String,
    pub(super) location: Option<String>,
    /// Notes for the Calendar event itself.
    pub(super) description: Option<String>,
    /// Email addresses of people to invite. They're only invited if the user
    /// sends it.
    pub(super) attendees: Option<Vec<String>>,
    /// How it repeats, as an iCalendar RRULE like FREQ=WEEKLY;BYDAY=MO.
    pub(super) repeat: Option<String>,
    /// A sentence to the user on why you suggest it.
    pub(super) note: Option<String>,
}

/// A time the model gave as a wall-clock time in the user's zone.
fn wall_clock(s: &str) -> Result<String, String> {
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&Local).format("%Y-%m-%dT%H:%M").to_string());
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .map(|t| t.format("%Y-%m-%dT%H:%M").to_string())
        .map_err(|_| {
            format!("not a time: {s}; give RFC 3339, a local time like 2026-10-05T14:00, or a date")
        })
}

pub(super) async fn draft_event(host: Host, a: DraftArgs) -> Result<String, String> {
    let all_day = NaiveDate::parse_from_str(a.start.trim(), "%Y-%m-%d").is_ok();
    let (start, end) = if all_day {
        (a.start.trim().to_string(), a.end.trim().to_string())
    } else {
        (wall_clock(&a.start)?, wall_clock(&a.end)?)
    };
    let draft = json!({
        "title": a.title,
        "start": start,
        "end": end,
        "all_day": all_day,
        "time_zone": crate::calendar::home_zone().name(),
        "location": a.location,
        "description": a.description,
        "attendees": a.attendees.unwrap_or_default().into_iter()
            .map(|email| json!({ "email": email })).collect::<Vec<_>>(),
        "repeat": a.repeat,
    });
    host.call(
        "calendar_propose",
        json!({ "draft": draft, "accountId": null, "calendarId": null, "note": a.note }),
    )
    .await?;
    Ok(
        "Drafted. It waits in the user's Calendar Space, marked as a draft; nothing is saved \
        to their calendar or sent to anyone until they send it."
            .into(),
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Range {
    /// Where the range starts: an RFC 3339 time like 2026-10-05T09:00:00-03:00, or a
    /// date like 2026-10-05 for that day's start in the user's zone. Defaults to now.
    pub(super) from: Option<String>,
    /// Where it ends, the same way, exclusive. Defaults to a week after `from`.
    pub(super) to: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Search {
    /// The words to look for, as the user would say them: "dentist", "review ada".
    query: String,
    /// Search from here (RFC 3339 or a date). Defaults to a year ago.
    from: Option<String>,
    /// And to here. Defaults to a year from now.
    to: Option<String>,
    /// At most this many, keeping those nearest today. Defaults to 25.
    limit: Option<usize>,
}

/// A time the model gave, as the Host takes it: a bare date is that day's
/// start in the user's zone, which the Host shares, being this machine's.
fn when(s: &str) -> Result<String, String> {
    let s = s.trim();
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let local = Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
            .earliest()
            .ok_or_else(|| format!("{s} has no midnight here"))?;
        return Ok(local.to_rfc3339());
    }
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.to_rfc3339())
        .map_err(|_| {
            format!("not a time: {s}; give RFC 3339, like 2026-10-05T09:00:00-03:00, or a date")
        })
}

fn range(r: &Range) -> Result<(String, String), String> {
    let from = match &r.from {
        Some(f) => when(f)?,
        None => Local::now().to_rfc3339(),
    };
    let to = match &r.to {
        Some(t) => when(t)?,
        None => (DateTime::parse_from_rfc3339(&from).map_err(|e| e.to_string())?
            + chrono::Duration::days(7))
        .to_rfc3339(),
    };
    Ok((from, to))
}

/// A Calendar event as the model reads it.
#[derive(Serialize)]
pub(super) struct Listed {
    pub(super) title: String,
    /// In the user's zone, or a date for an all-day one.
    pub(super) start: String,
    pub(super) end: String,
    pub(super) all_day: bool,
    pub(super) calendar: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) location: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) people: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) meeting_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) notes: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) repeats: bool,
    /// A draft an Agent left, which the user hasn't sent: not in their calendar.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) draft: bool,
    /// Whether the user said no to it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) declined: bool,
}

fn local(s: &str) -> String {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Local).to_rfc3339())
        .unwrap_or_else(|_| s.to_string())
}

/// Each calendar's name, by account and calendar id.
type Names = Vec<(String, String, String)>;

fn listed(events: Vec<CalendarEvent>, names: &Names) -> Vec<Listed> {
    let name = |e: &CalendarEvent| {
        names
            .iter()
            .find(|(a, c, _)| *a == e.account_id && *c == e.calendar_id)
            .map(|(_, _, n)| n.clone())
            .unwrap_or_default()
    };
    events
        .into_iter()
        .map(|e| Listed {
            calendar: name(&e),
            start: if e.all_day {
                e.start.clone()
            } else {
                local(&e.start)
            },
            end: if e.all_day {
                e.end.clone()
            } else {
                local(&e.end)
            },
            all_day: e.all_day,
            people: e
                .attendees
                .iter()
                .map(|a| match (&a.name, &a.response) {
                    (Some(n), Some(r)) => format!("{n} <{}> ({r})", a.email),
                    (Some(n), None) => format!("{n} <{}>", a.email),
                    (None, Some(r)) => format!("{} ({r})", a.email),
                    (None, None) => a.email.clone(),
                })
                .collect(),
            declined: e
                .attendees
                .iter()
                .any(|a| a.is_self && a.response.as_deref() == Some("declined")),
            location: e.location,
            meeting_url: e.meeting_url,
            notes: e.description,
            repeats: e.recurring,
            draft: e.draft_id.is_some(),
            title: e.title,
        })
        .collect()
}

async fn names(host: &Host) -> Result<Names, String> {
    let o = host.call("calendar_overview", json!({})).await?;
    Ok(o["accounts"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|a| {
            let account = a["id"].as_str().unwrap_or_default().to_string();
            a["calendars"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |c| {
                    (
                        account.clone(),
                        c["id"].as_str().unwrap_or_default().to_string(),
                        c["name"].as_str().unwrap_or_default().to_string(),
                    )
                })
        })
        .collect())
}

pub(super) async fn list_events(host: Host, r: Range) -> Result<Vec<Listed>, String> {
    let (from, to) = range(&r)?;
    let events = host
        .call_as("calendar_events", json!({ "from": from, "to": to }))
        .await?;
    Ok(listed(events, &names(&host).await?))
}

async fn search(host: Host, s: Search) -> Result<Vec<Listed>, String> {
    let from = s.from.as_deref().map(when).transpose()?;
    let to = s.to.as_deref().map(when).transpose()?;
    let events = host
        .call_as(
            "calendar_search",
            json!({ "query": s.query, "from": from, "to": to, "limit": s.limit.unwrap_or(25) }),
        )
        .await?;
    Ok(listed(events, &names(&host).await?))
}

/// Free/busy with its times in the user's zone.
#[derive(Serialize)]
struct Spans {
    busy: Vec<[String; 2]>,
    free: Vec<[String; 2]>,
}

async fn free_busy(host: Host, r: Range) -> Result<Spans, String> {
    let (from, to) = range(&r)?;
    let fb: FreeBusy = host
        .call_as("calendar_free_busy", json!({ "from": from, "to": to }))
        .await?;
    let spans = |v: Vec<super::Span>| {
        v.into_iter()
            .map(|s| [local(&s.start), local(&s.end)])
            .collect()
    };
    Ok(Spans {
        busy: spans(fb.busy),
        free: spans(fb.free),
    })
}
