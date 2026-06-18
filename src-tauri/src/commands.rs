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
        })
        .collect())
}

// 🦀 Convert Gmail-shaped previews into DB rows (a few field names differ). Pulled
//    into a helper because both the incremental and full-resync paths use it.
fn to_rows(previews: Vec<MessagePreview>) -> Vec<db::StoredMessage> {
    previews
        .into_iter()
        .map(|p| db::StoredMessage {
            id: p.id,
            thread_id: p.thread_id,
            from_addr: p.from,
            subject: p.subject,
            snippet: p.snippet,
            date_header: p.date,
            internal_date: p.internal_date,
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
