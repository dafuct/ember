// 🦀 Files in `tests/` are *integration tests* — each file is compiled as its
//    own separate crate.  Because of this, the library is accessed as an
//    external dependency: `ember_lib::gmail::GmailClient`, not `crate::gmail`.
//    This mirrors how a real downstream user would consume the library.
use ember_lib::gmail::GmailClient;
use serde_json::json;
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

// 🦀 Helper: base64url-encode a string so tests can produce mock payloads without
//    depending on a separate fixture file.  Uses the same engine the production
//    code decodes with, so round-trips are guaranteed to match.
fn b64url(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
}

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
async fn get_message_preview_extracts_labels_and_list_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/n1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "n1",
            "labelIds": ["INBOX", "CATEGORY_PROMOTIONS"],
            "snippet": "Big sale",
            "payload": { "headers": [
                {"name": "From", "value": "Store <deals@store.com>"},
                {"name": "To", "value": "you@example.com"},
                {"name": "List-Unsubscribe", "value": "<mailto:unsub@store.com>"}
            ]}
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.get_message_preview("n1").await.unwrap();
    assert_eq!(m.label_ids, vec!["INBOX".to_string(), "CATEGORY_PROMOTIONS".to_string()]);
    assert_eq!(m.to_addr, "you@example.com");
    assert!(m.has_list_unsubscribe);
    assert!(!m.has_list_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_preview_flags_list_id_when_present() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/n2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "n2",
            "payload": { "headers": [
                {"name": "From", "value": "Dev List <team@list.example>"},
                {"name": "List-Id", "value": "<dev.list.example>"}
            ]}
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.get_message_preview("n2").await.unwrap();
    assert!(m.has_list_id);
    assert!(!m.has_list_unsubscribe);
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

#[tokio::test(flavor = "multi_thread")]
async fn list_history_collects_added_removed_and_archived() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "history": [
                { "messagesAdded": [{"message": {"id": "m1", "threadId": "t1"}}] },
                { "messagesDeleted": [{"message": {"id": "m2", "threadId": "t2"}}] },
                { "labelsRemoved": [{"message": {"id": "m3", "threadId": "t3"}, "labelIds": ["INBOX"]}] },
                { "labelsAdded": [{"message": {"id": "m4", "threadId": "t4"}, "labelIds": ["INBOX"]}] }
            ],
            "historyId": "999"
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let delta = client.list_history("100").await.unwrap();
    let mut added = delta.added_ids.clone();
    added.sort();
    let mut removed = delta.removed_ids.clone();
    removed.sort();
    assert_eq!(added, vec!["m1".to_string(), "m4".to_string()]);
    assert_eq!(removed, vec!["m2".to_string(), "m3".to_string()]);
    assert_eq!(delta.new_history_id, Some("999".to_string()));
    assert!(!delta.too_old);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_history_nets_add_then_archive_to_removed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "history": [
                { "messagesAdded": [{"message": {"id": "m1", "threadId": "t1"}}] },
                { "labelsRemoved": [{"message": {"id": "m1", "threadId": "t1"}, "labelIds": ["INBOX"]}] }
            ],
            "historyId": "1000"
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let delta = client.list_history("100").await.unwrap();
    assert_eq!(delta.added_ids, Vec::<String>::new());
    assert_eq!(delta.removed_ids, vec!["m1".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_history_flags_too_old_on_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let delta = client.list_history("1").await.unwrap();
    assert!(delta.too_old);
    assert!(delta.added_ids.is_empty());
    assert!(delta.removed_ids.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_body_extracts_html_from_multipart() {
    let server = MockServer::start().await;
    let html = "<p>Hello <b>world</b></p>";
    let text = "Hello world";
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m1",
            "payload": {
                "mimeType": "multipart/alternative",
                "parts": [
                    { "mimeType": "text/plain", "body": { "data": b64url(text) } },
                    { "mimeType": "text/html", "body": { "data": b64url(html) } }
                ]
            }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let body = client.get_message_body("m1").await.unwrap();
    assert_eq!(body.html.as_deref(), Some(html));
    assert_eq!(body.text.as_deref(), Some(text));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_message_body_handles_simple_plaintext() {
    let server = MockServer::start().await;
    let text = "just text, no parts";
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m2",
            "payload": { "mimeType": "text/plain", "body": { "data": b64url(text) } }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let body = client.get_message_body("m2").await.unwrap();
    assert_eq!(body.text.as_deref(), Some(text));
    assert_eq!(body.html, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn modify_message_posts_labels_and_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/a1/modify"))
        .and(body_json(json!({ "addLabelIds": [], "removeLabelIds": ["UNREAD"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "labelIds": ["INBOX", "STARRED"]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.modify_message("a1", &[], &["UNREAD"]).await.unwrap();
    assert_eq!(m.id, "a1");
    assert_eq!(
        m.label_ids,
        vec!["INBOX".to_string(), "STARRED".to_string()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn modify_message_sends_add_labels() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/a2/modify"))
        .and(body_json(json!({ "addLabelIds": ["STARRED"], "removeLabelIds": [] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a2",
            "labelIds": ["INBOX", "STARRED"]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.modify_message("a2", &["STARRED"], &[]).await.unwrap();
    assert_eq!(
        m.label_ids,
        vec!["INBOX".to_string(), "STARRED".to_string()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn trash_message_posts_to_trash_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/a1/trash"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "labelIds": ["TRASH"]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    // 🦀 We only care that it succeeded; the response body is ignored.
    client.trash_message("a1").await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn get_reply_context_extracts_message_id_references_and_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/r1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "r1",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name": "Message-ID", "value": "<orig@mail.example>"},
                    {"name": "References", "value": "<a@x> <b@y>"}
                ],
                "parts": [
                    {"mimeType": "text/plain", "body": {"data": b64url("Original body")}}
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let rc = client.get_reply_context("r1").await.unwrap();
    assert_eq!(rc.message_id, "<orig@mail.example>");
    assert_eq!(rc.references, "<a@x> <b@y>");
    assert_eq!(rc.quoted_text, "Original body");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_reply_context_returns_empty_references_when_absent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/r2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "r2",
            "payload": {
                "mimeType": "text/plain",
                "headers": [{"name": "Message-ID", "value": "<only-id@mail>"}],
                "body": {"data": b64url("Body text")}
            }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let rc = client.get_reply_context("r2").await.unwrap();
    assert_eq!(rc.message_id, "<only-id@mail>");
    assert_eq!(rc.references, ""); // absent header → empty string (frontend maps "" → null)
    assert_eq!(rc.quoted_text, "Body text");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_message_posts_base64url_raw_with_thread_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/send"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "sent1"})))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client
        .send_message("From: a@b\r\n\r\nhi", Some("thread-9"))
        .await
        .unwrap();

    // 🦀 Inspect the request the mock server actually received.
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["threadId"], "thread-9");
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body["raw"].as_str().unwrap())
        .unwrap();
    assert_eq!(String::from_utf8(decoded).unwrap(), "From: a@b\r\n\r\nhi");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_message_omits_thread_id_when_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/send"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "sent2"})))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client.send_message("hello", None).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert!(body.get("threadId").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn search_message_ids_searches_all_mail_without_inbox_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("q", "from:maya"))
        .and(query_param_is_missing("labelIds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{ "id": "s1" }, { "id": "s2" }]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.search_message_ids("from:maya", 50).await.unwrap();
    assert_eq!(ids, vec!["s1".to_string(), "s2".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_inbox_message_ids_paged_still_filters_to_inbox() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("labelIds", "INBOX"))
        .and(query_param("q", "newer_than:30d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{ "id": "i1" }]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.list_inbox_message_ids_paged("newer_than:30d", 50).await.unwrap();
    assert_eq!(ids, vec!["i1".to_string()]);
}
