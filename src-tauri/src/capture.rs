// src-tauri/src/capture.rs — live macOS audio capture (cpal) → streamed transcript (M24).
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| AppError::Other(format!("could not list input devices: {e}")))?;
    let mut out = Vec::new();
    for d in devices {
        // 🦀 In cpal 0.18 `DeviceTrait: Display`, so `to_string()` is the idiomatic way
        //    to get the device name. `description().name()` is the structured alternative.
        out.push(DeviceInfo { name: d.to_string() });
    }
    Ok(out)
}

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
    transcriber: tauri::State<'_, crate::transcribe::TranscriberState>,
) -> Result<()> {
    // 🦀 The in-process Whisper engine must be loaded first (the frontend calls
    //    prepare_transcription before this). Clone the Arc to hand to the worker, and fail
    //    early with a clear message if it isn't ready rather than erroring on every window.
    let transcriber: crate::transcribe::TranscriberState = (*transcriber).clone();
    {
        let g = transcriber
            .lock()
            .map_err(|_| AppError::Other("transcriber lock poisoned".into()))?;
        if g.is_none() {
            return Err(AppError::Other(
                "transcription engine not ready — call prepare_transcription first".into(),
            ));
        }
    }
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
        .find(|d| d.to_string() == device_name)
        .ok_or_else(|| AppError::Other(format!("input device not found: {device_name}")))?;
    let supported = device
        .default_input_config()
        .map_err(|e| AppError::Other(format!("device config error: {e}")))?;
    // 🦀 v1 assumes the CoreAudio default sample format is f32 (true on macOS); a non-f32 device
    //    would fail at build_input_stream below, surfaced as a start error.
    // 🦀 cpal-0.18 adaptation: `SampleRate` is a `type SampleRate = u32` alias here (not a
    //    tuple-struct), so `sample_rate()` is already a u32 — no `.0` field.
    let in_rate = supported.sample_rate();
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
        // 🦀 cpal-0.18 adaptation: `build_input_stream` takes `config: StreamConfig` BY VALUE
        //    here (not `&StreamConfig`), so we pass the owned `config` rather than a borrow.
        let stream = device.build_input_stream(
            config,
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

    let worker = tokio::spawn(worker_loop(rx, on_event, in_rate, channels, transcriber));

    // 🦀 If storing the session fails (poisoned lock), don't leak the thread/worker we just
    //    spawned: signal stop (so the audio thread exits) and abort the worker before returning.
    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            worker.abort();
            return Err(AppError::Other("capture lock poisoned".into()));
        }
    };
    // 🦀 Close the check-then-store race: if another start_capture won the slot while we were
    //    building the stream, abort OUR session (signal stop + abort the worker) rather than
    //    overwriting and orphaning theirs.
    if guard.is_some() {
        stop.store(true, Ordering::Relaxed);
        worker.abort();
        return Err(AppError::Other("already capturing".into()));
    }
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

// 🦀 Phase B worker: emit a transcript chunk for every full ~10s window as it accumulates,
//    then flush the remainder when the stream closes. `transcribe_and_emit` is unchanged.
async fn worker_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    in_rate: u32,
    channels: u16,
    transcriber: crate::transcribe::TranscriberState,
) {
    const WINDOW_SECS: usize = 10;
    let stride = channels.max(1) as usize;
    // 🦀 interleaved samples per window = frames-per-window * channels
    let window_samples = in_rate as usize * WINDOW_SECS * stride;
    let mut buf: Vec<f32> = Vec::new();
    while let Some(chunk) = rx.recv().await {
        buf.extend_from_slice(&chunk);
        // 🦀 Emit each full window as it accumulates; `drain(..n)` removes the first n samples.
        while buf.len() >= window_samples {
            let window: Vec<f32> = buf.drain(..window_samples).collect();
            transcribe_and_emit(&transcriber, &window, channels, in_rate, &on_event);
        }
    }
    if !buf.is_empty() {
        transcribe_and_emit(&transcriber, &buf, channels, in_rate, &on_event);
    }
    let _ = on_event.send(CaptureEvent::Stopped);
}

// 🦀 One window: interleaved f32 → WAV → whisper → emit. Empty/silent windows are skipped;
//    a transcription error is reported but does NOT end the session.
// 🦀 Sync (no `.await`): downmix + resample to 16 kHz mono, then transcribe in-process via the
//    shared whisper-rs context. whisper-rs is CPU-bound and blocking; one 10s window per call on
//    a single capture stream is acceptable. The std Mutex guard is dropped before returning and
//    is never held across an await (there is none here).
fn transcribe_and_emit(
    transcriber: &crate::transcribe::TranscriberState,
    interleaved: &[f32],
    channels: u16,
    in_rate: u32,
    on_event: &tauri::ipc::Channel<CaptureEvent>,
) {
    let mono = crate::audio::downmix_to_mono(interleaved, channels);
    let samples = crate::audio::resample_to_16k(&mono, in_rate);
    let text = {
        let guard = match transcriber.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.as_ref() {
            Some(t) => t.transcribe_samples(&samples),
            None => {
                let _ = on_event.send(CaptureEvent::Error {
                    message: "transcriber not ready".into(),
                });
                return;
            }
        }
    };
    match text {
        Ok(text) if !text.trim().is_empty() => {
            let _ = on_event.send(CaptureEvent::Chunk { text });
        }
        Ok(_) => {} // silence → nothing to append
        Err(e) => {
            let _ = on_event.send(CaptureEvent::Error { message: e.to_string() });
        }
    }
}
