use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset};
use chrono_tz::Tz;
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

fn t(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

/// The starts of every occurrence in `ics` overlapping `from`..`to`.
fn starts(ics: &str, from: &str, to: &str, home: Tz) -> Vec<String> {
    let items = ical::parse_events(ics);
    let refs: Vec<&Item> = items.iter().collect();
    expand::expand(&refs, t(from), t(to), home)
        .iter()
        .map(|o| expand::to_event(o, "a", "c", None, home).start)
        .collect()
}

fn vcal(body: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\n{body}END:VCALENDAR\r\n")
}

// ------------------------------------------------------------ iCalendar

#[test]
fn reads_folded_lines_quoted_params_and_escapes() {
    let ics = vcal(
        "BEGIN:VEVENT\r\n\
         UID:one@test\r\n\
         DTSTART;TZID=America/Toronto:20261005T090000\r\n\
         DURATION:PT1H30M\r\n\
         SUMMARY:Plan\\, then build\\; ship\r\n\
         DESCRIPTION:First line\\nsecond line that is long enough to be fol\r\n ded here\r\n\
         LOCATION:Room 4\r\n\
         ORGANIZER;CN=\"Ada, L\":mailto:ada@example.com\r\n\
         ATTENDEE;CN=Bob;PARTSTAT=ACCEPTED:mailto:bob@example.com\r\n\
         ATTENDEE;CN=\"Ada, L\";PARTSTAT=NEEDS-ACTION:mailto:ada@example.com\r\n\
         BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Not the event's\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\n\
         END:VEVENT\r\n",
    );
    let items = ical::parse_events(&ics);
    assert_eq!(items.len(), 1);
    let e = &items[0];
    assert_eq!(e.title, "Plan, then build; ship");
    assert_eq!(
        e.description.as_deref(),
        Some("First line\nsecond line that is long enough to be folded here")
    );
    assert_eq!(e.location.as_deref(), Some("Room 4"));
    assert_eq!(
        e.end,
        Some(When::Time {
            at: chrono::NaiveDate::from_ymd_opt(2026, 10, 5)
                .unwrap()
                .and_hms_opt(10, 30, 0)
                .unwrap(),
            tz: Some("America/Toronto".into()),
        })
    );
    assert_eq!(
        e.organizer.as_ref().unwrap().name.as_deref(),
        Some("Ada, L")
    );
    assert_eq!(e.attendees.len(), 2);
    assert_eq!(e.attendees[0].response.as_deref(), Some("accepted"));
    assert!(e.attendees[1].organizer);
}

#[test]
fn durations() {
    assert_eq!(ical::parse_duration("PT1H30M"), Some(5400));
    assert_eq!(ical::parse_duration("P1D"), Some(86_400));
    assert_eq!(ical::parse_duration("P2W"), Some(14 * 86_400));
    assert_eq!(ical::parse_duration("-PT15M"), Some(-900));
    assert_eq!(ical::parse_duration("P1DT2H"), Some(93_600));
    assert_eq!(ical::parse_duration("nonsense"), None);
}

#[test]
fn finds_a_meeting_link_in_the_description() {
    let found = meeting_link([
        None,
        Some("Room 4"),
        Some("Join: <https://meet.google.com/abc-defg-hij>, or dial in"),
    ]);
    assert_eq!(
        found.as_deref(),
        Some("https://meet.google.com/abc-defg-hij")
    );
    assert_eq!(meeting_link([Some("https://example.com/agenda")]), None);
}

#[test]
fn reads_windows_and_prefixed_zone_names() {
    assert_eq!(zone("Eastern Standard Time"), Some(Tz::America__New_York));
    assert_eq!(
        zone("/mozilla.org/20050126_1/Europe/Paris"),
        Some(Tz::Europe__Paris)
    );
    assert_eq!(zone("Not/AZone"), None);
}

// ------------------------------------------------------- repeats and DST

#[test]
fn a_daily_repeat_keeps_its_wall_clock_time_across_the_fall_back() {
    // Toronto leaves DST on 1 November 2026: 09:00 is 13:00Z before, 14:00Z after.
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\n\
         DTSTART;TZID=America/Toronto:20261029T090000\r\n\
         DTEND;TZID=America/Toronto:20261029T091500\r\n\
         RRULE:FREQ=DAILY;COUNT=5\r\nEND:VEVENT\r\n",
    );
    assert_eq!(
        starts(
            &ics,
            "2026-10-26T00:00:00-04:00",
            "2026-11-09T00:00:00-05:00",
            Tz::UTC
        ),
        [
            "2026-10-29T13:00:00Z",
            "2026-10-30T13:00:00Z",
            "2026-10-31T13:00:00Z",
            "2026-11-01T14:00:00Z",
            "2026-11-02T14:00:00Z",
        ]
    );
}

#[test]
fn a_weekly_repeat_across_the_spring_forward_until_a_date() {
    // London enters BST on 29 March 2026. The UNTIL is a bare date, which
    // runs through the end of that day.
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:review\r\nSUMMARY:Review\r\n\
         DTSTART;TZID=Europe/London:20260316T100000\r\n\
         DTEND;TZID=Europe/London:20260316T110000\r\n\
         RRULE:FREQ=WEEKLY;BYDAY=MO;UNTIL=20260413\r\nEND:VEVENT\r\n",
    );
    assert_eq!(
        starts(
            &ics,
            "2026-03-01T00:00:00Z",
            "2026-05-01T00:00:00Z",
            Tz::UTC
        ),
        [
            "2026-03-16T10:00:00Z",
            "2026-03-23T10:00:00Z",
            "2026-03-30T09:00:00Z",
            "2026-04-06T09:00:00Z",
            "2026-04-13T09:00:00Z",
        ]
    );
}

#[test]
fn a_time_the_clocks_skip_lands_an_hour_later() {
    // 02:30 on 8 March 2026 doesn't happen in Toronto; read as 03:30 EDT.
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:gap\r\nSUMMARY:Gap\r\n\
         DTSTART;TZID=America/Toronto:20260308T023000\r\n\
         DTEND;TZID=America/Toronto:20260308T033000\r\nEND:VEVENT\r\n",
    );
    assert_eq!(
        starts(
            &ics,
            "2026-03-08T00:00:00Z",
            "2026-03-09T00:00:00Z",
            Tz::UTC
        ),
        ["2026-03-08T07:30:00Z"]
    );
}

#[test]
fn exceptions_move_cancel_and_exclude_occurrences() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:sync\r\nSUMMARY:Sync\r\n\
         DTSTART;TZID=Europe/Berlin:20261005T150000\r\n\
         DTEND;TZID=Europe/Berlin:20261005T153000\r\n\
         RRULE:FREQ=WEEKLY;BYDAY=MO\r\n\
         EXDATE;TZID=Europe/Berlin:20261012T150000\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:sync\r\nSUMMARY:Sync (moved)\r\n\
         RECURRENCE-ID;TZID=Europe/Berlin:20261019T150000\r\n\
         DTSTART;TZID=Europe/Berlin:20261020T090000\r\n\
         DTEND;TZID=Europe/Berlin:20261020T093000\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:sync\r\nSUMMARY:Sync\r\nSTATUS:CANCELLED\r\n\
         RECURRENCE-ID;TZID=Europe/Berlin:20261026T150000\r\n\
         DTSTART;TZID=Europe/Berlin:20261026T150000\r\nEND:VEVENT\r\n",
    );
    let items = ical::parse_events(&ics);
    let refs: Vec<&Item> = items.iter().collect();
    let got: Vec<(String, String)> = expand::expand(
        &refs,
        t("2026-10-01T00:00:00+02:00"),
        t("2026-11-07T00:00:00+01:00"),
        Tz::UTC,
    )
    .iter()
    .map(|o| {
        let e = expand::to_event(o, "a", "c", None, Tz::UTC);
        (e.start, e.title)
    })
    .collect();
    assert_eq!(
        got,
        [
            ("2026-10-05T13:00:00Z".to_string(), "Sync".to_string()),
            // 12 October is excluded.
            ("2026-10-20T07:00:00Z".into(), "Sync (moved)".into()),
            // 26 October is cancelled. Berlin is back on CET by 2 November.
            ("2026-11-02T14:00:00Z".into(), "Sync".into()),
        ]
    );
}

#[test]
fn a_moved_occurrence_shows_in_its_new_range_only() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:X\r\n\
         DTSTART:20261005T150000Z\r\nDTEND:20261005T160000Z\r\n\
         RRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:X moved\r\n\
         RECURRENCE-ID:20261012T150000Z\r\n\
         DTSTART:20261020T150000Z\r\nDTEND:20261020T160000Z\r\nEND:VEVENT\r\n",
    );
    // The week it was moved out of has nothing.
    assert!(starts(
        &ics,
        "2026-10-11T00:00:00Z",
        "2026-10-18T00:00:00Z",
        Tz::UTC
    )
    .is_empty());
    // The week it was moved into has it, and its regular one.
    assert_eq!(
        starts(
            &ics,
            "2026-10-18T00:00:00Z",
            "2026-10-25T00:00:00Z",
            Tz::UTC
        ),
        ["2026-10-19T15:00:00Z", "2026-10-20T15:00:00Z"]
    );
}

#[test]
fn an_all_day_repeat_is_the_same_date_in_every_zone() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:bday\r\nSUMMARY:Birthday\r\n\
         DTSTART;VALUE=DATE:19900315\r\nDTEND;VALUE=DATE:19900316\r\n\
         RRULE:FREQ=YEARLY\r\nTRANSP:TRANSPARENT\r\nEND:VEVENT\r\n",
    );
    for (from, to) in [
        ("2026-03-15T00:00:00+13:00", "2026-03-16T00:00:00+13:00"),
        ("2026-03-15T00:00:00-10:00", "2026-03-16T00:00:00-10:00"),
    ] {
        assert_eq!(
            starts(&ics, from, to, Tz::UTC),
            ["2026-03-15"],
            "asked {from}"
        );
    }
    assert!(starts(
        &ics,
        "2026-03-16T00:00:00-10:00",
        "2026-03-17T00:00:00-10:00",
        Tz::UTC
    )
    .is_empty());
}

#[test]
fn an_all_day_repeat_until_a_date_includes_that_date() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:trip\r\nSUMMARY:Trip\r\n\
         DTSTART;VALUE=DATE:20261001\r\n\
         RRULE:FREQ=DAILY;UNTIL=20261003\r\nEND:VEVENT\r\n",
    );
    assert_eq!(
        starts(
            &ics,
            "2026-09-28T00:00:00Z",
            "2026-10-10T00:00:00Z",
            Tz::UTC
        ),
        ["2026-10-01", "2026-10-02", "2026-10-03"]
    );
}

#[test]
fn a_long_event_that_began_before_the_range_overlaps_it() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:conf\r\nSUMMARY:Conference\r\n\
         DTSTART;VALUE=DATE:20261003\r\nDTEND;VALUE=DATE:20261007\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:night\r\nSUMMARY:Overnight\r\n\
         DTSTART:20261004T220000Z\r\nDTEND:20261005T060000Z\r\nEND:VEVENT\r\n",
    );
    assert_eq!(
        starts(
            &ics,
            "2026-10-05T00:00:00Z",
            "2026-10-06T00:00:00Z",
            Tz::UTC
        ),
        ["2026-10-03", "2026-10-04T22:00:00Z"]
    );
}

#[test]
fn a_floating_time_is_read_in_the_hosts_zone() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:f\r\nSUMMARY:Float\r\n\
         DTSTART:20261005T090000\r\nDTEND:20261005T100000\r\nEND:VEVENT\r\n",
    );
    assert_eq!(
        starts(
            &ics,
            "2026-10-05T00:00:00Z",
            "2026-10-06T00:00:00Z",
            Tz::America__Halifax
        ),
        ["2026-10-05T12:00:00Z"]
    );
}

#[test]
fn a_repeat_in_another_zone_names_it() {
    let ics = vcal(
        "BEGIN:VEVENT\r\nUID:tokyo\r\nSUMMARY:Tokyo call\r\n\
         DTSTART;TZID=Asia/Tokyo:20261005T090000\r\nDTEND;TZID=Asia/Tokyo:20261005T100000\r\n\
         RRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\n",
    );
    let items = ical::parse_events(&ics);
    let refs: Vec<&Item> = items.iter().collect();
    let occ = expand::expand(
        &refs,
        t("2026-10-12T00:00:00Z"),
        t("2026-10-13T00:00:00Z"),
        Tz::America__Halifax,
    );
    let e = expand::to_event(&occ[0], "a", "c", None, Tz::America__Halifax);
    assert_eq!(e.start, "2026-10-12T00:00:00Z");
    assert_eq!(e.time_zone.as_deref(), Some("Asia/Tokyo"));
    assert!(e.recurring);
}

// ---------------------------------------------------------------- Google

fn google_event() -> Value {
    json!({
        "id": "abc",
        "status": "confirmed",
        "htmlLink": "https://www.google.com/calendar/event?eid=abc",
        "summary": "Design review",
        "description": "Agenda:<br>1. Rail<br><a href=\"https://docs.example.com/x\">the doc</a>",
        "location": "Room 4",
        "start": {"dateTime": "2026-10-05T10:00:00-03:00", "timeZone": "America/Halifax"},
        "end": {"dateTime": "2026-10-05T11:00:00-03:00", "timeZone": "America/Halifax"},
        "recurrence": [
            "RRULE:FREQ=WEEKLY;BYDAY=MO",
            "EXDATE;TZID=America/Halifax:20261012T100000"
        ],
        "attendees": [
            {"email": "me@example.com", "self": true, "responseStatus": "accepted"},
            {"email": "ada@example.com", "displayName": "Ada", "responseStatus": "needsAction", "organizer": true}
        ],
        "organizer": {"email": "ada@example.com", "displayName": "Ada"},
        "conferenceData": {"entryPoints": [
            {"entryPointType": "phone", "uri": "tel:+1-555"},
            {"entryPointType": "video", "uri": "https://meet.google.com/abc-defg-hij"}
        ]}
    })
}

#[test]
fn reads_a_google_event() {
    let item = google::item(&google_event()).unwrap();
    assert_eq!(item.uid, "abc");
    assert_eq!(item.rrule, ["FREQ=WEEKLY;BYDAY=MO"]);
    assert_eq!(item.exdate.len(), 1);
    assert_eq!(
        item.meeting_url.as_deref(),
        Some("https://meet.google.com/abc-defg-hij")
    );
    assert_eq!(
        item.description.as_deref(),
        Some("Agenda:\n1. Rail\nthe doc (https://docs.example.com/x)")
    );
    assert_eq!(item.attendees[1].response.as_deref(), Some("needs-action"));
    assert!(item.attendees[0].is_self);

    let refs = [&item];
    let got: Vec<String> = expand::expand(
        &refs,
        t("2026-10-01T00:00:00Z"),
        t("2026-10-20T00:00:00Z"),
        Tz::UTC,
    )
    .iter()
    .map(|o| expand::to_event(o, "a", "c", None, Tz::UTC).start)
    .collect();
    assert_eq!(got, ["2026-10-05T13:00:00Z", "2026-10-19T13:00:00Z"]);
}

#[test]
fn skips_working_location_and_strips_html() {
    assert!(
        google::item(&json!({"id": "w", "eventType": "workingLocation",
        "start": {"date": "2026-10-05"}, "end": {"date": "2026-10-06"}}))
        .is_none()
    );
    assert_eq!(google::plain("a &amp; b<p>c</p>d"), "a & b\nc\nd");
}

fn client() -> oauth::Client {
    oauth::Client {
        id: "cid".into(),
        secret: Some("secret".into()),
    }
}

fn endpoints(server: &MockServer) -> google::Endpoints {
    google::Endpoints {
        auth: format!("{}/auth", server.uri()),
        token: format!("{}/token", server.uri()),
        api: format!("{}/calendar/v3", server.uri()),
    }
}

fn ms_endpoints(server: &MockServer) -> microsoft::Endpoints {
    microsoft::Endpoints {
        auth: format!("{}/ms/authorize", server.uri()),
        token: format!("{}/ms/token", server.uri()),
        api: format!("{}/graph", server.uri()),
    }
}

fn fresh_tokens() -> oauth::Tokens {
    oauth::Tokens {
        access_token: "live".into(),
        refresh_token: "refresh".into(),
        expires_at: chrono::Utc::now().timestamp() + 3600,
    }
}

async fn mock_calendar_list(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
            {"id": "me@example.com", "summary": "me@example.com", "summaryOverride": "Me",
             "backgroundColor": "#9fe1e7", "primary": true, "selected": true}
        ]})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn google_syncs_in_full_then_only_what_changed() {
    let server = MockServer::start().await;
    mock_calendar_list(&server).await;
    let events = "/calendar/v3/calendars/me%40example.com/events";
    Mock::given(method("GET"))
        .and(path(events))
        .and(query_param_is_missing("syncToken"))
        .and(header("authorization", "Bearer live"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [google_event(), {
                "id": "lunch", "status": "confirmed", "summary": "Lunch",
                "start": {"dateTime": "2026-10-06T12:00:00-03:00"},
                "end": {"dateTime": "2026-10-06T13:00:00-03:00"}
            }],
            "nextSyncToken": "t1"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(events))
        .and(query_param("syncToken", "t1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                {"id": "lunch", "status": "cancelled"},
                {"id": "abc_20261019T130000Z", "status": "cancelled", "recurringEventId": "abc",
                 "originalStartTime": {"dateTime": "2026-10-19T10:00:00-03:00", "timeZone": "America/Halifax"}}
            ],
            "nextSyncToken": "t2"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ep = endpoints(&server);
    let http = reqwest::Client::new();
    let client = client();
    let mut tokens = fresh_tokens();
    let mut cache = AccountCache::default();
    let mut g = google::Google::new(&http, &ep, client.clone(), &mut tokens);
    assert!(g.sync(&mut cache).await.unwrap());
    let cal = &cache.calendars[0];
    assert_eq!(
        (cal.name.as_str(), cal.sync_token.as_deref()),
        ("Me", Some("t1"))
    );
    assert_eq!(cal.resources.len(), 2);

    assert!(g.sync(&mut cache).await.unwrap());
    let cal = &cache.calendars[0];
    assert_eq!(cal.sync_token.as_deref(), Some("t2"));
    // Lunch is gone; the cancelled occurrence stays, to hide it.
    assert!(!cal.resources.contains_key("lunch"));
    let items: Vec<&Item> = cal.resources.values().flat_map(|r| &r.items).collect();
    let got: Vec<String> = expand::expand(
        &items,
        t("2026-10-01T00:00:00Z"),
        t("2026-10-27T00:00:00Z"),
        Tz::UTC,
    )
    .iter()
    .map(|o| expand::to_event(o, "a", "c", None, Tz::UTC).start)
    .collect();
    assert_eq!(got, ["2026-10-05T13:00:00Z", "2026-10-26T13:00:00Z"]);
}

#[tokio::test]
async fn google_starts_over_when_the_sync_token_is_gone() {
    let server = MockServer::start().await;
    mock_calendar_list(&server).await;
    let events = "/calendar/v3/calendars/me%40example.com/events";
    Mock::given(method("GET"))
        .and(path(events))
        .and(query_param("syncToken", "old"))
        .respond_with(
            ResponseTemplate::new(410)
                .set_body_json(json!({"error": {"message": "Sync token is no longer valid"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(events))
        .and(query_param_is_missing("syncToken"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"items": [google_event()], "nextSyncToken": "new"})),
        )
        .mount(&server)
        .await;

    let ep = endpoints(&server);
    let http = reqwest::Client::new();
    let client = client();
    let mut tokens = fresh_tokens();
    let mut cache = AccountCache {
        calendars: vec![CachedCalendar {
            id: "me@example.com".into(),
            sync_token: Some("old".into()),
            resources: [("stale".to_string(), Resource::default())].into(),
            ..Default::default()
        }],
    };
    google::Google::new(&http, &ep, client.clone(), &mut tokens)
        .sync(&mut cache)
        .await
        .unwrap();
    let cal = &cache.calendars[0];
    assert_eq!(cal.sync_token.as_deref(), Some("new"));
    assert_eq!(cal.resources.keys().collect::<Vec<_>>(), ["abc"]);
}

#[tokio::test]
async fn google_refreshes_an_access_token_about_to_run_out() {
    let server = MockServer::start().await;
    mock_calendar_list(&server).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token": "renewed", "expires_in": 3599})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/me%40example.com/events"))
        .and(header("authorization", "Bearer renewed"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"items": [], "nextSyncToken": "t"})),
        )
        .mount(&server)
        .await;

    let ep = endpoints(&server);
    let http = reqwest::Client::new();
    let client = client();
    let mut tokens = oauth::Tokens {
        expires_at: chrono::Utc::now().timestamp() + 30,
        ..fresh_tokens()
    };
    let mut cache = AccountCache::default();
    google::Google::new(&http, &ep, client.clone(), &mut tokens)
        .sync(&mut cache)
        .await
        .unwrap();
    assert_eq!(tokens.access_token, "renewed");
    assert_eq!(
        tokens.refresh_token, "refresh",
        "kept, since Google sent no new one"
    );
    assert!(tokens.expires_at > chrono::Utc::now().timestamp() + 3000);
    let form = String::from_utf8(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .find(|r| r.url.path() == "/token")
            .unwrap()
            .body
            .clone(),
    )
    .unwrap();
    assert!(form.contains("grant_type=refresh_token") && form.contains("refresh_token=refresh"));
}

#[tokio::test]
async fn google_refreshes_when_told_the_token_was_rejected_and_asks_to_reconnect_when_revoked() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": {"message": "Invalid Credentials"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."})))
        .expect(1)
        .mount(&server)
        .await;
    let ep = endpoints(&server);
    let http = reqwest::Client::new();
    let client = client();
    let mut tokens = fresh_tokens();
    let mut cache = AccountCache::default();
    let err = google::Google::new(&http, &ep, client.clone(), &mut tokens)
        .sync(&mut cache)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Auth(_)), "{err:?}");
}

#[tokio::test]
async fn signing_in_to_google_uses_pkce_and_the_loopback_redirect() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "first", "expires_in": 3599, "refresh_token": "long-lived"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList/primary"))
        .and(header("authorization", "Bearer first"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "me@example.com"})))
        .mount(&server)
        .await;
    let ep = endpoints(&server);
    let client = client();
    let pending = oauth::SignIn::start(&ep.oauth(), &client).await.unwrap();
    let url = reqwest::Url::parse(&pending.url).unwrap();
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["access_type"], "offline");
    assert_eq!(q["scope"], google::SCOPE);
    let redirect = q["redirect_uri"].clone();
    assert!(redirect.starts_with("http://127.0.0.1:"));

    let http = reqwest::Client::new();
    let (ep2, client2, http2) = (ep.clone(), client.clone(), http.clone());
    let finishing = tokio::spawn(async move {
        let mut tokens = pending.finish(&http2, &client2).await?;
        let name = google::Google::new(&http2, &ep2, client2.clone(), &mut tokens)
            .address()
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((name, tokens))
    });
    // The browser: a stray request, one from another sign-in, then Google's.
    let _ = http.get(format!("{redirect}/favicon.ico")).send().await;
    let wrong = http
        .get(format!("{redirect}/?code=nope&state=other"))
        .send()
        .await
        .unwrap();
    assert!(wrong.text().await.unwrap().contains("older sign-in"));
    let page = http
        .get(format!("{redirect}/?code=the-code&state={}", q["state"]))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("connected"));

    let (name, tokens) = finishing.await.unwrap().unwrap();
    assert_eq!(name, "me@example.com");
    assert_eq!(tokens.refresh_token, "long-lived");
    let form = String::from_utf8(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .find(|r| r.url.path() == "/token")
            .unwrap()
            .body
            .clone(),
    )
    .unwrap();
    let sent: std::collections::HashMap<_, _> = url::form_urlencoded::parse(form.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(sent["code"], "the-code");
    assert_eq!(sent["redirect_uri"], redirect);
    assert_eq!(
        crate::oauth::challenge(&sent["code_verifier"]),
        q["code_challenge"]
    );
}

// --------------------------------------------------------------- the Host

fn calendars(
    dir: &std::path::Path,
    server: &MockServer,
) -> (Arc<Calendars>, Arc<Mutex<Vec<String>>>) {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let h = heard.clone();
    let cals = Calendars::with_endpoints(
        dir.to_path_buf(),
        Arc::new(move |name: &str, _| h.lock().unwrap().push(name.to_string())),
        Endpoints {
            google: endpoints(server),
            microsoft: ms_endpoints(server),
        },
    );
    (cals, heard)
}

#[tokio::test]
async fn the_host_syncs_an_account_keeps_its_secrets_private_and_answers_from_the_cache() {
    let server = MockServer::start().await;
    mock_calendar_list(&server).await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/me%40example.com/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [google_event(), {
            "id": "focus", "status": "confirmed", "summary": "Focus", "transparency": "transparent",
            "start": {"dateTime": "2026-10-05T14:00:00Z"}, "end": {"dateTime": "2026-10-05T15:00:00Z"}
        }, {
            "id": "late", "status": "confirmed", "summary": "Late",
            "start": {"dateTime": "2026-10-05T13:30:00Z"}, "end": {"dateTime": "2026-10-05T14:30:00Z"}
        }], "nextSyncToken": "t"})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (cals, heard) = calendars(dir.path(), &server);
    assert!(!cals.overview().google_client);
    cals.set_google_client("cid", "secret").unwrap();
    assert!(cals.overview().google_client);

    let info = cals
        .add(Account {
            id: "acct".into(),
            name: "me@example.com".into(),
            source: Source::Google(fresh_tokens()),
        })
        .await
        .unwrap();
    assert!(
        matches!(info.status, Status::Ok { .. }),
        "{:?}",
        info.status
    );
    assert_eq!(info.calendars[0].name, "Me");
    assert!(heard
        .lock()
        .unwrap()
        .iter()
        .any(|n| n == "calendar-changed"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["accounts.json", "google-client.json", "cache/acct.json"] {
            let mode = std::fs::metadata(dir.path().join(f))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{f}");
        }
    }
    assert!(!std::fs::read_to_string(dir.path().join("accounts.json"))
        .unwrap()
        .is_empty());

    let events = cals
        .events("2026-10-05T00:00:00-03:00", "2026-10-06T00:00:00-03:00")
        .unwrap();
    let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Design review", "Late", "Focus"]);
    assert!(events[0].attendees[0].is_self);

    let found = cals.search("ada review", None, None, None).unwrap();
    assert!(!found.is_empty() && found.iter().all(|e| e.title == "Design review"));

    let fb = cals
        .free_busy("2026-10-05T12:00:00Z", "2026-10-05T16:00:00Z")
        .unwrap();
    // The review (13:00–14:00) and Late (13:30–14:30) merge; Focus is free.
    assert_eq!(
        fb.busy,
        [Span {
            start: "2026-10-05T13:00:00Z".into(),
            end: "2026-10-05T14:30:00Z".into()
        }]
    );
    assert_eq!(fb.free.len(), 2);

    // A new Host on the same directory answers from the cache before syncing.
    let (again, _) = calendars(dir.path(), &server);
    assert_eq!(
        again
            .events("2026-10-05T00:00:00-03:00", "2026-10-06T00:00:00-03:00")
            .unwrap()
            .len(),
        3
    );

    cals.remove("acct").unwrap();
    assert!(cals.overview().accounts.is_empty());
    assert!(!dir.path().join("cache/acct.json").exists());
}

#[test]
fn reads_the_client_file_google_downloads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("google-client.json"),
        r#"{"installed":{"client_id":"x.apps.googleusercontent.com","project_id":"p","client_secret":"s","redirect_uris":["http://localhost"]}}"#,
    )
    .unwrap();
    let c = store::load_google_client(dir.path()).unwrap();
    assert_eq!(c.client_id, "x.apps.googleusercontent.com");
}

// ---------------------------------------------------------------- CalDAV

/// The real server's tests take turns: each counts the calendars it sees.
static REAL_SERVER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Against a real CalDAV server, when one is given: `ORCHESTRATE_TEST_CALDAV`
/// as `url user password`, e.g. a throwaway Radicale.
#[tokio::test]
async fn caldav_against_a_real_server() {
    let _turn = REAL_SERVER.lock().await;
    let Ok(given) = std::env::var("ORCHESTRATE_TEST_CALDAV") else {
        eprintln!("skipped: ORCHESTRATE_TEST_CALDAV is not set");
        return;
    };
    let mut parts = given.split_whitespace();
    let (url, user, pass) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    let http = reqwest::Client::new();
    let base = format!("{}/{user}/", url.trim_end_matches('/'));
    let cal = format!("{base}orchestrate-test-{}/", crate::domain::new_id());
    // Make a calendar, and put two objects in it.
    let mk = http
        .request(reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(), &cal)
        .basic_auth(user, Some(pass))
        .body(r#"<?xml version="1.0"?><c:mkcalendar xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:set><d:prop><d:displayname>Test</d:displayname></d:prop></d:set></c:mkcalendar>"#)
        .send()
        .await
        .unwrap();
    assert!(mk.status().is_success(), "MKCALENDAR: {}", mk.status());
    let put = |name: &str, body: String| {
        let req = http
            .put(format!("{cal}{name}.ics"))
            .basic_auth(user, Some(pass))
            .header("content-type", "text/calendar")
            .body(body);
        async move { assert!(req.send().await.unwrap().status().is_success()) }
    };
    put("standup", vcal("BEGIN:VEVENT\r\nUID:standup\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:Standup\r\nDTSTART;TZID=America/Toronto:20261029T090000\r\nDTEND;TZID=America/Toronto:20261029T091500\r\nRRULE:FREQ=DAILY;COUNT=5\r\nEND:VEVENT\r\n")).await;
    put("lunch", vcal("BEGIN:VEVENT\r\nUID:lunch\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:Lunch\r\nDTSTART:20261030T160000Z\r\nDTEND:20261030T170000Z\r\nEND:VEVENT\r\n")).await;

    let mut login = caldav::Login {
        url: url.to_string(),
        username: user.into(),
        password: pass.into(),
        home: None,
        address: None,
        schedules: false,
    };
    let mut cache = AccountCache::default();
    let mut dav = caldav::CalDav::new(&http, &mut login);
    assert!(dav.sync(&mut cache).await.unwrap());
    let ours = |cache: &AccountCache| {
        cache
            .calendars
            .iter()
            .find(|c| c.id == cal)
            .cloned()
            .unwrap()
    };
    assert_eq!(ours(&cache).resources.len(), 2);
    // Nothing moved: the ctag says so, and nothing is fetched.
    assert!(!dav.sync(&mut cache).await.unwrap());

    // Change one, remove the other.
    put("standup", vcal("BEGIN:VEVENT\r\nUID:standup\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:Standup (new)\r\nDTSTART;TZID=America/Toronto:20261029T090000\r\nDTEND;TZID=America/Toronto:20261029T091500\r\nRRULE:FREQ=DAILY;COUNT=5\r\nEND:VEVENT\r\n")).await;
    http.delete(format!("{cal}lunch.ics"))
        .basic_auth(user, Some(pass))
        .send()
        .await
        .unwrap();
    assert!(dav.sync(&mut cache).await.unwrap());
    let c = ours(&cache);
    assert_eq!(c.resources.len(), 1);
    let items: Vec<&Item> = c.resources.values().flat_map(|r| &r.items).collect();
    let got: Vec<(String, String)> = expand::expand(
        &items,
        t("2026-10-31T00:00:00Z"),
        t("2026-11-02T00:00:00Z"),
        Tz::UTC,
    )
    .iter()
    .map(|o| {
        let e = expand::to_event(o, "a", "c", None, Tz::UTC);
        (e.start, e.title)
    })
    .collect();
    assert_eq!(
        got,
        [
            (
                "2026-10-31T13:00:00Z".to_string(),
                "Standup (new)".to_string()
            ),
            ("2026-11-01T14:00:00Z".into(), "Standup (new)".into()),
        ]
    );

    // A wrong password asks to reconnect.
    let mut bad = caldav::Login {
        password: "wrong".into(),
        home: None,
        ..login.clone()
    };
    let err = caldav::CalDav::new(&http, &mut bad)
        .discover()
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Auth(_)), "{err:?}");

    http.delete(&cal)
        .basic_auth(user, Some(pass))
        .send()
        .await
        .unwrap();
}

// ---------------------------------------------------------------- writing

fn draft(start: &str, end: &str) -> write::Draft {
    write::Draft {
        title: "Plan, then build; ship".into(),
        start: start.into(),
        end: end.into(),
        all_day: start.len() == 10,
        time_zone: "America/Halifax".into(),
        location: Some("Room 4".into()),
        description: Some("First\nsecond".into()),
        attendees: vec![write::Guest {
            email: "ada@example.com".into(),
            name: Some("Ada, L".into()),
        }],
        repeat: None,
        busy: true,
    }
}

fn starts_of(ics: &str, from: &str, to: &str) -> Vec<(String, String)> {
    let items = ical::parse_events(ics);
    let refs: Vec<&Item> = items.iter().collect();
    expand::expand(&refs, t(from), t(to), Tz::UTC)
        .iter()
        .map(|o| {
            let e = expand::to_event(o, "a", "c", None, Tz::UTC);
            (e.start, e.title)
        })
        .collect()
}

#[test]
fn a_draft_checks_what_a_provider_would_refuse() {
    assert!(draft("2026-10-05T09:00", "2026-10-05T10:00")
        .check()
        .is_ok());
    assert!(draft("2026-10-05T10:00", "2026-10-05T09:00")
        .check()
        .is_err());
    let mut d = draft("2026-10-05T09:00", "2026-10-05T10:00");
    d.attendees[0].email = "not an address".into();
    assert!(d.check().is_err());
    d.attendees.clear();
    d.title = "  ".into();
    assert!(d.check().is_err());
}

#[test]
fn a_repeat_until_a_date_runs_through_that_day() {
    let mut d = draft("2026-10-05T09:00", "2026-10-05T10:00");
    d.repeat = Some("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261102".into());
    // The end of 2 November in Halifax, after the clocks went back, in UTC.
    assert_eq!(
        d.rrule().unwrap().as_deref(),
        Some("FREQ=WEEKLY;BYDAY=MO;UNTIL=20261103T035959Z")
    );
    let mut all_day = draft("2026-10-05", "2026-10-06");
    all_day.repeat = Some("FREQ=DAILY;UNTIL=2026-10-07".into());
    assert_eq!(
        all_day.rrule().unwrap().as_deref(),
        Some("FREQ=DAILY;UNTIL=20261007")
    );
}

#[test]
fn a_new_object_reads_back_as_written_with_its_zone_defined() {
    let mut d = draft("2026-10-26T09:00", "2026-10-26T09:30");
    d.repeat = Some("FREQ=WEEKLY;BYDAY=MO;COUNT=3".into());
    let rule = d.rrule().unwrap();
    let ics = ical::new_object(
        "u1@orchestrate",
        &d,
        rule.as_deref(),
        Some("me@example.com"),
    )
    .unwrap();
    assert!(ics.lines().all(|l| l.len() <= 76), "folded:\n{ics}");
    assert!(ics.contains("TZID:America/Halifax"));
    assert!(ics.contains("RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU"));
    assert!(ics.contains("RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU"));
    let items = ical::parse_events(&ics);
    let e = &items[0];
    assert_eq!(e.title, "Plan, then build; ship");
    assert_eq!(e.description.as_deref(), Some("First\nsecond"));
    assert_eq!(e.organizer.as_ref().unwrap().email, "me@example.com");
    let people: Vec<_> = e
        .attendees
        .iter()
        .map(|a| (a.email.as_str(), a.response.as_deref()))
        .collect();
    assert_eq!(
        people,
        [
            ("me@example.com", Some("accepted")),
            ("ada@example.com", Some("needs-action"))
        ]
    );
    // 09:00 Halifax across the change of clocks on 1 November.
    assert_eq!(
        starts_of(&ics, "2026-10-01T00:00:00Z", "2026-12-01T00:00:00Z")
            .into_iter()
            .map(|(s, _)| s)
            .collect::<Vec<_>>(),
        [
            "2026-10-26T12:00:00Z",
            "2026-11-02T13:00:00Z",
            "2026-11-09T13:00:00Z"
        ]
    );
}

const WITH_ALARM: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//other app//EN\r\n\
BEGIN:VEVENT\r\nUID:w1\r\nSEQUENCE:4\r\nSUMMARY:Weekly\r\nX-OTHER-APP:keep me\r\n\
DTSTART;TZID=America/Halifax:20261005T090000\r\nDTEND;TZID=America/Halifax:20261005T093000\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO\r\n\
ORGANIZER:mailto:me@example.com\r\n\
ATTENDEE;PARTSTAT=ACCEPTED:mailto:me@example.com\r\n\
ATTENDEE;CN=Ada;PARTSTAT=TENTATIVE:mailto:ada@example.com\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:The alarm's own\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

#[test]
fn an_edit_changes_what_the_draft_decides_and_keeps_the_rest() {
    let mut obj = ical::Object::parse(WITH_ALARM);
    let span = obj.master().unwrap();
    let mut d = draft("2026-10-05T10:00", "2026-10-05T11:00");
    d.title = "Weekly, later".into();
    d.repeat = Some("FREQ=WEEKLY;BYDAY=MO".into());
    d.attendees.push(write::Guest {
        email: "bob@example.com".into(),
        name: None,
    });
    obj.apply(
        span,
        &d,
        d.rrule().unwrap().as_deref(),
        Some("me@example.com"),
        false,
    )
    .unwrap();
    let ics = obj.render();
    for kept in [
        "X-OTHER-APP:keep me",
        "DESCRIPTION:The alarm's own",
        "TRIGGER:-PT10M",
        "SEQUENCE:5",
    ] {
        assert!(ics.contains(kept), "{kept} in:\n{ics}");
    }
    assert_eq!(ics.matches("RRULE:FREQ=WEEKLY").count(), 1);
    let items = ical::parse_events(&ics);
    let e = &items[0];
    assert_eq!(e.title, "Weekly, later");
    let ada = e
        .attendees
        .iter()
        .find(|a| a.email == "ada@example.com")
        .unwrap();
    assert_eq!(
        ada.response.as_deref(),
        Some("tentative"),
        "her answer carries over"
    );
    assert!(e.attendees.iter().any(|a| a.email == "bob@example.com"));
}

#[test]
fn changing_one_occurrence_splits_it_off_and_deleting_one_excludes_it() {
    let master = ical::parse_events(WITH_ALARM).remove(0);
    let mut obj = ical::Object::parse(WITH_ALARM);
    // The occurrence on 12 October, 09:00 Halifax = 12:00Z.
    let span = obj.split_off("20261012T120000Z", &master, Tz::UTC).unwrap();
    let mut d = draft("2026-10-13T15:00", "2026-10-13T15:30");
    d.title = "Weekly (moved)".into();
    obj.apply(span, &d, None, None, true).unwrap();
    obj.exclude("20261019T120000Z", &master, Tz::UTC).unwrap();
    let ics = obj.render();
    assert!(ics.contains("RECURRENCE-ID;TZID=America/Halifax:20261012T090000"));
    assert!(ics.contains("EXDATE;TZID=America/Halifax:20261019T090000"));
    assert_eq!(
        starts_of(&ics, "2026-10-05T00:00:00Z", "2026-10-27T00:00:00Z"),
        [
            ("2026-10-05T12:00:00Z".to_string(), "Weekly".to_string()),
            ("2026-10-13T18:00:00Z".into(), "Weekly (moved)".into()),
            ("2026-10-26T12:00:00Z".into(), "Weekly".into()),
        ]
    );
}

#[test]
fn answering_sets_the_users_partstat_only() {
    let mut obj = ical::Object::parse(&WITH_ALARM.replace(
        "ORGANIZER:mailto:me@example.com",
        "ORGANIZER:mailto:ada@example.com",
    ));
    let span = obj.master().unwrap();
    obj.answer(span, "me@example.com", "DECLINED").unwrap();
    assert!(obj.answer(span, "nobody@example.com", "DECLINED").is_err());
    let e = ical::parse_events(&obj.render()).remove(0);
    let me = e
        .attendees
        .iter()
        .find(|a| a.email == "me@example.com")
        .unwrap();
    assert_eq!(me.response.as_deref(), Some("declined"));
    assert!(
        obj.render().contains("SEQUENCE:4"),
        "an answer isn't a change to it"
    );
}

fn target(uid: &str, occurrence: &str) -> write::Target {
    write::Target {
        account_id: "acct".into(),
        calendar_id: "me@example.com".into(),
        uid: uid.into(),
        occurrence: occurrence.into(),
    }
}

/// A Google account on a mock server with the review (weekly, with Ada) in
/// its cache, ready to write.
async fn google_host(server: &MockServer) -> (tempfile::TempDir, Arc<Calendars>) {
    mock_calendar_list(server).await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/me%40example.com/events"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"items": [google_event()], "nextSyncToken": "t"})),
        )
        .mount(server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (cals, _) = calendars(dir.path(), server);
    cals.set_google_client("cid", "secret").unwrap();
    cals.add(Account {
        id: "acct".into(),
        name: "me@example.com".into(),
        source: Source::Google(fresh_tokens()),
    })
    .await
    .unwrap();
    (dir, cals)
}

fn sent(req: &wiremock::Request) -> Value {
    serde_json::from_slice(&req.body).unwrap()
}

#[tokio::test]
async fn google_writes_and_invites() {
    let server = MockServer::start().await;
    let (_dir, cals) = google_host(&server).await;
    let api = "/calendar/v3/calendars/me%40example.com/events";
    Mock::given(method("POST"))
        .and(path(api))
        .and(query_param("sendUpdates", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "new"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("{api}/abc_20261019T130000Z")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(format!("{api}/abc")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let mut d = draft("2026-10-06T09:00", "2026-10-06T09:30");
    d.repeat = Some("FREQ=WEEKLY;BYDAY=TU;UNTIL=20261103".into());
    cals.write(
        "acct",
        Op::Create {
            calendar_id: "me@example.com".into(),
            draft: d,
        },
    )
    .await
    .unwrap();
    // One occurrence of the review, moved.
    let mut moved = draft("2026-10-19T11:00", "2026-10-19T12:00");
    moved.attendees = vec![write::Guest {
        email: "ada@example.com".into(),
        name: None,
    }];
    cals.write(
        "acct",
        Op::Update {
            target: target("abc", "20261019T130000Z"),
            draft: moved,
            scope: write::Scope::This,
        },
    )
    .await
    .unwrap();
    cals.write(
        "acct",
        Op::Respond {
            target: target("abc", "20261019T130000Z"),
            answer: write::Answer::Declined,
            scope: write::Scope::This,
        },
    )
    .await
    .unwrap();
    cals.write(
        "acct",
        Op::Delete {
            target: target("abc", ""),
            scope: write::Scope::All,
        },
    )
    .await
    .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let created = sent(
        reqs.iter()
            .find(|r| r.method.as_str() == "POST" && r.url.path() == api)
            .unwrap(),
    );
    assert_eq!(
        created["start"],
        json!({"date": null, "dateTime": "2026-10-06T09:00:00", "timeZone": "America/Halifax"})
    );
    assert_eq!(
        created["recurrence"],
        json!(["RRULE:FREQ=WEEKLY;BYDAY=TU;UNTIL=20261104T035959Z"])
    );
    assert_eq!(created["attendees"][0]["email"], "ada@example.com");
    let patches: Vec<Value> = reqs
        .iter()
        .filter(|r| r.method.as_str() == "PATCH")
        .map(sent)
        .collect();
    assert!(
        patches[0].get("recurrence").is_none(),
        "one occurrence has no repeat of its own"
    );
    assert_eq!(
        patches[0]["attendees"][0]["responseStatus"], "needsAction",
        "Ada's answer kept"
    );
    let mine = patches[1]["attendees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["email"] == "me@example.com")
        .unwrap()
        .clone();
    assert_eq!(mine["responseStatus"], "declined");
}

/// Google merges a PATCH's `start` into the one it has, so a Calendar event
/// made all-day, or given a time again, would keep its old `date` or
/// `dateTime` beside the new one, and Google refuses that as an invalid
/// start time. Each says the other is gone.
#[tokio::test]
async fn google_clears_the_old_kind_of_time_when_all_day_changes() {
    let server = MockServer::start().await;
    let (_dir, cals) = google_host(&server).await;
    let api = "/calendar/v3/calendars/me%40example.com/events";
    Mock::given(method("PATCH"))
        .and(path(format!("{api}/abc")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(2)
        .mount(&server)
        .await;

    for d in [
        draft("2026-10-05", "2026-10-06"),
        draft("2026-10-05T14:00", "2026-10-05T15:00"),
    ] {
        cals.write(
            "acct",
            Op::Update {
                target: target("abc", ""),
                draft: d,
                scope: write::Scope::All,
            },
        )
        .await
        .unwrap();
    }

    let reqs = server.received_requests().await.unwrap();
    let patches: Vec<Value> = reqs
        .iter()
        .filter(|r| r.method.as_str() == "PATCH")
        .map(sent)
        .collect();
    assert_eq!(
        patches[0]["start"],
        json!({"date": "2026-10-05", "dateTime": null, "timeZone": null})
    );
    assert_eq!(
        patches[1]["end"],
        json!({"date": null, "dateTime": "2026-10-05T15:00:00", "timeZone": "America/Halifax"})
    );
}

#[tokio::test]
async fn google_asks_to_sign_in_again_for_a_token_without_writing() {
    let server = MockServer::start().await;
    let (_dir, cals) = google_host(&server).await;
    Mock::given(method("POST"))
        .and(path("/calendar/v3/calendars/me%40example.com/events"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
            "message": "Request had insufficient authentication scopes.",
            "errors": [{"reason": "insufficientPermissions"}]}})))
        .mount(&server)
        .await;
    let err = cals
        .write(
            "acct",
            Op::Create {
                calendar_id: "me@example.com".into(),
                draft: draft("2026-10-06T09:00", "2026-10-06T10:00"),
            },
        )
        .await
        .unwrap_err();
    assert!(err.contains("sign in to Google again"), "{err}");
    assert!(matches!(
        cals.overview().accounts[0].status,
        Status::Reconnect { .. }
    ));
}

// ---------------------------------------------------------------- Microsoft

fn graph_occurrence() -> Value {
    json!({
        "id": "occ-1", "type": "occurrence", "seriesMasterId": "series-1",
        "subject": "Sync with Contoso", "isAllDay": false, "isCancelled": false,
        "start": {"dateTime": "2026-10-07T13:00:00.0000000", "timeZone": "UTC"},
        "end": {"dateTime": "2026-10-07T13:30:00.0000000", "timeZone": "UTC"},
        "location": {"displayName": "Teams"},
        "body": {"contentType": "html", "content": "<p>Agenda</p>"},
        "showAs": "busy", "isOrganizer": false,
        "organizer": {"emailAddress": {"name": "Ada", "address": "ada@contoso.com"}},
        "attendees": [{"type": "required", "status": {"response": "accepted"},
                       "emailAddress": {"name": "Ada", "address": "ada@contoso.com"}}],
        "responseStatus": {"response": "tentativelyAccepted"},
        "onlineMeeting": {"joinUrl": "https://teams.microsoft.com/l/meetup-join/abc"},
        "webLink": "https://outlook.live.com/calendar/item/occ-1"
    })
}

#[test]
fn reads_a_graph_occurrence() {
    let item = microsoft::item(&graph_occurrence(), Some("me@outlook.com")).unwrap();
    assert_eq!(item.uid, "series-1");
    assert!(item.recurrence_id.is_some());
    assert_eq!(item.description.as_deref(), Some("Agenda"));
    assert_eq!(
        item.meeting_url.as_deref(),
        Some("https://teams.microsoft.com/l/meetup-join/abc")
    );
    let me = item.attendees.iter().find(|a| a.is_self).unwrap();
    assert_eq!(
        (me.email.as_str(), me.response.as_deref()),
        ("me@outlook.com", Some("tentative"))
    );
    // An all-day one read in UTC lands on its own date.
    let mut all_day = graph_occurrence();
    all_day["isAllDay"] = json!(true);
    all_day["start"]["dateTime"] = json!("2026-10-07T21:00:00.0000000");
    let item = microsoft::item(&all_day, None).unwrap();
    assert_eq!(
        item.start,
        When::Date {
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()
        }
    );
}

#[test]
fn rrules_become_graph_patterns() {
    let d = |s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let p = microsoft::pattern(
        "FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261104T025959Z",
        d("2026-10-05"),
        Tz::America__Halifax,
    )
    .unwrap();
    assert_eq!(
        p["pattern"],
        json!({"type": "weekly", "interval": 1, "daysOfWeek": ["monday", "wednesday"], "firstDayOfWeek": "sunday"})
    );
    assert_eq!(
        p["range"],
        json!({"type": "endDate", "startDate": "2026-10-05", "endDate": "2026-11-03"})
    );
    let p =
        microsoft::pattern("FREQ=MONTHLY;BYDAY=-1FR;COUNT=6", d("2026-10-30"), Tz::UTC).unwrap();
    assert_eq!(p["pattern"]["type"], "relativeMonthly");
    assert_eq!(p["pattern"]["index"], "last");
    assert_eq!(p["range"]["numberOfOccurrences"], 6);
    let p = microsoft::pattern("FREQ=YEARLY", d("2026-03-15"), Tz::UTC).unwrap();
    assert_eq!(
        p["pattern"],
        json!({"type": "absoluteYearly", "interval": 1, "month": 3, "dayOfMonth": 15})
    );
}

#[tokio::test]
async fn microsoft_syncs_by_delta_and_writes_through_graph() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/graph/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"mail": "me@outlook.com"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/graph/me/calendars"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
            {"id": "cal-1", "name": "Calendar", "hexColor": "", "color": "lightBlue",
             "isDefaultCalendar": true, "canEdit": true}]})))
        .mount(&server)
        .await;
    let delta = format!("{}/graph/delta-next", server.uri());
    Mock::given(method("GET"))
        .and(path("/graph/me/calendars/cal-1/calendarView/delta"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [graph_occurrence()], "@odata.deltaLink": delta})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/graph/delta-next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [{"id": "occ-1", "@removed": {"reason": "deleted"}}],
            "@odata.deltaLink": delta})))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/graph/me/events/series-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graph/me/events/occ-1/decline"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let (cals, _) = calendars(dir.path(), &server);
    cals.set_microsoft_client("app-id").unwrap();
    // Added without syncing, so the writes below find the occurrence cached.
    cals.accounts.lock().unwrap().push(Account {
        id: "ms".into(),
        name: "me@outlook.com".into(),
        source: Source::Microsoft(fresh_tokens()),
    });
    {
        let _lock = cals.syncing.lock().await;
        cals.sync_locked("ms").await;
    }
    let events = cals
        .events("2026-10-07T00:00:00Z", "2026-10-08T00:00:00Z")
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        cals.overview().accounts[0].calendars[0].color.as_deref(),
        Some("#4f9fe8")
    );
    let e = &events[0];
    let target = write::Target {
        account_id: "ms".into(),
        calendar_id: "cal-1".into(),
        uid: e.uid.clone(),
        occurrence: e.occurrence.clone(),
    };
    let mut d = draft("2026-10-07T10:00", "2026-10-07T10:30");
    d.repeat = Some("FREQ=WEEKLY;BYDAY=WE".into());
    let reqs = || async { server.received_requests().await.unwrap() };
    // The writes sync after; the delta then says the occurrence is gone.
    cals.write(
        "ms",
        Op::Respond {
            target: target.clone(),
            answer: write::Answer::Declined,
            scope: write::Scope::This,
        },
    )
    .await
    .unwrap();
    assert!(cals
        .events("2026-10-07T00:00:00Z", "2026-10-08T00:00:00Z")
        .unwrap()
        .is_empty());
    // Put it back to change the series.
    {
        let mut caches = cals.caches.lock().unwrap();
        let cal = &mut caches.get_mut("ms").unwrap().calendars[0];
        cal.resources.insert(
            "occ-1".into(),
            Resource {
                etag: None,
                items: vec![microsoft::item(&graph_occurrence(), None).unwrap()],
                raw: None,
            },
        );
    }
    cals.write(
        "ms",
        Op::Update {
            target,
            draft: d,
            scope: write::Scope::All,
        },
    )
    .await
    .unwrap();
    let patch = reqs()
        .await
        .into_iter()
        .find(|r| r.method.as_str() == "PATCH")
        .unwrap();
    let b = sent(&patch);
    assert_eq!(
        b["recurrence"]["pattern"]["daysOfWeek"],
        json!(["wednesday"])
    );
    assert_eq!(
        b["attendees"][0]["emailAddress"]["address"],
        "ada@example.com"
    );
    let token_prefer = reqs()
        .await
        .into_iter()
        .find(|r| r.url.path() == "/graph/me/calendars")
        .unwrap();
    assert!(token_prefer
        .headers
        .get("prefer")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("UTC"));
}

#[tokio::test]
async fn signing_in_to_microsoft_goes_back_to_localhost_with_no_secret() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/ms/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "a", "expires_in": 3599, "refresh_token": "r"})))
        .mount(&server)
        .await;
    let ep = ms_endpoints(&server);
    let client = oauth::Client {
        id: "app-id".into(),
        secret: None,
    };
    let pending = oauth::SignIn::start(&ep.oauth(), &client).await.unwrap();
    let url = reqwest::Url::parse(&pending.url).unwrap();
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert!(q["redirect_uri"].starts_with("http://localhost:"));
    assert!(q["scope"].contains("offline_access") && q["scope"].contains("Calendars.ReadWrite"));
    let port = q["redirect_uri"].rsplit(':').next().unwrap().to_string();
    let http = reqwest::Client::new();
    let (h2, c2) = (http.clone(), client.clone());
    let finishing = tokio::spawn(async move { pending.finish(&h2, &c2).await });
    http.get(format!(
        "http://127.0.0.1:{port}/?code=c&state={}",
        q["state"]
    ))
    .send()
    .await
    .unwrap();
    let tokens = finishing.await.unwrap().unwrap();
    assert_eq!(tokens.refresh_token, "r");
    let form =
        String::from_utf8(server.received_requests().await.unwrap()[0].body.clone()).unwrap();
    let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(form.as_bytes())
        .into_owned()
        .collect();
    assert!(!form.contains_key("client_secret"));
    assert!(form["scope"].contains("Calendars.ReadWrite"));
}

/// Writing to a real CalDAV server, when one is given (see above).
#[tokio::test]
async fn caldav_writes_against_a_real_server() {
    let _turn = REAL_SERVER.lock().await;
    let Ok(given) = std::env::var("ORCHESTRATE_TEST_CALDAV") else {
        eprintln!("skipped: ORCHESTRATE_TEST_CALDAV is not set");
        return;
    };
    let mut parts = given.split_whitespace();
    let (url, user, pass) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    let http = reqwest::Client::new();
    let cal_url = format!(
        "{}/{user}/orchestrate-w-{}/",
        url.trim_end_matches('/'),
        crate::domain::new_id()
    );
    let mk = http
        .request(
            reqwest::Method::from_bytes(b"MKCALENDAR").unwrap(),
            &cal_url,
        )
        .basic_auth(user, Some(pass))
        .send()
        .await
        .unwrap();
    assert!(mk.status().is_success());

    let mut login = caldav::Login {
        url: url.to_string(),
        username: user.into(),
        password: pass.into(),
        home: None,
        address: None,
        schedules: false,
    };
    let mut cache = AccountCache::default();
    let mut dav = caldav::CalDav::new(&http, &mut login);
    dav.discover().await.unwrap();
    dav.sync(&mut cache).await.unwrap();
    let cal = |c: &AccountCache| {
        c.calendars
            .iter()
            .find(|x| x.id == cal_url)
            .unwrap()
            .clone()
    };

    let mut d = draft("2026-10-05T09:00", "2026-10-05T09:30");
    d.title = "Weekly".into();
    d.attendees.clear();
    d.repeat = Some("FREQ=WEEKLY;BYDAY=MO;COUNT=4".into());
    dav.create(&cal(&cache), &d).await.unwrap();
    dav.sync(&mut cache).await.unwrap();
    let uid = cal(&cache).resources.values().next().unwrap().items[0]
        .uid
        .clone();
    let tgt = |occ: &str| write::Target {
        account_id: "a".into(),
        calendar_id: cal_url.clone(),
        uid: uid.clone(),
        occurrence: occ.into(),
    };
    let home = home_zone();
    let titles = |c: &AccountCache| -> Vec<(String, String)> {
        let c = cal(c);
        let items: Vec<&Item> = c.resources.values().flat_map(|r| &r.items).collect();
        expand::expand(
            &items,
            t("2026-10-01T00:00:00Z"),
            t("2026-11-01T00:00:00Z"),
            home,
        )
        .iter()
        .map(|o| {
            let e = expand::to_event(o, "a", "c", None, home);
            (e.start, e.title)
        })
        .collect()
    };
    assert_eq!(titles(&cache).len(), 4);

    // Move the second, drop the third, then rename them all.
    let mut moved = d.clone();
    moved.title = "Weekly (moved)".into();
    (moved.start, moved.end, moved.repeat) =
        ("2026-10-13T14:00".into(), "2026-10-13T14:30".into(), None);
    let found = find(&cal(&cache), &tgt("20261012T120000Z"), home).unwrap();
    dav.update(&cal(&cache), &found, &moved, write::Scope::This)
        .await
        .unwrap();
    dav.sync(&mut cache).await.unwrap();
    let found = find(&cal(&cache), &tgt("20261019T120000Z"), home).unwrap();
    dav.delete(&cal(&cache), &found, write::Scope::This)
        .await
        .unwrap();
    dav.sync(&mut cache).await.unwrap();
    let mut renamed = d.clone();
    renamed.title = "Weekly sync".into();
    let found = find(&cal(&cache), &tgt(""), home).unwrap();
    dav.update(&cal(&cache), &found, &renamed, write::Scope::All)
        .await
        .unwrap();
    dav.sync(&mut cache).await.unwrap();
    assert_eq!(
        titles(&cache),
        [
            (
                "2026-10-05T12:00:00Z".to_string(),
                "Weekly sync".to_string()
            ),
            ("2026-10-13T17:00:00Z".into(), "Weekly (moved)".into()),
            ("2026-10-26T12:00:00Z".into(), "Weekly sync".into()),
        ]
    );
    // A write against a stale etag is refused rather than overwriting.
    let mut stale = find(&cal(&cache), &tgt(""), home).unwrap();
    stale.etag = Some("\"not-it\"".into());
    let err = dav
        .update(&cal(&cache), &stale, &renamed, write::Scope::All)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("changed on the server"), "{err}");

    let found = find(&cal(&cache), &tgt(""), home).unwrap();
    dav.delete(&cal(&cache), &found, write::Scope::All)
        .await
        .unwrap();
    dav.sync(&mut cache).await.unwrap();
    assert!(titles(&cache).is_empty());
    http.delete(&cal_url)
        .basic_auth(user, Some(pass))
        .send()
        .await
        .unwrap();
}

// ---------------------------------------------------------------- drafts

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_drafts_a_calendar_event_and_only_the_user_sends_it() {
    let _env = crate::test_util::StateEnv::new();
    crate::paths::ensure_dirs().unwrap();
    let (rt, rx) = crate::runtime::AgentRuntime::with_bin("true");
    let host = crate::host::Host::new(rt, rx, vec![]);
    let socket = crate::paths::host_socket().unwrap();
    let listener = crate::host::local::Listener::bind(&socket).unwrap();
    let serving = tokio::spawn(crate::host::serve(host, listener, std::future::ready(None)));

    let mcp = crate::mcp::Host::local().await.unwrap();
    let said = mcp::draft_event(
        mcp.clone(),
        mcp::DraftArgs {
            title: "Pair on the rail".into(),
            start: "2026-10-06T14:00:00-03:00".into(),
            end: "2026-10-06T15:00:00-03:00".into(),
            location: None,
            description: None,
            attendees: Some(vec!["ada@example.com".into()]),
            repeat: None,
            note: Some("You're both free then.".into()),
        },
    )
    .await
    .unwrap();
    assert!(said.contains("nothing is saved"), "{said}");

    // A window sees it, marked as a draft, and nothing went anywhere.
    let shown: Vec<CalendarEvent> = mcp
        .call_as(
            "calendar_events",
            json!({ "from": "2026-10-06T00:00:00-03:00", "to": "2026-10-07T00:00:00-03:00" }),
        )
        .await
        .unwrap();
    assert_eq!(shown.len(), 1);
    let d = &shown[0];
    assert_eq!(d.title, "Pair on the rail");
    assert!(d.draft_id.is_some());
    assert_eq!(d.attendees[0].email, "ada@example.com");
    assert_eq!(d.draft_note.as_deref(), Some("You're both free then."));
    assert_eq!(
        d.description, None,
        "the note isn't part of the Calendar event"
    );
    // An Agent reading the calendar sees its draft for what it is.
    let listed = mcp::list_events(
        mcp.clone(),
        mcp::Range {
            from: Some("2026-10-06".into()),
            to: Some("2026-10-07".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].draft);

    mcp.call("calendar_discard_draft", json!({ "id": d.draft_id }))
        .await
        .unwrap();
    let drafts: Vec<drafts::Proposed> = mcp.call_as("calendar_drafts", json!({})).await.unwrap();
    assert!(drafts.is_empty());
    serving.abort();
}
