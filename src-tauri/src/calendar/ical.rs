//! Reading iCalendar (RFC 5545), the format CalDAV servers hand over: just
//! enough of it to turn each `VEVENT` into an [`Item`]. Time zones are taken
//! by their IANA name, which is what every server in use writes; a server's
//! `VTIMEZONE` blocks are skipped rather than interpreted.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use super::{meeting_link, Attendee, Item, When};

/// One content line: `NAME;PARAM=value:VALUE`, unfolded.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Line {
    fn param(&self, name: &str) -> Option<&str> {
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
