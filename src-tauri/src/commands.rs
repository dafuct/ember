
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::auth::tokens::{delete_token, load_token, save_token, StoredToken};
use crate::auth::{ensure_token_for, GoogleOAuth, PRIMARY_ACCOUNT};
use crate::db;
use crate::error::{AppError, Result};
use crate::gmail::types::{MessagePreview, ReplyContext};
use crate::calendar::types::{BusySpan, CalendarEvent, CalendarSummary, EventWrite};
use crate::calendar::{map_event, CalendarClient};
use crate::gmail::GmailClient;
use crate::html::sanitize_html;
use crate::people::{PeopleClient, PersonHit};
use crate::scheduling::{self, BusyInterval, Slot, WorkingHours};
use crate::scorer;
use chrono::{DateTime, Local, Offset, Utc};

pub type Db = Arc<Mutex<Connection>>;

#[derive(serde::Serialize)]
pub struct SyncSummary {
    pub added: usize,
    pub removed: usize,
}

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
const SYNC_WINDOW_DAYS: i64 = 365;
const PREVIEW_MAX: u32 = 2000;

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

pub fn migrate_legacy_primary_account(conn: &rusqlite::Connection) -> Result<()> {
    if !db::get_accounts(conn)?.is_empty() {
        return Ok(());
    }
    let Some(token) = load_token(PRIMARY_ACCOUNT)? else {
        return Ok(());
    };
    let email = token.email.clone();
    save_token(&email, &token)?;
    db::stamp_legacy_account(conn, &email)?;
    db::add_account(conn, &email)?;
    db::set_active_account(conn, &email)?;
    delete_token(PRIMARY_ACCOUNT)?;
    Ok(())
}

#[tauri::command]
pub async fn connect_gmail(state: tauri::State<'_, Db>) -> Result<String> {
    let oauth = GoogleOAuth::resolve()?;
    let stored = oauth.connect().await?;
    {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::add_account(&conn, &stored.email)?;
        db::set_active_account(&conn, &stored.email)?;
    }
    Ok(stored.email)
}

#[tauri::command]
pub async fn get_connected_account(state: tauri::State<'_, Db>) -> Result<Option<String>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::get_active_account(&conn)
}

async fn sync_one_account(
    state: &tauri::State<'_, Db>,
    email: &str,
) -> Result<AccountSyncSummary> {
    let stored = ensure_token_for(email).await?;
    let client = GmailClient::new(stored.access_token);
    let cutoff_ms = (now_secs() as i64 - SYNC_WINDOW_DAYS * 24 * 60 * 60) * 1000;

    let last_history_id: Option<i64> = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_sync_state(&conn, email)?.and_then(|s| s.last_history_id)
    };

    if let Some(hid) = last_history_id {
        let delta = client.list_history(&hid.to_string()).await?;
        if !delta.too_old {
            let previews = client
                .get_message_previews(&delta.added_ids, PREVIEW_CONCURRENCY)
                .await?;
            let new_previews = previews.clone();
            let rows = to_rows(previews);
            let count = rows.len();
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
                db::apply_delta(&conn, email, &rows, &delta.removed_ids, cutoff_ms)?;
                db::set_sync_state(&conn, email, Some(new_hid), now_secs() as i64)?;
            }
            return Ok(AccountSyncSummary {
                account: email.to_string(),
                added: count,
                removed,
                baseline: false,
                new_previews,
            });
        }
    }

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
            Err(e) => eprintln!("[ember] sync failed for {email}: {e}"),
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn fetch_inbox_preview(
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, PREVIEW_MAX);
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
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
                label_ids: p.label_ids.join(","),
                to_addr: p.to_addr,
                has_list_unsubscribe: p.has_list_unsubscribe,
                has_list_id: p.has_list_id,
                category,
            }
        })
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(serde::Serialize)]
pub struct MessageBody {
    pub html: String,
    pub is_html: bool,
    pub blocked_images: bool,
    pub attachments: Vec<crate::gmail::types::AttachmentMeta>,
}

#[tauri::command]
pub async fn fetch_message_body(
    id: String,
    load_images: bool,
    state: tauri::State<'_, Db>,
) -> Result<MessageBody> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    let raw = client.get_message_body(&id).await?;
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
    std::fs::write(&dest_path, &bytes)
        .map_err(|e| AppError::Other(format!("could not save attachment: {e}")))?;
    Ok(())
}

async fn set_label(
    id: &str,
    label: &str,
    present: bool,
    state: &tauri::State<'_, Db>,
) -> Result<()> {
    let stored = active_token(state).await?;
    let client = GmailClient::new(stored.access_token);
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

#[tauri::command]
pub async fn set_message_read(
    id: String,
    read: bool,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    set_label(&id, "UNREAD", !read, &state).await
}

#[tauri::command]
pub async fn set_message_starred(
    id: String,
    starred: bool,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    set_label(&id, "STARRED", starred, &state).await
}


struct LabelPlan {
    trash: bool,
    untrash: bool,
    batch_add: Vec<String>,
    batch_remove: Vec<String>,
}

fn plan_label_changes(add: &[String], remove: &[String]) -> LabelPlan {
    LabelPlan {
        trash: add.iter().any(|l| l == "TRASH"),
        untrash: remove.iter().any(|l| l == "TRASH"),
        batch_add: add.iter().filter(|l| l.as_str() != "TRASH").cloned().collect(),
        batch_remove: remove.iter().filter(|l| l.as_str() != "TRASH").cloned().collect(),
    }
}

#[tauri::command]
pub async fn batch_modify_messages(
    ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    if ids.is_empty() || (add.is_empty() && remove.is_empty()) {
        return Ok(());
    }
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    let plan = plan_label_changes(&add, &remove);
    if !plan.batch_add.is_empty() || !plan.batch_remove.is_empty() {
        let add_refs: Vec<&str> = plan.batch_add.iter().map(String::as_str).collect();
        let remove_refs: Vec<&str> = plan.batch_remove.iter().map(String::as_str).collect();
        client.batch_modify(&ids, &add_refs, &remove_refs).await?;
    }
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
    if add.iter().any(|l| l == "TRASH") || remove.iter().any(|l| l == "INBOX") {
        db::delete_messages(&conn, &stored.email, &ids)?;
    } else {
        db::apply_label_delta(&conn, &stored.email, &ids, &add, &remove)?;
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct ForwardedAttachmentRef {
    pub message_id: String,
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
}

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
    if attachment_paths.is_empty() && forwarded_attachments.is_empty() {
        let raw = crate::mime::build_rfc822(&msg);
        return client.send_message(&raw, thread_id.as_deref()).await;
    }
    let mut attachments = Vec::new();
    let mut total = 0usize;
    for path in &attachment_paths {
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::Other(format!("could not read attachment {path}: {e}")))?;
        total = total.saturating_add(bytes.len());
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let mime_type = crate::mime::mime_for_ext(&filename).to_string();
        attachments.push(crate::mime::OutgoingAttachment { filename, mime_type, bytes });
    }
    for fa in &forwarded_attachments {
        let bytes = client.get_attachment(&fa.message_id, &fa.attachment_id).await?;
        total = total.saturating_add(bytes.len());
        attachments.push(crate::mime::OutgoingAttachment {
            filename: fa.filename.clone(),
            mime_type: fa.mime_type.clone(),
            bytes,
        });
    }
    if total > crate::mime::MAX_ATTACHMENT_BYTES {
        return Err(AppError::Other(format!(
            "attachments total {total} bytes exceed the {} MB limit",
            crate::mime::MAX_ATTACHMENT_BYTES / (1024 * 1024)
        )));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let boundary = format!("ember_boundary_{nanos}");
    let raw = crate::mime::build_multipart_rfc822(&msg, &attachments, &boundary);
    client.send_message(&raw, thread_id.as_deref()).await
}

#[tauri::command]
pub async fn get_reply_context(id: String, state: tauri::State<'_, Db>) -> Result<ReplyContext> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.get_reply_context(&id).await
}

#[tauri::command]
pub async fn search_messages(
    query: String,
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.search_message_ids(&query, max).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
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
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
    Ok(previews)
}

#[tauri::command]
pub async fn fetch_folder(
    folder: String,
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);

    if folder == "drafts" {
        let refs = client.list_drafts(max).await?;
        let ids: Vec<String> = refs.iter().map(|d| d.message_id.clone()).collect();
        let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
        let by_msg: std::collections::HashMap<&str, &str> =
            refs.iter().map(|d| (d.message_id.as_str(), d.id.as_str())).collect();
        for p in &mut previews {
            p.draft_id = by_msg.get(p.id.as_str()).map(|s| s.to_string());
        }
        previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
        return Ok(previews);
    }

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

#[tauri::command]
pub async fn get_draft(
    draft_id: String,
    state: tauri::State<'_, Db>,
) -> Result<crate::gmail::types::DraftContent> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.get_draft(&draft_id).await
}

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
    match draft_id {
        Some(id) => client.update_draft(&id, &raw, thread_id.as_deref()).await,
        None => client.create_draft(&raw, thread_id.as_deref()).await,
    }
}

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

#[tauri::command]
pub async fn delete_draft(draft_id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.delete_draft(&draft_id).await
}

#[tauri::command]
pub async fn list_labels(
    state: tauri::State<'_, Db>,
) -> Result<Vec<crate::gmail::types::Label>> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.list_labels().await
}

#[tauri::command]
pub async fn create_label(
    name: String,
    state: tauri::State<'_, Db>,
) -> Result<crate::gmail::types::Label> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.create_label(&name).await
}

#[tauri::command]
pub async fn fetch_label(
    label_id: String,
    max: u32,
    state: tauri::State<'_, Db>,
) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.list_message_ids(Some(label_id.as_str()), "", max, false).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
    Ok(previews)
}

#[tauri::command]
pub async fn restore_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.untrash_message(&id).await
}

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

#[tauri::command]
pub async fn unsnooze_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = active_token(&state).await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(std::slice::from_ref(&id), &["INBOX", "UNREAD"], &[]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_snoozes(&conn, &stored.email, std::slice::from_ref(&id))?;
    Ok(())
}

#[tauri::command]
pub async fn wake_due_snoozes(state: tauri::State<'_, Db>) -> Result<Vec<String>> {
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
    db::delete_snoozes(&conn, &stored.email, &ids)?;
    Ok(ids)
}

#[tauri::command]
pub fn list_snoozed(state: tauri::State<'_, Db>) -> Result<Vec<db::SnoozedRow>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(Vec::new());
    };
    db::list_snoozes(&conn, &account)
}

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

#[tauri::command]
pub async fn fetch_calendar_week(
    time_min: String,
    time_max: String,
    state: tauri::State<'_, Db>,
) -> Result<Vec<CalendarEvent>> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);

    let shown: Vec<_> = client
        .list_calendars()
        .await?
        .into_iter()
        .filter(|c| c.selected != Some(false))
        .collect();

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
            Err(AppError::Auth(m)) => return Err(AppError::Auth(m)),
            Err(_) => {}
        }
    }
    all.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(all)
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, Db>) -> Result<db::Settings> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::get_settings(&conn)
}

#[tauri::command]
pub async fn set_settings(settings: db::Settings, state: tauri::State<'_, Db>) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::save_settings(&conn, &settings)
}

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

fn remove_account_inner(conn: &rusqlite::Connection, email: &str) -> Result<Option<String>> {
    delete_token(email)?;
    db::remove_account_data(conn, email)?;
    db::remove_account_and_repoint(conn, email)
}

#[tauri::command]
pub async fn remove_account(state: tauri::State<'_, Db>, email: String) -> Result<Option<String>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    remove_account_inner(&conn, &email)
}

#[tauri::command]
pub async fn disconnect(state: tauri::State<'_, Db>) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    if let Some(active) = db::get_active_account(&conn)? {
        remove_account_inner(&conn, &active)?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct AccountInfo {
    pub email: String,
    pub active: bool,
    pub unread: i64,
}

#[tauri::command]
pub async fn list_accounts(state: tauri::State<'_, Db>) -> Result<Vec<AccountInfo>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let active = db::get_active_account(&conn)?;
    let mut out = Vec::new();
    for email in db::get_accounts(&conn)? {
        let unread = db::unread_count(&conn, &email)?;
        let active_flag = active.as_deref() == Some(email.as_str());
        out.push(AccountInfo { email, active: active_flag, unread });
    }
    Ok(out)
}

#[tauri::command]
pub async fn set_active_account(state: tauri::State<'_, Db>, email: String) -> Result<()> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    if !db::get_accounts(&conn)?.iter().any(|a| a == &email) {
        return Err(AppError::Other(format!("unknown account {email}")));
    }
    db::set_active_account(&conn, &email)
}

#[tauri::command]
pub async fn list_calendars(state: tauri::State<'_, Db>) -> Result<Vec<CalendarSummary>> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    let entries = client.list_calendars().await?;
    Ok(entries
        .into_iter()
        .map(|c| CalendarSummary {
            id: c.id,
            summary: c.summary.unwrap_or_else(|| "(unnamed)".to_string()),
            primary: c.primary.unwrap_or(false),
            writable: matches!(c.access_role.as_deref(), Some("owner") | Some("writer")),
        })
        .collect())
}

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

#[tauri::command]
pub async fn open_external(url: String) -> Result<()> {
    if !crate::calendar::is_safe_url(&url) {
        return Err(AppError::Other("refusing to open a non-web URL".into()));
    }
    open::that(&url).map_err(|e| AppError::Other(format!("couldn't open link: {e}")))?;
    Ok(())
}

#[tauri::command]
pub async fn respond_to_event(
    calendar_id: String,
    event_id: String,
    response_status: String,
    state: tauri::State<'_, Db>,
) -> Result<CalendarEvent> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    client
        .respond_to_event(&calendar_id, &event_id, &response_status, &stored.email)
        .await
}

#[tauri::command]
pub async fn get_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<Option<db::MeetingNote>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(None);
    };
    db::get_meeting_note(&conn, &calendar_id, &event_id, &account)
}

#[tauri::command]
pub async fn save_meeting_note(
    note: db::MeetingNoteWrite,
    state: tauri::State<'_, Db>,
) -> Result<db::MeetingNote> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let account = db::get_active_account(&conn)?
        .ok_or_else(|| AppError::Auth("no active account".into()))?;
    db::upsert_meeting_note(&conn, &account, &note, now_millis())
}

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

#[tauri::command]
pub async fn list_meeting_notes(state: tauri::State<'_, Db>) -> Result<Vec<db::MeetingNote>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let Some(account) = db::get_active_account(&conn)? else {
        return Ok(Vec::new());
    };
    db::list_meeting_notes(&conn, &account)
}

#[tauri::command]
pub async fn summarize_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<db::MeetingNote> {
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
    let summary = crate::ollama::OllamaClient::new().summarize(&input).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::set_meeting_note_summary(&conn, &calendar_id, &event_id, &account, &summary, now_millis())
}

#[tauri::command]
pub async fn read_transcript_file(path: String) -> Result<String> {
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
    let text = if path.to_lowercase().ends_with(".vtt") {
        crate::transcript::vtt_to_text(&raw)
    } else {
        raw
    };
    Ok(text.trim().to_string())
}

#[tauri::command]
pub async fn transcribe_recording(
    state: tauri::State<'_, crate::transcribe::TranscriberState>,
    path: String,
) -> Result<String> {
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
    let model_str = model.to_string_lossy().to_string();
    let loaded = tokio::task::spawn_blocking(move || crate::transcribe::Transcriber::load(&model_str))
        .await
        .map_err(|e| AppError::Other(e.to_string()))??;
    {
        let mut guard = state
            .lock()
            .map_err(|_| AppError::Other("transcriber state poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(loaded);
        }
    }
    let _ = on_progress.send(crate::model::PrepProgress::Ready);
    Ok(())
}

#[tauri::command]
pub async fn search_people(
    query: String,
    state: tauri::State<'_, Db>,
) -> Result<Vec<PersonHit>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let stored = active_token(&state).await?;
    let client = PeopleClient::new(stored.access_token);
    Ok(client.search(query.trim()).await)
}

#[derive(serde::Serialize)]
pub struct PersonBusy {
    pub email: String,
    pub busy: Vec<BusySpan>,
    pub error: Option<String>,
}

#[derive(serde::Serialize)]
pub struct FindTimesResult {
    pub grid: Vec<PersonBusy>,
    pub suggestions: Vec<Slot>,
    pub unavailable: Vec<String>,
}

#[tauri::command]
pub async fn find_meeting_times(
    attendees: Vec<String>,
    time_min: String,
    time_max: String,
    duration_min: i64,
    state: tauri::State<'_, Db>,
) -> Result<FindTimesResult> {
    let stored = active_token(&state).await?;

    // Always include the organizer's own calendar.
    let mut emails = attendees.clone();
    if !emails.iter().any(|e| e.eq_ignore_ascii_case(&stored.email)) {
        emails.insert(0, stored.email.clone());
    }

    let client = CalendarClient::new(stored.access_token);
    let fb = client.free_busy(&emails, &time_min, &time_max).await?;

    let range_start = DateTime::parse_from_rfc3339(&time_min)
        .map_err(|e| AppError::Other(format!("bad time_min: {e}")))?
        .with_timezone(&Utc);
    let range_end = DateTime::parse_from_rfc3339(&time_max)
        .map_err(|e| AppError::Other(format!("bad time_max: {e}")))?
        .with_timezone(&Utc);
    let tz = Local::now().offset().fix();

    Ok(build_find_times_result(&emails, &fb, range_start, range_end, tz, duration_min))
}

pub(crate) fn build_find_times_result(
    emails: &[String],
    fb: &crate::calendar::types::FreeBusyResult,
    range_start: chrono::DateTime<chrono::Utc>,
    range_end: chrono::DateTime<chrono::Utc>,
    tz: chrono::FixedOffset,
    duration_min: i64,
) -> FindTimesResult {
    // Build a lowercased-key view once so lookup is case-insensitive.
    let by_lc: std::collections::HashMap<String, &crate::calendar::types::PersonFreeBusy> =
        fb.calendars.iter().map(|(k, v)| (k.to_lowercase(), v)).collect();

    let mut grid = Vec::new();
    let mut unavailable = Vec::new();
    let mut busy_all: Vec<BusyInterval> = Vec::new();

    for email in emails {
        let (busy_spans, error) = match by_lc.get(&email.to_lowercase()) {
            Some(p) => (p.busy.clone(), p.error.clone()),
            None => (vec![], Some("no data".to_string())),
        };
        if error.is_some() {
            unavailable.push(email.clone());
        } else {
            for b in &busy_spans {
                if let (Ok(s), Ok(e)) = (
                    DateTime::parse_from_rfc3339(&b.start),
                    DateTime::parse_from_rfc3339(&b.end),
                ) {
                    busy_all.push(BusyInterval {
                        start: s.with_timezone(&Utc),
                        end: e.with_timezone(&Utc),
                    });
                }
            }
        }
        grid.push(PersonBusy { email: email.clone(), busy: busy_spans, error });
    }

    let suggestions = scheduling::suggest_slots(
        busy_all,
        range_start,
        range_end,
        tz,
        WorkingHours::default(),
        duration_min,
        30,
        6,
    );

    FindTimesResult { grid, suggestions, unavailable }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gmail::types::MessagePreview;

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
        let plan = plan_label_changes(&["TRASH".into()], &[]);
        assert!(plan.trash);
        assert!(!plan.untrash);
        assert!(plan.batch_add.is_empty(), "TRASH must NOT ride batchModify");
        assert!(plan.batch_remove.is_empty());
    }

    #[test]
    fn plan_routes_untrash_to_dedicated_endpoint() {
        let plan = plan_label_changes(&[], &["TRASH".into()]);
        assert!(!plan.trash);
        assert!(plan.untrash);
        assert!(plan.batch_add.is_empty());
        assert!(plan.batch_remove.is_empty(), "TRASH must NOT ride batchModify");
    }

    #[test]
    fn plan_keeps_non_trash_labels_on_batch_modify() {
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
            preview(vec![], true),
        ]);
        assert_eq!(rows[0].category, "newsletters");
        assert_eq!(rows[0].label_ids, "INBOX,CATEGORY_PROMOTIONS");
        assert_eq!(rows[1].category, "people");
        assert_eq!(rows[2].category, "newsletters");
        assert_eq!(rows[2].label_ids, "");
    }

    #[test]
    fn build_find_times_matches_email_case_insensitively_and_excludes_errored() {
        use crate::calendar::types::{BusySpan, FreeBusyResult, PersonFreeBusy};
        use chrono::{FixedOffset, TimeZone, Utc};
        let mut calendars = std::collections::HashMap::new();
        // Google echoes the organizer key lowercased though we requested mixed-case.
        calendars.insert(
            "john.doe@company.com".to_string(),
            PersonFreeBusy {
                busy: vec![BusySpan {
                    start: "2026-07-01T06:00:00Z".into(),
                    end: "2026-07-01T07:00:00Z".into(),
                }],
                error: None,
            },
        );
        calendars.insert(
            "ext@gmail.com".to_string(),
            PersonFreeBusy { busy: vec![], error: Some("notFound".into()) },
        );
        let fb = FreeBusyResult { calendars };
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        let emails = vec!["John.Doe@company.com".to_string(), "ext@gmail.com".to_string()];
        let range_start = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let range_end = Utc.with_ymd_and_hms(2026, 7, 1, 23, 0, 0).unwrap();

        let res = build_find_times_result(&emails, &fb, range_start, range_end, tz, 60);

        // External guest with an error is unavailable.
        assert_eq!(res.unavailable, vec!["ext@gmail.com".to_string()]);
        // Organizer matched despite casing → their 09:00-10:00 local busy is respected,
        // so the first suggestion must NOT be 09:00.
        assert!(!res.suggestions.is_empty(), "expected some suggestions");
        assert!(
            !res.suggestions[0].start.starts_with("2026-07-01T09:00:00"),
            "organizer busy 09:00-10:00 must be respected, got {}",
            res.suggestions[0].start
        );
        // Grid preserves original casing.
        assert!(res.grid.iter().any(|g| g.email == "John.Doe@company.com"));
    }
}
