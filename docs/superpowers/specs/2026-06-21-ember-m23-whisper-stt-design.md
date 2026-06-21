# Ember — Milestone 23: Local Whisper STT of a recording file — Design Spec

**Status:** Approved design (2026-06-21). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Let the user pick an **audio/video recording file**, run **local speech-to-text** against a local Whisper HTTP server, and drop the resulting text into a note's existing **transcript** field — which then feeds the unchanged M21/M22 summarize spine. This is the **second of three sub-milestones** the "meeting transcription capture" step was decomposed into (M22 transcript import → **M23** local Whisper file STT → M24 live macOS meeting-audio capture). M23 de-risks Whisper **without** touching OS audio capture: a file already exists on disk; we only add the STT step in front of the transcript field.

**Architecture in one paragraph:** A new **`WhisperClient`** (`src-tauri/src/whisper.rs`) is the audio twin of the M21 `OllamaClient` — a swappable `base_url` + a reusable `reqwest::Client`, `new()`/`Default`/`with_base_url(...)` (test-only), and friendly mappings of the two common local-setup failures. It targets **whisper.cpp's own `whisper-server`**: `POST {base}/inference`, `multipart/form-data` with a `file` part + a `response_format=json` text part, returning `{"text": "..."}`. The model is chosen when the *server* starts, so — unlike Ollama — the request sends **no** model name. A DB-free command **`transcribe_recording(path) -> Result<String>`** mirrors `read_transcript_file`: a `std::fs::metadata` size guard, `std::fs::read` the bytes, `WhisperClient::new().transcribe(bytes, filename, mime).await`, return the trimmed text. The frontend `NotesModal` gains a **Transcribe…** button beside the M22 **Import…** button; it opens a `tauri-plugin-dialog` `open()` filtered to audio/video, calls `transcribeRecording(path)`, and **fills the transcript textarea** (replace, exactly like Import) — the user then reviews/edits and clicks Summarize. Local-only throughout; no transcoding, no model bundled, no second binary.

**Tech Stack:** Rust (reqwest **+ the `multipart` feature flag** — not a new crate; serde, Tauri 2, `tauri-plugin-dialog` — all already present), React 19 + TypeScript + Vite. **No new crate, no new plugin, no new OAuth scope, no new capability, no migration, no Settings.**

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept (here: `reqwest::multipart` form construction, `reqwest::Error::is_connect()` for connection-vs-other errors, reading a response body on a non-success status, owned `Vec<u8>` for a `'static` multipart `Part`, mirroring an existing client struct). After each Rust task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M22 are merged to `main`. Ember has local meeting notes (M20), local-Ollama summarization (M21: an `OllamaClient` posting to `/api/generate`, a `summarize_meeting_note` command using the lock-drop-before-`await`-relock discipline, a NotesModal Summarize/Regenerate button + summary panel + staleness hint), and a first-class **transcript** field with paste/import (M22: `meeting_notes.transcript`, the pure `transcript.rs` helpers `vtt_to_text` + `build_summary_input`, a DB-free `read_transcript_file(path)` command, and a NotesModal Transcript section with an **Import…** button). M22's spec explicitly named M23 as "local Whisper STT of an audio/video file… writes this **same** `transcript` field." M23 delivers exactly that.

**Reuse map:**
- The M21 **`OllamaClient` shape** (`src-tauri/src/ollama.rs`): swappable `base_url`, `build_http()` with a generous timeout, `new()`/`Default`/`with_base_url`, `is_connect()` → friendly "isn't running" message — `WhisperClient` mirrors it part-for-part.
- The M21 **wiremock test pattern** (`tests/ollama_test.rs`, a separate crate reaching `ember_lib::…`): happy path, connection-refused, empty-response.
- The M22 **`read_transcript_file` command shape**: DB-free, `std::fs::metadata` size cap → read → return trimmed text; **no `fs` capability** because the path comes from the frontend `open()` dialog and the byte read happens in Rust.
- The M22 **NotesModal Transcript section** (`handleImport`, the `open()` dialog, the `importing` busy flag, the `blocked` interlock) and the **`isTauri()` mock seam** (`lib/notes.ts` + `lib/mock.ts`, `mockReadTranscriptFile`).
- The M17 **path-from-dialog / bytes-in-Rust** file pattern (`download_attachment`, `send_email`'s attachment reads use `std::fs`).

---

## Scope

**In scope (lean v1):**
- New `src-tauri/src/whisper.rs`: a `WhisperClient` posting multipart audio to whisper.cpp's `whisper-server` `POST /inference`, parsing `{"text": …}`, with friendly setup errors.
- New DB-free command `transcribe_recording(path: String) -> Result<String>` (size guard → `std::fs::read` → `WhisperClient::transcribe` → trimmed text), registered in `lib.rs`.
- reqwest gains the **`multipart`** feature in `Cargo.toml`.
- Frontend: `transcribeRecording(path)` wrapper in `lib/notes.ts` (+ `mockTranscribeRecording` in `lib/mock.ts`); a **Transcribe…** button + `transcribing` busy state in `NotesModal`, filling the existing transcript textarea.

**Explicitly deferred (later sub-milestone / not M23):**
- **M24** — live macOS meeting-audio capture (ScreenCaptureKit or a BlackHole virtual device → a file/stream → Whisper). The hard/uncertain capstone; comes last.
- **Transcoding / audio decoding in-app** — we pass bytes through; the whisper server decodes. WAV (16 kHz) is universally accepted; other formats depend on the user's whisper build. No ffmpeg/symphonia/resampler in Ember.
- **Bundling a Whisper binary or model** — rejected in favor of the Ollama-style "user runs a local service" pattern (the approved architecture decision).
- **Settings/env for the Whisper URL** — hardcoded `http://localhost:8080` + `with_base_url` for tests only, exactly as M21 hardcoded Ollama.
- **Auto-save / auto-summarize after transcribing** — Transcribe only fills the field (like Import); the user controls Save/Summarize.
- **Diarization / speaker labels / timestamps / streaming progress / partial results** — plain text only; a single blocking call with a "Transcribing…" button state.

---

## Components

### Backend — new module `src-tauri/src/whisper.rs` (declared `pub mod whisper;` in `lib.rs`)
Mirrors `ollama.rs`:
- `const DEFAULT_BASE: &str = "http://localhost:8080";` (whisper.cpp `whisper-server` default).
- `pub struct WhisperClient { base_url: String, http: reqwest::Client }` + `impl Default` delegating to `new()`.
- `pub fn new() -> Self` (DEFAULT_BASE + `build_http()`); `pub fn with_base_url(base_url: String) -> Self` (test-only, points at wiremock).
- `fn build_http() -> reqwest::Client`: a **600s** timeout (CPU transcription of a long recording is slow; Ollama's 120s would truncate real meetings).
- `pub async fn transcribe(&self, audio: Vec<u8>, filename: &str, mime: &str) -> Result<String>`:
  - `let url = format!("{}/inference", self.base_url);`
  - Build a `reqwest::multipart::Form`: a `file` part = `multipart::Part::bytes(audio).file_name(filename.to_string()).mime_str(mime)?`; plus a text part `("response_format", "json")`. (`Part::bytes` needs `'static` data → an owned `Vec<u8>` is correct.)
  - `self.http.post(&url).multipart(form).send().await.map_err(|e| if e.is_connect() { AppError::Other("Whisper server isn't running at {base} — install whisper.cpp (e.g. `brew install whisper-cpp`) and run `whisper-server -m <model> --port 8080`.") } else { AppError::Http(e) })?`
  - On a non-success status: read the body text and return `AppError::Other(...)` including it (so a server complaint like "failed to decode audio" reaches the user) — i.e. **don't** just `error_for_status()?`, because the body carries the useful message.
  - Parse JSON into `struct InferenceResponse { text: String }`; `let text = parsed.text.trim().to_string();` empty → `AppError::Other("Whisper returned an empty transcript.")`; else `Ok(text)`.

### Backend — command (`src-tauri/src/commands.rs`, registered in `lib.rs`)
- New `transcribe_recording(path: String) -> Result<String>` (DB-free), mirroring `read_transcript_file`:
  - `const MAX_RECORDING_BYTES: u64 = 500 * 1024 * 1024;` — recordings dwarf the 25 MB text cap; `std::fs::metadata(&path)?.len()` guard → "recording file is too large (500 MB max)." (Deliberate in-memory `std::fs::read`, consistent with `download_attachment`/`send_email`; streaming is a noted future optimization, not v1.)
  - `let bytes = std::fs::read(&path).map_err(|e| AppError::Other(format!("could not read recording file: {e}")))?;`
  - Derive `filename` (basename of the path) and `mime` (a tiny extension→type map: `wav`→`audio/wav`, `mp3`→`audio/mpeg`, `m4a`/`mp4`→`audio/mp4`, `mov`→`video/quicktime`, `webm`→`audio/webm`, `ogg`→`audio/ogg`, `flac`→`audio/flac`, `aac`→`audio/aac`, else `application/octet-stream`). The server decodes by content regardless; the mime is best-effort metadata.
  - `crate::whisper::WhisperClient::new().transcribe(bytes, &filename, mime).await` → return (already trimmed by the client).
- Registered in `lib.rs`'s `generate_handler![ … , commands::transcribe_recording ]`.

### Frontend — `src/lib/notes.ts`
- `transcribeRecording(path: string): Promise<string>` → `isTauri() ? invoke("transcribe_recording", { path }) : mockTranscribeRecording(path)`. Placed beside `readTranscriptFile` (both are transcript-source helpers).

### Frontend — `src/lib/mock.ts`
- `mockTranscribeRecording(path: string)` returns a canned multi-line transcript (distinct from the `mockReadTranscriptFile` sample, so the two buttons are visibly different in the maket), letting the Transcribe flow demo offline.

### Frontend — `src/components/NotesModal.tsx`
- Import `transcribeRecording`.
- State += `transcribing` (busy flag); add it to `blocked` (`busy || summarizing || importing || transcribing`).
- In the `.note-transcript-head` row, add a **Transcribe…** button next to **Import…** (label → "Transcribing…" while busy; disabled while `blocked`).
- `handleTranscribe` mirrors `handleImport`: in Tauri, `open({ filters:[{ name:"Recording", extensions:["wav","mp3","m4a","mp4","mov","webm","ogg","flac","aac"] }] })` → if a non-array path was picked, `transcribeRecording(path)` → `setTranscript(text)`; in the maket (`!isTauri()`), skip the dialog and call `transcribeRecording("/mock/recording.m4a")`; errors → inline modal error; the `transcribing` spinner gates the buttons.
- It only fills the transcript field. Save and Summarize stay user-driven and unchanged (an imported/transcribed transcript bumps `updated_at` on Save → marks any prior summary stale, the M22 contract).

### Frontend — `src/styles/app.css`
- No new layout class required — the existing `.note-transcript-head` flex row holds both buttons. (Add a small gap/utility only if the two buttons need spacing; reuse existing `.btn`.)

### Data flow
`Open note → load body + transcript + summary (M22, unchanged)`. `Transcribe → open() dialog (audio/video) → transcribe_recording(path) → std::fs::read bytes → WhisperClient POST /inference (multipart) → {"text"} → fill transcript textarea (editable)`. `Save → persists body+transcript, bumps updated_at (M22)`. `Summarize → build_summary_input(body, transcript) → Ollama (M21, unchanged)`.

---

## Error handling

- **Whisper not running** (`is_connect()`) → `AppError::Other("Whisper server isn't running at … — install whisper.cpp … and run `whisper-server …`.")` → inline modal error. (The Ollama-style actionable hint.)
- **Non-success HTTP status** (e.g. the server can't decode the format, or no model loaded) → `AppError::Other` carrying the server's response body → inline. Honest passthrough; we don't pretend to know why.
- **Empty transcription** → `AppError::Other("Whisper returned an empty transcript.")`.
- **File too large / unreadable** → `AppError::Other("recording file is too large (500 MB max).")` / `"could not read recording file: …"`.
- **`open()` cancelled** (no path / array) → no-op (no error), same as M22 Import.
- **Other reqwest errors** → `AppError::Http(e)` (existing variant).

---

## Testing

- **Rust — new `tests/whisper_test.rs`** (wiremock, separate crate reaching `ember_lib::whisper::WhisperClient`, mirroring `tests/ollama_test.rs`):
  1. **Happy path:** mock `POST /inference` → `200 {"text":"hello from whisper"}`; `with_base_url(mock).transcribe(bytes, "a.wav", "audio/wav")` → `"hello from whisper"`. Assert the request hit `/inference` (wiremock `expect`).
  2. **Connection refused:** `with_base_url("http://127.0.0.1:<unused-port>").transcribe(...)` → `Err` whose message contains "isn't running".
  3. **Non-success body passthrough:** mock → `400` with a body like `"failed to decode audio"`; assert the `Err` message contains that body text.
  4. **Empty transcript:** mock → `200 {"text":"   "}` → `Err` containing "empty".
- **Rust — no DB tests** (M23 touches no DB; `transcribe_recording` is DB-free I/O, same as `read_transcript_file`, which had no dedicated Rust unit test).
- **Frontend:** no TS test harness (consistent through M22). Maket-verified by screenshot: the Transcript section now shows **Import…** *and* **Transcribe…**; clicking Transcribe fills the textarea with the canned mock transcript; Summarize still produces a summary.
- **Gates:** `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean. Genuinely testable locally with a real `whisper-server` running + a short WAV; the **live-Whisper leg stays owner-pending** as the Ollama leg did in M21.

---

## Known risks & decisions

- **Local HTTP Whisper server, not a bundled sidecar/model (approved decision).** Mirrors the M21 Ollama pattern the codebase already embraces: tiny client, fully wiremock-testable, zero model/binary bundled, no `tauri-plugin-shell`, no code-signing/notarization of a shipped binary. Cost: the user runs a second local service — acceptable and consistent (they already run Ollama).
- **We do not own transcoding.** Bytes pass through; the server decodes. WAV (16 kHz mono) is universally supported by whisper.cpp; compressed/video formats depend on whether the user's `whisper-server` was built with ffmpeg. Unsupported-format failures surface as the server's own error body — honest, and keeps ffmpeg/symphonia out of Ember entirely.
- **Targets whisper.cpp's native `/inference`** (multipart `file` + `response_format=json` → `{"text"}`), not the OpenAI-compatible `/v1/audio/transcriptions`. Chosen because it pairs with a one-line `brew install whisper-cpp` on macOS and is the canonical whisper.cpp server. The model is set at server-start, so our request carries no model name (simpler than Ollama).
- **Hardcoded URL + 600s timeout, no Settings/env** — same discipline as M21. `with_base_url` exists only for wiremock. A user-configurable endpoint is a future milestone if needed.
- **500 MB in-memory read.** A recording is far bigger than a text transcript; we still use the simple `std::fs::read` (consistent with existing file commands) and cap at 500 MB. Streaming the multipart body from disk is a noted future optimization, not v1.
- **Transcribe fills, never auto-saves/summarizes** — identical UX to M22 Import, so the user can edit raw STT output before it becomes summarizer input. A transcribed transcript marks a prior summary stale only after Save (the M22 `updated_at` contract).
- **No `fs` capability** — the path comes from the dialog, the byte read is Rust `std::fs` (the M17/M22 pattern).

---

## Non-goals / constraints

- **No live audio capture in M23** — that is M24 (ScreenCaptureKit / BlackHole). M23 transcribes a file that already exists on disk.
- **No transcoding, no in-app audio decoding, no bundled model or binary.**
- **No new crate, no new plugin, no new OAuth scope, no new capability, no migration, no Settings.** The only build change is adding the `multipart` feature to reqwest.
- **Plain-text transcript** — no speaker/timestamp UI, no streaming/progress, no diarization.
- **Tauri build unchanged for the maket** — `transcribeRecording` is `isTauri()`-gated; the Transcribe button uses a mock path offline; the rest of the Transcript/Summary UI is M22, untouched.
