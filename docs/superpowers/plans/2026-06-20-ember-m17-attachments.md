# M17 Attachments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user view + save attachments on received mail, and attach files to a new message or reply, in Ember (the local-first Tauri/React Gmail client).

**Architecture:** Receiving rides the existing `format=full` body fetch — a sibling `collect_attachments` MIME walk surfaces an attachment list on the `MessageBody` the reading pane already loads; bytes are fetched on click via a new `get_attachment` client method + a `download_attachment` command that writes with Rust `std::fs` (the path comes from a native Save dialog). Sending adds a **new** pure `build_multipart_rfc822` beside the unchanged `build_rfc822` (drafts/attachment-free sends are byte-for-byte unchanged); `send_email` reads the picked file paths, enforces a 25 MB cap, and injects a `SystemTime`-derived boundary so `mime.rs` stays clock-free. Drafts stay text-only.

**Tech Stack:** Rust (reqwest, serde, base64, Tauri 2, **new `tauri-plugin-dialog`**; wiremock tests), React 19 + TypeScript + Vite, lucide-react, **new `@tauri-apps/plugin-dialog`**.

**Learning mode (every task):** the repo owner is learning Rust — all Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept (not just intent), and each task ends with a 2-3 sentence plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Frontend testing note:** this repo has **no TypeScript/React test harness** (consistent through M16). Frontend tasks are verified by `npm run build` (which runs `tsc` typecheck + `vite build`) and, at the end, a browser-maket screenshot — not unit tests. Backend tasks use TDD with `wiremock`/pure unit tests.

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m17-attachments-design.md`

---

## File structure

**Backend (Rust, `src-tauri/`):**
- `src/gmail/types.rs` — *modify*: extend `PartBody` (+`attachment_id`, +`size`) and `MessagePart` (+`filename`); add `AttachmentMeta`.
- `src/gmail/mod.rs` — *modify*: add `decode_b64url_bytes` + refactor `decode_b64url`; add `collect_attachments`; extend `RawBody` (+`attachments`) and `get_message_body`; add `get_attachment` + its `AttachmentResponse` wire struct; import `AttachmentMeta` + `AppError`.
- `tests/gmail_test.rs` — *modify*: add two wiremock tests (attachment enumeration; `get_attachment` byte decode).
- `src/mime.rs` — *modify*: add `OutgoingAttachment`, `MAX_ATTACHMENT_BYTES`, `mime_for_ext`; extract `outgoing_headers` + `wrap76`; add `base64_bytes` + `build_multipart_rfc822`; add unit tests.
- `src/commands.rs` — *modify*: `MessageBody` (+`attachments`); `fetch_message_body`; add `download_attachment`; extend `send_email` (+`attachment_paths`).
- `src/lib.rs` — *modify*: register `download_attachment`; init the dialog plugin.
- `Cargo.toml` — *modify*: add `tauri-plugin-dialog`.
- `capabilities/default.json` — *modify*: add `dialog:default`.

**Frontend (`src/`, `package.json`):**
- `package.json` — *modify*: add `@tauri-apps/plugin-dialog`.
- `src/lib/api.ts` — *modify*: `Attachment` interface; `MessageBody.attachments`; gate `fetchMessageBody` + mock; `downloadAttachment`; `SendEmailPayload.attachment_paths` + `sendEmail`.
- `src/lib/attachments.ts` — *create*: `formatBytes`, `basename`.
- `src/lib/mock.ts` — *modify*: `mockMessageBody`, `mockPickFiles`.
- `src/components/ReadingPane.tsx` — *modify*: attachment strip + Save-As download.
- `src/components/ComposeModal.tsx` — *modify*: Attach button + file chips + send wiring + draft note.
- `src/styles/app.css` — *modify*: attachment-strip + attach-chip styles.

---

## Task 1: Receiving — enumerate attachment metadata

**Files:**
- Modify: `src-tauri/src/gmail/types.rs`
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/tests/gmail_test.rs` (end of file):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn get_message_body_enumerates_attachments() {
    let server = MockServer::start().await;
    let text = "See attached.";
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m3",
            "payload": {
                "mimeType": "multipart/mixed",
                "parts": [
                    { "mimeType": "text/plain", "body": { "data": b64url(text) } },
                    {
                        "mimeType": "application/pdf",
                        "filename": "report.pdf",
                        "body": { "attachmentId": "att1", "size": 1024 }
                    }
                ]
            }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let body = client.get_message_body("m3").await.unwrap();
    // text body still extracted, and the attachment part is enumerated (text part is not)
    assert_eq!(body.text.as_deref(), Some(text));
    assert_eq!(body.attachments.len(), 1);
    let a = &body.attachments[0];
    assert_eq!(a.filename, "report.pdf");
    assert_eq!(a.mime_type, "application/pdf");
    assert_eq!(a.size, 1024);
    assert_eq!(a.attachment_id, "att1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test get_message_body_enumerates_attachments`
Expected: FAIL to compile — `RawBody` has no field `attachments` (and `AttachmentMeta` doesn't exist).

- [ ] **Step 3: Extend the MIME types**

In `src-tauri/src/gmail/types.rs`, replace the `PartBody` struct with:

```rust
// 🦀 `Default` lets `#[serde(default)]` on the parent field create an empty
//    `PartBody` when the JSON has no `"body"` key.
#[derive(Debug, Default, Deserialize)]
pub struct PartBody {
    #[serde(default)]
    pub data: String,
    // 🦀 Attachment parts carry a separate handle instead of inline `data`. `Option`
    //    because text/html parts have none. rename: Gmail's JSON key is camelCase.
    #[serde(rename = "attachmentId", default)]
    pub attachment_id: Option<String>,
    // 🦀 Byte size of the part's content; `default` → 0 when Gmail omits it.
    #[serde(default)]
    pub size: i64,
}
```

In the same file, add a `filename` field to `MessagePart` (inside the existing struct, after `headers`):

```rust
    // 🦀 Non-empty only on attachment parts (the download filename); `default` → "".
    #[serde(default)]
    pub filename: String,
```

Append a new public type to `src-tauri/src/gmail/types.rs`:

```rust
/// One attachment on a received message: enough to list it and fetch its bytes.
// 🦀 `Serialize` (not `Deserialize`) — Tauri hands it to the frontend as JSON; we build
//    it by hand from the MIME walk, not from a single wire shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    pub attachment_id: String,
}
```

- [ ] **Step 4: Add the walk and wire it into `get_message_body`**

In `src-tauri/src/gmail/mod.rs`, add `AttachmentMeta` to the `use types::{ ... }` import list (the block that already imports `MessagePart`, `MessagePreview`, …).

Add `collect_attachments` directly below the existing `collect_body` function:

```rust
// 🦀 Sibling of `collect_body`: a recursive MIME walk gathering attachment parts.
//    A part is an attachment when it has a non-empty `filename` AND an `attachmentId`
//    (the handle for fetching its bytes separately). `out` is an out-param Vec to push into.
fn collect_attachments(part: &MessagePart, out: &mut Vec<AttachmentMeta>) {
    if !part.filename.is_empty() {
        if let Some(id) = &part.body.attachment_id {
            out.push(AttachmentMeta {
                filename: part.filename.clone(),
                mime_type: part.mime_type.clone(),
                size: part.body.size,
                attachment_id: id.clone(),
            });
        }
    }
    for child in &part.parts {
        collect_attachments(child, out);
    }
}
```

In the same file, add a field to `RawBody`:

```rust
/// Raw (un-sanitized) body extracted from a message.
pub struct RawBody {
    pub html: Option<String>,
    pub text: Option<String>,
    // 🦀 Attachments found on the message (metadata only — bytes fetched on demand).
    pub attachments: Vec<AttachmentMeta>,
}
```

Replace the body of `get_message_body` to also collect attachments (same single fetch):

```rust
    pub async fn get_message_body(&self, id: &str) -> Result<RawBody> {
        let url = format!("{}/gmail/v1/users/me/messages/{}?format=full", self.base_url, id);
        let full: FullMessage = self.get_json(&url).await?;
        let mut html = None;
        let mut text = None;
        collect_body(&full.payload, &mut html, &mut text);
        // 🦀 The same payload also yields the attachment list — no extra round-trip.
        let mut attachments = Vec::new();
        collect_attachments(&full.payload, &mut attachments);
        Ok(RawBody { html, text, attachments })
    }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --test gmail_test get_message_body_enumerates_attachments`
Expected: PASS. Then `cargo test --test gmail_test` — the existing body tests (`get_message_body_extracts_html_from_multipart`, `get_message_body_handles_simple_plaintext`) still PASS.

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo clippy --all-targets 2>&1 | tail -3
cd .. && git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m17): enumerate received-message attachments in the MIME walk"
```

**Rust recap to include:** how `#[serde(rename = "attachmentId", default)]` maps a camelCase JSON key onto a snake_case `Option` field that defaults to `None` when absent, and how out-param `&mut Vec` recursion mirrors the existing `collect_body` pattern.

---

## Task 2: Receiving — fetch one attachment's bytes (`get_attachment`)

**Files:**
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn get_attachment_decodes_base64url_bytes() {
    let server = MockServer::start().await;
    let payload = "PDF-BYTES-HERE";
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m3/attachments/att1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": payload.len(),
            "data": b64url(payload)
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let bytes = client.get_attachment("m3", "att1").await.unwrap();
    assert_eq!(bytes, payload.as_bytes());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test get_attachment_decodes_base64url_bytes`
Expected: FAIL to compile — no method `get_attachment`.

- [ ] **Step 3: Add a bytes decoder + the client method**

In `src-tauri/src/gmail/mod.rs`, change the error import from `use crate::error::Result;` to:

```rust
use crate::error::{AppError, Result};
```

Replace the existing `decode_b64url` with a bytes-first pair:

```rust
// 🦀 base64url-decode to raw BYTES (attachments are binary). `URL_SAFE_NO_PAD` is the
//    web-safe base64 variant Gmail uses. Returns None on empty/invalid input.
fn decode_b64url_bytes(data: &str) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data.trim().trim_end_matches('='))
        .ok()
}

// 🦀 Text parts decode to a String: bytes first, then lossily interpret as UTF-8 so
//    invalid sequences become U+FFFD rather than panicking.
fn decode_b64url(data: &str) -> Option<String> {
    decode_b64url_bytes(data).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}
```

Add the `get_attachment` method to the `impl GmailClient` block (e.g. right after `get_message_body`):

```rust
    /// Fetch one attachment's raw bytes. Gmail returns base64url `data` from a separate
    /// `messages/{id}/attachments/{attachmentId}` endpoint (the message payload carries only
    /// the handle). Returns the decoded bytes ready to write to disk.
    pub async fn get_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}/attachments/{}",
            self.base_url, message_id, attachment_id
        );
        let resp: AttachmentResponse = self.get_json(&url).await?;
        // 🦀 `ok_or_else` turns Option→Result, raising our AppError when the data was empty/invalid.
        decode_b64url_bytes(&resp.data)
            .ok_or_else(|| AppError::Other("attachment data was empty or not valid base64url".into()))
    }
```

Add the private wire struct near the other response structs at the bottom of the file (e.g. beside `DraftIdResponse`):

```rust
// 🦀 users.messages.attachments.get response: { size, data (base64url) }. We only need `data`.
#[derive(serde::Deserialize)]
struct AttachmentResponse {
    #[serde(default)]
    data: String,
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --test gmail_test get_attachment_decodes_base64url_bytes`
Expected: PASS. Then `cargo test` (all) — green (the `decode_b64url` refactor keeps the body tests passing).

- [ ] **Step 5: Commit**

```bash
cd src-tauri && cargo clippy --all-targets 2>&1 | tail -3
cd .. && git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m17): GmailClient::get_attachment (base64url bytes)"
```

**Rust recap to include:** how factoring `decode_b64url_bytes` out and having `decode_b64url` delegate keeps behavior identical (existing tests prove it) while adding a binary path, and how `Option::ok_or_else` bridges to the `Result`/`?` world.

---

## Task 3: Receiving — `download_attachment` command + body wiring

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

(No unit test — this is command glue over the TDD'd `get_attachment`. Verified by `cargo build` here and the maket/E2E later, consistent with how other command-layer glue in this repo is covered.)

- [ ] **Step 1: Add `attachments` to `MessageBody` and populate it**

In `src-tauri/src/commands.rs`, replace the `MessageBody` struct:

```rust
// 🦀 The body payload sent to the frontend. `is_html` tells the UI whether to render
//    `html` as sanitized HTML (sandboxed iframe) or as plain text.
#[derive(serde::Serialize)]
pub struct MessageBody {
    pub html: String,
    pub is_html: bool,
    pub blocked_images: bool,
    // 🦀 Attachment metadata for the reading-pane strip (bytes fetched on click). Empty when none.
    pub attachments: Vec<crate::gmail::types::AttachmentMeta>,
}
```

In `fetch_message_body`, set `attachments` in both return arms:

```rust
    if let Some(html) = raw.html {
        let (clean, blocked) = sanitize_html(&html, load_images);
        Ok(MessageBody { html: clean, is_html: true, blocked_images: blocked, attachments: raw.attachments })
    } else {
        Ok(MessageBody {
            html: raw.text.unwrap_or_default(),
            is_html: false,
            blocked_images: false,
            attachments: raw.attachments,
        })
    }
```

- [ ] **Step 2: Add the `download_attachment` command**

Add to `src-tauri/src/commands.rs` (near `fetch_message_body`):

```rust
/// Download one attachment to a path the user chose (via the frontend Save dialog).
/// DB-free: fetch the bytes from Gmail, then write them with std::fs.
// 🦀 `dest_path` arrives from JS (the native save-dialog result). The byte WRITE happens
//    here in Rust — Tauri commands have full OS access — so no `fs` capability is needed.
#[tauri::command]
pub async fn download_attachment(
    message_id: String,
    attachment_id: String,
    dest_path: String,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let bytes = client.get_attachment(&message_id, &attachment_id).await?;
    // 🦀 `map_err` converts the std::io::Error into our AppError so `?` can propagate it
    //    (io::Error has no `#[from]` impl on AppError, unlike reqwest/keyring/rusqlite).
    std::fs::write(&dest_path, &bytes)
        .map_err(|e| AppError::Other(format!("could not save attachment: {e}")))?;
    Ok(())
}
```

- [ ] **Step 3: Register the command**

In `src-tauri/src/lib.rs`, add to the `tauri::generate_handler![ ... ]` list (after `commands::fetch_message_body,`):

```rust
            commands::download_attachment,
```

- [ ] **Step 4: Build to verify**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: compiles clean (no warnings about unused `download_attachment`). Then `cargo clippy --all-targets 2>&1 | tail -3` — clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(m17): download_attachment command + attachments on MessageBody"
```

**Rust recap to include:** why `std::fs::write` needs an explicit `map_err` (no `#[from]` on `AppError` for `io::Error`) while reqwest errors flow through `?` automatically, and why the byte write lives in Rust rather than the JS layer.

---

## Task 4: Sending — multipart MIME builder (pure)

**Files:**
- Modify: `src-tauri/src/mime.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/mime.rs`:

```rust
    #[test]
    fn mime_for_ext_maps_known_and_falls_back() {
        assert_eq!(mime_for_ext("report.pdf"), "application/pdf");
        assert_eq!(mime_for_ext("PHOTO.JPG"), "image/jpeg");
        assert_eq!(mime_for_ext("data.unknownext"), "application/octet-stream");
        assert_eq!(mime_for_ext("noextension"), "application/octet-stream");
    }

    #[test]
    fn multipart_has_text_part_and_attachment_roundtrips() {
        let mut m = msg();
        m.body = "see file".into();
        let atts = vec![OutgoingAttachment {
            filename: "a.txt".into(),
            mime_type: "text/plain".into(),
            bytes: b"hello bytes".to_vec(),
        }];
        let out = build_multipart_rfc822(&m, &atts, "BOUND123");
        assert!(out.contains("Content-Type: multipart/mixed; boundary=\"BOUND123\""));
        assert!(out.contains("--BOUND123\r\n"));
        assert!(out.contains("Content-Disposition: attachment; filename=\"a.txt\""));
        assert!(out.trim_end().ends_with("--BOUND123--"));
        // the attachment's base64 decodes back to the original bytes
        let marker = "Content-Disposition: attachment; filename=\"a.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        let after = out.split(marker).nth(1).unwrap();
        let b64: String = after.split("\r\n--BOUND123--").next().unwrap().split("\r\n").collect();
        let decoded = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert_eq!(decoded, b"hello bytes");
    }

    #[test]
    fn multipart_encodes_non_ascii_filename() {
        let m = msg();
        let atts = vec![OutgoingAttachment {
            filename: "Звіт.pdf".into(),
            mime_type: "application/pdf".into(),
            bytes: b"x".to_vec(),
        }];
        let out = build_multipart_rfc822(&m, &atts, "B");
        assert!(out.contains("=?UTF-8?B?")); // RFC2047 encoded-word
        assert!(!out.contains("Звіт.pdf")); // raw non-ASCII name not emitted
    }

    #[test]
    fn multipart_sanitizes_crlf_in_filename() {
        let m = msg();
        let atts = vec![OutgoingAttachment {
            filename: "a\r\nContent-Type: evil".into(),
            mime_type: "text/plain".into(),
            bytes: b"x".to_vec(),
        }];
        let out = build_multipart_rfc822(&m, &atts, "B");
        for line in out.split("\r\n") {
            assert!(!line.starts_with("Content-Type: evil"));
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib mime`
Expected: FAIL to compile — `mime_for_ext`, `OutgoingAttachment`, `build_multipart_rfc822` don't exist.

- [ ] **Step 3: Add the type, constant, and helpers**

In `src-tauri/src/mime.rs`, add the attachment type after `OutgoingMessage`:

```rust
/// One file to attach to an outgoing message. `bytes` is the raw file content (the
/// command layer reads it from disk); `mime_type` is best-effort from the extension.
pub struct OutgoingAttachment {
    pub filename: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Total attachment byte cap (Gmail's UI limit). Larger payloads risk the ~35 MB raw
/// `messages.send` ceiling after base64 inflation, so we reject before encoding.
pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
```

Add `mime_for_ext`:

```rust
/// Best-effort MIME type from a filename's extension; `application/octet-stream` fallback.
// 🦀 `rsplit('.').next()` grabs the text after the last dot; `to_ascii_lowercase` makes
//    the match case-insensitive. Returns a `&'static str` — no allocation.
pub fn mime_for_ext(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}
```

- [ ] **Step 4: Extract shared helpers and add the multipart builder**

Replace `base64_body` with a shared `wrap76` + two thin wrappers:

```rust
// 🦀 Wrap an ASCII base64 string into 76-char lines joined by CRLF (RFC 2045).
//    Shared by the text body and binary attachment encoders.
fn wrap76(encoded: &str) -> String {
    encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

// 🦀 base64-encode the UTF-8 body and wrap (RFC 2045).
fn base64_body(body: &str) -> String {
    wrap76(&base64::engine::general_purpose::STANDARD.encode(body.as_bytes()))
}

// 🦀 base64-encode raw attachment bytes and wrap.
fn base64_bytes(bytes: &[u8]) -> String {
    wrap76(&base64::engine::general_purpose::STANDARD.encode(bytes))
}
```

Extract the shared header lines from `build_rfc822` into `outgoing_headers`, then make `build_rfc822` call it (output stays byte-identical — the existing tests prove it):

```rust
// 🦀 The address/subject/threading header lines shared by the plain and multipart builders.
//    Pulling them out keeps both builders DRY without changing the emitted bytes.
fn outgoing_headers(msg: &OutgoingMessage) -> Vec<String> {
    let mut headers: Vec<String> = Vec::new();
    headers.push(format!("From: {}", sanitize_header(&msg.from)));
    headers.push(format!("To: {}", msg.to.iter().map(|a| sanitize_header(a)).collect::<Vec<_>>().join(", ")));
    if !msg.cc.is_empty() {
        headers.push(format!("Cc: {}", msg.cc.iter().map(|a| sanitize_header(a)).collect::<Vec<_>>().join(", ")));
    }
    headers.push(format!("Subject: {}", encode_subject(&sanitize_header(&msg.subject))));
    if let Some(irt) = msg.in_reply_to.as_deref().filter(|s| !s.is_empty()) {
        headers.push(format!("In-Reply-To: {}", sanitize_header(irt)));
    }
    if let Some(refs) = msg.references.as_deref().filter(|s| !s.is_empty()) {
        headers.push(format!("References: {}", sanitize_header(refs)));
    }
    headers
}

/// Build the full RFC822 message: headers, a blank line, then the base64 body. Uses
/// CRLF line endings throughout (what SMTP/Gmail expect).
pub fn build_rfc822(msg: &OutgoingMessage) -> String {
    let mut headers = outgoing_headers(msg);
    headers.push("MIME-Version: 1.0".to_string());
    headers.push("Content-Type: text/plain; charset=\"utf-8\"".to_string());
    headers.push("Content-Transfer-Encoding: base64".to_string());
    format!("{}\r\n\r\n{}", headers.join("\r\n"), base64_body(&msg.body))
}

/// Build a `multipart/mixed` message: the text/plain body part + one base64 part per
/// attachment. The caller supplies a unique `boundary` (mime.rs stays clock/random-free).
pub fn build_multipart_rfc822(
    msg: &OutgoingMessage,
    attachments: &[OutgoingAttachment],
    boundary: &str,
) -> String {
    let mut headers = outgoing_headers(msg);
    headers.push("MIME-Version: 1.0".to_string());
    headers.push(format!("Content-Type: multipart/mixed; boundary=\"{boundary}\""));

    // 🦀 The text part, then one part per attachment, joined by CRLF; then the closing delimiter.
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "--{boundary}\r\nContent-Type: text/plain; charset=\"utf-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
        base64_body(&msg.body)
    ));
    for att in attachments {
        // 🦀 Sanitize CR/LF (header injection) and RFC2047-encode a non-ASCII filename,
        //    reusing the same `encode_subject` path the Subject header uses.
        let safe_name = encode_subject(&sanitize_header(&att.filename));
        let mime = sanitize_header(&att.mime_type);
        parts.push(format!(
            "--{boundary}\r\nContent-Type: {mime}; name=\"{safe_name}\"\r\nContent-Disposition: attachment; filename=\"{safe_name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
            base64_bytes(&att.bytes)
        ));
    }
    let body = format!("{}\r\n--{boundary}--", parts.join("\r\n"));
    format!("{}\r\n\r\n{}", headers.join("\r\n"), body)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib mime`
Expected: PASS — both the new tests and **all the existing `build_rfc822` tests** (the `outgoing_headers`/`wrap76` extraction kept the bytes identical).

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo clippy --all-targets 2>&1 | tail -3
cd .. && git add src-tauri/src/mime.rs
git commit -m "feat(m17): build_multipart_rfc822 + mime_for_ext (pure MIME)"
```

**Rust recap to include:** how extracting `outgoing_headers`/`wrap76` is a safe refactor (the unchanged tests are the proof), and why keeping `mime.rs` free of clock/randomness (boundary injected by the caller) makes it fully deterministic and unit-testable.

---

## Task 5: Sending — wire attachments into `send_email`

**Files:**
- Modify: `src-tauri/src/commands.rs`

(Glue over the TDD'd `mime_for_ext`/`build_multipart_rfc822`; verified by `cargo build` + later E2E.)

- [ ] **Step 1: Extend the command**

In `src-tauri/src/commands.rs`, replace the whole `send_email` command with:

```rust
/// Send a plain-text message, optionally with file attachments. With no attachments this
/// is the original single-part path; with attachments it builds a multipart/mixed message.
// 🦀 `#[allow(clippy::too_many_arguments)]` — these flat args mirror the JS `invoke` payload;
//    a shared `OutgoingFields` struct is a noted follow-up (kept out of M17 to stay focused).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_email(
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    thread_id: Option<String>,
    attachment_paths: Vec<String>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let msg = crate::mime::OutgoingMessage {
        from: stored.email,
        to,
        cc,
        subject,
        body,
        in_reply_to,
        references,
    };
    // 🦀 No files → the unchanged single-part path. `return` short-circuits before any file I/O.
    if attachment_paths.is_empty() {
        let raw = crate::mime::build_rfc822(&msg);
        return client.send_message(&raw, thread_id.as_deref()).await;
    }
    // 🦀 Read each picked file into memory, tagging it with a best-effort MIME type.
    let mut attachments = Vec::new();
    let mut total = 0usize;
    for path in &attachment_paths {
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::Other(format!("could not read attachment {path}: {e}")))?;
        total += bytes.len();
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let mime_type = crate::mime::mime_for_ext(&filename).to_string();
        attachments.push(crate::mime::OutgoingAttachment { filename, mime_type, bytes });
    }
    // 🦀 Reject oversized payloads before base64 inflation pushes us past the send ceiling.
    if total > crate::mime::MAX_ATTACHMENT_BYTES {
        return Err(AppError::Other(format!(
            "attachments total {total} bytes exceed the {} MB limit",
            crate::mime::MAX_ATTACHMENT_BYTES / (1024 * 1024)
        )));
    }
    // 🦀 A unique-enough multipart boundary from the wall clock; mime.rs itself stays clock-free.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let boundary = format!("ember_boundary_{nanos}");
    let raw = crate::mime::build_multipart_rfc822(&msg, &attachments, &boundary);
    client.send_message(&raw, thread_id.as_deref()).await
}
```

- [ ] **Step 2: Build to verify**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: compiles clean. Then `cargo test` (all) + `cargo clippy --all-targets 2>&1 | tail -3` — green/clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(m17): send_email reads attachment paths, caps size, builds multipart"
```

**Rust recap to include:** how the early `return` keeps the no-attachment path a literal pass-through, and how `std::path::Path::file_name().and_then(|n| n.to_str())` safely extracts a UTF-8 filename from an OS path.

---

## Task 6: Add the dialog plugin + capability

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `package.json`

- [ ] **Step 1: Add the Rust dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]`, add next to `tauri-plugin-notification = "2"`:

```toml
tauri-plugin-dialog = "2"
```

- [ ] **Step 2: Initialize the plugin**

In `src-tauri/src/lib.rs`, add a `.plugin(...)` call right after the existing notification plugin line:

```rust
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
```

- [ ] **Step 3: Grant the capability**

In `src-tauri/capabilities/default.json`, add `"dialog:default"` to the `permissions` array:

```json
  "permissions": [
    "core:default",
    "core:window:allow-set-focus",
    "notification:default",
    "dialog:default"
  ]
```

- [ ] **Step 4: Add the JS dependency**

In `package.json`, add to `dependencies`:

```json
    "@tauri-apps/plugin-dialog": "^2",
```

Then install:

Run: `npm install`
Expected: adds `@tauri-apps/plugin-dialog` to `node_modules` + lockfile, no errors.

- [ ] **Step 5: Verify both builds**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: compiles with the new plugin. Then `cd .. && npm run build 2>&1 | tail -5`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/capabilities/default.json package.json package-lock.json
git commit -m "feat(m17): add tauri-plugin-dialog + dialog capability"
```

**Rust recap to include:** how Tauri capabilities gate the *frontend* plugin API (so the JS `open`/`save` need `dialog:default`), while the Rust `std::fs::write` in `download_attachment` needs no capability because Rust commands run with full OS access.

---

## Task 7: Frontend — api wrappers + pure helpers + mocks

**Files:**
- Create: `src/lib/attachments.ts`
- Modify: `src/lib/mock.ts`
- Modify: `src/lib/api.ts`

(Mocks are defined here — before `api.ts` consumes them — so this task builds green on its own.)

- [ ] **Step 1: Create the pure helpers**

Create `src/lib/attachments.ts`:

```ts
// Pure display helpers for attachments. No I/O — safe in the maket.

// Human-readable byte size: 1023 → "1023 B", 2048 → "2.0 KB", 5_242_880 → "5.0 MB".
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let size = n / 1024;
  let i = 0;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(1)} ${units[i]}`;
}

// Last path segment, for displaying a picked file's name (handles / and \).
export function basename(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}
```

- [ ] **Step 2: Add the mock body + file-pick stub**

In `src/lib/mock.ts`, extend the type import to include `MessageBody, Attachment`:

```ts
import type { MessagePreview, SyncSummary, DraftContent, Label, MessageBody, Attachment } from "./api";
```

Append `mockMessageBody` (used by the gated `fetchMessageBody`) and `mockPickFiles` (used by compose in Task 9):

```ts
/** Browser-maket: a message body, with attachments on m1 so the strip is demoable. */
export function mockMessageBody(id: string): MessageBody {
  const attachments: Attachment[] =
    id === "m1"
      ? [
          { filename: "Q3-roadmap.pdf", mime_type: "application/pdf", size: 248000, attachment_id: "att1" },
          { filename: "budget.xlsx", mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", size: 18500, attachment_id: "att2" },
        ]
      : [];
  return {
    html: `<p style="font-family:system-ui">(mock body for ${id})</p>`,
    is_html: true,
    blocked_images: false,
    attachments,
  };
}

/** Browser-maket: pretend the user picked a file so the compose chips are demoable. */
export function mockPickFiles(): string[] {
  return ["/Users/you/Documents/proposal.pdf"];
}
```

- [ ] **Step 3: Extend `api.ts` — attachment types + body gating**

In `src/lib/api.ts`, add `mockMessageBody` to the existing mock import (the `from "./mock"` line):

```ts
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek, mockSearch, mockFolder, mockGetDraft, mockSaveDraft, MOCK_LABELS, mockFetchLabel, mockMessageBody } from "./mock";
```

Add the `Attachment` interface and extend `MessageBody` (replace the existing `MessageBody` interface):

```ts
export interface Attachment {
  filename: string;
  mime_type: string;
  size: number;
  attachment_id: string;
}

export interface MessageBody {
  html: string;
  is_html: boolean;
  blocked_images: boolean;
  attachments: Attachment[];
}
```

Replace `fetchMessageBody` to gate on `isTauri()` (this also closes the M16 maket gap where opening a body in the browser threw) and add `downloadAttachment`:

```ts
export const fetchMessageBody = (
  id: string,
  loadImages = false,
): Promise<MessageBody> =>
  isTauri()
    ? invoke<MessageBody>("fetch_message_body", { id, loadImages })
    : Promise.resolve(mockMessageBody(id));

export const downloadAttachment = (
  messageId: string,
  attachmentId: string,
  destPath: string,
): Promise<void> =>
  invoke<void>("download_attachment", { messageId, attachmentId, destPath });
```

- [ ] **Step 4: Extend the send payload**

In `src/lib/api.ts`, add `attachment_paths` to `SendEmailPayload` and forward it in `sendEmail`:

```ts
export interface SendEmailPayload {
  to: string[];
  cc: string[];
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
  attachment_paths: string[];
}

export const sendEmail = (p: SendEmailPayload): Promise<void> =>
  invoke<void>("send_email", {
    to: p.to,
    cc: p.cc,
    subject: p.subject,
    body: p.body,
    inReplyTo: p.in_reply_to,
    references: p.references,
    threadId: p.thread_id,
    attachmentPaths: p.attachment_paths,
  });
```

(`saveDraft`/`sendDraft` keep their existing wrappers — they accept `SendEmailPayload & { draft_id }` but simply don't forward `attachment_paths`, so drafts stay text-only.)

- [ ] **Step 5: Typecheck**

Run: `npm run build 2>&1 | tail -10`
Expected: `tsc` + `vite build` clean (the mocks from Step 2 satisfy the gated `fetchMessageBody`).

- [ ] **Step 6: Commit**

```bash
git add src/lib/attachments.ts src/lib/mock.ts src/lib/api.ts
git commit -m "feat(m17): api attachment types + downloadAttachment + gated body + mocks"
```

**Note to include:** call out that `fetchMessageBody` is now `isTauri()`-gated, closing the pre-existing M16 maket gap (opening a body in the browser used to throw `Cannot read properties of undefined (reading 'invoke')`).

---

## Task 8: Frontend — reading-pane attachment strip + Save-As download

**Files:**
- Modify: `src/components/ReadingPane.tsx`
- Modify: `src/styles/app.css`

(The mock body was added in Task 7, so the strip has data in the maket.)

- [ ] **Step 1: Render the strip + download handler**

In `src/components/ReadingPane.tsx`:

Update the imports — add `Paperclip` to the lucide import, and add:

```tsx
import { isTauri } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { downloadAttachment, type Attachment } from "../lib/api";
import { formatBytes } from "../lib/attachments";
```

Add download state next to the other `useState`s:

```tsx
  const [dlStatus, setDlStatus] = useState<Record<string, string>>({});
```

Reset it when the open message changes — extend the existing `useEffect(() => setConfirmDelete(false), [msg?.id])` to also clear status, or add:

```tsx
  useEffect(() => setDlStatus({}), [msg?.id]);
```

Add the handler (inside the component, before the `return`):

```tsx
  async function handleDownload(att: Attachment) {
    if (!msg) return;
    if (!isTauri()) {
      // In the browser maket there's no native dialog / filesystem.
      setDlStatus((s) => ({ ...s, [att.attachment_id]: "(maket: no download)" }));
      return;
    }
    const dest = await save({ defaultPath: att.filename });
    if (!dest) return; // user cancelled
    setDlStatus((s) => ({ ...s, [att.attachment_id]: "Saving…" }));
    try {
      await downloadAttachment(msg.id, att.attachment_id, dest);
      setDlStatus((s) => ({ ...s, [att.attachment_id]: "Saved ✓" }));
    } catch {
      setDlStatus((s) => ({ ...s, [att.attachment_id]: "Failed" }));
    }
  }
```

Render the strip between `.reading-head` and `.reading-body-area` (right after the closing `</div>` of `reading-head`):

```tsx
      {body && body.attachments.length > 0 && (
        <div className="attachments-strip">
          {body.attachments.map((att) => (
            <button
              key={att.attachment_id}
              className="attach-chip"
              onClick={() => handleDownload(att)}
              title={`Save ${att.filename}`}
            >
              <Paperclip size={13} />
              <span className="attach-name">{att.filename}</span>
              <span className="attach-size">{formatBytes(att.size)}</span>
              {dlStatus[att.attachment_id] && (
                <span className="attach-status">{dlStatus[att.attachment_id]}</span>
              )}
            </button>
          ))}
        </div>
      )}
```

- [ ] **Step 2: Add styles**

In `src/styles/app.css`, append (adjust `var(--...)` names to match existing tokens in `theme.css` if these aren't defined — the codebase uses CSS custom properties for theming):

```css
.attachments-strip {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--border);
}
.attach-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 9px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}
.attach-chip:hover { border-color: var(--accent); }
.attach-name { font-weight: 500; }
.attach-size { color: var(--text-dim); }
.attach-status { color: var(--accent); }
```

- [ ] **Step 3: Build to verify**

Run: `npm run build 2>&1 | tail -10`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 4: Commit**

```bash
git add src/components/ReadingPane.tsx src/styles/app.css
git commit -m "feat(m17): reading-pane attachment strip + Save-As download"
```

---

## Task 9: Frontend — compose attachments

**Files:**
- Modify: `src/components/ComposeModal.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Add the Attach control, chips, and send wiring**

In `src/components/ComposeModal.tsx`:

Update imports — add `Paperclip`, and:

```tsx
import { open } from "@tauri-apps/plugin-dialog";
import { isTauri } from "@tauri-apps/api/core";
import { basename } from "../lib/attachments";
import { mockPickFiles } from "../lib/mock";
```

Add attachment state near the other `useState`s:

```tsx
  const [attachPaths, setAttachPaths] = useState<string[]>([]);
```

Include attachments in the `dirty` check (so closing prompts to save when a file is attached):

```tsx
  const dirty =
    to.trim() !== "" || cc.trim() !== "" || subject.trim() !== "" || body !== initial.body || attachPaths.length > 0;
```

Add the pick + remove handlers (before `return`):

```tsx
  async function handleAttach() {
    if (!isTauri()) {
      // Browser maket: stub a picked file so the chips render.
      setAttachPaths((p) => [...p, ...mockPickFiles()]);
      return;
    }
    const picked = await open({ multiple: true });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    setAttachPaths((p) => [...p, ...paths]);
  }

  function removeAttach(path: string) {
    setAttachPaths((p) => p.filter((x) => x !== path));
  }
```

Add `attachment_paths` to the `fields()` return:

```tsx
  function fields(): SendEmailPayload {
    return {
      to: parseRecipients(to),
      cc: parseRecipients(cc),
      subject,
      body,
      in_reply_to: initial.inReplyTo,
      references: initial.references,
      thread_id: initial.threadId,
      attachment_paths: attachPaths,
    };
  }
```

In `handleSend`'s catch block (the failed-send outbox fallback), make the dropped-attachments behavior explicit — replace the existing saved-draft message line:

```tsx
        const id = await saveDraft({ ...f, draft_id: draftId });
        setDraftId(id);
        onDraftsChanged?.();
        setError(
          attachPaths.length > 0
            ? `Couldn't send (${String(e)}). Saved text to Drafts — attachments were NOT saved; re-attach and retry.`
            : `Couldn't send (${String(e)}). Saved to Drafts — retry from there.`,
        );
```

Render the Attach button + chips. Put the button row just below the `<textarea>` and above the `{error && ...}` line:

```tsx
        <div className="attach-row">
          <button className="compose-cc-toggle" onClick={handleAttach} type="button">
            <Paperclip size={13} /> Attach
          </button>
          {attachPaths.length > 0 && (
            <span className="attach-hint">Attachments send with the message but aren't saved to drafts yet.</span>
          )}
        </div>
        {attachPaths.length > 0 && (
          <div className="attach-file-chips">
            {attachPaths.map((p) => (
              <span key={p} className="attach-file-chip">
                <Paperclip size={12} />
                {basename(p)}
                <button className="attach-remove" aria-label={`Remove ${basename(p)}`} onClick={() => removeAttach(p)} type="button">
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
```

- [ ] **Step 2: Add styles**

Append to `src/styles/app.css`:

```css
.attach-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 0;
}
.attach-hint { font-size: 11px; color: var(--text-dim); }
.attach-file-chips { display: flex; flex-wrap: wrap; gap: 6px; padding-bottom: 4px; }
.attach-file-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 6px 3px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  font-size: 12px;
}
.attach-remove {
  border: none;
  background: none;
  cursor: pointer;
  color: var(--text-dim);
  font-size: 14px;
  line-height: 1;
  padding: 0 2px;
}
.attach-remove:hover { color: var(--danger, #c0392b); }
```

- [ ] **Step 3: Build to verify**

Run: `npm run build 2>&1 | tail -10`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 4: Commit**

```bash
git add src/components/ComposeModal.tsx src/styles/app.css
git commit -m "feat(m17): compose attachments — Attach picker + chips + send wiring"
```

---

## Task 10: Full verification + maket screenshots

**Files:** none (verification only)

- [ ] **Step 1: Full Rust suite + lint**

Run: `cd src-tauri && cargo test 2>&1 | tail -15`
Expected: all tests PASS (≥ 76: the prior count plus the 2 new gmail wiremock tests + 4 new mime tests).
Run: `cargo clippy --all-targets 2>&1 | tail -5`
Expected: clean (no warnings; `-D warnings` if CI uses it).

- [ ] **Step 2: Frontend build**

Run: `cd .. && npm run build 2>&1 | tail -5`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 3: Maket verification**

Run: `npm run dev` and open the printed local URL in a browser.
Verify and screenshot:
1. Open the **Maya / "Q3 roadmap" (m1)** message → the reading pane shows the **attachments strip** with `Q3-roadmap.pdf · 242.2 KB` and `budget.xlsx · 18.1 KB`; clicking a chip shows `(maket: no download)`.
2. Open **compose** (New message) → click **Attach** → a file chip `proposal.pdf` appears with a removable ×, and the "aren't saved to drafts yet" hint shows.

- [ ] **Step 4: Confirm clean tree**

Run: `git status -s`
Expected: empty (everything committed). If a reviewer subagent left changes, discard them (see the project's subagent git-hazard note).

- [ ] **Step 5: Final summary**

Report: test count delta, clippy/build status, maket screenshots, and the **owner-pending live Gmail E2E** (a real received-attachment download to disk + a real multipart send that arrives with the file intact). The wiki `[[ember]]` + `wiki/log.md` update happens at merge time (controller), not as a plan task.

---

## Self-review notes (for the controller)

- **Spec coverage:** receiving metadata (T1) · receiving bytes (T2) · download command (T3) · multipart builder (T4) · send wiring + cap + boundary (T5) · dialog plugin/capability (T6) · api/helpers + body gating (T7) · reading-pane strip (T8) · compose attach + draft-drop message (T9) · verification (T10). Deferred items (drafts-with-attachments, inline cid, list paperclip, resumable upload, OutgoingFields refactor) are intentionally absent.
- **Type consistency:** `AttachmentMeta {filename, mime_type, size, attachment_id}` (Rust) ↔ `Attachment` (TS) identical field names; `download_attachment(message_id, attachment_id, dest_path)` ↔ `downloadAttachment(messageId, attachmentId, destPath)`; `send_email(..., attachment_paths)` ↔ `sendEmail({..., attachment_paths})` → `attachmentPaths`. `build_multipart_rfc822(msg, attachments, boundary)` matches its only caller in T5.
- **No new OAuth scope, no DB migration.** Only `dialog:` capability added; the byte write is Rust `std::fs`.
