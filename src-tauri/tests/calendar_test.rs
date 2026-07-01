use ember_lib::calendar::CalendarClient;
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn list_calendars_parses_and_paginates() {
    let server = MockServer::start().await;
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
async fn insufficient_scope_403_maps_to_reconnect_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": { "code": 403, "message": "Request had insufficient authentication scopes.",
                       "status": "PERMISSION_DENIED" }
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let err = client.list_calendars().await.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("reconnect"), "got: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_disabled_403_surfaces_google_message_not_reconnect() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": { "code": 403,
                "message": "Google Calendar API has not been used in project 12345 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/calendar-json.googleapis.com/overview?project=12345 then retry.",
                "status": "PERMISSION_DENIED" }
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let err = client.list_calendars().await.unwrap_err().to_string().to_lowercase();
    assert!(err.contains("has not been used") || err.contains("disabled"), "got: {err}");
    assert!(!err.contains("reconnect"), "API-disabled must not be a reconnect error: {err}");
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
    use ember_lib::calendar::types::{Attendee, GAttendee, GEvent, GEventDateTime};

    let timed = GEvent {
        id: "e1".into(),
        summary: Some("Standup".into()),
        start: Some(GEventDateTime { date_time: Some("2026-06-15T09:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T09:30:00-07:00".into()), date: None }),
        location: Some("Zoom".into()),
        status: Some("confirmed".into()),
        description: Some("daily sync".into()),
        html_link: Some("https://cal/e1".into()),
        hangout_link: Some("https://meet.google.com/abc".into()),
        attendees: Some(vec![
            GAttendee { email: "me@x.com".into(), response_status: Some("needsAction".into()), is_self: true },
            GAttendee { email: "b@y.com".into(), response_status: Some("accepted".into()), is_self: false },
        ]),
    };
    let m = map_event(timed, "primary", Some("#16a34a")).unwrap();
    assert_eq!(m.title, "Standup");
    assert_eq!(m.start, "2026-06-15T09:00:00-07:00");
    assert!(!m.all_day);
    assert_eq!(m.meet_link.as_deref(), Some("https://meet.google.com/abc"));
    assert_eq!(m.html_link.as_deref(), Some("https://cal/e1"));
    assert_eq!(
        m.attendees,
        vec![
            Attendee { email: "me@x.com".into(), response_status: Some("needsAction".into()), is_self: true },
            Attendee { email: "b@y.com".into(), response_status: Some("accepted".into()), is_self: false },
        ]
    );
    assert_eq!(m.my_response_status.as_deref(), Some("needsAction"));

    let allday = GEvent {
        id: "e2".into(),
        summary: None,
        start: Some(GEventDateTime { date_time: None, date: Some("2026-06-16".into()) }),
        end: Some(GEventDateTime { date_time: None, date: Some("2026-06-17".into()) }),
        location: None,
        status: None,
        description: None,
        html_link: None,
        hangout_link: None,
        attendees: None,
    };
    let m2 = map_event(allday, "primary", None).unwrap();
    assert_eq!(m2.title, "(no title)");
    assert!(m2.all_day);
    assert!(m2.attendees.is_empty());
    assert_eq!(m2.my_response_status, None);

    let cancelled = GEvent {
        id: "e3".into(),
        summary: Some("Gone".into()),
        start: Some(GEventDateTime { date_time: Some("2026-06-15T10:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T10:30:00-07:00".into()), date: None }),
        location: None,
        status: Some("cancelled".into()),
        description: None,
        html_link: None,
        hangout_link: None,
        attendees: None,
    };
    assert!(map_event(cancelled, "primary", None).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn create_event_with_meet_posts_conference_and_attendees() {
    use ember_lib::calendar::types::EventWrite;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .and(query_param("conferenceDataVersion", "1"))
        .and(query_param("sendUpdates", "all"))
        .and(body_partial_json(json!({
            "summary": "Sync",
            "attendees": [{ "email": "a@x.com" }],
            "conferenceData": { "createRequest": { "conferenceSolutionKey": { "type": "hangoutsMeet" } } }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "new1",
            "summary": "Sync",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end": { "dateTime": "2026-06-21T11:00:00-07:00" },
            "hangoutLink": "https://meet.google.com/xyz"
        })))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let ev = EventWrite {
        title: "Sync".into(),
        start: "2026-06-21T10:00:00-07:00".into(),
        end: "2026-06-21T11:00:00-07:00".into(),
        all_day: false,
        description: None,
        location: None,
        attendees: vec!["a@x.com".into()],
    };
    let created = client.create_event("primary", &ev, true).await.unwrap();
    assert_eq!(created.id, "new1");
    assert_eq!(created.meet_link.as_deref(), Some("https://meet.google.com/xyz"));
}

#[tokio::test(flavor = "multi_thread")]
async fn update_event_patches_without_conference_data() {
    use ember_lib::calendar::types::EventWrite;
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .and(query_param("sendUpdates", "all"))
        .and(body_partial_json(json!({ "summary": "Renamed" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "e9",
            "summary": "Renamed",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end": { "dateTime": "2026-06-21T11:00:00-07:00" }
        })))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let ev = EventWrite {
        title: "Renamed".into(),
        start: "2026-06-21T10:00:00-07:00".into(),
        end: "2026-06-21T11:00:00-07:00".into(),
        all_day: false,
        description: None,
        location: None,
        attendees: vec![],
    };
    let updated = client.update_event("primary", "e9", &ev).await.unwrap();
    assert_eq!(updated.title, "Renamed");
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_event_issues_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .and(query_param("sendUpdates", "all"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    client.delete_event("primary", "e9").await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn respond_to_event_flips_self_and_preserves_others() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "e9", "summary": "Sync",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end":   { "dateTime": "2026-06-21T11:00:00-07:00" },
            "attendees": [
                { "email": "me@x.com", "self": true, "responseStatus": "needsAction" },
                { "email": "boss@x.com", "responseStatus": "accepted" }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .and(query_param("sendUpdates", "all"))
        .and(body_partial_json(json!({
            "attendees": [
                { "email": "me@x.com", "responseStatus": "accepted" },
                { "email": "boss@x.com", "responseStatus": "accepted" }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "e9", "summary": "Sync",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end":   { "dateTime": "2026-06-21T11:00:00-07:00" },
            "attendees": [
                { "email": "me@x.com", "self": true, "responseStatus": "accepted" },
                { "email": "boss@x.com", "responseStatus": "accepted" }
            ]
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let ev = client.respond_to_event("primary", "e9", "accepted", "unused@x.com").await.unwrap();
    assert_eq!(ev.my_response_status.as_deref(), Some("accepted"));
}

#[tokio::test(flavor = "multi_thread")]
async fn respond_to_event_errors_when_not_a_guest() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "e9", "summary": "Solo",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end":   { "dateTime": "2026-06-21T11:00:00-07:00" },
            "attendees": [ { "email": "boss@x.com", "responseStatus": "accepted" } ]
        })))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let err = client.respond_to_event("primary", "e9", "accepted", "me@x.com").await.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("not a guest"), "got: {err}");
}

#[test]
fn is_safe_url_allows_only_web_schemes() {
    use ember_lib::calendar::is_safe_url;
    assert!(is_safe_url("https://meet.google.com/abc"));
    assert!(is_safe_url("http://example.com"));
    assert!(is_safe_url("  HTTPS://Cal.example/e1  "));
    assert!(!is_safe_url("file:///etc/passwd"));
    assert!(!is_safe_url("javascript:alert(1)"));
    assert!(!is_safe_url("mailto:a@b.com"));
    assert!(!is_safe_url(""));
}

#[tokio::test(flavor = "multi_thread")]
async fn free_busy_parses_busy_and_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/calendar/v3/freeBusy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "calendars": {
                "me@company.com": { "busy": [
                    { "start": "2026-07-01T09:00:00Z", "end": "2026-07-01T10:00:00Z" }
                ] },
                "ext@gmail.com": { "busy": [], "errors": [ { "reason": "notFound" } ] }
            }
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let emails = vec!["me@company.com".to_string(), "ext@gmail.com".to_string()];
    let fb = client
        .free_busy(&emails, "2026-07-01T00:00:00Z", "2026-07-01T23:00:00Z")
        .await
        .unwrap();

    let me = fb.calendars.get("me@company.com").unwrap();
    assert_eq!(me.busy.len(), 1);
    assert_eq!(me.busy[0].start, "2026-07-01T09:00:00Z");
    assert!(me.error.is_none());

    let ext = fb.calendars.get("ext@gmail.com").unwrap();
    assert!(ext.busy.is_empty());
    assert_eq!(ext.error.as_deref(), Some("notFound"));
}
