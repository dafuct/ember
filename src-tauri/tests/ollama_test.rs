use ember_lib::ollama::OllamaClient;
use serde_json::json;
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn summarize_posts_generate_request_and_returns_trimmed_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .and(body_partial_json(json!({ "model": "llama3.2", "stream": false })))
        .and(body_string_contains("## Action items"))
        .and(body_string_contains("Reviewed the Q3 roadmap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "response": "\n## Summary\n- Discussed Q3\n\n## Action items\n- [ ] Share doc\n",
            "done": true
        })))
        .mount(&server)
        .await;

    let client = OllamaClient::with_base_url(server.uri());
    let summary = client.summarize("Reviewed the Q3 roadmap and assigned the doc.").await.unwrap();
    assert!(summary.contains("## Summary"));
    assert!(summary.contains("- [ ] Share doc"));
    assert_eq!(summary, summary.trim());
}

#[tokio::test(flavor = "multi_thread")]
async fn summarize_maps_404_to_pull_instruction() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": "model 'llama3.2' not found, try pulling it first"
        })))
        .mount(&server)
        .await;
    let client = OllamaClient::with_base_url(server.uri());
    let err = client.summarize("notes").await.unwrap_err().to_string().to_lowercase();
    assert!(err.contains("ollama pull"), "got: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn summarize_maps_connection_refused_to_friendly_message() {
    let client = OllamaClient::with_base_url("http://127.0.0.1:1".into());
    let err = client.summarize("notes").await.unwrap_err().to_string().to_lowercase();
    assert!(err.contains("isn't running") || err.contains("ollama serve"), "got: {err}");
}
