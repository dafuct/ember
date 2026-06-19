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
