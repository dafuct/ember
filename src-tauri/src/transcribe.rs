use std::sync::{Arc, Mutex};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::{AppError, Result};

pub struct Transcriber {
    ctx: WhisperContext,
}

pub type TranscriberState = Arc<Mutex<Option<Transcriber>>>;

impl Transcriber {
    pub fn load(model_path: &str) -> Result<Self> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| AppError::Other(format!("failed to load whisper model: {e}")))?;
        Ok(Self { ctx })
    }

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
