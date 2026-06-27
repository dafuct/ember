use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum CaptureEvent {
    Chunk { text: String },
    Error { message: String },
    Stopped,
}
