//! Writing the user's calendars: making a Calendar event, changing one,
//! deleting one, and answering an invitation. Each goes straight to the
//! provider, which stays the source of truth (ADR 0017); the Host then syncs
//! the account, so its cache, and every window, catch up from the provider
//! rather than from a guess.
//!
//! Inviting people is the provider's own doing: Google emails them when asked
//! to (`sendUpdates`), Microsoft always does, and a CalDAV server does when it
//! schedules on the user's behalf (RFC 6638). Only the user does any of it,
//! from the Space; an Agent can only leave a draft (see `drafts`).

use chrono::{Datelike, NaiveDate, NaiveDateTime, Weekday};
use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{local_instant, Item, When};

/// A Calendar event as the user writes it: a new one, or the new version of
/// one. Times are wall-clock times in `time_zone`, as the user picked them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Draft {
    pub title: String,
    /// `YYYY-MM-DD` for an all-day one; otherwise `YYYY-MM-DDTHH:MM`, a
    /// wall-clock time in `time_zone`.
    pub start: String,
    /// The same, exclusive: the day after the last, or the time it ends.
    pub end: String,
    #[serde(default)]
    pub all_day: bool,
    /// The IANA zone the times are in, like America/Halifax.
    pub time_zone: String,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// The people to invite, besides the user.
    #[serde(default)]
    pub attendees: Vec<Guest>,
    /// How it repeats, as an iCalendar RRULE (`FREQ=WEEKLY;BYDAY=MO`), or
    /// none. An `UNTIL` may be a bare date: the last day it happens.
    #[serde(default)]
    pub repeat: Option<String>,
    /// Shown as busy (the default) rather than free.
    #[serde(default = "yes")]
    pub busy: bool,
}

fn yes() -> bool {
    true
}

/// Someone to invite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Guest {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Which occurrences of a repeating Calendar event a change is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The one the user picked.
    This,
    /// Every one: the Calendar event as a whole.
    All,
}

/// An answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Accepted,
    Tentative,
    Declined,
}

impl Answer {
    /// As iCalendar's PARTSTAT says it.
    pub fn partstat(self) -> &'static str {
        match self {
            Answer::Accepted => "ACCEPTED",
            Answer::Tentative => "TENTATIVE",
            Answer::Declined => "DECLINED",
        }
    }
}

/// A Calendar event to change, as a window has it: the `uid` and
/// `occurrence` of a [`super::CalendarEvent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub account_id: String,
    pub calendar_id: String,
    pub uid: String,
    /// Which occurrence; empty for one that doesn't repeat.
    #[serde(default)]
    pub occurrence: String,
}

/// What the cache knows about a [`Target`]: where its master and the
/// occurrence's own exception live at the provider, by resource key (a
/// Google or Microsoft event id, or a CalDAV object's URL).
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// The Calendar event's uid: for Microsoft, its series' id.
    pub uid: String,
    pub master: Option<(String, Item)>,
    pub exception: Option<(String, Item)>,
    /// Which occurrence, as `expand` keys them; empty for a one-off.
    pub occurrence: String,
    /// The CalDAV object as the server sent it, and its etag.
    pub raw: Option<String>,
    pub etag: Option<String>,
}

impl Found {
    /// The resource key the change goes to: the occurrence's own exception
    /// when there is one and the change is for it alone, else the master.
    pub fn key(&self, scope: Scope) -> Option<&str> {
        match (scope, &self.exception, &self.master) {
            (Scope::This, Some((k, _)), _) => Some(k),
            (_, _, Some((k, _))) => Some(k),
            (_, Some((k, _)), None) => Some(k),
            _ => None,
        }
    }

    /// Whether this is one occurrence of a Calendar event that repeats.
    pub fn repeats(&self) -> bool {
        !self.occurrence.is_empty()
            && (self.master.as_ref().is_some_and(|(_, m)| m.repeats()) || self.exception.is_some())
    }

    /// The item a change for `scope` starts from.
    pub fn item(&self, scope: Scope) -> Option<&Item> {
        match scope {
            Scope::This => self
                .exception
                .as_ref()
                .or(self.master.as_ref())
                .map(|(_, i)| i),
            Scope::All => self
                .master
                .as_ref()
                .or(self.exception.as_ref())
                .map(|(_, i)| i),
        }
    }
}

/// Why a draft can't be written, said to the user.
fn bad(what: &str) -> String {
    format!("the Calendar event can't be saved: {what}")
}

impl Draft {
    pub fn zone(&self) -> Result<Tz, String> {
        super::zone(&self.time_zone)
            .ok_or_else(|| bad(&format!("unknown time zone {}", self.time_zone)))
    }

    fn naive(s: &str) -> Result<NaiveDateTime, String> {
        let s = s.trim();
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
            .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
            .map_err(|_| bad(&format!("{s} isn't a time like 2026-10-05T09:30")))
    }

    fn date(s: &str) -> Result<NaiveDate, String> {
        let s = s.trim();
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| bad(&format!("{s} isn't a date like 2026-10-05")))
    }

    /// The start and end as [`When`]s.
    pub fn whens(&self) -> Result<(When, When), String> {
        if self.all_day {
            let (s, e) = (Self::date(&self.start)?, Self::date(&self.end)?);
            return Ok((When::Date { date: s }, When::Date { date: e }));
        }
        let tz = self.zone()?;
        let (s, e) = (Self::naive(&self.start)?, Self::naive(&self.end)?);
        let tz = Some(tz.name().to_string());
        Ok((
            When::Time {
                at: s,
                tz: tz.clone(),
            },
            When::Time { at: e, tz },
        ))
    }

    /// Everything a provider would refuse, said before asking it.
    pub fn check(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err(bad("it needs a title"));
        }
        let (s, e) = self.whens()?;
        let tz = self.zone()?;
        if e.instant(tz) <= s.instant(tz) {
            return Err(bad("it ends before it starts"));
        }
        for g in &self.attendees {
            let ok = g.email.split_once('@').is_some_and(|(a, d)| {
                !a.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
            }) && !g.email.contains(char::is_whitespace);
            if !ok {
                return Err(bad(&format!("{} isn't an email address", g.email)));
            }
        }
        if let Some(rule) = &self.repeat {
            if !rule.to_ascii_uppercase().contains("FREQ=") {
                return Err(bad(&format!("{rule} isn't a repeat rule")));
            }
        }
        Ok(())
    }

    /// The repeat rule as a provider takes it: `UNTIL` in UTC for a timed
    /// Calendar event, through the end of its last day, and a date for an
    /// all-day one.
    pub fn rrule(&self) -> Result<Option<String>, String> {
        let Some(rule) = &self.repeat else {
            return Ok(None);
        };
        let tz = self.zone()?;
        let rule = rule.trim().trim_start_matches("RRULE:");
        let parts: Result<Vec<String>, String> = rule
            .split(';')
            .filter(|p| !p.is_empty())
            .map(|part| {
                let Some(value) = part
                    .strip_prefix("UNTIL=")
                    .or_else(|| part.strip_prefix("until="))
                else {
                    return Ok(part.to_ascii_uppercase());
                };
                let date = NaiveDate::parse_from_str(&value[..value.len().min(8)], "%Y%m%d")
                    .or_else(|_| NaiveDate::parse_from_str(value, "%Y-%m-%d"))
                    .map_err(|_| bad(&format!("{value} isn't a date to repeat until")))?;
                if self.all_day {
                    return Ok(format!("UNTIL={}", date.format("%Y%m%d")));
                }
                let next = date.succ_opt().unwrap_or(date);
                let last = local_instant(tz, next.and_time(super::ical::midnight()))
                    - chrono::Duration::seconds(1);
                Ok(format!("UNTIL={}", last.format("%Y%m%dT%H%M%SZ")))
            })
            .collect();
        Ok(Some(parts?.join(";")))
    }
}

/// The RRULE BYDAY code for a weekday.
pub fn byday(d: Weekday) -> &'static str {
    match d {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

/// A draft's start date, for rules that repeat on its weekday or day.
pub fn start_date(d: &Draft) -> Option<NaiveDate> {
    d.start
        .get(..10)
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

/// The day of the month a draft starts on, for a monthly rule.
pub fn start_day(d: &Draft) -> Option<u32> {
    start_date(d).map(|d| d.day())
}
