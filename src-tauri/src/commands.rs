// 🦀 `#[tauri::command]` is a procedural attribute macro that wraps an async fn
//    into a Tauri IPC handler the JS frontend can call by name via `invoke(...)`.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::auth::tokens::{delete_token, load_token, save_token, StoredToken};
use crate::auth::{ensure_token_for, GoogleOAuth, PRIMARY_ACCOUNT};
use crate::db;
use crate::error::{AppError, Result};
use crate::gmail::types::{MessagePreview, ReplyContext};
use crate::calendar::types::{CalendarEvent, CalendarSummary, EventWrite};
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

/// Per-account result of a sync. `new_previews` carries the incrementally-added messages so
/// the frontend can notify per account; it is EMPTY on a `baseline` run (full resync) to
/// avoid notifying on ~30 days of backfill. `account` is the email this summary is for.
#[derive(serde::Serialize)]
pub struct AccountSyncSummary {
    pub account: String,
    pub added: usize,
    pub removed: usize,
    pub baseline: bool,
    pub new_previews: Vec<MessagePreview>,
}

const PREVIEW_CONCURRENCY: usize = 8;
const SEARCH_MAX: u32 = 50;
const CALENDAR_CONCURRENCY: usize = 6;
// 🦀 Inbox sync/cache window. Drives both the `newer_than:{N}d` Gmail query and the prune
//    cutoff, so the local cache holds ~12 months of INBOX. The first sync after raising this
//    pulls a year of mail (one-time, bounded); later syncs stay fast history deltas.
const SYNC_WINDOW_DAYS: i64 = 365;
// 🦀 Safety ceiling for the inbox preview page size. The frontend grows its fetch limit as
//    you scroll; this bounds a single request so it can never ask for unbounded rows.
const PREVIEW_MAX: u32 = 2000;

/// Resolve the active account from the DB pointer, then load + refresh its token.
/// This is the multi-account replacement for the old `ensure_access_token()`.
// 🦀 The DB lock is taken inside a block so the MutexGuard is dropped before the
//    `.await` below — a std MutexGuard must never be held across an await point, and
//    every command that calls this then takes its own lock afterward.
async fn active_token(state: &tauri::State<'_, Db>) -> Result<StoredToken> {
    let account = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_active_account(&conn)?
    }
    .ok_or_else(|| AppError::Auth("no active account".into()))?;
    ensure_token_for(&account).await
}

/// Run once at startup: if a legacy "primary" Keychain token exists and no accounts are
/// registered yet, migrate it to the email-keyed scheme and stamp cached rows. Idempotent.
pub fn migrate_legacy_primary_account(conn: &rusqlite::Connection) -> Result<()> {
    // Already migrated (accounts registered) → nothing to do.
    if !db::get_accounts(conn)?.is_empty() {
        return Ok(());
    }
    // No legacy token (fresh install) → nothing to migrate.
    let Some(token) = load_token(PRIMARY_ACCOUNT)? else {
        return Ok(());
    };
    let email = token.email.clone();
    // 🦀 Order matters for crash-safety. `add_account` is the "already migrated" marker
    //    (the `get_accounts().is_empty()` guard above). Stamp the cache rows FIRST so that
    //    a crash before `add_account` just re-runs the whole (idempotent) migration next
    //    startup, rather than marking it done with rows still left at account=''.
    save_token(&email, &token)?;
    db::stamp_legacy_account(conn, &email)?;
    db::add_account(conn, &email)?;
    db::set_active_account(conn, &email)?;
    delete_token(PRIMARY_ACCOUNT)?;
    Ok(())
}

/// Run the interactive Google sign-in, register the account in the index, and make it
/// active. Returns the connected email address.
#[tauri::command]
pub async fn connect_gmail(state: tauri::State<'_, Db>) -> Result<String> {
    let oauth = GoogleOAuth::resolve()?;
    let stored = oauth.connect().await?; // already saves the token under stored.email
    // 🦀 Register + activate AFTER the await (the OAuth round-trip is the slow part); the
    //    DB lock is taken only for these two quick writes and dropped at the block's end.
    {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::add_account(&conn, &stored.email)?;      // idempotent — no dup if already present
        db::set_active_account(&conn, &stored.email)?; // newly connected account becomes active
    }
    Ok(stored.email)
}

/// The currently active account email, if any.
#[tauri::command]
pub async fn get_connected_account(state: tauri::State<'_, Db>) -> Result<Option<String>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::get_active_account(&conn)
}

/// Sync ONE account's INBOX into the scoped cache. Fast path (history delta) reports
/// baseline=false with the added previews; slow path (first sync / expired historyId)
/// reports baseline=true with empty new_previews (so callers don't notify on backfill).
// 🦀 Takes an explicit `email` and resolves its token via `ensure_token_for` (not the
//    active-account `active_token`), so the all-accounts loop can sync any account.
async fn sync_one_account(
    state: &tauri::State<'_, Db>,
    email: &str,
) -> Result<AccountSyncSummary> {
    let stored = ensure_token_for(email).await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 Prune cutoff: 30 days ago, in Unix MILLISECONDS (internal_date's unit).
    let cutoff_ms = (now_secs() as i64 - SYNC_WINDOW_DAYS * 24 * 60 * 60) * 1000;

    // 🦀 Read the stored historyId in an await-free locked block, then drop the lock
    //    before any network I/O (a std MutexGuard cannot be held across `.await`).
    let last_history_id: Option<i64> = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_sync_state(&conn, email)?.and_then(|s| s.last_history_id)
    };

    // 🦀 Fast path: with a baseline, ask Gmail only for what CHANGED since then.
    if let Some(hid) = last_history_id {
        let delta = client.list_history(&hid.to_string()).await?;
        if !delta.too_old {
            let previews = client
                .get_message_previews(&delta.added_ids, PREVIEW_CONCURRENCY)
                .await?;
            // 🦀 Clone the previews for the notification payload BEFORE `to_rows` consumes
            //    the Vec — only the FAST path notifies (baseline=false).
            let new_previews = previews.clone();
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
                db::apply_delta(&conn, email, &rows, &delta.removed_ids, cutoff_ms)?;
                db::set_sync_state(&conn, email, Some(new_hid), now_secs() as i64)?;
            }
            // NOTE (known limitation): Gmail's labelId=INBOX history filter can omit a
            // *hard* delete of an INBOX message; such rows are removed by the 30-day prune.
            return Ok(AccountSyncSummary {
                account: email.to_string(),
                added: count,
                removed,
                baseline: false,
                new_previews,
            });
        }
        // 🦀 `too_old` → stored historyId expired (Gmail keeps ~a week). Fall through to a
        //    full resync (treated as a baseline → no notifications).
    }

    // Slow path / baseline: first sync ever, or the historyId aged out — pull the whole
    // window. No notifications fire on this path (new_previews stays empty).
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
        db::apply_delta(&conn, email, &rows, &[], cutoff_ms)?;
        db::set_sync_state(&conn, email, Some(baseline_hid), now_secs() as i64)?;
    }
    Ok(AccountSyncSummary {
        account: email.to_string(),
        added: count,
        removed: 0,
        baseline: true,
        new_previews: Vec::new(),
    })
}

/// Sync the ACTIVE account's INBOX into the local DB. Thin wrapper over
/// `sync_one_account`. Uses Gmail history deltas when we have a stored historyId
/// (fast); falls back to a full ~30-day resync on first run or if the historyId
/// expired. Returns the number of messages added and removed this run.
#[tauri::command]
pub async fn sync_inbox(state: tauri::State<'_, Db>) -> Result<SyncSummary> {
    let email = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_active_account(&conn)?
            .ok_or_else(|| AppError::Auth("no active account".into()))?
    };
    let s = sync_one_account(&state, &email).await?;
    Ok(SyncSummary {
        added: s.added,
        removed: s.removed,
    })
}

/// Sync EVERY connected account into the scoped cache (the all-accounts background loop).
/// Returns one summary per account so the frontend can notify per account. A single account's
/// failure is logged and skipped — it must not block syncing the others.
#[tauri::command]
pub async fn sync_all_accounts(state: tauri::State<'_, Db>) -> Result<Vec<AccountSyncSummary>> {
    let accounts = {
        let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_accounts(&conn)?
    };
    let mut out = Vec::new();
    for email in accounts {
        match sync_one_account(&state, &email).await {
            Ok(summary) => out.push(summary),
            // 🦀 One bad account (revoked token, network blip) shouldn't abort the whole loop.
            Err(e) => eprintln!("[ember] sync failed for {email}: {e}"),
        }
    }
    Ok(out)
}

/// The most recent inbox previews, read from the local DB (fast, works offline).
#[tauri::command]
pub async fn fetch_inbox_preview(
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, PREVIEW_MAX);
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 The inbox preview is a pure DB read (no token needed), but it MUST be scoped to the
    //    active account so it only ever surfaces that account's mail. No active account → no
    //    mail to show, so return an empty list rather than the whole (unscoped) cache.
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(Vec::new());
    };
    let rows = db::recent_previews(&conn, &account, max)?;
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

// 🦀 Current Unix time in MILLISECONDS — meeting-note timestamps use the same unit as the
//    JS `Date.now()` the frontend formats with. `as i64` is safe for any real wall-clock time.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
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
pub async fn fetch_message_body(
    id: String,
    load_images: bool,
    state: tauri::State<'_, Db>,
) -> Result<MessageBody> {
    let stored = active_token(&state).await?;
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
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = active_token(&state).await?;
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
    let stored = active_token(state).await?;
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
    db::update_message_labels(&conn, id, &csv, &stored.email)?;
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


/// How a label delta maps onto Gmail calls. The TRASH label can't ride `batchModify`
/// — Gmail returns 204 but silently ignores it, so the message never leaves INBOX. It
/// has to go through the dedicated `messages/{id}/trash` (and `/untrash`) endpoints,
/// which take one id each. Everything else (INBOX/UNREAD/STARRED/user labels) rides a
/// single `batchModify`.
struct LabelPlan {
    trash: bool,
    untrash: bool,
    batch_add: Vec<String>,
    batch_remove: Vec<String>,
}

/// Pure split of a label delta into "trash/untrash via dedicated endpoint" vs.
/// "everything else via batchModify". TRASH is pulled out of the batchModify payload.
fn plan_label_changes(add: &[String], remove: &[String]) -> LabelPlan {
    LabelPlan {
        trash: add.iter().any(|l| l == "TRASH"),
        untrash: remove.iter().any(|l| l == "TRASH"),
        batch_add: add.iter().filter(|l| l.as_str() != "TRASH").cloned().collect(),
        batch_remove: remove.iter().filter(|l| l.as_str() != "TRASH").cloned().collect(),
    }
}

/// Add/remove labels on many messages, then reconcile the local cache. Archive (remove
/// INBOX) / trash (add TRASH) drop the rows from the inbox cache like the M7 single
/// actions; everything else (read/star) applies the delta in place. DB-aware, but a
/// no-op on the DB for ids that aren't cached (search/folder results).
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
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    let plan = plan_label_changes(&add, &remove);
    // 🦀 Non-TRASH label changes still go through one batchModify call (if any remain).
    if !plan.batch_add.is_empty() || !plan.batch_remove.is_empty() {
        let add_refs: Vec<&str> = plan.batch_add.iter().map(String::as_str).collect();
        let remove_refs: Vec<&str> = plan.batch_remove.iter().map(String::as_str).collect();
        client.batch_modify(&ids, &add_refs, &remove_refs).await?;
    }
    // 🦀 TRASH has no batch endpoint — loop the dedicated per-message call. In this app
    //    TRASH is never mixed with other labels, and a single delta can't both trash and
    //    untrash, so the `else if` is safe.
    if plan.trash {
        for id in &ids {
            client.trash_message(id).await?;
        }
    } else if plan.untrash {
        for id in &ids {
            client.untrash_message(id).await?;
        }
    }

    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 `iter().any(|l| l == "TRASH")` — does the slice contain this label? Archiving
    //    (remove INBOX) or trashing (add TRASH) means the row leaves the inbox cache.
    if add.iter().any(|l| l == "TRASH") || remove.iter().any(|l| l == "INBOX") {
        db::delete_messages(&conn, &stored.email, &ids)?;
    } else {
        db::apply_label_delta(&conn, &stored.email, &ids, &add, &remove)?;
    }
    Ok(())
}

/// A reference to an attachment on an existing message, for forwarding. The bytes are
/// NOT carried from JS — the backend re-fetches them via `get_attachment` at send time.
// 🦀 `Deserialize` so Tauri can build it from the JS object. Field names are snake_case,
//    so the JS side must pass `{ message_id, attachment_id, filename, mime_type }`.
#[derive(serde::Deserialize)]
pub struct ForwardedAttachmentRef {
    pub message_id: String,
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
}

/// Send a plain-text message, optionally with file attachments and/or forwarded attachments.
/// No attachments of either kind → the original single-part path; otherwise multipart/mixed.
// 🦀 `#[allow(clippy::too_many_arguments)]` — these flat args mirror the JS `invoke` payload;
//    a shared `OutgoingFields` struct is a noted follow-up (kept out of M18 to stay focused).
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
    forwarded_attachments: Vec<ForwardedAttachmentRef>,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = active_token(&state).await?;
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
    // 🦀 Neither kind of attachment → the unchanged single-part path.
    if attachment_paths.is_empty() && forwarded_attachments.is_empty() {
        let raw = crate::mime::build_rfc822(&msg);
        return client.send_message(&raw, thread_id.as_deref()).await;
    }
    let mut attachments = Vec::new();
    let mut total = 0usize;
    // 🦀 (a) Files the user picked from disk (M17 path).
    for path in &attachment_paths {
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::Other(format!("could not read attachment {path}: {e}")))?;
        // 🦀 `saturating_add` clamps at usize::MAX instead of wrapping, so the combined cap
        //    check below stays a true guarantee even on a (hypothetical) 32-bit target.
        total = total.saturating_add(bytes.len());
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let mime_type = crate::mime::mime_for_ext(&filename).to_string();
        attachments.push(crate::mime::OutgoingAttachment { filename, mime_type, bytes });
    }
    // 🦀 (b) Attachments forwarded from an existing message — re-fetched from Gmail by id.
    for fa in &forwarded_attachments {
        let bytes = client.get_attachment(&fa.message_id, &fa.attachment_id).await?;
        total = total.saturating_add(bytes.len());
        attachments.push(crate::mime::OutgoingAttachment {
            filename: fa.filename.clone(),
            mime_type: fa.mime_type.clone(),
            bytes,
        });
    }
    // 🦀 Cap the COMBINED total before base64 inflation pushes us past the send ceiling.
    if total > crate::mime::MAX_ATTACHMENT_BYTES {
        return Err(AppError::Other(format!(
            "attachments total {total} bytes exceed the {} MB limit",
            crate::mime::MAX_ATTACHMENT_BYTES / (1024 * 1024)
        )));
    }
    // 🦀 A unique-enough multipart boundary from the wall clock; mime.rs itself stays clock-free.
    //    The `ember_boundary_` prefix + standard base64's alphabet (which has no `_`) guarantees the
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
pub async fn get_reply_context(id: String, state: tauri::State<'_, Db>) -> Result<ReplyContext> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.get_reply_context(&id).await
}

/// Search all mail with a Gmail `q=` query and return hydrated, recency-sorted previews. DB-free —
/// results are fetched live, not cached. Reuses the smart-inbox scorer to set each result's category
/// (for the category dot), exactly as the sync path does.
#[tauri::command]
pub async fn search_messages(
    query: String,
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    // 🦀 `clamp` keeps `max` within 1..=SEARCH_MAX regardless of what the frontend sends.
    let max = max.clamp(1, SEARCH_MAX);
    let stored = active_token(&state).await?;
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
pub async fn fetch_folder(
    folder: String,
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    let stored = active_token(&state).await?;
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
pub async fn get_draft(
    draft_id: String,
    state: tauri::State<'_, Db>,
) -> Result<crate::gmail::types::DraftContent> {
    let stored = active_token(&state).await?;
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
    state: tauri::State<'_, Db>,
) -> Result<String> {
    let stored = active_token(&state).await?;
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
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    let msg = crate::mime::OutgoingMessage { from: stored.email, to, cc, subject, body, in_reply_to, references };
    let raw = crate::mime::build_rfc822(&msg);
    client.send_draft(&draft_id, &raw, thread_id.as_deref()).await
}

/// Permanently delete a draft. DB-free.
#[tauri::command]
pub async fn delete_draft(draft_id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.delete_draft(&draft_id).await
}

/// List the user's user-created labels (DB-free). Drives the rail labels section + picker + chips.
#[tauri::command]
pub async fn list_labels(
    state: tauri::State<'_, Db>,
) -> Result<Vec<crate::gmail::types::Label>> {
    let stored = active_token(&state).await?; // 🦀 refresh token if expired, same pattern as every DB-free command
    let client = GmailClient::new(stored.access_token); // 🦀 thin wrapper around an access token + reqwest client
    client.list_labels().await // 🦀 delegate straight to GmailClient; ? propagates any AppError
}

/// Create a new user label (DB-free). Returns the created label.
#[tauri::command]
pub async fn create_label(
    name: String,
    state: tauri::State<'_, Db>,
) -> Result<crate::gmail::types::Label> {
    let stored = active_token(&state).await?; // 🦀 same token-refresh dance
    let client = GmailClient::new(stored.access_token);
    client.create_label(&name).await // 🦀 &name borrows the owned String as &str — no copy needed
}

/// Fetch one label's messages (DB-free) — a user label is just a label id, so this mirrors
/// fetch_folder's generic arm over list_message_ids.
#[tauri::command]
pub async fn fetch_label(
    label_id: String,
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX); // 🦀 clamp: saturate to [1, SEARCH_MAX] regardless of frontend input
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 `Some(label_id.as_str())` → Option<&str> to match list_message_ids' `label` param.
    let ids = client.list_message_ids(Some(label_id.as_str()), "", max, false).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date)); // 🦀 newest-first, same as fetch_folder
    Ok(previews)
}

/// Restore a trashed message (untrash). DB-free — the Trash folder isn't cached.
#[tauri::command]
pub async fn restore_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.untrash_message(&id).await
}

/// Permanently delete a message (irreversible) and drop it from the local cache if present.
#[tauri::command]
pub async fn delete_message_forever(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.delete_message_forever(&id).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, &stored.email, std::slice::from_ref(&id))?;
    Ok(())
}

/// Snooze: archive on Gmail (remove INBOX), drop from the inbox cache, record a local wake-time.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn snooze_message(
    id: String, wake_at: i64, thread_id: String, from_addr: String,
    subject: String, snippet: String, internal_date: i64,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(std::slice::from_ref(&id), &[], &["INBOX"]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, &stored.email, std::slice::from_ref(&id))?;
    db::insert_snooze(&conn, &stored.email, &db::SnoozedRow {
        message_id: id, thread_id, wake_at, snoozed_at: now_millis(),
        from_addr, subject, snippet, internal_date,
    })?;
    Ok(())
}

/// Manual un-snooze: re-add INBOX + UNREAD on Gmail, drop the local row.
#[tauri::command]
pub async fn unsnooze_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(std::slice::from_ref(&id), &["INBOX", "UNREAD"], &[]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_snoozes(&conn, &stored.email, std::slice::from_ref(&id))?;
    Ok(())
}

/// Wake all snoozes whose wake_at has passed. Returns early (no network) when none are due.
#[tauri::command]
pub async fn wake_due_snoozes(state: tauri::State<'_, Db>) -> Result<Vec<String>> {
    // 🦀 Resolve the active account inside the first locked block so the due-query is scoped to
    //    it. No active account → nothing to wake (mirror fetch_inbox_preview's empty-result path).
    let ids = {
        let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        let Some(account) = db::get_active_account(&conn)? else {
            return Ok(Vec::new());
        };
        db::due_snoozes(&conn, &account, now_millis())?
    };
    if ids.is_empty() { return Ok(Vec::new()); }
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(&ids, &["INBOX", "UNREAD"], &[]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 `stored.email` is the same active account resolved above — active_token reads the same
    //    active_account pointer, so the delete is scoped to the account the ids came from.
    db::delete_snoozes(&conn, &stored.email, &ids)?;
    Ok(ids)
}

/// List pending snoozes for the Snoozed view (DB-only).
#[tauri::command]
pub fn list_snoozed(state: tauri::State<'_, Db>) -> Result<Vec<db::SnoozedRow>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 Scope to the active account; no active account → no snoozes to show (mirror
    //    fetch_inbox_preview's empty-result path for a read with no account).
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(Vec::new());
    };
    db::list_snoozes(&conn, &account)
}

/// Restore MANY trashed messages (untrash). Gmail has no batch untrash, so loop the
/// dedicated per-message endpoint. DB-free — the Trash folder isn't cached.
#[tauri::command]
pub async fn batch_restore_messages(ids: Vec<String>, state: tauri::State<'_, Db>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    for id in &ids {
        client.untrash_message(id).await?;
    }
    Ok(())
}

/// PERMANENTLY delete MANY messages (irreversible) in one Gmail batchDelete call, then drop
/// them from the local cache. Powers the Trash folder's batch "Delete forever".
#[tauri::command]
pub async fn batch_delete_messages(ids: Vec<String>, state: tauri::State<'_, Db>) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_delete(&ids).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, &stored.email, &ids)?;
    Ok(())
}

/// Fetch the user's events for the week window [time_min, time_max) (RFC3339 strings from the
/// frontend, in local time). Reads all *selected* calendars concurrently, merges, and sorts.
/// DB-free — calendar data is fetched live, not cached.
#[tauri::command]
pub async fn fetch_calendar_week(
    time_min: String,
    time_max: String,
    state: tauri::State<'_, Db>,
) -> Result<Vec<CalendarEvent>> {
    let stored = active_token(&state).await?;
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

/// Whether Google OAuth credentials are available, and from which source. Never returns
/// the secret — only `configured` + a source label for the UI.
#[derive(serde::Serialize)]
pub struct CredentialStatus {
    pub configured: bool,
    pub source: String,
}

#[tauri::command]
pub async fn google_credentials_status() -> Result<CredentialStatus> {
    let source = GoogleOAuth::credentials_source()?;
    Ok(CredentialStatus { configured: source != "none", source: source.to_string() })
}

#[tauri::command]
pub async fn set_google_credentials(client_id: String, client_secret: String) -> Result<()> {
    let id = client_id.trim();
    let secret = client_secret.trim();
    if id.is_empty() || secret.is_empty() {
        return Err(AppError::Config("Client ID and secret are both required".into()));
    }
    crate::auth::tokens::save_credentials(id, secret)
}

#[tauri::command]
pub async fn clear_google_credentials() -> Result<()> {
    crate::auth::tokens::delete_credentials()
}

/// Remove one account everywhere: Keychain token, scoped cache, and the index; then
/// re-point `active_account` ONLY if the removed account was the active one (removing a
/// non-active account preserves the current active account — see db::remove_account_and_repoint).
/// Returns the resulting active account (None if none remain). Takes the already-held conn.
fn remove_account_inner(conn: &rusqlite::Connection, email: &str) -> Result<Option<String>> {
    delete_token(email)?;
    db::remove_account_data(conn, email)?;
    db::remove_account_and_repoint(conn, email)
}

/// Remove ONE account (by email). Returns the new active account, or None if none remain.
#[tauri::command]
pub async fn remove_account(state: tauri::State<'_, Db>, email: String) -> Result<Option<String>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    remove_account_inner(&conn, &email)
}

/// Sign out of the ACTIVE account (back-compat for the current frontend; superseded by
/// the per-account `remove_account` once the multi-account UI lands). Removes the active
/// account's token + scoped cache and re-points to another account if one exists.
#[tauri::command]
pub async fn disconnect(state: tauri::State<'_, Db>) -> Result<()> {
    // 🦀 `delete_token` (inside the helper) is synchronous (keyring), so there's no `.await`
    //    while the DB lock is held — no MutexGuard-across-await concern.
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    if let Some(active) = db::get_active_account(&conn)? {
        remove_account_inner(&conn, &active)?;
    }
    Ok(())
}

/// One row for the account-switcher UI. `active` marks the current account; `unread` is the
/// cached unread count for the per-account badge.
#[derive(serde::Serialize)]
pub struct AccountInfo {
    pub email: String,
    pub active: bool,
    pub unread: i64,
}

/// All connected accounts (for the switcher popover + Settings list).
#[tauri::command]
pub async fn list_accounts(state: tauri::State<'_, Db>) -> Result<Vec<AccountInfo>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let active = db::get_active_account(&conn)?;
    let mut out = Vec::new();
    for email in db::get_accounts(&conn)? {
        let unread = db::unread_count(&conn, &email)?;
        // 🦀 Compute the flag before moving `email` into the struct — the field init order
        //    left-to-right guarantees `active:` evaluates first, but the borrow-checker can
        //    complain if both the flag and the struct init reference `email` simultaneously.
        //    Using `as_deref()` compares `Option<String>` with `Option<&str>` cleanly.
        let active_flag = active.as_deref() == Some(email.as_str());
        out.push(AccountInfo { email, active: active_flag, unread });
    }
    Ok(out)
}

/// Switch the active account. Validates the email is a connected account.
#[tauri::command]
pub async fn set_active_account(state: tauri::State<'_, Db>, email: String) -> Result<()> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    if !db::get_accounts(&conn)?.iter().any(|a| a == &email) {
        return Err(AppError::Other(format!("unknown account {email}")));
    }
    db::set_active_account(&conn, &email)
}

/// List the user's calendars (for the create-event calendar picker). DB-free.
#[tauri::command]
pub async fn list_calendars(state: tauri::State<'_, Db>) -> Result<Vec<CalendarSummary>> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    let entries = client.list_calendars().await?;
    // 🦀 `.into_iter().map(...).collect()` is the idiomatic way to transform an owned Vec
    //    into another Vec of a different type — no explicit loop needed.
    // 🦀 `matches!(expr, pat1 | pat2)` is a macro that evaluates to `true` when the value
    //    matches any of the listed patterns; it's terser than an explicit `match` or `||` chain.
    Ok(entries
        .into_iter()
        .map(|c| CalendarSummary {
            id: c.id,
            summary: c.summary.unwrap_or_else(|| "(unnamed)".to_string()),
            primary: c.primary.unwrap_or(false),
            // writable = the user has owner or writer access to this calendar
            writable: matches!(c.access_role.as_deref(), Some("owner") | Some("writer")),
        })
        .collect())
}

/// Create a calendar event (optionally a Meet meeting). DB-free.
#[tauri::command]
pub async fn create_calendar_event(
    calendar_id: String,
    event: EventWrite,
    add_meet: bool,
    state: tauri::State<'_, Db>,
) -> Result<CalendarEvent> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    client.create_event(&calendar_id, &event, add_meet).await
}

/// Edit a calendar event. DB-free.
#[tauri::command]
pub async fn update_calendar_event(
    calendar_id: String,
    event_id: String,
    event: EventWrite,
    state: tauri::State<'_, Db>,
) -> Result<CalendarEvent> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    client.update_event(&calendar_id, &event_id, &event).await
}

/// Delete a calendar event. DB-free.
#[tauri::command]
pub async fn delete_calendar_event(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    client.delete_event(&calendar_id, &event_id).await
}

/// Read the meeting note for one event, if any (DB-only; no Google call).
#[tauri::command]
pub async fn get_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<Option<db::MeetingNote>> {
    // 🦀 Pure local read — no `.await` here, so we lock the Mutex directly (same as get_settings).
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 Scope to the active account; no active account → no note (mirror the read pattern in
    //    fetch_inbox_preview / list_snoozed).
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(None);
    };
    db::get_meeting_note(&conn, &calendar_id, &event_id, &account)
}

/// Create or update the meeting note for one event (upsert). Returns the stored note.
#[tauri::command]
pub async fn save_meeting_note(
    note: db::MeetingNoteWrite,
    state: tauri::State<'_, Db>,
) -> Result<db::MeetingNote> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 A write needs an owning account; without one there's nothing to scope the note to.
    let account = db::get_active_account(&conn)?
        .ok_or_else(|| AppError::Auth("no active account".into()))?;
    // 🦀 The backend stamps the timestamp; the frontend never sends one.
    db::upsert_meeting_note(&conn, &account, &note, now_millis())
}

/// Delete the meeting note for one event (silent no-op if there isn't one).
#[tauri::command]
pub async fn delete_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let account = db::get_active_account(&conn)?
        .ok_or_else(|| AppError::Auth("no active account".into()))?;
    db::delete_meeting_note(&conn, &calendar_id, &event_id, &account)
}

/// List all meeting notes, most-recently-edited first (drives the Notes panel).
#[tauri::command]
pub async fn list_meeting_notes(state: tauri::State<'_, Db>) -> Result<Vec<db::MeetingNote>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 Scope to the active account; no active account → no notes (read-path empty result).
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(Vec::new());
    };
    db::list_meeting_notes(&conn, &account)
}

/// Summarize a meeting note with local Ollama (M21/M22). Reads the SAVED note, combines the
/// freeform body + transcript, calls Ollama OUTSIDE the DB lock, then persists the summary.
/// Requires the note to be saved first.
#[tauri::command]
pub async fn summarize_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<db::MeetingNote> {
    // 🦀 Build the summary input from the SAVED note (notes + transcript combined) in a short
    //    locked block, then DROP the guard before the network await. The active account is
    //    resolved HERE (under the lock) and carried out so the read AND the later persist use the
    //    SAME account — no risk of a switch between the read and the write changing scope.
    let (input, account) = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        let account = db::get_active_account(&conn)?
            .ok_or_else(|| AppError::Auth("no active account".into()))?;
        let note = db::get_meeting_note(&conn, &calendar_id, &event_id, &account)?
            .ok_or_else(|| AppError::Other("Save the note before summarizing.".into()))?;
        (crate::transcript::build_summary_input(&note.body, &note.transcript), account)
    };
    if input.trim().is_empty() {
        return Err(AppError::Other(
            "Nothing to summarize — add notes or a transcript first.".into(),
        ));
    }
    // 🦀 The slow part: a local HTTP call to Ollama. No DB lock is held across this await.
    let summary = crate::ollama::OllamaClient::new().summarize(&input).await?;
    // 🦀 Re-lock to persist. This UPDATE does NOT bump the body's updated_at (staleness logic).
    //    Reuse the account resolved above so the write is scoped to the same note we read.
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::set_meeting_note_summary(&conn, &calendar_id, &event_id, &account, &summary, now_millis())
}

/// Read a user-picked transcript file (.txt or .vtt) into plain text (M22). DB-free; `.vtt` is
/// stripped to spoken text. The byte read happens here in Rust (std::fs) so no fs capability is
/// needed — the frontend supplies the path from the native open dialog.
#[tauri::command]
pub async fn read_transcript_file(path: String) -> Result<String> {
    // 🦀 Guard against an accidental huge pick before slurping the whole file into memory.
    //    25 MB is far beyond any real meeting transcript (plain text).
    const MAX_TRANSCRIPT_BYTES: u64 = 25 * 1024 * 1024;
    let len = std::fs::metadata(&path)
        .map_err(|e| AppError::Other(format!("could not read transcript file: {e}")))?
        .len();
    if len > MAX_TRANSCRIPT_BYTES {
        return Err(AppError::Other(format!(
            "transcript file is too large ({} MB max).",
            MAX_TRANSCRIPT_BYTES / (1024 * 1024)
        )));
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Other(format!("could not read transcript file: {e}")))?;
    // 🦀 `.ends_with` on the lowercased path picks the parser; .txt (and anything else) passes through.
    let text = if path.to_lowercase().ends_with(".vtt") {
        crate::transcript::vtt_to_text(&raw)
    } else {
        raw
    };
    Ok(text.trim().to_string())
}

/// Transcribe a user-picked audio/video recording in-process (decode via symphonia → whisper-rs).
/// DB-free; the path comes from the frontend dialog → no `fs` capability. The transcriber must be
/// loaded first (the frontend calls prepare_transcription).
#[tauri::command]
pub async fn transcribe_recording(
    state: tauri::State<'_, crate::transcribe::TranscriberState>,
    path: String,
) -> Result<String> {
    // 🦀 Cap the pick before decoding. Recordings dwarf a text transcript (audio = tens of MB,
    //    video more), so 500 MB rather than the 25 MB text cap.
    const MAX_RECORDING_BYTES: u64 = 500 * 1024 * 1024;
    let len = std::fs::metadata(&path)
        .map_err(|e| AppError::Other(format!("could not read recording file: {e}")))?
        .len();
    if len > MAX_RECORDING_BYTES {
        return Err(AppError::Other(format!(
            "recording file is too large ({} MB max).",
            MAX_RECORDING_BYTES / (1024 * 1024)
        )));
    }
    // 🦀 Decode + transcribe entirely off the async runtime (both are CPU-heavy and blocking);
    //    the std Mutex is locked only on the blocking thread, never across an await.
    let tr: crate::transcribe::TranscriberState = (*state).clone();
    let text = tokio::task::spawn_blocking(move || -> Result<String> {
        let samples = crate::decode::decode_to_16k_mono(&path)?;
        let guard = tr
            .lock()
            .map_err(|_| AppError::Other("transcriber state poisoned".into()))?;
        let t = guard.as_ref().ok_or_else(|| {
            AppError::Other("transcription not ready — open a meeting note so it can set up first".into())
        })?;
        t.transcribe_samples(&samples)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;
    Ok(text)
}

/// Ensure the Whisper model is downloaded and the in-process context is loaded. Streams progress
/// (download %, loading) over `on_progress`. Idempotent — a no-op once the context is loaded.
#[tauri::command]
pub async fn prepare_transcription(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::transcribe::TranscriberState>,
    on_progress: tauri::ipc::Channel<crate::model::PrepProgress>,
) -> Result<()> {
    {
        let guard = state
            .lock()
            .map_err(|_| AppError::Other("transcriber state poisoned".into()))?;
        if guard.is_some() {
            let _ = on_progress.send(crate::model::PrepProgress::Ready);
            return Ok(());
        }
    }
    let model = crate::model::ensure_model(&app, &on_progress).await?;
    let _ = on_progress.send(crate::model::PrepProgress::Loading);
    // 🦀 Loading the model is CPU-heavy; do it off the async runtime so the UI stays responsive.
    let model_str = model.to_string_lossy().to_string();
    let loaded = tokio::task::spawn_blocking(move || crate::transcribe::Transcriber::load(&model_str))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    {
        let mut guard = state
            .lock()
            .map_err(|_| AppError::Other("transcriber state poisoned".into()))?;
        // 🦀 If another prepare loaded it while we were working, keep theirs (drop ours).
        if guard.is_none() {
            *guard = Some(loaded);
        }
    }
    let _ = on_progress.send(crate::model::PrepProgress::Ready);
    Ok(())
}

/// Whether transcription is set up + which capture device hint to show. No secret/heavy work.
#[derive(serde::Serialize)]
pub struct TranscriptionStatus {
    pub model_present: bool,
    pub ready: bool,
    pub blackhole_present: bool,
}

#[tauri::command]
pub async fn transcription_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::transcribe::TranscriberState>,
) -> Result<TranscriptionStatus> {
    let model_present = crate::model::model_present(&app);
    let ready = {
        let guard = state
            .lock()
            .map_err(|_| AppError::Other("transcriber state poisoned".into()))?;
        guard.is_some()
    };
    // 🦀 BlackHole = a virtual audio device for capturing the meeting's output; guide the user
    //    to install it when absent. Reuses the existing device enumeration.
    let blackhole_present = crate::capture::list_input_devices()
        .await
        .map(|ds| ds.iter().any(|d| d.name.to_lowercase().contains("blackhole")))
        .unwrap_or(false);
    Ok(TranscriptionStatus { model_present, ready, blackhole_present })
}

/// Assisted setup: fetch the official BlackHole 2ch installer and open it in macOS Installer so
/// the user can capture meeting audio. DB-free; the user still authenticates the driver install.
#[tauri::command]
pub async fn install_blackhole() -> Result<()> {
    crate::blackhole::install_2ch().await
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
    fn plan_routes_trash_to_dedicated_endpoint_not_batch_modify() {
        // The bug: trash was sent as add=["TRASH"] through batchModify, which Gmail
        // 204s but ignores. The plan must route TRASH out of the batchModify payload
        // and onto the dedicated trash endpoint instead.
        let plan = plan_label_changes(&["TRASH".into()], &[]);
        assert!(plan.trash);
        assert!(!plan.untrash);
        assert!(plan.batch_add.is_empty(), "TRASH must NOT ride batchModify");
        assert!(plan.batch_remove.is_empty());
    }

    #[test]
    fn plan_routes_untrash_to_dedicated_endpoint() {
        // Undo of a trash sends remove=["TRASH"]; same restriction → /untrash.
        let plan = plan_label_changes(&[], &["TRASH".into()]);
        assert!(!plan.trash);
        assert!(plan.untrash);
        assert!(plan.batch_add.is_empty());
        assert!(plan.batch_remove.is_empty(), "TRASH must NOT ride batchModify");
    }

    #[test]
    fn plan_keeps_non_trash_labels_on_batch_modify() {
        // Archive (remove INBOX) and read/star toggles still ride batchModify.
        let plan = plan_label_changes(&["STARRED".into()], &["INBOX".into(), "UNREAD".into()]);
        assert!(!plan.trash);
        assert!(!plan.untrash);
        assert_eq!(plan.batch_add, vec!["STARRED".to_string()]);
        assert_eq!(plan.batch_remove, vec!["INBOX".to_string(), "UNREAD".to_string()]);
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
