use std::sync::{Arc, Mutex};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::{AppError, Result};

pub struct Transcriber {
    ctx: WhisperContext,
}

pub type TranscriberState = Arc<Mutex<Option<Transcriber>>>;

const BEAM_SIZE: i32 = 5;

pub fn resolve_language(lang: Option<&str>) -> &str {
    match lang {
        Some(s) if !s.trim().is_empty() => s,
        _ => "auto",
    }
}

pub fn initial_prompt_for(lang: &str) -> Option<&'static str> {
    match lang {
        "uk" => Some("Це розшифровка ділової зустрічі українською мовою."),
        _ => None,
    }
}

fn n_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|c| c.get().min(8))
        .unwrap_or(4) as i32
}

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
        params.set_language(Some("auto"));
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

#[cfg(test)]
mod tests {
    use super::{initial_prompt_for, resolve_language};

    #[test]
    fn resolve_language_defaults_to_auto() {
        assert_eq!(resolve_language(None), "auto");
        assert_eq!(resolve_language(Some("")), "auto");
        assert_eq!(resolve_language(Some("   ")), "auto");
    }

    #[test]
    fn resolve_language_passes_codes_through() {
        assert_eq!(resolve_language(Some("uk")), "uk");
        assert_eq!(resolve_language(Some("en")), "en");
    }

    #[test]
    fn ukrainian_prompt_is_cyrillic() {
        let p = initial_prompt_for("uk").expect("uk prompt present");
        assert!(p.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)));
    }

    #[test]
    fn non_ukrainian_has_no_prompt() {
        assert!(initial_prompt_for("en").is_none());
        assert!(initial_prompt_for("auto").is_none());
        assert!(initial_prompt_for("de").is_none());
    }
}
