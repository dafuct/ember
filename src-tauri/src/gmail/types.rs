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
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
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

#[derive(Debug, Deserialize)]
pub struct HistoryMessage {
    pub message: HistoryMessageRef,
}

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessagePreview {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub internal_date: i64,
    pub label_ids: Vec<String>,
    pub to_addr: String,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FullMessage {
    pub payload: MessagePart,
}

#[derive(Debug, Deserialize)]
pub struct MessagePart {
    #[serde(rename = "mimeType", default)]
    pub mime_type: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub body: PartBody,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PartBody {
    #[serde(default)]
    pub data: String,
    #[serde(rename = "attachmentId", default)]
    pub attachment_id: Option<String>,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct ReplyContext {
    pub message_id: String,
    pub references: String,
    pub quoted_text: String,
    pub to: String,
    pub cc: String,
    pub attachments: Vec<AttachmentMeta>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DraftRef {
    pub id: String,
    pub message_id: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DraftContent {
    pub draft_id: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub thread_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModifiedMessage {
    pub id: String,
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Label {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<LabelColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabelColor {
    #[serde(rename = "textColor", default)]
    pub text: String,
    #[serde(rename = "backgroundColor", default)]
    pub background: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    pub attachment_id: String,
}
