use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "http://localhost:11434";
const MODEL: &str = "llama3.2";

pub struct OllamaClient {
    base_url: String,
    http: reqwest::Client,
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaClient {
    pub fn new() -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), http: build_http() }
    }

    pub fn with_base_url(base_url: String) -> Self {
        Self { base_url, http: build_http() }
    }

    pub async fn summarize(&self, notes: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let req = GenerateRequest { model: MODEL, prompt: build_prompt(notes), stream: false };
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

fn build_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build reqwest client")
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

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
