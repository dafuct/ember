# Ember M23 — Local Whisper STT of a recording file — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick an audio/video recording file, transcribe it via a local whisper.cpp HTTP server, and drop the text into a note's existing transcript field (which then feeds the unchanged M21/M22 summarize spine).

**Architecture:** A new `WhisperClient` (`src-tauri/src/whisper.rs`) mirrors the M21 `OllamaClient` — it POSTs the file as `multipart/form-data` to whisper.cpp's `whisper-server` (`POST /inference`, `response_format=json` → `{"text": …}`) and maps the common local-setup failures to friendly messages. A DB-free command `transcribe_recording(path)` mirrors `read_transcript_file` (size guard → `std::fs::read` → client call). The frontend adds a **Transcribe…** button beside M22's **Import…**, filling the same transcript textarea.

**Tech Stack:** Rust (reqwest + the `multipart` feature flag, serde, Tauri 2, `tauri-plugin-dialog` — all present except the feature flag), React 19 + TypeScript + Vite, wiremock for tests.

**Learning mode (IMPORTANT):** the repo owner is learning Rust. Every Rust change carries concise `// 🦀` teaching comments on the *language* concept, and each Rust task ends with a 2-3 sentence plain-English recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** — this repo hand-formats.

**Working directory for all commands:** Rust commands run from `src-tauri/`; frontend commands run from the repo root `/Users/makar/dev/ownmail`.

---

## File Structure

| File | Create/Modify | Responsibility |
|---|---|---|
| `src-tauri/Cargo.toml` | Modify | Add `"multipart"` to reqwest's feature list |
| `src-tauri/src/whisper.rs` | **Create** | `WhisperClient` — POST multipart audio → `/inference`, parse `{"text"}`, friendly errors |
| `src-tauri/src/lib.rs` | Modify | `pub mod whisper;` + register `commands::transcribe_recording` |
| `src-tauri/src/commands.rs` | Modify | DB-free `transcribe_recording(path)` command + `mime_for_recording` helper |
| `src-tauri/tests/whisper_test.rs` | **Create** | wiremock tests: happy / refused / non-2xx body / empty |
| `src/lib/notes.ts` | Modify | `transcribeRecording(path)` wrapper |
| `src/lib/mock.ts` | Modify | `mockTranscribeRecording(path)` canned transcript |
| `src/components/NotesModal.tsx` | Modify | `transcribing` state, `handleTranscribe`, the Transcribe… button |
| `src/styles/app.css` | Modify | `.note-transcript-actions` flex row to hold both buttons |

---

## Task 1: WhisperClient (Rust) + wiremock tests

**Files:**
- Modify: `src-tauri/Cargo.toml` (reqwest features)
- Create: `src-tauri/src/whisper.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod whisper;`)
- Test: `src-tauri/tests/whisper_test.rs`

- [ ] **Step 1: Add the `multipart` feature to reqwest**

In `src-tauri/Cargo.toml`, change the existing reqwest line:

```toml
reqwest = { version = "0.12", features = ["json", "multipart"] }
```

(`multipart` is a feature flag on the already-present crate — not a new dependency.)

- [ ] **Step 2: Write the failing integration tests**

Create `src-tauri/tests/whisper_test.rs`:

```rust
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
```

- [ ] **Step 3: Run the tests to verify they fail (won't compile)**

Run: `cd src-tauri && cargo test --test whisper_test`
Expected: FAIL — compile error `unresolved import ember_lib::whisper` (module doesn't exist yet).

- [ ] **Step 4: Create the WhisperClient**

Create `src-tauri/src/whisper.rs`:

```rust
// src-tauri/src/whisper.rs — local Whisper STT client (audio/video → transcript, M23).
// Mirrors OllamaClient: a swappable base_url + a reusable reqwest::Client. Targets whisper.cpp's
// own `whisper-server`: POST /inference, multipart `file` + `response_format=json` → {"text": …}.
// The model is chosen when the SERVER starts, so the request carries no model name.
use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};

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
```

- [ ] **Step 5: Declare the module in lib.rs**

In `src-tauri/src/lib.rs`, after the existing `pub mod transcript;` declaration (around line 48), add:

```rust
// 🦀 Local Whisper STT client (M23) — the audio twin of `ollama`. `pub` so the wiremock
//    integration test in tests/whisper_test.rs (a separate crate) can reach it.
pub mod whisper;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test whisper_test`
Expected: PASS — 4 passed.

- [ ] **Step 7: Lint**

Run: `cd src-tauri && cargo clippy --all-targets 2>&1 | tail -20`
Expected: no warnings on `whisper.rs`.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/whisper.rs src-tauri/src/lib.rs src-tauri/tests/whisper_test.rs
git commit -m "feat(m23): WhisperClient — multipart POST to local whisper-server + wiremock tests"
```

**Rust recap (write this out after the task):** 2-3 sentences on `reqwest::multipart::Form`/`Part`, why `Part::bytes` needs an owned `Vec<u8>` (`'static`), and `is_connect()` vs reading a non-success body.

---

## Task 2: `transcribe_recording` command (Rust)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add command + `mime_for_recording` helper near `read_transcript_file`, ~line 903)
- Modify: `src-tauri/src/lib.rs` (register in `generate_handler!`)

No automated test for this command — it is thin `std::fs` + network I/O that constructs a real `WhisperClient`, exactly like `read_transcript_file` (which also has no Rust unit test). The client logic is covered by Task 1; this command is verified by `cargo build`/`clippy` and the maket in Task 5.

- [ ] **Step 1: Add the command + mime helper**

In `src-tauri/src/commands.rs`, immediately after the `read_transcript_file` function (ends ~line 903, before `#[cfg(test)] mod tests`), add:

```rust
/// Transcribe a user-picked audio/video recording via a local Whisper HTTP server (M23).
/// DB-free; mirrors `read_transcript_file`: a size guard, a Rust `std::fs::read`, then the
/// WhisperClient POSTs the bytes. The path comes from the frontend dialog → no `fs` capability.
#[tauri::command]
pub async fn transcribe_recording(path: String) -> Result<String> {
    // 🦀 Cap the pick before slurping the whole file into memory. Recordings dwarf a text
    //    transcript (audio = tens of MB, video more), so 500 MB rather than the 25 MB text cap.
    const MAX_RECORDING_BYTES: u64 = 500 * 1024 * 1024;
    let len = std::fs::metadata(&path)
        .map_err(|e| AppError::Other(format!("could not read recording file: {e}")))?
        .len();
    if len > MAX_RECORDING_BYTES {
        return Err(AppError::Other(format!(
            "recording file is too large ({} MB max).",
            MAX_RECORDING_BYTES / (1024 * 1024)
        )));
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| AppError::Other(format!("could not read recording file: {e}")))?;
    // 🦀 `Path::file_name`/`extension` return `Option<&OsStr>`; `.and_then(|s| s.to_str())` turns
    //    that into an `Option<&str>` we can fall back from with `unwrap_or`.
    let p = std::path::Path::new(&path);
    let filename = p.file_name().and_then(|s| s.to_str()).unwrap_or("recording");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let mime = mime_for_recording(&ext);
    crate::whisper::WhisperClient::new().transcribe(bytes, filename, mime).await
}

// 🦀 Best-effort content type from the file extension. The whisper server decodes by content
//    regardless, so this is just polite metadata on the multipart part. `&'static str` because
//    every arm returns a string literal baked into the binary.
fn mime_for_recording(ext: &str) -> &'static str {
    match ext {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" => "audio/mp4",
        "mov" => "video/quicktime",
        "webm" => "audio/webm",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]`, add a line right after `commands::read_transcript_file,`:

```rust
            commands::read_transcript_file,
            commands::transcribe_recording,
```

- [ ] **Step 3: Build + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -15 && cargo clippy --all-targets 2>&1 | tail -15`
Expected: builds clean; no clippy warnings on the new code.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(m23): transcribe_recording command — std::fs read + WhisperClient call"
```

**Rust recap (write this out after the task):** 2-3 sentences on `std::path::Path::file_name`/`extension` returning `Option<&OsStr>`, the `.and_then(|s| s.to_str())` chain, and why the helper returns `&'static str`.

---

## Task 3: Frontend wrapper + mock

**Files:**
- Modify: `src/lib/notes.ts` (add `transcribeRecording`)
- Modify: `src/lib/mock.ts` (add `mockTranscribeRecording`)

- [ ] **Step 1: Add the mock**

In `src/lib/mock.ts`, immediately after `mockReadTranscriptFile` (ends ~line 284), add:

```ts
// M23: a canned Whisper transcription, distinct from mockReadTranscriptFile so the two buttons
// are visibly different in the maket.
export function mockTranscribeRecording(_path: string): string {
  return "Dana: Thanks for joining the call.\nYou: Let's start with the budget review.\nDana: Action — send the revised figures by Wednesday.";
}
```

- [ ] **Step 2: Add the wrapper**

In `src/lib/notes.ts`, add `mockTranscribeRecording` to the existing import from `./mock`:

```ts
import {
  mockGetMeetingNote,
  mockSaveMeetingNote,
  mockDeleteMeetingNote,
  mockListMeetingNotes,
  mockSummarizeMeetingNote,
  mockReadTranscriptFile,
  mockTranscribeRecording,
} from "./mock";
```

Then, immediately after the `readTranscriptFile` export (end of file), add:

```ts
export const transcribeRecording = (path: string): Promise<string> =>
  isTauri()
    ? invoke<string>("transcribe_recording", { path })
    : Promise.resolve(mockTranscribeRecording(path));
```

- [ ] **Step 3: Typecheck**

Run: `npm run build 2>&1 | tail -15`
Expected: build succeeds (TypeScript compiles, no unused-import errors).

- [ ] **Step 4: Commit**

```bash
git add src/lib/notes.ts src/lib/mock.ts
git commit -m "feat(m23): transcribeRecording wrapper + mockTranscribeRecording"
```

---

## Task 4: NotesModal — Transcribe… button

**Files:**
- Modify: `src/components/NotesModal.tsx` (import, `transcribing` state, `blocked`, `handleTranscribe`, button)
- Modify: `src/styles/app.css` (`.note-transcript-actions`)

- [ ] **Step 1: Import the wrapper**

In `src/components/NotesModal.tsx`, add `transcribeRecording` to the existing import from `../lib/notes`:

```ts
import {
  getMeetingNote,
  saveMeetingNote,
  deleteMeetingNote,
  summarizeMeetingNote,
  readTranscriptFile,
  transcribeRecording,
} from "../lib/notes";
```

- [ ] **Step 2: Add the `transcribing` state**

After the `importing` state (~line 41), add:

```tsx
  const [importing, setImporting] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
```

- [ ] **Step 3: Include it in `blocked`**

Change the `blocked` line (~line 169) from:

```tsx
  const blocked = busy || summarizing || importing;
```

to:

```tsx
  const blocked = busy || summarizing || importing || transcribing;
```

- [ ] **Step 4: Add the `handleTranscribe` handler**

Immediately after the `handleImport` function (ends ~line 143), add:

```tsx
  async function handleTranscribe() {
    setTranscribing(true);
    setError(null);
    try {
      let path: string | null;
      if (isTauri()) {
        const sel = await open({
          filters: [
            { name: "Recording", extensions: ["wav", "mp3", "m4a", "mp4", "mov", "webm", "ogg", "flac", "aac"] },
          ],
        });
        path = typeof sel === "string" ? sel : null; // null if cancelled (or a multi-array)
      } else {
        path = "/mock/recording.m4a"; // maket: skip the native dialog
      }
      if (!path) return; // cancelled
      const text = await transcribeRecording(path);
      setTranscript(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setTranscribing(false);
    }
  }
```

- [ ] **Step 5: Add the button (wrap both buttons in an actions row)**

Replace the existing `.note-transcript-head` block (~lines 193-198):

```tsx
            <div className="note-transcript-head">
              <span>Transcript</span>
              <button className="btn" onClick={handleImport} disabled={blocked}>
                {importing ? "Importing…" : "Import…"}
              </button>
            </div>
```

with:

```tsx
            <div className="note-transcript-head">
              <span>Transcript</span>
              <div className="note-transcript-actions">
                <button className="btn" onClick={handleImport} disabled={blocked}>
                  {importing ? "Importing…" : "Import…"}
                </button>
                <button className="btn" onClick={handleTranscribe} disabled={blocked}>
                  {transcribing ? "Transcribing…" : "Transcribe…"}
                </button>
              </div>
            </div>
```

- [ ] **Step 6: Add the CSS for the button row**

In `src/styles/app.css`, immediately after the `.note-transcript-head { … }` rule (ends ~line 493), add:

```css
.note-transcript-actions {
  display: flex;
  gap: 6px;
}
```

- [ ] **Step 7: Typecheck/build**

Run: `npm run build 2>&1 | tail -15`
Expected: build succeeds.

- [ ] **Step 8: Commit**

```bash
git add src/components/NotesModal.tsx src/styles/app.css
git commit -m "feat(m23): NotesModal Transcribe… button → fills the transcript field"
```

---

## Task 5: Full gates + maket verification

**Files:** none (verification only)

- [ ] **Step 1: Rust gates**

Run: `cd src-tauri && cargo test 2>&1 | tail -25 && cargo clippy --all-targets 2>&1 | tail -15`
Expected: all tests pass (including the 4 new `whisper_test` cases); no clippy warnings.

- [ ] **Step 2: Frontend gate**

Run: `npm run build 2>&1 | tail -15`
Expected: clean build.

- [ ] **Step 3: Maket verification (browser mock)**

Start the dev server (`npm run dev`) and, via the preview/chrome-devtools tools, open a calendar event's note (or the Notes drawer), confirm the Transcript section now shows **Import…** *and* **Transcribe…**, click **Transcribe…**, and verify the textarea fills with the canned `mockTranscribeRecording` text (distinct from the Import sample). Confirm no errors in the console. Screenshot for the record.

- [ ] **Step 4: Confirm `git status -s` is clean** (no stray files from any reviewer).

- [ ] **Step 5: Final commit if anything was adjusted** (otherwise nothing to do).

---

## Live E2E (owner-pending, not a code task)

Like the M21 live-Ollama leg, the live-Whisper path is owner-verified, not CI-gated. To exercise it manually:

1. `brew install whisper-cpp` (provides `whisper-server` + `whisper-cli`).
2. Download a ggml model, e.g. `ggml-base.en.bin`.
3. `whisper-server -m /path/to/ggml-base.en.bin --port 8080`.
4. In Ember (real Tauri build), open a note → **Transcribe…** → pick a short **16 kHz WAV** (universally supported; other formats need an ffmpeg-enabled build) → the transcript fills → **Summarize** (needs Ollama running per M21).

---

## Self-Review notes (already applied)

- **Spec coverage:** WhisperClient (Task 1) ✓; `/inference` multipart + `{"text"}` + 600s timeout + friendly/refused/non-2xx-body/empty errors (Task 1 tests) ✓; `transcribe_recording` DB-free with 500 MB cap + mime map (Task 2) ✓; reqwest `multipart` feature (Task 1) ✓; `transcribeRecording` wrapper + mock (Task 3) ✓; NotesModal Transcribe… button filling the transcript field (Task 4) ✓; no migration/Settings/new capability ✓; maket + owner-pending E2E (Task 5 / Live E2E) ✓.
- **Type consistency:** `transcribe(audio: Vec<u8>, filename: &str, mime: &str)` used identically in Task 1 (impl + tests) and Task 2 (caller); `transcribeRecording(path)` matches the Rust command name `transcribe_recording` and the `{ path }` invoke arg.
- **No placeholders:** every code step shows complete code; commands include working-directory context.
