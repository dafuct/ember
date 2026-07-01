use ember_lib::zoom::client::ZoomClient;
use serde_json::json;
use wiremock::matchers::{method, path, body_partial_json};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn create_meeting_parses_join_url() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/users/me/meetings"))
        .and(body_partial_json(json!({ "type": 2, "topic": "Sync" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 89012345678i64,
            "join_url": "https://us05web.zoom.us/j/89012345678?pwd=abc",
            "password": "abc"
        })))
        .mount(&server)
        .await;

    let client = ZoomClient::with_base_url("tok".into(), server.uri());
    let m = client.create_meeting("Sync", "2026-07-02T14:00:00+03:00", 30, "Europe/Kyiv").await.unwrap();
    assert_eq!(m.id, "89012345678");
    assert_eq!(m.join_url, "https://us05web.zoom.us/j/89012345678?pwd=abc");
    assert_eq!(m.password.as_deref(), Some("abc"));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_me_parses_account() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/users/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "abc123", "email": "me@zoomer.com", "first_name": "Me"
        })))
        .mount(&server)
        .await;

    let client = ZoomClient::with_base_url("tok".into(), server.uri());
    let acct = client.get_me().await.unwrap();
    assert_eq!(acct.email, "me@zoomer.com");
    assert_eq!(acct.account_id, "abc123");
}

#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_maps_to_reconnect_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/users/me"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "message": "Invalid access token" })))
        .mount(&server)
        .await;

    let client = ZoomClient::with_base_url("tok".into(), server.uri());
    let err = client.get_me().await.unwrap_err();
    assert!(err.to_string().to_lowercase().contains("reconnect zoom"), "got: {err}");
}
