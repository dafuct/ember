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
//    connection at a time (rusqlite's Connection is `Send` but not `Sync`). This
//    alias keeps the long type readable across command signatures and `lib.rs`.
pub type Db = Arc<Mutex<Connection>>;

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

/// Sync the last ~30 days of INBOX into the local DB. Returns how many messages
/// were fetched and stored.
#[tauri::command]
pub async fn sync_inbox(state: tauri::State<'_, Db>) -> Result<usize> {
    // 🦀 All async network I/O happens FIRST, before we acquire the DB lock.
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client
        .list_inbox_message_ids_paged("newer_than:30d", 500)
        .await?;
    // 🦀 Fetch up to 8 previews concurrently (see gmail::get_message_previews).
    let previews = client.get_message_previews(&ids, 8).await?;

    // 🦀 Convert Gmail-shaped previews into DB rows (a few field names differ).
    let rows: Vec<db::StoredMessage> = previews
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
        .collect();
    let count = rows.len();

    // 🦀 Only now — inside a block with NO `.await` — do we lock the Mutex and run
    //    the synchronous DB writes. A std::sync::MutexGuard is not `Send`, so
    //    holding it across an `.await` would make this async fn fail to compile;
    //    keeping the lock in an await-free block sidesteps that entirely.
    {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::upsert_messages(&conn, &rows)?;
        db::set_sync_state(&conn, PRIMARY_ACCOUNT, None, now_secs() as i64)?;
    }
    Ok(count)
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
    // 🦀 Map DB rows back to the frontend's MessagePreview shape.
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

// 🦀 Current Unix time in seconds. (Local copy to avoid making auth::now_secs public.)
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
