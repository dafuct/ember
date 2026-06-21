// src-tauri/src/ollama.rs — local Ollama client (meeting-note summarization, M21).
// Mirrors GmailClient/CalendarClient: a swappable base_url + a reusable reqwest::Client.
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "http://localhost:11434";
const MODEL: &str = "llama3.2";

pub struct OllamaClient {
    base_url: String,
    http: reqwest::Client,
}

// 🦀 `new()` takes no args, so clippy wants a matching `Default` impl — provide one that delegates.
impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaClient {
    pub fn new() -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), http: build_http() }
    }

    /// Point the client at a mock server in tests.
    pub fn with_base_url(base_url: String) -> Self {
        Self { base_url, http: build_http() }
    }

    /// Summarize meeting notes via Ollama's blocking /api/generate. Maps the two common
    /// local-setup failures (Ollama not running, model not pulled) to actionable messages.
    pub async fn summarize(&self, notes: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let req = GenerateRequest { model: MODEL, prompt: build_prompt(notes), stream: false };
        // 🦀 `.send()` can fail before any HTTP status — e.g. connection refused. `is_connect()`
        //    tells "couldn't even reach the server" apart from other errors, so we can show a
        //    friendly setup hint instead of a raw reqwest message.
        let resp = self.http.post(&url).json(&req).send().await.map_err(|e| {
            if e.is_connect() {
                AppError::Other(format!(
                    "Ollama isn't running at {} — install it from https://ollama.com and run `ollama serve`.",
                    self.base_url
                ))
            } else {
                AppError::Http(e)
            }
        })?;
        // 🦀 Ollama returns 404 when the requested model hasn't been pulled.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::Other(format!(
                "Ollama model '{MODEL}' not found. Run: ollama pull {MODEL}"
            )));
        }
        let resp = resp.error_for_status()?;
        let parsed: GenerateResponse = resp.json().await?;
        let summary = parsed.response.trim().to_string();
        if summary.is_empty() {
            return Err(AppError::Other("Ollama returned an empty summary.".into()));
        }
        Ok(summary)
    }
}

// 🦀 One place to build the HTTP client: a generous 120s timeout (local CPU generation is slow);
//    `.build()` returns a Result, and `.expect` mirrors what `reqwest::Client::new()` does internally.
fn build_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build reqwest client")
}

// 🦀 The /api/generate request body. `<'a>` lets `model` borrow the &'static str; `prompt` is owned.
#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
}

// 🦀 We only need `response` from Ollama's JSON; serde ignores the other fields (done, etc.).
#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

// 🦀 Pure prompt builder (no I/O), kept private — the wiremock happy-path test asserts its output
//    via the captured request body. Asks for a compact, factual markdown summary + action items.
fn build_prompt(notes: &str) -> String {
    format!(
        "You are a meeting-notes assistant. Summarize the meeting notes below into concise \
         GitHub-flavored markdown with exactly these two sections:\n\
         ## Summary\n- 2 to 4 short bullet points capturing the key points\n\
         ## Action items\n- one `- [ ]` checkbox per action item; if there are none, write \"_None_\"\n\n\
         Be factual and concise. Do not invent information that is not in the notes.\n\n\
         Notes:\n{notes}"
    )
}
