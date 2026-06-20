# Ember — Milestone 15: Batch actions + undo (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Multi-select messages and **Archive / Trash / Mark-read / Star** them in a single Gmail `batchModify` call, and show an **Undo toast** after any archive or trash — single *or* batch. Second of the M14→M15→M16→M17 arc. **No new OAuth scope, no DB migration.**

**Architecture in one paragraph:** Selection is a frontend `Set<string>` over the existing M11/M12 "active list" (inbox / search / folder), surfaced as a per-row checkbox plus a **batch action bar** that replaces the list header when anything is selected. Every batch op is one Gmail `users.messages.batchModify` call (≤1000 ids, returns 204). The milestone **unifies archive & trash — single and batch — onto one `batch_modify` path**: archive = `batchModify(remove INBOX)`, trash = `batchModify(add TRASH)`, and **Undo is the symmetric inverse** of the same call (swap add/remove) plus restoring the removed rows to the list. This replaces M7's single `archive_message`/`trash_message` commands (a 1-element batch subsumes them), so reading-pane single archive/trash *also* gain undo, with exactly one code path. Single mark-read/star keep the M7 `modify_message` (which echoes server labels); they get no toast (reversible in place). The cache reconciles like M7: archive/trash delete the cached rows; batch read/star apply a label delta to them.

**Tech Stack:** Rust (reqwest, serde, rusqlite, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M14 are merged to `main`. Ember reads/classifies/mutates/sends mail, has settings + calendar + search + folders + notifications + drafts. M7 added single-message actions (read/unread, star, archive, trash) with optimistic UI + rollback, but **multi-select and undo were explicitly deferred at M7**. M15 adds them. It is the second of the new arc (**M14 drafts → M15 batch+undo → M16 labels → M17 attachments**).

**Reuse map:** M7 `modify_message` (single, echoes labels — kept for read/star), `db::delete_messages` (archive/trash cache removal), `db::update_message_labels`, the optimistic-with-rollback pattern (`withActiveRollback`/`removeWithAction`); M11/M12 "active list" (`activeList`/`setActiveList`/`activeSelectedId`); `src/lib/labels.ts` (`withLabel`/`isUnread`/`isStarred`/`UNREAD`/`STARRED`); the Gmail client JSON helpers.

---

## Scope

**In scope (lean v1):**
- **Multi-select** via per-row checkboxes over the active list (inbox/search/folder).
- A **batch action bar** (replaces the list header when ≥1 selected): N selected · **Archive · Trash · Mark read · Star** · select-all · clear (✕).
- Each batch op = **one `batchModify` call** (archive=remove INBOX, trash=add TRASH, read=remove UNREAD, star=add STARRED).
- **Unify archive/trash (single + batch) onto `batch_modify`**, removing M7's single `archive_message`/`trash_message`.
- An **Undo toast** after any archive/trash (single + batch): ~6s auto-dismiss, restores the rows + issues the inverse `batchModify`. Single-level (latest action only).
- Cache reconcile: archive/trash → `db::delete_messages`; batch read/star → new `db::apply_label_delta`.
- A **browser mock** so selection + batch ops + undo work in the maket.

**Explicitly deferred (not in M15):**
- **Mark-unread / Unstar** buttons in the bar (chose Archive/Trash/Mark-read/Star).
- **Multi-level undo** (only the latest archive/trash is undoable).
- **Undo for read/star** (reversible in place — the controls flip them back).
- **Select-all across unloaded messages** (only the loaded/visible list).
- **Keyboard shortcuts** (e=archive, #=trash, etc.).
- **Undo persistence across app restart**; an undo for batch mark-read/star.

---

## Components

### Backend — `GmailClient::batch_modify` (`src-tauri/src/gmail/mod.rs`)
- `pub async fn batch_modify(&self, ids: &[String], add: &[&str], remove: &[&str]) -> Result<()>` — `POST /gmail/v1/users/me/messages/batchModify`, body `{ "ids", "addLabelIds", "removeLabelIds" }`. Gmail returns **204 (empty body)**, so it uses a new private helper `post_json_no_response<B: Serialize>(url, body)` (posts JSON, `error_for_status`, discards the empty body — `post_json` would fail trying to parse an empty body).
- The single `trash_message` client method is **removed** (no longer used after unification; `modify_message` stays for read/star, `untrash_message` stays for M12 folder restore).

### Backend — command (`src-tauri/src/commands.rs`, registered in `lib.rs`)
- `batch_modify_messages(ids: Vec<String>, add: Vec<String>, remove: Vec<String>, state) -> Result<()>`: calls `client.batch_modify(&ids, &add_refs, &remove_refs)`, then reconciles the cache in one locked block:
  - if `add` contains `"TRASH"` **or** `remove` contains `"INBOX"` → `db::delete_messages(&conn, &ids)` (rows leave the inbox cache, mirroring M7 archive/trash).
  - else → `db::apply_label_delta(&conn, &ids, &add, &remove)` (read/star: update cached rows in place).
- **Remove** the M7 `archive_message` and `trash_message` commands + their `lib.rs` registrations.

### Backend — `db::apply_label_delta` (`src-tauri/src/db/mod.rs`)
- `pub fn apply_label_delta(conn, ids: &[String], add: &[String], remove: &[String]) -> Result<()>` — for each id present in `messages`, load its comma-joined `label_ids`, apply remove-then-add as a set, re-join, `UPDATE`. Idempotent; ids not in the cache (search/folder results) are silently skipped. One transaction.

### Frontend — selection (`src/App.tsx`)
- `selectedIds: Set<string>` state over the active list. Cleared by `handleSelectFolder`, `handleSearch`/`handleClearSearch`, the stream `onSelectStream`, and after every batch action. A derived `selectedMsgs = activeList.filter(m => selectedIds.has(m.id))`.
- `toggleSelect(id)`, `clearSelection()`, `selectAllVisible(ids)`.

### Frontend — `MessageItem` / `MessageList`
- `MessageItem` gains a leading **checkbox** (`checked: boolean`, `onToggleSelect: (id) => void`); clicking it toggles selection and does **not** call `onSelect` (stop propagation). The row shows a `selected-for-batch` style when checked.
- `MessageList` gains `selectedIds`, `onToggleSelect`, `onSelectAll`, `onClearSelection`, and the four batch handlers. When `selectedIds.size > 0`, the `msglist-header` is replaced by a **batch action bar**: a select-all checkbox, "N selected", buttons **Archive / Trash / Mark read / Star**, and a clear (✕). The bar operates on the currently-visible selected ids.

### Frontend — unified actions (`src/App.tsx`)
- `removeMessages(msgs: MessagePreview[], op: { add: string[]; remove: string[]; verb: string })`: snapshot the active list; optimistically remove those ids (and fix `activeSelectedId` via the existing `nextSelectedId` if the open message is removed); call `batchModifyMessages(ids, op.add, op.remove)`; on error roll back + `setError`; on success register an Undo `{ verb, rows: msgs, ids, inverse: { add: op.remove, remove: op.add } }`. Used by:
  - `handleArchive(m)` → `removeMessages([m], { add: [], remove: ["INBOX"], verb: "Archived" })`
  - `handleTrash(m)` → `removeMessages([m], { add: ["TRASH"], remove: [], verb: "Trashed" })`
  - `batchArchive()` / `batchTrash()` → same with `selectedMsgs`.
- `batchMarkRead()` / `batchStar()`: optimistic in-place `withLabel` on the active list (`UNREAD`→false / `STARRED`→true), `batchModifyMessages(ids, add, remove)`, roll back on error. **No toast.** Clear selection after.

### Frontend — `UndoToast` (`src/components/UndoToast.tsx`, new) + `lib/api.ts`
- App state `undo: { verb: string; count: number; onUndo: () => void } | null` with a ~6s auto-dismiss timer (cleared on new toast / unmount / manual dismiss).
- `UndoToast` renders a bottom-center pill: `"{verb} {count}"` + an **Undo** button. Undo: merge `rows` back into the active list (dedupe by id, re-sort by `internal_date` desc), issue `batchModifyMessages(ids, inverse.add, inverse.remove)`, clear the toast; on failure re-remove + `setError`.
- `lib/api.ts`: `batchModifyMessages(ids, add, remove)` wrapper (`isTauri()`-gated; mock = `Promise.resolve()`). **Remove** the `archiveMessage`/`trashMessage` wrappers; reroute their callers.

### Data flow
`check rows → batch bar button → removeMessages / batchMarkRead / batchStar → optimistic active-list update + batchModify → (archive/trash) Undo toast → Undo = restore rows + inverse batchModify`.

---

## Error handling

- A failed `batchModify` rolls back the optimistic change (restore the snapshot) and surfaces the error (existing `withActiveRollback` pattern). Selection still clears.
- A failed **undo** re-applies the removal and shows the error.
- Single-level undo: starting a new archive/trash replaces the toast (the prior action is no longer undoable — its backend change already committed and is left as-is).
- Search/folder ids not in the cache make `apply_label_delta`/`delete_messages` no-ops (idempotent) — batch ops still work there via the live Gmail call.

---

## Testing

- **Rust:** a wiremock test for `batch_modify` (asserts `POST /messages/batchModify` with body `{ids, addLabelIds, removeLabelIds}` and a 204 response is handled). A `db::apply_label_delta` unit test (delta applied to a cached row; idempotent on re-apply; an uncached id is skipped without error). Confirm removing `archive_message`/`trash_message` leaves the suite green (any tests referencing them are updated/removed).
- **Frontend:** no TS test harness exists (consistent through M14). Selection, the batch bar, optimistic batch ops, and the undo toast are entirely frontend-exercisable — verified in the **browser maket** (select rows → bar appears → Archive removes them → Undo restores them; Mark read/Star flip in place) and a screenshot.
- `cargo test` + `cargo clippy --all-targets` stay green; `npm run build` clean. **Live Gmail E2E** (real batch archive/trash/read/star + undo round-trips, esp. the trash-via-label equivalence) is **owner-pending**, consistent with M10–M14.

---

## Known risks & decisions

- **Trash via label vs the `/trash` endpoint (the one correctness risk):** M15 trashes by `batchModify(add TRASH)` and untrashes by `batchModify(remove TRASH)`, replacing M7's dedicated `/trash`. Adding/removing the system `TRASH` label is a documented Gmail modify behavior, and the wiremock test pins the request shape — but real trash/untrash round-trips are part of the owner-pending E2E. If live testing ever shows a discrepancy, single trash can fall back to the retained `untrash_message` for undo without changing the UI.
- **Unifying archive/trash onto `batch_modify` (removing the two M7 single commands)** — deliberate: one code path, symmetric undo, and reading-pane single actions gain undo for free. The cost is touching working M7 code; mitigated by the wiremock + the existing optimistic-rollback tests.
- **Single-level undo** — only the latest archive/trash is undoable; a new one replaces the toast. Multi-level is deferred.
- **Undo restores in-memory rows + inverse `batchModify`; the DB row stays deleted until the next sync** re-caches it (same optimistic-then-reconcile model as M7). Acceptable.
- **Selection scope** — `selectedIds` is over the active list and cleared on any list switch, so a stale selection can't apply an action to the wrong list.

---

## Non-goals / constraints

- **No new OAuth scope** — `gmail.modify` already permits `batchModify`.
- **No DB migration** — `apply_label_delta` operates on the existing `label_ids` column.
- **Tauri build unchanged for the maket** — `batchModifyMessages` is `isTauri()`-gated; selection/undo are pure frontend.
- **Single read/star unchanged** — they keep M7's `modify_message` (server-echoed labels) and get no toast.
