use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

use crate::capture::CaptureEvent;
use crate::error::{AppError, Result};

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

struct Chunk {
    is_mic: bool,
    samples: Vec<f32>,
    rate: u32,
}

struct Handle(*mut c_void);
unsafe impl Send for Handle {}

pub struct SysSession {
    handle: Handle,
    _tx: Box<tokio::sync::mpsc::UnboundedSender<Chunk>>,
    worker: tokio::task::JoinHandle<()>,
}

pub type SysCaptureState = Arc<Mutex<Option<SysSession>>>;

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
        let guard = state.lock().map_err(|_| AppError::Other("capture lock poisoned".into()))?;
        if guard.is_some() {
            return Err(AppError::Other("already capturing".into()));
        }
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Chunk>();
    let boxed_tx = Box::new(tx);
    let ctx_addr = (&*boxed_tx) as *const tokio::sync::mpsc::UnboundedSender<Chunk> as usize;
    let mic = if capture_mic { 1 } else { 0 };

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

async fn mix_worker(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Chunk>,
    on_event: tauri::ipc::Channel<CaptureEvent>,
    capture_mic: bool,
    language: Option<String>,
    transcriber: crate::transcribe::TranscriberState,
) {
    const WINDOW: usize = 16_000 * 10;
    let lang = language.as_deref();
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
                b.resize(WINDOW, 0.0);
                mix(&a, &b)
            } else {
                a
            };
            transcribe_and_emit(&transcriber, &window, lang, &on_event);
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
            transcribe_and_emit(&transcriber, &window, lang, &on_event);
        }
    }
    let _ = on_event.send(CaptureEvent::Stopped);
}

fn transcribe_and_emit(
    transcriber: &crate::transcribe::TranscriberState,
    samples_16k: &[f32],
    lang: Option<&str>,
    on_event: &tauri::ipc::Channel<CaptureEvent>,
) {
    let text = {
        let guard = match transcriber.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.as_ref() {
            Some(t) => t.transcribe_samples(samples_16k, lang),
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
