// 🦀 Integration tests live in a separate crate, so the client is reached via the public
//    crate path `ember_lib::whisper` (just like tests/ollama_test.rs reaches `ember_lib::ollama`).
use ember_lib::whisper::WhisperClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn transcribe_posts_inference_and_returns_trimmed_text() {
    let server = MockServer::start().await;
    // `.expect(1)` makes wiremock verify (on drop) that POST /inference was hit exactly once.
    Mock::given(method("POST"))
        .and(path("/inference"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "  hello from whisper  " })))
        .expect(1)
        .mount(&server)
        .await;

    let client = WhisperClient::with_base_url(server.uri());
    let text = client.transcribe(b"RIFFfake-wav".to_vec(), "a.wav", "audio/wav").await.unwrap();
    assert_eq!(text, "hello from whisper"); // trimmed
}

#[tokio::test(flavor = "multi_thread")]
async fn transcribe_maps_connection_refused_to_friendly_message() {
    // 🦀 Port 1 has nothing listening → immediate connection-refused (reqwest `is_connect()`).
    let client = WhisperClient::with_base_url("http://127.0.0.1:1".into());
    let err = client
        .transcribe(b"x".to_vec(), "a.wav", "audio/wav")
        .await
        .unwrap_err()
        .to_string()
        .to_lowercase();
    assert!(err.contains("isn't running"), "got: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn transcribe_surfaces_server_error_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/inference"))
        .respond_with(ResponseTemplate::new(400).set_body_string("failed to decode audio"))
        .mount(&server)
        .await;

    let client = WhisperClient::with_base_url(server.uri());
    let err = client
        .transcribe(b"x".to_vec(), "a.bin", "application/octet-stream")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("failed to decode audio"), "got: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn transcribe_rejects_empty_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/inference"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "   " })))
        .mount(&server)
        .await;

    let client = WhisperClient::with_base_url(server.uri());
    let err = client
        .transcribe(b"x".to_vec(), "a.wav", "audio/wav")
        .await
        .unwrap_err()
        .to_string()
        .to_lowercase();
    assert!(err.contains("empty"), "got: {err}");
}
