# Ember Snooze — archive + local wake (design)

**Date:** 2026-06-22
**Status:** Approved (brainstorming) → ready for implementation plan

## Goal

Let the user **snooze** an inbox message: it leaves the inbox now and **reappears (unread) at a chosen time**. Gmail has no snooze API, so Ember implements it with primitives it already has — archive (remove `INBOX`) + a **local wake-time** — and a small background timer that re-adds `INBOX` when due. Matches the "Snoozed" item and per-row clock the redesign mockup showed (both deferred from that pass).

## Decisions (from brainstorming)

- **Mechanism: real snooze.** Snoozing **archives** the message (`batchModify removeLabelIds:["INBOX"]`, the existing Archive path) and records a local wake-time. The message truly leaves the inbox everywhere (Gmail web too).
- **Wake = re-add INBOX + UNREAD** via `batchModify addLabelIds:["INBOX","UNREAD"]` (safe — only TRASH/SPAM are special on batchModify; INBOX/UNREAD work). Marking unread resurfaces it at the top, like Gmail/Spark.
- **Presets:** Later today (+3h), Tomorrow (9am), This weekend (Sat 9am), Next week (Mon 9am), plus a **Custom** date/time.
- **Wake timer runs whenever an account is connected — NOT gated on the notifications toggle** — plus an immediate check on app launch, so snoozes due while Ember was closed wake on next open.
- **Reliability caveat (inherent):** no server → no waking while the app is closed. Wake times are shown in the Snoozed view so it's never a surprise.
- **Snoozed view** lives in the sidebar's **Saved** section (next to Pinned).
- **Deferred (YAGNI):** a Gmail-visible "Snoozed" label, recurring snoozes, a wake notification banner.

## Architecture & data flow

```
Snooze:  [clock button] → SnoozeMenu (preset/custom) → wakeAt(ms)
         → snooze_message(id, wakeAt, snapshot)
             ├─ client.batch_modify([id], add:[], remove:["INBOX"])   (archive on Gmail)
             ├─ db::delete_messages([id])                              (drop from inbox cache)
             └─ db::insert_snooze(row)                                 (track locally)

Wake:    [60s timer, gated on account only] + [launch check]
         → wake_due_snoozes(now)
             ├─ ids = db rows WHERE wake_at <= now   (return early if none → no Gmail call)
             ├─ client.batch_modify(ids, add:["INBOX","UNREAD"], remove:[])
             └─ db::delete_snoozes(ids)  → returns woken ids
         → frontend runSync() to pull the re-INBOXed mail back into the cache

View:    Sidebar "Snoozed" → list_snoozed() (local snapshots + wake_at)
         → MessageList renders rows with an "Un-snooze" action → unsnooze_message(id)
```

## Storage — new `snoozed` table (additive, no migration)

Follows the `meeting_notes` pattern (`CREATE TABLE IF NOT EXISTS` in `db::init`, so no migration framework). The **snapshot** columns let the Snoozed view render without a Gmail fetch (the message is archived, not in the inbox cache).

```sql
CREATE TABLE IF NOT EXISTS snoozed (
  message_id    TEXT PRIMARY KEY,
  thread_id     TEXT NOT NULL DEFAULT '',
  wake_at       INTEGER NOT NULL,          -- unix ms
  snoozed_at    INTEGER NOT NULL,          -- unix ms
  from_addr     TEXT NOT NULL DEFAULT '',
  subject       TEXT NOT NULL DEFAULT '',
  snippet       TEXT NOT NULL DEFAULT '',
  internal_date INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_snoozed_wake_at ON snoozed(wake_at);
```

DB helpers (in `db/mod.rs`, unit-tested with an in-memory connection like the existing db tests): `insert_snooze(&conn, row)`, `delete_snoozes(&conn, &ids)`, `due_snoozes(&conn, now) -> Vec<String>`, `list_snoozes(&conn) -> Vec<SnoozedRow>` (ordered by `wake_at ASC`).

## Rust commands (`commands.rs`, registered in `lib.rs`)

- `snooze_message(id, wake_at, thread_id, from_addr, subject, snippet, internal_date, state)` — archive on Gmail + drop from inbox cache + insert snooze row. Mirrors `delete_message_forever`'s shape (Gmail call → lock → DB write).
- `unsnooze_message(id, state)` — manual: `batch_modify([id], add:["INBOX","UNREAD"], remove:[])` + delete snooze row. (Inbox refresh happens via the frontend `runSync` afterward.)
- `wake_due_snoozes(state) -> Vec<String>` — read `due_snoozes(now)`; if empty return `vec![]` **before any network**; else `batch_modify(ids, add:["INBOX","UNREAD"], remove:[])` + `delete_snoozes(ids)`; return woken ids. `now` from `now_ms()` (server-free; reuse existing `now_secs()`-style helper × 1000, or add `now_ms`).
- `list_snoozed() -> Vec<SnoozedPreview>` — DB-only; maps `snoozed` rows to a preview shape the frontend list renders.

All Gmail calls reuse `GmailClient::batch_modify` (already handles INBOX/UNREAD correctly).

## Frontend

- **`lib/snooze.ts`** (pure, isTauri-agnostic): `snoozePresets(now: Date)` returning `{label, wakeAt}[]` for the 4 presets (local-time math: Tomorrow/Weekend/Next-week anchored to 9:00 local; "This weekend" = the coming Saturday, or next Saturday if today is already Sat/Sun); plus a custom path that takes a `datetime-local` value → ms. API wrappers (`isTauri`-gated, matching `lib/api.ts` style): `snoozeMessage`, `unsnoozeMessage`, `wakeDueSnoozes`, `listSnoozed`; mock equivalents in `lib/mock.ts`.
- **`SnoozeMenu.tsx`** — a small popover anchored to its trigger: the 4 presets (with computed times shown) + a Custom `datetime-local`; calls `onSnooze(wakeAt)`. Reused by the row clock and the reading-pane button.
- **Triggers:** a **clock icon** on the message card (`MessageItem`, hover-revealed like the star) and a clock in the **ReadingPane** action cluster. Both open `SnoozeMenu`. On choose → `snoozeMessage(...)` → optimistically drop the row from the active list (same pattern as archive/trash in `App.tsx`).
- **Snoozed view:** a **"Snoozed"** item in `Sidebar` (Saved section). Selecting it sets a virtual `folder = "snoozed"`; `App.tsx` recognizes this key and loads `listSnoozed()` (local) instead of a Gmail folder, rendering rows via `MessageList` with the action set swapped to **Un-snooze** (like the Trash folder swaps to Restore/Delete-forever). Each row shows its wake time.
- **Wake loop:** a new `useEffect` interval in `App.tsx`, **gated on `account` only** (separate from the `POLL_MS` notifications poller): every 60s call `wakeDueSnoozes()`; if it returns ids, `runSync(false)` to refresh the inbox. Also call it once on launch (and when the window regains focus, best-effort) so overdue snoozes wake promptly.

## Files touched

| File | Change |
|---|---|
| `src-tauri/src/db/mod.rs` | `snoozed` table in `init` + `SnoozedRow` + insert/delete/due/list helpers + db tests |
| `src-tauri/src/commands.rs` | `snooze_message`, `unsnooze_message`, `wake_due_snoozes`, `list_snoozed` (+ `now_ms` if needed) |
| `src-tauri/src/lib.rs` | register the 4 commands |
| `src/lib/snooze.ts` | **New** — preset time math + isTauri-gated API wrappers |
| `src/lib/mock.ts` | mock snooze store for the maket |
| `src/components/SnoozeMenu.tsx` | **New** — preset/custom popover |
| `src/components/MessageItem.tsx` | hover clock button → SnoozeMenu |
| `src/components/ReadingPane.tsx` | clock in the action cluster → SnoozeMenu |
| `src/components/Sidebar.tsx` | "Snoozed" item in Saved |
| `src/App.tsx` | snooze handler (optimistic remove), `folder==="snoozed"` virtual view, Un-snooze action wiring, wake-loop effect |
| `src/styles/app.css` | SnoozeMenu + clock button styles |

## Out of scope

Gmail-visible "Snoozed" label; recurring snoozes; wake notification banners; snooze sync across devices; per-message custom default times.

## Verification

- Rust: `cargo test` green incl. **new db tests** for insert/due/list/delete (in-memory conn; assert `due_snoozes` boundary at exactly `wake_at == now`, and ordering). `wake_due_snoozes`'s Gmail half isn't unit-tested (it's a thin batch_modify wrapper, like other commands) — covered by the existing `batch_modify` wiremock test.
- `npx tsc --noEmit` clean.
- **Maket** (`ember-maket`): snooze a message via the row clock → it leaves the list; open Snoozed view → it's listed with its wake time; Un-snooze → returns. Preset times render correctly. (The wake-on-timer Gmail round-trip isn't exercisable in the maket — verify the menu, optimistic remove, Snoozed list, and un-snooze; the live wake is owner-verified in the Tauri build.)
- Confirm the wake loop is **not** gated on `settings.notifications` (snoozes wake with notifications off).

## Risks / notes

- **App-closed wake delay** — inherent; mitigated by the launch check + visible wake times.
- **Double-archive interaction:** if a user archives a message that's already snoozed (edge), the snooze row would linger; low-risk, but `wake_due_snoozes` re-adding INBOX to an already-archived/again-present message is idempotent enough (batchModify add INBOX is a no-op if present). Not handling explicitly in v1.
- **Timezone:** preset math is local-time in the frontend (`Date`), so "Tomorrow 9am" is the user's 9am — correct for a desktop app.
- The maket's pre-existing un-gated-`invoke` issue is unrelated; snooze wrappers WILL be isTauri-gated.
