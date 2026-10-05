//! Reading iCalendar (RFC 5545), the format CalDAV servers hand over: just
//! enough of it to turn each `VEVENT` into an [`Item`]. Time zones are taken
//! by their IANA name, which is what every server in use writes; a server's
//! `VTIMEZONE` blocks are skipped rather than interpreted.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

use super::{meeting_link, Attendee, Item, When};

/// One content line: `NAME;PARAM=value:VALUE`, unfolded.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Line {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Undo line folding: a line break followed by a space or tab continues the
/// line before it.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !raw.is_empty() {
            lines.push(raw.to_string());
        }
    }
    lines
}

/// Split one unfolded line into its name, parameters and value. Parameter
/// values may be quoted, and a quoted one may hold `:` or `;`.
pub fn parse_line(line: &str) -> Option<Line> {
    let mut name_end = None;
    let mut in_quotes = false;
    let mut value_start = None;
    let mut params = Vec::new();
    let mut param_start = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' | ':' if !in_quotes => {
                if name_end.is_none() {
                    name_end = Some(i);
                } else if let Some(start) = param_start {
                    params.push(&line[start..i]);
                }
                param_start = Some(i + 1);
                if c == ':' {
                    value_start = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let name = line[..name_end?].trim().to_ascii_uppercase();
    let value = line[value_start?..].to_string();
    let params = params
        .into_iter()
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        })
        .collect();
    Some(Line {
        name,
        params,
        value,
    })
}

/// A TEXT value with its escapes undone.
pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A DATE or DATE-TIME value, in the zone its `TZID` names.
pub fn parse_when(value: &str, tzid: Option<&str>, date_only: bool) -> Option<When> {
    let value = value.trim();
    if date_only || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .ok()
            .map(|date| When::Date { date });
    }
    let (body, utc) = match value.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (value, false),
    };
    let at = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    let tz = if utc {
        Some("UTC".to_string())
    } else {
        tzid.map(str::to_string)
    };
    Some(When::Time { at, tz })
}

/// The values of a DATE or DATE-TIME property that can hold several
/// (`EXDATE`, `RDATE`), comma-separated.
fn parse_whens(line: &Line) -> Vec<When> {
    let date_only = line.param("VALUE") == Some("DATE");
    line.value
        .split(',')
        .filter_map(|v| parse_when(v, line.param("TZID"), date_only))
        .collect()
}

/// A DURATION (`P1D`, `PT1H30M`, `P2W`, `-PT15M`), in seconds.
pub fn parse_duration(value: &str) -> Option<i64> {
    let (sign, rest) = match value.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = 0i64;
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += n * match (unit, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(sign * total)
}

fn attendee(line: &Line) -> Attendee {
    let email = line
        .value
        .trim()
        .trim_start_matches("mailto:")
        .trim_start_matches("MAILTO:")
        .to_string();
    Attendee {
        name: line.param("CN").map(str::to_string),
        email,
        response: line.param("PARTSTAT").map(|s| s.to_ascii_lowercase()),
        is_self: false,
        organizer: false,
    }
}

/// Every `VEVENT` in an iCalendar document, masters and their exceptions
/// alike. A `VEVENT` with no `UID` or no start is skipped.
pub fn parse_events(text: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut depth_in_event: Option<usize> = None;
    let mut stack: Vec<String> = Vec::new();
    let mut current: Vec<Line> = Vec::new();
    for raw in unfold(text) {
        let Some(line) = parse_line(&raw) else {
            continue;
        };
        match line.name.as_str() {
            "BEGIN" => {
                let kind = line.value.trim().to_ascii_uppercase();
                if kind == "VEVENT" && depth_in_event.is_none() {
                    depth_in_event = Some(stack.len());
                    current.clear();
                }
                stack.push(kind);
            }
            "END" => {
                stack.pop();
                if depth_in_event == Some(stack.len()) {
                    depth_in_event = None;
                    items.extend(item_from(&current));
                }
            }
            // Only the event's own properties: not those of an alarm in it.
            _ if depth_in_event.map(|d| d + 1) == Some(stack.len()) => current.push(line),
            _ => {}
        }
    }
    items
}

fn item_from(lines: &[Line]) -> Option<Item> {
    let get = |name: &str| lines.iter().find(|l| l.name == name);
    let text = |name: &str| {
        get(name)
            .map(|l| unescape(&l.value))
            .filter(|s| !s.is_empty())
    };
    let when = |name: &str| {
        get(name)
            .and_then(|l| parse_when(&l.value, l.param("TZID"), l.param("VALUE") == Some("DATE")))
    };

    let uid = text("UID")?;
    let start = when("DTSTART")?;
    let end = when("DTEND").or_else(|| {
        let secs = get("DURATION").and_then(|l| parse_duration(&l.value))?;
        Some(start.plus_seconds(secs))
    });
    let organizer = get("ORGANIZER").map(|l| Attendee {
        organizer: true,
        ..attendee(l)
    });
    let organizer_email = organizer.as_ref().map(|o| o.email.to_ascii_lowercase());
    let attendees = lines
        .iter()
        .filter(|l| l.name == "ATTENDEE")
        .map(|l| {
            let mut a = attendee(l);
            a.organizer = organizer_email.as_deref() == Some(&a.email.to_ascii_lowercase());
            a
        })
        .collect();
    let description = text("DESCRIPTION");
    let location = text("LOCATION");
    let url = text("URL");
    let meeting = text("X-GOOGLE-CONFERENCE")
        .or_else(|| meeting_link([url.as_deref(), location.as_deref(), description.as_deref()]));

    Some(Item {
        uid,
        recurrence_id: when("RECURRENCE-ID"),
        cancelled: get("STATUS").is_some_and(|l| l.value.eq_ignore_ascii_case("CANCELLED")),
        tentative: get("STATUS").is_some_and(|l| l.value.eq_ignore_ascii_case("TENTATIVE")),
        title: text("SUMMARY").unwrap_or_default(),
        description,
        location,
        start,
        end,
        rrule: lines
            .iter()
            .filter(|l| l.name == "RRULE")
            .map(|l| l.value.clone())
            .collect(),
        rdate: lines
            .iter()
            .filter(|l| l.name == "RDATE")
            .flat_map(parse_whens)
            .collect(),
        exdate: lines
            .iter()
            .filter(|l| l.name == "EXDATE")
            .flat_map(parse_whens)
            .collect(),
        attendees,
        organizer,
        meeting_url: meeting,
        link: url,
        transparent: get("TRANSP").is_some_and(|l| l.value.eq_ignore_ascii_case("TRANSPARENT")),
    })
}

/// Midnight, for a DATE read as the start of its day.
pub fn midnight() -> NaiveTime {
    NaiveTime::MIN
}

// ------------------------------------------------------------------ writing

/// One content line folded to RFC 5545's 75 octets, never splitting a
/// character, with CRLF line ends.
pub fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut width = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
    out
}

/// A TEXT value escaped.
pub fn text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// A parameter value, quoted when it holds what would end it.
fn param(s: &str) -> String {
    let s = s.replace('"', "'");
    if s.contains([';', ':', ',']) {
        format!("\"{s}\"")
    } else {
        s
    }
}

/// An instant as iCalendar writes UTC.
pub fn utc(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// A DATE or DATE-TIME property: `DTSTART;VALUE=DATE:20261005`,
/// `DTSTART;TZID=Europe/Paris:20261005T090000` or `DTSTART:20261005T070000Z`.
pub fn when_line(name: &str, when: &When) -> String {
    match when {
        When::Date { date } => format!("{name};VALUE=DATE:{}", date.format("%Y%m%d")),
        When::Time { at, tz: Some(tz) } if tz != "UTC" => {
            format!("{name};TZID={tz}:{}", at.format("%Y%m%dT%H%M%S"))
        }
        When::Time { at, tz: Some(_) } => format!("{name}:{}Z", at.format("%Y%m%dT%H%M%S")),
        When::Time { at, tz: None } => format!("{name}:{}", at.format("%Y%m%dT%H%M%S")),
    }
}

/// A VTIMEZONE for `tz`, so a server that reads TZIDs by their definition
/// rather than by name reads this one right. Its rules are this year's
/// changes of offset, as yearly rules on the same weekday of the month.
pub fn vtimezone(tz: chrono_tz::Tz, year: i32) -> Vec<String> {
    use chrono::{Datelike, Offset};
    use chrono_tz::OffsetName;
    let offset = |t: DateTime<Utc>| tz.offset_from_utc_datetime(&t.naive_utc()).fix();
    let hhmm = |secs: i32| {
        let sign = if secs < 0 { '-' } else { '+' };
        let m = secs.abs() / 60;
        format!("{sign}{:02}{:02}", m / 60, m % 60)
    };
    let first = Utc
        .with_ymd_and_hms(year, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or_default();
    let mut prev = offset(first);
    let mut changes = Vec::new();
    for h in 1..(366 * 24) {
        let t = first + chrono::Duration::hours(h);
        let o = offset(t);
        if o != prev {
            changes.push((t, prev, o));
            prev = o;
        }
    }
    let mut out = vec!["BEGIN:VTIMEZONE".to_string(), format!("TZID:{}", tz.name())];
    if changes.is_empty() {
        let o = prev.local_minus_utc();
        out.extend([
            "BEGIN:STANDARD".into(),
            "DTSTART:19700101T000000".into(),
            format!("TZOFFSETFROM:{}", hhmm(o)),
            format!("TZOFFSETTO:{}", hhmm(o)),
            "END:STANDARD".into(),
        ]);
    }
    for (t, from, to) in changes {
        let local = t.naive_utc() + chrono::Duration::seconds(i64::from(from.local_minus_utc()));
        let kind = if to.local_minus_utc() > from.local_minus_utc() {
            "DAYLIGHT"
        } else {
            "STANDARD"
        };
        let day = local.day();
        let days_in_month = NaiveDate::from_ymd_opt(local.year(), local.month(), 1)
            .and_then(|d| d.checked_add_months(chrono::Months::new(1)))
            .map(|d| (d - chrono::Duration::days(1)).day())
            .unwrap_or(31);
        let nth = if day + 7 > days_in_month {
            -1
        } else {
            ((day - 1) / 7 + 1) as i32
        };
        out.push(format!("BEGIN:{kind}"));
        out.push(format!("DTSTART:{}", local.format("%Y%m%dT%H%M%S")));
        out.push(format!(
            "RRULE:FREQ=YEARLY;BYMONTH={};BYDAY={nth}{}",
            local.month(),
            super::write::byday(local.weekday())
        ));
        out.push(format!("TZOFFSETFROM:{}", hhmm(from.local_minus_utc())));
        out.push(format!("TZOFFSETTO:{}", hhmm(to.local_minus_utc())));
        if let Some(name) = tz.offset_from_utc_datetime(&t.naive_utc()).abbreviation() {
            out.push(format!("TZNAME:{name}"));
        }
        out.push(format!("END:{kind}"));
    }
    out.push("END:VTIMEZONE".into());
    out
}

/// The properties a draft decides, which an edit replaces and leaves the
/// rest alone. `RRULE` is decided only for the Calendar event as a whole.
const DECIDED: [&str; 12] = [
    "SUMMARY",
    "DTSTART",
    "DTEND",
    "DURATION",
    "LOCATION",
    "DESCRIPTION",
    "ATTENDEE",
    "ORGANIZER",
    "TRANSP",
    "SEQUENCE",
    "DTSTAMP",
    "LAST-MODIFIED",
];

fn address(line: &Line) -> String {
    line.value
        .trim()
        .trim_start_matches("mailto:")
        .trim_start_matches("MAILTO:")
        .to_ascii_lowercase()
}

/// A draft as VEVENT properties. `old` are the event's ATTENDEE lines so far,
/// whose answers carry over for the people still invited; `organizer` is the
/// user's own address, which a server schedules invitations from.
fn draft_props(
    draft: &super::write::Draft,
    rule: Option<&str>,
    organizer: Option<&str>,
    old: &[Line],
    sequence: u32,
) -> Result<Vec<String>, String> {
    let (start, end) = draft.whens()?;
    let now = utc(Utc::now());
    let mut out = vec![
        format!("SUMMARY:{}", text(draft.title.trim())),
        when_line("DTSTART", &start),
        when_line("DTEND", &end),
        format!("SEQUENCE:{sequence}"),
        format!("DTSTAMP:{now}"),
        format!("LAST-MODIFIED:{now}"),
    ];
    if let Some(l) = draft.location.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push(format!("LOCATION:{}", text(l.trim())));
    }
    if let Some(d) = draft
        .description
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        out.push(format!("DESCRIPTION:{}", text(d.trim())));
    }
    if !draft.busy {
        out.push("TRANSP:TRANSPARENT".into());
    }
    if let Some(rule) = rule {
        out.push(format!("RRULE:{rule}"));
    }
    if !draft.attendees.is_empty() {
        let kept = |email: &str| {
            old.iter()
                .find(|l| address(l) == email.to_ascii_lowercase())
        };
        if let Some(me) = organizer {
            out.push(format!("ORGANIZER:mailto:{me}"));
            out.push(match kept(me) {
                Some(l) => raw_line(l),
                None => format!("ATTENDEE;ROLE=CHAIR;PARTSTAT=ACCEPTED:mailto:{me}"),
            });
        }
        for g in &draft.attendees {
            if organizer.is_some_and(|me| me.eq_ignore_ascii_case(&g.email)) {
                continue;
            }
            out.push(match kept(&g.email) {
                Some(l) => raw_line(l),
                None => {
                    let cn = g
                        .name
                        .as_deref()
                        .filter(|n| !n.trim().is_empty())
                        .map(|n| format!(";CN={}", param(n.trim())))
                        .unwrap_or_default();
                    format!(
                        "ATTENDEE{cn};ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{}",
                        g.email.trim()
                    )
                }
            });
        }
    }
    Ok(out)
}

/// A content line put back together.
fn raw_line(l: &Line) -> String {
    let mut s = l.name.clone();
    for (k, v) in &l.params {
        s.push(';');
        s.push_str(k);
        s.push('=');
        s.push_str(&param(v));
    }
    s.push(':');
    s.push_str(&l.value);
    s
}

/// A whole new iCalendar object holding one Calendar event.
pub fn new_object(
    uid: &str,
    draft: &super::write::Draft,
    rule: Option<&str>,
    organizer: Option<&str>,
) -> Result<String, String> {
    let mut obj = Object {
        lines: vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//Orchestrate//Calendar//EN".into(),
            "END:VCALENDAR".into(),
        ],
    };
    let mut event = vec!["BEGIN:VEVENT".to_string(), format!("UID:{uid}")];
    event.extend(draft_props(draft, rule, organizer, &[], 0)?);
    event.push("END:VEVENT".into());
    obj.insert(event);
    obj.zones_for(draft)?;
    Ok(obj.render())
}

/// An iCalendar object being edited, as unfolded lines.
pub struct Object {
    lines: Vec<String>,
}

/// Where one VEVENT sits in an [`Object`]: its BEGIN and END lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    begin: usize,
    end: usize,
}

impl Object {
    pub fn parse(raw: &str) -> Self {
        Self { lines: unfold(raw) }
    }

    pub fn render(&self) -> String {
        self.lines.iter().map(|l| fold(l)).collect()
    }

    /// Every top-level VEVENT, with its RECURRENCE-ID line if it has one.
    fn events(&self) -> Vec<(Span, Option<Line>)> {
        let mut out = Vec::new();
        let mut depth = 0;
        let mut open: Option<(usize, Option<Line>)> = None;
        for (i, raw) in self.lines.iter().enumerate() {
            let Some(line) = parse_line(raw) else {
                continue;
            };
            match line.name.as_str() {
                "BEGIN" => {
                    depth += 1;
                    if depth == 2 && line.value.eq_ignore_ascii_case("VEVENT") {
                        open = Some((i, None));
                    }
                }
                "END" => {
                    if depth == 2 && line.value.eq_ignore_ascii_case("VEVENT") {
                        if let Some((begin, rid)) = open.take() {
                            out.push((Span { begin, end: i }, rid));
                        }
                    }
                    depth -= 1;
                }
                "RECURRENCE-ID" if depth == 2 => {
                    if let Some((_, rid)) = &mut open {
                        *rid = Some(line);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The master VEVENT: the one with no RECURRENCE-ID.
    pub fn master(&self) -> Option<Span> {
        self.events()
            .into_iter()
            .find(|(_, rid)| rid.is_none())
            .map(|(s, _)| s)
    }

    /// The exception for the occurrence keyed `key` (see `expand::key_of`).
    pub fn exception(&self, key: &str, master: &Item, home: chrono_tz::Tz) -> Option<Span> {
        self.events().into_iter().find_map(|(span, rid)| {
            let rid = rid?;
            let when = parse_when(
                &rid.value,
                rid.param("TZID"),
                rid.param("VALUE") == Some("DATE"),
            )?;
            (super::expand::key_of(&when, master, home) == key).then_some(span)
        })
    }

    /// The properties of the VEVENT at `span`, its own and not its alarms'.
    fn props(&self, span: Span) -> Vec<(usize, Line)> {
        let mut depth = 0;
        let mut out = Vec::new();
        for i in span.begin + 1..span.end {
            let Some(line) = parse_line(&self.lines[i]) else {
                continue;
            };
            match line.name.as_str() {
                "BEGIN" => depth += 1,
                "END" => depth -= 1,
                _ if depth == 0 => out.push((i, line)),
                _ => {}
            }
        }
        out
    }

    /// Rewrite the VEVENT at `span` from a draft, keeping everything the
    /// draft doesn't decide. A repeat rule replaces the old one when given;
    /// `exception` strips the repeat altogether, for one occurrence.
    pub fn apply(
        &mut self,
        span: Span,
        draft: &super::write::Draft,
        rule: Option<&str>,
        organizer: Option<&str>,
        exception: bool,
    ) -> Result<(), String> {
        let props = self.props(span);
        let sequence = props
            .iter()
            .find(|(_, l)| l.name == "SEQUENCE")
            .and_then(|(_, l)| l.value.trim().parse::<u32>().ok())
            .map_or(0, |s| s + 1);
        let old: Vec<Line> = props
            .iter()
            .filter(|(_, l)| l.name == "ATTENDEE")
            .map(|(_, l)| l.clone())
            .collect();
        // The organizer it had stays its organizer.
        let organizer = props
            .iter()
            .find(|(_, l)| l.name == "ORGANIZER")
            .map(|(_, l)| address(l))
            .or_else(|| organizer.map(str::to_string));
        let drop: Vec<usize> = props
            .iter()
            .filter(|(_, l)| {
                DECIDED.contains(&l.name.as_str())
                    || (exception && matches!(l.name.as_str(), "RRULE" | "RDATE" | "EXDATE"))
                    || (!exception && l.name == "RRULE")
            })
            .map(|(i, _)| *i)
            .collect();
        let fresh = draft_props(
            draft,
            rule.filter(|_| !exception),
            organizer.as_deref(),
            &old,
            sequence,
        )?;
        let mut lines = Vec::with_capacity(self.lines.len() + fresh.len());
        for (i, l) in self.lines.iter().enumerate() {
            if drop.contains(&i) {
                continue;
            }
            lines.push(l.clone());
            if i == span.begin {
                lines.extend(fresh.iter().cloned());
            }
        }
        self.lines = lines;
        self.zones_for(draft)
    }

    /// A copy of the master as the exception for one occurrence, which the
    /// caller then changes. Returns where it is.
    pub fn split_off(&mut self, key: &str, master: &Item, home: chrono_tz::Tz) -> Option<Span> {
        let span = self.master()?;
        let rid = occurrence_line("RECURRENCE-ID", key, master, home)?;
        let mut copy: Vec<String> = Vec::new();
        let props: Vec<usize> = self
            .props(span)
            .into_iter()
            .filter(|(_, l)| matches!(l.name.as_str(), "RRULE" | "RDATE" | "EXDATE"))
            .map(|(i, _)| i)
            .collect();
        for i in span.begin..=span.end {
            if props.contains(&i) {
                continue;
            }
            copy.push(self.lines[i].clone());
            if i == span.begin {
                copy.push(rid.clone());
            }
        }
        self.insert(copy);
        self.exception(key, master, home)
    }

    /// Leave one occurrence out of the master's repeat, and drop its
    /// exception if it had one.
    pub fn exclude(&mut self, key: &str, master: &Item, home: chrono_tz::Tz) -> Result<(), String> {
        if let Some(span) = self.exception(key, master, home) {
            self.lines.drain(span.begin..=span.end);
        }
        let span = self
            .master()
            .ok_or("the repeating Calendar event has no master")?;
        let line = occurrence_line("EXDATE", key, master, home).ok_or("no such occurrence")?;
        self.lines.insert(span.begin + 1, line);
        Ok(())
    }

    /// Set the user's answer on the VEVENT at `span`.
    pub fn answer(&mut self, span: Span, me: &str, partstat: &str) -> Result<(), String> {
        let (i, mut line) = self
            .props(span)
            .into_iter()
            .find(|(_, l)| l.name == "ATTENDEE" && address(l) == me.to_ascii_lowercase())
            .ok_or("you aren't among the people invited to it")?;
        line.params.retain(|(k, _)| k != "PARTSTAT" && k != "RSVP");
        line.params.push(("PARTSTAT".into(), partstat.into()));
        self.lines[i] = raw_line(&line);
        // Not a change to the Calendar event itself: no new SEQUENCE, but a
        // new stamp, which is how its organizer tells answers apart.
        if let Some((j, _)) = self
            .props(span)
            .into_iter()
            .find(|(_, l)| l.name == "DTSTAMP")
        {
            self.lines[j] = format!("DTSTAMP:{}", utc(Utc::now()));
        }
        Ok(())
    }

    /// Every VEVENT's span: the master's and each exception's.
    pub fn all(&self) -> Vec<Span> {
        self.events().into_iter().map(|(s, _)| s).collect()
    }

    fn insert(&mut self, lines: Vec<String>) {
        let at = self
            .lines
            .iter()
            .rposition(|l| l.eq_ignore_ascii_case("END:VCALENDAR"))
            .unwrap_or(self.lines.len());
        self.lines.splice(at..at, lines);
    }

    /// A VTIMEZONE for the draft's zone, if its times name one this object
    /// doesn't define yet.
    fn zones_for(&mut self, draft: &super::write::Draft) -> Result<(), String> {
        if draft.all_day {
            return Ok(());
        }
        let tz = draft.zone()?;
        if tz.name() == "UTC" {
            return Ok(());
        }
        let defined = self
            .lines
            .iter()
            .any(|l| l.eq_ignore_ascii_case(&format!("TZID:{}", tz.name())));
        if defined {
            return Ok(());
        }
        let year = draft
            .start
            .get(..4)
            .and_then(|y| y.parse().ok())
            .unwrap_or(2026);
        let at = self
            .lines
            .iter()
            .position(|l| l.eq_ignore_ascii_case("BEGIN:VEVENT"))
            .unwrap_or(self.lines.len().saturating_sub(1));
        self.lines.splice(at..at, vtimezone(tz, year));
        Ok(())
    }
}

/// A RECURRENCE-ID or EXDATE naming the occurrence keyed `key`, written the
/// way the master's DTSTART is.
fn occurrence_line(name: &str, key: &str, master: &Item, home: chrono_tz::Tz) -> Option<String> {
    match &master.start {
        When::Date { .. } => {
            let date = NaiveDate::parse_from_str(key.get(..8)?, "%Y%m%d").ok()?;
            Some(when_line(name, &When::Date { date }))
        }
        When::Time { tz, .. } => {
            let at = NaiveDateTime::parse_from_str(key, "%Y%m%dT%H%M%SZ").ok()?;
            let instant = Utc.from_utc_datetime(&at);
            match tz.as_deref() {
                Some("UTC") => Some(format!("{name}:{key}")),
                _ => {
                    let zone = master.start.zone(home);
                    let local = instant.with_timezone(&zone).naive_local();
                    Some(when_line(
                        name,
                        &When::Time {
                            at: local,
                            tz: tz.clone(),
                        },
                    ))
                }
            }
        }
    }
}
