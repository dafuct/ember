use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, Result};

pub const MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";
pub const DEFAULT_MODEL_ID: &str = "medium";

pub struct ModelSpec {
    pub id: &'static str,
    pub filename: &'static str,
    pub min_bytes: u64,
    pub label: &'static str,
}

impl ModelSpec {
    pub fn url(&self) -> String {
        format!("{MODEL_BASE_URL}{}", self.filename)
    }
}

pub static MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "medium",
        filename: "ggml-medium.bin",
        min_bytes: 1_400_000_000,
        label: "Standard — medium (1.5 GB)",
    },
    ModelSpec {
        id: "large-v3-turbo",
        filename: "ggml-large-v3-turbo.bin",
        min_bytes: 1_500_000_000,
        label: "High accuracy — large-v3-turbo (1.6 GB)",
    },
];

pub fn model_spec(id: &str) -> &'static ModelSpec {
    MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[0])
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(format!("no app data dir: {e}")))?
        .join("models");
    Ok(dir.join(model_spec(id).filename))
}

pub fn model_present(app: &AppHandle, id: &str) -> bool {
    let min = model_spec(id).min_bytes;
    model_path(app, id)
        .ok()
        .and_then(|p| std::fs::metadata(&p).ok())
        .map(|m| m.len() >= min)
        .unwrap_or(false)
}

#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum PrepProgress {
    Downloading { percent: u8 },
    Loading,
    Ready,
    Error { message: String },
}

pub async fn ensure_model(
    app: &AppHandle,
    id: &str,
    on: &tauri::ipc::Channel<PrepProgress>,
) -> Result<PathBuf> {
    let spec = model_spec(id);
    let path = model_path(app, id)?;
    if model_present(app, id) {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Other(e.to_string()))?;
    }
    let tmp = path.with_extension("part");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(AppError::from)?;
    let resp = client.get(spec.url()).send().await.map_err(AppError::from)?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!("model download failed: HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last: u8 = 0;
    let mut file = std::fs::File::create(&tmp).map_err(|e| AppError::Other(e.to_string()))?;
    use futures::StreamExt;
    use std::io::Write;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(AppError::from)?;
        file.write_all(&chunk).map_err(|e| AppError::Other(e.to_string()))?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            let pct = ((downloaded * 100) / total) as u8;
            if pct != last {
                last = pct;
                let _ = on.send(PrepProgress::Downloading { percent: pct });
            }
        }
    }
    file.flush().map_err(|e| AppError::Other(e.to_string()))?;
    drop(file);
    if std::fs::metadata(&tmp).map(|m| m.len() < spec.min_bytes).unwrap_or(true) {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::Other("model download was incomplete; please retry".into()));
    }
    std::fs::rename(&tmp, &path).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_medium() {
        let s = model_spec("medium");
        assert_eq!(s.id, "medium");
        assert_eq!(s.filename, "ggml-medium.bin");
        assert!(s.url().ends_with("ggml-medium.bin"));
        assert!(s.url().starts_with("https://"));
        assert!(s.min_bytes < 1_533_763_059); // real medium size on HF
    }

    #[test]
    fn turbo_model_spec_is_large_v3_turbo() {
        let s = model_spec("large-v3-turbo");
        assert_eq!(s.filename, "ggml-large-v3-turbo.bin");
        assert!(s.url().ends_with("ggml-large-v3-turbo.bin"));
        assert!(s.min_bytes > 1_000_000_000);
        assert!(s.min_bytes < 1_624_555_275); // real turbo size on HF
    }

    #[test]
    fn unknown_or_empty_id_falls_back_to_medium() {
        assert_eq!(model_spec("nope").id, "medium");
        assert_eq!(model_spec("").id, "medium");
    }

    #[test]
    fn no_english_only_models() {
        for s in MODELS {
            assert!(!s.filename.contains(".en."));
            assert!(s.url().starts_with("https://"));
        }
    }
}
