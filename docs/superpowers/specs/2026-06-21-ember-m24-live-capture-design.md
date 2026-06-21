# Ember — Milestone 24: Live macOS system-audio capture → streaming transcript — Design Spec

**Status:** Approved design (2026-06-21). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Capture **live macOS audio** during a meeting and stream a **growing transcript** into a note in real time. The user picks an input device, hits **Record**, and the transcript textarea fills incrementally as they talk; **Stop** finalizes it. "System audio" (the far-end voices from Meet/Zoom) is captured by selecting the **BlackHole** virtual device. This is the **third and final sub-milestone** of the decomposed "meeting transcription capture" feature (M22 transcript import → M23 local Whisper file STT → **M24** live capture — the hard/risky capstone, deliberately built last).

**Why this shape (the two approved decisions):** (1) **Capture mechanism = `cpal` + a user-selected input device (BlackHole for system audio)**, chosen over ScreenCaptureKit and Core Audio process taps. `cpal` is mature pure-Rust CoreAudio capture; the code is **device-agnostic** (we capture whatever device the user selects), so "system audio" is just *"select BlackHole"*. This needs only the **microphone** TCC permission (far more stable for an **unsigned dev build** than ScreenCaptureKit's "Screen & System Audio Recording" permission, which breaks on every ad-hoc rebuild), **no Swift helper**, **no entitlements**, **no codesigning**, **no `tauri-plugin-shell`**. It mirrors the project's "user runs/install local infra" ethos (they already install + run Ollama and whisper-server). (2) **Scope = live streaming transcript** (not record-then-transcribe-once): windowed chunks transcribed and appended as the meeting runs.

**Architecture in one paragraph:** A `cpal` input stream runs on a **dedicated thread** (CoreAudio `Stream` is `!Send`); its realtime callback does only a cheap non-blocking `send` of f32 frames into a `tokio::sync::mpsc::unbounded_channel`. An **async worker** accumulates **non-overlapping ~10 s windows**, then for each window: **downmix to mono → `rubato` resample to 16 kHz → hand-rolled PCM16 WAV → REUSE the M23 `WhisperClient.transcribe(bytes, "chunk.wav", "audio/wav")`** → emit the resulting text over a **Tauri `Channel<CaptureEvent>`**. The frontend appends each chunk to the note's existing **transcript** textarea live. **Stop** signals the audio thread to drop the stream, the worker flushes the final partial window, transcribes it, emits a final `Stopped` event. New pure helpers live in `src-tauri/src/audio.rs` (downmix/resample/WAV — unit-tested); the capture session + three commands (`list_input_devices`, `start_capture`, `stop_capture`) live in `src-tauri/src/capture.rs`. The captured transcript then flows through the **unchanged** M21/M22 Save → Summarize spine.

**Tech Stack:** Rust (NEW deps **`cpal`** + **`rubato`**; WAV encoded by hand — no `hound`; reuses `reqwest`/`serde`/Tauri 2 + the M23 `WhisperClient`), React 19 + TypeScript + Vite, the Tauri 2 **`Channel`** IPC-streaming primitive. macOS: a new **`Info.plist`** entry `NSMicrophoneUsageDescription`. **No new OAuth scope, no new Tauri capability, no migration, no Settings, no `tauri-plugin-shell`.**

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language/runtime* concept (here: a `!Send` value pinned to its own thread, the realtime-audio callback constraint, `tokio::sync::mpsc::unbounded_channel` send-from-sync-thread, `AtomicBool` stop-signaling, the in-memory WAV/RIFF byte layout, `rubato` resampling, the Tauri `Channel`). After each Rust task, give a short plain-English recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (M23 saw a reviewer leave a rogue `.gitignore` edit — caught and reverted; do not let it recur).

---

## Milestone context

M1–M23 are merged to `main`. The meeting-notes feature area is built out: local notes (M20), local-Ollama summarization (M21), a first-class **transcript** field with paste/import (M22), and **local Whisper file STT** (M23 — a `WhisperClient` POSTing a WAV/bytes to a local `whisper-server` `/inference`, a `transcribe_recording(path)` command, and a NotesModal **Transcribe…** button that fills the transcript field). M22 and M23 both explicitly named M24 as the live-capture capstone. M24 reuses the M23 transcription spine wholesale and only adds the **capture front-end** that produces audio for it.

**Reuse map:**
- The M23 **`WhisperClient`** (`src-tauri/src/whisper.rs`): `transcribe(audio: Vec<u8>, filename: &str, mime: &str) -> Result<String>` — M24 feeds it one 16 kHz mono WAV per window. **Unchanged.**
- The M22 **transcript field + Save/Summarize spine**: captured text lands in the same `NotesModal` `transcript` state → `saveMeetingNote` → `summarize_meeting_note`. **Unchanged.**
- The **`isTauri()` mock seam** (`lib/notes.ts` + `lib/mock.ts`) and the NotesModal modal/busy-flag patterns.
- The `error::AppError`/`Result` types; the `pub mod` + managed-state registration pattern in `lib.rs` (`app.manage(...)`, `tauri::State<'_, T>`).

---

## Scope

**In scope (lean v1):**
- New pure `src-tauri/src/audio.rs`: `downmix_to_mono`, `resample_to_16k` (rubato), `encode_wav_pcm16_16k_mono` (hand-rolled), `f32_to_i16` — all unit-tested.
- New `src-tauri/src/capture.rs`: the `cpal` capture session (dedicated thread + async worker + windowing) and three commands — `list_input_devices()`, `start_capture(deviceName, onEvent: Channel<CaptureEvent>)`, `stop_capture()`.
- Managed capture state in `lib.rs` (`Arc<Mutex<Option<CaptureSession>>>`); the three commands registered.
- New deps `cpal` + `rubato` in `Cargo.toml`; `NSMicrophoneUsageDescription` in `src-tauri/Info.plist` (+ `Info.dev.plist` if the dev-bundle needs it).
- Frontend: `lib/notes.ts` wrappers (`listInputDevices`, `startCapture(deviceName, onEvent)`, `stopCapture`) + mock equivalents; `NotesModal` gains a capture row (device `<select>` + Record/Stop + "listening…" indicator) that appends chunks to the transcript textarea live.

**Explicitly deferred (not M24):**
- **Overlap/dedup at window boundaries** — v1 uses non-overlapping windows; a split word at a boundary is acceptable (editable text → summarizer). VAD/silence-based segmentation deferred.
- **Speaker diarization / labels / timestamps; partial-word "interim" results within a window.**
- **ScreenCaptureKit / Core Audio process-tap capture** (the no-BlackHole-install paths) — deferred to a future signed/notarized release where their TCC story is viable.
- **Bundling/auto-installing BlackHole** — documented manual one-time setup, like running Ollama/whisper-server.
- **A configurable window length / whisper URL via Settings** — hardcoded (~10 s window; the M23 hardcoded whisper endpoint).
- **Unbounded-lag mitigation** — if CPU whisper runs slower than realtime the transcript lags (memory grows); acceptable for v1, noted as a known limitation.

---

## Components

### Backend — new pure module `src-tauri/src/audio.rs` (`pub mod audio;` in `lib.rs`)
All functions are pure (no I/O), fully unit-testable:
- `pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32>` — average the channels per frame (1 channel → passthrough).
- `pub fn resample_to_16k(mono: &[f32], in_rate: u32) -> Vec<f32>` — `rubato` resample from `in_rate` to 16000 (passthrough if already 16k). Handles non-integer ratios (44.1 kHz).
- `pub fn f32_to_i16(samples: &[f32]) -> Vec<i16>` — clamp to [-1.0, 1.0], scale by 32767.
- `pub fn encode_wav_pcm16_16k_mono(samples: &[i16]) -> Vec<u8>` — hand-rolled 44-byte RIFF/WAVE header (PCM, 1 channel, 16000 Hz, 16-bit) + little-endian sample bytes. `// 🦀` documents each header field.
- A convenience `pub fn window_to_wav(interleaved: &[f32], channels: u16, in_rate: u32) -> Vec<u8>` chaining the four (the worker calls this).

### Backend — new module `src-tauri/src/capture.rs` (`pub mod capture;` in `lib.rs`)
- Types: `#[derive(Serialize)] struct DeviceInfo { name: String }`; `#[derive(Serialize)] #[serde(tag = "type")] enum CaptureEvent { Chunk { text: String }, Error { message: String }, Stopped }`.
- `CaptureSession`: holds `stop: Arc<AtomicBool>`, the audio-thread `JoinHandle<()>`, and the worker `tokio::task::JoinHandle<()>`.
- **Command `list_input_devices() -> Result<Vec<DeviceInfo>>`**: enumerate `cpal::default_host().input_devices()`, collect names. DB-free, no state.
- **Command `start_capture(device_name: String, on_event: Channel<CaptureEvent>, state: State<CaptureState>) -> Result<()>`**:
  - If a session is already active → `AppError::Other("already capturing")`.
  - Find the input device by name; read its default input `StreamConfig` (sample rate + channels).
  - Spawn the **audio thread**: build the `cpal` input stream whose callback clones a `tokio::sync::mpsc::UnboundedSender<Vec<f32>>` and `send`s each buffer (non-blocking; realtime-safe); `stream.play()`; then loop-park checking the `stop` flag (short sleep), dropping the stream on stop. (`// 🦀` the stream is `!Send`, so it never leaves this thread.)
  - Spawn the **async worker** (tokio task): owns the `UnboundedReceiver` + the `Channel` + the device config + a `WhisperClient::new()`. Accumulate frames; when `frames >= in_rate * WINDOW_SECS` (const `WINDOW_SECS = 10`), take the window → `audio::window_to_wav` → `whisper.transcribe(...)` → on Ok non-empty, `on_event.send(Chunk{text})`; on Err, `on_event.send(Error{message})` and **continue**. When the channel closes (sender dropped on stop), process the remaining buffer as a final window, then `on_event.send(Stopped)`.
  - Store the `CaptureSession` in `state`.
- **Command `stop_capture(state) -> Result<()>`**: set `stop = true`; the audio thread drops the stream (closing the channel), the worker flushes + emits `Stopped` + exits; join both handles; clear the state. Idempotent (no session → Ok).

### Backend — `src-tauri/src/lib.rs`
- `pub mod audio; pub mod capture;`
- `app.manage(CaptureState::default())` in `setup` (type alias `CaptureState = Arc<Mutex<Option<capture::CaptureSession>>>`).
- Register `capture::list_input_devices`, `capture::start_capture`, `capture::stop_capture` in `generate_handler!`.

### Backend — `src-tauri/Cargo.toml` + macOS plist
- Add `cpal = "0.16"` (or current) and `rubato = "0.16"` (or current) to `[dependencies]`.
- New `src-tauri/Info.plist` with `NSMicrophoneUsageDescription = "Ember records meeting audio to transcribe it locally."` (and `Info.dev.plist` if `cargo tauri dev` needs the prompt). Tauri merges it at bundle time.

### Frontend — `src/lib/notes.ts`
- `interface DeviceInfo { name: string }` and `type CaptureEvent = { type: "Chunk"; text: string } | { type: "Error"; message: string } | { type: "Stopped" }`.
- `listInputDevices(): Promise<DeviceInfo[]>` — `isTauri() ? invoke("list_input_devices") : mockListInputDevices()`.
- `startCapture(deviceName: string, onEvent: (e: CaptureEvent) => void): Promise<void>` — Tauri: `const ch = new Channel<CaptureEvent>(); ch.onmessage = onEvent; await invoke("start_capture", { deviceName, onEvent: ch })`. Mock: `mockStartCapture(deviceName, onEvent)` (drives `onEvent` via timers).
- `stopCapture(): Promise<void>` — `isTauri() ? invoke("stop_capture") : mockStopCapture()`.

### Frontend — `src/lib/mock.ts`
- `mockListInputDevices()` → `[{ name: "MacBook Pro Microphone" }, { name: "BlackHole 2ch" }]`.
- `mockStartCapture(deviceName, onEvent)` → a `setInterval` that emits 3–4 canned `Chunk` events (e.g. "[live] Dana: Let's get started.") a second apart, so the live-append UI demos offline. `mockStopCapture()` clears the interval and emits `{ type: "Stopped" }`.

### Frontend — `src/components/NotesModal.tsx`
- State: `devices: DeviceInfo[]`, `selectedDevice: string`, `recording: boolean`. Load devices when the modal opens (or on first Record).
- A **capture row** in the Transcript section (near Import…/Transcribe…): a device `<select>`, a **Record/Stop** toggle button (`recording ? "Stop" : "Record"`), and a "listening…" pulse while recording.
- `handleRecord`: `setRecording(true)`; `startCapture(selectedDevice, (e) => { if e.type==="Chunk" append e.text to transcript (with a separating newline); if "Error" setError; if "Stopped" setRecording(false) })`. Appends to the existing `transcript` state so the textarea fills live.
- `handleStop`: `await stopCapture()` (the `Stopped` event flips `recording` off).
- `blocked` includes `recording`; Save/Summarize/Import/Transcribe disabled while recording. After Stop, the transcript behaves exactly as M22/M23 (editable, Save bumps `updated_at` → marks the summary stale, Summarize reuses the combined input).

### Frontend — `src/styles/app.css`
- A `.note-capture-row` (flex: device select + Record/Stop + a small pulse dot). Reuse existing `.btn`.

### Data flow
`Open note → load body/transcript/summary (M22)`. `Pick device → Record → start_capture → cpal thread → frames → worker windows (10 s) → downmix→16 kHz→WAV → M23 WhisperClient → CaptureEvent::Chunk over Channel → append to transcript textarea LIVE`. `Stop → stop_capture → flush final window → Chunk + Stopped → recording off`. `Save → persists body+transcript (M22)`. `Summarize → build_summary_input → Ollama (M21)`.

---

## Error handling

- **No input devices / device won't open** → `AppError::Other` → inline modal error.
- **Microphone permission denied** → the cpal stream build fails → `AppError::Other("Microphone access denied — enable it in System Settings → Privacy & Security → Microphone.")`.
- **A window's whisper call fails** (server down / decode error) → emit `CaptureEvent::Error{message}` for that window and **keep capturing** (one bad window doesn't end the session); the frontend surfaces it inline without flipping `recording` off.
- **`start_capture` while already capturing** → `AppError::Other("already capturing")`.
- **`stop_capture` with no session** → no-op `Ok(())`.
- **BlackHole not installed** → simply absent from the device list (documented setup), not an error.
- **Whisper slower than realtime** → the transcript lags and the buffer grows (unbounded) — a documented v1 limitation, not an error.

---

## Testing

- **Rust — `audio.rs` inline `#[cfg(test)]`:** `encode_wav_pcm16_16k_mono` produces a valid header (assert `RIFF`/`WAVE`/`fmt `/`data` markers, channels=1, rate=16000, bits=16, data length = samples*2, file length = 44 + data); `downmix_to_mono` averages a 2-channel interleaved buffer correctly (and passes mono through); `f32_to_i16` clamps out-of-range and scales (1.0→32767, -1.0→-32767, 0.0→0); `resample_to_16k` output length ≈ `in_len * 16000 / in_rate` (±a small tolerance) and passes 16 kHz through unchanged.
- **Rust — `whisper.rs`:** already covered by M23's `tests/whisper_test.rs` (unchanged).
- **Not unit-testable (honest):** the live `cpal` capture, the thread/worker lifecycle, the Tauri `Channel` plumbing, and the real whisper-over-the-window path require a real audio device + a running whisper-server → **owner-pending manual E2E** (mirrors the M21/M23 owner-pending live legs). This milestone has **thinner automated coverage** than M23 by nature.
- **Maket (browser mock):** chrome-devtools — open a note, the capture row shows the mock device list; clicking **Record** appends the canned `[live]` chunks to the transcript textarea ~1 s apart; **Stop** ends it; then **Summarize** still works.
- **Gates:** `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean.

### Owner-pending live E2E (manual)
1. `brew install blackhole-2ch` (or download BlackHole); in **Audio MIDI Setup** create a **Multi-Output Device** (your speakers + BlackHole 2ch) and set it as the system output so you still hear the call.
2. Run a real whisper-server (`whisper-server -m ggml-base.en.bin --port 8080`, per M23).
3. In Ember: open a note → pick **BlackHole 2ch** → **Record** → play/join a meeting → watch the transcript fill → **Stop** → **Summarize** (needs Ollama). Grant the **Microphone** prompt when it appears.

---

## Known risks & decisions

- **`cpal` + selectable input device (BlackHole) over ScreenCaptureKit/process-taps** — the approved decision. Pure-Rust, mic-permission-only (stable for unsigned dev builds), no Swift/entitlements/codesigning/shell-plugin. Device-agnostic code → "system audio" is "select BlackHole." Cost: a one-time BlackHole install + Multi-Output Device setup (documented; same ethos as running Ollama/whisper-server).
- **Microphone TCC permission is the one real OS gate.** Capturing any input device (including the BlackHole virtual device) trips the mic prompt on recent macOS; needs `NSMicrophoneUsageDescription`. Mic TCC is far more stable for unsigned/ad-hoc builds than screen-recording TCC, but a rebuild can occasionally require re-granting — acceptable and far better than the ScreenCaptureKit path.
- **`cpal::Stream` is `!Send`** → it is built and owned entirely on a dedicated thread, controlled via an `AtomicBool` stop flag; the realtime callback only does a non-blocking `UnboundedSender::send`. All heavy work (resample/WAV/HTTP) happens on the async worker, never the audio thread.
- **Non-overlapping ~10 s windows, appended in order** — the simplest streaming model; a word may split at a boundary (tolerable for editable text → summarizer). Overlap/dedup and VAD deferred.
- **Backpressure is unbounded in v1** — if whisper is slower than realtime, lag and memory grow; the meeting ends and Stop flushes the rest. Documented; a cap is a future refinement.
- **Reuses the M23 `WhisperClient` and the M22 transcript spine unchanged** — M24 is purely the capture front-end; the transcription/summary halves are already proven.
- **New native deps `cpal` + `rubato` are unavoidable** — this is the native-audio milestone. WAV is hand-rolled (no `hound`) to keep the dep surface minimal and teach the format. No `tauri-plugin-shell`, no new OAuth scope, no new Tauri capability, no migration, no Settings.
- **Build in two verified phases (one spec):** Phase A — capture → a single window → transcribe → fill the field (prove cpal + mic permission + WAV + the M23 reuse + device picker + Record/Stop). Phase B — the streaming loop (windowing + `Channel` + live incremental append + Stop-flush). De-risks the capstone without a separate milestone.

---

## Non-goals / constraints

- **No ScreenCaptureKit / Core Audio process-tap capture, no Swift helper, no codesigning/notarization work** in M24.
- **No overlap/dedup, no VAD, no diarization, no interim partial-word results.**
- **No BlackHole bundling/auto-install; no Settings; no new OAuth scope, capability, migration, or `tauri-plugin-shell`.** Only new deps are `cpal` + `rubato`, plus an `Info.plist` mic-usage string.
- **Tauri build unchanged for the maket** — capture wrappers are `isTauri()`-gated; the maket drives the live-append UI with mock chunks on a timer.
- **Plain-text transcript** — no speaker/timestamp UI; the captured text is ordinary transcript text feeding the existing summarizer.
