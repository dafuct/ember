// 🦀 Files in `tests/` are *integration tests* — each file is compiled as its
//    own separate crate.  Because of this, the library is accessed as an
//    external dependency: `ember_lib::gmail::GmailClient`, not `crate::gmail`.
//    This mirrors how a real downstream user would consume the library.
use ember_lib::gmail::GmailClient;
use serde_json::json;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

// 🦀 `#[tokio::test]` is an attribute macro from the `tokio` crate.  It wraps
//    the async test function in a Tokio runtime so you can `.await` futures
//    inside a test without starting the runtime manually.
//    `flavor = "multi_thread"` spins up a full multi-threaded runtime, which
//    wiremock needs to serve HTTP requests concurrently with the client calls.
#[tokio::test(flavor = "multi_thread")]
async fn get_profile_parses_email() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "emailAddress": "jordan@example.com",
            "messagesTotal": 1234
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let profile = client.get_profile().await.unwrap();
    assert_eq!(profile.email_address, "jordan@example.com");
    assert_eq!(profile.messages_total, 1234);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_inbox_message_ids_collects_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("labelIds", "INBOX"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{"id": "a1"}, {"id": "a2"}]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.list_inbox_message_ids(20).await.unwrap();
    assert_eq!(ids, vec!["a1".to_string(), "a2".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_preview_extracts_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/a1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "snippet": "Hello there",
            "payload": { "headers": [
                {"name": "From", "value": "Maya <maya@studio.co>"},
                {"name": "Subject", "value": "Q3 roadmap"},
                {"name": "Date", "value": "Wed, 18 Jun 2026 09:42:00 -0700"}
            ]}
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.get_message_preview("a1").await.unwrap();
    assert_eq!(m.from, "Maya <maya@studio.co>");
    assert_eq!(m.subject, "Q3 roadmap");
    assert_eq!(m.snippet, "Hello there");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_inbox_message_ids_paged_follows_next_page_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{"id": "a1"}, {"id": "a2"}],
            "nextPageToken": "PAGE2"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("pageToken", "PAGE2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{"id": "a3"}]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client
        .list_inbox_message_ids_paged("newer_than:30d", 100)
        .await
        .unwrap();
    assert_eq!(ids, vec!["a1".to_string(), "a2".to_string(), "a3".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_preview_parses_internal_date_and_thread_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/a1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "threadId": "thread-9",
            "internalDate": "1718700000000",
            "snippet": "hi",
            "payload": { "headers": [{"name": "Subject", "value": "Hello"}] }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.get_message_preview("a1").await.unwrap();
    assert_eq!(m.thread_id, "thread-9");
    assert_eq!(m.internal_date, 1718700000000);
    assert_eq!(m.subject, "Hello");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_previews_fetches_all_ids() {
    let server = MockServer::start().await;
    for id in ["a1", "a2", "a3"] {
        Mock::given(method("GET"))
            .and(path(format!("/gmail/v1/users/me/messages/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": id,
                "threadId": "t",
                "internalDate": "1000",
                "snippet": "s",
                "payload": { "headers": [{"name": "From", "value": format!("{id}@x.com")}] }
            })))
            .mount(&server)
            .await;
    }
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = vec!["a1".to_string(), "a2".to_string(), "a3".to_string()];
    let mut previews = client.get_message_previews(&ids, 4).await.unwrap();
    previews.sort_by(|a, b| a.id.cmp(&b.id)); // buffer_unordered → sort for a stable assert
    let got: Vec<&str> = previews.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(got, vec!["a1", "a2", "a3"]);
}
