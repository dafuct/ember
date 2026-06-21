// src-tauri/src/capture.rs — live macOS audio capture (cpal) → streamed transcript (M24).
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
