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

fn client() -> GoogleClient {
    GoogleClient {
        client_id: "cid".into(),
        client_secret: "secret".into(),
    }
}

fn endpoints(server: &MockServer) -> google::Endpoints {
    google::Endpoints {
        auth: format!("{}/auth", server.uri()),
        token: format!("{}/token", server.uri()),
        api: format!("{}/calendar/v3", server.uri()),
    }
}

fn fresh_tokens() -> google::Tokens {
    google::Tokens {
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
    let mut g = google::Google::new(&http, &ep, &client, &mut tokens);
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
    google::Google::new(&http, &ep, &client, &mut tokens)
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
    let mut tokens = google::Tokens {
        expires_at: chrono::Utc::now().timestamp() + 30,
        ..fresh_tokens()
    };
    let mut cache = AccountCache::default();
    google::Google::new(&http, &ep, &client, &mut tokens)
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
    let err = google::Google::new(&http, &ep, &client, &mut tokens)
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
    let pending = google::SignIn::start(&ep, &client).await.unwrap();
    let url = reqwest::Url::parse(&pending.url).unwrap();
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["access_type"], "offline");
    assert_eq!(q["scope"], google::SCOPE);
    let redirect = q["redirect_uri"].clone();
    assert!(redirect.starts_with("http://127.0.0.1:"));

    let http = reqwest::Client::new();
    let (ep2, client2, http2) = (ep.clone(), client.clone(), http.clone());
    let finishing = tokio::spawn(async move { pending.finish(&http2, &ep2, &client2).await });
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
    let sent: std::collections::HashMap<_, _> = google::query_pairs(&form).into_iter().collect();
    assert_eq!(sent["code"], "the-code");
    assert_eq!(sent["redirect_uri"], redirect);
    assert_eq!(
        google::challenge(&sent["code_verifier"]),
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
        endpoints(server),
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

/// Against a real CalDAV server, when one is given: `ORCHESTRATE_TEST_CALDAV`
/// as `url user password`, e.g. a throwaway Radicale.
#[tokio::test]
async fn caldav_against_a_real_server() {
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
