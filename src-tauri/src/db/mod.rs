use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredMessage {
    pub id: String,
    pub thread_id: String,
    pub from_addr: String,
    pub subject: String,
    pub snippet: String,
    pub date_header: String,
    pub internal_date: i64,
    pub label_ids: String,
    pub to_addr: String,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncState {
    pub last_history_id: Option<i64>,
    pub last_synced_at: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub signature: String,
    pub remote_images: bool,
    pub notifications: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MeetingNote {
    pub id: i64,
    pub calendar_id: String,
    pub event_id: String,
    pub event_title: String,
    pub event_start: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub summary: String,
    pub summary_updated_at: i64,
    pub transcript: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MeetingNoteWrite {
    pub calendar_id: String,
    pub event_id: String,
    pub event_title: String,
    pub event_start: String,
    pub body: String,
    #[serde(default)]
    pub transcript: String,
}

pub fn init(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS messages (
            id            TEXT PRIMARY KEY,
            thread_id     TEXT NOT NULL DEFAULT '',
            from_addr     TEXT NOT NULL DEFAULT '',
            subject       TEXT NOT NULL DEFAULT '',
            snippet       TEXT NOT NULL DEFAULT '',
            date_header   TEXT NOT NULL DEFAULT '',
            internal_date INTEGER NOT NULL DEFAULT 0,
            label_ids     TEXT NOT NULL DEFAULT '',
            to_addr       TEXT NOT NULL DEFAULT '',
            has_list_unsubscribe INTEGER NOT NULL DEFAULT 0,
            has_list_id   INTEGER NOT NULL DEFAULT 0,
            category      TEXT NOT NULL DEFAULT '',
            account       TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_messages_internal_date
            ON messages(internal_date DESC);
        CREATE TABLE IF NOT EXISTS sync_state (
            account         TEXT PRIMARY KEY,
            last_history_id INTEGER,
            last_synced_at  INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS meeting_notes (
            id          INTEGER PRIMARY KEY,
            calendar_id TEXT NOT NULL,
            event_id    TEXT NOT NULL,
            event_title TEXT NOT NULL DEFAULT '',
            event_start TEXT NOT NULL DEFAULT '',
            body        TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL,
            summary     TEXT NOT NULL DEFAULT '',
            summary_updated_at INTEGER NOT NULL DEFAULT 0,
            transcript  TEXT NOT NULL DEFAULT '',
            account     TEXT NOT NULL DEFAULT '',
            UNIQUE(calendar_id, event_id)
        );
        CREATE TABLE IF NOT EXISTS snoozed (
            message_id    TEXT PRIMARY KEY,
            thread_id     TEXT NOT NULL DEFAULT '',
            wake_at       INTEGER NOT NULL,
            snoozed_at    INTEGER NOT NULL,
            from_addr     TEXT NOT NULL DEFAULT '',
            subject       TEXT NOT NULL DEFAULT '',
            snippet       TEXT NOT NULL DEFAULT '',
            internal_date INTEGER NOT NULL DEFAULT 0,
            account       TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_snoozed_wake_at ON snoozed(wake_at);",
    )?;

    let needs_migration = !column_exists(conn, "messages", "category")?;
    add_column_if_missing(conn, "messages", "label_ids", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "messages", "to_addr", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "messages", "has_list_unsubscribe", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "messages", "has_list_id", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "messages", "category", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "meeting_notes", "summary", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "meeting_notes", "summary_updated_at", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "meeting_notes", "transcript", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "messages", "account", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "snoozed", "account", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "meeting_notes", "account", "TEXT NOT NULL DEFAULT ''")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_category ON messages(category)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_account_internal_date
         ON messages(account, internal_date DESC)",
        [],
    )?;
    if needs_migration {
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM messages", [])?;
        tx.execute("UPDATE sync_state SET last_history_id = NULL", [])?;
        tx.commit()?;
    }
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, col: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == col {
            return Ok(true);
        }
    }
    Ok(false)
}

fn add_column_if_missing(conn: &Connection, table: &str, col: &str, decl: &str) -> Result<()> {
    if !column_exists(conn, table, col)? {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {decl}"), [])?;
    }
    Ok(())
}

const UPSERT_SQL: &str = "INSERT INTO messages
        (id, thread_id, from_addr, subject, snippet, date_header, internal_date,
         label_ids, to_addr, has_list_unsubscribe, has_list_id, category, account)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
     ON CONFLICT(id) DO UPDATE SET
        thread_id = excluded.thread_id,
        from_addr = excluded.from_addr,
        subject = excluded.subject,
        snippet = excluded.snippet,
        date_header = excluded.date_header,
        internal_date = excluded.internal_date,
        label_ids = excluded.label_ids,
        to_addr = excluded.to_addr,
        has_list_unsubscribe = excluded.has_list_unsubscribe,
        has_list_id = excluded.has_list_id,
        category = excluded.category,
        account = excluded.account";

fn upsert_one(conn: &Connection, account: &str, m: &StoredMessage) -> Result<()> {
    conn.execute(
        UPSERT_SQL,
        params![
            m.id, m.thread_id, m.from_addr, m.subject, m.snippet, m.date_header,
            m.internal_date, m.label_ids, m.to_addr, m.has_list_unsubscribe,
            m.has_list_id, m.category, account
        ],
    )?;
    Ok(())
}

pub fn upsert_messages(conn: &Connection, account: &str, messages: &[StoredMessage]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for m in messages {
        upsert_one(&tx, account, m)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn apply_delta(
    conn: &Connection,
    account: &str,
    upserts: &[StoredMessage],
    delete_ids: &[String],
    prune_cutoff_ms: i64,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for m in upserts {
        upsert_one(&tx, account, m)?;
    }
    for id in delete_ids {
        tx.execute("DELETE FROM messages WHERE id = ?1 AND account = ?2", params![id, account])?;
    }
    tx.execute(
        "DELETE FROM messages WHERE internal_date < ?1",
        params![prune_cutoff_ms],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn delete_messages(conn: &Connection, account: &str, ids: &[String]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        tx.execute("DELETE FROM messages WHERE id = ?1 AND account = ?2", params![id, account])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn update_message_labels(conn: &Connection, id: &str, label_ids_csv: &str, account: &str) -> Result<()> {
    conn.execute(
        "UPDATE messages SET label_ids = ?1 WHERE id = ?2 AND account = ?3",
        params![label_ids_csv, id, account],
    )?;
    Ok(())
}

pub fn apply_label_delta(conn: &Connection, account: &str, ids: &[String], add: &[String], remove: &[String]) -> Result<()> {
    if add.is_empty() && remove.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        let current: Option<String> = tx
            .query_row("SELECT label_ids FROM messages WHERE id = ?1 AND account = ?2", params![id, account], |r| r.get(0))
            .optional()?;
        let Some(csv) = current else { continue };
        let mut labels: Vec<String> = csv.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
        labels.retain(|l| !remove.contains(l));
        for a in add {
            if !labels.contains(a) {
                labels.push(a.clone());
            }
        }
        tx.execute("UPDATE messages SET label_ids = ?1 WHERE id = ?2 AND account = ?3", params![labels.join(","), id, account])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn prune_older_than(conn: &Connection, cutoff_ms: i64) -> Result<usize> {
    let removed = conn.execute(
        "DELETE FROM messages WHERE internal_date < ?1",
        params![cutoff_ms],
    )?;
    Ok(removed)
}

pub fn unread_count(conn: &Connection, account: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account = ?1 AND label_ids LIKE '%UNREAD%'",
        params![account],
        |r| r.get(0),
    )?;
    Ok(n)
}

pub fn recent_previews(conn: &Connection, account: &str, max: u32) -> Result<Vec<StoredMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, from_addr, subject, snippet, date_header, internal_date,
                label_ids, to_addr, has_list_unsubscribe, has_list_id, category
         FROM messages
         WHERE account = ?1
         ORDER BY internal_date DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![account, max], |row| {
        Ok(StoredMessage {
            id: row.get(0)?,
            thread_id: row.get(1)?,
            from_addr: row.get(2)?,
            subject: row.get(3)?,
            snippet: row.get(4)?,
            date_header: row.get(5)?,
            internal_date: row.get(6)?,
            label_ids: row.get(7)?,
            to_addr: row.get(8)?,
            has_list_unsubscribe: row.get(9)?,
            has_list_id: row.get(10)?,
            category: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_sync_state(conn: &Connection, account: &str) -> Result<Option<SyncState>> {
    let mut stmt =
        conn.prepare("SELECT last_history_id, last_synced_at FROM sync_state WHERE account = ?1")?;
    let mut rows = stmt.query_map(params![account], |row| {
        Ok(SyncState {
            last_history_id: row.get(0)?,
            last_synced_at: row.get(1)?,
        })
    })?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn set_sync_state(
    conn: &Connection,
    account: &str,
    last_history_id: Option<i64>,
    last_synced_at: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (account, last_history_id, last_synced_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(account) DO UPDATE SET
            last_history_id = excluded.last_history_id,
            last_synced_at = excluded.last_synced_at",
        params![account, last_history_id, last_synced_at],
    )?;
    Ok(())
}

fn get_setting_raw(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

fn set_setting_raw(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_settings(conn: &Connection) -> Result<Settings> {
    let signature = get_setting_raw(conn, "signature")?.unwrap_or_default();
    let remote_images = get_setting_raw(conn, "remote_images")?
        .map(|v| v == "1")
        .unwrap_or(true);
    let notifications = get_setting_raw(conn, "notifications")?
        .map(|v| v == "1")
        .unwrap_or(true);
    Ok(Settings { signature, remote_images, notifications })
}

pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    set_setting_raw(&tx, "signature", &s.signature)?;
    set_setting_raw(&tx, "remote_images", if s.remote_images { "1" } else { "0" })?;
    set_setting_raw(&tx, "notifications", if s.notifications { "1" } else { "0" })?;
    tx.commit()?;
    Ok(())
}

pub fn get_accounts(conn: &Connection) -> Result<Vec<String>> {
    match get_setting_raw(conn, "accounts")? {
        Some(json) => serde_json::from_str(&json).map_err(|e| crate::error::AppError::Other(e.to_string())),
        None => Ok(Vec::new()),
    }
}

pub fn add_account(conn: &Connection, email: &str) -> Result<()> {
    let mut accounts = get_accounts(conn)?;
    if !accounts.iter().any(|a| a == email) {
        accounts.push(email.to_string());
        let json = serde_json::to_string(&accounts).map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        set_setting_raw(conn, "accounts", &json)?;
    }
    Ok(())
}

pub fn remove_account(conn: &Connection, email: &str) -> Result<()> {
    let mut accounts = get_accounts(conn)?;
    let before = accounts.len();
    accounts.retain(|a| a != email);
    if accounts.len() != before {
        let json = serde_json::to_string(&accounts).map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        set_setting_raw(conn, "accounts", &json)?;
    }
    Ok(())
}

pub fn get_active_account(conn: &Connection) -> Result<Option<String>> {
    get_setting_raw(conn, "active_account")
}

pub fn set_active_account(conn: &Connection, email: &str) -> Result<()> {
    set_setting_raw(conn, "active_account", email)
}

pub fn clear_account_data(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM messages", [])?;
    tx.execute("DELETE FROM sync_state", [])?;
    tx.commit()?;
    Ok(())
}

pub fn remove_account_data(conn: &Connection, account: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM messages WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM snoozed WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM meeting_notes WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM sync_state WHERE account = ?1", params![account])?;
    tx.commit()?;
    Ok(())
}

pub fn clear_active_account(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = 'active_account'", [])?;
    Ok(())
}

pub fn remove_account_and_repoint(conn: &Connection, email: &str) -> Result<Option<String>> {
    let was_active = get_active_account(conn)?.as_deref() == Some(email);
    remove_account(conn, email)?;
    if was_active {
        match get_accounts(conn)?.first() {
            Some(e) => set_active_account(conn, e)?,
            None => clear_active_account(conn)?,
        }
    }
    get_active_account(conn)
}

pub fn stamp_legacy_account(conn: &Connection, email: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE messages SET account = ?1 WHERE account = ''", params![email])?;
    tx.execute("UPDATE snoozed SET account = ?1 WHERE account = ''", params![email])?;
    tx.execute("UPDATE meeting_notes SET account = ?1 WHERE account = ''", params![email])?;
    tx.execute("UPDATE sync_state SET account = ?1 WHERE account = 'primary'", params![email])?;
    tx.commit()?;
    Ok(())
}

const NOTE_COLS: &str = "id, calendar_id, event_id, event_title, event_start, body, created_at, updated_at, summary, summary_updated_at, transcript";

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<MeetingNote> {
    Ok(MeetingNote {
        id: row.get(0)?,
        calendar_id: row.get(1)?,
        event_id: row.get(2)?,
        event_title: row.get(3)?,
        event_start: row.get(4)?,
        body: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        summary: row.get(8)?,
        summary_updated_at: row.get(9)?,
        transcript: row.get(10)?,
    })
}

pub fn get_meeting_note(conn: &Connection, calendar_id: &str, event_id: &str, account: &str) -> Result<Option<MeetingNote>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM meeting_notes WHERE calendar_id = ?1 AND event_id = ?2 AND account = ?3"
    ))?;
    let note = stmt.query_row(params![calendar_id, event_id, account], row_to_note).optional()?;
    Ok(note)
}

pub fn list_meeting_notes(conn: &Connection, account: &str) -> Result<Vec<MeetingNote>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM meeting_notes WHERE account = ?1 ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map(params![account], row_to_note)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn upsert_meeting_note(conn: &Connection, account: &str, w: &MeetingNoteWrite, now_ms: i64) -> Result<MeetingNote> {
    conn.execute(
        "INSERT INTO meeting_notes
            (calendar_id, event_id, event_title, event_start, body, transcript, created_at, updated_at, account)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)
         ON CONFLICT(calendar_id, event_id) DO UPDATE SET
            event_title = excluded.event_title,
            event_start = excluded.event_start,
            body = excluded.body,
            transcript = excluded.transcript,
            updated_at = excluded.updated_at,
            account = excluded.account",
        params![w.calendar_id, w.event_id, w.event_title, w.event_start, w.body, w.transcript, now_ms, account],
    )?;
    get_meeting_note(conn, &w.calendar_id, &w.event_id, account)?
        .ok_or_else(|| crate::error::AppError::Other("meeting note vanished after upsert".into()))
}

pub fn delete_meeting_note(conn: &Connection, calendar_id: &str, event_id: &str, account: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM meeting_notes WHERE calendar_id = ?1 AND event_id = ?2 AND account = ?3",
        params![calendar_id, event_id, account],
    )?;
    Ok(())
}

pub fn set_meeting_note_summary(
    conn: &Connection,
    calendar_id: &str,
    event_id: &str,
    account: &str,
    summary: &str,
    now_ms: i64,
) -> Result<MeetingNote> {
    conn.execute(
        "UPDATE meeting_notes SET summary = ?1, summary_updated_at = ?2
         WHERE calendar_id = ?3 AND event_id = ?4 AND account = ?5",
        params![summary, now_ms, calendar_id, event_id, account],
    )?;
    get_meeting_note(conn, calendar_id, event_id, account)?
        .ok_or_else(|| crate::error::AppError::Other("note not found".into()))
}


#[derive(Debug, Clone, serde::Serialize)]
pub struct SnoozedRow {
    pub message_id: String,
    pub thread_id: String,
    pub wake_at: i64,
    pub snoozed_at: i64,
    pub from_addr: String,
    pub subject: String,
    pub snippet: String,
    pub internal_date: i64,
}

pub fn insert_snooze(conn: &Connection, account: &str, r: &SnoozedRow) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO snoozed
           (message_id, thread_id, wake_at, snoozed_at, from_addr, subject, snippet, internal_date, account)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![r.message_id, r.thread_id, r.wake_at, r.snoozed_at, r.from_addr, r.subject, r.snippet, r.internal_date, account],
    )?;
    Ok(())
}

pub fn delete_snoozes(conn: &Connection, account: &str, ids: &[String]) -> Result<()> {
    if ids.is_empty() { return Ok(()); }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM snoozed WHERE message_id IN ({placeholders}) AND account = ?");
    let mut refs: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    refs.push(&account as &dyn rusqlite::ToSql);
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

pub fn due_snoozes(conn: &Connection, account: &str, now_ms: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT message_id FROM snoozed WHERE wake_at <= ?1 AND account = ?2 ORDER BY wake_at ASC",
    )?;
    let rows = stmt.query_map(params![now_ms, account], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn list_snoozes(conn: &Connection, account: &str) -> Result<Vec<SnoozedRow>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, thread_id, wake_at, snoozed_at, from_addr, subject, snippet, internal_date
         FROM snoozed WHERE account = ?1 ORDER BY wake_at ASC",
    )?;
    let rows = stmt.query_map(params![account], |r| Ok(SnoozedRow {
        message_id: r.get(0)?,
        thread_id:  r.get(1)?,
        wake_at:    r.get(2)?,
        snoozed_at: r.get(3)?,
        from_addr:  r.get(4)?,
        subject:    r.get(5)?,
        snippet:    r.get(6)?,
        internal_date: r.get(7)?,
    }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, internal_date: i64) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            thread_id: "t".into(),
            from_addr: "a@b.com".into(),
            subject: "subj".into(),
            snippet: "snip".into(),
            date_header: "Wed, 18 Jun 2026".into(),
            internal_date,
            label_ids: String::new(),
            to_addr: String::new(),
            has_list_unsubscribe: false,
            has_list_id: false,
            category: "people".into(),
        }
    }

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }

    #[test]
    fn init_is_idempotent() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        init(&c).unwrap();
        upsert_messages(&c, "me@x.com", &[msg("a", 1)]).unwrap();
        assert_eq!(recent_previews(&c, "me@x.com", 10).unwrap().len(), 1);
    }

    #[test]
    fn upsert_updates_existing_row() {
        let c = conn();
        upsert_messages(&c, "me@x.com", &[msg("a", 1)]).unwrap();
        let mut updated = msg("a", 1);
        updated.subject = "new subject".into();
        upsert_messages(&c, "me@x.com", &[updated]).unwrap();
        let rows = recent_previews(&c, "me@x.com", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject, "new subject");
    }

    #[test]
    fn recent_previews_orders_newest_first_and_limits() {
        let c = conn();
        upsert_messages(&c, "me@x.com", &[msg("old", 100), msg("new", 300), msg("mid", 200)]).unwrap();
        let rows = recent_previews(&c, "me@x.com", 2).unwrap();
        assert_eq!(
            rows.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "mid"]
        );
    }

    #[test]
    fn recent_previews_filters_by_account() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let a = msg("a1", 1);
        let b = msg("b1", 2);
        upsert_messages(&c, "a@x.com", std::slice::from_ref(&a)).unwrap();
        upsert_messages(&c, "b@x.com", std::slice::from_ref(&b)).unwrap();
        let only_a = recent_previews(&c, "a@x.com", 50).unwrap();
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_a[0].id, "a1");
    }

    #[test]
    fn delete_messages_removes_only_given_ids() {
        let c = conn();
        upsert_messages(&c, "me@x.com", &[msg("a", 1), msg("b", 2), msg("c", 3)]).unwrap();
        delete_messages(&c, "me@x.com", &["b".to_string()]).unwrap();
        let ids: Vec<String> = recent_previews(&c, "me@x.com", 10)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec!["c".to_string(), "a".to_string()]);
    }

    #[test]
    fn prune_older_than_deletes_below_cutoff_and_counts() {
        let c = conn();
        upsert_messages(&c, "me@x.com", &[msg("old", 100), msg("new", 300)]).unwrap();
        let removed = prune_older_than(&c, 200).unwrap();
        assert_eq!(removed, 1);
        let ids: Vec<String> = recent_previews(&c, "me@x.com", 10)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec!["new".to_string()]);
    }

    #[test]
    fn sync_state_round_trips_and_missing_is_none() {
        let c = conn();
        assert_eq!(get_sync_state(&c, "primary").unwrap(), None);
        set_sync_state(&c, "primary", Some(42), 1718700000).unwrap();
        let s = get_sync_state(&c, "primary").unwrap().unwrap();
        assert_eq!(s.last_history_id, Some(42));
        assert_eq!(s.last_synced_at, 1718700000);
    }

    #[test]
    fn apply_delta_upserts_deletes_and_prunes_atomically() {
        let c = conn();
        upsert_messages(&c, "me@x.com", &[msg("keep", 500), msg("archive", 600), msg("old", 100)]).unwrap();
        apply_delta(&c, "me@x.com", &[msg("newmsg", 700)], &["archive".to_string()], 200).unwrap();
        let mut ids: Vec<String> = recent_previews(&c, "me@x.com", 10)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["keep".to_string(), "newmsg".to_string()]);
    }

    #[test]
    fn migration_adds_columns_wipes_cache_and_is_idempotent() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY, thread_id TEXT NOT NULL DEFAULT '',
                from_addr TEXT NOT NULL DEFAULT '', subject TEXT NOT NULL DEFAULT '',
                snippet TEXT NOT NULL DEFAULT '', date_header TEXT NOT NULL DEFAULT '',
                internal_date INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE sync_state (account TEXT PRIMARY KEY, last_history_id INTEGER,
                last_synced_at INTEGER NOT NULL DEFAULT 0);
             INSERT INTO messages (id) VALUES ('old1');
             INSERT INTO sync_state (account, last_history_id, last_synced_at)
                VALUES ('primary', 42, 1);",
        )
        .unwrap();

        init(&c).unwrap();

        assert!(column_exists(&c, "messages", "category").unwrap());
        assert_eq!(recent_previews(&c, "me@x.com", 10).unwrap().len(), 0);
        assert_eq!(
            get_sync_state(&c, "primary").unwrap().unwrap().last_history_id,
            None
        );

        upsert_messages(&c, "me@x.com", &[msg("fresh", 5)]).unwrap();
        init(&c).unwrap();
        assert!(column_exists(&c, "messages", "category").unwrap());
        let rows = recent_previews(&c, "me@x.com", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "fresh");
    }

    #[test]
    fn update_message_labels_changes_only_labels() {
        let c = conn();
        let mut m = msg("a", 1);
        m.category = "people".into();
        m.label_ids = "INBOX,UNREAD".into();
        upsert_messages(&c, "me@x.com", &[m]).unwrap();

        update_message_labels(&c, "a", "INBOX", "me@x.com").unwrap();

        let rows = recent_previews(&c, "me@x.com", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label_ids, "INBOX");
        assert_eq!(rows[0].category, "people");
    }

    #[test]
    fn upsert_and_read_preserve_category_and_signals() {
        let c = conn();
        let mut m = msg("a", 1);
        m.category = "newsletters".into();
        m.has_list_unsubscribe = true;
        m.label_ids = "INBOX,CATEGORY_PROMOTIONS".into();
        upsert_messages(&c, "me@x.com", &[m]).unwrap();
        let rows = recent_previews(&c, "me@x.com", 10).unwrap();
        assert_eq!(rows[0].category, "newsletters");
        assert!(rows[0].has_list_unsubscribe);
        assert_eq!(rows[0].label_ids, "INBOX,CATEGORY_PROMOTIONS");
    }

    #[test]
    fn get_settings_returns_defaults_when_empty() {
        let c = conn();
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "");
        assert!(s.remote_images);
        assert!(s.notifications);
    }

    #[test]
    fn save_then_get_settings_round_trips() {
        let c = conn();
        save_settings(
            &c,
            &Settings { signature: "Cheers,\nDmytro".into(), remote_images: false, notifications: false },
        )
        .unwrap();
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "Cheers,\nDmytro");
        assert!(!s.remote_images);
        assert!(!s.notifications);
        save_settings(
            &c,
            &Settings { signature: "Cheers,\nDmytro".into(), remote_images: true, notifications: true },
        )
        .unwrap();
        let s = get_settings(&c).unwrap();
        assert!(s.remote_images);
        assert!(s.notifications);
    }

    #[test]
    fn apply_label_delta_adds_and_removes_on_cached_rows() {
        let c = conn();
        let mut m = msg("x", 1);
        m.label_ids = "INBOX,UNREAD".into();
        upsert_messages(&c, "me@x.com", &[m]).unwrap();

        apply_label_delta(&c, "me@x.com", &["x".to_string()], &["STARRED".to_string()], &["UNREAD".to_string()]).unwrap();
        let labels: Vec<String> = recent_previews(&c, "me@x.com", 10).unwrap()[0]
            .label_ids
            .split(',')
            .map(String::from)
            .collect();
        assert!(labels.contains(&"INBOX".to_string()));
        assert!(labels.contains(&"STARRED".to_string()));
        assert!(!labels.contains(&"UNREAD".to_string()));

        apply_label_delta(&c, "me@x.com", &["x".to_string()], &["STARRED".to_string()], &["UNREAD".to_string()]).unwrap();
        let again: Vec<String> = recent_previews(&c, "me@x.com", 10).unwrap()[0].label_ids.split(',').map(String::from).collect();
        assert_eq!(again.iter().filter(|l| *l == "STARRED").count(), 1);
        assert!(again.contains(&"INBOX".to_string()));
        assert!(!again.contains(&"UNREAD".to_string()));

        let before = recent_previews(&c, "me@x.com", 10).unwrap()[0].label_ids.clone();
        apply_label_delta(&c, "me@x.com", &["nope".to_string()], &[], &["INBOX".to_string()]).unwrap();
        assert_eq!(recent_previews(&c, "me@x.com", 10).unwrap()[0].label_ids, before);
    }

    #[test]
    fn clear_account_data_wipes_cache_but_keeps_settings() {
        let c = conn();
        upsert_messages(&c, "me@x.com", &[msg("a", 1)]).unwrap();
        set_sync_state(&c, "primary", Some(7), 1).unwrap();
        save_settings(&c, &Settings { signature: "sig".into(), remote_images: false, notifications: false }).unwrap();

        clear_account_data(&c).unwrap();

        assert_eq!(recent_previews(&c, "me@x.com", 10).unwrap().len(), 0);
        assert_eq!(get_sync_state(&c, "primary").unwrap(), None);
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "sig");
        assert!(!s.remote_images);
        assert!(!s.notifications);
    }

    #[test]
    fn remove_account_data_only_wipes_that_account() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c.execute("INSERT INTO messages (id, account) VALUES ('a1','a@x.com')", []).unwrap();
        c.execute("INSERT INTO messages (id, account) VALUES ('b1','b@x.com')", []).unwrap();
        c.execute("INSERT INTO sync_state (account,last_history_id,last_synced_at) VALUES ('a@x.com',1,0)", []).unwrap();
        remove_account_data(&c, "a@x.com").unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let left: String = c.query_row("SELECT account FROM messages", [], |r| r.get(0)).unwrap();
        assert_eq!(left, "b@x.com");
        let synced: i64 = c.query_row("SELECT COUNT(*) FROM sync_state WHERE account='a@x.com'", [], |r| r.get(0)).unwrap();
        assert_eq!(synced, 0);
    }

    fn note_write(cal: &str, ev: &str, body: &str) -> MeetingNoteWrite {
        MeetingNoteWrite {
            calendar_id: cal.into(),
            event_id: ev.into(),
            event_title: "Standup".into(),
            event_start: "2026-06-22T09:00:00-07:00".into(),
            body: body.into(),
            transcript: "".into(),
        }
    }

    #[test]
    fn meeting_note_upsert_inserts_then_updates_same_row() {
        let c = conn();
        let inserted = upsert_meeting_note(&c, "me@x.com", &note_write("primary", "e1", "first"), 1000).unwrap();
        assert_eq!(inserted.created_at, 1000);
        assert_eq!(inserted.updated_at, 1000);
        assert_eq!(inserted.body, "first");

        let mut w = note_write("primary", "e1", "second");
        w.event_title = "Standup (edited)".into();
        let updated = upsert_meeting_note(&c, "me@x.com", &w, 2000).unwrap();
        assert_eq!(updated.id, inserted.id);
        assert_eq!(updated.created_at, 1000);
        assert_eq!(updated.updated_at, 2000);
        assert_eq!(updated.body, "second");
        assert_eq!(updated.event_title, "Standup (edited)");
        assert_eq!(list_meeting_notes(&c, "me@x.com").unwrap().len(), 1);
    }

    #[test]
    fn meeting_note_get_returns_some_and_none() {
        let c = conn();
        assert!(get_meeting_note(&c, "primary", "missing", "me@x.com").unwrap().is_none());
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "e1", "hi"), 1).unwrap();
        let got = get_meeting_note(&c, "primary", "e1", "me@x.com").unwrap().unwrap();
        assert_eq!(got.body, "hi");
        assert!(get_meeting_note(&c, "other", "e1", "me@x.com").unwrap().is_none());
    }

    #[test]
    fn list_meeting_notes_orders_by_updated_at_desc() {
        let c = conn();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "old", "o"), 100).unwrap();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "new", "n"), 300).unwrap();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "mid", "m"), 200).unwrap();
        let ids: Vec<String> = list_meeting_notes(&c, "me@x.com").unwrap().into_iter().map(|n| n.event_id).collect();
        assert_eq!(ids, vec!["new".to_string(), "mid".to_string(), "old".to_string()]);
    }

    #[test]
    fn delete_meeting_note_removes_only_that_note() {
        let c = conn();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "a", "a"), 1).unwrap();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "b", "b"), 2).unwrap();
        delete_meeting_note(&c, "primary", "a", "me@x.com").unwrap();
        let ids: Vec<String> = list_meeting_notes(&c, "me@x.com").unwrap().into_iter().map(|n| n.event_id).collect();
        assert_eq!(ids, vec!["b".to_string()]);
        assert!(get_meeting_note(&c, "primary", "a", "me@x.com").unwrap().is_none());
    }

    #[test]
    fn init_creates_meeting_notes_table_idempotently() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        init(&c).unwrap();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "e1", "ok"), 1).unwrap();
        assert_eq!(list_meeting_notes(&c, "me@x.com").unwrap().len(), 1);
    }

    #[test]
    fn set_meeting_note_summary_sets_summary_without_bumping_updated_at() {
        let c = conn();
        let n = upsert_meeting_note(&c, "me@x.com", &note_write("primary", "e1", "body"), 1000).unwrap();
        assert_eq!(n.summary, "");
        assert_eq!(n.summary_updated_at, 0);
        assert_eq!(n.updated_at, 1000);

        let updated = set_meeting_note_summary(&c, "primary", "e1", "me@x.com", "## Summary\n- ok", 2000).unwrap();
        assert_eq!(updated.summary, "## Summary\n- ok");
        assert_eq!(updated.summary_updated_at, 2000);
        assert_eq!(updated.updated_at, 1000);
        assert_eq!(updated.created_at, 1000);
    }

    #[test]
    fn body_resave_preserves_existing_summary() {
        let c = conn();
        upsert_meeting_note(&c, "me@x.com", &note_write("primary", "e1", "body1"), 1000).unwrap();
        set_meeting_note_summary(&c, "primary", "e1", "me@x.com", "the summary", 1500).unwrap();
        let mut w = note_write("primary", "e1", "body2");
        w.event_title = "1:1".into();
        let after = upsert_meeting_note(&c, "me@x.com", &w, 3000).unwrap();
        assert_eq!(after.body, "body2");
        assert_eq!(after.updated_at, 3000);
        assert_eq!(after.summary, "the summary");
        assert_eq!(after.summary_updated_at, 1500);
    }

    #[test]
    fn init_adds_summary_columns_to_an_m20_shaped_table() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE meeting_notes (
                id INTEGER PRIMARY KEY, calendar_id TEXT NOT NULL, event_id TEXT NOT NULL,
                event_title TEXT NOT NULL DEFAULT '', event_start TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                UNIQUE(calendar_id, event_id));
             INSERT INTO meeting_notes
                (calendar_id, event_id, event_title, event_start, body, created_at, updated_at)
                VALUES ('primary','e1','T','2026-01-01','b',1,1);",
        )
        .unwrap();

        init(&c).unwrap();

        let n = get_meeting_note(&c, "primary", "e1", "").unwrap().unwrap();
        assert_eq!(n.summary, "");
        assert_eq!(n.summary_updated_at, 0);
        assert_eq!(n.body, "b");

        init(&c).unwrap();
        assert!(get_meeting_note(&c, "primary", "e1", "").unwrap().is_some());
    }

    #[test]
    fn upsert_meeting_note_round_trips_transcript() {
        let c = conn();
        let mut w = note_write("primary", "e1", "body");
        w.transcript = "line one\nline two".into();
        let n = upsert_meeting_note(&c, "me@x.com", &w, 1000).unwrap();
        assert_eq!(n.transcript, "line one\nline two");
        let got = get_meeting_note(&c, "primary", "e1", "me@x.com").unwrap().unwrap();
        assert_eq!(got.transcript, "line one\nline two");
    }

    #[test]
    fn body_transcript_resave_preserves_summary() {
        let c = conn();
        let mut w = note_write("primary", "e1", "body1");
        w.transcript = "t1".into();
        upsert_meeting_note(&c, "me@x.com", &w, 1000).unwrap();
        set_meeting_note_summary(&c, "primary", "e1", "me@x.com", "sum", 1200).unwrap();
        let mut w2 = note_write("primary", "e1", "body2");
        w2.transcript = "t2".into();
        let after = upsert_meeting_note(&c, "me@x.com", &w2, 3000).unwrap();
        assert_eq!(after.body, "body2");
        assert_eq!(after.transcript, "t2");
        assert_eq!(after.updated_at, 3000);
        assert_eq!(after.summary, "sum");
        assert_eq!(after.summary_updated_at, 1200);
    }

    #[test]
    fn snooze_insert_due_list_delete() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let row = |id: &str, wake: i64| SnoozedRow {
            message_id: id.into(), thread_id: "t".into(), wake_at: wake, snoozed_at: 1,
            from_addr: "a@b.co".into(), subject: "s".into(), snippet: "sn".into(), internal_date: wake,
        };
        insert_snooze(&c, "me@x.com", &row("a", 1000)).unwrap();
        insert_snooze(&c, "me@x.com", &row("b", 3000)).unwrap();
        assert_eq!(due_snoozes(&c, "me@x.com", 999).unwrap(), Vec::<String>::new());
        assert_eq!(due_snoozes(&c, "me@x.com", 1000).unwrap(), vec!["a".to_string()]);
        assert_eq!(due_snoozes(&c, "me@x.com", 5000).unwrap(), vec!["a".to_string(), "b".to_string()]);
        insert_snooze(&c, "me@x.com", &row("a", 9000)).unwrap();
        assert_eq!(list_snoozes(&c, "me@x.com").unwrap().len(), 2);
        delete_snoozes(&c, "me@x.com", &["a".to_string()]).unwrap();
        let left = list_snoozes(&c, "me@x.com").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].message_id, "b");
    }

    #[test]
    fn snoozes_filter_by_account() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let row = |id: &str, wake: i64| SnoozedRow {
            message_id: id.into(), thread_id: "t".into(), wake_at: wake, snoozed_at: 1,
            from_addr: "a@b.co".into(), subject: "s".into(), snippet: "sn".into(), internal_date: wake,
        };
        let row_a = row("s_a", 1000);
        let row_b = row("s_b", 1000);
        insert_snooze(&c, "a@x.com", &row_a).unwrap();
        insert_snooze(&c, "b@x.com", &row_b).unwrap();
        assert_eq!(list_snoozes(&c, "a@x.com").unwrap().len(), 1);
        assert_eq!(list_snoozes(&c, "b@x.com").unwrap().len(), 1);
        assert_eq!(due_snoozes(&c, "a@x.com", 5000).unwrap(), vec!["s_a".to_string()]);
        delete_snoozes(&c, "a@x.com", &["s_b".to_string()]).unwrap();
        assert_eq!(list_snoozes(&c, "b@x.com").unwrap().len(), 1);
        delete_snoozes(&c, "b@x.com", &["s_b".to_string()]).unwrap();
        assert_eq!(list_snoozes(&c, "b@x.com").unwrap().len(), 0);
    }

    #[test]
    fn meeting_notes_filter_by_account() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let note_a = note_write("primary", "ev_a", "note a");
        let note_b = note_write("primary", "ev_b", "note b");
        upsert_meeting_note(&c, "a@x.com", &note_a, 1000).unwrap();
        upsert_meeting_note(&c, "b@x.com", &note_b, 1000).unwrap();
        assert_eq!(list_meeting_notes(&c, "a@x.com").unwrap().len(), 1);
        assert_eq!(list_meeting_notes(&c, "b@x.com").unwrap().len(), 1);
        assert!(get_meeting_note(&c, &note_b.calendar_id, &note_b.event_id, "a@x.com").unwrap().is_none());
        assert!(get_meeting_note(&c, &note_b.calendar_id, &note_b.event_id, "b@x.com").unwrap().is_some());
    }

    #[test]
    fn init_adds_transcript_column_to_a_pre_m22_table() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE meeting_notes (
                id INTEGER PRIMARY KEY, calendar_id TEXT NOT NULL, event_id TEXT NOT NULL,
                event_title TEXT NOT NULL DEFAULT '', event_start TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                summary TEXT NOT NULL DEFAULT '', summary_updated_at INTEGER NOT NULL DEFAULT 0,
                UNIQUE(calendar_id, event_id));
             INSERT INTO meeting_notes
                (calendar_id, event_id, event_title, event_start, body, created_at, updated_at)
                VALUES ('primary','e1','T','2026-01-01','b',1,1);",
        )
        .unwrap();
        init(&c).unwrap();
        let n = get_meeting_note(&c, "primary", "e1", "").unwrap().unwrap();
        assert_eq!(n.transcript, "");
        assert_eq!(n.body, "b");
        init(&c).unwrap();
        assert!(get_meeting_note(&c, "primary", "e1", "").unwrap().is_some());
    }

    #[test]
    fn accounts_index_round_trips() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        assert_eq!(get_accounts(&c).unwrap(), Vec::<String>::new());
        add_account(&c, "a@gmail.com").unwrap();
        add_account(&c, "b@gmail.com").unwrap();
        add_account(&c, "a@gmail.com").unwrap();
        assert_eq!(get_accounts(&c).unwrap(), vec!["a@gmail.com", "b@gmail.com"]);
        remove_account(&c, "a@gmail.com").unwrap();
        assert_eq!(get_accounts(&c).unwrap(), vec!["b@gmail.com"]);
    }

    #[test]
    fn stamp_legacy_account_backfills_empty_account_rows() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c.execute("INSERT INTO messages (id, account) VALUES ('m1', '')", []).unwrap();
        c.execute("INSERT INTO sync_state (account, last_history_id, last_synced_at) VALUES ('primary', 5, 0)", []).unwrap();
        stamp_legacy_account(&c, "me@gmail.com").unwrap();
        let acct: String = c.query_row("SELECT account FROM messages WHERE id='m1'", [], |r| r.get(0)).unwrap();
        assert_eq!(acct, "me@gmail.com");
        let sacct: String = c.query_row("SELECT account FROM sync_state", [], |r| r.get(0)).unwrap();
        assert_eq!(sacct, "me@gmail.com");
    }

    #[test]
    fn active_account_pointer_round_trips() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        assert_eq!(get_active_account(&c).unwrap(), None);
        set_active_account(&c, "b@gmail.com").unwrap();
        assert_eq!(get_active_account(&c).unwrap(), Some("b@gmail.com".to_string()));
    }

    #[test]
    fn remove_account_and_repoint_covers_all_three_branches() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        for e in ["a@x.com", "b@x.com", "c@x.com"] {
            add_account(&c, e).unwrap();
        }
        set_active_account(&c, "b@x.com").unwrap();
        assert_eq!(
            remove_account_and_repoint(&c, "c@x.com").unwrap(),
            Some("b@x.com".to_string())
        );
        assert_eq!(
            remove_account_and_repoint(&c, "b@x.com").unwrap(),
            Some("a@x.com".to_string())
        );
        assert_eq!(remove_account_and_repoint(&c, "a@x.com").unwrap(), None);
        assert_eq!(get_accounts(&c).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn unread_count_counts_unread_for_account() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c.execute("INSERT INTO messages (id, account, label_ids) VALUES ('a1','a@x.com','INBOX,UNREAD')", []).unwrap();
        c.execute("INSERT INTO messages (id, account, label_ids) VALUES ('a2','a@x.com','INBOX')", []).unwrap();
        c.execute("INSERT INTO messages (id, account, label_ids) VALUES ('b1','b@x.com','INBOX,UNREAD')", []).unwrap();
        assert_eq!(unread_count(&c, "a@x.com").unwrap(), 1);
        assert_eq!(unread_count(&c, "b@x.com").unwrap(), 1);
    }

    #[test]
    fn recent_previews_returns_up_to_max_above_50() {
        let c = conn();
        for i in 0..60u32 {
            let mut m = msg(&format!("m{i}"), 0);
            m.internal_date = 1_000 + i as i64;
            upsert_messages(&c, "me@x.com", std::slice::from_ref(&m)).unwrap();
        }
        let rows = recent_previews(&c, "me@x.com", 55).unwrap();
        assert_eq!(rows.len(), 55);
        assert_eq!(rows[0].id, "m59");
    }
}
