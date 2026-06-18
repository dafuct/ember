// 🦀 `serde::Deserialize` lets Rust automatically generate code that reads JSON
//    (or any serde format) into this struct.  No manual parsing needed — the
//    macro inspects each field at compile time and builds an efficient parser.
//    `Serialize` is the mirror trait: it generates code to *write* the struct
//    out as JSON, needed here for `MessagePreview` which travels to the frontend.
use serde::{Deserialize, Serialize};

// 🦀 `#[serde(rename = "emailAddress")]` tells serde to look for the JSON key
//    "emailAddress" when deserializing into `email_address`.  Gmail's API uses
//    camelCase; Rust convention is snake_case.  This bridges the two worlds
//    without changing either the JSON contract or the Rust naming style.
#[derive(Debug, Deserialize)]
pub struct Profile {
    #[serde(rename = "emailAddress")]
    pub email_address: String,
    // 🦀 `#[serde(default)]` means: if "messagesTotal" is absent in the JSON,
    //    use `u64::default()` (which is `0`) instead of failing.  Useful when
    //    the server omits optional fields rather than sending `null`.
    #[serde(rename = "messagesTotal", default)]
    pub messages_total: u64,
}

#[derive(Debug, Deserialize)]
pub struct MessageList {
    // 🦀 `#[serde(default)]` on a `Vec` gives an empty vector when "messages"
    //    is absent — e.g. an empty inbox returns `{}` rather than `{"messages":[]}`.
    #[serde(default)]
    pub messages: Vec<MessageRef>,
}

#[derive(Debug, Deserialize)]
pub struct MessageRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct RawMessage {
    pub id: String,
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
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
}
