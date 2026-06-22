// 🦀 `pub mod types;` makes the sibling file `types.rs` a *public submodule*
//    of this `gmail` module.  The full path is `ember_lib::gmail::types::Profile`.
pub mod types;

use std::collections::HashMap;
use types::{
    AttachmentMeta, FullMessage, HistoryResponse, Label, LabelColor, MessageList, MessagePart,
    MessagePreview, ModifiedMessage, Profile, RawMessage, ReplyContext,
};
use types::{DraftContent, DraftRef};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://gmail.googleapis.com";

pub struct GmailClient {
    base_url: String,
    access_token: String,
    // 🦀 `reqwest::Client` holds a connection pool and HTTP configuration.
    //    It is cheaply cloneable (Arc-backed internally) and is meant to be
    //    reused across requests — one instance per logical "service" is typical.
    http: reqwest::Client,
}

/// The net result of replaying history since a stored historyId.
// 🦀 `#[derive(Default)]` lets us build a value with all fields at their defaults
//    (empty Vecs, None, false) via `HistoryDelta::default()` / `..Default::default()`.
#[derive(Debug, Default, PartialEq)]
pub struct HistoryDelta {
    pub added_ids: Vec<String>,
    pub removed_ids: Vec<String>,
    pub new_history_id: Option<String>,
    /// True if Gmail returned 404 (startHistoryId too old) → caller should full-resync.
    pub too_old: bool,
}

/// Raw (un-sanitized) body extracted from a message.
pub struct RawBody {
    pub html: Option<String>,
    pub text: Option<String>,
    // 🦀 Attachments found on the message (metadata only — bytes fetched on demand).
    pub attachments: Vec<AttachmentMeta>,
}

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

// 🦀 Recursive tree walk: `&mut Option<String>` are *out-params* — the caller passes
//    in mutable references to the slots it wants filled. Each recursive call can write
//    into the same slots without returning values, which keeps the signature clean.
//    The recursion bottoms out when `part.parts` is empty (leaf node).
fn collect_body(part: &MessagePart, html: &mut Option<String>, text: &mut Option<String>) {
    match part.mime_type.as_str() {
        "text/html" if html.is_none() => *html = decode_b64url(&part.body.data),
        "text/plain" if text.is_none() => *text = decode_b64url(&part.body.data),
        _ => {}
    }
    for child in &part.parts {
        collect_body(child, html, text);
    }
}

// 🦀 Sibling of `collect_body`: a recursive MIME walk gathering attachment parts.
//    A part is an attachment when it has a non-empty `filename` AND an `attachmentId`
//    (the handle for fetching its bytes separately). `out` is an out-param Vec to push into.
//    Parts with a filename but NO `attachmentId` are intentionally skipped — Gmail inlines
//    small attachments that way (body.data holds the bytes directly); fetching them requires
//    no separate handle and is out of M17 scope.
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

impl GmailClient {
    pub fn new(access_token: String) -> Self {
        Self {
            base_url: DEFAULT_BASE.to_string(),
            access_token,
            http: reqwest::Client::new(),
        }
    }

    /// Used by tests to point the client at a mock server.
    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self {
            base_url,
            access_token,
            http: reqwest::Client::new(),
        }
    }

    // 🦀 `async fn` declares an *asynchronous function*.  Its return type is
    //    implicitly wrapped in a `Future` — calling it doesn't run the body;
    //    you must `.await` the returned future to drive it to completion.
    //    The `<T: serde::de::DeserializeOwned>` is a *generic type parameter
    //    with a trait bound*: `T` can be any type, as long as serde can
    //    deserialize it from bytes.  The compiler monomorphises a concrete
    //    version for each `T` used at call sites.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        // 🦀 The builder chain below reads left-to-right:
        //    `self.http.get(url)` — configures the HTTP method and URL,
        //    `.bearer_auth(&self.access_token)` — adds an `Authorization: Bearer …` header,
        //    `.send()` — returns a `Future`; we `.await` it to actually send,
        //    `?` — if it errors (network failure etc.) the `From` impl on
        //         `AppError` converts `reqwest::Error` and returns early,
        //    `.error_for_status()` — turns 4xx/5xx responses into an `Err`,
        //    `?` again — propagates that error the same way.
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        // 🦀 `resp.json::<T>()` reads the response body and deserializes it
        //    into type `T`.  The turbofish `::<T>` tells the compiler which
        //    concrete type to use here.  `.await?` drives the async read to
        //    completion and propagates any parse error.
        Ok(resp.json::<T>().await?)
    }

    // 🦀 POST with no request body — Gmail's trash/untrash endpoints take none. But Gmail
    //    still REQUIRES a `Content-Length: 0` header and answers 411 Length Required without
    //    it. reqwest OMITS Content-Length for any empty body (it treats zero length as "no
    //    body"), so an empty `.body("")` is NOT enough — we must set the header explicitly.
    async fn post_no_body(&self, url: &str) -> Result<()> {
        self.http
            .post(url)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::CONTENT_LENGTH, 0)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    // 🦀 DELETE with no body — Gmail's permanent-delete endpoint. Like post_no_body but the verb
    //    is DELETE. Set Content-Length: 0 explicitly here too, for the same reason.
    async fn delete_no_body(&self, url: &str) -> Result<()> {
        self.http
            .delete(url)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::CONTENT_LENGTH, 0)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    // 🦀 The write-side twin of get_json: serialize `body` to JSON, POST it with
    //    bearer auth, turn 4xx/5xx into errors, then deserialize the response into T.
    //    `B: serde::Serialize` is the request body type; `T` the response type.
    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    // 🦀 POST a JSON body but expect NO response body (Gmail's batchModify returns 204).
    //    Like post_no_body, but carries a JSON payload; we only check the status, never
    //    parse — post_json would error trying to deserialize an empty body.
    async fn post_json_no_response<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<()> {
        self.http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

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

    // 🦀 `pub async fn` — public and async.  Callers write:
    //    `let profile = client.get_profile().await?;`
    pub async fn get_profile(&self) -> Result<Profile> {
        let url = format!("{}/gmail/v1/users/me/profile", self.base_url);
        self.get_json(&url).await
    }

    pub async fn list_inbox_message_ids(&self, max: u32) -> Result<Vec<String>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages?maxResults={}&labelIds=INBOX",
            self.base_url, max
        );
        let list: MessageList = self.get_json(&url).await?;
        // 🦀 `.into_iter()` consumes the Vec, yielding owned `MessageRef` values.
        //    `.map(|m| m.id)` transforms each `MessageRef` into its `id: String`
        //    using a *closure* — the anonymous function `|m| m.id`.
        //    `.collect()` gathers the mapped values into a new `Vec<String>`.
        Ok(list.messages.into_iter().map(|m| m.id).collect())
    }

    pub async fn get_message_preview(&self, id: &str) -> Result<MessagePreview> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}?format=metadata\
             &metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date\
             &metadataHeaders=To&metadataHeaders=List-Id&metadataHeaders=List-Unsubscribe",
            self.base_url, id
        );
        let raw: RawMessage = self.get_json(&url).await?;
        // 🦀 This is a *closure that captures `raw` by reference*.
        //    `let header = |name: &str| { … };` binds a closure to `header`.
        //    Inside it uses `raw.payload.headers` — the closure *closes over*
        //    that variable, borrowing it for as long as the closure lives.
        //
        //    `.find(|h| h.name.eq_ignore_ascii_case(name))` scans the iterator
        //    and stops at the first `Header` whose name matches case-insensitively
        //    (so "from", "From", "FROM" all match).
        //
        //    `.map(|h| h.value.clone())` transforms the found `&Header` into a
        //    cloned `String` (we need an owned value to put in `MessagePreview`).
        //
        //    `.unwrap_or_default()` returns an empty `String` if no header matched
        //    — `String::default()` is `""`.
        let header = |name: &str| {
            raw.payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap_or_default()
        };
        // 🦀 Presence check: does a header with this name exist at all? Distinct from
        //    `header()` (which returns the value or ""): for List-* we care only that
        //    the header EXISTS, so an is_empty() check on the value would be wrong.
        //    `.any(...)` also avoids cloning the value just to test it.
        let has_header = |name: &str| {
            raw.payload
                .headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case(name))
        };
        // 🦀 Pull every header-derived value out FIRST, while the `header` closure's
        //    borrow of `raw.payload` is live. After the last call the borrow ends
        //    (non-lexical lifetimes), so we can then MOVE owned fields out of `raw`
        //    (no clones needed) when building the struct below.
        let from = header("From");
        let subject = header("Subject");
        let date = header("Date");
        let to_addr = header("To");
        let has_list_unsubscribe = has_header("List-Unsubscribe");
        let has_list_id = has_header("List-Id");
        let internal_date = raw.internal_date.parse::<i64>().unwrap_or(0);
        Ok(MessagePreview {
            id: raw.id,
            thread_id: raw.thread_id,
            from,
            subject,
            date,
            snippet: raw.snippet,
            internal_date,
            label_ids: raw.label_ids,
            to_addr,
            has_list_unsubscribe,
            has_list_id,
            category: String::new(), // scored at sync time, not here
            draft_id: None, // populated only by the drafts command (fetch_folder "drafts" arm)
        })
    }

    /// Shared paging loop for `messages.list`. `label = Some("INBOX")` restricts to a label; `None`
    /// searches all mail. `include_spam_trash` adds `&includeSpamTrash=true` — Gmail omits Trash/Spam
    /// messages without it. Follows `nextPageToken` up to `max_total` ids.
    // 🦀 Now `pub` so the folder command can call it directly with a label + the spam/trash flag.
    pub async fn list_message_ids(
        &self,
        label: Option<&str>,
        query: &str,
        max_total: u32,
        include_spam_trash: bool,
    ) -> Result<Vec<String>> {
        // 🦀 Percent-encode the query value so characters like ':' are URL-safe.
        let encoded_q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let mut ids = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/messages?maxResults=100&q={}",
                self.base_url, encoded_q
            );
            // 🦀 `if let Some(l) = label` runs the block only when a label was supplied, binding
            //    the inner `&str` to `l`. No label → search across all mail.
            if let Some(l) = label {
                url.push_str(&format!("&labelIds={l}"));
            }
            // 🦀 Only add the flag when asked — keeps the inbox/search requests byte-identical.
            if include_spam_trash {
                url.push_str("&includeSpamTrash=true");
            }
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={token}"));
            }
            let list: MessageList = self.get_json(&url).await?;
            for m in list.messages {
                ids.push(m.id);
                if ids.len() >= max_total as usize {
                    return Ok(ids);
                }
            }
            match list.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }
        Ok(ids)
    }

    /// List INBOX message ids matching `query` (e.g. "newer_than:30d"), following pagination up to
    /// `max_total` ids. (Sync path — behavior unchanged; now delegates to `list_message_ids`.)
    pub async fn list_inbox_message_ids_paged(
        &self,
        query: &str,
        max_total: u32,
    ) -> Result<Vec<String>> {
        self.list_message_ids(Some("INBOX"), query, max_total, false).await
    }

    /// Search across ALL mail (no label restriction) for `query`. Gmail excludes Spam/Trash by
    /// default. Follows pagination up to `max_total` ids.
    pub async fn search_message_ids(&self, query: &str, max_total: u32) -> Result<Vec<String>> {
        self.list_message_ids(None, query, max_total, false).await
    }

    /// Replay INBOX history since `start_history_id`, returning the net set of added
    /// and removed message ids. On a 404 (expired historyId), returns `too_old = true`.
    pub async fn list_history(&self, start_history_id: &str) -> Result<HistoryDelta> {
        // 🦀 A HashMap<id, bool> tracks the NET state per message across all records:
        //    true = currently in INBOX (add), false = left INBOX (remove). Later records
        //    overwrite earlier ones, so "added then archived" correctly nets to removed.
        let mut state: HashMap<String, bool> = HashMap::new();
        let mut page_token: Option<String> = None;
        let mut latest_history_id: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/history?startHistoryId={}&labelId=INBOX&maxResults=500\
                 &historyTypes=messageAdded&historyTypes=messageDeleted\
                 &historyTypes=labelAdded&historyTypes=labelRemoved",
                self.base_url, start_history_id
            );
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={token}"));
            }

            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await?;
            // 🦀 Check the status BEFORE `error_for_status()`: a 404 here is expected
            //    (the stored historyId aged out), so we treat it as data, not an error.
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(HistoryDelta {
                    too_old: true,
                    ..Default::default()
                });
            }
            let resp = resp.error_for_status()?;
            let page: HistoryResponse = resp.json().await?;

            for record in page.history {
                for m in record.messages_added {
                    state.insert(m.message.id, true);
                }
                for c in record.labels_added {
                    if c.label_ids.iter().any(|l| l == "INBOX") {
                        state.insert(c.message.id, true);
                    }
                }
                for m in record.messages_deleted {
                    state.insert(m.message.id, false);
                }
                for c in record.labels_removed {
                    if c.label_ids.iter().any(|l| l == "INBOX") {
                        state.insert(c.message.id, false);
                    }
                }
            }
            if !page.history_id.is_empty() {
                latest_history_id = Some(page.history_id);
            }
            match page.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }

        // 🦀 Split the net state into the two id lists the sync step will act on.
        let added_ids = state
            .iter()
            .filter(|(_, &present)| present)
            .map(|(id, _)| id.clone())
            .collect();
        let removed_ids = state
            .iter()
            .filter(|(_, &present)| !present)
            .map(|(id, _)| id.clone())
            .collect();
        Ok(HistoryDelta {
            added_ids,
            removed_ids,
            new_history_id: latest_history_id,
            too_old: false,
        })
    }

    /// Fetch previews for many ids concurrently (at most `concurrency` in flight).
    /// Individual fetch failures are skipped; the returned Vec's order is not guaranteed.
    pub async fn get_message_previews(
        &self,
        ids: &[String],
        concurrency: usize,
    ) -> Result<Vec<MessagePreview>> {
        // 🦀 `futures::stream` + `buffer_unordered` runs up to `concurrency` futures at
        //    once, yielding each as it finishes — replacing M1's slow one-at-a-time loop
        //    with bounded concurrency (polite to Gmail's rate limits).
        use futures::stream::StreamExt;
        let results = futures::stream::iter(ids.iter().cloned())
            .map(|id| async move { self.get_message_preview(&id).await })
            .buffer_unordered(concurrency)
            .collect::<Vec<Result<MessagePreview>>>()
            .await;
        // 🦀 Keep the successes and skip individual failures: `filter_map` drops the
        //    `Err`s (`r.ok()` turns `Result<T>` into `Option<T>`). One message that
        //    404s or gets rate-limited won't abort the whole sync — we store the rest.
        Ok(results.into_iter().filter_map(|r| r.ok()).collect())
    }

    /// Fetch the full message and extract its HTML and/or plain-text body (decoded, NOT sanitized).
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
        // 🦀 New: the original recipients (for reply-all) — same case-insensitive closure.
        let to = header("To");
        let cc = header("Cc");
        // 🦀 Reuse the existing recursive MIME walk to pull the text/plain part.
        let mut html = None;
        let mut text = None;
        collect_body(&full.payload, &mut html, &mut text);
        // 🦀 New: the original's attachments (for forward) — reuses the M17 walk on the same payload.
        let mut attachments = Vec::new();
        collect_attachments(&full.payload, &mut attachments);
        Ok(ReplyContext {
            message_id,
            references,
            quoted_text: text.unwrap_or_default(),
            to,
            cc,
            attachments,
        })
    }

    /// Send a raw RFC822 message. `thread_id` threads a reply into its conversation.
    pub async fn send_message(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()> {
        // 🦀 Reuse the shared base64url helper (same encoding the read path decodes with).
        let raw = encode_raw(raw_rfc822);
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

    /// Move a message to Trash. Gmail `messages/{id}/trash` — the dedicated endpoint.
    /// NOTE: adding the TRASH label via `batchModify` returns 204 but Gmail silently
    /// ignores it (the message stays in INBOX), so trashing must go through here.
    pub async fn trash_message(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}/trash", self.base_url, id);
        self.post_no_body(&url).await
    }

    /// Restore a trashed message (removes the TRASH label). Gmail `messages/{id}/untrash`.
    pub async fn untrash_message(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}/untrash", self.base_url, id);
        self.post_no_body(&url).await
    }

    /// PERMANENTLY delete a message (bypasses Trash, irreversible). Gmail `DELETE messages/{id}`.
    pub async fn delete_message_forever(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}", self.base_url, id);
        self.delete_no_body(&url).await
    }

    /// Add and/or remove labels on a single message. Returns the message's label
    /// set *after* the change (Gmail echoes the updated resource), so the caller can
    /// persist the server-authoritative labels.
    pub async fn modify_message(
        &self,
        id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> Result<ModifiedMessage> {
        // 🦀 A short-lived request struct whose serde field names match Gmail's JSON
        //    (`addLabelIds`/`removeLabelIds`). The `<'a>` lifetime ties the borrowed
        //    slices to the struct so we serialize without cloning the label strings.
        #[derive(serde::Serialize)]
        struct ModifyRequest<'a> {
            #[serde(rename = "addLabelIds")]
            add_label_ids: &'a [&'a str],
            #[serde(rename = "removeLabelIds")]
            remove_label_ids: &'a [&'a str],
        }
        let url = format!("{}/gmail/v1/users/me/messages/{}/modify", self.base_url, id);
        let body = ModifyRequest {
            add_label_ids: add,
            remove_label_ids: remove,
        };
        self.post_json(&url, &body).await
    }

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
        // 🦀 collect_body fills both; we keep only the text/plain part (the editor is plain-text),
        //    so `html` is intentionally discarded.
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

    /// Add and/or remove labels on MANY messages in one call (`messages.batchModify`,
    /// up to 1000 ids; returns 204 with no body). Used by the M15 batch actions and undo.
    pub async fn batch_modify(&self, ids: &[String], add: &[&str], remove: &[&str]) -> Result<()> {
        // 🦀 A short-lived request struct; serde field names match Gmail's JSON. The `<'a>`
        //    ties the borrowed slices to the struct so we serialize without cloning.
        #[derive(serde::Serialize)]
        struct BatchModifyRequest<'a> {
            ids: &'a [String],
            #[serde(rename = "addLabelIds")]
            add_label_ids: &'a [&'a str],
            #[serde(rename = "removeLabelIds")]
            remove_label_ids: &'a [&'a str],
        }
        let url = format!("{}/gmail/v1/users/me/messages/batchModify", self.base_url);
        let body = BatchModifyRequest { ids, add_label_ids: add, remove_label_ids: remove };
        self.post_json_no_response(&url, &body).await
    }

    /// PERMANENTLY delete MANY messages in one call (`messages.batchDelete`, irreversible;
    /// returns 204 with no body). Unlike trashing, there is a real batch endpoint for this.
    /// Used by the Trash folder's batch "Delete forever".
    pub async fn batch_delete(&self, ids: &[String]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct BatchDeleteRequest<'a> {
            ids: &'a [String],
        }
        let url = format!("{}/gmail/v1/users/me/messages/batchDelete", self.base_url);
        self.post_json_no_response(&url, &BatchDeleteRequest { ids }).await
    }

    /// List the user's *user-created* labels (system labels like INBOX/UNREAD are dropped —
    /// they're handled by the rail's fixed folders + the scorer). Gmail `users.labels.list`.
    pub async fn list_labels(&self) -> Result<Vec<Label>> {
        let url = format!("{}/gmail/v1/users/me/labels", self.base_url);
        let resp: LabelsListResponse = self.get_json(&url).await?;
        // 🦀 `filter` keeps only user labels; `map` drops the wire-only `label_type` field.
        Ok(resp
            .labels
            .into_iter()
            .filter(|l| l.label_type == "user")
            .map(|l| Label { id: l.id, name: l.name, color: l.color })
            .collect())
    }

    /// Create a new user label. Gmail `users.labels.create`. Returns the created label.
    pub async fn create_label(&self, name: &str) -> Result<Label> {
        // 🦀 Short-lived request struct; the visibility fields make the label show in
        //    both the label list and the message-list label menu (Gmail's defaults).
        #[derive(serde::Serialize)]
        struct CreateLabelRequest<'a> {
            name: &'a str,
            #[serde(rename = "labelListVisibility")]
            label_list_visibility: &'a str,
            #[serde(rename = "messageListVisibility")]
            message_list_visibility: &'a str,
        }
        let url = format!("{}/gmail/v1/users/me/labels", self.base_url);
        let body = CreateLabelRequest {
            name,
            label_list_visibility: "labelShow",
            message_list_visibility: "show",
        };
        // 🦀 Reuse RawLabel for the response, then map to the public Label (drop label_type).
        let raw: RawLabel = self.post_json(&url, &body).await?;
        Ok(Label { id: raw.id, name: raw.name, color: raw.color })
    }
}

// 🦀 Gmail wants the whole RFC822 message base64url-encoded (web-safe, no padding) in
//    `raw`. Shared by every send/draft path (create/update/send_draft and send_message)
//    so the encoding lives in exactly one place.
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

// 🦀 drafts.send wants a Draft resource: the draft id PLUS the (edited) message.
#[derive(serde::Serialize)]
struct DraftSendBody<'a> {
    id: &'a str,
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

// 🦀 users.messages.attachments.get response: { size, data (base64url) }. We only need `data`.
#[derive(serde::Deserialize)]
struct AttachmentResponse {
    #[serde(default)]
    data: String,
}

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

// 🦀 users.labels.list shape. `RawLabel` carries `type` (a Rust keyword → `serde(rename)`
//    onto `label_type`) so we can filter to user labels.
#[derive(serde::Deserialize)]
struct LabelsListResponse {
    #[serde(default)]
    labels: Vec<RawLabel>,
}
#[derive(serde::Deserialize)]
struct RawLabel {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    label_type: String,
    #[serde(default)]
    color: Option<LabelColor>,
}
