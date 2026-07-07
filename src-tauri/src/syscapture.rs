use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

use crate::capture::CaptureEvent;
use crate::error::{AppError, Result};

extern "C" {
    fn ember_syscapture_start(
        capture_mic: c_int,
        cb: extern "C" fn(*mut c_void, *const f32, c_int, f64, c_int),
        stop_cb: extern "C" fn(*mut c_void, *const c_char),
        ctx: *mut c_void,
        err_out: *mut c_char,
        err_len: c_int,
    ) -> *mut c_void;
    fn ember_syscapture_stop(handle: *mut c_void);
}

struct Chunk {
    is_mic: bool,
    samples: Vec<f32>,
    rate: u32,
}

enum Feed {
    Audio(Chunk),
    Died(String),
}

struct Handle(*mut c_void);
unsafe impl Send for Handle {}

pub struct SysSession {
    handle: Handle,
    _tx: Box<tokio::sync::mpsc::UnboundedSender<Feed>>,
    worker: tokio::task::JoinHandle<()>,
}

pub type SysCaptureState = Arc<Mutex<Option<SysSession>>>;

extern "C" fn on_audio(ctx: *mut c_void, mono: *const f32, frames: c_int, rate: f64, is_mic: c_int) {
    if ctx.is_null() || mono.is_null() || frames <= 0 {
        return;
    }
    let tx = unsafe { &*(ctx as *const tokio::sync::mpsc::UnboundedSender<Feed>) };
    let mut samples = unsafe { std::slice::from_raw_parts(mono, frames as usize) }.to_vec();
    // A single NaN/inf sample (seen with misdeclared USB-mic formats) would poison the whole
    // mixed window and silently mute the transcriber — degrade that sample to silence instead.
    for s in &mut samples {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    let _ = tx.send(Feed::Audio(Chunk { is_mic: is_mic != 0, samples, rate: rate.max(1.0) as u32 }));
}

extern "C" fn on_stream_died(ctx: *mut c_void, message: *const c_char) {
    if ctx.is_null() {
        return;
    }
    let tx = unsafe { &*(ctx as *const tokio::sync::mpsc::UnboundedSender<Feed>) };
    let msg = if message.is_null() {
        String::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy().into_owned()
    };
    let _ = tx.send(Feed::Died(msg));
}

fn err_buf_to_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

#[tauri::command]
pub async fn start_system_capture(
    capture_mic: bool,
    language: Option<String>,
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
        // A stream that died on its own (screen lock, replayd restart) leaves a finished
        // worker behind; reclaim it so the next Record works instead of "already capturing".
        let stale = {
            let mut guard =
                state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
            match guard.as_ref() {
                Some(s) if s.worker.is_finished() => guard.take(),
                Some(_) => return Err(AppError::Other("already capturing".into())),
                None => None,
            }
        };
        if let Some(s) = stale {
            let h = s.handle.0 as usize;
            let _ = tokio::task::spawn_blocking(move || unsafe {
                ember_syscapture_stop(h as *mut c_void)
            })
            .await;
        }
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Feed>();
    let boxed_tx = Box::new(tx);
    let ctx_addr = (&*boxed_tx) as *const tokio::sync::mpsc::UnboundedSender<Feed> as usize;
    let mic = if capture_mic { 1 } else { 0 };

    let started = tokio::task::spawn_blocking(move || {
        let mut err = [0u8; 320];
        let handle = unsafe {
            ember_syscapture_start(
                mic,
                on_audio,
                on_stream_died,
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
        Err(msg) => return Err(AppError::Other(msg)),
    };

    let worker = tokio::spawn(mix_worker(rx, on_event, capture_mic, language, transcriber));

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

#[tauri::command]
pub async fn stop_system_capture(state: tauri::State<'_, SysCaptureState>) -> Result<()> {
    let session = {
        let mut guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
        guard.take()
    };
    if let Some(s) = session {
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

fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b.iter()).map(|(x, y)| (x + y).clamp(-1.0, 1.0)).collect()
}

// Whisper is trained on 30-second chunks; transcribing in 30s windows (rather than
// the old 10s) stops words being split mid-boundary into garbage like "Тестплафема".
const WINDOW: usize = 16_000 * 30;
// Below this RMS a window is treated as silence and never sent to Whisper — silent
// windows are what made it hallucinate "Дякую за перегляд!" over and over.
const SILENCE_RMS: f32 = 0.005;
// How much of the previous window's text to carry forward as decoding context.
// Whisper truncates an over-long prompt itself, so this only needs to be "recent".
const CONTEXT_TAIL_CHARS: usize = 200;

/// Trailing `max_chars` characters of `s`, cut on a char boundary.
fn context_tail(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    s.chars().skip(n - max_chars).collect()
}

/// Transcribe one window, then fold the result back into the running state:
/// pin an auto-detected language (D) and remember the tail for context (C).
fn process_window(
    transcriber: &crate::transcribe::TranscriberState,
    window: &[f32],
    effective_lang: &mut Option<String>,
    context: &mut String,
    on_event: &tauri::ipc::Channel<CaptureEvent>,
) {
    let ctx = if context.is_empty() { None } else { Some(context.as_str()) };
    if let Some(tr) = transcribe_and_emit(transcriber, window, effective_lang.as_deref(), ctx, on_event) {
        *context = context_tail(&tr.text, CONTEXT_TAIL_CHARS);
        // Learn the language once (auto-detect only), then reuse it so later windows
        // don't independently re-detect and drift between Ukrainian and Russian.
        if effective_lang.is_none() {
            if let Some(lang) = tr.language {
                *effective_lang = Some(lang);
            }
        }
    }
}

async fn mix_worker(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Feed>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    capture_mic: bool,
    language: Option<String>,
    transcriber: crate::transcribe::TranscriberState,
) {
    // `None` means auto-detect; the first transcribed window pins it (see process_window).
    let mut effective_lang = crate::transcribe::normalize_language(language.as_deref());
    let mut context = String::new();
    let mut sys: Vec<f32> = Vec::new();
    let mut mic: Vec<f32> = Vec::new();
    while let Some(feed) = rx.recv().await {
        let c = match feed {
            Feed::Audio(c) => c,
            Feed::Died(msg) => {
                let message = if msg.is_empty() {
                    "capture stopped by macOS — quit and reopen Ember, then Record again".into()
                } else {
                    format!("capture stopped: {msg}")
                };
                let _ = on_event.send(CaptureEvent::Error { message });
                break;
            }
        };
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
                b.resize(WINDOW, 0.0);
                mix(&a, &b)
            } else {
                a
            };
            process_window(&transcriber, &window, &mut effective_lang, &mut context, &on_event);
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
            process_window(&transcriber, &window, &mut effective_lang, &mut context, &on_event);
        }
    }
    let _ = on_event.send(CaptureEvent::Stopped);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_audio_zeroes_non_finite_samples() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Feed>();
        let boxed = Box::new(tx);
        let samples = [0.5f32, f32::INFINITY, f32::NAN, -0.25, f32::NEG_INFINITY];
        on_audio(
            (&*boxed) as *const tokio::sync::mpsc::UnboundedSender<Feed> as *mut c_void,
            samples.as_ptr(),
            samples.len() as c_int,
            32000.0,
            1,
        );
        let Feed::Audio(chunk) = rx.try_recv().expect("chunk delivered") else {
            panic!("expected an audio chunk");
        };
        assert!(chunk.is_mic);
        assert_eq!(chunk.samples, vec![0.5, 0.0, 0.0, -0.25, 0.0]);
        assert_eq!(chunk.rate, 32000);
    }

    #[test]
    fn on_stream_died_forwards_the_error_message() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Feed>();
        let boxed = Box::new(tx);
        let ctx = (&*boxed) as *const tokio::sync::mpsc::UnboundedSender<Feed> as *mut c_void;
        let msg = std::ffi::CString::new("stream stopped by the system").unwrap();
        on_stream_died(ctx, msg.as_ptr());
        on_stream_died(ctx, std::ptr::null());
        let Feed::Died(first) = rx.try_recv().expect("death delivered") else {
            panic!("expected a death notice");
        };
        assert_eq!(first, "stream stopped by the system");
        let Feed::Died(second) = rx.try_recv().expect("null message tolerated") else {
            panic!("expected a death notice");
        };
        assert_eq!(second, "");
    }

    #[test]
    fn mix_stays_finite_and_clamped() {
        let mixed = mix(&[0.9, -0.9, 0.5], &[0.9, -0.9, -0.25]);
        assert_eq!(mixed, vec![1.0, -1.0, 0.25]);
        assert!(mixed.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn context_tail_keeps_recent_chars_on_a_char_boundary() {
        // Shorter than the cap → returned whole.
        assert_eq!(context_tail("hello", 200), "hello");
        // Longer → only the trailing `max_chars` survive.
        assert_eq!(context_tail("abcdef", 3), "def");
        // Multi-byte Cyrillic must be cut on a char boundary, not a byte one.
        let s = "абвгд";
        assert_eq!(context_tail(s, 2), "гд");
        assert_eq!(context_tail(s, 0), "");
    }
}

fn transcribe_and_emit(
    transcriber: &crate::transcribe::TranscriberState,
    samples_16k: &[f32],
    lang: Option<&str>,
    context: Option<&str>,
    on_event: &tauri::ipc::Channel<CaptureEvent>,
) -> Option<crate::transcribe::Transcription> {
    // Silence gate (A): a near-silent window is the classic hallucination trigger,
    // so drop it before it ever reaches Whisper.
    if crate::audio::rms(samples_16k) < SILENCE_RMS {
        return None;
    }
    let result = {
        let guard = transcriber.lock().ok()?;
        match guard.as_ref() {
            Some(t) => t.transcribe_samples(samples_16k, lang, context),
            None => return None,
        }
    };
    match result {
        Ok(tr) if !tr.text.trim().is_empty() => {
            let _ = on_event.send(CaptureEvent::Chunk { text: tr.text.clone() });
            Some(tr)
        }
        Ok(_) => None,
        Err(e) => {
            let _ = on_event.send(CaptureEvent::Error { message: e.to_string() });
            None
        }
    }
}
