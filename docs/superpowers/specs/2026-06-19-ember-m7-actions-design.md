# Ember — Milestone 7: Actions (lean core) — Design Spec

**Status:** Approved design (2026-06-19). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Give the user the four core mailbox actions on their live Gmail account —
**mark read/unread**, **star/unstar**, **archive**, **trash** — with an optimistic UI that
updates instantly and rolls back on failure. This is Ember's first **write-path** milestone:
until now the app only reads (sync + bodies) and classifies (M6); M7 begins mutating the real
mailbox.

**Architecture in one paragraph:** Each action maps to a Gmail **label** operation
(`UNREAD`, `STARRED`, `INBOX`) via `users.messages.modify`, except trash which uses the
dedicated `users.messages.trash` endpoint. The Gmail client — GET-only today — gains a POST
path and two methods. Four thin, semantic Tauri commands perform the durable work (Gmail call
first, then the local DB). The React frontend keeps `messages` as its source of truth and
applies each change **optimistically**: snapshot → apply locally → `invoke` → roll back to the
snapshot and surface an error on rejection. Because read/star map to labels, and `label_ids`
is **already synced, stored, and serialized to the frontend**, M7 needs **no schema change and
no migration** — only one new DB helper that updates the `label_ids` column.

**Tech Stack:** Rust (rusqlite, reqwest, serde, wiremock for tests), Tauri 2, React 19 +
TypeScript + Vite, lucide-react icons.

**Learning mode (IMPORTANT — applies to every implementer):** The repo owner is learning Rust.
All Rust code MUST include concise `// 🦀` teaching comments explaining the *language* concept
(ownership/borrowing, `Result`/`Option`/`?`, `match`, traits/`impl`, lifetimes `'a`, slices
`&[T]`, closures, derive macros, async/`.await`), not just intent. After each task, give a short
plain-English recap of the Rust concepts it introduced. TypeScript/React gets normal comments —
the owner knows JS/React.

---

## Milestone context

M1–M6 are merged to `main`. The app reads mail (30-day INBOX sync via history deltas; full
message bodies) and classifies it into People/Notifications/Newsletters streams (M6), but cannot
mutate or send. M7 adds the four core **actions**. Re-sequenced roadmap: **M7 actions** → M8
compose → M9 settings/onboarding → M10 calendar.

The OAuth scope is already `gmail.modify` (granted in M1, deliberately ahead of need), so **no
re-consent** is required for write operations.

---

## Scope

**In scope (lean core):** mark read/unread, star/unstar, archive, trash — single message at a
time. Optimistic UI with rollback. Actions available from the reading-pane toolbar **and** as
hover-revealed icons on list rows. Opening an unread message auto-marks it read. Trash is
immediate (no confirm dialog) — Gmail keeps trashed mail recoverable for 30 days, matching
Gmail web.

**Explicitly deferred (not in M7):**
- Arbitrary user labels (apply/remove existing Gmail labels, label picker).
- **Pin** (local-only "keep at top" state).
- **Snooze** (Gmail has no snooze API → would be a fully local scheduler: hide → wake-timer →
  re-surface; milestone-sized on its own).
- Multi-select **batch** actions (`users.messages.batchModify`). Commands act on one id; a future
  milestone can add batching.
- Undo toast for archive/trash.
- Reconciling read/star changes made on **other** clients in real time (see Known limitations).
- Frontend unit tests (Vitest is not configured — consistent with M4/M5).

---

## Action → Gmail mapping

| Action        | Gmail call                                  | Local DB effect                          |
|---------------|---------------------------------------------|------------------------------------------|
| Mark read     | `modify` remove `UNREAD`                    | update row `label_ids`                   |
| Mark unread   | `modify` add `UNREAD`                        | update row `label_ids`                   |
| Star          | `modify` add `STARRED`                       | update row `label_ids`                   |
| Unstar        | `modify` remove `STARRED`                    | update row `label_ids`                   |
| Archive       | `modify` remove `INBOX`                      | delete row (leaves the inbox cache)      |
| Trash         | `messages/{id}/trash` (dedicated endpoint)  | delete row                               |

Archive and trash both remove the message from the local INBOX cache — the same effect the sync
delta already produces for messages removed upstream (`db::delete_messages`, which already
exists). Read/star keep the row and only rewrite its `label_ids`.

---

## File structure

**Backend (Rust, `src-tauri/`):**
- `src/gmail/types.rs` — **NEW** small `ModifiedMessage { id, label_ids }` deserialize type for
  the `modify` response (robust: does not depend on `payload` being present in the response).
- `src/gmail/mod.rs` — add `post_json<B, T>` + `post_no_body` helpers (mirror `get_json`),
  `modify_message(id, add, remove)`, `trash_message(id)`.
- `src/db/mod.rs` — **one** new helper `update_message_labels(conn, id, label_ids_csv)` (updates
  only `label_ids`; category/signals untouched). No schema change.
- `src/commands.rs` — four commands: `set_message_read`, `set_message_starred`,
  `archive_message`, `trash_message`.
- `src/lib.rs` — register the four commands in `generate_handler!`.
- `tests/gmail_test.rs` — wiremock tests for `modify_message` and `trash_message`.

**Frontend (`src/`):**
- `lib/api.ts` — add `label_ids: string[]` to `MessagePreview` (already on the wire); add four
  `invoke` wrappers.
- `lib/labels.ts` — **NEW**, pure: `isUnread(m)`, `isStarred(m)`, `withLabel(m, label, present)`
  (returns an updated copy for optimistic state), and the `UNREAD` / `STARRED` constants.
- `App.tsx` — owns the optimistic engine (snapshot/apply/rollback) and the action handlers;
  auto-mark-read on open; advance selection after archive/trash.
- `components/MessageItem.tsx` — unread styling (bold) + star indicator; hover-revealed archive +
  star icons (`stopPropagation` so they don't open the message).
- `components/ReadingPane.tsx` — wire the existing disabled Archive/Trash buttons; add Star toggle
  + Mark-unread. Reply stays disabled (M8).
- `styles/app.css` — unread/starred row styles, hover-action styling.

---

## Component contracts

### Gmail client (`gmail/mod.rs`)

```rust
// 🦀 Mirrors get_json: POST a JSON body, bearer auth, turn 4xx/5xx into errors,
//    deserialize the response into T.
async fn post_json<B: Serialize, T: DeserializeOwned>(&self, url: &str, body: &B) -> Result<T>;

// 🦀 POST with no request body (the trash endpoint). Response body is ignored.
async fn post_no_body(&self, url: &str) -> Result<()>;

/// Add/remove labels on one message; returns the message's new label set.
pub async fn modify_message(
    &self, id: &str, add: &[&str], remove: &[&str],
) -> Result<ModifiedMessage>;

/// Move one message to Trash (recoverable ~30 days).
pub async fn trash_message(&self, id: &str) -> Result<()>;
```

`modify` request body serializes to `{ "addLabelIds": [...], "removeLabelIds": [...] }`
(serde rename). `ModifiedMessage` parses `{ id, labelIds }` from the response so the command can
persist Gmail's authoritative post-mutation label set.

### Tauri commands (`commands.rs`)

```rust
#[tauri::command] pub async fn set_message_read(id: String, read: bool, state: State<'_, Db>) -> Result<()>;
#[tauri::command] pub async fn set_message_starred(id: String, starred: bool, state: State<'_, Db>) -> Result<()>;
#[tauri::command] pub async fn archive_message(id: String, state: State<'_, Db>) -> Result<()>;
#[tauri::command] pub async fn trash_message(id: String, state: State<'_, Db>) -> Result<()>;
```

Each: `ensure_access_token` → build `GmailClient` → **Gmail call first** → **then DB**. Read/star
persist the labels returned by `modify` via `db::update_message_labels`; archive/trash call
`db::delete_messages(&[id])`. As elsewhere, the `Mutex` guard is taken in a short await-free block
(never held across `.await`).

### DB (`db/mod.rs`)

```rust
/// Replace a message's stored label set (used by read/star). Other columns untouched.
pub fn update_message_labels(conn: &Connection, id: &str, label_ids_csv: &str) -> Result<()>;
```

### Frontend optimistic engine (`App.tsx` + `lib/labels.ts`)

`lib/labels.ts` is pure and side-effect-free:
```ts
export const UNREAD = "UNREAD";
export const STARRED = "STARRED";
export const isUnread  = (m: MessagePreview) => m.label_ids.includes(UNREAD);
export const isStarred = (m: MessagePreview) => m.label_ids.includes(STARRED);
export const withLabel = (m: MessagePreview, label: string, present: boolean): MessagePreview => /* copy with label added/removed */;
```

`App.tsx` runs every action through one path:
1. `const snapshot = messages;`
2. compute and `setMessages(next)` (optimistic): read/star rewrite the row's `label_ids` via
   `withLabel`; archive/trash filter the row out and advance `selectedId` to the next visible row.
3. `await invoke(...)`. On rejection: `setMessages(snapshot)` and show the error in the existing
   error bar.

Auto-mark-read: a wrapped `onSelect` selects the message and, if it `isUnread`, fires
`set_message_read(id, true)` through the same optimistic path.

---

## Data flow (archive example)

1. User clicks Archive (toolbar or row hover).
2. `App` snapshots `messages`, removes the row, advances selection — UI updates instantly.
3. `invoke("archive_message", { id })`.
4. Command: refresh token → `modify_message(id, &[], &["INBOX"])` → `db::delete_messages(&[id])`.
5. Success → optimistic state stands. The next sync's history delta will also report the message
   as removed; `apply_delta`'s delete is idempotent (the row is already gone).
6. Failure → command returns `Err`; `App` restores the snapshot and shows the error.

---

## Error handling

- **Optimistic rollback** to the pre-action snapshot on any command rejection, surfaced in the
  existing error bar (`App` already renders `error`).
- **401 / expired token** is handled by `ensure_access_token`, which every command calls before
  the Gmail request (refresh path already exists from M1).
- **Ordering (Gmail → DB)** guarantees a Gmail failure leaves the DB untouched, so rollback is
  clean. The rare case — Gmail succeeds but the subsequent DB write fails — returns `Err` (UI
  rolls back) while Gmail is already mutated; this transient drift self-heals on the next sync.

---

## Testing strategy

- `tests/gmail_test.rs` (wiremock):
  - `modify_message` — assert the request is `POST …/messages/{id}/modify`, assert the JSON body
    contains the expected `addLabelIds`/`removeLabelIds`, and that the returned `labelIds` parse
    into `ModifiedMessage`.
  - `trash_message` — assert `POST …/messages/{id}/trash` and that a success response is `Ok`.
- `db` unit test: `update_message_labels` changes only `label_ids` and preserves `category` and
  the M6 signal columns.
- `lib/labels.ts` is kept pure so it is unit-testable the moment Vitest is added (none now).
- **Manual E2E** against live Gmail (project norm): archive/trash a message and confirm it leaves
  the inbox and appears in Gmail's Archive/Trash; toggle read and star and confirm both reflect in
  Gmail web; verify rollback by simulating a failure (e.g. offline).

---

## Known limitations (carried as deferrals)

- **Single message only** — no multi-select/`batchModify` yet.
- **Cross-client label drift** — read/star changes made on *another* client are not reflected by
  the INBOX history delta (it only nets INBOX membership, not arbitrary label changes — a
  pre-existing limitation noted in M6). They reconcile on the next full resync. Our own actions
  persist Gmail's returned labels, so the acted-on message is always correct locally.
- **No undo** for archive/trash (trash is recoverable in Gmail for ~30 days).
- **Snooze, pin, arbitrary labels** deferred per the scope decision.

---

## Definition of done

- Four commands implemented, registered, and reachable from the frontend.
- All four actions work end-to-end against live Gmail (manually verified) with optimistic UI +
  rollback.
- Actions available from both the reading-pane toolbar and list-row hover; unread/starred state is
  visible in the list; opening an unread message marks it read.
- New Rust code carries `// 🦀` teaching comments; a plain-English Rust recap accompanies each task.
- `cargo test` green (existing suite + new gmail/db tests); `cargo clippy`/build clean;
  `tsc`/Vite build clean.
- No schema migration introduced (verified — `label_ids` reused).
