# Ember — Milestone 12: Folder & Sent views (lean v1) — Design Spec

**Status:** Approved design (2026-06-19). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Let the user browse mailboxes beyond the smart INBOX — **Sent, Starred, Archive, Trash, Spam** — via a left **folder rail**, with **Trash management** (restore + delete-forever). Second of the M11→M12→M13 sequence; reuses M11's `list_message_ids` helper and list-aware action handlers.

**Architecture in one paragraph:** A new left **FolderRail** (in the mail body, left of the SplitView — the calendar view is untouched) drives a `folder` state. This **extends M11's "active list" to three sources**: search results (highest priority), else the cached smart INBOX when `folder === "inbox"` (with stream tabs, unchanged), else a **live-fetched flat `folderResults`** for the selected folder. Because every M11 action handler already operates on the active list, star/archive/trash/reply work in folders with no new wiring. Backend: each folder maps to a `(labelId, query, includeSpamTrash)` triple fed to the M11 `list_message_ids` helper (extended with an `include_spam_trash` flag — Gmail's `messages.list` hides Trash/Spam without it); a DB-free `fetch_folder` command hydrates results via the existing concurrent `get_message_previews`. Trash gains two new operations — `untrash` (restore) and a **permanent `DELETE`** (irreversible, behind an inline confirm). Folders are live-fetched, **DB-free, no migration**; the INBOX remains the only cached/synced mailbox.

**Tech Stack:** Rust (reqwest, serde, futures, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M11 are merged to `main`. Ember reads/classifies/mutates/sends mail, has settings + a read-only calendar (M10), and server-side search (M11). The only browsable mailbox is the smart INBOX; **Sent/Starred/Archive/Trash/Spam are not reachable** (folder views were deferred repeatedly since M7). M12 adds them. It **reuses M11 directly**: the `list_message_ids(label, query, max)` paging helper and the list-aware action handlers (the "active list"). M12 is the second of three sequenced milestones (**M11 search → M12 folders → M13 notifications**). Note: M12 reintroduces a **left rail** — a deliberate reversal of M4's "sidebar folded into the header", chosen by the owner because folders are the classic sidebar case.

---

## Scope

**In scope (lean v1):**
- A left **folder rail**: Inbox · Sent · Starred · Archive · Trash · Spam (with icons; active highlight).
- **Live server-side fetch** per folder (flat list, recency-sorted), reusing the M11 fetch + preview path. DB-free, no migration.
- **Sent shows the recipient** ("To: …") instead of From.
- **Full actions on folder results** via the existing list-aware handlers (star/archive/trash/reply).
- **Trash management:** Restore (untrash) and Delete-forever (permanent, behind an inline confirm).
- A **browser mock** so folders work in the maket.

**Explicitly deferred (not in M12):**
- Arbitrary user labels; Drafts (Ember has no drafts feature yet — folder would be empty).
- Multi-select / batch actions; "Empty Trash/Spam" bulk operations.
- Per-folder message caching / offline folder browsing (only INBOX is cached).
- Unread badges / counts per folder in the rail.
- Drag-between-folders; folder-scoped search (search stays global).
- Live-refreshing a folder's membership after an in-place label action (see Known limitations).

---

## Components & contracts

### Backend — `src-tauri/src/gmail/mod.rs`
Extend the M11 helper with an `include_spam_trash` flag and add the two Trash operations:
```rust
// list_message_ids gains a 4th param AND becomes `pub` (the folder command calls it directly —
// no separate wrapper). The two existing callers pass `false`.
pub async fn list_message_ids(&self, label: Option<&str>, query: &str, max_total: u32,
                              include_spam_trash: bool) -> Result<Vec<String>>;
// → appends "&includeSpamTrash=true" when the flag is set. Gmail's messages.list omits
//   Trash/Spam messages unless this is true — required for the Trash and Spam folders.

pub async fn list_inbox_message_ids_paged(&self, query, max_total) -> …  // delegates: (Some("INBOX"), q, max, false)
pub async fn search_message_ids(&self, query, max_total) -> …            // delegates: (None, q, max, false)

/// Restore a trashed message (Gmail messages/{id}/untrash). POST, body discarded.
pub async fn untrash_message(&self, id: &str) -> Result<()>;          // reuses post_no_body
/// PERMANENTLY delete a message (Gmail DELETE messages/{id}) — bypasses Trash, irreversible.
pub async fn delete_message_forever(&self, id: &str) -> Result<()>;   // new `delete_no_body` helper (http.delete + bearer + error_for_status)
```

### Backend — `src-tauri/src/commands.rs` + `src-tauri/src/lib.rs`
```rust
#[tauri::command]
pub async fn fetch_folder(folder: String, max: u32) -> Result<Vec<MessagePreview>>;
#[tauri::command]
pub async fn restore_message(id: String) -> Result<()>;          // untrash
#[tauri::command]
pub async fn delete_message_forever(id: String, state: State<'_, Db>) -> Result<()>; // permanent + drop from local cache if present
```
- The command calls `client.list_message_ids(label, query, max, include_spam_trash)` directly.
- **Folder mapping** (in `fetch_folder`, a `match folder.as_str()`):
  | folder | label | query | includeSpamTrash |
  |---|---|---|---|
  | `sent` | `Some("SENT")` | `""` | false |
  | `starred` | `Some("STARRED")` | `""` | false |
  | `trash` | `Some("TRASH")` | `""` | **true** |
  | `spam` | `Some("SPAM")` | `""` | **true** |
  | `archive` | `None` | `"-in:inbox -in:sent -in:trash -in:spam"` | false |
  | (other) | → `AppError::Other("unknown folder")` |
  `max` clamped to 1..=50. Hydrate via `get_message_previews(ids, PREVIEW_CONCURRENCY)`, sort by `internal_date` desc. **Not** classified (category dots are an INBOX concept — folder results leave `category` empty). DB-free.
- `restore_message`: `untrash_message(&id)` (DB-free — Trash isn't cached).
- `delete_message_forever`: `delete_message_forever(&id)` then lock DB + `db::delete_messages(std::slice::from_ref(&id))` (drops it from the INBOX cache if it happened to be there). MutexGuard taken after the await, per convention.
- All three registered in `lib.rs`.

### Backend — tests (`src-tauri/tests/gmail_test.rs`)
- `list_folder_message_ids` with `include_spam_trash=true` sends `includeSpamTrash=true` + the label; Archive variant sends the query and omits `labelIds`.
- Regression: `list_inbox_message_ids_paged` / `search_message_ids` do **not** send `includeSpamTrash`.
- `untrash_message` POSTs to `/messages/{id}/untrash`; `delete_message_forever` issues `DELETE /messages/{id}`.

### Frontend — `src/lib/folders.ts` (NEW)
```ts
export type Folder = "inbox" | "sent" | "starred" | "archive" | "trash" | "spam";
export interface FolderDef { key: Folder; label: string } // icon chosen in FolderRail
export const FOLDERS: FolderDef[]; // ordered for the rail
```

### Frontend — `src/lib/api.ts`
- Add `to_addr: string` to the `MessagePreview` interface (the backend already returns it).
- `fetchFolder(folder, max=50)`, `restoreMessage(id)`, `deleteMessageForever(id)` wrappers, each `isTauri() ? invoke(...) : mock`. `mock.ts` gains `mockFolder(folder)` returning a small per-folder set (Sent entries carry `to_addr`).

### Frontend — `src/components/FolderRail.tsx` (NEW) + CSS
A slim left rail rendering `FOLDERS` as icon+label buttons (lucide `Inbox`, `Send`, `Star`, `Archive`, `Trash2`, `ShieldAlert`); the active folder highlighted; `onSelectFolder(key)`. ~74px wide; styled with existing tokens (`--surface-2`, `--accent-weak`, `--accent-text`). Also add a `.mail-body { flex:1; min-height:0; display:flex }` wrapper rule so the rail + `SplitView` sit side-by-side and the SplitView flexes to fill the remaining width.

### Frontend — `src/App.tsx`
- New state: `folder: Folder` (default `"inbox"`), `folderResults: MessagePreview[]`, `folderSelectedId: string | null`, `folderLoading: boolean`.
- **3-way active list** (extends M11), with `inFolder = folder !== "inbox"`:
  ```ts
  const activeList          = inSearch ? searchResults  : inFolder ? folderResults    : messages;
  const setActiveList       = inSearch ? setSearchResults : inFolder ? setFolderResults : setMessages;
  const activeSelectedId    = inSearch ? searchSelectedId : inFolder ? folderSelectedId : selectedId;
  const setActiveSelectedId = inSearch ? setSearchSelectedId : inFolder ? setFolderSelectedId : setSelectedId;
  ```
  (All three list setters are `Dispatch<SetStateAction<MessagePreview[]>>`; all three id setters `…<string|null>` — the nested ternaries unify.)
- `nextSelectedId`: flat order when `inSearch || inFolder`, else `orderedForStream(messages, stream)`.
- `handleSelectFolder(f)`: set `folder=f`; clear search (`inSearch=false`, results/selection cleared); if `f === "inbox"` nothing more (cached inbox shows); else `folderLoading=true` → `fetchFolder(f)` → `folderResults` + `folderSelectedId=null`, with error → inline error + retry (reuses the M11 pattern, keyed by a reload counter).
- `handleRestore(m)` = `removeWithAction(m, () => restoreMessage(m.id))`; `handleDeleteForever(m)` = `removeWithAction(m, () => deleteMessageForever(m.id))` — both reuse the list-aware optimistic removal.
- Render: mail body becomes `<div className="mail-body"><FolderRail folder onSelectFolder/><SplitView …/></div>`. `MessageList` gets `messages={activeList}`, `flat={inSearch || inFolder}`, a folder/search-appropriate `title`/`emptyText`, and `showRecipient={folder === "sent"}`. `ReadingPane` gets `folder` + `onRestore`/`onDeleteForever`.
- Header stream tabs show only when `folder === "inbox" && !inSearch` → pass `inFolder` to Header (hide streams when in a folder).

### Frontend — `src/components/MessageList.tsx` + `MessageItem.tsx`
- `MessageList` gains `showRecipient?: boolean`, passed through to `MessageItem`.
- `MessageItem` shows `msg.to_addr` (prefixed "To: ") instead of `msg.from` when `showRecipient` — used for the Sent folder.

### Frontend — `src/components/Header.tsx`
- Add optional `inFolder?: boolean`; the stream nav renders only when `account && !isCal && !inSearch && !inFolder`.

### Frontend — `src/components/ReadingPane.tsx`
- New optional props: `folder?: Folder`, `onRestore?: (m) => void`, `onDeleteForever?: (m) => void`.
- When `folder === "trash"`: replace the Archive/Trash buttons with **Restore** and **Delete forever**; Delete-forever uses an inline two-step confirm ("Delete forever?" → confirm) because it is irreversible. Star/Reply remain. Other folders: the existing buttons.

---

## Data flow

**Select a folder:** rail click → `handleSelectFolder(f)` → (non-inbox) `fetchFolder(f)` → `folderResults` → the two-pane shows the flat list (Sent shows recipients). Inbox → the cached smart inbox + stream tabs return.

**Act in a folder:** open a result (live `fetch_message_body`, marks read); star/archive/trash/reply via the list-aware handlers on `folderResults`. In **Trash**: Restore (untrash) or Delete-forever (confirm → permanent DELETE) → optimistic removal from `folderResults`.

**Search interplay:** running a search overrides the folder view (search is global). Clearing search returns to the current folder.

**Maket (browser):** `!isTauri()` → `mockFolder` returns per-folder mock messages; Trash actions/mutations aren't exercised in the maket.

---

## Error handling

- **Permanent delete is irreversible** → guarded by an explicit inline confirm in the reading pane before `deleteMessageForever` is called.
- Folder fetch failure → inline error + Retry (reuses the M11 results-error pattern); the inbox is untouched.
- Action / restore / delete failure → optimistic change rolls back on the active (folder) list, error surfaces (existing list-aware rollback).
- `fetch_folder` for an unknown folder string → `AppError::Other("unknown folder: …")`.
- `delete_message_forever`'s DB cleanup takes the MutexGuard only after the network await (no guard across `.await`).

## Testing strategy

- **Rust** (wiremock): `include_spam_trash` flag wiring (Trash/Spam send it, inbox/search don't); Archive sends the query without a label; `untrash_message` → `/untrash` POST; `delete_message_forever` → `DELETE`.
- **Frontend**: no JS runner (consistent with M10–M11). Verified via the maket (mock folders, recipient-in-Sent, the 3-way active list) + live E2E.
- **Maket E2E (browser):** `npm run dev` → click each rail folder → mock list renders; Sent shows "To:".
- **Live E2E (Tauri):** open Sent/Starred/Archive/Trash/Spam and confirm real mail; Sent shows recipients; in Trash, Restore returns a message to Gmail and Delete-forever permanently removes it (after confirm); star/archive/trash/reply work from a folder; search still overrides and clears back to the folder.

## Definition of done

- The folder rail switches between Inbox · Sent · Starred · Archive · Trash · Spam; non-inbox folders live-fetch and render flat, most-recent-first; Sent shows recipients.
- Actions work in folders; Trash has working Restore + Delete-forever (confirmed, irreversible).
- Inbox + search behavior unchanged (the 3-way active list is behavior-preserving for those paths).
- App (folders included) runs in the browser maket; the Tauri build is unchanged when `isTauri()` is true.
- New Rust code carries `// 🦀` comments; a plain-English Rust recap per Rust task.
- `cargo test` green (existing + new folder/untrash/delete tests); `cargo clippy --all-targets -- -D warnings` clean; `npm run build` clean. No DB migration; no new OAuth scope (`gmail.modify` already permits trash/untrash/delete + reading any label).

## Known limitations (carried as deferrals)

- Folder results aren't cached; switching folders re-hits Gmail; only the first 50 per folder.
- An in-place label action inside a folder (e.g. unstar in Starred, archive in Archive) updates the row but doesn't drop it from the current folder list until you re-open the folder; ordering is recency, not Gmail relevance.
- No per-folder unread counts, no batch/empty-folder operations, no arbitrary-label folders, no offline folder browsing.
- Acting on a folder result doesn't live-refresh the cached inbox list (reconciles on next sync), same as M11.
