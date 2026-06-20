// 🦀 `#[tauri::command]` is a procedural attribute macro that wraps an async fn
//    into a Tauri IPC handler the JS frontend can call by name via `invoke(...)`.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::auth::tokens::{delete_token, load_token};
use crate::auth::{ensure_access_token, GoogleOAuth, PRIMARY_ACCOUNT};
use crate::db;
use crate::error::{AppError, Result};
use crate::gmail::types::{MessagePreview, ReplyContext};
use crate::calendar::types::CalendarEvent;
use crate::calendar::{map_event, CalendarClient};
use crate::gmail::GmailClient;
use crate::html::sanitize_html;
use crate::scorer;

// 🦀 The SQLite connection is shared application state. `Arc` lets every command
//    invocation share ownership of it; `Mutex` ensures only one touches the
//    connection at a time (rusqlite's Connection is `Send` but not `Sync`).
pub type Db = Arc<Mutex<Connection>>;

// 🦀 What `sync_inbox` reports back. `#[derive(serde::Serialize)]` lets Tauri send it to
//    the frontend as JSON `{ "added": N, "removed": M }`.
#[derive(serde::Serialize)]
pub struct SyncSummary {
    pub added: usize,
    pub removed: usize,
}

const PREVIEW_CONCURRENCY: usize = 8;
const SEARCH_MAX: u32 = 50;
const CALENDAR_CONCURRENCY: usize = 6;
const SYNC_WINDOW_DAYS: i64 = 30;

/// Run the interactive Google sign-in. Returns the connected email address.
#[tauri::command]
pub async fn connect_gmail() -> Result<String> {
    let oauth = GoogleOAuth::from_env()?;
    let stored = oauth.connect().await?;
    Ok(stored.email)
}

/// The currently connected account email, if any.
#[tauri::command]
pub async fn get_connected_account() -> Result<Option<String>> {
    Ok(load_token(PRIMARY_ACCOUNT)?.map(|t| t.email))
}

/// Sync INBOX into the local DB. Uses Gmail history deltas when we have a stored
/// historyId (fast); falls back to a full ~30-day resync on first run or if the
/// historyId expired. Returns the number of messages added and removed this run.
#[tauri::command]
pub async fn sync_inbox(state: tauri::State<'_, Db>) -> Result<SyncSummary> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 Prune cutoff: 30 days ago, in Unix MILLISECONDS (internal_date's unit).
    let cutoff_ms = (now_secs() as i64 - SYNC_WINDOW_DAYS * 24 * 60 * 60) * 1000;

    // 🦀 Read the stored historyId in an await-free locked block, then drop the lock
    //    before any network I/O (a std MutexGuard cannot be held across `.await`).
    let last_history_id: Option<i64> = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_sync_state(&conn, PRIMARY_ACCOUNT)?.and_then(|s| s.last_history_id)
    };

    // 🦀 Fast path: with a baseline, ask Gmail only for what CHANGED since then.
    if let Some(hid) = last_history_id {
        let delta = client.list_history(&hid.to_string()).await?;
        if !delta.too_old {
            let previews = client
                .get_message_previews(&delta.added_ids, PREVIEW_CONCURRENCY)
                .await?;
            let rows = to_rows(previews);
            let count = rows.len();
            // 🦀 Advance the baseline: use the new historyId when Gmail returns one,
            //    keep the old one if the delta was empty (None), and fail loudly rather
            //    than silently stalling if it's somehow present but non-numeric.
            let new_hid = match delta.new_history_id {
                Some(s) => s.parse::<i64>().map_err(|_| {
                    AppError::Other("Gmail returned a non-numeric historyId".into())
                })?,
                None => hid,
            };
            let removed = delta.removed_ids.len();
            {
                let conn = state
                    .lock()
                    .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
                // 🦀 One atomic transaction for the whole delta (see db::apply_delta).
                db::apply_delta(&conn, &rows, &delta.removed_ids, cutoff_ms)?;
                db::set_sync_state(&conn, PRIMARY_ACCOUNT, Some(new_hid), now_secs() as i64)?;
            }
            // NOTE (known limitation): Gmail's labelId=INBOX history filter can omit a
            // *hard* delete of an INBOX message; such rows are removed by the 30-day prune.
            return Ok(SyncSummary { added: count, removed });
        }
        // 🦀 `too_old` → stored historyId expired (Gmail keeps ~a week). Fall through.
    }

    // Slow path: first sync ever, or the historyId aged out — pull the whole window.
    // 🦀 Read the baseline historyId BEFORE the heavy message fetch: if this network
    //    call fails we bail early (no wasted fetch), and capturing it first means any
    //    messages that arrive *during* the fetch are simply re-seen on the next sync
    //    (upsert is idempotent) rather than skipped. Parse with a real error instead of
    //    silently storing NULL — a NULL baseline would force a full resync forever.
    let baseline_hid: i64 = client
        .get_profile()
        .await?
        .history_id
        .parse()
        .map_err(|_| AppError::Other("Gmail returned a non-numeric historyId".into()))?;
    let ids = client
        .list_inbox_message_ids_paged(&format!("newer_than:{SYNC_WINDOW_DAYS}d"), 500)
        .await?;
    let previews = client
        .get_message_previews(&ids, PREVIEW_CONCURRENCY)
        .await?;
    let rows = to_rows(previews);
    let count = rows.len();
    {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        // 🦀 Full resync: upsert everything, no deletes, prune old — one transaction.
        db::apply_delta(&conn, &rows, &[], cutoff_ms)?;
        db::set_sync_state(&conn, PRIMARY_ACCOUNT, Some(baseline_hid), now_secs() as i64)?;
    }
    Ok(SyncSummary { added: count, removed: 0 })
}

/// The most recent inbox previews, read from the local DB (fast, works offline).
#[tauri::command]
pub async fn fetch_inbox_preview(
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, 50);
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let rows = db::recent_previews(&conn, max)?;
    Ok(rows
        .into_iter()
        .map(|m| MessagePreview {
            id: m.id,
            thread_id: m.thread_id,
            from: m.from_addr,
            subject: m.subject,
            date: m.date_header,
            snippet: m.snippet,
            internal_date: m.internal_date,
            // 🦀 Split the comma-joined labels back into a Vec; drop empties so an
            //    empty string yields [] rather than [""].
            label_ids: m
                .label_ids
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            to_addr: m.to_addr,
            has_list_unsubscribe: m.has_list_unsubscribe,
            has_list_id: m.has_list_id,
            category: m.category,
            draft_id: None,
        })
        .collect())
}

// 🦀 Convert Gmail-shaped previews into DB rows, classifying each message in the
//    process. The scorer borrows the preview's signals; once `category` is computed
//    the borrow ends and we move the owned fields into the StoredMessage.
fn to_rows(previews: Vec<MessagePreview>) -> Vec<db::StoredMessage> {
    previews
        .into_iter()
        .map(|p| {
            let category = scorer::classify(&scorer::MessageFeatures {
                label_ids: &p.label_ids,
                from_addr: &p.from,
                has_list_unsubscribe: p.has_list_unsubscribe,
                has_list_id: p.has_list_id,
            })
            .as_str()
            .to_string();
            db::StoredMessage {
                id: p.id,
                thread_id: p.thread_id,
                from_addr: p.from,
                subject: p.subject,
                snippet: p.snippet,
                date_header: p.date,
                internal_date: p.internal_date,
                // 🦀 Persist the raw signals so a future re-score needs no Gmail refetch.
                //    Stored comma-joined: Gmail label ids are uppercase-ASCII / "Label_<n>"
                //    tokens that never contain commas, so this CSV round-trips losslessly.
                label_ids: p.label_ids.join(","),
                to_addr: p.to_addr,
                has_list_unsubscribe: p.has_list_unsubscribe,
                has_list_id: p.has_list_id,
                category,
            }
        })
        .collect()
}

// 🦀 Current Unix time in seconds. (Local copy to avoid making auth::now_secs public.)
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

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

/// Fetch one message's body and sanitize it. With `load_images = false`, remote images
/// are stripped and `blocked_images` reports whether any were present. No DB needed —
/// bodies are fetched live from Gmail on demand.
// 🦀 `#[tauri::command]` with plain args (no DB state needed): `id: String` and
//    `load_images: bool` are passed directly from JS `invoke("fetch_message_body", { id, loadImages })`.
//    The return type `Result<MessageBody>` serializes to JSON via the `#[derive(serde::Serialize)]`
//    on `MessageBody` — Tauri handles the conversion automatically.
#[tauri::command]
pub async fn fetch_message_body(id: String, load_images: bool) -> Result<MessageBody> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let raw = client.get_message_body(&id).await?;
    // 🦀 Prefer the HTML body; fall back to plain text. `if let Some(..)` both checks the
    //    Option and binds the inner String in one move.
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
}

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

// 🦀 Shared core for the label-toggle actions (read/star). `present` decides whether
//    the label is added or removed. We call Gmail FIRST; only on success do we take
//    the DB lock and persist the label set Gmail returns, so a network failure leaves
//    the local cache untouched (the frontend then rolls back its optimistic update).
//    The std MutexGuard is created AFTER every `.await`, never held across one.
async fn set_label(
    id: &str,
    label: &str,
    present: bool,
    state: &tauri::State<'_, Db>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 Pass the one-element slice directly as an argument so its temporary lives
    //    for the call (a `let` binding of `&[label]` would be dropped too early).
    let modified = if present {
        client.modify_message(id, &[label], &[]).await?
    } else {
        client.modify_message(id, &[], &[label]).await?
    };
    let csv = modified.label_ids.join(",");
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::update_message_labels(&conn, id, &csv)?;
    Ok(())
}

/// Mark a message read (`read = true` → remove UNREAD) or unread (`read = false` → add UNREAD).
#[tauri::command]
pub async fn set_message_read(
    id: String,
    read: bool,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    // 🦀 read == true means the UNREAD label should be ABSENT, so `present = !read`.
    set_label(&id, "UNREAD", !read, &state).await
}

/// Star (`starred = true`) or unstar (`starred = false`) a message via the STARRED label.
#[tauri::command]
pub async fn set_message_starred(
    id: String,
    starred: bool,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    set_label(&id, "STARRED", starred, &state).await
}


/// Add/remove labels on many messages in one Gmail call, then reconcile the local cache.
/// Archive (remove INBOX) / trash (add TRASH) drop the rows from the inbox cache like the
/// M7 single actions; everything else (read/star) applies the delta in place. DB-aware,
/// but a no-op on the DB for ids that aren't cached (search/folder results).
#[tauri::command]
pub async fn batch_modify_messages(
    ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    // 🦀 Gmail rejects a batchModify with no ids or no label changes (400). Both are
    //    vacuous no-ops, so short-circuit before the network call.
    if ids.is_empty() || (add.is_empty() && remove.is_empty()) {
        return Ok(());
    }
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 `&[&str]` is what batch_modify wants; map the owned Strings to borrowed &str.
    let add_refs: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove_refs: Vec<&str> = remove.iter().map(String::as_str).collect();
    client.batch_modify(&ids, &add_refs, &remove_refs).await?;

    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 `iter().any(|l| l == "TRASH")` — does the slice contain this label? Archiving
    //    (remove INBOX) or trashing (add TRASH) means the row leaves the inbox cache.
    if add.iter().any(|l| l == "TRASH") || remove.iter().any(|l| l == "INBOX") {
        db::delete_messages(&conn, &ids)?;
    } else {
        db::apply_label_delta(&conn, &ids, &add, &remove)?;
    }
    Ok(())
}

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
    //    The `ember_boundary_` prefix + base64's alphabet (which has no `_`) guarantees the
    //    boundary string can never appear inside an encoded attachment body — so framing is safe.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let boundary = format!("ember_boundary_{nanos}");
    let raw = crate::mime::build_multipart_rfc822(&msg, &attachments, &boundary);
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

/// Search all mail with a Gmail `q=` query and return hydrated, recency-sorted previews. DB-free —
/// results are fetched live, not cached. Reuses the smart-inbox scorer to set each result's category
/// (for the category dot), exactly as the sync path does.
#[tauri::command]
pub async fn search_messages(query: String, max: u32) -> Result<Vec<MessagePreview>> {
    // 🦀 `clamp` keeps `max` within 1..=SEARCH_MAX regardless of what the frontend sends.
    let max = max.clamp(1, SEARCH_MAX);
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.search_message_ids(&query, max).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    // 🦀 `for p in &mut previews` iterates by mutable reference so we can write `p.category` in place.
    for p in &mut previews {
        p.category = scorer::classify(&scorer::MessageFeatures {
            label_ids: &p.label_ids,
            from_addr: &p.from,
            has_list_unsubscribe: p.has_list_unsubscribe,
            has_list_id: p.has_list_id,
        })
        .as_str()
        .to_string();
    }
    // 🦀 `get_message_previews` returns results out of order (buffer_unordered); sort newest-first.
    //    `Reverse` wraps the key so the natural ascending sort becomes descending.
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
    Ok(previews)
}

/// Fetch one mailbox's previews (live, DB-free). Maps the folder key to a Gmail label/query +
/// includeSpamTrash flag, lists ids, hydrates, recency-sorts. Folder results are NOT classified
/// (category dots are an inbox concept).
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

/// Fetch one draft's editable content (DB-free). Used to open a draft in the compose editor.
#[tauri::command]
pub async fn get_draft(draft_id: String) -> Result<crate::gmail::types::DraftContent> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.get_draft(&draft_id).await
}

/// Create (when `draft_id` is None) or update an existing draft. Returns the draft id.
/// DB-free. Reuses the M8 RFC822 builder. No recipient validation — drafts may be partial.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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

/// List the user's user-created labels (DB-free). Drives the rail labels section + picker + chips.
#[tauri::command]
pub async fn list_labels() -> Result<Vec<crate::gmail::types::Label>> {
    let stored = ensure_access_token().await?; // 🦀 refresh token if expired, same pattern as every DB-free command
    let client = GmailClient::new(stored.access_token); // 🦀 thin wrapper around an access token + reqwest client
    client.list_labels().await // 🦀 delegate straight to GmailClient; ? propagates any AppError
}

/// Create a new user label (DB-free). Returns the created label.
#[tauri::command]
pub async fn create_label(name: String) -> Result<crate::gmail::types::Label> {
    let stored = ensure_access_token().await?; // 🦀 same token-refresh dance
    let client = GmailClient::new(stored.access_token);
    client.create_label(&name).await // 🦀 &name borrows the owned String as &str — no copy needed
}

/// Fetch one label's messages (DB-free) — a user label is just a label id, so this mirrors
/// fetch_folder's generic arm over list_message_ids.
#[tauri::command]
pub async fn fetch_label(label_id: String, max: u32) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX); // 🦀 clamp: saturate to [1, SEARCH_MAX] regardless of frontend input
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 `Some(label_id.as_str())` → Option<&str> to match list_message_ids' `label` param.
    let ids = client.list_message_ids(Some(label_id.as_str()), "", max, false).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date)); // 🦀 newest-first, same as fetch_folder
    Ok(previews)
}

/// Restore a trashed message (untrash). DB-free — the Trash folder isn't cached.
#[tauri::command]
pub async fn restore_message(id: String) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.untrash_message(&id).await
}

/// Permanently delete a message (irreversible) and drop it from the local cache if present.
#[tauri::command]
pub async fn delete_message_forever(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.delete_message_forever(&id).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, std::slice::from_ref(&id))?;
    Ok(())
}

/// Fetch the user's events for the week window [time_min, time_max) (RFC3339 strings from the
/// frontend, in local time). Reads all *selected* calendars concurrently, merges, and sorts.
/// DB-free — calendar data is fetched live, not cached.
#[tauri::command]
pub async fn fetch_calendar_week(time_min: String, time_max: String) -> Result<Vec<CalendarEvent>> {
    let stored = ensure_access_token().await?;
    let client = CalendarClient::new(stored.access_token);

    // 🦀 Google omits `selected` on the primary calendar; treat "absent" as shown. We only
    //    drop calendars the user has explicitly hidden (`selected == Some(false)`).
    let shown: Vec<_> = client
        .list_calendars()
        .await?
        .into_iter()
        .filter(|c| c.selected != Some(false))
        .collect();

    // 🦀 Borrow the client + window once; `async move` then copies these references (which are
    //    Copy) into each per-calendar future, so all futures can run concurrently.
    let client_ref = &client;
    let tmin: &str = &time_min;
    let tmax: &str = &time_max;

    use futures::stream::StreamExt;
    let results = futures::stream::iter(shown)
        .map(|cal| async move {
            let color = cal.background_color.clone();
            let events = client_ref.list_events(&cal.id, tmin, tmax).await?;
            let mapped: Vec<CalendarEvent> = events
                .into_iter()
                .filter_map(|e| map_event(e, &cal.id, color.as_deref()))
                .collect();
            Ok::<Vec<CalendarEvent>, AppError>(mapped)
        })
        .buffer_unordered(CALENDAR_CONCURRENCY)
        .collect::<Vec<Result<Vec<CalendarEvent>>>>()
        .await;

    let mut all = Vec::new();
    for r in results {
        match r {
            Ok(evts) => all.extend(evts),
            // 🦀 An auth/scope error must surface so the UI can prompt reconnect; other
            //    per-calendar failures are skipped (one broken calendar ≠ whole-week failure).
            Err(AppError::Auth(m)) => return Err(AppError::Auth(m)),
            Err(_) => {}
        }
    }
    // Best-effort ordering; the frontend re-sorts/positions by parsed local time.
    all.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(all)
}

/// Read persisted app settings (signature, remote-images), with defaults for first run.
#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, Db>) -> Result<db::Settings> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::get_settings(&conn)
}

/// Persist app settings.
#[tauri::command]
pub async fn set_settings(settings: db::Settings, state: tauri::State<'_, Db>) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::save_settings(&conn, &settings)
}

/// Sign out: delete the Keychain token and clear the local mail cache (messages +
/// sync_state). Settings (user prefs) are kept. After this, the app returns to the
/// connect screen.
#[tauri::command]
pub async fn disconnect(state: tauri::State<'_, Db>) -> Result<()> {
    // 🦀 `delete_token` is synchronous (keyring), so there's no `.await` between it and
    //    taking the DB lock — no MutexGuard-across-await concern.
    delete_token(PRIMARY_ACCOUNT)?;
    // 🦀 Token-first ordering is deliberate: a sign-out must guarantee the credential is
    //    gone (the privacy-critical part). If clear_account_data below then fails, the
    //    cache rows are orphaned but harmless — get_connected_account still returns None
    //    (token deleted → connect screen) and the stale rows are overwritten on next sync.
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::clear_account_data(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gmail::types::MessagePreview;

    // 🦀 Build a MessagePreview with given labels/flags; other fields are filler.
    fn preview(labels: Vec<&str>, lu: bool) -> MessagePreview {
        MessagePreview {
            id: "x".into(),
            thread_id: "t".into(),
            from: "sender@example.com".into(),
            subject: "s".into(),
            date: "d".into(),
            snippet: "snip".into(),
            internal_date: 1,
            label_ids: labels.into_iter().map(String::from).collect(),
            to_addr: "you@example.com".into(),
            has_list_unsubscribe: lu,
            has_list_id: false,
            category: String::new(),
            draft_id: None,
        }
    }

    #[test]
    fn to_rows_classifies_and_joins_labels() {
        let rows = to_rows(vec![
            preview(vec!["INBOX", "CATEGORY_PROMOTIONS"], false),
            preview(vec!["INBOX", "CATEGORY_PERSONAL"], false),
            preview(vec![], true), // List-Unsubscribe → newsletters
        ]);
        assert_eq!(rows[0].category, "newsletters");
        assert_eq!(rows[0].label_ids, "INBOX,CATEGORY_PROMOTIONS");
        assert_eq!(rows[1].category, "people");
        assert_eq!(rows[2].category, "newsletters");
        // empty input labels join to "" (so the read-side split yields [] later)
        assert_eq!(rows[2].label_ids, "");
    }
}
