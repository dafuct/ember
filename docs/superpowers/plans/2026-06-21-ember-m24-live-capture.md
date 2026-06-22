# Ember M24 — Live macOS system-audio capture → streaming transcript — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture live audio from a user-selected input device (BlackHole = system audio), and stream a growing transcript into a note in real time, reusing the M23 Whisper spine.

**Architecture:** A `cpal` input stream runs on a dedicated thread (its `Stream` is `!Send`); the realtime callback only does a non-blocking `send` into a `tokio::sync::mpsc` channel. An async worker accumulates ~10 s windows, downmixes→16 kHz→hand-rolled WAV, calls the M23 `WhisperClient`, and emits each window's text over a Tauri `Channel<CaptureEvent>` that the NotesModal appends to the transcript live. Built in two phases: **A** = capture → transcribe-the-whole-buffer-on-stop (prove the pipeline + mic permission + device picker + Record/Stop end-to-end); **B** = upgrade the worker to incremental ~10 s windowing (the streaming behavior).

**Tech Stack:** Rust (NEW dep **`cpal`**; resampler + WAV encoder hand-rolled — no `rubato`, no `hound`; reuses the M23 `WhisperClient`), React 19 + TypeScript + Vite, the Tauri 2 **`Channel`** IPC-streaming primitive, an `Info.plist` `NSMicrophoneUsageDescription`.

**Learning mode (IMPORTANT):** the repo owner is learning Rust. Every Rust change carries concise `// 🦀` teaching comments on the language/runtime concept (a `!Send` value pinned to its thread, the realtime-callback constraint, `tokio::sync::mpsc::unbounded_channel`, `AtomicBool` stop-signaling, the WAV/RIFF byte layout, linear-interpolation resampling, the Tauri `Channel`). End each Rust task with a 2–3 sentence plain-English recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`.**

**Working directory:** Rust commands run from `src-tauri/`; frontend commands from the repo root `/Users/makar/dev/ownmail`.

---

## File Structure

| File | Create/Modify | Responsibility |
|---|---|---|
| `src-tauri/Cargo.toml` | Modify | Add `cpal` |
| `src-tauri/src/audio.rs` | **Create** | Pure: downmix, resample, f32→i16, WAV encode, `window_to_wav` (unit-tested) |
| `src-tauri/src/capture.rs` | **Create** | `DeviceInfo`/`CaptureEvent`, `list_input_devices`, `start_capture`/`stop_capture`, the cpal session + worker |
| `src-tauri/src/lib.rs` | Modify | `pub mod audio/capture;`, manage `CaptureState`, register 3 commands |
| `src-tauri/Info.plist` + `Info.dev.plist` | **Create** | `NSMicrophoneUsageDescription` |
| `src/lib/notes.ts` | Modify | `DeviceInfo`/`CaptureEvent` types + `listInputDevices`/`startCapture`/`stopCapture` |
| `src/lib/mock.ts` | Modify | Mock device list + timer-based chunk emission |
| `src/components/NotesModal.tsx` | Modify | Device picker + Record/Stop + live chunk append |
| `src/styles/app.css` | Modify | `.note-capture-row` |

---

# PHASE A — Capture foundation (prove the pipeline end-to-end)

## Task 1: Pure audio helpers (`audio.rs`) + `cpal` dep

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/audio.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod audio;`)

- [ ] **Step 1: Add the `cpal` dependency**

Run: `cd src-tauri && cargo add cpal`
Expected: adds the current `cpal` to `[dependencies]` in `Cargo.toml`.

- [ ] **Step 2: Write `audio.rs` with the pure functions AND their tests**

Create `src-tauri/src/audio.rs`:

```rust
// src-tauri/src/audio.rs — pure audio helpers for live capture (no I/O, unit-testable, M24).

/// Average all channels of an interleaved frame buffer down to mono.
/// `interleaved` is [L,R,L,R,…] for 2 channels; returns one sample per frame.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    // 🦀 `chunks_exact(ch)` yields one slice per frame; a trailing partial frame is dropped.
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Linear-interpolation resample a mono buffer from `in_rate` to 16 000 Hz.
/// Passthrough when already 16 kHz (or too short). Handles arbitrary ratios (e.g. 44.1 kHz).
pub fn resample_to_16k(mono: &[f32], in_rate: u32) -> Vec<f32> {
    const OUT_RATE: u32 = 16_000;
    if in_rate == OUT_RATE || mono.len() < 2 {
        return mono.to_vec();
    }
    let ratio = OUT_RATE as f64 / in_rate as f64;
    let out_len = ((mono.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        // 🦀 `src` is the fractional sample position in the input; lerp between its neighbours.
        let src = i as f64 / ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(mono.len() - 1);
        let frac = (src - i0 as f64) as f32;
        out.push(mono[i0] * (1.0 - frac) + mono[i1] * frac);
    }
    out
}

/// Convert f32 samples in [-1.0, 1.0] to 16-bit PCM (clamp then scale).
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

/// Encode 16 kHz mono 16-bit PCM as a WAV byte buffer (44-byte RIFF/WAVE header + data).
// 🦀 A WAV file is a RIFF container: a "RIFF"+size+"WAVE" header, a "fmt " chunk describing the
//    format, then a "data" chunk of little-endian samples. We write the 44 bytes by hand.
pub fn encode_wav_pcm16_16k_mono(samples: &[i16]) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 16_000;
    const CHANNELS: u16 = 1;
    const BITS: u16 = 16;
    let data_len = (samples.len() * 2) as u32; // 2 bytes per i16 sample
    let byte_rate = SAMPLE_RATE * CHANNELS as u32 * (BITS as u32 / 8);
    let block_align = CHANNELS * (BITS / 8);
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes()); // RIFF chunk size = 36 + data
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size (PCM)
    v.extend_from_slice(&1u16.to_le_bytes()); // audio format = 1 (PCM)
    v.extend_from_slice(&CHANNELS.to_le_bytes());
    v.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&BITS.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// Full window pipeline: interleaved capture frames → mono → 16 kHz → PCM16 → WAV bytes.
pub fn window_to_wav(interleaved: &[f32], channels: u16, in_rate: u32) -> Vec<u8> {
    let mono = downmix_to_mono(interleaved, channels);
    let resampled = resample_to_16k(&mono, in_rate);
    let pcm = f32_to_i16(&resampled);
    encode_wav_pcm16_16k_mono(&pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo_and_passes_mono() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.1, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn f32_to_i16_clamps_and_scales() {
        assert_eq!(f32_to_i16(&[0.0, 1.0, -1.0, 2.0, -2.0]), vec![0, 32767, -32767, 32767, -32767]);
    }

    #[test]
    fn resample_passes_16k_through_and_resizes_48k() {
        assert_eq!(resample_to_16k(&vec![0.0f32; 100], 16_000).len(), 100);
        let out = resample_to_16k(&vec![0.0f32; 4800], 48_000); // 0.1s @48k → ~0.1s @16k
        assert!((out.len() as i64 - 1600).abs() <= 2, "got {}", out.len());
    }

    #[test]
    fn wav_header_is_well_formed() {
        let wav = encode_wav_pcm16_16k_mono(&[0, 1, -1]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1); // channels
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000); // rate
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16); // bits
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6); // data len = 3*2
        assert_eq!(wav.len(), 44 + 6);
    }
}
```

- [ ] **Step 3: Declare the module**

In `src-tauri/src/lib.rs`, after `pub mod whisper;` (added in M23), add:
```rust
// 🦀 Pure audio helpers (downmix/resample/WAV) for live capture — no I/O, unit-tested (M24).
pub mod audio;
```

- [ ] **Step 4: Run the tests**

Run: `cd src-tauri && cargo test audio:: 2>&1 | tail -15`
Expected: 4 `audio` tests pass.

- [ ] **Step 5: Lint**

Run: `cd src-tauri && cargo clippy --all-targets 2>&1 | tail -10`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/audio.rs src-tauri/src/lib.rs
git commit -m "feat(m24): pure audio helpers (downmix/resample/WAV) + cpal dep"
```

**Rust recap:** cover `chunks_exact`, linear interpolation for resampling, and hand-writing the little-endian RIFF/WAVE header.

---

## Task 2: `capture.rs` types + `list_input_devices`

**Files:**
- Create: `src-tauri/src/capture.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod capture;` + register `list_input_devices`)

- [ ] **Step 1: Create `capture.rs` with the types + device listing**

Create `src-tauri/src/capture.rs`:

```rust
// src-tauri/src/capture.rs — live macOS audio capture (cpal) → streamed transcript (M24).
use serde::Serialize;

use crate::error::{AppError, Result};

// 🦀 What the device picker shows. `Serialize` so Tauri can send it to the frontend as JSON.
#[derive(Serialize)]
pub struct DeviceInfo {
    pub name: String,
}

// 🦀 The streamed capture events. `#[serde(tag = "type")]` serializes each variant as
//    `{ "type": "Chunk", "text": "…" }`, so the frontend can switch on `type`.
#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum CaptureEvent {
    Chunk { text: String },
    Error { message: String },
    Stopped,
}

/// List the system's audio input devices (the user picks BlackHole for system audio). DB-free.
#[tauri::command]
pub async fn list_input_devices() -> Result<Vec<DeviceInfo>> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| AppError::Other(format!("could not list input devices: {e}")))?;
    let mut out = Vec::new();
    for d in devices {
        // 🦀 `d.name()` returns a Result; skip any device whose name can't be read.
        if let Ok(name) = d.name() {
            out.push(DeviceInfo { name });
        }
    }
    Ok(out)
}
```

- [ ] **Step 2: Declare + register**

In `src-tauri/src/lib.rs`: after `pub mod audio;` add:
```rust
// 🦀 Live audio capture session + commands (M24). `pub` is not required (only the IPC bridge
//    calls these), but mirrors the sibling modules.
pub mod capture;
```
And in `tauri::generate_handler![ … ]`, after `commands::transcribe_recording,` add:
```rust
            commands::transcribe_recording,
            capture::list_input_devices,
```

- [ ] **Step 3: Build + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -15 && cargo clippy --all-targets 2>&1 | tail -10`
Expected: clean (cpal compiles; `list_input_devices` registered).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/capture.rs src-tauri/src/lib.rs
git commit -m "feat(m24): capture types + list_input_devices command"
```

**Rust recap:** cover the `#[serde(tag = "type")]` enum tagging and the cpal host/device traits.

---

## Task 3: `start_capture` / `stop_capture` + one-shot worker + Info.plist

**Files:**
- Modify: `src-tauri/src/capture.rs` (append session, commands, worker)
- Modify: `src-tauri/src/lib.rs` (manage `CaptureState` + register the 2 commands)
- Create: `src-tauri/Info.plist`, `src-tauri/Info.dev.plist`

No automated test — this is real device/thread/network I/O (owner-pending E2E, like `transcribe_recording`). Verified by `cargo build`/`clippy` and the maket (Task 5).

- [ ] **Step 1: Append the session, commands, and worker to `capture.rs`**

At the end of `src-tauri/src/capture.rs` add:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// 🦀 The running session: a stop flag plus the two handles we join on stop. Lives in
//    Tauri-managed state so start/stop share it.
pub struct CaptureSession {
    stop: Arc<AtomicBool>,
    audio_thread: std::thread::JoinHandle<()>,
    worker: tokio::task::JoinHandle<()>,
}

// 🦀 Type alias for the managed state: an optional in-flight session behind a Mutex.
pub type CaptureState = Arc<Mutex<Option<CaptureSession>>>;

/// Start capturing from `device_name`; transcript text streams back over `on_event`.
#[tauri::command]
pub async fn start_capture(
    device_name: String,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    state: tauri::State<'_, CaptureState>,
) -> Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    // 🦀 Refuse a second concurrent capture. Scope the lock so the guard drops before we spawn.
    {
        let guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
        if guard.is_some() {
            return Err(AppError::Other("already capturing".into()));
        }
    }
    // Find the device + read its default config in the command (cpal Device is Send; Stream is not).
    let host = cpal::default_host();
    let device = host
        .input_devices()
        .map_err(|e| AppError::Other(format!("could not list input devices: {e}")))?
        .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
        .ok_or_else(|| AppError::Other(format!("input device not found: {device_name}")))?;
    let supported = device
        .default_input_config()
        .map_err(|e| AppError::Other(format!("device config error: {e}")))?;
    // 🦀 v1 assumes the CoreAudio default sample format is f32 (true on macOS); a non-f32 device
    //    would fail at build_input_stream below, surfaced as a start error.
    let in_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let config: cpal::StreamConfig = supported.config();

    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<f32>>();
    // 🦀 A blocking std channel used as a oneshot so the command can report a stream-build error
    //    (e.g. microphone permission denied) that happens on the audio thread.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

    let stop_audio = stop.clone();
    let audio_thread = std::thread::spawn(move || {
        // 🦀 The cpal Stream is !Send, so it is built and owned entirely on this thread.
        let data_tx = tx; // dropped on thread exit → closes rx → the worker flushes + stops
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // 🦀 Realtime callback: only a cheap non-blocking send — no heavy work here.
                let _ = data_tx.send(data.to_vec());
            },
            move |e| eprintln!("cpal input stream error: {e}"),
            None,
        );
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                let _ = ready_tx.send(Err(e.to_string()));
                return;
            }
        };
        if let Err(e) = stream.play() {
            let _ = ready_tx.send(Err(e.to_string()));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        // 🦀 Park until stop, then dropping `stream` at scope end stops capture.
        while !stop_audio.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    // 🦀 Wait for the stream to actually start (or fail) before reporting success.
    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = audio_thread.join();
            return Err(AppError::Other(format!(
                "could not start capture: {e}. If this is a permissions issue, enable Microphone \
                 for Ember in System Settings → Privacy & Security → Microphone."
            )));
        }
        Err(_) => return Err(AppError::Other("audio thread failed to start".into())),
    }

    let worker = tokio::spawn(worker_loop(rx, on_event, in_rate, channels));

    let mut guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
    *guard = Some(CaptureSession { stop, audio_thread, worker });
    Ok(())
}

/// Stop the active capture (no-op if none). Flushes the final audio, then emits `Stopped`.
#[tauri::command]
pub async fn stop_capture(state: tauri::State<'_, CaptureState>) -> Result<()> {
    // 🦀 Take the session out under the lock, then drop the guard before joining/awaiting.
    let session = {
        let mut guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
        guard.take()
    };
    if let Some(s) = session {
        s.stop.store(true, Ordering::Relaxed);
        let _ = s.audio_thread.join(); // returns within ~100ms (the park interval)
        let _ = s.worker.await; // worker drains the remainder + emits Stopped
    }
    Ok(())
}

// 🦀 Phase A worker: accumulate the WHOLE capture, transcribe once when the stream closes.
//    Phase B (Task 6) upgrades this to incremental ~10s windows.
async fn worker_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    in_rate: u32,
    channels: u16,
) {
    let whisper = crate::whisper::WhisperClient::new();
    let mut buf: Vec<f32> = Vec::new();
    while let Some(chunk) = rx.recv().await {
        buf.extend_from_slice(&chunk);
    }
    if !buf.is_empty() {
        transcribe_and_emit(&whisper, &buf, channels, in_rate, &on_event).await;
    }
    let _ = on_event.send(CaptureEvent::Stopped);
}

// 🦀 One window: interleaved f32 → WAV → whisper → emit. Empty/silent windows are skipped;
//    a transcription error is reported but does NOT end the session.
async fn transcribe_and_emit(
    whisper: &crate::whisper::WhisperClient,
    interleaved: &[f32],
    channels: u16,
    in_rate: u32,
    on_event: &tauri::ipc::Channel<CaptureEvent>,
) {
    let wav = crate::audio::window_to_wav(interleaved, channels, in_rate);
    match whisper.transcribe(wav, "chunk.wav", "audio/wav").await {
        Ok(text) if !text.trim().is_empty() => {
            let _ = on_event.send(CaptureEvent::Chunk { text });
        }
        Ok(_) => {} // silence → nothing to append
        Err(e) => {
            let _ = on_event.send(CaptureEvent::Error { message: e.to_string() });
        }
    }
}
```

- [ ] **Step 2: Manage state + register the commands in `lib.rs`**

In `src-tauri/src/lib.rs` `setup`, after the `app.manage(std::sync::Arc::new(std::sync::Mutex::new(conn)));` line, add:
```rust
            // 🦀 Live-capture session state (M24): starts empty; start/stop_capture fill/clear it.
            app.manage(crate::capture::CaptureState::default());
```
And in `generate_handler!`, after `capture::list_input_devices,` add:
```rust
            capture::list_input_devices,
            capture::start_capture,
            capture::stop_capture,
```

- [ ] **Step 3: Create the macOS mic-usage plists**

Create `src-tauri/Info.plist`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSMicrophoneUsageDescription</key>
    <string>Ember records meeting audio to transcribe it locally.</string>
</dict>
</plist>
```
Create `src-tauri/Info.dev.plist` with identical content (Tauri merges `Info.dev.plist` for `tauri dev`, `Info.plist` for release).

- [ ] **Step 4: Build + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -20 && cargo clippy --all-targets 2>&1 | tail -12`
Expected: clean. (The plists affect bundling only, not `cargo build`; their correctness is owner-verified at E2E.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/capture.rs src-tauri/src/lib.rs src-tauri/Info.plist src-tauri/Info.dev.plist
git commit -m "feat(m24): start/stop_capture + one-shot worker + mic Info.plist"
```

**Rust recap:** cover pinning a `!Send` value to its own thread, `AtomicBool` stop-signaling, the `std::sync::mpsc` oneshot for reporting the build result, and why the Mutex guard must drop before `.await`.

---

## Task 4: Frontend wrappers + mock

**Files:**
- Modify: `src/lib/notes.ts`
- Modify: `src/lib/mock.ts`

- [ ] **Step 1: Add the mock (`mock.ts`)**

In `src/lib/mock.ts`, ensure the type-only import from `./notes` includes the new types — find the existing `import type { … } from "./notes";` line and add `DeviceInfo, CaptureEvent` to it (if there is no such line, add `import type { DeviceInfo, CaptureEvent } from "./notes";` near the top). Then append at the end of the file:

```ts
// M24: mock input devices for the maket device picker.
export function mockListInputDevices(): DeviceInfo[] {
  return [{ name: "MacBook Pro Microphone" }, { name: "BlackHole 2ch" }];
}

// M24: simulate live capture by emitting canned chunks on a timer until stopped.
let mockCaptureTimer: ReturnType<typeof setInterval> | null = null;
let mockOnEvent: ((e: CaptureEvent) => void) | null = null;

export function mockStartCapture(
  _deviceName: string,
  onEvent: (e: CaptureEvent) => void,
): Promise<void> {
  mockOnEvent = onEvent;
  const lines = [
    "[live] Dana: Let's kick off the weekly sync.",
    "[live] You: Shipping the capture milestone today.",
    "[live] Dana: Action — review the transcript before the demo.",
  ];
  let i = 0;
  mockCaptureTimer = setInterval(() => {
    if (i < lines.length) {
      onEvent({ type: "Chunk", text: lines[i] });
      i += 1;
    } else if (mockCaptureTimer) {
      clearInterval(mockCaptureTimer);
      mockCaptureTimer = null;
    }
  }, 1000);
  return Promise.resolve();
}

export function mockStopCapture(): Promise<void> {
  if (mockCaptureTimer) {
    clearInterval(mockCaptureTimer);
    mockCaptureTimer = null;
  }
  if (mockOnEvent) {
    mockOnEvent({ type: "Stopped" });
    mockOnEvent = null;
  }
  return Promise.resolve();
}
```

- [ ] **Step 2: Add the wrappers + types (`notes.ts`)**

In `src/lib/notes.ts`:

(a) Add `Channel` to the `@tauri-apps/api/core` import:
```ts
import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
```

(b) Add `mockListInputDevices, mockStartCapture, mockStopCapture` to the existing import from `./mock`.

(c) Append at the end of the file:
```ts
export interface DeviceInfo {
  name: string;
}

// M24: the streamed capture events (matches the Rust #[serde(tag = "type")] enum).
export type CaptureEvent =
  | { type: "Chunk"; text: string }
  | { type: "Error"; message: string }
  | { type: "Stopped" };

export const listInputDevices = (): Promise<DeviceInfo[]> =>
  isTauri() ? invoke<DeviceInfo[]>("list_input_devices") : Promise.resolve(mockListInputDevices());

export const startCapture = (
  deviceName: string,
  onEvent: (e: CaptureEvent) => void,
): Promise<void> => {
  if (!isTauri()) return mockStartCapture(deviceName, onEvent);
  // The Tauri Channel streams CaptureEvent objects from the Rust worker to onEvent.
  const ch = new Channel<CaptureEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("start_capture", { deviceName, onEvent: ch });
};

export const stopCapture = (): Promise<void> =>
  isTauri() ? invoke<void>("stop_capture") : mockStopCapture();
```

- [ ] **Step 3: Typecheck**

Run: `npm run build 2>&1 | tail -15`
Expected: build succeeds.

- [ ] **Step 4: Commit**

```bash
git add src/lib/notes.ts src/lib/mock.ts
git commit -m "feat(m24): capture lib wrappers (Channel) + timer-based mock"
```

---

## Task 5: NotesModal capture UI (device picker + Record/Stop + live append)

**Files:**
- Modify: `src/components/NotesModal.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Import the wrappers + types**

In `src/components/NotesModal.tsx`, add to the existing import from `../lib/notes`: `listInputDevices`, `startCapture`, `stopCapture`, and (type) `DeviceInfo`. The cleanest form — add the values to the existing brace list, and add a separate type import line below it:
```ts
import type { DeviceInfo, CaptureEvent } from "../lib/notes";
```
(Add `listInputDevices, startCapture, stopCapture,` to the existing value import from `../lib/notes`.)

- [ ] **Step 2: Add state**

After the `const [transcribing, setTranscribing] = useState(false);` line (currently line 43), add:
```tsx
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [recording, setRecording] = useState(false);
```

- [ ] **Step 3: Load the device list on open**

After the existing `useEffect` that registers the Esc key handler (near the top of the component), add a new effect:
```tsx
  // Load audio input devices for the capture picker (best-effort; empty list if none).
  useEffect(() => {
    listInputDevices()
      .then((ds) => {
        setDevices(ds);
        setSelectedDevice((prev) => prev || ds[0]?.name || "");
      })
      .catch(() => {});
  }, []);
```

- [ ] **Step 4: Add `recording` to `blocked`**

Change the `blocked` line (currently line 196) from:
```tsx
  const blocked = busy || summarizing || importing || transcribing;
```
to:
```tsx
  const blocked = busy || summarizing || importing || transcribing || recording;
```

- [ ] **Step 5: Add the capture handlers**

Immediately after the `handleTranscribe` function's closing brace, add:
```tsx
  async function handleRecord() {
    setError(null);
    setRecording(true);
    try {
      await startCapture(selectedDevice, (e) => {
        if (e.type === "Chunk") {
          // Append each transcribed chunk to the transcript, newline-separated.
          setTranscript((t) => (t ? t + "\n" : "") + e.text);
        } else if (e.type === "Error") {
          setError(e.message);
        } else if (e.type === "Stopped") {
          setRecording(false);
        }
      });
    } catch (err) {
      setError(String(err));
      setRecording(false);
    }
  }

  async function handleStop() {
    try {
      await stopCapture();
    } catch (err) {
      setError(String(err));
    }
    setRecording(false);
  }
```

- [ ] **Step 6: Add the capture row to the JSX**

In `src/components/NotesModal.tsx`, the transcript `<textarea>` block currently ends at line 237 (`/>`), immediately before `<div className="note-summary-section">` (line 238). Insert the capture row between them:
```tsx
            <div className="note-capture-row">
              <select
                className="note-device-select"
                value={selectedDevice}
                onChange={(e) => setSelectedDevice(e.target.value)}
                disabled={blocked}
              >
                {devices.length === 0 && <option value="">No input devices</option>}
                {devices.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}
                  </option>
                ))}
              </select>
              {recording ? (
                <button className="btn btn-danger-outline" onClick={handleStop}>
                  Stop
                </button>
              ) : (
                <button className="btn" onClick={handleRecord} disabled={blocked || !selectedDevice}>
                  Record
                </button>
              )}
              {recording && <span className="note-capture-pulse">● listening…</span>}
            </div>
```

- [ ] **Step 7: Add the CSS**

In `src/styles/app.css`, after the `.note-transcript-actions { … }` rule (added in M23), add:
```css
.note-capture-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 8px 0;
}
.note-device-select {
  flex: 1;
  min-width: 0;
}
.note-capture-pulse {
  font-size: 12px;
  color: var(--danger, #c0392b);
}
```

- [ ] **Step 8: Typecheck + maket verify**

Run: `npm run build 2>&1 | tail -15` (expect clean).
Then start the dev server and, via the preview/chrome-devtools tools, open a calendar event's note: confirm the capture row shows the two mock devices; click **Record** and confirm the canned `[live]` chunks append to the transcript textarea ~1 s apart; click **Stop** and confirm it ends; confirm no console errors. Screenshot.

- [ ] **Step 9: Commit**

```bash
git add src/components/NotesModal.tsx src/styles/app.css
git commit -m "feat(m24): NotesModal capture row — device picker + Record/Stop + live append"
```

---

# PHASE B — Streaming (incremental windowing)

## Task 6: Upgrade the worker to ~10 s windows

**Files:**
- Modify: `src-tauri/src/capture.rs` (replace `worker_loop` body)

- [ ] **Step 1: Replace `worker_loop` with the windowed version**

In `src-tauri/src/capture.rs`, replace the entire Phase-A `worker_loop` function with:
```rust
// 🦀 Phase B worker: emit a transcript chunk for every full ~10s window as it accumulates,
//    then flush the remainder when the stream closes. `transcribe_and_emit` is unchanged.
async fn worker_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    in_rate: u32,
    channels: u16,
) {
    const WINDOW_SECS: usize = 10;
    let whisper = crate::whisper::WhisperClient::new();
    let stride = channels.max(1) as usize;
    // interleaved samples per window = frames-per-window * channels
    let window_samples = in_rate as usize * WINDOW_SECS * stride;
    let mut buf: Vec<f32> = Vec::new();
    while let Some(chunk) = rx.recv().await {
        buf.extend_from_slice(&chunk);
        while buf.len() >= window_samples {
            let window: Vec<f32> = buf.drain(..window_samples).collect();
            transcribe_and_emit(&whisper, &window, channels, in_rate, &on_event).await;
        }
    }
    if !buf.is_empty() {
        transcribe_and_emit(&whisper, &buf, channels, in_rate, &on_event).await;
    }
    let _ = on_event.send(CaptureEvent::Stopped);
}
```

- [ ] **Step 2: Build + lint**

Run: `cd src-tauri && cargo build 2>&1 | tail -15 && cargo clippy --all-targets 2>&1 | tail -10`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/capture.rs
git commit -m "feat(m24): stream transcript in ~10s windows (Phase B)"
```

**Rust recap:** cover draining a `Vec` in fixed slices (`drain(..n)`) and why windowing in interleaved-sample units needs the channel stride.

---

## Task 7: Full gates + maket re-verify

**Files:** none (verification only)

- [ ] **Step 1: Rust gates**

Run: `cd src-tauri && cargo test 2>&1 | tail -25 && cargo clippy --all-targets 2>&1 | tail -12`
Expected: all tests pass (incl. the 4 new `audio` tests); clippy clean.

- [ ] **Step 2: Frontend gate**

Run: `npm run build 2>&1 | tail -12`
Expected: clean.

- [ ] **Step 3: Maket re-verify**

Confirm (preview/chrome-devtools) the capture row still works: Record appends the mock chunks live, Stop ends it, then Summarize still works. Screenshot.

- [ ] **Step 4: Confirm clean tree** — `git status -s` shows no stray files (watch for the M23-style rogue edits).

---

## Owner-pending live E2E (manual — not a code task)

1. `brew install blackhole-2ch`; in **Audio MIDI Setup** create a **Multi-Output Device** (your speakers + BlackHole 2ch) and set it as the system output (so you still hear the call).
2. Run a whisper-server (`whisper-server -m ggml-base.en.bin --port 8080`, per M23).
3. In Ember: open a note → pick **BlackHole 2ch** → **Record** → join/play a meeting → watch the transcript fill in ~10 s increments → **Stop** → **Summarize** (needs Ollama). Grant the **Microphone** prompt when it appears.

---

## Self-Review notes (already applied)

- **Spec coverage:** audio.rs pure helpers + tests (T1) ✓; `cpal` dep (T1) ✓; `list_input_devices` (T2) ✓; `start_capture`/`stop_capture` + cpal session (dedicated `!Send` thread → tokio channel → worker) + mic Info.plist (T3) ✓; managed `CaptureState` + registration (T2/T3) ✓; frontend wrappers via Tauri `Channel` + mock (T4) ✓; NotesModal device picker + Record/Stop + live append + `recording` in `blocked` (T5) ✓; ~10 s windowing (T6) ✓; non-overlapping windows, mic-permission-only, no new scope/capability/migration/Settings, owner-pending E2E (throughout) ✓.
- **Type consistency:** `CaptureEvent` tagged `{type:"Chunk"|"Error"|"Stopped"}` matches the Rust `#[serde(tag="type")]` enum; `startCapture(deviceName, onEvent)` ↔ `start_capture(device_name, on_event)` (Tauri camel↔snake); `window_to_wav(interleaved, channels, in_rate)` used identically in `transcribe_and_emit` and both worker versions; `WhisperClient::transcribe(bytes, "chunk.wav", "audio/wav")` matches the M23 signature.
- **No placeholders:** every code step shows complete code; commands include working-directory context. cpal version added via `cargo add` (no guessed pin).
