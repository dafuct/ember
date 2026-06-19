# Ember — Milestone 6: Smart-inbox scorer (lean v1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Classify every synced INBOX message locally and deterministically into one of three streams — People / Notifications / Newsletters — and surface them as a Smart Inbox (All grouped view + per-stream tabs) in the UI.

**Architecture:** A new pure Rust `scorer` module (ordered-precedence rules, table-tested) classifies a message from its Gmail `CATEGORY_*` labels plus `List-Unsubscribe` / `List-Id` / `To` headers and sender shape. The Gmail fetch is widened to return those signals; the SQLite `messages` table gains columns for the raw signals + the computed `category` (additive idempotent migration that wipes the local cache once and forces a full resync). Classification runs at sync time and the category is persisted; the React UI filters/groups client-side over the already-fetched previews.

**Tech Stack:** Rust (rusqlite, reqwest, serde, wiremock for tests), Tauri 2, React 19 + TypeScript + Vite, lucide-react icons.

**Learning mode (IMPORTANT — applies to every implementer):** The repo owner is learning Rust. All Rust code MUST include concise `// 🦀` teaching comments explaining the *language* concept (ownership/borrowing, `Result`/`Option`/`?`, `match`, enums, traits/`impl`, lifetimes `'a`, slices `&[T]`, closures, derive macros), not just intent. After each task, give a short plain-English recap of the Rust concepts it introduced. TypeScript/React gets normal comments — the owner knows JS/React.

**Design source:** `wiki/concepts/smart-inbox-scorer.md` and `raw/ember-m6-smart-inbox-scorer-design.md` (the approved design). These are gitignored/local.

---

## Milestone context

M1–M5 are merged to `main`. The app reads mail (30-day INBOX sync via history deltas; full message bodies) but does not classify, mutate, or send. M6 adds classification only — **no mutations to the live mailbox**. Re-sequenced roadmap after M6: M7 actions → M8 compose → M9 settings/onboarding → M10 calendar.

**Scope (lean v1):** signals are Gmail `CATEGORY_*` labels + `List-Unsubscribe`/`List-Id`/`To` headers + automated-sender detection only. The Sent-mailbox "people you reply to" affinity signal is **deferred**. Also deferred: re-categorizing on post-insert label changes, tunable-weight settings UI, multi-account, and frontend unit tests (Vitest is not configured — consistent with M4/M5; see "Deferred follow-ups").

---

## File structure

**Backend (Rust, `src-tauri/`):**
- `src/scorer.rs` — **NEW.** Pure classifier: `Category` enum, `MessageFeatures<'a>`, `classify()`, `is_automated_sender()`. Owns all classification logic + table tests.
- `src/lib.rs` — register `pub mod scorer;`.
- `src/gmail/types.rs` — add `label_ids` to `RawMessage`; add signal + `category` fields to `MessagePreview`.
- `src/gmail/mod.rs` — widen `get_message_preview` URL + extract the new signals.
- `src/db/mod.rs` — `StoredMessage` new fields; full-schema `CREATE TABLE`; idempotent column migration + one-time cache wipe; widened UPSERT/SELECT; migration + round-trip tests.
- `src/commands.rs` — `to_rows` classifies at sync; `fetch_inbox_preview` returns `category`; a unit test for `to_rows`.
- `tests/gmail_test.rs` — add a wiremock test for label/header parsing.

**Frontend (`src/`):**
- `lib/api.ts` — add `category` to the `MessagePreview` interface.
- `lib/streams.ts` — **NEW.** Pure `Stream` type + `filterByStream` / `groupByStream` helpers + labels.
- `components/Header.tsx` — Smart Inbox nav group (All/People/Notifications/Newsletters) replacing the single Inbox tab.
- `components/MessageList.tsx` — grouped (All) vs filtered (single stream) rendering.
- `components/MessageItem.tsx` — per-row category dot.
- `App.tsx` — `stream` state, wired to Header + MessageList.
- `styles/app.css` — nav-group, group-header, and category-dot styles.

---

## Setup: create the milestone branch

- [ ] **Step 0: Branch off `main`**

Run:
```bash
git checkout main && git pull --ff-only 2>/dev/null; git checkout -b m6-smart-inbox-scorer
git status
```
Expected: on a new branch `m6-smart-inbox-scorer`, clean tree (the gitignored `wiki/`, `raw/`, `CLAUDE.md` do not appear).

---

## Task 1: The `scorer` module (pure, table-tested)

Build the classifier first, in isolation, with TDD. It has no I/O — just data in, `Category` out.

**Files:**
- Create: `src-tauri/src/scorer.rs`
- Modify: `src-tauri/src/lib.rs` (register the module)

- [ ] **Step 1: Register the module so the test can compile**

In `src-tauri/src/lib.rs`, immediately after the `mod html;` line, add:
```rust
// 🦀 `pub mod scorer;` wires in the pure smart-inbox classifier (no I/O, fully
//    unit-testable). `pub` so integration tests / future callers can reach it.
pub mod scorer;
```

- [ ] **Step 2: Write the failing test file**

Create `src-tauri/src/scorer.rs` with ONLY the test module first (the types/functions it references don't exist yet, so it won't compile — that's the failing state):
```rust
//! Smart-inbox scorer — classifies one message into People / Notifications / Newsletters.
//! Pure: no network, no DB. The single source of truth for Ember's stream classification.

#[cfg(test)]
mod tests {
    use super::*;

    // 🦀 Test helper: builds a `MessageFeatures` borrowing the passed-in slices/strs.
    //    The lifetime `'a` ties the returned struct's borrows to the caller's data.
    fn feat<'a>(labels: &'a [String], from: &'a str, lu: bool, li: bool) -> MessageFeatures<'a> {
        MessageFeatures { label_ids: labels, from_addr: from, has_list_unsubscribe: lu, has_list_id: li }
    }

    #[test]
    fn classifies_streams_by_precedence() {
        // (labels, from, has_list_unsubscribe, has_list_id, expected)
        let cases: Vec<(Vec<&str>, &str, bool, bool, Category)> = vec![
            (vec!["CATEGORY_PROMOTIONS"], "deals@store.com", false, false, Category::Newsletters),
            (vec!["CATEGORY_FORUMS"], "list@group.com", false, false, Category::Newsletters),
            (vec![], "news@brand.com", true, false, Category::Newsletters), // List-Unsubscribe
            (vec!["CATEGORY_UPDATES"], "updates@app.com", false, false, Category::Notifications),
            (vec!["CATEGORY_SOCIAL"], "social@app.com", false, false, Category::Notifications),
            (vec![], "no-reply@service.com", false, false, Category::Notifications), // automated
            (vec![], "notifications@github.com", false, false, Category::Notifications),
            (vec![], "team@startup.com", false, true, Category::Notifications), // List-Id
            (vec!["CATEGORY_PERSONAL"], "maya@studio.co", false, false, Category::People),
            (vec![], "maya@studio.co", false, false, Category::People), // no labels → People
            // precedence: Newsletters rule beats Notifications rule
            (vec!["CATEGORY_PROMOTIONS", "CATEGORY_UPDATES"], "x@y.com", false, false, Category::Newsletters),
            // precedence: List-Unsubscribe (Newsletters) beats List-Id (Notifications)
            (vec![], "x@y.com", true, true, Category::Newsletters),
        ];
        for (labels, from, lu, li, expected) in cases {
            let owned: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
            let got = classify(&feat(&owned, from, lu, li));
            assert_eq!(got, expected, "from={from} labels={labels:?} lu={lu} li={li}");
        }
    }

    #[test]
    fn category_as_str_gives_storage_keys() {
        assert_eq!(Category::People.as_str(), "people");
        assert_eq!(Category::Notifications.as_str(), "notifications");
        assert_eq!(Category::Newsletters.as_str(), "newsletters");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails (won't compile)**

Run: `cd src-tauri && cargo test --lib scorer`
Expected: FAIL — compile errors `cannot find type \`Category\``, `cannot find function \`classify\``.

- [ ] **Step 4: Implement the classifier above the test module**

Insert this between the `//!` doc comment and the `#[cfg(test)]` line in `src-tauri/src/scorer.rs`:
```rust
// 🦀 An `enum` is a type that is exactly one of a fixed set of variants. `derive`
//    auto-implements traits: `Copy` makes it cheap to pass by value (no move),
//    `PartialEq`/`Eq` enable `==` and `assert_eq!`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    People,
    Notifications,
    Newsletters,
}

impl Category {
    // 🦀 `self` by value is fine because `Category` is `Copy`. Returns a
    //    `&'static str` — a string slice baked into the binary, valid forever.
    //    These are the keys persisted in the DB and sent to the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::People => "people",
            Category::Notifications => "notifications",
            Category::Newsletters => "newsletters",
        }
    }
}

// 🦀 The inputs the classifier reads. It borrows rather than owns: `&'a [String]`
//    is a slice (a view into someone else's Vec) and `&'a str` a string slice.
//    The lifetime `'a` says "these borrows must outlive this struct" — no copying.
pub struct MessageFeatures<'a> {
    pub label_ids: &'a [String],
    pub from_addr: &'a str,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
}

// 🦀 Heuristic: does the sender address look like an automated/no-reply mailbox?
//    `to_ascii_lowercase` copies once so matching is case-insensitive. `.contains`
//    is a substring check. "notification" also matches "notifications@".
fn is_automated_sender(from: &str) -> bool {
    let f = from.to_ascii_lowercase();
    const MARKERS: [&str; 6] = [
        "no-reply", "noreply", "no_reply", "donotreply", "do-not-reply", "mailer-daemon",
    ];
    MARKERS.iter().any(|m| f.contains(m)) || f.contains("notification")
}

/// Classify a message into exactly one stream. Ordered precedence: the first rule
/// that matches wins, so Newsletters outranks Notifications outranks People (default).
pub fn classify(f: &MessageFeatures) -> Category {
    // 🦀 A closure capturing `f` by reference; `has("X")` asks "is label X present?".
    let has = |label: &str| f.label_ids.iter().any(|l| l == label);

    if has("CATEGORY_PROMOTIONS") || has("CATEGORY_FORUMS") || f.has_list_unsubscribe {
        return Category::Newsletters;
    }
    if has("CATEGORY_UPDATES")
        || has("CATEGORY_SOCIAL")
        || is_automated_sender(f.from_addr)
        || f.has_list_id
    {
        return Category::Notifications;
    }
    Category::People
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib scorer`
Expected: PASS — `classifies_streams_by_precedence` and `category_as_str_gives_storage_keys` both ok.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/scorer.rs src-tauri/src/lib.rs
git commit -m "feat(scorer): pure People/Notifications/Newsletters classifier with table tests"
```

**Rust recap to give:** `enum` + `match`, `derive` (incl. `Copy`), `impl` methods, lifetimes (`'a`) and why borrowing slices/`&str` avoids copies, closures capturing by reference, `&'static str`.

---

## Task 2: DB schema — signal columns, idempotent migration, persistence

Add the storage for raw signals + the computed `category`, with a migration that is safe to run on every startup and self-heals old databases.

**Files:**
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/db/mod.rs`, inside the existing `#[cfg(test)] mod tests { ... }`, add two tests at the end (before the closing `}`):
```rust
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

        // Idempotent: a second init() does not error and keeps the column.
        init(&c).unwrap();
        assert!(column_exists(&c, "messages", "category").unwrap());
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
```

- [ ] **Step 2: Run the tests to verify they fail (won't compile)**

Run: `cd src-tauri && cargo test --lib db`
Expected: FAIL — `no field \`category\` on type \`StoredMessage\``, `cannot find function \`column_exists\``.

- [ ] **Step 3: Extend `StoredMessage`**

In `src-tauri/src/db/mod.rs`, replace the `StoredMessage` struct with:
```rust
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
```

- [ ] **Step 4: Full-schema `CREATE TABLE` + migration in `init`**

In `init`, replace the `conn.execute_batch( ... )?;` block with the full-schema version (note: `messages` now lists all 12 columns; `idx_messages_category` is created AFTER the migration since the column may not exist yet on old DBs):
```rust
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
    //    installs need their columns added here. We capture `needs_migration` BEFORE
    //    adding anything: the absence of `category` is the sentinel for "old schema".
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
        conn.execute("DELETE FROM messages", [])?;
        conn.execute("UPDATE sync_state SET last_history_id = NULL", [])?;
    }
    Ok(())
```
(Leave the `journal_mode`/`busy_timeout` pragma lines above this untouched; only the `execute_batch(...)?;` call and the `Ok(())` region change.)

- [ ] **Step 5: Add the migration helper functions**

In `src-tauri/src/db/mod.rs`, add these two free functions just below `init` (above `UPSERT_SQL`):
```rust
// 🦀 Does `table` have a column named `col`? PRAGMA statements can't take bound
//    params, but `table` here is always an internal constant ("messages"), never
//    user input, so formatting it in is injection-safe. `query_map` yields each
//    column's name (index 1 of PRAGMA table_info), and `?` propagates row errors.
fn column_exists(conn: &Connection, table: &str, col: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    while let Some(name) = rows.next() {
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
```

- [ ] **Step 6: Widen the UPSERT and the read query**

Replace `UPSERT_SQL` with:
```rust
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
```
Replace the `params![...]` in `upsert_one` with:
```rust
        params![
            m.id, m.thread_id, m.from_addr, m.subject, m.snippet, m.date_header,
            m.internal_date, m.label_ids, m.to_addr, m.has_list_unsubscribe,
            m.has_list_id, m.category
        ],
```
In `recent_previews`, replace the SQL string and the row-mapping closure:
```rust
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, from_addr, subject, snippet, date_header, internal_date,
                label_ids, to_addr, has_list_unsubscribe, has_list_id, category
         FROM messages
         ORDER BY internal_date DESC
         LIMIT ?1",
    )?;
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
```

- [ ] **Step 7: Update the `msg` test helper**

In the `#[cfg(test)] mod tests`, replace the `msg` helper so it builds the new fields:
```rust
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
```

- [ ] **Step 8: Run the DB tests to verify they pass**

Run: `cd src-tauri && cargo test --lib db`
Expected: PASS — all existing db tests plus `migration_adds_columns_wipes_cache_and_is_idempotent` and `upsert_and_read_preserve_category_and_signals`.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(db): smart-inbox columns + idempotent migration with one-time cache wipe"
```

**Rust recap to give:** `PRAGMA` introspection, why PRAGMA can't bind params (and why it's safe here), `ALTER TABLE ADD COLUMN` idempotency, rusqlite `bool` ↔ INTEGER mapping, `while let Some(..)` over a fallible iterator.

---

## Task 3: Gmail fetch — return labels + list headers

Widen the metadata fetch so each preview carries the signals the scorer needs.

**Files:**
- Modify: `src-tauri/src/gmail/types.rs`
- Modify: `src-tauri/src/gmail/mod.rs`
- Modify: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing integration test**

In `src-tauri/tests/gmail_test.rs`, add a new test (after `get_message_preview_extracts_headers`):
```rust
#[tokio::test(flavor = "multi_thread")]
async fn get_message_preview_extracts_labels_and_list_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/n1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "n1",
            "labelIds": ["INBOX", "CATEGORY_PROMOTIONS"],
            "snippet": "Big sale",
            "payload": { "headers": [
                {"name": "From", "value": "Store <deals@store.com>"},
                {"name": "To", "value": "you@example.com"},
                {"name": "List-Unsubscribe", "value": "<mailto:unsub@store.com>"}
            ]}
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.get_message_preview("n1").await.unwrap();
    assert_eq!(m.label_ids, vec!["INBOX".to_string(), "CATEGORY_PROMOTIONS".to_string()]);
    assert_eq!(m.to_addr, "you@example.com");
    assert!(m.has_list_unsubscribe);
    assert!(!m.has_list_id);
}
```

- [ ] **Step 2: Run it to verify it fails (won't compile)**

Run: `cd src-tauri && cargo test --test gmail_test get_message_preview_extracts_labels_and_list_headers`
Expected: FAIL — `no field \`label_ids\` on type \`MessagePreview\``.

- [ ] **Step 3: Add `label_ids` to `RawMessage`**

In `src-tauri/src/gmail/types.rs`, inside `struct RawMessage`, add this field (after `thread_id`):
```rust
    // 🦀 Gmail returns the message's labels (incl. CATEGORY_* tabs) at the top level
    //    in format=metadata. `default` makes it an empty Vec when the key is absent.
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
```

- [ ] **Step 4: Add the signal + category fields to `MessagePreview`**

In `src-tauri/src/gmail/types.rs`, replace `struct MessagePreview` with:
```rust
/// What the UI consumes for the inbox preview. Also carries the M6 scoring signals;
/// the frontend only reads `category` (the rest are persisted for re-scoring).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MessagePreview {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub internal_date: i64,
    pub label_ids: Vec<String>,
    pub to_addr: String,
    pub has_list_unsubscribe: bool,
    pub has_list_id: bool,
    /// Filled by the scorer at sync time (empty on the raw Gmail-fetch path).
    pub category: String,
}
```

- [ ] **Step 5: Widen the fetch URL + extraction**

In `src-tauri/src/gmail/mod.rs`, in `get_message_preview`, replace the `let url = format!(...)` with:
```rust
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}?format=metadata\
             &metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date\
             &metadataHeaders=To&metadataHeaders=List-Id&metadataHeaders=List-Unsubscribe",
            self.base_url, id
        );
```
Then replace the body from the `let header = |name: &str| { ... };` closure through the `Ok(MessagePreview { ... })` with:
```rust
        let header = |name: &str| {
            raw.payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap_or_default()
        };
        // 🦀 Pull every header-derived value out FIRST, while the `header` closure's
        //    borrow of `raw.payload` is live. After the last call the borrow ends
        //    (non-lexical lifetimes), so we can then MOVE owned fields out of `raw`
        //    (no clones needed) when building the struct below.
        let from = header("From");
        let subject = header("Subject");
        let date = header("Date");
        let to_addr = header("To");
        let has_list_unsubscribe = !header("List-Unsubscribe").is_empty();
        let has_list_id = !header("List-Id").is_empty();
        let internal_date = raw.internal_date.parse::<i64>().unwrap_or(0);
        Ok(MessagePreview {
            id: raw.id,
            thread_id: raw.thread_id,
            from,
            subject,
            date,
            snippet: raw.snippet,
            internal_date,
            label_ids: raw.label_ids,
            to_addr,
            has_list_unsubscribe,
            has_list_id,
            category: String::new(), // scored at sync time, not here
        })
```

- [ ] **Step 6: Run the gmail tests to verify they pass**

Run: `cd src-tauri && cargo test --test gmail_test`
Expected: PASS — existing gmail tests plus the new `get_message_preview_extracts_labels_and_list_headers`.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): fetch labelIds + To/List-Id/List-Unsubscribe for scoring"
```

**Rust recap to give:** `#[serde(rename/default)]` for JSON shape, non-lexical lifetimes letting a borrow (the closure) end so owned fields can be moved, moving vs cloning out of a struct.

---

## Task 4: Sync integration — classify at sync, return category on read

Wire the scorer into the sync write-path and surface `category` on the read-path.

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Write the failing unit test**

In `src-tauri/src/commands.rs`, add a test module at the very end of the file:
```rust
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd src-tauri && cargo test --lib commands`
Expected: FAIL — `to_rows` still produces a `StoredMessage` without `category`/`label_ids` set the new way (compile error: missing fields in `to_rows`'s struct literal).

- [ ] **Step 3: Import the scorer and rewrite `to_rows`**

In `src-tauri/src/commands.rs`, add to the `use` block near the top:
```rust
use crate::scorer;
```
Replace the whole `to_rows` function with:
```rust
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
```

- [ ] **Step 4: Return `category` (and signals) from the read-path**

In `src-tauri/src/commands.rs`, in `fetch_inbox_preview`, replace the `.map(|m| MessagePreview { ... })` closure with:
```rust
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
```

- [ ] **Step 5: Run the test + the full backend suite**

Run: `cd src-tauri && cargo test`
Expected: PASS — all tests across lib + integration (`to_rows_classifies_and_joins_labels`, scorer, db, gmail).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(sync): classify messages at sync time and expose category to the UI"
```

**Rust recap to give:** borrow-then-move ordering inside `.map`, building one owned struct from another, `split`/`filter`/`map`/`collect` iterator chain, why `to_rows` is now the single place classification happens.

---

## Task 5: Frontend data layer — `category` type + stream helpers

Pure TypeScript; no UI yet. Sets up the contract and the grouping/filtering logic in isolation.

**Files:**
- Modify: `src/lib/api.ts`
- Create: `src/lib/streams.ts`

- [ ] **Step 1: Add `category` to the `MessagePreview` interface**

In `src/lib/api.ts`, add to the `MessagePreview` interface (after `internal_date`):
```ts
  /** Smart-inbox stream from the backend scorer: "people" | "notifications" | "newsletters". */
  category: string;
```

- [ ] **Step 2: Create the stream helpers**

Create `src/lib/streams.ts`:
```ts
import type { MessagePreview } from "./api";

// "all" is the grouped view; the other three are the scorer's category keys.
export type Stream = "all" | "people" | "notifications" | "newsletters";

export const STREAMS: { key: Stream; label: string }[] = [
  { key: "all", label: "All" },
  { key: "people", label: "People" },
  { key: "notifications", label: "Notifications" },
  { key: "newsletters", label: "Newsletters" },
];

// Display label for a category key (used for section headers and the dot title).
export const CATEGORY_LABEL: Record<string, string> = {
  people: "People",
  notifications: "Notifications",
  newsletters: "Newsletters",
};

// Order the grouped "All" view shows its sections in.
const STREAM_ORDER = ["people", "notifications", "newsletters"] as const;

export function filterByStream(
  msgs: MessagePreview[],
  stream: Stream,
): MessagePreview[] {
  if (stream === "all") return msgs;
  return msgs.filter((m) => m.category === stream);
}

export interface StreamGroup {
  category: string;
  label: string;
  messages: MessagePreview[];
}

// Group messages into the three streams, dropping empty groups.
export function groupByStream(msgs: MessagePreview[]): StreamGroup[] {
  return STREAM_ORDER.map((cat) => ({
    category: cat,
    label: CATEGORY_LABEL[cat],
    messages: msgs.filter((m) => m.category === cat),
  })).filter((g) => g.messages.length > 0);
}
```

- [ ] **Step 3: Typecheck**

Run: `npm run build`
Expected: `tsc` passes (no type errors) and Vite builds. (If `tsc` flags unused exports, that's fine — they're consumed in Task 6; this step only verifies the new code typechecks.)

- [ ] **Step 4: Commit**

```bash
git add src/lib/api.ts src/lib/streams.ts
git commit -m "feat(ui): category field + pure stream filter/group helpers"
```

---

## Task 6: Frontend UI — Smart Inbox nav, grouped list, category dots

Wire the stream selector into the header, group/filter the list, and add a per-row category dot. Verified by running the app (no JS test runner; see Deferred follow-ups).

**Files:**
- Modify: `src/components/Header.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/MessageList.tsx`
- Modify: `src/components/MessageItem.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Smart Inbox nav in `Header.tsx`**

Replace the imports block and the component in `src/components/Header.tsx`:
```tsx
import {
  Flame,
  RefreshCw,
  Sun,
  Moon,
  Inbox,
  Users,
  Bell,
  Newspaper,
  Star,
  Send,
  Archive,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useTheme, type Theme } from "../theme";
import { STREAMS, type Stream } from "../lib/streams";

const THEME_ICON: Record<Theme, LucideIcon> = { light: Sun, dark: Moon };

const STREAM_ICON: Record<Stream, LucideIcon> = {
  all: Inbox,
  people: Users,
  notifications: Bell,
  newsletters: Newspaper,
};

const FOLDERS: { icon: LucideIcon; label: string }[] = [
  { icon: Star, label: "Starred" },
  { icon: Send, label: "Sent" },
  { icon: Archive, label: "Archive" },
  { icon: Trash2, label: "Trash" },
];

export function Header({
  busy,
  onSync,
  status,
  account = null,
  stream = "all",
  onSelectStream,
}: {
  busy: boolean;
  onSync?: () => void;
  status: string | null;
  account?: string | null;
  stream?: Stream;
  onSelectStream?: (s: Stream) => void;
}) {
  const { theme, cycleTheme } = useTheme();
  const ThemeIcon = THEME_ICON[theme];
  return (
    <header className="app-header">
      <span className="brand">
        <Flame size={20} className="brand-icon" /> Ember
      </span>
      {account && (
        <nav className="header-nav">
          {STREAMS.map((s) => {
            const Icon = STREAM_ICON[s.key];
            return (
              <button
                key={s.key}
                className={
                  s.key === stream
                    ? "header-nav-item active"
                    : "header-nav-item"
                }
                title={s.label}
                onClick={() => onSelectStream?.(s.key)}
              >
                <Icon size={15} /> <span className="nav-label">{s.label}</span>
              </button>
            );
          })}
          {FOLDERS.map((f) => {
            const Icon = f.icon;
            return (
              <button
                key={f.label}
                className="header-nav-item"
                title={f.label}
                disabled
              >
                <Icon size={15} /> <span className="nav-label">{f.label}</span>
              </button>
            );
          })}
        </nav>
      )}
      <span className="spacer" />
      {status && <span className="status-text">{status}</span>}
      {onSync && (
        <button className="btn btn-accent" onClick={onSync} disabled={busy}>
          <RefreshCw size={15} className={busy ? "spin" : undefined} />
          {busy ? "Syncing…" : "Sync"}
        </button>
      )}
      {account && (
        <div className="header-account" title={account}>
          <div className="avatar">{account.charAt(0).toUpperCase()}</div>
          <span className="account-email">{account}</span>
        </div>
      )}
      <button
        className="icon-btn"
        onClick={cycleTheme}
        aria-label={`Theme: ${theme}. Click to switch.`}
      >
        <ThemeIcon size={16} />
      </button>
    </header>
  );
}
```

- [ ] **Step 2: `stream` state in `App.tsx`**

In `src/App.tsx`: add the import and state, and pass props to `Header` and `MessageList`.

Add to the imports:
```tsx
import type { Stream } from "./lib/streams";
```
Add to the state (after the `selectedId` state line):
```tsx
  const [stream, setStream] = useState<Stream>("all");
```
Replace the authenticated-view `<Header ... />` with:
```tsx
      <Header
        busy={busy}
        onSync={handleSync}
        status={status}
        account={account}
        stream={stream}
        onSelectStream={setStream}
      />
```
Replace the `<MessageList ... />` inside `SplitView` `left={...}` with:
```tsx
          <MessageList
            messages={messages}
            stream={stream}
            selectedId={selectedId}
            onSelect={setSelectedId}
          />
```

- [ ] **Step 3: Group/filter in `MessageList.tsx`**

Replace `src/components/MessageList.tsx` entirely:
```tsx
import type { MessagePreview } from "../lib/api";
import { MessageItem } from "./MessageItem";
import {
  STREAMS,
  filterByStream,
  groupByStream,
  type Stream,
} from "../lib/streams";

export function MessageList({
  messages,
  stream,
  selectedId,
  onSelect,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  const visible = filterByStream(messages, stream);
  const title = STREAMS.find((s) => s.key === stream)?.label ?? "Inbox";

  return (
    <section className="msglist">
      <div className="msglist-header">
        <span className="msglist-title">{title}</span>
        <span className="msglist-count">{visible.length} messages</span>
      </div>
      <div className="msglist-scroll">
        {visible.length === 0 ? (
          <div className="empty">No messages here — hit Sync.</div>
        ) : stream === "all" ? (
          // Grouped view: a labeled section per non-empty stream.
          groupByStream(messages).map((group) => (
            <div key={group.category} className="msglist-group">
              <div className="msglist-group-header">
                <span>{group.label}</span>
                <span className="msglist-group-count">
                  {group.messages.length}
                </span>
              </div>
              {group.messages.map((m) => (
                <MessageItem
                  key={m.id}
                  msg={m}
                  selected={m.id === selectedId}
                  onSelect={onSelect}
                />
              ))}
            </div>
          ))
        ) : (
          visible.map((m) => (
            <MessageItem
              key={m.id}
              msg={m}
              selected={m.id === selectedId}
              onSelect={onSelect}
            />
          ))
        )}
      </div>
    </section>
  );
}
```

- [ ] **Step 4: Category dot in `MessageItem.tsx`**

In `src/components/MessageItem.tsx`, replace the `msg-top` block so a colored dot precedes the sender:
```tsx
      <div className="msg-top">
        <span className="msg-sender">
          {msg.category && (
            <span
              className={`cat-dot cat-${msg.category}`}
              title={msg.category}
              aria-hidden
            />
          )}
          {msg.from || "(unknown sender)"}
        </span>
        <span className="msg-time">{relativeTime(msg.internal_date)}</span>
      </div>
```

- [ ] **Step 5: Styles in `styles/app.css`**

Append to `src/styles/app.css`:
```css
/* Smart Inbox — grouped list sections */
.msglist-group-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 12px;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-lo, #888);
  background: var(--bg-1, rgba(127, 127, 127, 0.06));
  position: sticky;
  top: 0;
  z-index: 1;
}
.msglist-group-count {
  opacity: 0.7;
}

/* Per-row category dot */
.cat-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  margin-right: 7px;
  vertical-align: middle;
  flex: none;
}
.cat-people {
  background: #ff7a9c;
}
.cat-notifications {
  background: #ffc25c;
}
.cat-newsletters {
  background: #74d0bd;
}
```
(If a `.header-nav-item.active` rule does not already exist, the existing `.active` styling from M4 applies; no change needed.)

- [ ] **Step 6: Typecheck + build**

Run: `npm run build`
Expected: `tsc` + Vite build succeed with no errors.

- [ ] **Step 7: Manual verification — run the app**

Run: `npm run tauri dev`
Verify, with a connected account and after a Sync:
1. The header shows **All · People · Notifications · Newsletters** plus the disabled folder tabs.
2. **All** shows up to three labeled sections; each message row has a colored dot matching its section.
3. Clicking **People / Notifications / Newsletters** filters the list to that stream; the title and count update.
4. An empty stream shows the empty-state line.
5. Because of the one-time migration cache wipe, the first Sync after upgrading does a full 30-day resync (may take a few seconds) and messages reappear classified.

- [ ] **Step 8: Commit**

```bash
git add src/components/Header.tsx src/App.tsx src/components/MessageList.tsx src/components/MessageItem.tsx src/styles/app.css
git commit -m "feat(ui): Smart Inbox streams — nav tabs, grouped All view, category dots"
```

---

## Task 7: Final verification & wrap-up

- [ ] **Step 1: Full backend test suite**

Run: `cd src-tauri && cargo test`
Expected: PASS — all tests (scorer, db incl. migration, gmail incl. label/header parse, commands `to_rows`).

- [ ] **Step 2: Frontend build**

Run: `npm run build`
Expected: clean `tsc` + Vite build.

- [ ] **Step 3: Update the local wiki + memory**

Append one line to `wiki/log.md`:
```
## [2026-06-18] update | M6 smart-inbox scorer implemented on branch m6-smart-inbox-scorer
```
In `wiki/concepts/smart-inbox-scorer.md` frontmatter, bump `updated:` to the implementation date. (These files are gitignored — no commit.) Then update the project memory note for M6 from "designed" to "implemented (branch `m6-smart-inbox-scorer`, N tests)".

- [ ] **Step 4: Integrate the branch**

Use superpowers:finishing-a-development-branch to choose merge / PR. The prior milestones squash-merged to `main` with a `Merge M{n}: …` message after review. Match that convention.

---

## Deferred follow-ups (out of scope for M6)

- **Sent-mailbox affinity** for the People stream (needs Sent sync) — the lean-v1 cut.
- **Re-categorization on label change** after a message is already stored (history `labelAdded`/`labelRemoved` currently only tracks INBOX membership).
- **Frontend unit tests** — Vitest + Testing Library are not configured (M4/M5 also shipped without them). When added: a test that `groupByStream`/`filterByStream` partition correctly and that `MessageList` renders sections; mock `@tauri-apps/api/core`.
- **Tunable rule weights / settings UI** — belongs to M9 (settings).
- A `rescore_all` command that reclassifies stored rows from their persisted signals without a Gmail refetch (cheap, enabled by storing the raw signals).

---

## Self-review notes

- **Spec coverage:** three streams + ordered rules (Task 1), signal fetch (Task 3), persistence + migration (Task 2), classify-at-sync (Task 4), All-grouped + per-stream UI (Tasks 5–6) — all covered.
- **Type consistency:** `Category::as_str()` keys (`"people"`/`"notifications"`/`"newsletters"`) match the TS `Stream` keys and the CSS `cat-*` classes and the `CATEGORY_LABEL` map. `MessageFeatures` fields match how `to_rows` (Task 4) and the scorer tests (Task 1) construct them. `StoredMessage`'s 12 fields match the UPSERT params, the SELECT mapping, and the `msg()`/`preview()` test helpers.
- **No live-mailbox mutations** anywhere — classification is read-only/derived, per the milestone's risk goal.
