use std::sync::{Arc, Mutex};

use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use crate::error::{AppError, Result};

pub struct Transcriber {
    ctx: WhisperContext,
    model_id: String,
}

pub type TranscriberState = Arc<Mutex<Option<Transcriber>>>;

const BEAM_SIZE: i32 = 5;

/// One transcription pass: the recognized text plus the language Whisper actually
/// used. `language` lets the live pipeline pin an auto-detected language after the
/// first window instead of re-detecting (and drifting uk↔ru) on every window.
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
}

pub fn resolve_language(lang: Option<&str>) -> &str {
    match lang {
        Some(s) if !s.trim().is_empty() => s,
        _ => "auto",
    }
}

/// Map a user-facing language choice to an explicit ISO code, or `None` when the
/// caller wants auto-detect. Blank and the literal "auto" both mean auto-detect.
// 🦀 Returns an owned String (not &str) because the caller stores it across many
// windows; borrowing the input would tie it to the caller's short-lived argument.
pub fn normalize_language(lang: Option<&str>) -> Option<String> {
    match lang {
        Some(s) if !s.trim().is_empty() && s.trim() != "auto" => Some(s.trim().to_string()),
        _ => None,
    }
}

pub fn initial_prompt_for(lang: &str) -> Option<&'static str> {
    match lang {
        "uk" => Some("Це розшифровка ділової зустрічі українською мовою."),
        _ => None,
    }
}

/// Combine the per-language priming prompt with carried-over text from the previous
/// window so Whisper keeps terminology and sentence flow across the fixed capture
/// windows instead of restarting cold (and mis-splitting words) each time.
pub fn build_initial_prompt(lang: &str, context: Option<&str>) -> Option<String> {
    let base = initial_prompt_for(lang);
    let ctx = context.map(str::trim).filter(|s| !s.is_empty());
    match (base, ctx) {
        (Some(b), Some(c)) => Some(format!("{b} {c}")),
        (Some(b), None) => Some(b.to_string()),
        (None, Some(c)) => Some(c.to_string()),
        (None, None) => None,
    }
}

fn n_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|c| c.get().min(8))
        .unwrap_or(4) as i32
}

impl Transcriber {
    pub fn load(model_path: &str, model_id: &str) -> Result<Self> {
        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map_err(|e| AppError::Other(format!("failed to load whisper model: {e}")))?;
        Ok(Self {
            ctx,
            model_id: model_id.to_string(),
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn transcribe_samples(
        &self,
        samples_16k_mono: &[f32],
        lang: Option<&str>,
        context: Option<&str>,
    ) -> Result<Transcription> {
        if samples_16k_mono.is_empty() {
            return Ok(Transcription { text: String::new(), language: None });
        }
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| AppError::Other(format!("whisper state error: {e}")))?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: BEAM_SIZE,
            patience: -1.0,
        });
        let language = resolve_language(lang);
        params.set_language(Some(language));
        if let Some(prompt) = build_initial_prompt(language, context) {
            params.set_initial_prompt(&prompt);
        }
        // Anti-hallucination gates. On low-confidence audio (silence, noise, music)
        // Whisper otherwise emits its most-likely training phrase; these thresholds
        // make it drop such segments, and the temperature fallback ladder retries a
        // garbled decode at higher temperature before accepting it.
        params.set_suppress_blank(true);
        params.set_suppress_nst(true); // suppress non-speech tokens ([music], etc.)
        params.set_no_speech_thold(0.6);
        params.set_entropy_thold(2.4);
        params.set_logprob_thold(-1.0);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.2);
        params.set_n_threads(n_threads());
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
        // Report the language Whisper settled on (a negative id means "none") so the
        // caller can pin it for later windows instead of re-detecting each time.
        let language = state
            .full_lang_id_from_state()
            .ok()
            .filter(|&id| id >= 0)
            .and_then(get_lang_str)
            .map(str::to_string);
        Ok(Transcription { text: out.trim().to_string(), language })
    }
}

#[cfg(test)]
mod tests {
    use super::{build_initial_prompt, initial_prompt_for, normalize_language, resolve_language};

    #[test]
    fn resolve_language_defaults_to_auto() {
        assert_eq!(resolve_language(None), "auto");
        assert_eq!(resolve_language(Some("")), "auto");
        assert_eq!(resolve_language(Some("   ")), "auto");
    }

    #[test]
    fn normalize_language_treats_blank_and_auto_as_none() {
        assert_eq!(normalize_language(None), None);
        assert_eq!(normalize_language(Some("")), None);
        assert_eq!(normalize_language(Some("   ")), None);
        assert_eq!(normalize_language(Some("auto")), None);
        assert_eq!(normalize_language(Some("uk")), Some("uk".to_string()));
        assert_eq!(normalize_language(Some(" en ")), Some("en".to_string()));
    }

    #[test]
    fn build_initial_prompt_merges_base_and_context() {
        // uk has a base prompt; context is appended after it.
        let merged = build_initial_prompt("uk", Some("  попередній текст  ")).unwrap();
        assert!(merged.starts_with("Це розшифровка"));
        assert!(merged.ends_with("попередній текст"));

        // uk with no usable context keeps just the base prompt.
        assert_eq!(
            build_initial_prompt("uk", None).as_deref(),
            initial_prompt_for("uk"),
        );
        assert_eq!(build_initial_prompt("uk", Some("   ")).as_deref(), initial_prompt_for("uk"));

        // A language without a base prompt still carries context forward.
        assert_eq!(build_initial_prompt("en", Some("prior words")).as_deref(), Some("prior words"));

        // Nothing to prime with → no prompt at all.
        assert_eq!(build_initial_prompt("en", None), None);
        assert_eq!(build_initial_prompt("auto", Some("")), None);
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
