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
    // 🦀 Gmail returns the message's labels (incl. CATEGORY_* tabs) at the top level
    //    in format=metadata. `default` makes it an empty Vec when the key is absent.
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
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

/// What the UI consumes for the inbox preview. Also carries the M6 scoring signals;
/// the frontend only reads `category` (the rest are persisted for re-scoring).
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
    /// Filled by the scorer at sync time (empty on the raw Gmail-fetch path).
    pub category: String,
}

// 🦀 `format=full` response — wraps the top-level MIME part.
#[derive(Debug, Deserialize)]
pub struct FullMessage {
    pub payload: MessagePart,
}

// 🦀 A single MIME part in the tree.  `parts` may be empty (leaf) or hold
//    child parts (multipart/*).  `#[serde(default)]` means "use Default::default()
//    if the key is absent in the JSON" — handy because Gmail omits both `body`
//    and `parts` on multipart containers and leaf parts respectively.
#[derive(Debug, Deserialize)]
pub struct MessagePart {
    #[serde(rename = "mimeType", default)]
    pub mime_type: String,
    // 🦀 `format=full` includes the part's headers; `default` → empty Vec when absent.
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub body: PartBody,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
}

// 🦀 `Default` lets `#[serde(default)]` on the parent field create an empty
//    `PartBody { data: "" }` when the JSON has no `"body"` key.
#[derive(Debug, Default, Deserialize)]
pub struct PartBody {
    #[serde(default)]
    pub data: String,
}

/// What a reply needs from the original message: threading headers + the quoted text.
#[derive(Debug, Serialize)]
pub struct ReplyContext {
    pub message_id: String,
    pub references: String,
    pub quoted_text: String,
}

/// A draft reference: the draft's own id plus the id of its underlying message
/// (drafts and messages have *different* ids; editing/sending needs the draft id).
// 🦀 A plain struct we build by hand from Gmail's nested JSON — not `Deserialize`,
//    because the wire shape nests the message id one level down (mapped in mod.rs).
#[derive(Debug, Clone, PartialEq)]
pub struct DraftRef {
    pub id: String,
    pub message_id: String,
}

/// One draft's editable content, sent to the frontend to seed the compose editor.
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

/// The subset of the `users.messages.modify` response we use: the id and the
/// label set after the change. We don't request `payload`, so we don't model it —
/// keeping this type small means the parse never fails on a missing `payload`.
#[derive(Debug, Deserialize)]
pub struct ModifiedMessage {
    pub id: String,
    // 🦀 `default` makes serde fill an empty Vec if Gmail omits `labelIds`
    //    (it shouldn't, but this keeps the deserialize total/robust).
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
}
