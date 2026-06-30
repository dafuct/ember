use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, Result};

pub const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin";
const MODEL_MIN_BYTES: u64 = 1_400_000_000;

pub fn model_filename() -> &'static str {
    "ggml-medium.bin"
}

pub fn model_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(format!("no app data dir: {e}")))?
        .join("models");
    Ok(dir.join(model_filename()))
}

pub fn model_present(app: &AppHandle) -> bool {
    model_path(app)
        .ok()
        .and_then(|p| std::fs::metadata(&p).ok())
        .map(|m| m.len() >= MODEL_MIN_BYTES)
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

pub async fn ensure_model(app: &AppHandle, on: &tauri::ipc::Channel<PrepProgress>) -> Result<PathBuf> {
    let path = model_path(app)?;
    if model_present(app) {
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
    let resp = client.get(MODEL_URL).send().await.map_err(AppError::from)?;
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
    if std::fs::metadata(&tmp).map(|m| m.len() < MODEL_MIN_BYTES).unwrap_or(true) {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::Other("model download was incomplete; please retry".into()));
    }
    std::fs::rename(&tmp, &path).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{model_filename, MODEL_URL};

    #[test]
    fn model_filename_is_medium() {
        assert_eq!(model_filename(), "ggml-medium.bin");
    }

    #[test]
    fn model_url_points_at_medium() {
        assert!(MODEL_URL.ends_with("ggml-medium.bin"));
        assert!(!MODEL_URL.contains(".en."));
        assert!(MODEL_URL.starts_with("https://"));
    }
}
