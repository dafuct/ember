// src-tauri/src/whisper.rs — local Whisper STT client (audio/video → transcript, M23).
// Mirrors OllamaClient: a swappable base_url + a reusable reqwest::Client. Targets whisper.cpp's
// own `whisper-server`: POST /inference, multipart `file` + `response_format=json` → {"text": …}.
// The model is chosen when the SERVER starts, so the request carries no model name.
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};

// 🦀 whisper-server's default port. If something else already owns 8080, start it with
//    `--port <other>` (the URL is only overridable in tests via `with_base_url`).
const DEFAULT_BASE: &str = "http://localhost:8080";

pub struct WhisperClient {
    base_url: String,
    http: reqwest::Client,
}

// 🦀 `new()` takes no args, so clippy wants a matching `Default` impl — delegate to `new()`.
impl Default for WhisperClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperClient {
    pub fn new() -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), http: build_http() }
    }

    /// Point the client at a mock server in tests.
    pub fn with_base_url(base_url: String) -> Self {
        Self { base_url, http: build_http() }
    }

    /// Transcribe raw recording bytes via the local whisper-server. Maps the common local-setup
    /// failure (server not running) to an actionable message, and surfaces any server error body
    /// (e.g. "failed to decode audio") rather than a bare status code.
    pub async fn transcribe(&self, audio: Vec<u8>, filename: &str, mime: &str) -> Result<String> {
        let url = format!("{}/inference", self.base_url);
        // 🦀 `Part::bytes` needs `'static` data — an OWNED `Vec<u8>` satisfies that. `.mime_str`
        //    returns a reqwest::Result, so `?` works (AppError has `From<reqwest::Error>`).
        let part = reqwest::multipart::Part::bytes(audio)
            .file_name(filename.to_string())
            .mime_str(mime)?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("response_format", "json");

        // 🦀 `.send()` can fail before any HTTP status (e.g. connection refused). `is_connect()`
        //    distinguishes "couldn't reach the server" so we can show a friendly setup hint.
        let resp = self.http.post(&url).multipart(form).send().await.map_err(|e| {
            if e.is_connect() {
                AppError::Other(format!(
                    "Whisper server isn't running at {} — install whisper.cpp (e.g. `brew install whisper-cpp`) and run `whisper-server -m <model> --port 8080`.",
                    self.base_url
                ))
            } else {
                AppError::Http(e)
            }
        })?;

        // 🦀 On a non-2xx status, the body carries the useful message (bad format, no model
        //    loaded), so we read it instead of throwing away detail with `error_for_status()`.
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Other(format!("Whisper server returned {status}: {body}")));
        }

        let parsed: InferenceResponse = resp.json().await?;
        let text = parsed.text.trim().to_string();
        if text.is_empty() {
            return Err(AppError::Other("Whisper returned an empty transcript.".into()));
        }
        Ok(text)
    }
}

// 🦀 One place to build the HTTP client: a generous 600s timeout — local CPU transcription of a
//    long recording is slow (Ollama's 120s would truncate a real meeting).
fn build_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("failed to build reqwest client")
}

// 🦀 We only need `text` from whisper-server's JSON; serde ignores any other fields.
#[derive(Deserialize)]
struct InferenceResponse {
    text: String,
}
