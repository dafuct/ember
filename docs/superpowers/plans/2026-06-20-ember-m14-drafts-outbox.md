# Ember M14 — Drafts & outbox (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Save/edit/send/discard real Gmail drafts, surface them in a **Drafts** rail folder (rows open the compose editor), and make a failed send fall back to a saved draft (the minimal outbox).

**Architecture:** Add `users.drafts.*` methods to `GmailClient` (reusing `build_rfc822` + the JSON helpers); carry drafts through the existing M11/M12 "active list" via one additive `MessagePreview.draft_id`; four DB-free commands drive the lifecycle; `ComposeModal` gains Save-as-draft + draft editing + a dirty-close prompt + the failed-send fallback. Drafts are never cached — no DB migration, no new OAuth scope.

**Tech Stack:** Rust (reqwest, serde, Tauri 2, base64; wiremock tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT):** the owner is learning Rust — every Rust edit carries a concise `// 🦀` comment on the *language* concept, matching the voice in `gmail/mod.rs`. Give a short plain-English Rust recap after each Rust task. TS/React uses normal comments.

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m14-drafts-outbox-design.md`

**Existing patterns to mirror (read before starting):** `gmail/mod.rs` `send_message`/`modify_message`/`get_reply_context` (JSON helpers `post_json`/`get_json`/`delete_no_body`, base64url `raw`, `skip_serializing_if` on `threadId`, the `collect_body` MIME walk, the `header` closure); `tests/gmail_test.rs` (`GmailClient::with_base_url("tok".into(), server.uri())`, `Mock::given(method(...)).and(path(...)).and(body_json(...)).respond_with(ResponseTemplate::new(200).set_body_json(json!({...})))`, the `b64url` helper); `commands.rs` `fetch_folder`/`send_email`; `ComposeModal.tsx`; `SettingsModal.tsx` (the inline-confirm pattern).

---

## Task 1: GmailClient — draft write path (`create_draft` + `update_draft`) + types + `put_json`

**Files:**
- Modify: `src-tauri/src/gmail/types.rs` (add `DraftRef`, `DraftContent`)
- Modify: `src-tauri/src/gmail/mod.rs` (add `put_json`, the private draft wire structs, `create_draft`, `update_draft`)
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn create_draft_posts_raw_and_returns_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .and(body_json(json!({ "message": { "raw": b64url("hello-draft") } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "dr1", "message": { "id": "m1" } })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let id = client.create_draft("hello-draft", None).await.unwrap();
    assert_eq!(id, "dr1");
}

#[tokio::test(flavor = "multi_thread")]
async fn update_draft_puts_raw_with_thread_id() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/gmail/v1/users/me/drafts/dr1"))
        .and(body_json(json!({ "message": { "raw": b64url("edited"), "threadId": "t9" } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "dr1", "message": { "id": "m2" } })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let id = client.update_draft("dr1", "edited", Some("t9")).await.unwrap();
    assert_eq!(id, "dr1");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --test gmail_test create_draft update_draft`
Expected: FAIL — `create_draft`/`update_draft` not found.

- [ ] **Step 3: Add the public types**

In `src-tauri/src/gmail/types.rs`, after the `ReplyContext` struct (~line 161), add:

```rust
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
```

- [ ] **Step 4: Add `put_json` + the create/update methods**

In `src-tauri/src/gmail/mod.rs`, add a `put_json` helper right after `post_json` (~line 163):

```rust
    // 🦀 PUT twin of post_json — Gmail's drafts.update replaces a draft via PUT.
    //    Same generics: `B` the request body, `T` the parsed response.
    async fn put_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self
            .http
            .put(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }
```

Then add the private draft wire structs + the two methods at the end of the `impl GmailClient` block, just before its closing `}` (~line 525, after `modify_message`):

```rust
    /// Create a new draft from a raw RFC822 message. Returns the new draft's id.
    pub async fn create_draft(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<String> {
        let url = format!("{}/gmail/v1/users/me/drafts", self.base_url);
        let body = DraftWriteBody {
            message: DraftWriteMessage { raw: encode_raw(raw_rfc822), thread_id },
        };
        let resp: DraftIdResponse = self.post_json(&url, &body).await?;
        Ok(resp.id)
    }

    /// Replace an existing draft's message (drafts.update). Returns the draft id.
    pub async fn update_draft(&self, draft_id: &str, raw_rfc822: &str, thread_id: Option<&str>) -> Result<String> {
        let url = format!("{}/gmail/v1/users/me/drafts/{}", self.base_url, draft_id);
        let body = DraftWriteBody {
            message: DraftWriteMessage { raw: encode_raw(raw_rfc822), thread_id },
        };
        let resp: DraftIdResponse = self.put_json(&url, &body).await?;
        Ok(resp.id)
    }
```

Add these module-level helpers + private structs near the other free functions in `gmail/mod.rs` (e.g. just below the `impl` block / next to `collect_body`):

```rust
// 🦀 Gmail wants the whole RFC822 message base64url-encoded (web-safe, no padding) in
//    `raw`. Factored out so create/update/send share one encoding (send_message inlines
//    its own; leaving that untouched to avoid churn).
fn encode_raw(raw_rfc822: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw_rfc822.as_bytes())
}

// 🦀 Request bodies for drafts.create/update/send. `skip_serializing_if` drops `threadId`
//    entirely (not null) when there's no thread — Gmail rejects a null threadId.
#[derive(serde::Serialize)]
struct DraftWriteBody<'a> {
    message: DraftWriteMessage<'a>,
}
#[derive(serde::Serialize)]
struct DraftWriteMessage<'a> {
    raw: String,
    #[serde(rename = "threadId", skip_serializing_if = "Option::is_none")]
    thread_id: Option<&'a str>,
}

// 🦀 We only need the draft id back from create/update; ignore the nested message.
#[derive(serde::Deserialize)]
struct DraftIdResponse {
    id: String,
}
```

Ensure `DraftRef`/`DraftContent` are in scope in `mod.rs` the same way `MessagePreview` is (the existing `use` of the `types` module). If `MessagePreview` is referenced unqualified, `DraftRef`/`DraftContent` will be too once added to `types.rs`.

- [ ] **Step 5: Run to verify they pass + clippy**

Run: `cd src-tauri && cargo test --test gmail_test create_draft update_draft && cargo clippy --lib --tests`
Expected: both tests PASS; clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m14): GmailClient create_draft/update_draft + put_json + draft types"
```

**🦀 Recap:** PUT vs POST are just different HTTP verbs over the same JSON helper shape; `skip_serializing_if` lets one body struct serve both "with thread" and "without thread" cases by omitting the field entirely.

---

## Task 2: GmailClient — draft read path (`list_drafts` + `get_draft`)

**Files:**
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn list_drafts_returns_draft_and_message_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "drafts": [
                { "id": "dr1", "message": { "id": "m1", "threadId": "t1" } },
                { "id": "dr2", "message": { "id": "m2" } }
            ]
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let refs = client.list_drafts(25).await.unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].id, "dr1");
    assert_eq!(refs[0].message_id, "m1");
    assert_eq!(refs[1].message_id, "m2");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_draft_parses_headers_and_text_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/drafts/dr1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "dr1",
            "message": {
                "threadId": "t1",
                "payload": {
                    "mimeType": "text/plain",
                    "headers": [
                        { "name": "To", "value": "maya@studio.co" },
                        { "name": "Subject", "value": "Re: Q3" },
                        { "name": "References", "value": "<a@x>" }
                    ],
                    "body": { "data": b64url("draft body text") }
                }
            }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let d = client.get_draft("dr1").await.unwrap();
    assert_eq!(d.draft_id, "dr1");
    assert_eq!(d.to, "maya@studio.co");
    assert_eq!(d.subject, "Re: Q3");
    assert_eq!(d.body, "draft body text");
    assert_eq!(d.references.as_deref(), Some("<a@x>"));
    assert_eq!(d.in_reply_to, None);
    assert_eq!(d.thread_id.as_deref(), Some("t1"));
    assert_eq!(d.cc, "");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --test gmail_test list_drafts get_draft`
Expected: FAIL — methods not found.

- [ ] **Step 3: Implement the methods + their wire structs**

In `gmail/mod.rs`, add to the `impl GmailClient` block (next to the create/update methods):

```rust
    /// List up to `max` drafts as (draft id, message id) pairs (for the Drafts folder).
    pub async fn list_drafts(&self, max: u32) -> Result<Vec<DraftRef>> {
        let url = format!("{}/gmail/v1/users/me/drafts?maxResults={}", self.base_url, max);
        let resp: DraftListResponse = self.get_json(&url).await?;
        // 🦀 `into_iter().map(...).collect()` builds the clean Vec<DraftRef> from the
        //    nested wire items, moving the owned ids out (no clones).
        Ok(resp
            .drafts
            .into_iter()
            .map(|d| DraftRef { id: d.id, message_id: d.message.id })
            .collect())
    }

    /// Fetch one draft's editable content (recipients, subject, plain-text body, threading).
    pub async fn get_draft(&self, draft_id: &str) -> Result<DraftContent> {
        let url = format!("{}/gmail/v1/users/me/drafts/{}?format=full", self.base_url, draft_id);
        let resp: DraftGetResponse = self.get_json(&url).await?;
        // 🦀 Same case-insensitive header closure as get_reply_context, but returning
        //    Option so absent In-Reply-To/References become None (not "").
        let header = |name: &str| {
            resp.message
                .payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
        };
        let mut html = None;
        let mut text = None;
        collect_body(&resp.message.payload, &mut html, &mut text);
        // 🦀 Empty threadId string → None (a draft for a brand-new message has no thread).
        let thread_id = if resp.message.thread_id.is_empty() {
            None
        } else {
            Some(resp.message.thread_id)
        };
        Ok(DraftContent {
            draft_id: resp.id,
            to: header("To").unwrap_or_default(),
            cc: header("Cc").unwrap_or_default(),
            subject: header("Subject").unwrap_or_default(),
            body: text.unwrap_or_default(),
            in_reply_to: header("In-Reply-To"),
            references: header("References"),
            thread_id,
        })
    }
```

Add the private wire structs near the other draft structs:

```rust
// 🦀 drafts.list shape: a list of { id (draft), message: { id (message) } }.
#[derive(serde::Deserialize)]
struct DraftListResponse {
    #[serde(default)]
    drafts: Vec<DraftListItem>,
}
#[derive(serde::Deserialize)]
struct DraftListItem {
    id: String,
    message: DraftMsgRef,
}
#[derive(serde::Deserialize)]
struct DraftMsgRef {
    id: String,
}

// 🦀 drafts.get?format=full shape: the draft id + its full message (threadId + payload).
//    Reuses the existing MessagePart type for the MIME payload.
#[derive(serde::Deserialize)]
struct DraftGetResponse {
    id: String,
    message: DraftGetMessage,
}
#[derive(serde::Deserialize)]
struct DraftGetMessage {
    #[serde(rename = "threadId", default)]
    thread_id: String,
    payload: MessagePart,
}
```

(`MessagePart` and `collect_body` already exist in `mod.rs`/`types.rs` — reuse them.)

- [ ] **Step 4: Run to verify they pass + clippy**

Run: `cd src-tauri && cargo test --test gmail_test list_drafts get_draft && cargo clippy --lib --tests`
Expected: both PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m14): GmailClient list_drafts + get_draft (reuses MIME walk)"
```

**🦀 Recap:** `get_draft` reuses two existing pieces — the case-insensitive `header` closure and the recursive `collect_body` MIME walk — so a draft is parsed exactly like a received message; the only new work is mapping empty strings/absent headers to `None`.

---

## Task 3: GmailClient — `send_draft` + `delete_draft`

**Files:**
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn send_draft_posts_id_and_raw() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts/send"))
        .and(body_json(json!({ "id": "dr1", "message": { "raw": b64url("final") } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "sent1" })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client.send_draft("dr1", "final", None).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_draft_issues_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/gmail/v1/users/me/drafts/dr1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client.delete_draft("dr1").await.unwrap();
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --test gmail_test send_draft delete_draft`
Expected: FAIL — methods not found.

- [ ] **Step 3: Implement**

In `gmail/mod.rs` `impl GmailClient`:

```rust
    /// Send an existing draft, applying the latest edits, in one call. Gmail removes it
    /// from Drafts. (Single-call form: `POST /drafts/send` with `{ id, message:{raw} }`.
    /// If a future live test shows it doesn't apply edits, switch to `update_draft` then
    /// `POST /drafts/send` with `{ id }` — the public signature is unaffected.)
    pub async fn send_draft(&self, draft_id: &str, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/drafts/send", self.base_url);
        let body = DraftSendBody {
            id: draft_id,
            message: DraftWriteMessage { raw: encode_raw(raw_rfc822), thread_id },
        };
        // 🦀 Discard the returned Message resource into a throwaway Value, like send_message.
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    /// Permanently delete a draft (drafts.delete). No reading-pane equivalent — drafts
    /// aren't trashed, they're removed.
    pub async fn delete_draft(&self, draft_id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/drafts/{}", self.base_url, draft_id);
        self.delete_no_body(&url).await
    }
```

Add the send-body struct next to the other draft structs:

```rust
// 🦀 drafts.send wants a Draft resource: the draft id PLUS the (edited) message.
#[derive(serde::Serialize)]
struct DraftSendBody<'a> {
    id: &'a str,
    message: DraftWriteMessage<'a>,
}
```

- [ ] **Step 4: Run to verify + clippy**

Run: `cd src-tauri && cargo test --test gmail_test send_draft delete_draft && cargo clippy --lib --tests`
Expected: PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m14): GmailClient send_draft + delete_draft"
```

**🦀 Recap:** `send_draft` reuses `post_json` and the same `DraftWriteMessage`; `delete_draft` reuses `delete_no_body` (added in M12) — the milestone keeps adding thin methods over a small set of shared HTTP helpers.

---

## Task 4: `MessagePreview.draft_id` (additive field, all three constructors)

**Files:**
- Modify: `src-tauri/src/gmail/types.rs` (struct)
- Modify: `src-tauri/src/gmail/mod.rs` (the `get_message_preview` constructor)
- Modify: `src-tauri/src/commands.rs` (the `fetch_inbox_preview` mapper + the test helper)

- [ ] **Step 1: Add the field**

In `src-tauri/src/gmail/types.rs`, in `struct MessagePreview` after `pub category: String,` (~line 121):

```rust
    /// Set only on the drafts-fetch path (a draft id wraps a message id); `None` elsewhere.
    // 🦀 `skip_serializing_if` omits the key from JSON when None, so the frontend sees
    //    `undefined` (matching `draft_id?: string`) rather than an explicit `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
```

- [ ] **Step 2: Run to verify it fails to compile (missing field in 3 constructors)**

Run: `cd src-tauri && cargo build`
Expected: FAIL — `missing field draft_id` at `gmail/mod.rs:236`, `commands.rs:149`, `commands.rs:537`.

- [ ] **Step 3: Set `draft_id: None` in the three constructors**

`src-tauri/src/gmail/mod.rs` — in `get_message_preview`, after `category: String::new(), // scored at sync time, not here`:
```rust
            draft_id: None, // populated only by the drafts command (fetch_folder "drafts" arm)
```

`src-tauri/src/commands.rs` — in `fetch_inbox_preview`'s `.map(|m| MessagePreview { … })`, after `category: m.category,`:
```rust
            draft_id: None,
```

`src-tauri/src/commands.rs` — in the test helper `preview(...)` (~line 537), after its `category: …,` line:
```rust
            draft_id: None,
```

- [ ] **Step 4: Verify it builds + tests pass**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/src/commands.rs
git commit -m "feat(m14): additive MessagePreview.draft_id (None everywhere but drafts)"
```

**🦀 Recap:** adding one `Option<String>` field forces every struct literal to set it — the compiler's "missing field" error is a feature here, listing exactly the three constructors that need `draft_id: None`.

---

## Task 5: Commands — drafts arm in `fetch_folder` + `get_draft`/`save_draft`/`send_draft`/`delete_draft`

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (register the four commands)

- [ ] **Step 1: Add a `"drafts"` arm to `fetch_folder`**

In `src-tauri/src/commands.rs`, replace the body of `fetch_folder` (currently ~lines 399–418) with this (moves client creation up, adds the drafts branch, keeps the existing label/query path verbatim):

```rust
#[tauri::command]
pub async fn fetch_folder(folder: String, max: u32) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);

    // Drafts have their own API (a draft id wraps a message id), so they can't go through
    // the generic label/query path — fetch the (draft id, message id) pairs, hydrate the
    // messages, then stamp each preview's draft_id so the editor can open it.
    if folder == "drafts" {
        let refs = client.list_drafts(max).await?;
        let ids: Vec<String> = refs.iter().map(|d| d.message_id.clone()).collect();
        let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
        // 🦀 Build a message_id -> draft_id lookup, then stamp each hydrated preview.
        let by_msg: std::collections::HashMap<&str, &str> =
            refs.iter().map(|d| (d.message_id.as_str(), d.id.as_str())).collect();
        for p in &mut previews {
            p.draft_id = by_msg.get(p.id.as_str()).map(|s| s.to_string());
        }
        previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
        return Ok(previews);
    }

    // 🦀 A match returning a tuple: each folder picks its (label, query, includeSpamTrash).
    let (label, query, include_spam_trash): (Option<&str>, &str, bool) = match folder.as_str() {
        "sent" => (Some("SENT"), "", false),
        "starred" => (Some("STARRED"), "", false),
        "trash" => (Some("TRASH"), "", true),
        "spam" => (Some("SPAM"), "", true),
        "archive" => (None, "-in:inbox -in:sent -in:trash -in:spam", false),
        other => return Err(AppError::Other(format!("unknown folder: {other}"))),
    };
    let ids = client.list_message_ids(label, query, max, include_spam_trash).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
    Ok(previews)
}
```

- [ ] **Step 2: Add the four draft commands**

Immediately after `fetch_folder`, add:

```rust
/// Fetch one draft's editable content (DB-free). Used to open a draft in the compose editor.
#[tauri::command]
pub async fn get_draft(draft_id: String) -> Result<crate::gmail::DraftContent> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.get_draft(&draft_id).await
}

/// Create (when `draft_id` is None) or update an existing draft. Returns the draft id.
/// DB-free. Reuses the M8 RFC822 builder. No recipient validation — drafts may be partial.
#[tauri::command]
pub async fn save_draft(
    draft_id: Option<String>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    thread_id: Option<String>,
) -> Result<String> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let msg = crate::mime::OutgoingMessage { from: stored.email, to, cc, subject, body, in_reply_to, references };
    let raw = crate::mime::build_rfc822(&msg);
    // 🦀 `match` on the Option: Some(id) updates that draft, None creates a fresh one.
    match draft_id {
        Some(id) => client.update_draft(&id, &raw, thread_id.as_deref()).await,
        None => client.create_draft(&raw, thread_id.as_deref()).await,
    }
}

/// Send an existing draft (applying the latest field edits). DB-free.
#[tauri::command]
pub async fn send_draft(
    draft_id: String,
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    thread_id: Option<String>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let msg = crate::mime::OutgoingMessage { from: stored.email, to, cc, subject, body, in_reply_to, references };
    let raw = crate::mime::build_rfc822(&msg);
    client.send_draft(&draft_id, &raw, thread_id.as_deref()).await
}

/// Permanently delete a draft. DB-free.
#[tauri::command]
pub async fn delete_draft(draft_id: String) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.delete_draft(&draft_id).await
}
```

Make sure `DraftContent` resolves: it's `crate::gmail::DraftContent` above (fully-qualified, so no new `use` needed). Confirm `crate::gmail` re-exports it (it lives in `gmail::types` and is glob-re-exported like `MessagePreview`).

- [ ] **Step 3: Register the commands**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]` (after `commands::fetch_folder,`), add:

```rust
            commands::get_draft,
            commands::save_draft,
            commands::send_draft,
            commands::delete_draft,
```

- [ ] **Step 4: Verify it builds + clippy + full tests**

Run: `cd src-tauri && cargo build && cargo clippy --all-targets && cargo test`
Expected: builds; clippy clean; all tests pass (the new gmail draft tests included).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(m14): drafts arm in fetch_folder + get/save/send/delete_draft commands"
```

**🦀 Recap:** the draft commands mirror `send_email` exactly (build `OutgoingMessage` → `build_rfc822` → call the client), and the `"drafts"` arm short-circuits `fetch_folder` before the generic label path because drafts need their own id-mapping.

---

## Task 6: Frontend — api wrappers, types, folders entry, mocks

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/lib/folders.ts`
- Modify: `src/lib/mock.ts`

- [ ] **Step 1: `lib/folders.ts` — add the Drafts folder**

Replace the contents of `src/lib/folders.ts`:

```ts
// src/lib/folders.ts — the mailbox folders shown in the left rail (M12; Drafts added M14).
export type Folder = "inbox" | "sent" | "drafts" | "starred" | "archive" | "trash" | "spam";

export interface FolderDef {
  key: Folder;
  label: string;
}

export const FOLDERS: FolderDef[] = [
  { key: "inbox", label: "Inbox" },
  { key: "sent", label: "Sent" },
  { key: "drafts", label: "Drafts" },
  { key: "starred", label: "Starred" },
  { key: "archive", label: "Archive" },
  { key: "trash", label: "Trash" },
  { key: "spam", label: "Spam" },
];
```

- [ ] **Step 2: `lib/api.ts` — draft_id field, DraftContent, wrappers**

In `src/lib/api.ts`: add `draft_id?: string;` to the `MessagePreview` interface (after `to_addr`). Update the mock import line (line 3) to also import the new mock helpers:

```ts
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek, mockSearch, mockFolder, mockGetDraft, mockSaveDraft } from "./mock";
```

Add, after the `getReplyContext` export (~line 100):

```ts
export interface DraftContent {
  draft_id: string;
  to: string;
  cc: string;
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
}

export const getDraft = (id: string): Promise<DraftContent> =>
  isTauri() ? invoke<DraftContent>("get_draft", { draftId: id }) : Promise.resolve(mockGetDraft(id));

// A save payload is a send payload plus the draft id (null when creating a new draft).
export const saveDraft = (p: SendEmailPayload & { draft_id: string | null }): Promise<string> =>
  isTauri()
    ? invoke<string>("save_draft", {
        draftId: p.draft_id,
        to: p.to,
        cc: p.cc,
        subject: p.subject,
        body: p.body,
        inReplyTo: p.in_reply_to,
        references: p.references,
        threadId: p.thread_id,
      })
    : Promise.resolve(mockSaveDraft());

export const sendDraft = (p: SendEmailPayload & { draft_id: string }): Promise<void> =>
  isTauri()
    ? invoke<void>("send_draft", {
        draftId: p.draft_id,
        to: p.to,
        cc: p.cc,
        subject: p.subject,
        body: p.body,
        inReplyTo: p.in_reply_to,
        references: p.references,
        threadId: p.thread_id,
      })
    : Promise.resolve();

export const deleteDraft = (id: string): Promise<void> =>
  isTauri() ? invoke<void>("delete_draft", { draftId: id }) : Promise.resolve();
```

- [ ] **Step 3: `lib/mock.ts` — drafts in the maket**

In `src/lib/mock.ts`: add `DraftContent` to the type import from `./api`:
```ts
import type { MessagePreview, SyncSummary, DraftContent } from "./api";
```
Add a `"drafts"` case to `mockFolder`'s `switch` (the `base(...)` helper is already in scope there):
```ts
    case "drafts":
      return [
        { ...base("dm1", MOCK_ACCOUNT, "Maya <maya@studio.co>", "Re: Q3 roadmap", "Draft: I think we should…"), draft_id: "dr1" },
        { ...base("dm2", MOCK_ACCOUNT, "", "(no recipient)", "Half-written idea…"), draft_id: "dr2" },
      ];
```
Add the draft mock helpers at the end of the file:
```ts
/** Browser-maket: return editable content for a mock draft. */
export function mockGetDraft(draftId: string): DraftContent {
  if (draftId === "dr2") {
    return { draft_id: "dr2", to: "", cc: "", subject: "", body: "Half-written idea…", in_reply_to: null, references: null, thread_id: null };
  }
  return { draft_id: "dr1", to: "Maya <maya@studio.co>", cc: "", subject: "Re: Q3 roadmap", body: "Draft: I think we should…", in_reply_to: null, references: null, thread_id: null };
}

/** Browser-maket: pretend a save succeeded, returning a stable fake draft id. */
export function mockSaveDraft(): string {
  return "dr-mock";
}
```

- [ ] **Step 4: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS (`tsc && vite build`). The wrappers/mocks aren't wired into UI yet.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/lib/folders.ts src/lib/mock.ts
git commit -m "feat(m14): draft api wrappers + DraftContent type + Drafts folder + mocks"
```

---

## Task 7: ComposeModal — Save-as-draft, draft editing, send-from-draft, close-prompt, failed-send fallback

**Files:**
- Modify: `src/components/ComposeModal.tsx`

- [ ] **Step 1: Extend the imports + `ComposeInitial`**

In `src/components/ComposeModal.tsx`, change the api import (line 2):

```tsx
import { sendEmail, saveDraft, sendDraft, deleteDraft, type SendEmailPayload } from "../lib/api";
```

Add `draftId` to `ComposeInitial`:

```tsx
export interface ComposeInitial {
  to: string; // comma-separated text (prefilled for reply)
  cc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  threadId: string | null;
  draftId?: string | null; // set when editing an existing Gmail draft; optional so existing
                           // `setCompose({…})` literals in App.tsx keep compiling unchanged
}
```

- [ ] **Step 2: Extend props + state**

Change the component signature/props to add `onDraftsChanged`:

```tsx
export function ComposeModal({
  initial,
  onClose,
  onSent,
  onDraftsChanged,
}: {
  initial: ComposeInitial;
  onClose: () => void;
  onSent: () => void;
  onDraftsChanged?: () => void; // called after a save/discard so the parent can refresh Drafts
}) {
```

Add state after the existing `error` state (~line 31):

```tsx
  const [busy, setBusy] = useState(false); // save/discard in flight
  const [confirmingClose, setConfirmingClose] = useState(false);
  const [draftId, setDraftId] = useState<string | null>(initial.draftId ?? null);
```

- [ ] **Step 3: Dirty check + close handling + Esc**

Add below the state, then change the Esc effect to call `attemptClose`:

```tsx
  // "Dirty" = worth offering to save. A brand-new compose holding only its seeded body
  // (signature) is not dirty; editing a draft is dirty as soon as the body changes.
  const dirty =
    to.trim() !== "" || cc.trim() !== "" || subject.trim() !== "" || body !== initial.body;

  function attemptClose() {
    if (dirty) setConfirmingClose(true);
    else onClose();
  }
```

Change the Esc handler (~line 36) from `if (e.key === "Escape") onClose();` to:

```tsx
      if (e.key === "Escape") attemptClose();
```
and update the effect dependency array from `[onClose]` to `[onClose, dirty]`.

- [ ] **Step 4: Title + save/send/discard handlers**

Change the title line (~line 42):
```tsx
  const title = draftId ? "Draft" : initial.threadId ? "Reply" : "New message";
```

Replace `handleSend` (~lines 44–75) with the draft-aware version + add `handleSaveDraft` and `handleDeleteDraft`:

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
    };
  }

  async function handleSend() {
    const f = fields();
    if (f.to.length === 0 || !f.to.every(isPlausibleEmail)) {
      setError("Enter at least one valid recipient address.");
      return;
    }
    if (f.cc.length > 0 && !f.cc.every(isPlausibleEmail)) {
      setError("One of the Cc addresses looks invalid.");
      return;
    }
    setSending(true);
    setError(null);
    try {
      if (draftId) await sendDraft({ ...f, draft_id: draftId });
      else await sendEmail(f);
      onSent();
    } catch (e) {
      // Minimal outbox: a failed send becomes a saved draft so nothing is lost.
      try {
        const id = await saveDraft({ ...f, draft_id: draftId });
        setDraftId(id);
        onDraftsChanged?.();
        setError(`Couldn't send (${String(e)}). Saved to Drafts — retry from there.`);
      } catch {
        setError("Couldn't send or save — you appear to be offline. Your message is still here.");
      }
    } finally {
      setSending(false);
    }
  }

  // Save without sending. No recipient validation — a draft can be incomplete.
  async function handleSaveDraft() {
    setBusy(true);
    setError(null);
    try {
      const id = await saveDraft({ ...fields(), draft_id: draftId });
      setDraftId(id);
      onDraftsChanged?.();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // Discard (permanently delete) the draft being edited.
  async function handleDeleteDraft() {
    if (!draftId) return;
    setBusy(true);
    setError(null);
    try {
      await deleteDraft(draftId);
      onDraftsChanged?.();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
```

- [ ] **Step 5: Update the close button + actions row**

Change the header close button (~line 87) `onClick={onClose}` to `onClick={attemptClose}`.

Replace the actions block (~lines 124–135) with:

```tsx
        {confirmingClose ? (
          <div className="compose-actions">
            <span className="settings-label">Save this draft before closing?</span>
            <button className="btn" onClick={onClose} disabled={busy}>
              Discard
            </button>
            <button className="btn btn-accent" onClick={handleSaveDraft} disabled={busy}>
              Save draft
            </button>
          </div>
        ) : (
          <div className="compose-actions">
            {draftId && (
              <button className="btn btn-danger-outline" onClick={handleDeleteDraft} disabled={sending || busy}>
                Delete draft
              </button>
            )}
            <button className="btn" onClick={attemptClose} disabled={sending || busy}>
              Cancel
            </button>
            <button className="btn" onClick={handleSaveDraft} disabled={sending || busy}>
              Save as draft
            </button>
            <button className="btn btn-accent" onClick={handleSend} disabled={sending || busy}>
              {sending ? "Sending…" : "Send"}
            </button>
          </div>
        )}
```

(`btn-danger-outline` already exists — it's used by SettingsModal's Disconnect.)

- [ ] **Step 6: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. `draftId` is **optional** on `ComposeInitial`, and `onDraftsChanged` is optional, so App.tsx's existing `setCompose({…})` literals and `<ComposeModal/>` call site still compile unchanged — the build stays green even though drafts aren't wired into App yet (Task 8).

- [ ] **Step 7: Commit**

```bash
git add src/components/ComposeModal.tsx
git commit -m "feat(m14): ComposeModal save-as-draft, draft edit/send, close-prompt, send fallback"
```

---

## Task 8: App — Drafts folder opens the editor; refresh after save/send/discard

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Imports**

In `src/App.tsx`, add `getDraft` to the api import block (alongside `fetchFolder` etc.):

```tsx
  getDraft,
```

- [ ] **Step 2: `draftId` on the two existing compose literals (optional, for clarity)**

`draftId` is optional on `ComposeInitial`, so these compile unchanged — but add `draftId: null,` to the `setCompose({ … })` objects in `openNewCompose` (~line 225) and `handleReply` (~line 244) to make "this is a fresh, non-draft compose" explicit. (Skip if you prefer; it has no behavioral effect.)

- [ ] **Step 3: Add `handleOpenDraft` + a row-select dispatcher**

After `handleSelect` (~line 263), add:

```tsx
  // Drafts open the compose editor (not the reading pane). Fetch the draft's content and
  // seed ComposeModal with its draftId so Save/Send target the existing draft.
  async function handleOpenDraft(m: MessagePreview) {
    if (!m.draft_id) return;
    setError(null);
    try {
      const d = await getDraft(m.draft_id);
      setCompose({
        to: d.to,
        cc: d.cc,
        subject: d.subject,
        body: d.body,
        inReplyTo: d.in_reply_to,
        references: d.references,
        threadId: d.thread_id,
        draftId: d.draft_id,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  // Row click: in the Drafts folder, open the editor; everywhere else, normal select.
  function handleRowSelect(id: string) {
    if (folder === "drafts") {
      const m = activeList.find((x) => x.id === id);
      if (m) void handleOpenDraft(m);
    } else {
      handleSelect(id);
    }
  }
```

- [ ] **Step 4: Wire MessageList + showRecipient**

In the `<MessageList … />` render, change `onSelect={handleSelect}` to `onSelect={handleRowSelect}`, and change `showRecipient={folder === "sent"}` to:

```tsx
                showRecipient={folder === "sent" || folder === "drafts"}
```

- [ ] **Step 5: Wire ComposeModal's new callbacks**

Replace the `<ComposeModal … />` render block (~lines 413–422) with:

```tsx
      {compose && (
        <ComposeModal
          initial={compose}
          onClose={() => setCompose(null)}
          onSent={() => {
            setCompose(null);
            setStatus("Sent ✓");
            // A sent draft disappears from Drafts — refresh if we're viewing them.
            if (folder === "drafts") setFolderReloadKey((k) => k + 1);
          }}
          onDraftsChanged={() => {
            if (folder === "drafts") setFolderReloadKey((k) => k + 1);
          }}
        />
      )}
```

- [ ] **Step 6: Verify it builds + maket smoke check**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS (the `draftId` literals from Task 7 are now satisfied).

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx
git commit -m "feat(m14): Drafts folder opens the editor + refresh after save/send/discard"
```

---

## Task 9: Verification, roadmap & wiki

**Files:**
- Modify: `wiki/entities/ember.md`, `wiki/log.md` (local-only, gitignored — edits live on disk, not committed)

- [ ] **Step 1: Full backend + frontend verification**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test && cargo clippy --all-targets`
Expected: all pass (prior tests + 6 new draft tests); clippy clean. Report the total count.
Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS.

- [ ] **Step 2: Maket check (screenshot)**

Run `npm run dev`; in the browser: open the rail → **Drafts** (lists two mock drafts showing "To:" recipients) → click one (opens ComposeModal pre-filled, titled "Draft", with **Save as draft** / **Delete draft** / **Send**) → edit + Cancel (the "Save this draft before closing?" prompt appears). Screenshot the Drafts list and the open editor.

- [ ] **Step 3: Update the wiki roadmap**

In `wiki/entities/ember.md`: bump `updated:` to `2026-06-20`; add an M14 bullet after M13; update the closing "As of M13…" paragraph to "As of M14…" mentioning drafts. M14 bullet:

```
- **M14 — Drafts & outbox (lean v1)** — *implemented on branch `m14-drafts-outbox`, pending merge.*
  First of the M14→M15→M16→M17 arc. Real **Gmail drafts** via new `GmailClient` methods over
  `users.drafts.*` (`create_draft`/`update_draft`/`get_draft`/`list_drafts`/`send_draft`/`delete_draft`),
  reusing the M8 `build_rfc822`, the JSON helpers (+ a new `put_json`), and the `collect_body` MIME walk.
  A **Drafts folder** in the rail: `fetch_folder` gained a `"drafts"` arm that lists drafts, hydrates via
  the concurrent `get_message_previews`, and stamps each preview's new additive `MessagePreview.draft_id`
  (None everywhere else; `skip_serializing_if`); drafts are the one folder whose rows **open the compose
  editor**, not the reading pane. `ComposeModal` gained **Save as draft**, draft editing (titled "Draft",
  with **Delete draft**), **send-from-draft** (`drafts.send` removes it), a **dirty-close prompt**, and the
  **minimal outbox** — a failed send saves the message as a draft ("retry from there"). Four DB-free
  commands (`get_draft`/`save_draft`/`send_draft`/`delete_draft`). **No DB migration, no new OAuth scope.**
  N tests (6 new gmail wiremock), clippy clean, `npm run build` clean. Maket verified by screenshot.
  **Live Gmail E2E pending owner** (real draft create/edit/send round-trip + the failed-send fallback).
  **Deferred:** auto-save, full background outbox/auto-retry, cross-device conflict UI, HTML/rich-text
  drafts, attachments-in-drafts (→ M17), scheduled send. Plan/spec under `docs/superpowers/`.
```

(Replace `N` with the actual total test count from Step 1.) Append a one-line `wiki/log.md` entry in the file's existing format.

- [ ] **Step 4: (No git commit — `wiki/` is gitignored.)**

The wiki edits live on disk only; do not `git add -f` them.

---

## Self-review (completed by plan author)

**Spec coverage:** Gmail drafts create/update/get/list/send/delete (T1–T3) ✓; `MessagePreview.draft_id` additive, no migration (T4) ✓; `fetch_folder "drafts"` arm + 4 commands + register (T5) ✓; Drafts rail folder (T6) ✓; api wrappers + DraftContent + mocks (T6) ✓; ComposeModal Save-as-draft + edit + send-from-draft + dirty-close prompt + failed-send fallback + delete (T7) ✓; App drafts-open-editor + refresh + showRecipient (T8) ✓; no new scope / no migration / isTauri maket (throughout) ✓; verification + wiki (T9) ✓; Rust learning comments (T1–T5) ✓; wiremock tests mirroring M12 (T1–T3) ✓.

**Placeholder scan:** no TBD/TODO; every code step shows full code; the one API uncertainty (drafts.send single-call) is implemented with a documented fallback, not deferred. `N` in the wiki bullet is explicitly "replace with the count from Step 1."

**Type/name consistency:** `DraftRef { id, message_id }`, `DraftContent { draft_id, to, cc, subject, body, in_reply_to, references, thread_id }`, `create_draft/update_draft/get_draft/list_drafts/send_draft/delete_draft`, commands `get_draft/save_draft/send_draft/delete_draft`, TS `getDraft/saveDraft/sendDraft/deleteDraft`, `MessagePreview.draft_id`, `ComposeInitial.draftId`, `onDraftsChanged`, `handleOpenDraft/handleRowSelect` — all consistent across tasks. `encode_raw` defined in T1, reused in T3. The three `MessagePreview` constructors enumerated in T4 match the grep (`gmail/mod.rs:236`, `commands.rs:149`, `commands.rs:537`).
