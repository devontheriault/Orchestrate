//! Turning what a provider stores into the occurrences a range shows: the
//! repeating Calendar events expanded with `rrule`, their deleted occurrences
//! dropped and their changed ones swapped in.
//!
//! A repeat is expanded in the zone it was made in, so a 09:00 meeting in
//! Toronto stays at 09:00 Toronto time on both sides of a DST change, and so
//! moves an hour for anyone reading it in a zone that didn't change. An
//! all-day one is expanded as dates, with no zone at all.

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rrule::{RRule, RRuleSet, Unvalidated};

use super::{ical, local_instant, Attendee, CalendarEvent, Item, When};

/// The most occurrences one repeating Calendar event yields for one range.
/// A month view of an every-minute repeat would otherwise be 40,000 boxes.
const MAX_PER_RANGE: u16 = 2_000;

/// When an occurrence starts or ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Moment {
    Day(NaiveDate),
    At(DateTime<Utc>),
}

impl Moment {
    fn to_wire(self) -> String {
        match self {
            Moment::Day(d) => d.format("%Y-%m-%d").to_string(),
            Moment::At(t) => t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }
}

/// One occurrence of a Calendar event.
#[derive(Debug, Clone)]
pub struct Occurrence<'a> {
    pub item: &'a Item,
    pub start: Moment,
    pub end: Moment,
    pub recurring: bool,
    /// Which occurrence of its master this is; empty for a one-off.
    pub key: String,
    /// How its master repeats, where the master is known.
    pub rule: Option<&'a str>,
}

/// The range asked for, both as instants and as the days it covers in the
/// asker's own offset.
struct Range {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    first_day: NaiveDate,
    last_day: NaiveDate,
}

impl Range {
    fn new(from: DateTime<FixedOffset>, to: DateTime<FixedOffset>) -> Self {
        Self {
            from: from.to_utc(),
            to: to.to_utc(),
            first_day: from.date_naive(),
            last_day: (to - chrono::Duration::nanoseconds(1)).date_naive(),
        }
    }

    fn overlaps(&self, start: Moment, end: Moment) -> bool {
        match (start, end) {
            (Moment::Day(s), Moment::Day(e)) => s <= self.last_day && e > self.first_day,
            (Moment::At(s), Moment::At(e)) => {
                s < self.to && (e > self.from || (e == s && s >= self.from))
            }
            // Mixed kinds only come from malformed data; place by the start.
            (Moment::Day(s), _) => s <= self.last_day && s >= self.first_day,
            (Moment::At(s), _) => s < self.to && s >= self.from,
        }
    }
}

/// A single Calendar event's own start and end.
fn span(item: &Item, home: Tz) -> (Moment, Moment) {
    match &item.start {
        When::Date { date } => {
            let end = match &item.end {
                Some(When::Date { date: e }) if e > date => *e,
                _ => *date + chrono::Duration::days(1),
            };
            (Moment::Day(*date), Moment::Day(end))
        }
        start => {
            let s = start.instant(home);
            let e = item
                .end
                .as_ref()
                .map(|e| e.instant(home))
                .filter(|e| *e >= s)
                .unwrap_or(s);
            (Moment::At(s), Moment::At(e))
        }
    }
}

/// Which occurrence `when` names, keyed the way the master's occurrences
/// are: by date for an all-day master, by instant otherwise.
pub(crate) fn key_of(when: &When, master: &Item, home: Tz) -> String {
    match (&master.start, when) {
        (When::Date { .. }, When::Date { date }) => date.format("%Y%m%d").to_string(),
        (When::Date { .. }, When::Time { at, .. }) => at.date().format("%Y%m%d").to_string(),
        (When::Time { at: start, .. }, When::Date { date }) => {
            let tz = master.start.zone(home);
            instant_key(local_instant(tz, date.and_time(start.time())))
        }
        (When::Time { .. }, t) => instant_key(t.instant(home)),
    }
}

fn instant_key(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Every occurrence of `items` that overlaps `from`..`to`. Items are one
/// calendar's, in any order; exceptions find their master by `uid`.
pub fn expand<'a>(
    items: &[&'a Item],
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
    home: Tz,
) -> Vec<Occurrence<'a>> {
    let range = Range::new(from, to);
    let mut groups: HashMap<&str, (Option<&'a Item>, Vec<&'a Item>)> = HashMap::new();
    for item in items {
        let group = groups.entry(item.uid.as_str()).or_default();
        if item.recurrence_id.is_some() {
            group.1.push(item);
        } else if group.0.is_none() {
            group.0 = Some(item);
        }
    }

    let mut out = Vec::new();
    for (master, exceptions) in groups.into_values() {
        let Some(master) = master.filter(|m| m.repeats()) else {
            // A one-off, or exceptions whose master this calendar doesn't
            // have (an invitation to one occurrence): each stands alone.
            for item in master.into_iter().chain(exceptions) {
                if item.cancelled {
                    continue;
                }
                let (start, end) = span(item, home);
                if range.overlaps(start, end) {
                    let key = item
                        .recurrence_id
                        .as_ref()
                        .map(|r| key_of(r, item, home))
                        .unwrap_or_default();
                    out.push(Occurrence {
                        item,
                        start,
                        end,
                        recurring: item.recurrence_id.is_some(),
                        key,
                        rule: None,
                    });
                }
            }
            continue;
        };
        if master.cancelled {
            continue;
        }
        let replaced: HashMap<String, &Item> = exceptions
            .iter()
            .filter_map(|e| Some((key_of(e.recurrence_id.as_ref()?, master, home), *e)))
            .collect();
        for occ in repeat(master, &range, home) {
            if !replaced.contains_key(&occ.key) {
                out.push(occ);
            }
        }
        for (key, item) in replaced {
            if item.cancelled {
                continue;
            }
            let (start, end) = span(item, home);
            if range.overlaps(start, end) {
                out.push(Occurrence {
                    item,
                    start,
                    end,
                    recurring: true,
                    key,
                    rule: master.rrule.first().map(String::as_str),
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| a.item.title.cmp(&b.item.title))
    });
    out
}

/// The occurrences of a repeating master in `range`, before its exceptions
/// are applied.
fn repeat<'a>(master: &'a Item, range: &Range, home: Tz) -> Vec<Occurrence<'a>> {
    let (first_start, first_end) = span(master, home);
    let all_day = matches!(first_start, Moment::Day(_));
    // Expanded in UTC as dates for an all-day master, which has no zone; in
    // its own zone otherwise, so its wall-clock time holds across DST.
    let tz = if all_day {
        Tz::UTC
    } else {
        master.start.zone(home)
    };
    let rtz = rrule::Tz::Tz(tz);
    let dt_start = match first_start {
        Moment::Day(d) => Utc.from_utc_datetime(&d.and_time(ical::midnight())),
        Moment::At(t) => t,
    }
    .with_timezone(&rtz);
    let length = match (first_start, first_end) {
        (Moment::Day(s), Moment::Day(e)) => (e - s).num_days().max(1),
        (Moment::At(s), Moment::At(e)) => (e - s).num_seconds(),
        _ => 0,
    };

    let at = |when: &When| -> DateTime<rrule::Tz> {
        match (when, &master.start) {
            (When::Date { date }, When::Date { .. }) => Utc
                .from_utc_datetime(&date.and_time(ical::midnight()))
                .with_timezone(&rtz),
            (When::Date { date }, When::Time { at, .. }) => {
                local_instant(tz, date.and_time(at.time())).with_timezone(&rtz)
            }
            (When::Time { at, .. }, When::Date { .. }) => Utc
                .from_utc_datetime(&at.date().and_time(ical::midnight()))
                .with_timezone(&rtz),
            (t, _) => t.instant(home).with_timezone(&rtz),
        }
    };

    let mut set = RRuleSet::new(dt_start);
    for rule in &master.rrule {
        let rule = normalize_until(rule, master, tz);
        match rule
            .parse::<RRule<Unvalidated>>()
            .and_then(|r| r.validate(dt_start))
        {
            Ok(r) => set = set.rrule(r),
            Err(e) => eprintln!("calendar: skipping a repeat rule we can't read ({rule}): {e}"),
        }
    }
    for d in &master.rdate {
        set = set.rdate(at(d));
    }
    for d in &master.exdate {
        set = set.exdate(at(d));
    }
    if set.get_rrule().is_empty() && set.get_rdate().is_empty() {
        // Nothing readable to repeat by: just the first.
        set = set.rdate(dt_start);
    }

    let (after, before) = if all_day {
        let day = |d: NaiveDate| Utc.from_utc_datetime(&d.and_time(ical::midnight()));
        (
            day(range.first_day - chrono::Duration::days(length)),
            day(range.last_day),
        )
    } else {
        (range.from - chrono::Duration::seconds(length), range.to)
    };
    let starts = set
        .after(after.with_timezone(&rtz))
        .before(before.with_timezone(&rtz))
        .all(MAX_PER_RANGE)
        .dates;

    starts
        .into_iter()
        .filter_map(|s| {
            let (start, end, key) = if all_day {
                let d = s.date_naive();
                (
                    Moment::Day(d),
                    Moment::Day(d + chrono::Duration::days(length)),
                    d.format("%Y%m%d").to_string(),
                )
            } else {
                let s = s.to_utc();
                (
                    Moment::At(s),
                    Moment::At(s + chrono::Duration::seconds(length)),
                    instant_key(s),
                )
            };
            range.overlaps(start, end).then_some(Occurrence {
                item: master,
                start,
                end,
                recurring: true,
                key,
                rule: master.rrule.first().map(String::as_str),
            })
        })
        .collect()
}

/// The rule with its `UNTIL` in UTC, which is how `rrule` insists on it.
/// Servers also write it as a date, or as a time in the event's own zone.
fn normalize_until(rule: &str, master: &Item, tz: Tz) -> String {
    let rule = rule.trim().trim_start_matches("RRULE:");
    rule.split(';')
        .map(|part| {
            let Some(value) = part
                .strip_prefix("UNTIL=")
                .or_else(|| part.strip_prefix("until="))
            else {
                return part.to_string();
            };
            if value.ends_with('Z') {
                return part.to_string();
            }
            let until = match (ical::parse_when(value, None, false), &master.start) {
                // An all-day repeat is expanded at midnight UTC, so its last
                // day is that day's midnight.
                (Some(When::Date { date }), When::Date { .. }) => {
                    Utc.from_utc_datetime(&date.and_time(ical::midnight()))
                }
                (Some(When::Time { at, .. }), When::Date { .. }) => {
                    Utc.from_utc_datetime(&at.date().and_time(ical::midnight()))
                }
                // A timed repeat until a date runs through the end of it.
                (Some(When::Date { date }), _) => {
                    let next = date.succ_opt().unwrap_or(date);
                    local_instant(tz, next.and_time(ical::midnight()))
                        - chrono::Duration::seconds(1)
                }
                (Some(When::Time { at, .. }), _) => local_instant(tz, at),
                (None, _) => return part.to_string(),
            };
            format!("UNTIL={}", instant_key(until))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// An occurrence as a window or an Agent sees it. `me` is the user's own
/// address on the account, to find them among the attendees.
pub fn to_event(
    occ: &Occurrence,
    account_id: &str,
    calendar_id: &str,
    me: Option<&str>,
    home: Tz,
) -> CalendarEvent {
    let item = occ.item;
    let attendees: Vec<Attendee> = item
        .attendees
        .iter()
        .map(|a| Attendee {
            is_self: a.is_self || me.is_some_and(|m| m.eq_ignore_ascii_case(&a.email)),
            ..a.clone()
        })
        .collect();
    let declined = attendees
        .iter()
        .any(|a| a.is_self && a.response.as_deref() == Some("declined"));
    let time_zone = match &item.start {
        When::Time { tz: Some(name), .. } => {
            let tz = item.start.zone(home);
            (tz != Tz::UTC && tz != home).then(|| name.clone())
        }
        _ => None,
    };
    CalendarEvent {
        id: format!("{account_id}|{calendar_id}|{}|{}", item.uid, occ.key),
        account_id: account_id.to_string(),
        calendar_id: calendar_id.to_string(),
        title: item.title.clone(),
        start: occ.start.to_wire(),
        end: occ.end.to_wire(),
        all_day: matches!(occ.start, Moment::Day(_)),
        time_zone,
        location: item.location.clone(),
        description: item.description.clone(),
        attendees,
        organizer: item.organizer.clone(),
        meeting_url: item.meeting_url.clone(),
        link: item.link.clone(),
        recurring: occ.recurring,
        tentative: item.tentative,
        busy: !item.transparent && !declined,
        uid: item.uid.clone(),
        occurrence: occ.key.clone(),
        repeat_rule: occ.rule.map(str::to_string),
        can_edit: false,
        draft_id: None,
    }
}
