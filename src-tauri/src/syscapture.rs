// src-tauri/src/syscapture.rs — zero-setup live capture via ScreenCaptureKit (native ObjC helper).
// Captures system audio (the call) + optionally the mic from one SCStream, mixes to 16 kHz mono,
// and transcribes in-process. No BlackHole / Multi-Output / Aggregate devices: the user grants
// Screen Recording (+ Microphone) once and hits Record. The ObjC bridge lives in native/syscapture.m.
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

use crate::capture::CaptureEvent;
use crate::error::{AppError, Result};

// 🦀 The C interface implemented in native/syscapture.m. `link_name`s match the ObjC function names.
extern "C" {
    fn ember_syscapture_start(
        capture_mic: c_int,
        cb: extern "C" fn(*mut c_void, *const f32, c_int, f64, c_int),
        ctx: *mut c_void,
        err_out: *mut c_char,
        err_len: c_int,
    ) -> *mut c_void;
    fn ember_syscapture_stop(handle: *mut c_void);
}

/// One mono chunk from a source, tagged with its real sample rate (the mic can differ from 48 kHz).
struct Chunk {
    is_mic: bool,
    samples: Vec<f32>,
    rate: u32,
}

// 🦀 Raw ObjC pointers aren't Send by default; wrap so the session can move across threads. Stopping
//    a ScreenCaptureKit stream is safe from any thread.
struct Handle(*mut c_void);
unsafe impl Send for Handle {}

pub struct SysSession {
    handle: Handle,
    // The boxed Sender is what the C callback writes to (via a raw pointer). Kept alive here and
    // dropped only AFTER the stream is stopped, so no callback can use a freed pointer.
    _tx: Box<tokio::sync::mpsc::UnboundedSender<Chunk>>,
    worker: tokio::task::JoinHandle<()>,
}

pub type SysCaptureState = Arc<Mutex<Option<SysSession>>>;

// 🦀 Called from the ObjC dispatch queue for every audio chunk. `ctx` is a pointer to the boxed
//    Sender. We copy the samples out immediately (the pointer is only valid for this call).
extern "C" fn on_audio(ctx: *mut c_void, mono: *const f32, frames: c_int, rate: f64, is_mic: c_int) {
    if ctx.is_null() || mono.is_null() || frames <= 0 {
        return;
    }
    let tx = unsafe { &*(ctx as *const tokio::sync::mpsc::UnboundedSender<Chunk>) };
    let samples = unsafe { std::slice::from_raw_parts(mono, frames as usize) }.to_vec();
    let _ = tx.send(Chunk { is_mic: is_mic != 0, samples, rate: rate.max(1.0) as u32 });
}

fn err_buf_to_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// Start zero-setup capture: system audio (+ mic if `capture_mic`), transcript streams over `on_event`.
#[tauri::command]
pub async fn start_system_capture(
    capture_mic: bool,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    state: tauri::State<'_, SysCaptureState>,
    transcriber: tauri::State<'_, crate::transcribe::TranscriberState>,
) -> Result<()> {
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
    {
        let guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
        if guard.is_some() {
            return Err(AppError::Other("already capturing".into()));
        }
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Chunk>();
    // 🦀 Box the Sender so it has a stable address for the C callback's `ctx` pointer.
    let boxed_tx = Box::new(tx);
    let ctx_addr = (&*boxed_tx) as *const tokio::sync::mpsc::UnboundedSender<Chunk> as usize;
    let mic = if capture_mic { 1 } else { 0 };

    // 🦀 Starting blocks on a Screen-Recording permission prompt (the user may take seconds), so run
    //    it on the blocking pool — never on the async runtime. `boxed_tx` stays alive across this
    //    await, so `ctx_addr` remains valid for the duration of the start call and beyond.
    let started = tokio::task::spawn_blocking(move || {
        let mut err = [0u8; 320];
        let handle = unsafe {
            ember_syscapture_start(
                mic,
                on_audio,
                ctx_addr as *mut c_void,
                err.as_mut_ptr() as *mut c_char,
                err.len() as c_int,
            )
        };
        if handle.is_null() {
            Err(err_buf_to_string(&err))
        } else {
            Ok(handle as usize)
        }
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;

    let handle = match started {
        Ok(h) => h as *mut c_void,
        Err(msg) => return Err(AppError::Other(msg)), // boxed_tx dropped here; nothing started
    };

    let worker = tokio::spawn(mix_worker(rx, on_event, capture_mic, transcriber));

    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(_) => {
            unsafe { ember_syscapture_stop(handle) };
            worker.abort();
            return Err(AppError::Other("capture lock poisoned".into()));
        }
    };
    if guard.is_some() {
        unsafe { ember_syscapture_stop(handle) };
        worker.abort();
        return Err(AppError::Other("already capturing".into()));
    }
    *guard = Some(SysSession { handle: Handle(handle), _tx: boxed_tx, worker });
    Ok(())
}

/// Stop the active system capture (no-op if none). Flushes the remainder, then emits `Stopped`.
#[tauri::command]
pub async fn stop_system_capture(state: tauri::State<'_, SysCaptureState>) -> Result<()> {
    let session = {
        let mut guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
        guard.take()
    };
    if let Some(s) = session {
        // 🦀 Stop the stream first (blocking → no callback after it returns), THEN drop the Sender
        //    (closes the channel so the worker flushes + emits Stopped).
        let h = s.handle.0 as usize;
        let _ = tokio::task::spawn_blocking(move || unsafe {
            ember_syscapture_stop(h as *mut c_void)
        })
        .await;
        drop(s._tx);
        let _ = s.worker.await;
    }
    Ok(())
}

// 🦀 Sum two mono signals, clamped to [-1, 1] (mixing two sources can clip).
fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b.iter()).map(|(x, y)| (x + y).clamp(-1.0, 1.0)).collect()
}

// 🦀 The mixing worker: system audio drives the ~10s cadence; the mic is mixed in when present
//    (padded with silence if it lags), so a slow/empty mic never stalls the transcript. Each chunk
//    is resampled to 16 kHz from its OWN reported rate (the mic can arrive at a native rate).
async fn mix_worker(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Chunk>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    capture_mic: bool,
    transcriber: crate::transcribe::TranscriberState,
) {
    const WINDOW: usize = 16_000 * 10; // 10s of 16 kHz mono
    let mut sys: Vec<f32> = Vec::new();
    let mut mic: Vec<f32> = Vec::new();
    while let Some(c) = rx.recv().await {
        let s16 = crate::audio::resample_to_16k(&c.samples, c.rate);
        if c.is_mic {
            mic.extend(s16);
        } else {
            sys.extend(s16);
        }
        while sys.len() >= WINDOW {
            let a: Vec<f32> = sys.drain(..WINDOW).collect();
            let window = if capture_mic {
                let take = mic.len().min(WINDOW);
                let mut b: Vec<f32> = mic.drain(..take).collect();
                b.resize(WINDOW, 0.0); // pad with silence if the mic is behind
                mix(&a, &b)
            } else {
                a
            };
            transcribe_and_emit(&transcriber, &window, &on_event);
        }
    }
    if !sys.is_empty() {
        let window = if capture_mic && !mic.is_empty() {
            let n = sys.len().min(mic.len());
            mix(&sys[..n], &mic[..n])
        } else {
            sys
        };
        if !window.is_empty() {
            transcribe_and_emit(&transcriber, &window, &on_event);
        }
    }
    let _ = on_event.send(CaptureEvent::Stopped);
}

// 🦀 Transcribe one 16 kHz mono window in-process and emit a Chunk (sync; the std Mutex guard is
//    dropped before returning and never held across an await).
fn transcribe_and_emit(
    transcriber: &crate::transcribe::TranscriberState,
    samples_16k: &[f32],
    on_event: &tauri::ipc::Channel<CaptureEvent>,
) {
    let text = {
        let guard = match transcriber.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.as_ref() {
            Some(t) => t.transcribe_samples(samples_16k),
            None => return,
        }
    };
    match text {
        Ok(t) if !t.trim().is_empty() => {
            let _ = on_event.send(CaptureEvent::Chunk { text: t });
        }
        Ok(_) => {}
        Err(e) => {
            let _ = on_event.send(CaptureEvent::Error { message: e.to_string() });
        }
    }
}
