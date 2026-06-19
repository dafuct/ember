// 🦀 Integration tests: a separate crate, so the client is reached as `ember_lib::calendar`.
use ember_lib::calendar::CalendarClient;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn list_calendars_parses_and_paginates() {
    let server = MockServer::start().await;
    // Page 1 → has nextPageToken; Page 2 → no token.
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": "personal@group", "summary": "Personal", "backgroundColor": "#b9722a", "selected": true }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": "primary", "summary": "Me", "backgroundColor": "#16a34a", "primary": true }],
            "nextPageToken": "p2"
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let cals = client.list_calendars().await.unwrap();
    assert_eq!(cals.len(), 2);
    assert_eq!(cals[0].id, "primary");
    assert_eq!(cals[1].id, "personal@group");
    assert_eq!(cals[0].background_color.as_deref(), Some("#16a34a"));
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_scope_maps_to_reconnect_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let err = client.list_calendars().await.unwrap_err();
    // 🦀 We don't name the (private) AppError type — Display gives us the message string.
    assert!(err.to_string().to_lowercase().contains("reconnect"), "got: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_events_parses_timed_and_all_day_with_query_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .and(query_param("singleEvents", "true"))
        .and(query_param("orderBy", "startTime"))
        .and(query_param("timeMin", "2026-06-15T00:00:00-07:00"))
        .and(query_param("timeMax", "2026-06-22T00:00:00-07:00"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": "t1", "summary": "Standup",
                  "start": { "dateTime": "2026-06-15T09:00:00-07:00" },
                  "end":   { "dateTime": "2026-06-15T09:30:00-07:00" } },
                { "id": "a1", "summary": "Q3 planning",
                  "start": { "date": "2026-06-17" }, "end": { "date": "2026-06-19" } }
            ]
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let evts = client
        .list_events("primary", "2026-06-15T00:00:00-07:00", "2026-06-22T00:00:00-07:00")
        .await
        .unwrap();
    assert_eq!(evts.len(), 2);
    assert_eq!(evts[0].start.as_ref().unwrap().date_time.as_deref(), Some("2026-06-15T09:00:00-07:00"));
    assert_eq!(evts[1].start.as_ref().unwrap().date.as_deref(), Some("2026-06-17"));
}

#[test]
fn map_event_normalizes_and_skips_cancelled() {
    use ember_lib::calendar::map_event;
    use ember_lib::calendar::types::{GEvent, GEventDateTime};

    // timed event → all_day false, uses dateTime, color attaches, missing summary → "(no title)"
    let timed = GEvent {
        id: "t1".into(), summary: None,
        start: Some(GEventDateTime { date_time: Some("2026-06-15T09:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T09:30:00-07:00".into()), date: None }),
        location: None, status: None,
    };
    let m = map_event(timed, "primary", Some("#16a34a")).unwrap();
    assert!(!m.all_day);
    assert_eq!(m.title, "(no title)");
    assert_eq!(m.start, "2026-06-15T09:00:00-07:00");
    assert_eq!(m.color.as_deref(), Some("#16a34a"));
    assert_eq!(m.calendar_id, "primary");

    // all-day event → all_day true, uses date
    let allday = GEvent {
        id: "a1".into(), summary: Some("Q3 planning".into()),
        start: Some(GEventDateTime { date_time: None, date: Some("2026-06-17".into()) }),
        end: Some(GEventDateTime { date_time: None, date: Some("2026-06-19".into()) }),
        location: None, status: None,
    };
    let m2 = map_event(allday, "primary", None).unwrap();
    assert!(m2.all_day);
    assert_eq!(m2.start, "2026-06-17");
    assert_eq!(m2.title, "Q3 planning");

    // cancelled → None
    let cancelled = GEvent {
        id: "c1".into(), summary: Some("Old".into()),
        start: Some(GEventDateTime { date_time: Some("2026-06-15T09:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T09:30:00-07:00".into()), date: None }),
        location: None, status: Some("cancelled".into()),
    };
    assert!(map_event(cancelled, "primary", None).is_none());
}
