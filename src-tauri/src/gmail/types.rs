use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Profile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    #[serde(rename = "messagesTotal", default)]
    pub messages_total: u64,
}

#[derive(Debug, Deserialize)]
pub struct MessageList {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    // 🦀 Gmail includes this key only when more pages exist. `default` makes the
    //    field `None` when the key is absent (without it serde errors on a missing field).
    #[serde(rename = "nextPageToken", default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct RawMessage {
    pub id: String,
    #[serde(rename = "threadId", default)]
    pub thread_id: String,
    // 🦀 Gmail sends `internalDate` as a STRING of milliseconds-since-epoch; we keep
    //    it as String here and parse to i64 in the client.
    #[serde(rename = "internalDate", default)]
    pub internal_date: String,
    #[serde(default)]
    pub snippet: String,
    pub payload: Payload,
}

#[derive(Debug, Deserialize)]
pub struct Payload {
    #[serde(default)]
    pub headers: Vec<Header>,
}

#[derive(Debug, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// What the UI consumes for the inbox preview.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessagePreview {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub internal_date: i64,
}
