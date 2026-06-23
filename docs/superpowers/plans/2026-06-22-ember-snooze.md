# Ember Snooze Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Snooze an inbox message so it archives now and reappears (unread) at a chosen time, implemented with archive (remove INBOX) + a local wake-time + a background wake timer.

**Architecture:** New local `snoozed` SQLite table (additive, no migration) tracks pending snoozes with a display snapshot. Snooze archives via the existing `batch_modify` path; a frontend timer (gated on account only, plus a launch check) calls `wake_due_snoozes`, which re-adds INBOX+UNREAD for due rows. A `SnoozeMenu` popover (presets + custom) triggers from the message card and reading pane; a `SnoozedList` view lives behind a "Snoozed" sidebar item.

**Tech Stack:** Rust (`rusqlite`, Tauri commands, `reqwest` via the existing `GmailClient`), React + TypeScript, `lucide-react`. Tests: `cargo test` (in-memory SQLite db tests), `npx tsc --noEmit`, browser maket (`ember-maket`, port 5190).

**Execution note:** Work on a `snooze` feature branch (`git checkout -b snooze`); `main` is clean. Commit per task. Frontend pixel/positioning tuning against the maket is expected.

**Verification pattern:** after each task, `npx tsc --noEmit` clean and (Rust tasks) `cd src-tauri && cargo test`; UI tasks verified in the maket (`preview_start` `ember-maket` → `preview_snapshot`/`preview_screenshot`/`preview_console_logs`, click **Mail** first since the maket defaults to Calendar). Maket snooze wrappers are mocked (no Gmail) — verify the menu, optimistic remove, Snoozed list, and un-snooze; the live wake round-trip is owner-verified in the Tauri build.

---

## Task 1: DB layer — `snoozed` table + helpers (TDD)

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (table in `init`, `SnoozedRow`, helpers, tests)

- [ ] **Step 1: Write failing db tests.** In the `#[cfg(test)] mod tests` block of `db/mod.rs` (it already uses `Connection::open_in_memory()`), add:

```rust
#[test]
fn snooze_insert_due_list_delete() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let row = |id: &str, wake: i64| SnoozedRow {
        message_id: id.into(), thread_id: "t".into(), wake_at: wake, snoozed_at: 1,
        from_addr: "a@b.co".into(), subject: "s".into(), snippet: "sn".into(), internal_date: wake,
    };
    insert_snooze(&c, &row("a", 1000)).unwrap();
    insert_snooze(&c, &row("b", 3000)).unwrap();
    // due is inclusive at the boundary; ordered by wake_at
    assert_eq!(due_snoozes(&c, 999).unwrap(), Vec::<String>::new());
    assert_eq!(due_snoozes(&c, 1000).unwrap(), vec!["a".to_string()]);
    assert_eq!(due_snoozes(&c, 5000).unwrap(), vec!["a".to_string(), "b".to_string()]);
    // insert is upsert on message_id
    insert_snooze(&c, &row("a", 9000)).unwrap();
    assert_eq!(list_snoozes(&c).unwrap().len(), 2);
    delete_snoozes(&c, &["a".to_string()]).unwrap();
    let left = list_snoozes(&c).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].message_id, "b");
}
```

- [ ] **Step 2: Run — verify it fails to compile** (`SnoozedRow`/helpers undefined).
Run: `cd src-tauri && cargo test snooze_insert_due_list_delete 2>&1 | tail -15`
Expected: compile error `cannot find ... SnoozedRow` / `insert_snooze`.

- [ ] **Step 3: Add the table to `init`.** In `db::init` (around line 94), alongside the existing `CREATE TABLE IF NOT EXISTS` statements (match the idiom used there — `conn.execute_batch("…")?` or the same call the neighbors use), add:
```rust
conn.execute_batch(
    "CREATE TABLE IF NOT EXISTS snoozed (
        message_id    TEXT PRIMARY KEY,
        thread_id     TEXT NOT NULL DEFAULT '',
        wake_at       INTEGER NOT NULL,
        snoozed_at    INTEGER NOT NULL,
        from_addr     TEXT NOT NULL DEFAULT '',
        subject       TEXT NOT NULL DEFAULT '',
        snippet       TEXT NOT NULL DEFAULT '',
        internal_date INTEGER NOT NULL DEFAULT 0
     );
     CREATE INDEX IF NOT EXISTS idx_snoozed_wake_at ON snoozed(wake_at);",
)?;
```

- [ ] **Step 4: Add `SnoozedRow` + helpers.** Near the `meeting_notes` helpers (e.g. after `list_meeting_notes`), add:
```rust
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

pub fn insert_snooze(conn: &Connection, r: &SnoozedRow) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO snoozed
           (message_id, thread_id, wake_at, snoozed_at, from_addr, subject, snippet, internal_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![r.message_id, r.thread_id, r.wake_at, r.snoozed_at, r.from_addr, r.subject, r.snippet, r.internal_date],
    )?;
    Ok(())
}

pub fn delete_snoozes(conn: &Connection, ids: &[String]) -> Result<()> {
    if ids.is_empty() { return Ok(()); }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM snoozed WHERE message_id IN ({placeholders})");
    let refs: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    conn.execute(&sql, refs.as_slice())?;
    Ok(())
}

pub fn due_snoozes(conn: &Connection, now_ms: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT message_id FROM snoozed WHERE wake_at <= ?1 ORDER BY wake_at ASC")?;
    let rows = stmt.query_map(params![now_ms], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn list_snoozes(conn: &Connection) -> Result<Vec<SnoozedRow>> {
    let mut stmt = conn.prepare(
        "SELECT message_id, thread_id, wake_at, snoozed_at, from_addr, subject, snippet, internal_date
         FROM snoozed ORDER BY wake_at ASC",
    )?;
    let rows = stmt.query_map([], |r| Ok(SnoozedRow {
        message_id: r.get(0)?, thread_id: r.get(1)?, wake_at: r.get(2)?, snoozed_at: r.get(3)?,
        from_addr: r.get(4)?, subject: r.get(5)?, snippet: r.get(6)?, internal_date: r.get(7)?,
    }))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
```
(If `delete_messages` in this file already has an `IN (…)` helper pattern, mirror it instead of the inline placeholder build.)

- [ ] **Step 5: Run tests — pass.**
Run: `cd src-tauri && cargo test snooze_insert_due_list_delete 2>&1 | tail -8`
Expected: `test result: ok. 1 passed`. Then full `cargo test` stays green.

- [ ] **Step 6: Commit.**
```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(snooze): snoozed table + insert/due/list/delete db helpers"
```

---

## Task 2: Rust commands — snooze / unsnooze / wake / list

**Files:**
- Modify: `src-tauri/src/commands.rs` (commands + `now_ms`)
- Modify: `src-tauri/src/lib.rs` (register 4 commands)

- [ ] **Step 1: Add `now_ms` + commands.** In `commands.rs`, after `now_secs()` (line ~211) add `fn now_ms() -> i64 { now_secs() as i64 * 1000 }`. Then add (near `delete_message_forever`):
```rust
/// Snooze: archive on Gmail (remove INBOX), drop from the inbox cache, record a local wake-time.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn snooze_message(
    id: String, wake_at: i64, thread_id: String, from_addr: String,
    subject: String, snippet: String, internal_date: i64,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(std::slice::from_ref(&id), &[], &["INBOX"]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, std::slice::from_ref(&id))?;
    db::insert_snooze(&conn, &db::SnoozedRow {
        message_id: id, thread_id, wake_at, snoozed_at: now_ms(),
        from_addr, subject, snippet, internal_date,
    })?;
    Ok(())
}

/// Manual un-snooze: re-add INBOX + UNREAD on Gmail, drop the local row.
#[tauri::command]
pub async fn unsnooze_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(std::slice::from_ref(&id), &["INBOX", "UNREAD"], &[]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_snoozes(&conn, std::slice::from_ref(&id))?;
    Ok(())
}

/// Wake all snoozes whose wake_at has passed. Returns early (no network) when none are due.
#[tauri::command]
pub async fn wake_due_snoozes(state: tauri::State<'_, Db>) -> Result<Vec<String>> {
    let ids = {
        let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::due_snoozes(&conn, now_ms())?
    };
    if ids.is_empty() { return Ok(Vec::new()); }
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.batch_modify(&ids, &["INBOX", "UNREAD"], &[]).await?;
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_snoozes(&conn, &ids)?;
    Ok(ids)
}

/// List pending snoozes for the Snoozed view (DB-only).
#[tauri::command]
pub fn list_snoozed(state: tauri::State<'_, Db>) -> Result<Vec<db::SnoozedRow>> {
    let conn = state.lock().map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::list_snoozes(&conn)
}
```

- [ ] **Step 2: Register in `lib.rs`.** Add to the `tauri::generate_handler![…]` list (next to `delete_message_forever`):
```rust
            commands::snooze_message,
            commands::unsnooze_message,
            commands::wake_due_snoozes,
            commands::list_snoozed,
```

- [ ] **Step 3: Build + test.**
Run: `cd src-tauri && cargo build 2>&1 | grep -iE "error|warning" | head; cargo test 2>&1 | grep "test result" | tail -3`
Expected: builds clean (no warnings), all tests green.

- [ ] **Step 4: Commit.**
```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(snooze): snooze/unsnooze/wake_due/list commands"
```

---

## Task 3: Frontend lib — presets + API wrappers + mock

**Files:**
- Create: `src/lib/snooze.ts`
- Modify: `src/lib/mock.ts` (mock snooze store)

- [ ] **Step 1: Create `src/lib/snooze.ts`.**
```ts
import { isTauri } from "@tauri-apps/api/core";
import { invoke } from "@tauri-apps/api/core";
import { mockSnooze, mockUnsnooze, mockWakeDue, mockListSnoozed } from "./mock";
import type { MessagePreview } from "./api";

export interface SnoozePreset { label: string; wakeAt: number; }
export interface SnoozedRow {
  message_id: string; thread_id: string; wake_at: number; snoozed_at: number;
  from_addr: string; subject: string; snippet: string; internal_date: number;
}

// Preset wake times, anchored to 09:00 local. "This weekend" = the coming Saturday
// (next Saturday if today is already Saturday). "Next week" = next Monday.
export function snoozePresets(now: Date = new Date()): SnoozePreset[] {
  const day = now.getDay(); // 0 Sun .. 6 Sat
  const at9 = (addDays: number): number => {
    const x = new Date(now);
    x.setDate(x.getDate() + addDays);
    x.setHours(9, 0, 0, 0);
    return x.getTime();
  };
  let weekendDays = (6 - day + 7) % 7; // 0 on Sat
  if (day === 6) weekendDays = 7;      // already Saturday → next Saturday
  let mondayDays = (1 - day + 7) % 7;  // 0 on Mon
  if (mondayDays === 0) mondayDays = 7;
  return [
    { label: "Later today", wakeAt: now.getTime() + 3 * 60 * 60 * 1000 },
    { label: "Tomorrow", wakeAt: at9(1) },
    { label: "This weekend", wakeAt: at9(weekendDays) },
    { label: "Next week", wakeAt: at9(mondayDays) },
  ];
}

export const snoozeMessage = (m: MessagePreview, wakeAt: number): Promise<void> =>
  isTauri()
    ? invoke<void>("snooze_message", {
        id: m.id, wakeAt, threadId: m.thread_id, fromAddr: m.from,
        subject: m.subject, snippet: m.snippet, internalDate: m.internal_date,
      })
    : mockSnooze(m, wakeAt);

export const unsnoozeMessage = (id: string): Promise<void> =>
  isTauri() ? invoke<void>("unsnooze_message", { id }) : mockUnsnooze(id);

export const wakeDueSnoozes = (): Promise<string[]> =>
  isTauri() ? invoke<string[]>("wake_due_snoozes") : mockWakeDue();

export const listSnoozed = (): Promise<SnoozedRow[]> =>
  isTauri() ? invoke<SnoozedRow[]>("list_snoozed") : mockListSnoozed();
```
NOTE: confirm the Tauri arg casing — this codebase's `invoke` calls pass camelCase keys (e.g. `batchModifyMessages` passes `{ ids, add, remove }`; `setMessageRead` passes `{ id, read }`). Tauri maps camelCase JS args to snake_case Rust params, so `wakeAt`→`wake_at`, `threadId`→`thread_id`, etc. Match the existing convention in `lib/api.ts`.

- [ ] **Step 2: Add the mock store to `src/lib/mock.ts`.** A module-level array so the maket can snooze/list/un-snooze without Tauri:
```ts
import type { MessagePreview } from "./api";
import type { SnoozedRow } from "./snooze";
const _snoozed: SnoozedRow[] = [];
export function mockSnooze(m: MessagePreview, wakeAt: number): Promise<void> {
  const i = _snoozed.findIndex((r) => r.message_id === m.id);
  const row: SnoozedRow = { message_id: m.id, thread_id: m.thread_id, wake_at: wakeAt,
    snoozed_at: Date.now(), from_addr: m.from, subject: m.subject, snippet: m.snippet, internal_date: m.internal_date };
  if (i >= 0) _snoozed[i] = row; else _snoozed.push(row);
  return Promise.resolve();
}
export function mockUnsnooze(id: string): Promise<void> {
  const i = _snoozed.findIndex((r) => r.message_id === id); if (i >= 0) _snoozed.splice(i, 1);
  return Promise.resolve();
}
export function mockWakeDue(): Promise<string[]> { return Promise.resolve([]); }
export function mockListSnoozed(): Promise<SnoozedRow[]> {
  return Promise.resolve([..._snoozed].sort((a, b) => a.wake_at - b.wake_at));
}
```
Keep the existing `mock.ts` ↔ `snooze.ts` type-only import discipline (import `SnoozedRow` as a `type`).

- [ ] **Step 3: Verify.** `npx tsc --noEmit` clean.

- [ ] **Step 4: Commit.**
```bash
git add src/lib/snooze.ts src/lib/mock.ts
git commit -m "feat(snooze): preset math + isTauri-gated wrappers + maket mock store"
```

---

## Task 4: SnoozeMenu + triggers + snooze handler

**Files:**
- Create: `src/components/SnoozeMenu.tsx`
- Modify: `src/components/MessageItem.tsx` (hover clock trigger)
- Modify: `src/components/ReadingPane.tsx` (clock in the action cluster)
- Modify: `src/App.tsx` (snooze state + handler, pass triggers down)
- Modify: `src/styles/app.css` (menu + clock styles)

- [ ] **Step 1: Create `SnoozeMenu.tsx`.** A popover anchored near the click; presets show their computed time.
```tsx
import { useState } from "react";
import { snoozePresets } from "../lib/snooze";

export function SnoozeMenu({
  anchor, onPick, onClose,
}: {
  anchor: { x: number; y: number };
  onPick: (wakeAt: number) => void;
  onClose: () => void;
}) {
  const [custom, setCustom] = useState("");
  const presets = snoozePresets();
  const fmt = (ms: number) =>
    new Date(ms).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" });
  return (
    <>
      <div className="snooze-backdrop" onClick={onClose} />
      <div className="snooze-menu" style={{ left: anchor.x, top: anchor.y }} role="menu">
        {presets.map((p) => (
          <button key={p.label} className="snooze-item" onClick={() => onPick(p.wakeAt)}>
            <span>{p.label}</span><span className="snooze-when">{fmt(p.wakeAt)}</span>
          </button>
        ))}
        <div className="snooze-custom">
          <input type="datetime-local" value={custom} onChange={(e) => setCustom(e.target.value)} aria-label="Custom snooze time" />
          <button className="snooze-go" disabled={!custom} onClick={() => { const t = new Date(custom).getTime(); if (!Number.isNaN(t)) onPick(t); }}>Snooze</button>
        </div>
      </div>
    </>
  );
}
```

- [ ] **Step 2: App snooze state + handler.** In `App.tsx`, add `const [snoozeTarget, setSnoozeTarget] = useState<{ msg: MessagePreview; x: number; y: number } | null>(null);`. Add a handler that reuses the existing optimistic-remove helper `removeWithAction` (which removes from the active list + rolls back on error):
```tsx
import { snoozeMessage } from "./lib/snooze";
// ...
const openSnoozeMenu = (msg: MessagePreview, e: { clientX: number; clientY: number }) =>
  setSnoozeTarget({ msg, x: e.clientX, y: e.clientY });
const handleSnoozePick = (wakeAt: number) => {
  const t = snoozeTarget; if (!t) return;
  setSnoozeTarget(null);
  removeWithAction(t.msg, () => snoozeMessage(t.msg, wakeAt));
};
```
Render the menu near the modals: `{snoozeTarget && <SnoozeMenu anchor={{ x: snoozeTarget.x, y: snoozeTarget.y }} onPick={handleSnoozePick} onClose={() => setSnoozeTarget(null)} />}`. Pass `onSnooze={(msg, e) => openSnoozeMenu(msg, e)}` to `MessageList` (→ `MessageItem`) and `ReadingPane`.

- [ ] **Step 3: MessageItem clock trigger.** Add an optional `onSnooze?: (msg: MessagePreview, e: { clientX: number; clientY: number }) => void` prop. Render a hover-revealed clock button next to the star (lucide `Clock`); `onClick={(e) => { e.stopPropagation(); onSnooze?.(msg, e); }}`. Thread `onSnooze` through `MessageList`'s props to each `MessageItem` (MessageList passes it like `onStar`). Only render the clock when `onSnooze` is provided.

- [ ] **Step 4: ReadingPane clock trigger.** Add `onSnooze?: (msg, e) => void`; render a `Clock` `.read-tool` button in the action cluster (only for non-trash). `onClick={(e) => onSnooze?.(msg, e)}`.

- [ ] **Step 5: CSS.** Append to `app.css`:
```css
.snooze-backdrop { position: fixed; inset: 0; z-index: 40; }
.snooze-menu { position: fixed; z-index: 41; min-width: 220px; padding: 6px; background: var(--surface); border: 1px solid var(--border-strong); border-radius: var(--radius-control); box-shadow: 0 8px 28px rgba(0,0,0,.4); display: flex; flex-direction: column; gap: 2px; }
.snooze-item { display: flex; justify-content: space-between; gap: 16px; align-items: center; padding: 8px 10px; border: none; border-radius: 8px; background: transparent; color: var(--text); font-size: 14px; cursor: pointer; }
.snooze-item:hover { background: var(--accent-weak); color: var(--accent-text); }
.snooze-when { color: var(--text-faint); font-size: 12px; }
.snooze-custom { display: flex; gap: 6px; padding: 6px; border-top: 1px solid var(--border); margin-top: 4px; }
.snooze-custom input { flex: 1; background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px; color: var(--text); padding: 4px 8px; }
.snooze-go { background: var(--accent); color: var(--accent-contrast); border: none; border-radius: 8px; padding: 4px 10px; cursor: pointer; }
.snooze-go:disabled { opacity: .5; cursor: default; }
.msg-clock { background: none; border: none; color: var(--text-faint); cursor: pointer; }
.msg-clock:hover { color: var(--accent-text); }
```

- [ ] **Step 6: Verify (maket).** `npx tsc --noEmit` clean. In the maket: hover a card → clock appears → click → menu shows 4 presets with times + custom; pick one → the row disappears from the inbox (optimistic remove). `preview_console_logs` clean. Also test the reading-pane clock.

- [ ] **Step 7: Commit.**
```bash
git add src/components/SnoozeMenu.tsx src/components/MessageItem.tsx src/components/ReadingPane.tsx src/App.tsx src/styles/app.css
git commit -m "feat(snooze): SnoozeMenu + card/reading-pane triggers + optimistic snooze"
```

---

## Task 5: Snoozed view + wake loop

**Files:**
- Create: `src/components/SnoozedList.tsx`
- Modify: `src/components/Sidebar.tsx` ("Snoozed" item in Saved)
- Modify: `src/App.tsx` (`folder==="snoozed"` view, load + un-snooze, wake-loop effect)
- Modify: `src/styles/app.css` (snoozed-row styles, reuse where possible)

- [ ] **Step 1: Create `SnoozedList.tsx`.** Renders snoozed rows with wake time + un-snooze.
```tsx
import { Clock, RotateCcw } from "lucide-react";
import type { SnoozedRow } from "../lib/snooze";

export function SnoozedList({ rows, onUnsnooze }: { rows: SnoozedRow[]; onUnsnooze: (id: string) => void }) {
  const fmt = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  return (
    <section className="msglist">
      <div className="list-head"><div className="list-title">Snoozed</div></div>
      {rows.length === 0 ? (
        <div className="empty">Nothing snoozed.</div>
      ) : (
        <div className="msglist-scroll">
          {rows.map((r) => (
            <div key={r.message_id} className="msg-card snoozed-card">
              <div className="msg-body">
                <div className="msg-top"><span className="name">{r.from_addr}</span></div>
                <div className="subject">{r.subject || "(no subject)"}</div>
                <div className="snippet">{r.snippet}</div>
                <div className="snooze-wake"><Clock size={12} /> Wakes {fmt(r.wake_at)}</div>
              </div>
              <button className="batch-btn" onClick={() => onUnsnooze(r.message_id)}>
                <RotateCcw size={14} /> Un-snooze
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
```

- [ ] **Step 2: Sidebar "Snoozed" item.** In `Sidebar.tsx`, in the **Saved** section under "Pinned", add (import `Clock` from lucide):
```tsx
<button className={`sb-item${folder === "snoozed" ? " active" : ""}`} onClick={() => onSelectFolder("snoozed")}>
  <span className="sb-ic"><Clock size={16} /></span><span className="sb-label">Snoozed</span>
</button>
```

- [ ] **Step 3: App — snoozed view + un-snooze.** In `App.tsx`:
  - State: `const [snoozedRows, setSnoozedRows] = useState<SnoozedRow[]>([]);` (import `SnoozedRow`, `listSnoozed`, `unsnoozeMessage`).
  - Load when entering the view: extend the folder-fetch effect (the one keyed on `[folder, folderReloadKey]`) — at the top, `if (folder === "snoozed") { listSnoozed().then(setSnoozedRows).catch((e) => setError(String(e))); return; }` (before the existing `fetchFolder`/`fetchLabel` branch). Also guard the existing effect's `folder === "inbox"` early-return so "snoozed" doesn't fall through to `fetchFolder("snoozed")`.
  - Un-snooze handler: `const handleUnsnooze = (id: string) => { setSnoozedRows((r) => r.filter((x) => x.message_id !== id)); unsnoozeMessage(id).catch((e) => setError(String(e))); };`
  - Render: where the left pane chooses `MessageList`, branch first: `folder === "snoozed" ? <SnoozedList rows={snoozedRows} onUnsnooze={handleUnsnooze} /> : <MessageList … />`. (The right pane / reading area can stay as-is; selecting a snoozed row is out of scope for v1 — un-snooze is the action.)

- [ ] **Step 4: Wake loop.** Add a NEW effect in `App.tsx`, gated on `account` ONLY (separate from the `POLL_MS` notifications poller):
```tsx
useEffect(() => {
  if (!account) return;
  const tick = () => wakeDueSnoozes().then((woken) => { if (woken.length > 0) void runSyncRef.current(false); }).catch(() => {});
  tick(); // launch check
  const id = setInterval(tick, 60_000);
  return () => clearInterval(id);
}, [account]);
```
(import `wakeDueSnoozes`. `runSyncRef` already exists for the notifications poller.)

- [ ] **Step 5: CSS.** Append:
```css
.snoozed-card { align-items: center; }
.snooze-wake { display: inline-flex; align-items: center; gap: 5px; margin-top: 4px; font-size: 12px; color: var(--accent-text); }
```

- [ ] **Step 6: Verify (maket).** `npx tsc --noEmit` clean. In the maket: snooze a message (Task 4) → click sidebar **Snoozed** → it appears with "Wakes …" → click **Un-snooze** → it leaves the list. No console errors. (Wake-timer Gmail round-trip is owner-verified in the Tauri build; in the maket `mockWakeDue` returns `[]`, so the loop is a safe no-op.)

- [ ] **Step 7: Commit.**
```bash
git add src/components/SnoozedList.tsx src/components/Sidebar.tsx src/App.tsx src/styles/app.css
git commit -m "feat(snooze): Snoozed sidebar view + un-snooze + background wake loop"
```

---

## Final verification

- [ ] `cd src-tauri && cargo test` — green incl. the new db test.
- [ ] `npx tsc --noEmit` — clean.
- [ ] Maket end-to-end: card clock → menu → snooze (row leaves) → Snoozed view lists it with wake time → un-snooze returns it. Light/dark both legible. No console errors.
- [ ] Confirm the wake-loop effect deps are `[account]` (NOT `settings.notifications`).
- [ ] Owner (Tauri build): snooze a real message, confirm it leaves the Gmail inbox; set a near-future custom time, keep the app open, confirm it reappears unread within ~60s of the wake time.

## Self-review notes (done while writing)

- **Spec coverage:** snoozed table + snapshot ✓ (T1), 4 commands ✓ (T2), preset math + wrappers + mock ✓ (T3), SnoozeMenu + card/reading triggers + optimistic remove ✓ (T4), Snoozed sidebar view + un-snooze + wake loop gated on account ✓ (T5). Deferred items (Gmail label, recurring, wake banner) intentionally absent.
- **Type consistency:** `SnoozedRow` fields identical in Rust (`db::SnoozedRow`) and TS (`lib/snooze.ts`) and the mock; command names match the `invoke` strings (`snooze_message`/`unsnooze_message`/`wake_due_snoozes`/`list_snoozed`); camelCase invoke args (`wakeAt`,`threadId`,`fromAddr`,`internalDate`) map to snake_case Rust params (verify against `lib/api.ts` convention in T3 step 1).
- **Deviation from spec:** un-snooze is realized via the dedicated `SnoozedList` row button (cleaner than overloading `MessageList`/`ReadingPane`); opening a snoozed message in the reading pane is out of v1 scope (un-snooze is the action). Wake-time display lives on the snoozed card.
