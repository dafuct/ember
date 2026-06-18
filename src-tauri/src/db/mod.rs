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
            internal_date INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_messages_internal_date
            ON messages(internal_date DESC);
        CREATE TABLE IF NOT EXISTS sync_state (
            account         TEXT PRIMARY KEY,
            last_history_id INTEGER,
            last_synced_at  INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    Ok(())
}

/// Insert each message, or update it in place if its id already exists.
pub fn upsert_messages(conn: &Connection, messages: &[StoredMessage]) -> Result<()> {
    for m in messages {
        // 🦀 `execute` runs a single SQL statement.  The `?1, ?2, ...` positional
        //    placeholders are **parameterized queries**: rusqlite binds values
        //    separately from the SQL text, so user-supplied data can never be
        //    interpreted as SQL.  This makes SQL injection impossible.
        //    Never use `format!()` to build SQL strings — that bypasses all protection.
        //
        // 🦀 SQLite UPSERT pattern: `ON CONFLICT(id) DO UPDATE SET col = excluded.col`
        //    means "if a row with this PRIMARY KEY already exists, update the listed
        //    columns using the values from the row we just tried to insert".
        //    `excluded` is a special alias for that incoming (conflicting) row.
        conn.execute(
            "INSERT INTO messages
                (id, thread_id, from_addr, subject, snippet, date_header, internal_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                thread_id = excluded.thread_id,
                from_addr = excluded.from_addr,
                subject = excluded.subject,
                snippet = excluded.snippet,
                date_header = excluded.date_header,
                internal_date = excluded.internal_date",
            // 🦀 `params![]` is a macro that builds a heterogeneous slice of
            //    `&dyn ToSql` trait objects, matched positionally to the `?1..?N`
            //    placeholders in the SQL string above.
            params![
                m.id, m.thread_id, m.from_addr, m.subject, m.snippet, m.date_header, m.internal_date
            ],
        )?;
    }
    Ok(())
}

/// The most recent `max` messages, newest first.
pub fn recent_previews(conn: &Connection, max: u32) -> Result<Vec<StoredMessage>> {
    // 🦀 `prepare` parses and compiles the SQL into a reusable `Statement` object.
    //    When a query runs in a tight loop you'd prepare once outside the loop and
    //    reuse it; here we prepare per call for simplicity since this path isn't hot.
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, from_addr, subject, snippet, date_header, internal_date
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
    fn sync_state_round_trips_and_missing_is_none() {
        let c = conn();
        assert_eq!(get_sync_state(&c, "primary").unwrap(), None);
        set_sync_state(&c, "primary", Some(42), 1718700000).unwrap();
        let s = get_sync_state(&c, "primary").unwrap().unwrap();
        assert_eq!(s.last_history_id, Some(42));
        assert_eq!(s.last_synced_at, 1718700000);
    }
}
