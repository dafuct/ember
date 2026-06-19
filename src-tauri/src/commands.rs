// 🦀 `#[tauri::command]` is a procedural attribute macro that wraps an async fn
//    into a Tauri IPC handler the JS frontend can call by name via `invoke(...)`.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::auth::tokens::load_token;
use crate::auth::{ensure_access_token, GoogleOAuth, PRIMARY_ACCOUNT};
use crate::db;
use crate::error::{AppError, Result};
use crate::gmail::types::MessagePreview;
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
//    `html` as sanitized HTML (in a sandboxed iframe) or as plain text.
#[derive(serde::Serialize)]
pub struct MessageBody {
    pub html: String,
    pub is_html: bool,
    pub blocked_images: bool,
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
        Ok(MessageBody { html: clean, is_html: true, blocked_images: blocked })
    } else {
        Ok(MessageBody {
            html: raw.text.unwrap_or_default(),
            is_html: false,
            blocked_images: false,
        })
    }
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
    }
}
