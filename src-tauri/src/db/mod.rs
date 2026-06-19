// 🦀 `Connection` is an owned handle to a single SQLite database file (or an
//    in-memory DB).  It IS `Send` (you can move it to another thread) but NOT
//    `Sync` (you can't share `&Connection` across threads at the same time).
//    Wrapping it in a `Mutex` gives safe shared access — `Mutex<Connection>` is
//    `Sync` — which is exactly what lets us hold it as shared Tauri state.
use rusqlite::{params, Connection};

// 🦀 `crate::error::Result` is the project's own `Result<T>` alias — it expands
//    to `std::result::Result<T, AppError>`.  Because `AppError` has a
//    `#[from] rusqlite::Error` variant, the `?` operator auto-converts any
//    `rusqlite::Error` into an `AppError` without extra boilerplate.
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
    // 🦀 Smart-inbox signals (M6). Stored so the cache can be re-scored later without
    //    re-fetching from Gmail. `label_ids` is the Gmail labels joined by commas.
    pub label_ids: String,
    pub to_addr: String,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
    /// The classifier's verdict: "people" | "notifications" | "newsletters".
    pub category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncState {
    // 🦀 `Option<i64>` maps directly to a nullable INTEGER column in SQLite.
    //    When `row.get(0)?` reads a NULL it returns `None`; a real integer
    //    becomes `Some(n)`.  This is handled automatically by the `FromSql`
    //    trait impl for `Option<T>`.
    pub last_history_id: Option<i64>,
    pub last_synced_at: i64,
}

/// Create tables and indexes if they don't exist. Safe to call on every startup.
pub fn init(conn: &Connection) -> Result<()> {
    // 🦀 WAL (write-ahead logging) lets reads proceed while a write is in progress —
    //    better concurrency and crash durability than the default rollback journal.
    //    (On an in-memory DB this is a no-op, which keeps the unit tests working.)
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // 🦀 If the DB is briefly locked by another connection, wait up to 5s for it to
    //    free up instead of immediately returning SQLITE_BUSY.
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    // 🦀 `execute_batch` runs multiple semicolon-separated SQL statements in one
    //    call, unlike `execute` which runs exactly one statement.  It's safe here
    //    because we're supplying literal SQL with no user-controlled values —
    //    never use `execute_batch` with dynamically-built strings.
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
            category      TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_messages_internal_date
            ON messages(internal_date DESC);
        CREATE TABLE IF NOT EXISTS sync_state (
            account         TEXT PRIMARY KEY,
            last_history_id INTEGER,
            last_synced_at  INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // 🦀 Additive migration for DBs created before the M6 smart-inbox columns. The
    //    CREATE TABLE above only fires on a brand-new DB (IF NOT EXISTS), so existing
    //    installs need their columns added here. `category` is always the LAST new
    //    column added below, so its absence is a reliable sentinel for "old schema".
    //    Capture it BEFORE adding anything — otherwise the check is always false and
    //    the one-time wipe would never run.
    let needs_migration = !column_exists(conn, "messages", "category")?;
    add_column_if_missing(conn, "messages", "label_ids", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "messages", "to_addr", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "messages", "has_list_unsubscribe", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "messages", "has_list_id", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(conn, "messages", "category", "TEXT NOT NULL DEFAULT ''")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_category ON messages(category)",
        [],
    )?;
    if needs_migration {
        // 🦀 Old rows lack label/header data, so they can't be scored. The messages
        //    table is a pure local cache of Gmail — wipe it and clear the history
        //    baseline so the next sync does a full 30-day resync with full data.
        // 🦀 Both run in ONE transaction (like apply_delta): otherwise a crash between
        //    them could leave the cache emptied while last_history_id still points at a
        //    baseline, so the next sync would skip the repairing resync and show nothing.
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM messages", [])?;
        tx.execute("UPDATE sync_state SET last_history_id = NULL", [])?;
        tx.commit()?;
    }
    Ok(())
}

// 🦀 Does `table` have a column named `col`? PRAGMA statements can't take bound
//    params, but `table` here is always an internal constant ("messages"), never
//    user input, so formatting it in is injection-safe. `query_map` yields each
//    column's name (index 1 of PRAGMA table_info), and `?` propagates row errors.
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

// 🦀 Idempotent `ALTER TABLE … ADD COLUMN`: SQLite has no "ADD COLUMN IF NOT
//    EXISTS", so we guard with `column_exists`. `decl` includes the type and a
//    DEFAULT (required when adding a NOT NULL column to a populated table).
fn add_column_if_missing(conn: &Connection, table: &str, col: &str, decl: &str) -> Result<()> {
    if !column_exists(conn, table, col)? {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {decl}"), [])?;
    }
    Ok(())
}

// 🦀 The UPSERT statement, in one `const` so `upsert_messages` and `apply_delta`
//    share exactly one definition instead of duplicating the SQL.
const UPSERT_SQL: &str = "INSERT INTO messages
        (id, thread_id, from_addr, subject, snippet, date_header, internal_date,
         label_ids, to_addr, has_list_unsubscribe, has_list_id, category)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
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
        category = excluded.category";

// 🦀 Upsert one message against a connection OR a transaction. rusqlite's `Transaction`
//    derefs to `Connection`, so callers can pass `&tx` here and it coerces to `&Connection`.
fn upsert_one(conn: &Connection, m: &StoredMessage) -> Result<()> {
    conn.execute(
        UPSERT_SQL,
        params![
            m.id, m.thread_id, m.from_addr, m.subject, m.snippet, m.date_header,
            m.internal_date, m.label_ids, m.to_addr, m.has_list_unsubscribe,
            m.has_list_id, m.category
        ],
    )?;
    Ok(())
}

/// Insert each message, or update it in place if its id already exists.
pub fn upsert_messages(conn: &Connection, messages: &[StoredMessage]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for m in messages {
        upsert_one(&tx, m)?;
    }
    tx.commit()?;
    Ok(())
}

/// Apply a sync delta in ONE transaction: upsert `upserts`, delete `delete_ids`, and
/// prune messages older than `prune_cutoff_ms`. All-or-nothing.
pub fn apply_delta(
    conn: &Connection,
    upserts: &[StoredMessage],
    delete_ids: &[String],
    prune_cutoff_ms: i64,
) -> Result<()> {
    // 🦀 A single transaction spanning all three steps: if any fails, `tx` drops without
    //    committing and the whole delta rolls back — the DB is never left half-applied
    //    (e.g. additions saved but removals lost).
    let tx = conn.unchecked_transaction()?;
    for m in upserts {
        upsert_one(&tx, m)?;
    }
    for id in delete_ids {
        tx.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
    }
    tx.execute(
        "DELETE FROM messages WHERE internal_date < ?1",
        params![prune_cutoff_ms],
    )?;
    tx.commit()?;
    Ok(())
}

/// Delete the given message ids (e.g. messages removed from Gmail or archived).
pub fn delete_messages(conn: &Connection, ids: &[String]) -> Result<()> {
    // 🦀 Reuse a single transaction so the whole batch of deletes commits at once.
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        tx.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

/// Replace one message's stored label set. Used by the read/star toggles: the
/// message stays in the cache, only its `label_ids` column changes (so its
/// category and the M6 scoring signals are preserved). A non-existent `id` is a
/// silent no-op (0 rows updated) — callers only toggle messages already cached.
pub fn update_message_labels(conn: &Connection, id: &str, label_ids_csv: &str) -> Result<()> {
    // 🦀 `conn.execute` runs one statement with bound params (`?1`, `?2`), which
    //    SQLite escapes for us — never string-format user values into SQL.
    conn.execute(
        "UPDATE messages SET label_ids = ?1 WHERE id = ?2",
        params![label_ids_csv, id],
    )?;
    Ok(())
}

/// Delete messages older than `cutoff_ms` (Unix ms). Returns how many were removed.
pub fn prune_older_than(conn: &Connection, cutoff_ms: i64) -> Result<usize> {
    // 🦀 `conn.execute` returns the number of rows it changed — here, how many old
    //    messages were pruned.
    let removed = conn.execute(
        "DELETE FROM messages WHERE internal_date < ?1",
        params![cutoff_ms],
    )?;
    Ok(removed)
}

/// The most recent `max` messages, newest first.
pub fn recent_previews(conn: &Connection, max: u32) -> Result<Vec<StoredMessage>> {
    // 🦀 `prepare` parses and compiles the SQL into a reusable `Statement` object.
    //    When a query runs in a tight loop you'd prepare once outside the loop and
    //    reuse it; here we prepare per call for simplicity since this path isn't hot.
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, from_addr, subject, snippet, date_header, internal_date,
                label_ids, to_addr, has_list_unsubscribe, has_list_id, category
         FROM messages
         ORDER BY internal_date DESC
         LIMIT ?1",
    )?;
    // 🦀 `query_map` executes the prepared statement and returns a lazy iterator
    //    of `rusqlite::Result<T>`.  The closure receives a `Row` and maps each
    //    column by zero-based index via `row.get(i)?`, which uses the `FromSql`
    //    trait to decode the SQLite column type into the Rust type on the left.
    let rows = stmt.query_map(params![max], |row| {
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
            // 🦀 rusqlite maps SQLite INTEGER 0/1 to Rust `bool` via the FromSql trait.
            has_list_unsubscribe: row.get(9)?,
            has_list_id: row.get(10)?,
            category: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    // 🦀 We loop instead of calling `.collect()` directly because each element is
    //    `rusqlite::Result<StoredMessage>`.  By writing `r?` inside the loop we
    //    propagate any row-level error immediately, turning the whole function's
    //    return type into our project's `Result<Vec<StoredMessage>>`.
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
        // 🦀 `open_in_memory()` creates a temporary in-process SQLite database that
        //    lives only for the lifetime of this `Connection` value.  Perfect for
        //    unit tests: zero disk I/O, no cleanup required, and each call returns a
        //    fully isolated database — no shared state between test runs.
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }

    #[test]
    fn init_is_idempotent() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        init(&c).unwrap();
        upsert_messages(&c, &[msg("a", 1)]).unwrap();
        assert_eq!(recent_previews(&c, 10).unwrap().len(), 1);
    }

    #[test]
    fn upsert_updates_existing_row() {
        let c = conn();
        upsert_messages(&c, &[msg("a", 1)]).unwrap();
        let mut updated = msg("a", 1);
        updated.subject = "new subject".into();
        upsert_messages(&c, &[updated]).unwrap();
        let rows = recent_previews(&c, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject, "new subject");
    }

    #[test]
    fn recent_previews_orders_newest_first_and_limits() {
        let c = conn();
        upsert_messages(&c, &[msg("old", 100), msg("new", 300), msg("mid", 200)]).unwrap();
        let rows = recent_previews(&c, 2).unwrap();
        assert_eq!(
            rows.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "mid"]
        );
    }

    #[test]
    fn delete_messages_removes_only_given_ids() {
        let c = conn();
        upsert_messages(&c, &[msg("a", 1), msg("b", 2), msg("c", 3)]).unwrap();
        delete_messages(&c, &["b".to_string()]).unwrap();
        let ids: Vec<String> = recent_previews(&c, 10)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec!["c".to_string(), "a".to_string()]); // newest first; "b" gone
    }

    #[test]
    fn prune_older_than_deletes_below_cutoff_and_counts() {
        let c = conn();
        upsert_messages(&c, &[msg("old", 100), msg("new", 300)]).unwrap();
        let removed = prune_older_than(&c, 200).unwrap();
        assert_eq!(removed, 1);
        let ids: Vec<String> = recent_previews(&c, 10)
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
        upsert_messages(&c, &[msg("keep", 500), msg("archive", 600), msg("old", 100)]).unwrap();
        apply_delta(&c, &[msg("newmsg", 700)], &["archive".to_string()], 200).unwrap();
        let mut ids: Vec<String> = recent_previews(&c, 10)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort();
        // "keep"(500) stays, "newmsg"(700) added, "archive" deleted, "old"(100) pruned.
        assert_eq!(ids, vec!["keep".to_string(), "newmsg".to_string()]);
    }

    #[test]
    fn migration_adds_columns_wipes_cache_and_is_idempotent() {
        // 🦀 Simulate an OLD-schema DB: hand-create the pre-M6 7-column table + a row,
        //    plus a sync_state baseline, BEFORE init() runs.
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

        // New column exists; old cache wiped; history baseline cleared → next sync is full.
        assert!(column_exists(&c, "messages", "category").unwrap());
        assert_eq!(recent_previews(&c, 10).unwrap().len(), 0);
        assert_eq!(
            get_sync_state(&c, "primary").unwrap().unwrap().last_history_id,
            None
        );

        // Idempotent: insert a fresh row, run init() AGAIN, and confirm it was NOT
        // wiped a second time (needs_migration must be false now that category exists).
        upsert_messages(&c, &[msg("fresh", 5)]).unwrap();
        init(&c).unwrap();
        assert!(column_exists(&c, "messages", "category").unwrap());
        let rows = recent_previews(&c, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "fresh");
    }

    #[test]
    fn update_message_labels_changes_only_labels() {
        let c = conn();
        let mut m = msg("a", 1);
        m.category = "people".into();
        m.label_ids = "INBOX,UNREAD".into();
        upsert_messages(&c, &[m]).unwrap();

        update_message_labels(&c, "a", "INBOX").unwrap();

        let rows = recent_previews(&c, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label_ids, "INBOX"); // UNREAD removed
        assert_eq!(rows[0].category, "people"); // category untouched
    }

    #[test]
    fn upsert_and_read_preserve_category_and_signals() {
        let c = conn();
        let mut m = msg("a", 1);
        m.category = "newsletters".into();
        m.has_list_unsubscribe = true;
        m.label_ids = "INBOX,CATEGORY_PROMOTIONS".into();
        upsert_messages(&c, &[m]).unwrap();
        let rows = recent_previews(&c, 10).unwrap();
        assert_eq!(rows[0].category, "newsletters");
        assert!(rows[0].has_list_unsubscribe);
        assert_eq!(rows[0].label_ids, "INBOX,CATEGORY_PROMOTIONS");
    }
}
