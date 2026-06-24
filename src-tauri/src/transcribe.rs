// src-tauri/src/transcribe.rs — in-process Whisper STT (replaces the HTTP whisper-server path).
// whisper-rs compiles whisper.cpp (with Metal) into the binary, so transcription needs no
// external server, port, or sidecar — only the model file on disk (downloaded by `model.rs`).
use std::sync::{Arc, Mutex};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::{AppError, Result};

// 🦀 The loaded model context, kept in Tauri-managed state so it's loaded once (expensive) and
//    reused across capture windows + file imports.
pub struct Transcriber {
    ctx: WhisperContext,
}

pub type TranscriberState = Arc<Mutex<Option<Transcriber>>>;

impl Transcriber {
    /// Load the model from disk. Expensive — call once and keep the result in state.
    pub fn load(model_path: &str) -> Result<Self> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| AppError::Other(format!("failed to load whisper model: {e}")))?;
        Ok(Self { ctx })
    }

    /// Transcribe 16 kHz mono f32 samples to text. Empty input → empty string.
    pub fn transcribe_samples(&self, samples_16k_mono: &[f32]) -> Result<String> {
        if samples_16k_mono.is_empty() {
            return Ok(String::new());
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| AppError::Other(format!("whisper state error: {e}")))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state
            .full(params, samples_16k_mono)
            .map_err(|e| AppError::Other(format!("transcription failed: {e}")))?;
        let n = state
            .full_n_segments()
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut out = String::new();
        for i in 0..n {
            if let Ok(seg) = state.full_get_segment_text(i) {
                out.push_str(seg.trim());
                out.push(' ');
            }
        }
        Ok(out.trim().to_string())
    }
}
