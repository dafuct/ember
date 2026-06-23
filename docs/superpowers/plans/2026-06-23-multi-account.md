# Multi-Account Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user connect several Google accounts, switch the active one (Gmail + Calendar + notes all follow), and receive background new-mail notifications for every connected account.

**Architecture:** Approach A from the spec — tokens keyed by email in the Keychain; an accounts index + active-account pointer in the `settings` table; one shared SQLite cache with an `account` column on `messages`/`snoozed`/`meeting_notes` (UI filters to the active account); the existing `ensure_access_token()` chokepoint resolves the active account so all network commands switch automatically; a background loop syncs all accounts and notifies per account.

**Tech Stack:** Rust (Tauri 2, rusqlite, keyring, oauth2, serde_json), React 19 + TypeScript (Vite). Backend tests: `cargo test`. Frontend: no unit runner — verify with `npm run build` (tsc) + the browser "maket" via the preview tools.

**Spec:** `docs/superpowers/specs/2026-06-23-multi-account-design.md`

**Test commands (use throughout):**
- Rust: `cargo test --manifest-path src-tauri/Cargo.toml <filter>`
- Types: `npm run build` (runs `tsc` then `vite build`)
- Maket: `preview_start` `ember-maket` → `preview_screenshot` / `preview_snapshot`

---

## File Structure

**Backend (`src-tauri/src/`):**
- `auth/tokens.rs` — Keychain save/load/delete, already keyed by an arbitrary account string. No signature change; call sites pass an email instead of `"primary"`.
- `auth/mod.rs` — `connect()` saves under email; replace `ensure_access_token()` with `ensure_token_for(email)`. The *active-account* resolution lives in `commands.rs::active_token(&state)` (it needs the DB handle). Keep `PRIMARY_ACCOUNT` only for the legacy migration.
- `db/mod.rs` — new accounts-index/active-pointer helpers (on the `settings` table); `account` column + migration/backfill; scope `recent_previews`, snooze reads, meeting-note reads/writes by account; `remove_account_data(account)`.
- `commands.rs` — extend `connect_gmail`; new `list_accounts`, `set_active_account`, `remove_account`, `sync_all_accounts`; `get_connected_account` returns the active account; `disconnect` → per-account removal.
- `lib.rs` — register new commands; run `migrate_legacy_primary_account` once at startup.

**Frontend (`src/`):**
- `lib/api.ts` — typed wrappers + `isTauri` gating: `listAccounts`, `setActiveAccount`, `removeAccount`, `syncAllAccounts`; `AccountInfo` type.
- `lib/mock.ts` — `MOCK_ACCOUNTS` + mutable mock active account + per-account messages.
- `components/AccountSwitcher.tsx` — NEW popover anchored to the rail avatar.
- `components/IconRail.tsx` — avatar opens the switcher (callback rename).
- `components/SettingsModal.tsx` — account list with per-account Remove + Add.
- `App.tsx` — `accounts` state, account-epoch reload, all-accounts notify loop, render the switcher.

---

## Phase 1 — Backend account substrate (no UX change)

Goal: tokens keyed by email, an accounts index + active pointer, `ensure_active_token`, and a one-time migration that registers the existing single account. After this phase the app behaves identically (one account) but on the multi-account substrate.

### Task 1.1: Accounts index + active pointer helpers (db)

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (add helpers near `get_settings`, ~line 484; add tests in the `#[cfg(test)]` module)

- [ ] **Step 1: Write failing tests**

Add to the `mod tests` block in `src-tauri/src/db/mod.rs`:

```rust
#[test]
fn accounts_index_round_trips() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    assert_eq!(get_accounts(&c).unwrap(), Vec::<String>::new());
    add_account(&c, "a@gmail.com").unwrap();
    add_account(&c, "b@gmail.com").unwrap();
    add_account(&c, "a@gmail.com").unwrap(); // dedup
    assert_eq!(get_accounts(&c).unwrap(), vec!["a@gmail.com", "b@gmail.com"]);
    remove_account(&c, "a@gmail.com").unwrap();
    assert_eq!(get_accounts(&c).unwrap(), vec!["b@gmail.com"]);
}

#[test]
fn active_account_pointer_round_trips() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    assert_eq!(get_active_account(&c).unwrap(), None);
    set_active_account(&c, "b@gmail.com").unwrap();
    assert_eq!(get_active_account(&c).unwrap(), Some("b@gmail.com".to_string()));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml accounts_index_round_trips active_account_pointer`
Expected: FAIL — `cannot find function get_accounts` (etc.).

- [ ] **Step 3: Implement helpers**

Add after `save_settings` (~line 484) in `src-tauri/src/db/mod.rs`. These reuse the existing private `get_setting_raw`/`set_setting_raw`:

```rust
/// The connected-account index, stored as a JSON array under settings key "accounts".
/// The Keychain can't enumerate entries, so this is the source of truth for "which
/// accounts exist". Order is insertion order; duplicates are ignored.
pub fn get_accounts(conn: &Connection) -> Result<Vec<String>> {
    match get_setting_raw(conn, "accounts")? {
        Some(json) => serde_json::from_str(&json).map_err(|e| AppError::Other(e.to_string())),
        None => Ok(Vec::new()),
    }
}

pub fn add_account(conn: &Connection, email: &str) -> Result<()> {
    let mut accounts = get_accounts(conn)?;
    if !accounts.iter().any(|a| a == email) {
        accounts.push(email.to_string());
        let json = serde_json::to_string(&accounts).map_err(|e| AppError::Other(e.to_string()))?;
        set_setting_raw(conn, "accounts", &json)?;
    }
    Ok(())
}

pub fn remove_account(conn: &Connection, email: &str) -> Result<()> {
    let mut accounts = get_accounts(conn)?;
    accounts.retain(|a| a != email);
    let json = serde_json::to_string(&accounts).map_err(|e| AppError::Other(e.to_string()))?;
    set_setting_raw(conn, "accounts", &json)
}

/// The active account email, or None if none is set.
pub fn get_active_account(conn: &Connection) -> Result<Option<String>> {
    get_setting_raw(conn, "active_account")
}

pub fn set_active_account(conn: &Connection, email: &str) -> Result<()> {
    set_setting_raw(conn, "active_account", email)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml accounts_index_round_trips active_account_pointer`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(accounts): db helpers for accounts index + active pointer"
```

### Task 1.2: Token storage keyed by email (auth)

**Files:**
- Modify: `src-tauri/src/auth/mod.rs:184` (`connect()` saves under email), `auth/mod.rs:210-227` (`ensure_access_token` → active/per-account).

`auth/tokens.rs` already takes an arbitrary `account: &str` — no change there; we just stop passing the `"primary"` constant.

- [ ] **Step 1: Change `connect()` to save under the email**

In `src-tauri/src/auth/mod.rs`, replace line 184 (`save_token(PRIMARY_ACCOUNT, &stored)?;`) with:

```rust
        save_token(&stored.email, &stored)?;
```

- [ ] **Step 2: Split the resolution chokepoint**

Replace the whole `ensure_access_token` function (lines 210-227) with:

```rust
/// Load + refresh the token for a SPECIFIC account email.
pub async fn ensure_token_for(account: &str) -> Result<StoredToken> {
    let mut stored = load_token(account)?
        .ok_or_else(|| AppError::Auth(format!("no token for account {account}")))?;
    if stored.is_expired(now_secs(), 60) {
        let oauth = GoogleOAuth::from_env()?;
        let (access, expires_at) = oauth.refresh(&stored.refresh_token).await?;
        stored.access_token = access;
        stored.expires_at = expires_at;
        save_token(account, &stored)?;
    }
    Ok(stored)
}
```

`ensure_active_token` (which needs the DB to read the active pointer) is added in Task 1.4, after the DB is wired into auth. For now keep compilation green by adding a temporary shim that resolves the legacy account; we replace it in 1.4. Add directly below `ensure_token_for`:

```rust
/// TEMPORARY (replaced in Task 1.4): resolve the legacy primary account.
pub async fn ensure_access_token() -> Result<StoredToken> {
    ensure_token_for(PRIMARY_ACCOUNT).await
}
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds (existing `ensure_access_token()` call sites unchanged; `connect` now saves under email — but nothing reads the email key yet, so behavior is still legacy via the shim). Existing tests still pass:
Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/auth/mod.rs
git commit -m "refactor(auth): key tokens by email; add ensure_token_for"
```

### Task 1.3: Active-account-aware `get_connected_account` + DB access in auth resolution

The active pointer lives in the DB, but `auth/mod.rs` has no DB handle. Resolve the active account in the **command layer** (which has `tauri::State<Db>`) and pass the email into `ensure_token_for`. Introduce a command-layer helper `active_token(&state)`.

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `active_token` helper; rewrite `get_connected_account`)

- [ ] **Step 1: Add the `active_token` helper**

In `src-tauri/src/commands.rs`, update the auth import (line 10) and add a helper near the top (after imports):

```rust
use crate::auth::{ensure_token_for, GoogleOAuth, PRIMARY_ACCOUNT};
```

```rust
/// Resolve the active account from the DB pointer, then load + refresh its token.
/// This is the multi-account replacement for the old `ensure_access_token()`.
async fn active_token(state: &tauri::State<'_, Db>) -> Result<StoredToken> {
    let account = {
        let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_active_account(&conn)?
    }
    .ok_or_else(|| AppError::Auth("no active account".into()))?;
    ensure_token_for(&account).await
}
```

(Ensure `StoredToken` is imported in commands.rs; add `use crate::auth::tokens::StoredToken;` if not already present.)

- [ ] **Step 2: Rewrite `get_connected_account` (lines 47-50)**

```rust
/// The currently active account email, if any.
#[tauri::command]
pub async fn get_connected_account(state: tauri::State<'_, Db>) -> Result<Option<String>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::get_active_account(&conn)
}
```

- [ ] **Step 3: Build**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: FAILS — `get_connected_account` now needs `state`, and the ~30 `ensure_access_token().await?` call sites + `PRIMARY_ACCOUNT` sync_state references still use the legacy path. That's expected; fixed in Task 1.4.

### Task 1.4: Swap all command call sites to the active account

**Files:**
- Modify: `src-tauri/src/commands.rs` (every `ensure_access_token().await?` → `active_token(&state).await?`; `PRIMARY_ACCOUNT` sync_state keys → `stored.email`)
- Modify: `src-tauri/src/auth/mod.rs` (delete the temporary `ensure_access_token` shim)

- [ ] **Step 1: Replace `ensure_access_token()` call sites**

In `src-tauri/src/commands.rs`, replace every occurrence of:

```rust
    let stored = ensure_access_token().await?;
```

with:

```rust
    let stored = active_token(&state).await?;
```

There are ~30 occurrences (lines 57, 247, 275, 296, 374, 438, 504, 516, 543, 581, 600, 624, 634, 642, 650, 660, 672, 680, 698, 713, 729, 751, 766, 781, 868, 894, 906, 914, …). Each enclosing command already has `state: tauri::State<'_, Db>` (they touch the DB) **except** the DB-free network commands (calendar list/create/update/delete, label fetch, reply context, etc.). For those, add `state: tauri::State<'_, Db>` to the command signature so `active_token` can read the pointer. Example for `list_calendars` (line 866-870):

```rust
#[tauri::command]
pub async fn list_calendars(state: tauri::State<'_, Db>) -> Result<Vec<CalendarSummary>> {
    let stored = active_token(&state).await?;
    let client = CalendarClient::new(stored.access_token);
    ...
```

Apply the same `state` parameter addition to every previously-DB-free command that called `ensure_access_token()`.

- [ ] **Step 2: Replace `PRIMARY_ACCOUNT` sync_state keys with the active email**

In `sync_inbox` and any function calling `db::get_sync_state`/`db::set_sync_state` with `PRIMARY_ACCOUNT` (lines 68, 96, 131), replace `PRIMARY_ACCOUNT` with `&stored.email` (the value returned by `active_token`). Example (line 68):

```rust
        db::get_sync_state(&conn, &stored.email)?.and_then(|s| s.last_history_id)
```

and lines 96 / 131:

```rust
                db::set_sync_state(&conn, &stored.email, Some(new_hid), now_secs() as i64)?;
```

- [ ] **Step 3: Remove the temporary shim**

In `src-tauri/src/auth/mod.rs`, delete the temporary `ensure_access_token` function added in Task 1.2. Keep `PRIMARY_ACCOUNT` (used by the migration in Task 1.5). Remove `ensure_access_token` from the `commands.rs` import (Step 1 of Task 1.3 already dropped it).

- [ ] **Step 4: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: builds. If a command still calls `active_token(&state)` without a `state` param, the compiler names it — add the param.
Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 5: Update frontend `getConnectedAccount` (no arg change needed)**

`get_connected_account` now takes `state`, but Tauri injects `State` automatically — the JS `invoke("get_connected_account")` call is unchanged. No frontend edit. Verify types still build:
Run: `npm run build`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/auth/mod.rs
git commit -m "refactor(accounts): resolve active account at every command call site"
```

### Task 1.5: One-time legacy migration (register the existing account)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `migrate_legacy_primary_account`)
- Modify: `src-tauri/src/lib.rs` (call it once at startup, after `db::init`)
- Modify: `src-tauri/src/db/mod.rs` (add `stamp_legacy_account` backfill helper + test)

- [ ] **Step 1: Write a failing db backfill test**

Add to the `mod tests` block in `db/mod.rs`:

```rust
#[test]
fn stamp_legacy_account_backfills_empty_account_rows() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    // a row with the pre-migration empty account
    c.execute("INSERT INTO messages (id, account) VALUES ('m1', '')", []).unwrap();
    c.execute("INSERT INTO sync_state (account, last_history_id, last_synced_at) VALUES ('primary', 5, 0)", []).unwrap();
    stamp_legacy_account(&c, "me@gmail.com").unwrap();
    let acct: String = c.query_row("SELECT account FROM messages WHERE id='m1'", [], |r| r.get(0)).unwrap();
    assert_eq!(acct, "me@gmail.com");
    let sacct: String = c.query_row("SELECT account FROM sync_state", [], |r| r.get(0)).unwrap();
    assert_eq!(sacct, "me@gmail.com");
}
```

(This test depends on the `account` column existing on `messages` — that column is added in Task 2.1. **Order note:** if executing strictly in order, move Task 2.1 before this test, or temporarily add `account TEXT NOT NULL DEFAULT ''` to the `messages` CREATE now. Recommended: do Task 2.1's column-add step first, then this test.)

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml stamp_legacy_account`
Expected: FAIL — `cannot find function stamp_legacy_account`.

- [ ] **Step 3: Implement `stamp_legacy_account`**

In `db/mod.rs`:

```rust
/// One-time migration: stamp pre-multi-account cache rows (account='') and the legacy
/// 'primary' sync_state with the given account email.
pub fn stamp_legacy_account(conn: &Connection, email: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE messages SET account = ?1 WHERE account = ''", params![email])?;
    tx.execute("UPDATE snoozed SET account = ?1 WHERE account = ''", params![email])?;
    tx.execute("UPDATE meeting_notes SET account = ?1 WHERE account = ''", params![email])?;
    tx.execute("UPDATE sync_state SET account = ?1 WHERE account = 'primary'", params![email])?;
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml stamp_legacy_account`
Expected: PASS.

- [ ] **Step 5: Implement `migrate_legacy_primary_account` (commands.rs)**

```rust
/// Run once at startup: if a legacy "primary" Keychain token exists and no accounts
/// are registered yet, migrate it to the email-keyed scheme and stamp cached rows.
pub fn migrate_legacy_primary_account(conn: &rusqlite::Connection) -> Result<()> {
    if !db::get_accounts(conn)?.is_empty() {
        return Ok(()); // already migrated
    }
    let Some(token) = load_token(PRIMARY_ACCOUNT)? else {
        return Ok(()); // fresh install — nothing to migrate
    };
    let email = token.email.clone();
    save_token(&email, &token)?;
    db::add_account(conn, &email)?;
    db::set_active_account(conn, &email)?;
    db::stamp_legacy_account(conn, &email)?;
    delete_token(PRIMARY_ACCOUNT)?;
    Ok(())
}
```

(Ensure `load_token`, `save_token`, `delete_token`, `PRIMARY_ACCOUNT` are imported in commands.rs.)

- [ ] **Step 6: Call it at startup (lib.rs)**

In `src-tauri/src/lib.rs`, after the DB is initialized in the Tauri `setup` closure, add (matching the existing `db::init` call style):

```rust
            {
                let conn = db.lock().expect("db lock");
                commands::migrate_legacy_primary_account(&conn)
                    .unwrap_or_else(|e| eprintln!("[ember] legacy account migration failed: {e}"));
            }
```

(Read the existing `setup`/`manage(Db)` block first and mirror how the `Connection`/`Db` is obtained.)

- [ ] **Step 7: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/db/mod.rs
git commit -m "feat(accounts): one-time migration of the legacy primary account"
```

### Task 1.6: `connect_gmail` registers + activates the account

Without this, a newly added account never lands in the accounts index (so `list_accounts` won't show it), and a **fresh install's** first connect leaves `active_account` unset (so nothing works). This is required for both the add-account flow and first-run.

**Files:**
- Modify: `src-tauri/src/commands.rs:39-44` (`connect_gmail`)

- [ ] **Step 1: Add `state` + register the account**

Replace `connect_gmail` (lines 39-44) with:

```rust
/// Run the interactive Google sign-in, register the account in the index, and make it
/// active. Returns the connected email address.
#[tauri::command]
pub async fn connect_gmail(state: tauri::State<'_, Db>) -> Result<String> {
    let oauth = GoogleOAuth::from_env()?;
    let stored = oauth.connect().await?; // already saves the token under stored.email
    {
        let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::add_account(&conn, &stored.email)?;     // idempotent — no-op if already present
        db::set_active_account(&conn, &stored.email)?; // newly added account becomes active
    }
    Ok(stored.email)
}
```

The JS call `invoke("connect_gmail")` is unchanged (Tauri injects `State`).

- [ ] **Step 2: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib && npm run build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(accounts): connect_gmail registers + activates the account"
```

---

## Phase 2 — Account-scoped cache

Goal: add the `account` column, scope every cache query by account, and generalize disconnect into per-account removal. Still single-account in the UX.

### Task 2.1: Add the `account` column + composite index

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (CREATE TABLE additions + `add_column_if_missing` migrations + index)

- [ ] **Step 1: Add columns to the CREATE TABLE literals**

In `init()`, add `account TEXT NOT NULL DEFAULT ''` to the `messages`, `snoozed`, and `meeting_notes` CREATE TABLE statements (so fresh DBs have it). For `messages` add it after `category`. For `snoozed` after `internal_date`. For `meeting_notes` after `transcript`.

- [ ] **Step 2: Add idempotent migrations for existing DBs**

In `init()`, alongside the other `add_column_if_missing` calls (~line 177):

```rust
    add_column_if_missing(conn, "messages", "account", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "snoozed", "account", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "meeting_notes", "account", "TEXT NOT NULL DEFAULT ''")?;
```

And add a composite index after the existing `idx_messages_category` index:

```rust
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_messages_account_internal_date
         ON messages(account, internal_date DESC)",
        [],
    )?;
```

- [ ] **Step 3: Verify existing tests still pass (columns are additive)**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS (defaults keep old tests valid).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(accounts): add account column to messages/snoozed/meeting_notes"
```

### Task 2.2: Scope message reads/writes by account

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (`upsert`/`apply_delta`/`recent_previews`/`update_message_labels`/`apply_label_delta` gain an `account` parameter)
- Modify: `src-tauri/src/commands.rs` (pass `&stored.email`)

- [ ] **Step 1: Write a failing scoping test**

Add to `db/mod.rs` tests:

```rust
#[test]
fn recent_previews_filters_by_account() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    // Build StoredMessage values the SAME way the existing upsert tests do
    // (e.g. the `sample`/inline builder already in this test module — reuse it).
    let a = sample_message("a1");
    let b = sample_message("b1");
    upsert_messages(&c, "a@x.com", std::slice::from_ref(&a)).unwrap();
    upsert_messages(&c, "b@x.com", std::slice::from_ref(&b)).unwrap();
    let only_a = recent_previews(&c, "a@x.com", 50).unwrap();
    assert_eq!(only_a.len(), 1);
    assert_eq!(only_a[0].id, "a1");
}
```

Function names are the EXISTING ones with an added `account` parameter — `upsert_messages(conn, account, &[StoredMessage])` and `recent_previews(conn, account, max)` — not new `_for` variants. For `sample_message`, reuse whatever `StoredMessage` builder the current `mod tests` already uses (read the test module first and match it); if tests build the struct inline, do the same here.

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml recent_previews_filters_by_account`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Add `account` to the SQL + signatures**

Update `UPSERT_SQL` to include the `account` column and a `?13` bind. Rename/extend the public functions to take an account:
- `upsert_messages(conn, account, &[StoredMessage])` — stamps `account` on every row.
- `apply_delta(conn, account, rows, removed_ids, cutoff_ms)` — upserts stamped, and the prune `DELETE` stays global (cutoff prune is account-agnostic and harmless) OR scope deletes to the account; scope `removed_ids` deletes to `WHERE id = ?1 AND account = ?2`.
- `recent_previews(conn, account, max)` — add `WHERE account = ?1` and shift `LIMIT` to `?2`.
- `update_message_labels` / `apply_label_delta` — add `AND account = ?` to the WHERE.

Show the `recent_previews` change explicitly:

```rust
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
            id: row.get(0)?, thread_id: row.get(1)?, from_addr: row.get(2)?,
            subject: row.get(3)?, snippet: row.get(4)?, date_header: row.get(5)?,
            internal_date: row.get(6)?, label_ids: row.get(7)?, to_addr: row.get(8)?,
            has_list_unsubscribe: row.get(9)?, has_list_id: row.get(10)?, category: row.get(11)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows { out.push(r?); }
    Ok(out)
}
```

Update `UPSERT_SQL` and `upsert_one`/`upsert_messages` to stamp `account`. Add the `account` as the first param of each scoped function.

- [ ] **Step 4: Update commands.rs call sites**

In `commands.rs`, pass `&stored.email` to `db::recent_previews`, `db::upsert_messages`, `db::apply_delta`, `db::update_message_labels`, `db::apply_label_delta` (the enclosing commands all have `stored` from `active_token`). Use `cargo build` errors to find each site.

- [ ] **Step 5: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS (adjust the test helper names in Step 1 to match the codebase).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db/mod.rs src-tauri/src/commands.rs
git commit -m "feat(accounts): scope message cache reads/writes by account"
```

### Task 2.3: Scope snoozed + meeting_notes by account

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (`insert_snooze`, `list_snoozes`, `due_snoozes`, `delete_snoozes`; `get_meeting_note`, `list_meeting_notes`, `upsert_meeting_note`, `delete_meeting_note`, `set_meeting_note_summary` gain `account`)
- Modify: `src-tauri/src/commands.rs` (pass `&stored.email`)

- [ ] **Step 1: Write failing tests**

Add tests mirroring 2.2 for snoozed and notes (insert under two accounts, assert `list_*_for(account)` returns only that account's rows). Match existing test helpers/builders.

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml snooze_filters_by_account note_filters_by_account`
Expected: FAIL.

- [ ] **Step 3: Implement scoping**

- `snoozed`: `insert_snooze` stamps `account`; `list_snoozes(conn, account)` and `due_snoozes(conn, account, now_ms)` add `WHERE account = ?`. (`delete_snoozes` by id can stay id-only — message ids are globally unique — but add `AND account = ?` for safety if cheap.)
- `meeting_notes`: every read/write gains `account`; the UNIQUE constraint stays `(calendar_id, event_id)` — calendar/event ids are globally unique per Google, and notes are scoped by which account owns the calendar, so also stamp `account` on write and filter reads by it.

- [ ] **Step 4: Update commands.rs call sites**

Pass `&stored.email` from `active_token` into each scoped snooze/note db call. For the snooze **wake loop** command (`wake_due_snoozes`), it must wake due snoozes for the **active** account (Phase 3 will broaden if needed) — pass `stored.email`.

- [ ] **Step 5: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db/mod.rs src-tauri/src/commands.rs
git commit -m "feat(accounts): scope snoozed + meeting_notes by account"
```

### Task 2.4: Per-account removal (generalize disconnect)

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (`remove_account_data(conn, account)` replacing the body of `clear_account_data`; keep `clear_account_data` calling it for all-accounts if still referenced, else remove)
- Modify: `src-tauri/src/commands.rs` (`disconnect` → `remove_account(email)` command)

- [ ] **Step 1: Write a failing test**

```rust
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
}
```

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml remove_account_data_only_wipes`
Expected: FAIL.

- [ ] **Step 3: Implement `remove_account_data`**

```rust
/// Wipe all cache rows for one account (used on per-account removal).
pub fn remove_account_data(conn: &Connection, account: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM messages WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM snoozed WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM meeting_notes WHERE account = ?1", params![account])?;
    tx.execute("DELETE FROM sync_state WHERE account = ?1", params![account])?;
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4: Implement the `remove_account` command**

Replace `disconnect` (commands.rs ~851) with:

```rust
/// Remove ONE account: delete its Keychain token + scoped cache, drop it from the index,
/// and re-point `active_account` to another connected account (or clear it if none remain).
#[tauri::command]
pub async fn remove_account(state: tauri::State<'_, Db>, email: String) -> Result<Option<String>> {
    delete_token(&email)?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::remove_account_data(&conn, &email)?;
    db::remove_account(&conn, &email)?;
    let remaining = db::get_accounts(&conn)?;
    let next = remaining.first().cloned();
    match &next {
        Some(e) => db::set_active_account(&conn, e)?,
        None => db::clear_active_account(&conn)?, // last account removed → no active pointer
    }
    Ok(next)
}
```

This needs a `clear_active_account` helper. Add it to `db/mod.rs` (next to `set_active_account`):

```rust
pub fn clear_active_account(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = 'active_account'", [])?;
    Ok(())
}
```

`get_active_account` already returns `None` when the key is absent, so after removing the last account `get_connected_account` returns `None` and the frontend shows the connect screen.

- [ ] **Step 5: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/db/mod.rs src-tauri/src/commands.rs
git commit -m "feat(accounts): per-account removal replacing global disconnect"
```

---

## Phase 3 — Switching UI

Goal: user-visible add / switch / remove. Background polling still active-account-only (Phase 4 broadens it).

### Task 3.1: `list_accounts` + `set_active_account` commands

**Files:**
- Modify: `src-tauri/src/commands.rs` (two commands + `AccountInfo` struct)
- Modify: `src-tauri/src/db/mod.rs` (helper `unread_count(conn, account)` for the badge)
- Modify: `src-tauri/src/lib.rs` (register `list_accounts`, `set_active_account`, `remove_account`)

- [ ] **Step 1: Write a failing db test for `unread_count`**

```rust
#[test]
fn unread_count_counts_unread_for_account() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    c.execute("INSERT INTO messages (id, account, label_ids) VALUES ('a1','a@x.com','INBOX,UNREAD')", []).unwrap();
    c.execute("INSERT INTO messages (id, account, label_ids) VALUES ('a2','a@x.com','INBOX')", []).unwrap();
    assert_eq!(unread_count(&c, "a@x.com").unwrap(), 1);
}
```

- [ ] **Step 2: Verify failure, then implement**

```rust
pub fn unread_count(conn: &Connection, account: &str) -> Result<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account = ?1 AND label_ids LIKE '%UNREAD%'",
        params![account], |r| r.get(0),
    )?;
    Ok(n)
}
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml unread_count_counts_unread`
Expected: PASS.

- [ ] **Step 3: Add the commands**

```rust
#[derive(serde::Serialize)]
pub struct AccountInfo { pub email: String, pub active: bool, pub unread: i64 }

#[tauri::command]
pub async fn list_accounts(state: tauri::State<'_, Db>) -> Result<Vec<AccountInfo>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    let active = db::get_active_account(&conn)?;
    let mut out = Vec::new();
    for email in db::get_accounts(&conn)? {
        let unread = db::unread_count(&conn, &email)?;
        out.push(AccountInfo { active: Some(&email) == active.as_ref(), email, unread });
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
```

- [ ] **Step 4: Register all new commands in `lib.rs`**

Add `list_accounts`, `set_active_account`, `remove_account` to the `tauri::generate_handler![…]` list (read the existing list and append; remove `disconnect` if it was registered).

- [ ] **Step 5: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/db/mod.rs src-tauri/src/lib.rs
git commit -m "feat(accounts): list_accounts + set_active_account commands"
```

### Task 3.2: Frontend api + mock multi-account

**Files:**
- Modify: `src/lib/api.ts` (`AccountInfo` type + `listAccounts`/`setActiveAccount`/`removeAccount` wrappers; update `connectGmail`)
- Modify: `src/lib/mock.ts` (`MOCK_ACCOUNTS`, mutable active, per-account messages)

- [ ] **Step 1: Add types + wrappers in `api.ts`**

```ts
export interface AccountInfo { email: string; active: boolean; unread: number }

export const listAccounts = (): Promise<AccountInfo[]> =>
  isTauri() ? invoke<AccountInfo[]>("list_accounts") : Promise.resolve(mockListAccounts());

export const setActiveAccount = (email: string): Promise<void> =>
  isTauri() ? invoke<void>("set_active_account", { email }) : (mockSetActiveAccount(email), Promise.resolve());

export const removeAccount = (email: string): Promise<string | null> =>
  isTauri() ? invoke<string | null>("remove_account", { email }) : Promise.resolve(mockRemoveAccount(email));
```

Replace the `disconnect` export with `removeAccount`. Keep `getConnectedAccount` returning the active account; in the maket return the mock active account.

- [ ] **Step 2: Add mock multi-account state in `mock.ts`**

```ts
export const MOCK_ACCOUNTS = ["you@example.com (mock)", "work@example.com (mock)"];
let mockActive = MOCK_ACCOUNTS[0];
export const mockGetActive = () => mockActive;
export const mockSetActiveAccount = (email: string) => { mockActive = email; };
export const mockListAccounts = () =>
  MOCK_ACCOUNTS.map((email) => ({ email, active: email === mockActive, unread: email === mockActive ? 2 : 1 }));
export const mockRemoveAccount = (email: string) => {
  const i = MOCK_ACCOUNTS.indexOf(email);
  if (i >= 0) MOCK_ACCOUNTS.splice(i, 1);
  if (mockActive === email) mockActive = MOCK_ACCOUNTS[0] ?? null;
  return mockActive;
};
```

Update `MOCK_ACCOUNT` usages: keep the const but have `getConnectedAccount`'s mock branch return `mockGetActive()`. Optionally vary `MOCK_MESSAGES` per active account (e.g. a second small array) so switching visibly changes the list.

- [ ] **Step 3: Typecheck**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/lib/api.ts src/lib/mock.ts
git commit -m "feat(accounts): frontend api wrappers + multi-account maket mock"
```

### Task 3.3: AccountSwitcher popover + IconRail wiring

**Files:**
- Create: `src/components/AccountSwitcher.tsx`
- Modify: `src/components/IconRail.tsx` (avatar opens switcher via a new `onAvatar` prop, replacing the `onSettings` wired to the avatar)
- Modify: `src/styles/*` (popover styles — match existing `.compose-overlay`/popover conventions; read a sibling component first)

- [ ] **Step 1: Create the component**

```tsx
import { Check, Plus, Settings as SettingsIcon } from "lucide-react";
import type { AccountInfo } from "../lib/api";

export function AccountSwitcher({
  accounts, onSwitch, onAdd, onManage, onClose,
}: {
  accounts: AccountInfo[];
  onSwitch: (email: string) => void;
  onAdd: () => void;
  onManage: () => void;
  onClose: () => void;
}) {
  return (
    <>
      <div className="popover-backdrop" onClick={onClose} />
      <div className="account-switcher" role="menu" aria-label="Accounts">
        {accounts.map((a) => (
          <button key={a.email} className="account-row" role="menuitem"
            onClick={() => { if (!a.active) onSwitch(a.email); onClose(); }}>
            <span className="account-initials">{a.email.slice(0, 2).toUpperCase()}</span>
            <span className="account-email">{a.email}</span>
            {a.unread > 0 && <span className="account-unread">{a.unread}</span>}
            {a.active && <Check size={16} className="account-check" />}
          </button>
        ))}
        <button className="account-row account-action" role="menuitem" onClick={() => { onAdd(); onClose(); }}>
          <Plus size={16} /> Add account
        </button>
        <button className="account-row account-action" role="menuitem" onClick={() => { onManage(); onClose(); }}>
          <SettingsIcon size={16} /> Manage in Settings
        </button>
      </div>
    </>
  );
}
```

- [ ] **Step 2: Wire IconRail avatar**

In `IconRail.tsx`, rename the avatar's `onClick={onSettings}` to a new prop `onAvatar` and add it to the props type; keep `onSettings` for any other callers (or remove if unused). The parent (App) passes `onAvatar={() => setSwitcherOpen(true)}`.

- [ ] **Step 3: Add minimal styles**

Read an existing popover/overlay rule in `src/styles/` and add `.popover-backdrop`, `.account-switcher` (absolute, anchored bottom-left near the rail), `.account-row`, `.account-initials`, `.account-unread`, `.account-check` matching the dark theme + green accent.

- [ ] **Step 4: Typecheck**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/AccountSwitcher.tsx src/components/IconRail.tsx src/styles
git commit -m "feat(accounts): AccountSwitcher popover + rail avatar wiring"
```

### Task 3.4: App state — accounts, switching, epoch reload

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add state + load accounts**

Add near the other `useState`s (line ~59):

```tsx
const [accounts, setAccounts] = useState<AccountInfo[]>([]);
const [switcherOpen, setSwitcherOpen] = useState(false);
const [accountEpoch, setAccountEpoch] = useState(0);
```

In the mount `useEffect` (line ~159), also `listAccounts().then(setAccounts).catch(() => {})`. Import `listAccounts`, `setActiveAccount`, `removeAccount`, `AccountInfo` from `./lib/api`.

- [ ] **Step 2: Switch handler + epoch-driven reload**

```tsx
async function handleSwitchAccount(email: string) {
  await setActiveAccount(email);
  setAccount(email);
  setSelectedId(null);
  setStream("all");
  setFolder("inbox");
  setAccountEpoch((e) => e + 1); // forces mail + calendar reloads
  const [acc, list] = await Promise.all([listAccounts(), fetchInboxPreview(50)]);
  setAccounts(acc);
  setMessages(list);
  seedKnown(list);
}
```

Add `accountEpoch` to the dependency arrays of the inbox-load effect and the calendar view's reload trigger (mirror how `folderReloadKey` is threaded). The CalendarView re-fetches on `accountEpoch` change.

- [ ] **Step 3: Render the switcher**

Where `IconRail` is rendered (line ~775), pass `onAvatar={() => setSwitcherOpen(true)}`. After it, conditionally render:

```tsx
{switcherOpen && (
  <AccountSwitcher
    accounts={accounts}
    onSwitch={handleSwitchAccount}
    onAdd={handleConnect}            // reuse existing connect flow (adds + activates)
    onManage={() => setSettingsOpen(true)}
    onClose={() => setSwitcherOpen(false)}
  />
)}
```

`handleConnect` already calls `connectGmail` + syncs; after it, also refresh `listAccounts()` and bump the epoch so the newly added account becomes active in the UI.

- [ ] **Step 4: Typecheck**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 5: Verify in the maket**

`preview_start` `ember-maket`; click the avatar; confirm the switcher lists both mock accounts, switching flips the active check + (if per-account mock messages added) the list; "Manage in Settings" opens Settings.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx
git commit -m "feat(accounts): account state, switching, epoch-driven reload"
```

### Task 3.5: Settings account list + per-account remove

**Files:**
- Modify: `src/components/SettingsModal.tsx`
- Modify: `src/App.tsx` (`handleDisconnected` → `handleAccountRemoved(next)`)

- [ ] **Step 1: Replace the single account row + disconnect**

`SettingsModal` takes `accounts: AccountInfo[]`, `onRemove: (email) => Promise<void>`, `onAdd: () => void` (replacing `onDisconnected`). Render the account list, each with a Remove button (keep the confirm-before-remove pattern). Add an "Add account" button calling `onAdd`.

- [ ] **Step 2: App wiring**

```tsx
async function handleRemoveAccount(email: string) {
  const next = await removeAccount(email);
  const acc = await listAccounts();
  setAccounts(acc);
  if (next) { setAccount(next); setAccountEpoch((e) => e + 1); /* reload */ }
  else handleDisconnected(); // last account removed → connect screen
}
```

Pass `accounts`, `onRemove={handleRemoveAccount}`, `onAdd={handleConnect}` to `SettingsModal`.

- [ ] **Step 3: Typecheck + maket**

Run: `npm run build`; in the maket, open Settings, remove the non-active mock account (list shrinks), remove the active one (switches), remove the last (connect screen).

- [ ] **Step 4: Commit**

```bash
git add src/components/SettingsModal.tsx src/App.tsx
git commit -m "feat(accounts): Settings account list + per-account remove"
```

---

## Phase 4 — All-accounts background sync + notifications

Goal: poll every connected account in the background; notify per account; suppress an account's first (baseline) sync.

### Task 4.1: Per-account sync extraction + baseline flag

**Files:**
- Modify: `src-tauri/src/commands.rs` (extract `sync_one_account(state, email) -> AccountSyncSummary`; refactor `sync_inbox` to call it for the active account)

- [ ] **Step 1: Define the summary type**

```rust
#[derive(serde::Serialize)]
pub struct AccountSyncSummary {
    pub account: String,
    pub added: usize,
    pub removed: usize,
    pub baseline: bool,             // true when this run only established the historyId
    pub new_previews: Vec<MessagePreview>, // newly-added previews (empty when baseline)
}
```

- [ ] **Step 2: Extract `sync_one_account`**

Move the body of `sync_inbox` into `async fn sync_one_account(state, email) -> Result<AccountSyncSummary>` that takes an explicit account email (instead of resolving the active one), uses `ensure_token_for(email)`, scopes all db calls by `email`, sets `baseline = last_history_id.is_none()` at entry, and returns the added previews (empty when `baseline`). Re-implement `sync_inbox` as:

```rust
#[tauri::command]
pub async fn sync_inbox(state: tauri::State<'_, Db>) -> Result<SyncSummary> {
    let email = { let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
                  db::get_active_account(&conn)?.ok_or_else(|| AppError::Auth("no active account".into()))? };
    let s = sync_one_account(&state, &email).await?;
    Ok(SyncSummary { added: s.added, removed: s.removed })
}
```

- [ ] **Step 3: Build + existing tests**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (gmail_test integration unaffected — it tests the GmailClient, not the command; confirm).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "refactor(accounts): extract sync_one_account with baseline flag"
```

### Task 4.2: `sync_all_accounts` command

**Files:**
- Modify: `src-tauri/src/commands.rs` (new command)
- Modify: `src-tauri/src/lib.rs` (register it)

- [ ] **Step 1: Implement**

```rust
/// Sync every connected account into the scoped cache. Returns per-account summaries
/// (with new_previews for non-baseline runs) so the frontend can notify per account.
#[tauri::command]
pub async fn sync_all_accounts(state: tauri::State<'_, Db>) -> Result<Vec<AccountSyncSummary>> {
    let accounts = { let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
                     db::get_accounts(&conn)? };
    let mut out = Vec::new();
    for email in accounts {
        match sync_one_account(&state, &email).await {
            Ok(s) => out.push(s),
            Err(e) => eprintln!("[ember] sync failed for {email}: {e}"), // one bad account can't block others
        }
    }
    Ok(out)
}
```

Register `sync_all_accounts` in `lib.rs`.

- [ ] **Step 2: Build + test**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(accounts): sync_all_accounts background command"
```

### Task 4.3: Frontend all-accounts notify loop

**Files:**
- Modify: `src/lib/api.ts` (`syncAllAccounts` wrapper + `AccountSyncSummary` type)
- Modify: `src/lib/notify.ts` (`notifyNewMail` gains an account label, e.g. title `New mail · <account>`)
- Modify: `src/App.tsx` (background loop calls `syncAllAccounts`; notify per account; refresh active list)

- [ ] **Step 1: api wrapper + type**

```ts
export interface AccountSyncSummary { account: string; added: number; removed: number; baseline: boolean; new_previews: MessagePreview[] }
export const syncAllAccounts = (): Promise<AccountSyncSummary[]> =>
  isTauri() ? invoke<AccountSyncSummary[]>("sync_all_accounts") : Promise.resolve([]);
```

- [ ] **Step 2: notify per account**

In `notify.ts`, extend `notifyNewMail(m, accountLabel?)` to prefix the title with the account when provided. Read the current `notifyNewMail` first and keep its body.

- [ ] **Step 3: Rework `runSync`'s background path**

In `App.tsx`, change the **background** branch (the timer) to call `syncAllAccounts()`: for each summary with `!baseline`, fire `notifyNewMail(m, summary.account)` for its `new_previews` (respecting `notifyAllowedRef` + `!document.hasFocus()`), and for the **active** account also `fetchInboxPreview(50)` → `setMessages`. The manual Sync button keeps the active-account `runSync(true)` path. Keep the `known`-ids fold for the active account to avoid double-notifying on the next manual sync.

- [ ] **Step 4: Typecheck + maket**

Run: `npm run build`. In the maket `syncAllAccounts` returns `[]` (no Tauri), so the loop is a no-op there — verify no console errors and the app still renders. Live notification behavior is owner-verified in the real Tauri build.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/lib/notify.ts src/App.tsx
git commit -m "feat(accounts): all-accounts background sync + per-account notifications"
```

### Task 4.4: Final verification

- [ ] **Step 1: Full backend test run**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all PASS.

- [ ] **Step 2: Full frontend build**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 3: Maket smoke test**

`preview_start` `ember-maket`; verify: avatar opens switcher; switch flips active; add-account path (mock); Settings list remove (non-active, active, last); no console errors (`preview_console_logs level=error`).

- [ ] **Step 4: Update the auto-memory**

Append an `M23-multi-account` note to the Ember memory per the project's milestone-logging habit (new commands, the account-scoped cache, the migration, the switcher).

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "test(accounts): full multi-account verification pass"
```

---

## Notes & risks (from the spec self-review)

- **Migration ordering:** Task 2.1 (add `account` column) must precede Task 1.5's backfill test. If executing strictly top-to-bottom, do Task 2.1's column-add step first. The runtime migration is safe regardless (additive + idempotent backfill on `account=''`).
- **Baseline suppression** (Task 4.1) is the subtlest correctness point: a newly added account's first sync establishes the historyId and must NOT notify, or adding an account dumps ~30 days of alerts.
- **DB-free commands gain a `state` param** (Task 1.4) only so they can read the active pointer; their network behavior is unchanged.
- **Out of scope:** unified inbox, non-Google accounts, per-account signatures, the pre-existing ungated-`invoke` maket gap.
