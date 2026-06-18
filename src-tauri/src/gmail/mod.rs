// 🦀 `pub mod types;` makes the sibling file `types.rs` a *public submodule*
//    of this `gmail` module.  The full path is `ember_lib::gmail::types::Profile`.
pub mod types;

use std::collections::HashMap;
use types::{FullMessage, HistoryResponse, MessageList, MessagePart, MessagePreview, Profile, RawMessage};

use crate::error::Result;

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
}

// 🦀 base64url-decode a part's data into a String. `URL_SAFE_NO_PAD` is the web-safe
//    base64 variant Gmail uses ('+' → '-', '/' → '_', no '=' padding).
//    `Engine` is a trait from the `base64` crate — `.decode()` lives on the engine,
//    so we `use base64::Engine` to bring the trait method into scope.
//    `from_utf8_lossy` turns the byte vec into a String, replacing any invalid UTF-8
//    sequences with the U+FFFD replacement character so we never panic.
fn decode_b64url(data: &str) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data.trim().trim_end_matches('='))
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
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
            "{}/gmail/v1/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
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
        Ok(MessagePreview {
            id: raw.id.clone(),
            thread_id: raw.thread_id.clone(),
            from: header("From"),
            subject: header("Subject"),
            date: header("Date"),
            snippet: raw.snippet,
            // 🦀 internalDate is ms-since-epoch as a string; parse to i64, 0 if absent.
            internal_date: raw.internal_date.parse::<i64>().unwrap_or(0),
        })
    }

    /// List INBOX message ids matching `query` (e.g. "newer_than:30d"), following
    /// pagination up to `max_total` ids.
    pub async fn list_inbox_message_ids_paged(
        &self,
        query: &str,
        max_total: u32,
    ) -> Result<Vec<String>> {
        // 🦀 Percent-encode the query value so characters like ':' are URL-safe.
        let encoded_q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let mut ids = Vec::new();
        // 🦀 The pagination cursor: None on the first request, then Some(token) per page
        //    until Gmail stops returning one.
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/messages?labelIds=INBOX&maxResults=100&q={}",
                self.base_url, encoded_q
            );
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
        Ok(RawBody { html, text })
    }
}
