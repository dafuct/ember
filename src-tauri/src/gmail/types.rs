use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct Profile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    #[serde(rename = "messagesTotal", default)]
    pub messages_total: u64,
    #[serde(rename = "historyId", default)]
    pub history_id: String,
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

// 🦀 The users.history.list response. `#[serde(default)]` lets serde fill in empty
//    Vecs / None when Gmail omits a field on a given page.
#[derive(Debug, Deserialize)]
pub struct HistoryResponse {
    #[serde(default)]
    pub history: Vec<HistoryRecord>,
    #[serde(rename = "historyId", default)]
    pub history_id: String,
    #[serde(rename = "nextPageToken", default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryRecord {
    #[serde(rename = "messagesAdded", default)]
    pub messages_added: Vec<HistoryMessage>,
    #[serde(rename = "messagesDeleted", default)]
    pub messages_deleted: Vec<HistoryMessage>,
    #[serde(rename = "labelsAdded", default)]
    pub labels_added: Vec<HistoryLabelChange>,
    #[serde(rename = "labelsRemoved", default)]
    pub labels_removed: Vec<HistoryLabelChange>,
}

// 🦀 messagesAdded / messagesDeleted entries wrap a single message.
#[derive(Debug, Deserialize)]
pub struct HistoryMessage {
    pub message: HistoryMessageRef,
}

// 🦀 labelsAdded / labelsRemoved entries carry the message AND which labels changed.
#[derive(Debug, Deserialize)]
pub struct HistoryLabelChange {
    pub message: HistoryMessageRef,
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryMessageRef {
    pub id: String,
    #[serde(rename = "threadId", default)]
    pub thread_id: String,
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
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
