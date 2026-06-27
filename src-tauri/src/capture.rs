// src-tauri/src/capture.rs — the shared streamed-capture event type emitted to the frontend.
// Live capture itself is native (ScreenCaptureKit) in `syscapture.rs`; this enum is the wire
// format it streams over the Tauri Channel.
use serde::Serialize;

// 🦀 The streamed capture events. `#[serde(tag = "type")]` serializes each variant as
//    `{ "type": "Chunk", "text": "…" }`, so the frontend can switch on `type`.
#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum CaptureEvent {
    Chunk { text: String },
    Error { message: String },
    Stopped,
}
