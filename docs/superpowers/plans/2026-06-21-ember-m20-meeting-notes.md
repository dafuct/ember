# Ember — M20 Meeting Notes (storage foundation) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add local, per-event meeting notes — one editable plain-text note document per Google Calendar event — with a `NotesModal` editor, a "has-notes" dot on the week grid, and a collapsible Notes panel inside the calendar view.

**Architecture:** A new local-only `meeting_notes` SQLite table (added additively in `db::init()`), keyed `UNIQUE(calendar_id, event_id)`, with a denormalized `event_title`/`event_start` snapshot + backend-stamped `created_at`/`updated_at`. Four DB-only Tauri commands (`get`/`save`-upsert/`delete`/`list`) — **no Calendar API, no auth, no network**. The frontend adds `lib/notes.ts` (wrappers + `noteKey` + mocks via `mock.ts`), a `NotesModal`, and wiring into `CalendarView`/`WeekGrid`. Notes never sync to Google.

**Tech Stack:** Rust (rusqlite, serde, Tauri 2), React 19 + TypeScript + Vite, lucide-react. No new dependencies, no new OAuth scope.

**Learning mode (IMPORTANT):** the repo owner is learning Rust — every Rust block below already carries `// 🦀` teaching comments; keep them. After each Rust task give a 2–3 sentence plain-English recap. TS/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats). Commit messages use the `feat(m20:)` / `test(m20:)` style and end with the `Co-Authored-By` trailer shown in each commit step.

**Reference (read before starting):** spec at `docs/superpowers/specs/2026-06-21-ember-m20-meeting-notes-design.md`. Existing patterns this plan mirrors: `db/mod.rs` (`UPSERT_SQL`/`upsert_one`, `get_sync_state`, the `#[cfg(test)]` in-memory `conn()` helper), `commands.rs` (`get_settings` locks state without `.await`; `now_secs`), `lib.rs` (`invoke_handler` list), `EventModal.tsx` (modal pattern), `lib/api.ts` + `lib/mock.ts` (`isTauri()`-gated wrappers + mock store), `CalendarView.tsx`/`WeekGrid.tsx`.

---

## File structure

**Backend**
- `src-tauri/src/db/mod.rs` — *modify*: add `MeetingNote`/`MeetingNoteWrite` structs, the `meeting_notes` table in `init()`, and `upsert_meeting_note`/`get_meeting_note`/`list_meeting_notes`/`delete_meeting_note` + tests.
- `src-tauri/src/commands.rs` — *modify*: add `now_millis()` + four `#[tauri::command]` wrappers.
- `src-tauri/src/lib.rs` — *modify*: register the four commands.

**Frontend**
- `src/lib/notes.ts` — *create*: `MeetingNote`/`MeetingNoteWrite` interfaces, `noteKey`, four `isTauri()`-gated wrappers.
- `src/lib/mock.ts` — *modify*: in-memory mock note store + seed.
- `src/components/NotesModal.tsx` — *create*: the editor.
- `src/components/CalendarView.tsx` — *modify*: load notes, Notes panel + toggle, popover "Notes" button, `NotesModal` host, `notesReloadKey`.
- `src/components/WeekGrid.tsx` — *modify*: `notesByKey` prop + dot.
- `src/styles/app.css` — *modify*: dot, drawer, note rows.

---

## Task 1: DB layer — `meeting_notes` table, types, CRUD (Rust, TDD)

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (structs near `Settings` ~line 53; table in `init()` ~line 93; functions after `clear_account_data` ~line 426; tests in the `#[cfg(test)] mod tests` block)

- [ ] **Step 1: Write the failing tests**

Add these helpers + tests inside `mod tests` in `src-tauri/src/db/mod.rs` (after the existing `clear_account_data_wipes_cache_but_keeps_settings` test, before the closing `}` of `mod tests`):

```rust
    // 🦀 Build a MeetingNoteWrite with given key + body; snapshot fields are filler.
    fn note_write(cal: &str, ev: &str, body: &str) -> MeetingNoteWrite {
        MeetingNoteWrite {
            calendar_id: cal.into(),
            event_id: ev.into(),
            event_title: "Standup".into(),
            event_start: "2026-06-22T09:00:00-07:00".into(),
            body: body.into(),
        }
    }

    #[test]
    fn meeting_note_upsert_inserts_then_updates_same_row() {
        let c = conn();
        let inserted = upsert_meeting_note(&c, &note_write("primary", "e1", "first"), 1000).unwrap();
        assert_eq!(inserted.created_at, 1000);
        assert_eq!(inserted.updated_at, 1000);
        assert_eq!(inserted.body, "first");

        let mut w = note_write("primary", "e1", "second");
        w.event_title = "Standup (edited)".into();
        let updated = upsert_meeting_note(&c, &w, 2000).unwrap();
        assert_eq!(updated.id, inserted.id); // same row, not a second insert
        assert_eq!(updated.created_at, 1000); // preserved on update
        assert_eq!(updated.updated_at, 2000); // refreshed
        assert_eq!(updated.body, "second");
        assert_eq!(updated.event_title, "Standup (edited)"); // snapshot refreshed
        assert_eq!(list_meeting_notes(&c).unwrap().len(), 1); // still exactly one row
    }

    #[test]
    fn meeting_note_get_returns_some_and_none() {
        let c = conn();
        assert!(get_meeting_note(&c, "primary", "missing").unwrap().is_none());
        upsert_meeting_note(&c, &note_write("primary", "e1", "hi"), 1).unwrap();
        let got = get_meeting_note(&c, "primary", "e1").unwrap().unwrap();
        assert_eq!(got.body, "hi");
        // a different calendar with the same event id is a distinct note
        assert!(get_meeting_note(&c, "other", "e1").unwrap().is_none());
    }

    #[test]
    fn list_meeting_notes_orders_by_updated_at_desc() {
        let c = conn();
        upsert_meeting_note(&c, &note_write("primary", "old", "o"), 100).unwrap();
        upsert_meeting_note(&c, &note_write("primary", "new", "n"), 300).unwrap();
        upsert_meeting_note(&c, &note_write("primary", "mid", "m"), 200).unwrap();
        let ids: Vec<String> = list_meeting_notes(&c).unwrap().into_iter().map(|n| n.event_id).collect();
        assert_eq!(ids, vec!["new".to_string(), "mid".to_string(), "old".to_string()]);
    }

    #[test]
    fn delete_meeting_note_removes_only_that_note() {
        let c = conn();
        upsert_meeting_note(&c, &note_write("primary", "a", "a"), 1).unwrap();
        upsert_meeting_note(&c, &note_write("primary", "b", "b"), 2).unwrap();
        delete_meeting_note(&c, "primary", "a").unwrap();
        let ids: Vec<String> = list_meeting_notes(&c).unwrap().into_iter().map(|n| n.event_id).collect();
        assert_eq!(ids, vec!["b".to_string()]);
        assert!(get_meeting_note(&c, "primary", "a").unwrap().is_none());
    }

    #[test]
    fn init_creates_meeting_notes_table_idempotently() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        init(&c).unwrap(); // second init must not error on the existing table
        upsert_meeting_note(&c, &note_write("primary", "e1", "ok"), 1).unwrap();
        assert_eq!(list_meeting_notes(&c).unwrap().len(), 1);
    }
```

- [ ] **Step 2: Run the tests to verify they fail (don't compile)**

Run: `cd src-tauri && cargo test meeting_note 2>&1 | tail -20`
Expected: FAIL — compile errors like `cannot find type MeetingNoteWrite` / `cannot find function upsert_meeting_note`.

- [ ] **Step 3: Add the `MeetingNote` + `MeetingNoteWrite` structs**

In `src-tauri/src/db/mod.rs`, after the `Settings` struct (the `}` ending ~line 53), add:

```rust
// 🦀 A stored meeting note (Serialize → frontend). One row per (calendar_id, event_id).
//    `event_title`/`event_start` are a SNAPSHOT of the event at save time, so the note
//    stays meaningful even if the event is later deleted on Google or falls outside the
//    fetched week. `created_at`/`updated_at` are Unix MILLISECONDS (matches JS Date.now()).
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
}

// 🦀 The save input from the frontend (Deserialize). snake_case field names → the JS side
//    passes `{ calendar_id, event_id, event_title, event_start, body }`. Timestamps are NOT
//    sent from JS — the backend stamps them (see upsert_meeting_note's `now_ms`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MeetingNoteWrite {
    pub calendar_id: String,
    pub event_id: String,
    pub event_title: String,
    pub event_start: String,
    pub body: String,
}
```

- [ ] **Step 4: Add the table to `init()`**

In `init()`, inside the existing `conn.execute_batch("…")?;` call, append the new table to the SQL string immediately after the `settings` table definition (i.e. change the trailing `);` of the `settings` block so the batch also creates `meeting_notes`). The block becomes:

```rust
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
            UNIQUE(calendar_id, event_id)
        );",
    )?;
```

(Only the closing `");` moves down to the end of the new table — no other line in `init()` changes. This is purely additive: `IF NOT EXISTS` makes re-init a no-op, and the messages-specific migration block below it is untouched.)

- [ ] **Step 5: Add the CRUD functions**

In `src-tauri/src/db/mod.rs`, after `clear_account_data` (the `}` ~line 426, before `#[cfg(test)]`), add:

```rust
// 🦀 The column list, in one `const` so get + list read the same shape (DRY).
const NOTE_COLS: &str = "id, calendar_id, event_id, event_title, event_start, body, created_at, updated_at";

// 🦀 Map one meeting_notes row into a MeetingNote. `&rusqlite::Row` borrows the row for the
//    closure; column indices match NOTE_COLS order. Returns rusqlite::Result so it can be
//    handed straight to `query_row`/`query_map` as the row-mapping closure.
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
    })
}

/// Read one note by (calendar_id, event_id), or `None` if there isn't one.
pub fn get_meeting_note(conn: &Connection, calendar_id: &str, event_id: &str) -> Result<Option<MeetingNote>> {
    // 🦀 NOTE_COLS is a compile-time constant (never user input), so formatting it into the
    //    SQL is injection-safe; the actual values are still passed as bound `?` params.
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM meeting_notes WHERE calendar_id = ?1 AND event_id = ?2"
    ))?;
    // 🦀 `.optional()` (OptionalExtension) turns "no rows" into Ok(None) instead of an error.
    let note = stmt.query_row(params![calendar_id, event_id], row_to_note).optional()?;
    Ok(note)
}

/// All notes, most-recently-edited first (drives the Notes panel).
pub fn list_meeting_notes(conn: &Connection) -> Result<Vec<MeetingNote>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {NOTE_COLS} FROM meeting_notes ORDER BY updated_at DESC"
    ))?;
    let rows = stmt.query_map([], row_to_note)?;
    // 🦀 Each item is a rusqlite::Result; `r?` propagates a row error into our Result<Vec<…>>.
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Insert a new note, or update the existing one for this (calendar_id, event_id). `now_ms`
/// is the caller's clock (Unix ms): it sets `updated_at` always and `created_at` only on the
/// initial insert (preserved on update). The snapshot (title/start) is refreshed each save.
pub fn upsert_meeting_note(conn: &Connection, w: &MeetingNoteWrite, now_ms: i64) -> Result<MeetingNote> {
    // 🦀 `?6` is reused for BOTH created_at and updated_at on insert. ON CONFLICT updates
    //    updated_at (= excluded.updated_at = ?6) but NOT created_at — so created_at keeps
    //    its first-insert value while updated_at moves forward. `excluded` is the row that
    //    WOULD have been inserted; it's how SQLite exposes the new values inside DO UPDATE.
    conn.execute(
        "INSERT INTO meeting_notes
            (calendar_id, event_id, event_title, event_start, body, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(calendar_id, event_id) DO UPDATE SET
            event_title = excluded.event_title,
            event_start = excluded.event_start,
            body = excluded.body,
            updated_at = excluded.updated_at",
        params![w.calendar_id, w.event_id, w.event_title, w.event_start, w.body, now_ms],
    )?;
    // 🦀 Re-read the stored row so the caller gets the real id + preserved created_at. The row
    //    must exist now, so `None` here is a genuine bug — surface it loudly rather than panic.
    get_meeting_note(conn, &w.calendar_id, &w.event_id)?
        .ok_or_else(|| crate::error::AppError::Other("meeting note vanished after upsert".into()))
}

/// Delete the note for (calendar_id, event_id). A missing note is a silent no-op (0 rows).
pub fn delete_meeting_note(conn: &Connection, calendar_id: &str, event_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM meeting_notes WHERE calendar_id = ?1 AND event_id = ?2",
        params![calendar_id, event_id],
    )?;
    Ok(())
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test meeting_note 2>&1 | tail -20`
Expected: PASS — `meeting_note_upsert_inserts_then_updates_same_row`, `meeting_note_get_returns_some_and_none`, `list_meeting_notes_orders_by_updated_at_desc`, `delete_meeting_note_removes_only_that_note`, `init_creates_meeting_notes_table_idempotently` all green. Then run the full suite + lint:
Run: `cd src-tauri && cargo test 2>&1 | tail -15 && cargo clippy --all-targets 2>&1 | tail -15`
Expected: all tests pass; clippy clean (no warnings).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "$(cat <<'EOF'
feat(m20): meeting_notes table + CRUD (upsert/get/list/delete)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** SQLite "upsert" = `INSERT … ON CONFLICT(<unique cols>) DO UPDATE SET …`; the `excluded` pseudo-table holds the values you tried to insert, so listing a column there overwrites it while omitting one (here `created_at`) preserves the stored value. `.optional()` turns a not-found row into `Ok(None)`, and reusing one bound param (`?6`) for two columns avoids passing the timestamp twice.

---

## Task 2: Tauri commands + registration (Rust)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `now_millis` near `now_secs` ~line 216; add four commands near the other DB-backed commands, e.g. after `delete_calendar_event` ~line 784)
- Modify: `src-tauri/src/lib.rs` (`invoke_handler` list ~line 94–125)

There is no unit-test harness for Tauri commands in this repo (DB logic is already covered in Task 1; `commands.rs` only unit-tests the pure `to_rows`). Verification for this task is **compile + clippy + existing tests green**.

- [ ] **Step 1: Add the `now_millis` helper**

In `src-tauri/src/commands.rs`, right after `now_secs()` (~line 216), add:

```rust
// 🦀 Current Unix time in MILLISECONDS — meeting-note timestamps use the same unit as the
//    JS `Date.now()` the frontend formats with. `as i64` is safe for any real wall-clock time.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 2: Add the four commands**

In `src-tauri/src/commands.rs`, after `delete_calendar_event` (~line 784, before `#[cfg(test)]`), add:

```rust
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
    db::get_meeting_note(&conn, &calendar_id, &event_id)
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
    // 🦀 The backend stamps the timestamp; the frontend never sends one.
    db::upsert_meeting_note(&conn, &note, now_millis())
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
    db::delete_meeting_note(&conn, &calendar_id, &event_id)
}

/// List all meeting notes, most-recently-edited first (drives the Notes panel).
#[tauri::command]
pub async fn list_meeting_notes(state: tauri::State<'_, Db>) -> Result<Vec<db::MeetingNote>> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::list_meeting_notes(&conn)
}
```

- [ ] **Step 3: Register the commands in `lib.rs`**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]`, add these four lines after `commands::delete_calendar_event,` (~line 121):

```rust
            commands::get_meeting_note,
            commands::save_meeting_note,
            commands::delete_meeting_note,
            commands::list_meeting_notes,
```

- [ ] **Step 4: Verify compile + lint + tests**

Run: `cd src-tauri && cargo test 2>&1 | tail -15 && cargo clippy --all-targets 2>&1 | tail -15`
Expected: builds, all tests pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(m20): meeting-note commands (get/save/delete/list) + register

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** Tauri commands that touch the DB take `state: tauri::State<'_, Db>` and call `.lock()`; because these do no network `.await`, holding the `MutexGuard` is fine (the rule we follow elsewhere is only "don't hold a std `MutexGuard` across `.await`"). Tauri also maps snake_case command params to camelCase on the JS side, which is why the wrappers in Task 3 pass `{ calendarId, eventId }`.

---

## Task 3: Frontend wrappers, types, mock store (TypeScript)

**Files:**
- Create: `src/lib/notes.ts`
- Modify: `src/lib/mock.ts` (add the mock note store + seed; near the other event mocks ~line 169)

- [ ] **Step 1: Create `src/lib/notes.ts`**

```ts
// src/lib/notes.ts — meeting-note API wrappers + types. Notes are LOCAL-only (no Google).
// Every wrapper is isTauri()-gated so the browser maket runs against an in-memory mock store.
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  mockGetMeetingNote,
  mockSaveMeetingNote,
  mockDeleteMeetingNote,
  mockListMeetingNotes,
} from "./mock";

export interface MeetingNote {
  id: number;
  calendar_id: string;
  event_id: string;
  event_title: string;
  event_start: string;
  body: string;
  /** Unix milliseconds. */
  created_at: number;
  /** Unix milliseconds. */
  updated_at: number;
}

// The save payload — snake_case keys to match the Rust MeetingNoteWrite (serde default).
export interface MeetingNoteWrite {
  calendar_id: string;
  event_id: string;
  event_title: string;
  event_start: string;
  body: string;
}

/** Stable composite key for the "has-notes" Set + lookups (a pipe never appears in calendar/event ids). */
export function noteKey(calendarId: string, eventId: string): string {
  return `${calendarId}|${eventId}`;
}

export const getMeetingNote = (calendarId: string, eventId: string): Promise<MeetingNote | null> =>
  isTauri()
    ? invoke<MeetingNote | null>("get_meeting_note", { calendarId, eventId })
    : Promise.resolve(mockGetMeetingNote(calendarId, eventId));

export const saveMeetingNote = (note: MeetingNoteWrite): Promise<MeetingNote> =>
  isTauri()
    ? invoke<MeetingNote>("save_meeting_note", { note })
    : Promise.resolve(mockSaveMeetingNote(note));

export const deleteMeetingNote = (calendarId: string, eventId: string): Promise<void> =>
  isTauri()
    ? invoke<void>("delete_meeting_note", { calendarId, eventId })
    : Promise.resolve(mockDeleteMeetingNote(calendarId, eventId));

export const listMeetingNotes = (): Promise<MeetingNote[]> =>
  isTauri() ? invoke<MeetingNote[]>("list_meeting_notes") : Promise.resolve(mockListMeetingNotes());
```

- [ ] **Step 2: Add the mock note store to `src/lib/mock.ts`**

First extend the existing top-of-file type import (line 5) to also pull the note types **as types only** (keeps `mock → notes` type-only, avoiding a runtime import cycle — same discipline as the existing `mock → api` import):

Change line 5 from:
```ts
import type { MessagePreview, SyncSummary, DraftContent, Label, MessageBody, Attachment, ReplyContext, EventWrite, CalendarSummary } from "./api";
```
to add a second type-only import line right below it:
```ts
import type { MeetingNote, MeetingNoteWrite } from "./notes";
```

Then append the store + seed at the end of `src/lib/mock.ts`:

```ts
// --- Meeting notes (M20) -----------------------------------------------------
// In-memory note store for the browser maket. Keyed by `${calendar_id}|${event_id}`
// inline (NOT importing notes.ts's noteKey, to keep this module's import of notes.ts
// type-only — mirrors how mock.ts imports api.ts). Seeded with two notes on events that
// mockCalendarWeek always produces (e2 "1:1 with Dana", e6 "Roadmap"), so the panel + dots
// are demoable on any visible week.
const mockNoteKey = (calendarId: string, eventId: string) => `${calendarId}|${eventId}`;

const MOCK_NOTES = new Map<string, MeetingNote>([
  [
    mockNoteKey("primary", "e2"),
    {
      id: 1, calendar_id: "primary", event_id: "e2",
      event_title: "1:1 with Dana", event_start: "2026-06-22T14:00:00-07:00",
      body: "- Career growth check-in\n- Reviewed Q3 priorities\n- Action: share the roadmap doc",
      created_at: 1_750_000_000_000, updated_at: 1_750_000_200_000,
    },
  ],
  [
    mockNoteKey("primary", "e6"),
    {
      id: 2, calendar_id: "primary", event_id: "e6",
      event_title: "Roadmap", event_start: "2026-06-25T10:00:00-07:00",
      body: "Draft milestones for H2. Decide M21 scope next.",
      created_at: 1_750_000_000_000, updated_at: 1_750_000_100_000,
    },
  ],
]);

let mockNoteId = 100; // fresh ids for newly-created mock notes

export function mockGetMeetingNote(calendarId: string, eventId: string): MeetingNote | null {
  return MOCK_NOTES.get(mockNoteKey(calendarId, eventId)) ?? null;
}

export function mockSaveMeetingNote(w: MeetingNoteWrite): MeetingNote {
  const key = mockNoteKey(w.calendar_id, w.event_id);
  const now = 1_750_000_500_000; // fixed clock (no Date.now in maket data, keeps it deterministic)
  const existing = MOCK_NOTES.get(key);
  const note: MeetingNote = {
    id: existing?.id ?? mockNoteId++,
    calendar_id: w.calendar_id,
    event_id: w.event_id,
    event_title: w.event_title,
    event_start: w.event_start,
    body: w.body,
    created_at: existing?.created_at ?? now,
    updated_at: now,
  };
  MOCK_NOTES.set(key, note);
  return note;
}

export function mockDeleteMeetingNote(calendarId: string, eventId: string): void {
  MOCK_NOTES.delete(mockNoteKey(calendarId, eventId));
}

export function mockListMeetingNotes(): MeetingNote[] {
  return [...MOCK_NOTES.values()].sort((a, b) => b.updated_at - a.updated_at);
}
```

- [ ] **Step 3: Verify the build (type-check)**

Run: `npm run build 2>&1 | tail -20`
Expected: clean build (TypeScript compiles, Vite bundles). No "circular"/"cannot find" errors.

- [ ] **Step 4: Commit**

```bash
git add src/lib/notes.ts src/lib/mock.ts
git commit -m "$(cat <<'EOF'
feat(m20): note api wrappers + noteKey + maket mock store

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `NotesModal` editor (React/TypeScript)

**Files:**
- Create: `src/components/NotesModal.tsx`

- [ ] **Step 1: Create `src/components/NotesModal.tsx`**

```tsx
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { getMeetingNote, saveMeetingNote, deleteMeetingNote } from "../lib/notes";

// What the editor needs to open: the event identity + a title/start snapshot to store.
export interface NoteTarget {
  calendarId: string;
  eventId: string;
  eventTitle: string;
  eventStart: string;
}

export function NotesModal({
  target,
  onClose,
  onSaved,
}: {
  target: NoteTarget;
  onClose: () => void;
  onSaved: () => void; // reload the panel + dots
}) {
  const [body, setBody] = useState("");
  const [exists, setExists] = useState(false); // a note already stored → show Delete
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Esc closes (matches EventModal/ComposeModal — window listener, no backdrop close).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Load any existing note for this event on open.
  useEffect(() => {
    let cancelled = false;
    getMeetingNote(target.calendarId, target.eventId)
      .then((n) => {
        if (cancelled) return;
        setBody(n?.body ?? "");
        setExists(!!n);
        setLoading(false);
      })
      .catch((e) => {
        if (!cancelled) {
          setError(String(e));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [target.calendarId, target.eventId]);

  async function handleSave() {
    if (body.trim() === "") return; // Save is disabled when empty; guard regardless
    setBusy(true);
    setError(null);
    try {
      await saveMeetingNote({
        calendar_id: target.calendarId,
        event_id: target.eventId,
        event_title: target.eventTitle,
        event_start: target.eventStart,
        body,
      });
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (!window.confirm("Delete this note?")) return;
    setBusy(true);
    setError(null);
    try {
      await deleteMeetingNote(target.calendarId, target.eventId);
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="compose-overlay">
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="note-title">
        <div className="compose-head">
          <span className="compose-title" id="note-title">Notes — {target.eventTitle}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="note-when">{new Date(target.eventStart).toLocaleString()}</div>
        {loading ? (
          <div className="cal-loading">Loading…</div>
        ) : (
          <textarea
            className="compose-body"
            placeholder="Write meeting notes…"
            value={body}
            onChange={(e) => setBody(e.target.value)}
            rows={12}
            autoFocus
          />
        )}
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {exists && (
            <button className="btn btn-danger-outline" onClick={handleDelete} disabled={busy}>
              Delete
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            className="btn btn-accent"
            onClick={handleSave}
            disabled={busy || body.trim() === ""}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Verify the build**

Run: `npm run build 2>&1 | tail -20`
Expected: clean build (the component compiles; it's not yet rendered anywhere — that's Task 5).

- [ ] **Step 3: Commit**

```bash
git add src/components/NotesModal.tsx
git commit -m "$(cat <<'EOF'
feat(m20): NotesModal editor (load/save/delete, empty-disabled save)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Wire notes into CalendarView + WeekGrid + styles (React/TypeScript)

**Files:**
- Modify: `src/components/WeekGrid.tsx` (add `notesByKey` prop + dot)
- Modify: `src/components/CalendarView.tsx` (load notes, panel, popover button, host `NotesModal`)
- Modify: `src/styles/app.css` (dot, drawer, rows)

- [ ] **Step 1: Add the `notesByKey` prop + dot to `WeekGrid.tsx`**

Replace the entire contents of `src/components/WeekGrid.tsx` with:

```tsx
import { useEffect, useRef } from "react";
import {
  type CalendarEvent,
  weekDays,
  splitAllDay,
  layoutDay,
  allDayOnDay,
} from "../lib/calendar";
import { noteKey } from "../lib/notes";

const PX_PER_MIN = 0.8;                       // 48px / hour
const GRID_HEIGHT = 24 * 60 * PX_PER_MIN;     // 1152px
const HOURS = Array.from({ length: 24 }, (_, h) => h);

function hourLabel(h: number): string {
  if (h === 0) return "12 AM";
  if (h === 12) return "12 PM";
  return h < 12 ? `${h} AM` : `${h - 12} PM`;
}

function fmtTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

function sameLocalDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** Use the calendar's color for the left border only; the faint fill comes from CSS. */
function tint(e: CalendarEvent): React.CSSProperties {
  return e.color ? { borderLeftColor: e.color } : {};
}

export function WeekGrid({
  weekStart,
  events,
  now,
  notesByKey,
  onSlotClick,
  onEventClick,
}: {
  weekStart: Date;
  events: CalendarEvent[];
  now: Date;
  notesByKey?: Set<string>;
  onSlotClick?: (at: Date) => void;
  onEventClick?: (ev: CalendarEvent) => void;
}) {
  const days = weekDays(weekStart);
  const { allDay, timed } = splitAllDay(events);
  const scrollRef = useRef<HTMLDivElement>(null);
  const hasNote = (e: CalendarEvent) => !!notesByKey?.has(noteKey(e.calendar_id, e.id));

  // Scroll so ~7 AM is at the top on mount / week change.
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 7 * 60 * PX_PER_MIN;
  }, [weekStart]);

  const weekEndExclusive = new Date(days[6].getFullYear(), days[6].getMonth(), days[6].getDate() + 1);
  const nowInWeek = now >= days[0] && now < weekEndExclusive;
  const nowTopMin = now.getHours() * 60 + now.getMinutes();

  return (
    <div className="cal-grid">
      <div className="cal-dayhead-row">
        <div className="cal-gutter-cell" />
        {days.map((d) => (
          <div key={d.toISOString()} className={sameLocalDay(d, now) ? "cal-dayhead today" : "cal-dayhead"}>
            <span className="cal-dow">{d.toLocaleString("en-US", { weekday: "short" })}</span>
            <span className="cal-daynum">{d.getDate()}</span>
          </div>
        ))}
      </div>

      {allDay.length > 0 && (
        <div className="cal-allday-row">
          <div className="cal-gutter-cell cal-allday-label">all-day</div>
          {days.map((d) => (
            <div key={d.toISOString()} className="cal-allday-cell">
              {allDay.filter((e) => allDayOnDay(e, d)).map((e) => (
                <div key={e.id} className="cal-allday-ev" style={tint(e)} title={e.title} onClick={() => onEventClick?.(e)}>
                  {hasNote(e) && <span className="cal-ev-note-dot" aria-label="Has notes" />}
                  {e.title}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}

      <div className="cal-scroll" ref={scrollRef}>
        <div className="cal-body" style={{ height: GRID_HEIGHT }}>
          <div className="cal-gutter">
            {HOURS.map((h) => (
              <div key={h} className="cal-hour" style={{ top: h * 60 * PX_PER_MIN }}>
                {hourLabel(h)}
              </div>
            ))}
          </div>
          {days.map((d) => {
            const positioned = layoutDay(timed, d);
            return (
              <div key={d.toISOString()} className="cal-col"
                onClick={(e) => {
                  if (!onSlotClick) return;
                  const rect = e.currentTarget.getBoundingClientRect();
                  const min = Math.max(0, Math.round((e.clientY - rect.top) / PX_PER_MIN));
                  const at = new Date(d.getFullYear(), d.getMonth(), d.getDate(), Math.floor(min / 60), 0, 0);
                  onSlotClick(at);
                }}
              >
                {HOURS.map((h) => (
                  <div key={h} className="cal-hourline" style={{ top: h * 60 * PX_PER_MIN }} />
                ))}
                {positioned.map((p) => (
                  <div
                    key={p.ev.id}
                    className="cal-ev"
                    onClick={(e) => { e.stopPropagation(); onEventClick?.(p.ev); }}
                    title={`${p.ev.title} · ${fmtTime(p.ev.start)}`}
                    style={{
                      top: p.topMin * PX_PER_MIN,
                      height: Math.max(14, p.heightMin * PX_PER_MIN - 2),
                      left: `calc(${(p.lane / p.lanes) * 100}% + 2px)`,
                      width: `calc(${100 / p.lanes}% - 4px)`,
                      ...tint(p.ev),
                    }}
                  >
                    {hasNote(p.ev) && <span className="cal-ev-note-dot" aria-label="Has notes" />}
                    <span className="cal-ev-title">{p.ev.title}</span>
                    <span className="cal-ev-time">{fmtTime(p.ev.start)}</span>
                  </div>
                ))}
                {nowInWeek && sameLocalDay(d, now) && (
                  <div className="cal-nowline" style={{ top: nowTopMin * PX_PER_MIN }} />
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Wire notes into `CalendarView.tsx`**

Replace the entire contents of `src/components/CalendarView.tsx` with:

```tsx
import { useEffect, useMemo, useState } from "react";
import { type CalendarEvent, toTimeMinMax } from "../lib/calendar";
import { fetchCalendarWeek, connectGmail, listCalendars, type CalendarSummary } from "../lib/api";
import { listMeetingNotes, noteKey, type MeetingNote } from "../lib/notes";
import { WeekGrid } from "./WeekGrid";
import { EventModal, type EventInitial } from "./EventModal";
import { NotesModal, type NoteTarget } from "./NotesModal";
import { NotebookPen } from "lucide-react";

// The backend maps a missing calendar scope to the specific message
// "Calendar access not granted — reconnect Google to enable it." Match that phrasing
// precisely so an unrelated error that merely mentions "permission" isn't misrouted here.
function isScopeError(msg: string): boolean {
  return /reconnect google|calendar access not granted/i.test(msg);
}

export function CalendarView({ weekStart }: { weekStart: Date }) {
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState<Date>(() => new Date());
  const [reloadKey, setReloadKey] = useState(0);
  const [calendars, setCalendars] = useState<CalendarSummary[]>([]);
  const [modal, setModal] = useState<EventInitial | null>(null);
  const [detail, setDetail] = useState<CalendarEvent | null>(null);

  // Meeting notes (M20): local-only, separate from the live calendar fetch.
  const [notes, setNotes] = useState<MeetingNote[]>([]);
  const [notesReloadKey, setNotesReloadKey] = useState(0);
  const [noteTarget, setNoteTarget] = useState<NoteTarget | null>(null);
  const [notesPanelOpen, setNotesPanelOpen] = useState(false);

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(id);
  }, []);

  // Load the writable calendars for the create form's picker (on mount + after each
  // save/reconnect via reloadKey). Silent on failure — the form falls back to "primary".
  useEffect(() => {
    listCalendars().then(setCalendars).catch(() => setCalendars([]));
  }, [reloadKey]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const { timeMin, timeMax } = toTimeMinMax(weekStart);
    fetchCalendarWeek(timeMin, timeMax)
      .then((evts) => { if (!cancelled) { setEvents(evts); setLoading(false); } })
      .catch((e) => { if (!cancelled) { setError(String(e)); setLoading(false); } });
    return () => { cancelled = true; };
  }, [weekStart, reloadKey]);

  // Load all notes (on mount + after any save/delete). Silent on failure.
  useEffect(() => {
    listMeetingNotes().then(setNotes).catch(() => setNotes([]));
  }, [notesReloadKey]);

  // Set of `${calendar_id}|${event_id}` keys for the week grid's has-notes dot.
  const notesByKey = useMemo(
    () => new Set(notes.map((n) => noteKey(n.calendar_id, n.event_id))),
    [notes],
  );

  async function handleReconnect() {
    setError(null);
    setLoading(true);
    try {
      await connectGmail();
      setReloadKey((k) => k + 1);
    } catch (e) {
      setError(String(e));
      setLoading(false);
    }
  }

  const refetch = () => setReloadKey((k) => k + 1);
  const reloadNotes = () => setNotesReloadKey((k) => k + 1);
  const openNew = (startAt?: Date) => setModal({ calendars, startAt });
  const openEdit = (ev: CalendarEvent) => { setDetail(null); setModal({ calendars, event: ev }); };
  const openNotesForEvent = (ev: CalendarEvent) => {
    setDetail(null);
    setNoteTarget({ calendarId: ev.calendar_id, eventId: ev.id, eventTitle: ev.title, eventStart: ev.start });
  };
  const openNotesForNote = (n: MeetingNote) =>
    setNoteTarget({ calendarId: n.calendar_id, eventId: n.event_id, eventTitle: n.event_title, eventStart: n.event_start });

  if (error && isScopeError(error)) {
    return (
      <div className="cal-view">
        <div className="cal-empty">
          <p>Ember needs permission to manage your Google Calendar.</p>
          <button className="btn btn-accent" onClick={handleReconnect}>Reconnect Google</button>
        </div>
      </div>
    );
  }
  if (error) {
    return (
      <div className="cal-view">
        <div className="cal-empty">
          <pre className="error-text">{error}</pre>
          <button className="btn" onClick={refetch}>Retry</button>
        </div>
      </div>
    );
  }

  return (
    <div className="cal-view">
      <div className="cal-toolbar">
        <button className="btn btn-accent" onClick={() => openNew()}>New event</button>
        <button
          className={notesPanelOpen ? "btn btn-toggle active" : "btn btn-toggle"}
          aria-pressed={notesPanelOpen}
          onClick={() => setNotesPanelOpen((o) => !o)}
        >
          <NotebookPen size={15} /> Notes
        </button>
      </div>
      <div className="cal-stage">
        {loading ? (
          <div className="cal-loading">Loading your week…</div>
        ) : (
          <WeekGrid
            weekStart={weekStart}
            events={events}
            now={now}
            notesByKey={notesByKey}
            onSlotClick={openNew}
            onEventClick={setDetail}
          />
        )}

        {notesPanelOpen && (
          <aside className="notes-drawer" aria-label="Meeting notes">
            <div className="notes-drawer-head">Meeting notes</div>
            {notes.length === 0 ? (
              <div className="notes-empty">No meeting notes yet.</div>
            ) : (
              <ul className="notes-list">
                {notes.map((n) => (
                  <li key={n.id}>
                    <button className="notes-row" onClick={() => openNotesForNote(n)}>
                      <span className="notes-row-title">{n.event_title || "(untitled event)"}</span>
                      <span className="notes-row-date">{new Date(n.event_start).toLocaleDateString()}</span>
                      <span className="notes-row-snippet">{n.body.split("\n")[0]}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </aside>
        )}
      </div>

      {detail && (
        <div className="event-detail-overlay" onClick={() => setDetail(null)}>
          <div className="event-detail" role="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>{detail.title}</h3>
            <div className="event-detail-when">
              {new Date(detail.start).toLocaleString()} – {new Date(detail.end).toLocaleString()}
            </div>
            {detail.location && <div>{detail.location}</div>}
            {detail.description && <p className="event-detail-desc">{detail.description}</p>}
            {detail.attendees && detail.attendees.length > 0 && (
              <div className="event-detail-guests">Guests: {detail.attendees.join(", ")}</div>
            )}
            {detail.meet_link && (
              <a className="event-meet" href={detail.meet_link} target="_blank" rel="noreferrer">{detail.meet_link}</a>
            )}
            <div className="compose-actions">
              <button className="btn" onClick={() => setDetail(null)}>Close</button>
              <button className="btn" onClick={() => openNotesForEvent(detail)}>
                <NotebookPen size={15} /> Notes
              </button>
              <button className="btn btn-accent" onClick={() => openEdit(detail)}>Edit</button>
            </div>
          </div>
        </div>
      )}

      {modal && (
        <EventModal initial={modal} onClose={() => setModal(null)} onSaved={refetch} />
      )}

      {noteTarget && (
        <NotesModal target={noteTarget} onClose={() => setNoteTarget(null)} onSaved={reloadNotes} />
      )}
    </div>
  );
}
```

- [ ] **Step 3: Add the styles to `src/styles/app.css`**

Append to the end of `src/styles/app.css`:

```css
/* M20 meeting notes */
.cal-stage {
  position: relative;
  display: flex;
  align-items: stretch;
}
.cal-stage .cal-grid {
  flex: 1 1 auto;
  min-width: 0;
}
.cal-ev-note-dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-right: 4px;
  border-radius: 50%;
  background: var(--accent, #16a34a);
  vertical-align: middle;
}
.btn-toggle.active {
  background: var(--accent, #16a34a);
  color: #fff;
}
.notes-drawer {
  flex: 0 0 280px;
  border-left: 1px solid var(--border, #2a2a2a);
  padding: 8px 10px;
  overflow-y: auto;
  max-height: 70vh;
}
.notes-drawer-head {
  font-weight: 600;
  margin-bottom: 8px;
}
.notes-empty {
  color: var(--muted, #888);
  font-size: 13px;
  padding: 8px 2px;
}
.notes-list {
  list-style: none;
  margin: 0;
  padding: 0;
}
.notes-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 100%;
  text-align: left;
  background: none;
  border: none;
  border-radius: 6px;
  padding: 8px;
  cursor: pointer;
  color: inherit;
}
.notes-row:hover {
  background: var(--hover, rgba(255, 255, 255, 0.06));
}
.notes-row-title {
  font-weight: 600;
  font-size: 13px;
}
.notes-row-date {
  font-size: 11px;
  color: var(--muted, #888);
}
.notes-row-snippet {
  font-size: 12px;
  color: var(--muted, #888);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.note-when {
  font-size: 12px;
  color: var(--muted, #888);
  margin-bottom: 6px;
}
```

(If the CSS variables `--accent`/`--border`/`--muted`/`--hover` don't exist in `theme.css`, the fallbacks after the comma apply — verify the dot/drawer are visible in Step 5 and adjust to match the existing variable names in `src/styles/theme.css` if needed.)

- [ ] **Step 4: Verify the build**

Run: `npm run build 2>&1 | tail -20`
Expected: clean build.

- [ ] **Step 5: Maket visual check**

Run the dev server and view in a browser (it defaults to the Calendar view in maket mode):
Run: `npm run dev` (then open the printed localhost URL; stop with Ctrl-C when done)
Verify:
1. The week grid shows a small green dot on "1:1 with Dana" and "Roadmap" (the seeded notes).
2. Clicking the toolbar **Notes** toggle opens a right-hand drawer listing both seeded notes (newest-edited first: "1:1 with Dana" above "Roadmap").
3. Clicking a drawer row opens the editor with the stored body; **Save** is enabled, **Delete** is shown.
4. Clicking an event → the detail popover now has a **Notes** button; it opens the editor (empty body for an event with no note → **Save** disabled until you type, no **Delete** button).

- [ ] **Step 6: Commit**

```bash
git add src/components/WeekGrid.tsx src/components/CalendarView.tsx src/styles/app.css
git commit -m "$(cat <<'EOF'
feat(m20): wire notes into calendar — dot, drawer, popover button

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Final verification gates

**Files:** none (verification only)

- [ ] **Step 1: Full backend gate**

Run: `cd src-tauri && cargo test 2>&1 | tail -20 && cargo clippy --all-targets 2>&1 | tail -20`
Expected: all tests pass (including the five new `meeting_note*` tests); clippy reports no warnings. (Do NOT run `cargo fmt`.)

- [ ] **Step 2: Full frontend gate**

Run: `npm run build 2>&1 | tail -20`
Expected: TypeScript type-checks and Vite builds with no errors.

- [ ] **Step 3: Confirm clean tree + review the diff**

Run: `git status -s && git log --oneline main..HEAD`
Expected: working tree clean; five M20 commits (Tasks 1–5) on the `m20-meeting-notes` branch atop the spec commit.

- [ ] **Step 4 (optional, owner): live persistence check**

In the real Tauri dev build (`npm run tauri dev`), open an event → Notes → type → Save; reload the app and confirm the dot + note persist. This is genuinely testable locally (notes never touch Google), unlike the Google E2E paths which remain owner-pending.

---

## Notes for the executor

- **No DB migration / no ALTER** — the new table is purely additive in `init()`; the messages migration block is untouched. The global Flyway `db-migrations` rule and `@db-migration-reviewer` do **not** apply (this is rusqlite).
- **Reviewers are READ-ONLY** — any code-review subagent prompt must forbid Edit/Write and any git change ("REPORT ONLY"); run `git status -s` after each review (a prior milestone had a reviewer leave a rogue edit).
- **Deferred (not this milestone):** Ollama `summary` column, `transcript` column, markdown rendering, multiple notes per meeting, note search/export. Each AI column lands later via the existing `add_column_if_missing` helper.
```
