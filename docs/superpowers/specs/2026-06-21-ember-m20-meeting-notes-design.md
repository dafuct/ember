# Ember — Milestone 20: Meeting notes (storage foundation) — Design Spec

**Status:** Approved design (2026-06-21). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Give Ember **local meeting notes** — one editable, plain-text note document per Google Calendar event. This is the **second step** of the "calendar & meeting notes" feature area (M19 calendar write → **M20 meeting-notes storage** → later: local Ollama summarization → meeting transcription capture). M20 delivers the storage foundation + the note-taking UI; the AI pieces come in later milestones and bolt onto the same row via additive columns. **New local SQLite table** (first new table since the core schema), added purely additively. **No new OAuth scope, no Google API calls** — notes are local-only user data and never sync back to Google.

**Architecture in one paragraph:** A new `meeting_notes` SQLite table stores one note doc per event, keyed by `UNIQUE(calendar_id, event_id)` with a denormalized snapshot (`event_title`, `event_start`) so a note stays meaningful when its event is in the past, outside the fetched week, or later deleted on Google. Four DB-only Tauri commands (`get` / `save`-upsert / `delete` / `list`) sit beside the existing commands; **none touch the Calendar API**. The frontend adds a `NotesModal` editor (reusing the M8/M19 modal pattern), opened from the **M19 event-detail popover** (a "Notes" button) and from a new **collapsible Notes panel inside `CalendarView`** that lists meetings-with-notes (newest-edited first, built entirely from the local snapshot). A small **"has-notes" dot** marks events on the week grid. Every save/delete bumps a `notesReloadKey` so the panel + dots refresh.

**Tech Stack:** Rust (rusqlite, serde, Tauri 2), React 19 + TypeScript + Vite, lucide-react. No new dependencies.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept (here: SQLite `ON CONFLICT` upsert, `Option` returns, `SystemTime` → millis, `#[serde]` row mapping), not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M19 are merged to `main`. Ember reads/classifies/mutates/sends/forwards mail, has settings + onboarding, a Google Calendar week view that is now **writable** (M19: create/edit/delete events + meetings with guests + Google Meet), search, folders, notifications, drafts, batch actions + undo, labels, and attachments. M19 added the `calendar.events` scope, `CalendarClient` create/update/delete, the normalized `CalendarEvent` shape (with `description`/`meet_link`/`html_link`/`attendees`), and an **event-detail popover** + `EventModal`. M19's spec explicitly named meeting-notes storage as the **next** step and deferred it. M20 delivers exactly that storage foundation.

**Reuse map:** the M8/M19 **modal pattern** (`role="dialog"`, window-Esc close, no backdrop-close — see `ComposeModal`/`EventModal`); the M19 **event-detail popover** in `WeekGrid` (add a "Notes" entry point); the M19 **`reloadKey`** refetch pattern in `CalendarView` (mirror it as `notesReloadKey`); the `isTauri()` **mock seam** + `lib/mock.ts`; the **`db::init()` additive pattern** (the same `execute_batch` + `CREATE TABLE IF NOT EXISTS` that M6 extended, plus the in-memory `Connection` test style in `db/mod.rs`); the **`SystemTime`** timestamp trick (M17 boundary / M19 Meet `requestId`).

---

## Scope

**In scope (v1):**
- A local **`meeting_notes` SQLite table** (additive `CREATE TABLE IF NOT EXISTS` in `db::init()`): one editable plain-text note doc per calendar event, keyed by `UNIQUE(calendar_id, event_id)`, with a denormalized snapshot (`event_title`, `event_start`) + backend-stamped `created_at`/`updated_at`.
- **DB CRUD** in `db/mod.rs`: `upsert_meeting_note`, `get_meeting_note`, `list_meeting_notes`, `delete_meeting_note`.
- **Four commands** (DB-only; no Google, no auth, no network): `get_meeting_note(calendar_id, event_id)`, `save_meeting_note(MeetingNoteWrite)` (upsert), `delete_meeting_note(calendar_id, event_id)`, `list_meeting_notes()`.
- **`NotesModal`** editor: plain-text body, **Save** (disabled when body is empty/whitespace → no ghost rows), **Cancel**, **Delete** (shown only when a note already exists, with a confirm).
- A collapsible **Notes panel inside `CalendarView`** (browse list, newest-edited first) + a **has-notes dot** on `WeekGrid` event cells + a **"Notes" button** in the M19 event-detail popover.
- **`isTauri()`-gated mocks** (in-memory store) so the panel + editor work in the browser maket.

**Explicitly deferred (later milestones in this arc):**
- **Local Ollama summarization** — a `summary` column added then (via the additive `add_column` helper). **Meeting transcription capture** — a `transcript` column added then. (M20's single-row-per-meeting shape is chosen precisely so these become sibling columns.)
- **Markdown / rich-text rendering** — the body is a format-agnostic plain string in v1.
- **Multiple note entries per meeting** (chose a single doc per meeting); **notes on non-calendar / ad-hoc meetings** (the anchor is always a calendar event); **note search**; **export**; **cross-device sync** (notes are local-only; Gmail/Calendar are the synced sources).

---

## Components

### Backend — `db::init()` (new table, additive)
Appended to the existing `execute_batch` in `init()`:

```sql
CREATE TABLE IF NOT EXISTS meeting_notes (
    id          INTEGER PRIMARY KEY,
    calendar_id TEXT NOT NULL,
    event_id    TEXT NOT NULL,
    event_title TEXT NOT NULL DEFAULT '',  -- snapshot: survives event deletion / out-of-week
    event_start TEXT NOT NULL DEFAULT '',  -- snapshot (RFC3339), for the panel's date display
    body        TEXT NOT NULL,             -- plain text; Save disabled when empty, so never ''
    created_at  INTEGER NOT NULL,          -- unix millis, backend-stamped on insert
    updated_at  INTEGER NOT NULL,          -- unix millis, backend-stamped every save
    UNIQUE(calendar_id, event_id)          -- one note doc per meeting → enables ON CONFLICT upsert
);
```
No `ALTER` on existing tables, no data migration; `IF NOT EXISTS` makes re-init a no-op (idempotent, exercised by an `init`-twice test). This follows the project's own rusqlite pattern (the same `init()` M6 extended). **The global Flyway `db-migrations` rule and `@db-migration-reviewer` are for Flyway/Java projects and do NOT apply here** (this is rusqlite with the project's additive `init` convention) — flagged so it's explicit.

### Backend — types (in `db/mod.rs`)
Placed beside the CRUD functions, matching the project's "all DB lives in `db/mod.rs`" convention. (If isolation is preferred during implementation, a `db/notes.rs` submodule is an acceptable equivalent — same types/functions, re-exported.)
- `MeetingNote` (`#[derive(Serialize)]` → frontend; `PartialEq` for tests): `{ id: i64, calendar_id: String, event_id: String, event_title: String, event_start: String, body: String, created_at: i64, updated_at: i64 }`.
- `MeetingNoteWrite` (`#[derive(Deserialize)]` from JS, snake_case fields): `{ calendar_id: String, event_id: String, event_title: String, event_start: String, body: String }`. The backend stamps `created_at`/`updated_at` from `SystemTime` (millis since epoch) — **the frontend never sends timestamps.**

### Backend — DB layer (`db/mod.rs`)
- `upsert_meeting_note(conn: &Connection, w: &MeetingNoteWrite) -> Result<MeetingNote>` — `INSERT INTO meeting_notes (calendar_id, event_id, event_title, event_start, body, created_at, updated_at) VALUES (…) ON CONFLICT(calendar_id, event_id) DO UPDATE SET body=excluded.body, event_title=excluded.event_title, event_start=excluded.event_start, updated_at=excluded.updated_at`. `created_at` is set on the initial insert and **preserved** on update (not in the `DO UPDATE SET` list); the snapshot (`event_title`/`event_start`) is refreshed on each save (keeps title/start current if the event was edited). Re-reads and returns the full stored row.
- `get_meeting_note(conn, calendar_id: &str, event_id: &str) -> Result<Option<MeetingNote>>` — `None` when absent.
- `list_meeting_notes(conn) -> Result<Vec<MeetingNote>>` — `ORDER BY updated_at DESC` ("what I worked on last," robust whether the meeting is past or future).
- `delete_meeting_note(conn, calendar_id: &str, event_id: &str) -> Result<()>`.

### Backend — commands (`src-tauri/src/commands.rs`, registered in `lib.rs`)
Thin wrappers that take the DB `Connection` from Tauri managed state (same as the existing db-backed commands) and call the layer above. All local; **no Calendar API, no auth, no network.**
- `get_meeting_note(calendar_id: String, event_id: String) -> Result<Option<MeetingNote>>`.
- `save_meeting_note(note: MeetingNoteWrite) -> Result<MeetingNote>`.
- `delete_meeting_note(calendar_id: String, event_id: String) -> Result<()>`.
- `list_meeting_notes() -> Result<Vec<MeetingNote>>`.

### Frontend — api + helpers (`src/lib/notes.ts`, new)
- Interfaces `MeetingNote` / `MeetingNoteWrite` mirroring the Rust shapes (timestamps are `number` millis).
- `isTauri()`-gated wrappers: `getMeetingNote(calId, eventId)`, `saveMeetingNote(write)`, `deleteMeetingNote(calId, eventId)`, `listMeetingNotes()`.
- Pure helper `noteKey(calId, eventId): string` — a stable composite key for the has-notes `Set` and the mock store.

### Frontend — mocks (`src/lib/mock.ts`, extend)
A module-level `Map` keyed by `noteKey` backs the four wrappers in maket mode (in-memory upsert/get/list/delete), seeded with 1–2 sample notes so the panel + editor demo offline.

### Frontend — components
- **`NotesModal.tsx`** (new): reuses the M8/M19 modal pattern (`role="dialog"`, window-Esc close, no backdrop-close). Header shows the meeting title + date (from the snapshot). Body = a plain `<textarea>`. On open, calls `getMeetingNote` to load any existing body. Buttons: **Save** (disabled while the body is empty/whitespace), **Cancel**, **Delete** (shown only when a note exists, with a confirm). Save → `saveMeetingNote(...)`; Delete → `deleteMeetingNote(...)`; both close + signal the parent to reload.
- **`CalendarView.tsx`** (extend): on mount and whenever `notesReloadKey` bumps, `listMeetingNotes()` → store the list + derive `notesByKey: Set<string>`. Render a **collapsible Notes panel** (a side region within the calendar view; open/closed state is local) listing notes newest-edited first — each row shows meeting title, meeting date, and a one-line body snippet; clicking a row opens `NotesModal`; empty state "No meeting notes yet." Owns the `NotesModal` open-state + `notesReloadKey`; any save/delete bumps the key.
- **`WeekGrid.tsx`** (extend): event cells whose `noteKey ∈ notesByKey` render a small **note dot/icon**; the M19 **event-detail popover** gains a **"Notes"** button that opens `NotesModal` for that event (passing calendar id, event id, title, start as the snapshot).

### Data flow
`CalendarView mount / notesReloadKey → listMeetingNotes() → panel list + notesByKey Set → WeekGrid dots`. `Click event → popover → Notes → NotesModal (getMeetingNote) → Save/Delete → bump notesReloadKey`. `Panel row → NotesModal → Save/Delete → bump notesReloadKey`. The calendar's own week-fetch is untouched (notes are a separate, local data source).

---

## Error handling

- **Local SQLite errors only** — notes never touch Google, so there is **no 401/403/reconnect path**. Failures map through the existing `AppError` and surface inline in the modal ("Couldn't save note") / as a small panel error.
- **Empty body** → Save is disabled (use Delete to remove a note); this guarantees no empty "ghost" rows and keeps the has-notes dot + panel accurate.
- **Orphan notes** (the underlying event was deleted on Google or is outside the fetched week) are intentional: the row persists and still lists via its snapshot; the week grid simply won't draw a dot for an event it isn't currently showing. Notes are the user's data and are never garbage-collected.

---

## Testing

- **Rust (`db/mod.rs` tests, in-memory `Connection`, existing style):**
  - `upsert` inserts a new row, then a repeat `(calendar_id, event_id)` **updates the same row** (body + `updated_at` change, `created_at` preserved, still exactly one row).
  - `get` returns `Some` for a stored note and `None` when absent.
  - `list` orders by `updated_at DESC`.
  - `delete` removes the row.
  - `init` is **idempotent** with the new table (running it twice succeeds; the existing idempotency test is extended).
- **Frontend:** no TS test harness (consistent through M19); `noteKey` is trivially pure. **Maket-verified by screenshot:** the Notes panel listing a seeded note, the editor open with body text, and a has-notes dot on a week-grid event.
- **Gates:** `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean. Notes are local, so **persistence across an app restart is genuinely testable in the dev build** (unlike the Google E2E paths, which remain owner-pending consistent with M10–M19).

---

## Known risks & decisions

- **Local-only storage, no Google sync** — notes are the user's private data, never written back to Calendar. The denormalized `event_title`/`event_start` snapshot is what makes a note durable independent of the live event (past, out-of-week, or deleted). Chosen over storing notes in the event `description` (which *would* sync but is shared with guests and round-trips through Google).
- **One note doc per `(calendar_id, event_id)`** — recurring events are expanded by M10 via `singleEvents`, so each instance has a distinct id and gets its own note (consistent with M19's per-instance edit/delete). `UNIQUE(calendar_id, event_id)` + `ON CONFLICT` upsert enforces the single-doc model.
- **First new table since the core schema** — added purely additively in `db::init()`; no `ALTER`, no data migration, idempotent. The Flyway `db-migrations` rule / `@db-migration-reviewer` are Flyway/Java-specific and **do not apply** to this rusqlite setup.
- **Forward-compat without dead weight (Approach A)** — `summary` (Ollama) and `transcript` columns are deferred to their own milestones and added later via the existing `add_column` helper, rather than shipped now as unused columns. The single-row-per-meeting shape is what lets them become sibling columns cleanly.
- **Plain-text body in v1** — a format-agnostic string; markdown/rich rendering is deferred (the later AI summary may emit markdown, which the body column already stores fine).
- **No new OAuth scope, no new dependency, Tauri build unchanged for the maket** — every new wrapper is `isTauri()`-gated; the panel/editor are frontend over mock data.

---

## Non-goals / constraints

- **No new OAuth scope, no Google API calls** — notes are local-only.
- **No `ALTER` to existing tables, no data migration** — one additive `CREATE TABLE IF NOT EXISTS`.
- **No AI in M20** — Ollama summarization and transcription capture are later milestones in this feature area; M20 only lays the storage + manual-note UI they will build on.
- **Plain create/edit** — no rich text, no multiple entries, no note search/export in v1.
