# Ember — Milestone 8: Compose & Send (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user send mail from Ember — a fresh compose and a reply to an open message — as plain text, from a focused modal, with a clear "Sent ✓" confirmation and errors that never lose the typed text.

**Architecture:** A new pure Rust `mime` module builds an RFC822 message (headers + base64 plain-text body; RFC2047 encoded-word for non-ASCII subjects). The Gmail client gains `send_message` (POST `messages.send` with the base64url raw + optional `threadId`) and `get_reply_context` (original Message-ID/References + plain-text body). Two DB-free Tauri commands (`send_email`, `get_reply_context`) sit on top. The React frontend adds a pure `compose` helper, a `ComposeModal`, a header Compose button, and wires the reading-pane Reply button. No schema migration.

**Tech Stack:** Rust (reqwest, serde, serde_json, base64, wiremock for tests), Tauri 2, React 19 + TypeScript + Vite, lucide-react icons.

**Design source:** `docs/superpowers/specs/2026-06-19-ember-m8-compose-send-design.md` (approved).

**Learning mode (IMPORTANT — every implementer):** The repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept (ownership/borrowing, `Result`/`Option`/`?`, slices, iterators, closures, lifetimes, derive macros), not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

**Environment:** `cargo` is on PATH at `/opt/homebrew/bin/cargo`; backend commands run from `src-tauri/`. Frontend commands (`npm run build`) run from the repo root. **`cargo fmt` is NOT used in this repo** (no rustfmt config/CI; deliberate hand-style) — do not run it; it is not a gate.

---

## Milestone context

M1–M7 are merged to `main`. The app reads, classifies, and mutates mail but cannot send. M8 adds compose & send. The existing `gmail.modify` OAuth scope already authorizes `messages.send` and drafts (no re-consent) — **verify this live early in E2E** (M7 lesson). Sending is DB-free: sent mail goes to Gmail's Sent folder, which Ember does not cache, so a "Sent ✓" confirmation is the only local signal.

**Scope (lean v1):** new compose + reply, plain text, modal overlay, immediate send. **Deferred:** drafts, outbox/retry, signature, attachments, HTML/rich-text, reply-all, `Reply-To` handling, display-name encoding, a Sent view.

---

## File structure

**Backend (`src-tauri/`):**
- `src/mime.rs` — **NEW.** Pure RFC822 builder (`OutgoingMessage`, `build_rfc822`, RFC2047 subject, base64 body). Table tests.
- `src/lib.rs` — `pub mod mime;` + register two commands.
- `src/gmail/types.rs` — add `headers` to `MessagePart`; add `ReplyContext`.
- `src/gmail/mod.rs` — add `send_message` + `get_reply_context`; import `ReplyContext`.
- `src/commands.rs` — `send_email` + `get_reply_context` commands.
- `tests/gmail_test.rs` — wiremock tests for `send_message` + `get_reply_context`.

**Frontend (`src/`):**
- `lib/compose.ts` — **NEW**, pure: `parseAddress`, `parseRecipients`, `isPlausibleEmail`, `replySubject`, `quoteBody`.
- `lib/api.ts` — `sendEmail` + `getReplyContext` wrappers + `ReplyContext`/`SendEmailPayload` types.
- `components/ComposeModal.tsx` — **NEW.** The compose modal.
- `App.tsx` — compose state + render modal + wire Header/ReadingPane.
- `components/Header.tsx` — Compose button.
- `components/ReadingPane.tsx` — enable + wire the Reply button.
- `styles/app.css` — modal styles.

---

## Task 1: `mime.rs` — RFC822 builder

**Files:**
- Create: `src-tauri/src/mime.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod mime;`)

- [ ] **Step 1: Register the module.** In `src-tauri/src/lib.rs`, after the `pub mod scorer;` line (~line 34), add:

```rust
// 🦀 Pure RFC822 message builder for outgoing mail (no I/O, fully unit-testable).
pub mod mime;
```

- [ ] **Step 2: Write `src-tauri/src/mime.rs` with the failing tests + implementation.**

```rust
// 🦀 A pure RFC822 (email) message builder for plain-text mail. No I/O and no clock,
//    so it is fully unit-testable. Gmail fills in Date and Message-ID for us, so this
//    module never touches the system time.

use base64::Engine;

/// A plain-text message to send. `from` is the connected account address; the reply
/// fields are `None` for a fresh compose and `Some(..)` when replying.
pub struct OutgoingMessage {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

// 🦀 RFC2047 "encoded-word" for a non-ASCII header value: `=?UTF-8?B?<base64>?=`.
//    Pure-ASCII subjects pass through unchanged. `str::is_ascii` is the cheap gate.
fn encode_subject(subject: &str) -> String {
    if subject.is_ascii() {
        subject.to_string()
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(subject.as_bytes());
        format!("=?UTF-8?B?{b64}?=")
    }
}

// 🦀 base64-encode the UTF-8 body and wrap to 76-char lines joined by CRLF (RFC 2045).
//    base64 output is ASCII, so chunking the bytes and re-reading as &str never fails.
fn base64_body(body: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
    encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Build the full RFC822 message: headers, a blank line, then the base64 body. Uses
/// CRLF line endings throughout (what SMTP/Gmail expect).
pub fn build_rfc822(msg: &OutgoingMessage) -> String {
    // 🦀 Collect header lines into a Vec, then join with CRLF — clearer than push_str-ing
    //    a String with manual separators.
    let mut headers: Vec<String> = Vec::new();
    headers.push(format!("From: {}", msg.from));
    headers.push(format!("To: {}", msg.to.join(", ")));
    if !msg.cc.is_empty() {
        headers.push(format!("Cc: {}", msg.cc.join(", ")));
    }
    headers.push(format!("Subject: {}", encode_subject(&msg.subject)));
    // 🦀 `if let Some(x) = &opt` borrows the inner value without consuming the Option.
    if let Some(irt) = &msg.in_reply_to {
        headers.push(format!("In-Reply-To: {irt}"));
    }
    if let Some(refs) = &msg.references {
        headers.push(format!("References: {refs}"));
    }
    headers.push("MIME-Version: 1.0".to_string());
    headers.push("Content-Type: text/plain; charset=\"utf-8\"".to_string());
    headers.push("Content-Transfer-Encoding: base64".to_string());
    format!("{}\r\n\r\n{}", headers.join("\r\n"), base64_body(&msg.body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    // 🦀 Decode a wrapped-base64 body back to its String (strip the CRLF wrapping first).
    fn decode_body(s: &str) -> String {
        let joined: String = s.split("\r\n").collect();
        let bytes = base64::engine::general_purpose::STANDARD.decode(joined).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    fn msg() -> OutgoingMessage {
        OutgoingMessage {
            from: "me@example.com".into(),
            to: vec!["a@x.com".into()],
            cc: vec![],
            subject: "Hello".into(),
            body: "Hi there".into(),
            in_reply_to: None,
            references: None,
        }
    }

    #[test]
    fn builds_basic_headers_and_body() {
        let out = build_rfc822(&msg());
        assert!(out.contains("From: me@example.com\r\n"));
        assert!(out.contains("To: a@x.com\r\n"));
        assert!(out.contains("Subject: Hello\r\n"));
        assert!(out.contains("MIME-Version: 1.0\r\n"));
        assert!(out.contains("Content-Type: text/plain; charset=\"utf-8\"\r\n"));
        assert!(out.contains("Content-Transfer-Encoding: base64\r\n"));
        let (_, body) = out.split_once("\r\n\r\n").unwrap();
        assert_eq!(decode_body(body), "Hi there");
    }

    #[test]
    fn omits_cc_when_empty_and_joins_when_present() {
        let mut m = msg();
        assert!(!build_rfc822(&m).contains("Cc:"));
        m.cc = vec!["c1@x.com".into(), "c2@x.com".into()];
        assert!(build_rfc822(&m).contains("Cc: c1@x.com, c2@x.com\r\n"));
    }

    #[test]
    fn joins_multiple_to() {
        let mut m = msg();
        m.to = vec!["a@x.com".into(), "b@x.com".into()];
        assert!(build_rfc822(&m).contains("To: a@x.com, b@x.com\r\n"));
    }

    #[test]
    fn encodes_non_ascii_subject_as_rfc2047() {
        let mut m = msg();
        m.subject = "Привіт".into();
        let expected = format!(
            "Subject: =?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode("Привіт".as_bytes())
        );
        assert!(build_rfc822(&m).contains(&expected));
    }

    #[test]
    fn includes_reply_threading_headers() {
        let mut m = msg();
        m.in_reply_to = Some("<abc@mail>".into());
        m.references = Some("<abc@mail>".into());
        let out = build_rfc822(&m);
        assert!(out.contains("In-Reply-To: <abc@mail>\r\n"));
        assert!(out.contains("References: <abc@mail>\r\n"));
    }

    #[test]
    fn body_base64_wraps_at_76_and_roundtrips() {
        let mut m = msg();
        m.body = "x".repeat(200);
        let (_, body) = build_rfc822(&m).split_once("\r\n\r\n").map(|(h, b)| (h.to_string(), b.to_string())).unwrap();
        for line in body.split("\r\n") {
            assert!(line.len() <= 76);
        }
        assert_eq!(decode_body(&body), "x".repeat(200));
    }

    #[test]
    fn no_date_or_message_id_headers() {
        let out = build_rfc822(&msg());
        assert!(!out.contains("Date:"));
        assert!(!out.contains("Message-ID:"));
    }
}
```

- [ ] **Step 3: Run the tests, verify they PASS.**

Run: `cd src-tauri && cargo test --lib mime`
Expected: all 7 `mime::tests::*` pass. (We wrote impl + tests together; if any fail, fix the impl, not the test.)

- [ ] **Step 4: Lint.**

Run: `cd src-tauri && cargo clippy --lib --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/mime.rs src-tauri/src/lib.rs
git commit -m "feat(mime): pure RFC822 builder for plain-text mail

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

**Rust recap:** building a String from a `Vec<String>` joined with a separator; `if let Some(x) = &opt` (borrowing an Option's contents); `str::is_ascii`; slicing bytes with `chunks`; the `base64::Engine` trait method `.encode`.

---

## Task 2: `ReplyContext` + `MessagePart.headers` + `get_reply_context`

**Files:**
- Modify: `src-tauri/src/gmail/types.rs`
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing test.** Append to `src-tauri/tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn get_reply_context_extracts_message_id_references_and_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/r1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "r1",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name": "Message-ID", "value": "<orig@mail.example>"},
                    {"name": "References", "value": "<a@x> <b@y>"}
                ],
                "parts": [
                    {"mimeType": "text/plain", "body": {"data": b64url("Original body")}}
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let rc = client.get_reply_context("r1").await.unwrap();
    assert_eq!(rc.message_id, "<orig@mail.example>");
    assert_eq!(rc.references, "<a@x> <b@y>");
    assert_eq!(rc.quoted_text, "Original body");
}
```

- [ ] **Step 2: Run it, verify it FAILS** (`no method named 'get_reply_context'`):

Run: `cd src-tauri && cargo test --test gmail_test get_reply_context_extracts_message_id_references_and_text`

- [ ] **Step 3a: Add `headers` to `MessagePart` and add `ReplyContext` in `src-tauri/src/gmail/types.rs`.**

In the `MessagePart` struct, add a `headers` field (it already has `mime_type`, `body`, `parts`):

```rust
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
```

Append the `ReplyContext` type (serialized to the frontend):

```rust
/// What a reply needs from the original message: threading headers + the quoted text.
#[derive(Debug, Serialize)]
pub struct ReplyContext {
    pub message_id: String,
    pub references: String,
    pub quoted_text: String,
}
```

- [ ] **Step 3b: Add `get_reply_context` in `src-tauri/src/gmail/mod.rs`.**

Add `ReplyContext` to the `use types::{...}` import line:

```rust
use types::{
    FullMessage, HistoryResponse, MessageList, MessagePart, MessagePreview, ModifiedMessage,
    Profile, RawMessage, ReplyContext,
};
```

Add the method inside `impl GmailClient` (e.g. after `get_message_body`):

```rust
    /// Fetch what a reply needs: the original's `Message-ID` + `References` headers (for
    /// threading) and its decoded plain-text body (for quoting). One `format=full` fetch.
    pub async fn get_reply_context(&self, id: &str) -> Result<ReplyContext> {
        let url = format!("{}/gmail/v1/users/me/messages/{}?format=full", self.base_url, id);
        let full: FullMessage = self.get_json(&url).await?;
        // 🦀 Closure that borrows the payload headers and finds one case-insensitively
        //    ("Message-ID" vs "Message-Id"), cloning the matched value out.
        let header = |name: &str| {
            full.payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap_or_default()
        };
        let message_id = header("Message-ID");
        let references = header("References");
        // 🦀 Reuse the existing recursive MIME walk to pull the text/plain part.
        let mut html = None;
        let mut text = None;
        collect_body(&full.payload, &mut html, &mut text);
        Ok(ReplyContext {
            message_id,
            references,
            quoted_text: text.unwrap_or_default(),
        })
    }
```

- [ ] **Step 4: Run the test, verify it PASSES**, then the whole gmail suite:

Run: `cd src-tauri && cargo test --test gmail_test get_reply_context_extracts_message_id_references_and_text`
Run: `cd src-tauri && cargo test --test gmail_test`
Expected: PASS, no regressions.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): add get_reply_context (Message-ID/References + quoted text)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

**Rust recap:** adding a serde field with `#[serde(default)]`; a closure capturing a borrow of `full.payload`; `eq_ignore_ascii_case`; reusing `collect_body` via out-params.

---

## Task 3: `send_message`

**Files:**
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing tests.** Append to `src-tauri/tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn send_message_posts_base64url_raw_with_thread_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/send"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "sent1"})))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client
        .send_message("From: a@b\r\n\r\nhi", Some("thread-9"))
        .await
        .unwrap();

    // 🦀 Inspect the request the mock server actually received.
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["threadId"], "thread-9");
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body["raw"].as_str().unwrap())
        .unwrap();
    assert_eq!(String::from_utf8(decoded).unwrap(), "From: a@b\r\n\r\nhi");
}

#[tokio::test(flavor = "multi_thread")]
async fn send_message_omits_thread_id_when_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/send"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "sent2"})))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client.send_message("hello", None).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert!(body.get("threadId").is_none());
}
```

- [ ] **Step 2: Run them, verify they FAIL** (`no method named 'send_message'`):

Run: `cd src-tauri && cargo test --test gmail_test send_message`

- [ ] **Step 3: Implement in `src-tauri/src/gmail/mod.rs`** (inside `impl GmailClient`, next to `get_reply_context`):

```rust
    /// Send a raw RFC822 message. `thread_id` threads a reply into its conversation.
    pub async fn send_message(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()> {
        use base64::Engine;
        // 🦀 Gmail wants the whole RFC822 message base64url-encoded (web-safe, no padding)
        //    in the `raw` field — the same encoding the read path decodes with.
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw_rfc822.as_bytes());
        // 🦀 Short-lived request struct. `skip_serializing_if` drops `threadId` entirely
        //    (not `null`) when there's no thread — Gmail rejects a null threadId.
        #[derive(serde::Serialize)]
        struct SendRequest<'a> {
            raw: String,
            #[serde(rename = "threadId", skip_serializing_if = "Option::is_none")]
            thread_id: Option<&'a str>,
        }
        let url = format!("{}/gmail/v1/users/me/messages/send", self.base_url);
        let body = SendRequest { raw, thread_id };
        // 🦀 We don't use the returned Message resource; deserialize into a throwaway
        //    `serde_json::Value` and drop it.
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }
```

- [ ] **Step 4: Run the tests, verify they PASS**, then the whole gmail suite:

Run: `cd src-tauri && cargo test --test gmail_test send_message`
Run: `cd src-tauri && cargo test --test gmail_test`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): add send_message (messages.send with base64url raw)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

**Rust recap:** `skip_serializing_if` on an `Option` serde field; base64url vs standard base64; discarding a typed response with `let _: serde_json::Value`.

---

## Task 4: Commands `send_email` + `get_reply_context` + registration

No automated test (these need a live token + network, like the other commands). Gate: `cargo build` + `cargo clippy --all-targets -- -D warnings` clean + `cargo test` green.

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the commands to `src-tauri/src/commands.rs`.**

Extend the existing types import (currently `use crate::gmail::types::MessagePreview;`) to:

```rust
use crate::gmail::types::{MessagePreview, ReplyContext};
```

Append the two commands at the end of the file:

```rust
/// Build an RFC822 message from the compose fields and send it via Gmail. DB-free —
/// sent mail lands in Gmail's Sent folder, which Ember does not cache.
#[tauri::command]
pub async fn send_email(
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    thread_id: Option<String>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    // 🦀 Partial move: `access_token` moves into the client, then `email` moves into the
    //    message — Rust allows moving distinct fields out of `stored` separately.
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
    let raw = crate::mime::build_rfc822(&msg);
    // 🦀 `Option<String>::as_deref()` → `Option<&str>` to match send_message's signature.
    client.send_message(&raw, thread_id.as_deref()).await
}

/// Fetch the data a reply needs: the original's Message-ID/References (threading) and its
/// plain-text body (quoting).
#[tauri::command]
pub async fn get_reply_context(id: String) -> Result<ReplyContext> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.get_reply_context(&id).await
}
```

- [ ] **Step 2: Register in `src-tauri/src/lib.rs`.** Extend the `tauri::generate_handler![...]` list (currently ends with `commands::trash_message,`):

```rust
            commands::set_message_read,
            commands::set_message_starred,
            commands::archive_message,
            commands::trash_message,
            commands::send_email,
            commands::get_reply_context,
        ])
```

- [ ] **Step 3: Build + lint + full test suite.**

Run: `cd src-tauri && cargo build`
Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings`
Run: `cd src-tauri && cargo test`
Expected: all clean/green.

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add send_email + get_reply_context and register them

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

**Rust recap:** partial moves of struct fields; `Option::as_deref`; why these commands need no `Db` state.

---

## Task 5: Frontend API wrappers + pure `compose` helpers

**Files:**
- Modify: `src/lib/api.ts`
- Create: `src/lib/compose.ts`

- [ ] **Step 1: Add types + wrappers to `src/lib/api.ts`** (append at the end):

```ts
export interface ReplyContext {
  message_id: string;
  references: string;
  quoted_text: string;
}

export interface SendEmailPayload {
  to: string[];
  cc: string[];
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
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
  });

export const getReplyContext = (id: string): Promise<ReplyContext> =>
  invoke<ReplyContext>("get_reply_context", { id });
```

(Note: Tauri converts the camelCase keys `inReplyTo`/`threadId` to the Rust `in_reply_to`/`thread_id` params — the same convention `fetchMessageBody` uses.)

- [ ] **Step 2: Create `src/lib/compose.ts`:**

```ts
// Pure helpers for composing/replying. No I/O — unit-testable once Vitest lands.

// Extract a bare address from a header like "Maya <maya@studio.co>" → "maya@studio.co".
export function parseAddress(headerValue: string): string {
  const m = headerValue.match(/<([^>]+)>/);
  return (m ? m[1] : headerValue).trim();
}

// Split a recipient input on commas/semicolons into trimmed, non-empty addresses.
export function parseRecipients(input: string): string[] {
  return input
    .split(/[,;]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

// Loose validity: one "@", a dot after it, no whitespace.
export function isPlausibleEmail(addr: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(addr);
}

// Prefix "Re: " unless already present (case-insensitive).
export function replySubject(subject: string): string {
  return /^re:/i.test(subject.trim()) ? subject : `Re: ${subject}`;
}

// Build a quoted reply body: a blank gap, an attribution line, then "> "-prefixed original.
export function quoteBody(
  fromLabel: string,
  dateLabel: string,
  text: string,
): string {
  const quoted = text
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
  return `\n\nOn ${dateLabel}, ${fromLabel} wrote:\n${quoted}\n`;
}
```

- [ ] **Step 3: Type-check.**

Run: `npm run build`
Expected: `tsc` + Vite clean.

- [ ] **Step 4: Commit.**

```bash
git add src/lib/api.ts src/lib/compose.ts
git commit -m "feat(ui): add send/reply API wrappers and pure compose helpers

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: `ComposeModal.tsx` + styles

**Files:**
- Create: `src/components/ComposeModal.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Create `src/components/ComposeModal.tsx`:**

```tsx
import { useState } from "react";
import { sendEmail, type SendEmailPayload } from "../lib/api";
import { parseRecipients, isPlausibleEmail } from "../lib/compose";
import { X } from "lucide-react";

export interface ComposeInitial {
  to: string; // comma-separated text (prefilled for reply)
  cc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  threadId: string | null;
}

export function ComposeModal({
  initial,
  onClose,
  onSent,
}: {
  initial: ComposeInitial;
  onClose: () => void;
  onSent: () => void;
}) {
  const [to, setTo] = useState(initial.to);
  const [cc, setCc] = useState(initial.cc);
  const [showCc, setShowCc] = useState(initial.cc.length > 0);
  const [subject, setSubject] = useState(initial.subject);
  const [body, setBody] = useState(initial.body);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const title = initial.threadId ? "Reply" : "New message";

  async function handleSend() {
    const toList = parseRecipients(to);
    const ccList = parseRecipients(cc);
    if (toList.length === 0 || !toList.every(isPlausibleEmail)) {
      setError("Enter at least one valid recipient address.");
      return;
    }
    if (ccList.length > 0 && !ccList.every(isPlausibleEmail)) {
      setError("One of the Cc addresses looks invalid.");
      return;
    }
    const payload: SendEmailPayload = {
      to: toList,
      cc: ccList,
      subject,
      body,
      in_reply_to: initial.inReplyTo,
      references: initial.references,
      thread_id: initial.threadId,
    };
    setSending(true);
    setError(null);
    try {
      await sendEmail(payload);
      onSent();
    } catch (e) {
      // Keep every field intact so the user can retry without retyping.
      setError(String(e));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="compose-overlay" onClick={onClose}>
      <div
        className="compose-card"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
        }}
      >
        <div className="compose-head">
          <span className="compose-title">{title}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <input
          className="compose-field"
          placeholder="To"
          value={to}
          onChange={(e) => setTo(e.target.value)}
          autoFocus
        />
        {showCc ? (
          <input
            className="compose-field"
            placeholder="Cc"
            value={cc}
            onChange={(e) => setCc(e.target.value)}
          />
        ) : (
          <button className="compose-cc-toggle" onClick={() => setShowCc(true)}>
            Add Cc
          </button>
        )}
        <input
          className="compose-field"
          placeholder="Subject"
          value={subject}
          onChange={(e) => setSubject(e.target.value)}
        />
        <textarea
          className="compose-body"
          placeholder="Write your message…"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={12}
        />
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          <button className="btn" onClick={onClose} disabled={sending}>
            Cancel
          </button>
          <button
            className="btn btn-accent"
            onClick={handleSend}
            disabled={sending}
          >
            {sending ? "Sending…" : "Send"}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Append modal styles to `src/styles/app.css`:**

```css
/* M8 — compose modal */
.compose-overlay { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.45); display: flex; align-items: center; justify-content: center; z-index: 50; }
.compose-card { width: min(620px, 92vw); max-height: 88vh; display: flex; flex-direction: column; gap: 8px; background: var(--surface); border: 1px solid var(--border-strong); border-radius: 12px; padding: 14px 16px; box-shadow: 0 12px 40px rgba(0, 0, 0, 0.35); }
.compose-head { display: flex; align-items: center; justify-content: space-between; }
.compose-title { font-size: 14px; font-weight: 600; }
.compose-field { height: 34px; padding: 0 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font-size: 13px; }
.compose-cc-toggle { align-self: flex-start; background: transparent; border: none; color: var(--accent-text); font-size: 12px; cursor: pointer; padding: 2px 0; }
.compose-body { flex: 1; min-height: 220px; resize: vertical; padding: 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font-size: 14px; line-height: 1.6; font-family: inherit; }
.compose-error { color: var(--danger); font-size: 13px; white-space: pre-wrap; }
.compose-actions { display: flex; justify-content: flex-end; gap: 8px; }
```

- [ ] **Step 3: Type-check.**

Run: `npm run build`
Expected: clean (the modal is exported but not yet rendered — that's fine for `tsc`).

- [ ] **Step 4: Commit.**

```bash
git add src/components/ComposeModal.tsx src/styles/app.css
git commit -m "feat(ui): add ComposeModal (To/Cc/Subject/body, send + error state)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Wire compose into App, Header, and ReadingPane

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/components/Header.tsx`
- Modify: `src/components/ReadingPane.tsx`

- [ ] **Step 1: `src/App.tsx` — imports.** Add to the `./lib/api` import the `getReplyContext` function; add new imports below the labels import:

```tsx
import {
  archiveMessage,
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  getReplyContext,
  setMessageRead,
  setMessageStarred,
  syncInbox,
  trashMessage,
  type MessagePreview,
} from "./lib/api";
import { orderedForStream, type Stream } from "./lib/streams";
import { isStarred, isUnread, UNREAD, STARRED, withLabel } from "./lib/labels";
import { parseAddress, replySubject, quoteBody } from "./lib/compose";
import { ComposeModal, type ComposeInitial } from "./components/ComposeModal";
```

- [ ] **Step 2: `src/App.tsx` — state + handlers.** Add the compose state next to the other `useState` hooks:

```tsx
  const [compose, setCompose] = useState<ComposeInitial | null>(null);
```

Add these handlers inside `App()` (e.g. after `handleSelect`):

```tsx
  function openNewCompose() {
    setCompose({
      to: "",
      cc: "",
      subject: "",
      body: "",
      inReplyTo: null,
      references: null,
      threadId: null,
    });
  }

  async function handleReply(m: MessagePreview) {
    setError(null);
    try {
      const ctx = await getReplyContext(m.id);
      const dateLabel = m.internal_date
        ? new Date(m.internal_date).toLocaleString()
        : m.date;
      setCompose({
        to: parseAddress(m.from),
        cc: "",
        subject: replySubject(m.subject),
        body: quoteBody(m.from, dateLabel, ctx.quoted_text),
        inReplyTo: ctx.message_id || null,
        references: ctx.references || ctx.message_id || null,
        threadId: m.thread_id || null,
      });
    } catch (e) {
      setError(String(e));
    }
  }
```

- [ ] **Step 3: `src/App.tsx` — JSX.** Add `onCompose` to the `<Header>` (the signed-in one), `onReply` to `<ReadingPane>`, and render the modal. The Header in the signed-in branch becomes:

```tsx
      <Header
        busy={busy}
        onSync={handleSync}
        status={status}
        account={account}
        stream={stream}
        onSelectStream={(s) => {
          setStream(s);
          setSelectedId(null);
        }}
        onCompose={openNewCompose}
      />
```

The `<ReadingPane>` gains `onReply={handleReply}`:

```tsx
          <ReadingPane
            msg={selected}
            onArchive={handleArchive}
            onTrash={handleTrash}
            onToggleStar={toggleStar}
            onMarkUnread={(m) => toggleRead(m, false)}
            onReply={handleReply}
          />
```

And render the modal just before the closing `</div>` of the signed-in `return` (after `</SplitView>`):

```tsx
      {compose && (
        <ComposeModal
          initial={compose}
          onClose={() => setCompose(null)}
          onSent={() => {
            setCompose(null);
            setStatus("Sent ✓");
          }}
        />
      )}
```

- [ ] **Step 4: `src/components/Header.tsx` — Compose button.** Add `Pencil` to the lucide import; add an optional `onCompose` prop; render the button just before the Sync button.

Add `Pencil` to the import list:

```tsx
import {
  Flame,
  RefreshCw,
  Sun,
  Moon,
  Inbox,
  Users,
  Bell,
  Newspaper,
  Pencil,
  type LucideIcon,
} from "lucide-react";
```

Add `onCompose` to the props type and destructure (alongside `onSync`):

```tsx
export function Header({
  busy,
  onSync,
  status,
  account = null,
  stream = "all",
  onSelectStream,
  onCompose,
}: {
  busy: boolean;
  onSync?: () => void;
  status: string | null;
  account?: string | null;
  stream?: Stream;
  onSelectStream?: (s: Stream) => void;
  onCompose?: () => void;
}) {
```

Render the button immediately before the `{onSync && (...)}` block:

```tsx
      {onCompose && (
        <button className="btn" onClick={onCompose}>
          <Pencil size={15} /> <span className="nav-label">Compose</span>
        </button>
      )}
```

- [ ] **Step 5: `src/components/ReadingPane.tsx` — enable Reply.** Add `onReply` to the props type/signature and wire the Reply button (it is currently `disabled`).

Extend the signature props:

```tsx
export function ReadingPane({
  msg,
  onArchive,
  onTrash,
  onToggleStar,
  onMarkUnread,
  onReply,
}: {
  msg: MessagePreview | null;
  onArchive: (m: MessagePreview) => void;
  onTrash: (m: MessagePreview) => void;
  onToggleStar: (m: MessagePreview) => void;
  onMarkUnread: (m: MessagePreview) => void;
  onReply: (m: MessagePreview) => void;
}) {
```

Replace the disabled Reply button with a wired one (this is in the `msg`-present branch, so `msg` is non-null):

```tsx
        <button
          className="icon-btn"
          aria-label="Reply"
          onClick={() => onReply(msg)}
        >
          <CornerUpLeft size={15} />
        </button>
```

- [ ] **Step 6: Type-check the whole frontend.**

Run: `npm run build`
Expected: `tsc` + Vite clean (the five wiring points now line up: `onCompose`, `onReply`, the modal props, and the new imports).

- [ ] **Step 7: Commit.**

```bash
git add src/App.tsx src/components/Header.tsx src/components/ReadingPane.tsx
git commit -m "feat(ui): wire Compose button + Reply into the compose modal

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Full verification + docs + finish

**Files:**
- Modify: `wiki/entities/ember.md`, `wiki/log.md` (gitignored, local)
- (Memory) `~/.claude/projects/-Users-makar-dev-ownmail/memory/ember-project.md`

- [ ] **Step 1: Backend — full suite + lint.**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all green/clean. (Do NOT run `cargo fmt` — not used in this repo.)

- [ ] **Step 2: Frontend — build.**

Run: `npm run build`
Expected: clean.

- [ ] **Step 3: Manual E2E against live Gmail** (run early to confirm `gmail.modify` actually permits send):

Run: `npm run tauri dev` (from repo root). If port 1420 is busy from a prior run: `lsof -ti tcp:1420 | xargs kill` first.

Verify:
- **New compose:** click **Compose** in the header → modal opens → To = your own address, a Subject and body → **Send** → modal closes, header shows **"Sent ✓"**; confirm the message arrives in Gmail.
- **Reply:** open a message → click **Reply** (the ↩ toolbar icon) → modal opens prefilled (recipient, `Re:` subject, quoted body) → **Send** → confirm in Gmail web that it landed **in the same thread**.
- **Unicode subject:** send one with a Cyrillic subject → confirm it displays correctly in Gmail (RFC2047).
- **Error preserves text:** turn off Wi-Fi, click Send → the modal stays open with your text and shows an error; re-enable Wi-Fi and Send succeeds.

- [ ] **Step 4: Update the wiki.** In `wiki/entities/ember.md`, change the M8 roadmap line to a done/state entry, e.g.:

```markdown
- **M8 — Compose & send (lean v1)** — *done, merged 2026-06-19.* RFC822/MIME builder
  (`mime.rs`, unicode-safe subjects) + `messages.send` + `get_reply_context`; a compose
  modal for new mail and replies (plain text), threaded via `threadId` +
  `In-Reply-To`/`References`; "Sent ✓" confirmation; DB-free; reuses `gmail.modify` (no
  re-consent). Deferred: drafts, outbox, signature, attachments, HTML, reply-all, a Sent view.
```

Update the "As of M7…" capability sentence to mention sending (M8), and append a one-line entry to `wiki/log.md` in the existing format.

- [ ] **Step 5: Update project memory.** In `~/.claude/projects/-Users-makar-dev-ownmail/memory/ember-project.md`, add an M8 milestone entry (done + merge SHA), and update the `MEMORY.md` index line to "M1–M8 merged … next M9 settings/onboarding".

- [ ] **Step 6: Commit docs** (wiki is gitignored, so this mainly covers any tracked docs):

```bash
git add -A && git commit -m "docs(m8): record Compose & Send milestone

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>" || echo "(nothing tracked to commit — wiki is gitignored)"
```

- [ ] **Step 7: Finish the branch.** Use `superpowers:finishing-a-development-branch` to merge `m8-compose-send` to `main` (verify tests on the merged result, then delete the branch).

---

## Self-review (completed during planning)

- **Spec coverage:** RFC822 builder → Task 1; `ReplyContext`/threading fetch → Task 2; `send_message` → Task 3; commands → Task 4; api/compose helpers → Task 5; ComposeModal → Task 6; Compose button + Reply wiring + "Sent ✓" → Task 7; error-preserves-text → Task 6 (`handleSend` catch keeps fields); live scope check + threading + unicode → Task 8 E2E. All spec sections map to a task.
- **Type consistency:** `OutgoingMessage`/`build_rfc822`, `ReplyContext { message_id, references, quoted_text }`, `send_message(raw, thread_id)`, `get_reply_context(id)`, and the JS `SendEmailPayload`/`ReplyContext`/`ComposeInitial` shapes + wrapper arg keys (`inReplyTo`/`threadId`) are used identically across tasks. Prop names (`onCompose`, `onReply`, `onClose`, `onSent`, `initial`) match between `App.tsx`, `Header.tsx`, `ReadingPane.tsx`, and `ComposeModal.tsx`.
- **Placeholder scan:** no TBD/TODO; every code step shows complete code.
- **Intentional caveat:** the frontend builds green at each of Tasks 5, 6, 7 (the modal is committed before it's rendered — a valid unused export under `tsc`).
