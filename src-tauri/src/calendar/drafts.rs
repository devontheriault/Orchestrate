//! Calendar events an Agent suggests. An Agent may draft one over MCP, since
//! a draft stays on this machine, but never send it: it waits in the Space,
//! dashed, until the user sends it, changes it, or throws it away (ADR 0017).

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::write::Draft;
use super::{store, CalendarEvent, Calendars, When};
use crate::domain::new_id;

/// A Calendar event an Agent drafted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposed {
    pub id: String,
    pub draft: Draft,
    /// Where it would go; the user's main calendar if the Agent didn't say.
    pub account_id: Option<String>,
    pub calendar_id: Option<String>,
    /// Why the Agent suggested it, for the user to read.
    #[serde(default)]
    pub note: Option<String>,
    pub made_at: String,
}

impl Calendars {
    pub fn drafts(&self) -> Vec<Proposed> {
        self.drafts.lock().unwrap().clone()
    }

    fn save_drafts(&self) -> Result<(), String> {
        let drafts = self.drafts.lock().unwrap().clone();
        store::save_drafts(&self.dir, &drafts)
    }

    /// Keep a Calendar event an Agent drafted, for the user to send or not.
    pub fn propose(
        &self,
        draft: Draft,
        account_id: Option<String>,
        calendar_id: Option<String>,
        note: Option<String>,
    ) -> Result<Proposed, String> {
        draft.check()?;
        let (account_id, calendar_id) = match (account_id, calendar_id) {
            (Some(a), Some(c)) => (Some(a), Some(c)),
            _ => self.main_calendar().unzip(),
        };
        let proposed = Proposed {
            id: new_id(),
            draft,
            account_id,
            calendar_id,
            note,
            made_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        self.drafts.lock().unwrap().push(proposed.clone());
        self.save_drafts()?;
        self.changed();
        Ok(proposed)
    }

    pub fn discard_draft(&self, id: &str) -> Result<(), String> {
        self.drafts.lock().unwrap().retain(|d| d.id != id);
        self.save_drafts()?;
        self.changed();
        Ok(())
    }

    /// The calendar a new Calendar event goes on unless the user picks
    /// another: the first account's primary one it can write, or its first.
    pub fn main_calendar(&self) -> Option<(String, String)> {
        let accounts = self.accounts.lock().unwrap().clone();
        let caches = self.caches.lock().unwrap();
        accounts.iter().find_map(|a| {
            let cals = &caches.get(&a.id)?.calendars;
            cals.iter()
                .find(|c| c.primary && c.writable)
                .or_else(|| cals.iter().find(|c| c.writable))
                .map(|c| (a.id.clone(), c.id.clone()))
        })
    }

    /// The drafts in `from`..`to`, as Calendar events marked as drafts.
    pub(super) fn draft_events(
        &self,
        from: chrono::DateTime<chrono::FixedOffset>,
        to: chrono::DateTime<chrono::FixedOffset>,
    ) -> Vec<CalendarEvent> {
        self.drafts()
            .into_iter()
            .filter_map(|p| {
                let (start, end) = p.draft.whens().ok()?;
                let tz = p.draft.zone().ok()?;
                let (s, e, all_day) = match (&start, &end) {
                    (When::Date { date: s }, When::Date { date: e }) => {
                        let first = from.date_naive();
                        let last = (to - chrono::Duration::nanoseconds(1)).date_naive();
                        if *s > last || *e <= first {
                            return None;
                        }
                        (s.to_string(), e.to_string(), true)
                    }
                    _ => {
                        let (s, e) = (start.instant(tz), end.instant(tz));
                        if s >= to || e <= from {
                            return None;
                        }
                        let w = |t: chrono::DateTime<Utc>| {
                            t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                        };
                        (w(s), w(e), false)
                    }
                };
                let d = &p.draft;
                Some(CalendarEvent {
                    id: format!("draft|{}", p.id),
                    account_id: p.account_id.clone().unwrap_or_default(),
                    calendar_id: p.calendar_id.clone().unwrap_or_default(),
                    title: d.title.clone(),
                    start: s,
                    end: e,
                    all_day,
                    time_zone: Some(d.time_zone.clone()),
                    location: d.location.clone(),
                    description: p.note.clone().or_else(|| d.description.clone()),
                    attendees: d
                        .attendees
                        .iter()
                        .map(|g| super::Attendee {
                            name: g.name.clone(),
                            email: g.email.clone(),
                            response: None,
                            is_self: false,
                            organizer: false,
                        })
                        .collect(),
                    organizer: None,
                    meeting_url: None,
                    link: None,
                    recurring: d.repeat.is_some(),
                    tentative: false,
                    busy: false,
                    uid: String::new(),
                    occurrence: String::new(),
                    repeat_rule: d.repeat.clone(),
                    can_edit: true,
                    draft_id: Some(p.id),
                })
            })
            .collect()
    }
}
