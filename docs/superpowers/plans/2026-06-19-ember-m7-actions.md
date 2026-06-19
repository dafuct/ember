# Ember — Milestone 7: Actions (lean core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the user four core mailbox actions on their live Gmail account — mark read/unread, star/unstar, archive, trash — with an optimistic UI that updates instantly and rolls back on failure.

**Architecture:** Each action is a Gmail **label** operation (`UNREAD`/`STARRED`/`INBOX`) via `users.messages.modify`, except trash which uses the dedicated `users.messages.trash` endpoint. The GET-only Gmail client gains a POST path + two methods; four thin Tauri commands do the durable work (Gmail first, then local DB). The React frontend keeps `messages` as its source of truth and applies each change optimistically (snapshot → apply → invoke → roll back on rejection). Read/star reuse the already-synced `label_ids` column, so there is **no schema change and no migration**.

**Tech Stack:** Rust (rusqlite, reqwest, serde, wiremock for tests), Tauri 2, React 19 + TypeScript + Vite, lucide-react icons.

**Design source:** `docs/superpowers/specs/2026-06-19-ember-m7-actions-design.md` (approved).

**Learning mode (IMPORTANT — applies to every implementer):** The repo owner is learning Rust. All Rust code MUST include concise `// 🦀` teaching comments explaining the *language* concept (ownership/borrowing, `Result`/`Option`/`?`, slices `&[T]`, lifetimes `'a`, closures, derive macros, async/`.await`), not just intent. After each task, give a short plain-English recap of the Rust concepts it introduced. TypeScript/React gets normal comments — the owner knows JS/React.

**Environment note:** `cargo`/`rustc` are symlinked into `/opt/homebrew/bin`; backend commands run from `src-tauri/`. Frontend commands run from the repo root.

---

## Milestone context

M1–M6 are merged to `main`. The app reads mail (30-day INBOX sync via history deltas; full message bodies) and classifies it into People/Notifications/Newsletters (M6) but cannot mutate or send. M7 adds the four core actions. The OAuth scope is already `gmail.modify` (granted in M1), so **no re-consent** is required.

**Scope (lean core):** read/unread, star, archive, trash — single message at a time, optimistic UI. Actions live on the reading-pane toolbar AND as hover-revealed list-row icons. Opening an unread message auto-marks it read. Trash is immediate (no confirm; Gmail keeps trash recoverable ~30 days). **Deferred:** arbitrary labels, pin, snooze, multi-select batch, undo toast, real-time cross-client read/star reconciliation, frontend unit tests (no Vitest).

---

## File structure

**Backend (`src-tauri/`):**
- `src/gmail/types.rs` — add `ModifiedMessage` deserialize type.
- `src/gmail/mod.rs` — add `post_json`/`post_no_body` helpers, `modify_message`, `trash_message`; import `ModifiedMessage`.
- `src/db/mod.rs` — add `update_message_labels` + a unit test.
- `src/commands.rs` — add `set_message_read`, `set_message_starred`, `archive_message`, `trash_message` (+ private `set_label` helper).
- `src/lib.rs` — register the four commands.
- `tests/gmail_test.rs` — wiremock tests for `modify_message` and `trash_message`.

**Frontend (`src/`):**
- `lib/api.ts` — add `label_ids` to `MessagePreview`; add four `invoke` wrappers.
- `lib/labels.ts` — **NEW** pure helpers: `isUnread`, `isStarred`, `withLabel`, constants.
- `App.tsx` — optimistic action handlers + rollback + auto-mark-read + selection advance.
- `components/MessageList.tsx` — thread `onArchive`/`onStar` to rows.
- `components/MessageItem.tsx` — unread/star styling + hover action buttons (structural change: row becomes a flex container).
- `components/ReadingPane.tsx` — wire toolbar (star, mark-unread, archive, trash).
- `styles/app.css` — row layout/action styles + `.icon-btn.active`.

---

## Task 1: DB helper — `update_message_labels`

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (add fn after `delete_messages`, ~line 214; add test in the `tests` module)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/db/mod.rs` (after `delete_messages_removes_only_given_ids`):

```rust
    #[test]
    fn update_message_labels_changes_only_labels() {
        let c = conn();
        let mut m = msg("a", 1);
        m.category = "people".into();
        m.label_ids = "INBOX,UNREAD".into();
        upsert_messages(&c, &[m]).unwrap();

        update_message_labels(&c, "a", "INBOX").unwrap();

        let rows = recent_previews(&c, 10).unwrap();
        assert_eq!(rows[0].label_ids, "INBOX"); // UNREAD removed
        assert_eq!(rows[0].category, "people"); // category untouched
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib update_message_labels_changes_only_labels`
Expected: FAIL — `cannot find function 'update_message_labels'`.

- [ ] **Step 3: Write minimal implementation**

Add after `delete_messages` (around line 214) in `src-tauri/src/db/mod.rs`:

```rust
/// Replace one message's stored label set. Used by the read/star toggles: the
/// message stays in the cache, only its `label_ids` column changes (so its
/// category and the M6 scoring signals are preserved).
pub fn update_message_labels(conn: &Connection, id: &str, label_ids_csv: &str) -> Result<()> {
    // 🦀 `conn.execute` runs one statement with bound params (`?1`, `?2`), which
    //    SQLite escapes for us — never string-format user values into SQL.
    conn.execute(
        "UPDATE messages SET label_ids = ?1 WHERE id = ?2",
        params![label_ids_csv, id],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib update_message_labels_changes_only_labels`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(db): add update_message_labels for read/star toggles"
```

**Rust recap to give the owner:** bound parameters (`?1`/`?2`) vs. string formatting; why we update one column instead of re-upserting the whole row (preserve `category`/signals).

---

## Task 2: Gmail client — `modify_message` (+ `ModifiedMessage`, `post_json`)

**Files:**
- Modify: `src-tauri/src/gmail/types.rs` (add `ModifiedMessage`)
- Modify: `src-tauri/src/gmail/mod.rs` (import it; add `post_json` + `modify_message`)
- Test: `src-tauri/tests/gmail_test.rs` (add wiremock test + `body_json` import)

- [ ] **Step 1: Write the failing test**

In `src-tauri/tests/gmail_test.rs`, change the matchers import (line 7) to add `body_json`:

```rust
use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
```

Append this test at the end of the file:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn modify_message_posts_labels_and_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/a1/modify"))
        .and(body_json(json!({ "addLabelIds": [], "removeLabelIds": ["UNREAD"] })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "labelIds": ["INBOX", "STARRED"]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let m = client.modify_message("a1", &[], &["UNREAD"]).await.unwrap();
    assert_eq!(m.id, "a1");
    assert_eq!(
        m.label_ids,
        vec!["INBOX".to_string(), "STARRED".to_string()]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test modify_message_posts_labels_and_parses_response`
Expected: FAIL — `no method named 'modify_message'`.

- [ ] **Step 3a: Add the `ModifiedMessage` type**

Append to `src-tauri/src/gmail/types.rs`:

```rust
/// The subset of the `users.messages.modify` response we use: the id and the
/// label set after the change. We don't request `payload`, so we don't model it —
/// keeping this type small means the parse never fails on a missing `payload`.
#[derive(Debug, Deserialize)]
pub struct ModifiedMessage {
    pub id: String,
    // 🦀 `default` makes serde fill an empty Vec if Gmail omits `labelIds`
    //    (it shouldn't, but this keeps the deserialize total/robust).
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
}
```

- [ ] **Step 3b: Import it and add `post_json` + `modify_message`**

In `src-tauri/src/gmail/mod.rs`, add `ModifiedMessage` to the `use types::{...}` line (line 6):

```rust
use types::{FullMessage, HistoryResponse, MessageList, MessagePart, MessagePreview, ModifiedMessage, Profile, RawMessage};
```

Inside `impl GmailClient`, add the POST helper next to `get_json` (after line 117):

```rust
    // 🦀 The write-side twin of get_json: serialize `body` to JSON, POST it with
    //    bearer auth, turn 4xx/5xx into errors, then deserialize the response into T.
    //    `B: serde::Serialize` is the request body type; `T` the response type.
    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }
```

Add `modify_message` as a public method (e.g. after `get_message_body`, near line 356):

```rust
    /// Add and/or remove labels on a single message. Returns the message's label
    /// set *after* the change (Gmail echoes the updated resource), so the caller can
    /// persist the server-authoritative labels.
    pub async fn modify_message(
        &self,
        id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> Result<ModifiedMessage> {
        // 🦀 A short-lived request struct whose serde field names match Gmail's JSON
        //    (`addLabelIds`/`removeLabelIds`). The `<'a>` lifetime ties the borrowed
        //    slices to the struct so we serialize without cloning the label strings.
        #[derive(serde::Serialize)]
        struct ModifyRequest<'a> {
            #[serde(rename = "addLabelIds")]
            add_label_ids: &'a [&'a str],
            #[serde(rename = "removeLabelIds")]
            remove_label_ids: &'a [&'a str],
        }
        let url = format!("{}/gmail/v1/users/me/messages/{}/modify", self.base_url, id);
        let body = ModifyRequest {
            add_label_ids: add,
            remove_label_ids: remove,
        };
        self.post_json(&url, &body).await
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --test gmail_test modify_message_posts_labels_and_parses_response`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): add modify_message (label add/remove) with POST helper"
```

**Rust recap:** generic functions with trait bounds (`B: Serialize`, `T: DeserializeOwned`); a struct defined inside a fn; lifetimes `'a` tying borrowed slices to a struct; `&[&str]` (a slice of string slices).

---

## Task 3: Gmail client — `trash_message`

**Files:**
- Modify: `src-tauri/src/gmail/mod.rs` (add `post_no_body` + `trash_message`)
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn trash_message_posts_to_trash_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/a1/trash"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "a1",
            "labelIds": ["TRASH"]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    // 🦀 We only care that it succeeded; the response body is ignored.
    client.trash_message("a1").await.unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test trash_message_posts_to_trash_endpoint`
Expected: FAIL — `no method named 'trash_message'`.

- [ ] **Step 3: Write minimal implementation**

In `src-tauri/src/gmail/mod.rs`, add a bodyless POST helper next to `post_json`:

```rust
    // 🦀 POST with no request body — Gmail's trash endpoint takes none. We only
    //    need to know it succeeded, so the response body is discarded.
    async fn post_no_body(&self, url: &str) -> Result<()> {
        self.http
            .post(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
```

Add `trash_message` next to `modify_message`:

```rust
    /// Move a single message to Trash (recoverable in Gmail for ~30 days).
    pub async fn trash_message(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}/trash", self.base_url, id);
        self.post_no_body(&url).await
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --test gmail_test trash_message_posts_to_trash_endpoint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): add trash_message via the trash endpoint"
```

**Rust recap:** returning `Result<()>` (the unit type — "succeeded, no value"); discarding a response with `?` + `Ok(())`.

---

## Task 4: Tauri commands + registration

No automated test (these need a live token + network; the existing commands are likewise not unit-tested). The gate is: it compiles, `cargo clippy` is clean, and the four commands are registered. The Gmail/DB layers underneath are already tested in Tasks 1–3.

**Files:**
- Modify: `src-tauri/src/commands.rs` (add helper + four commands)
- Modify: `src-tauri/src/lib.rs` (register them)

- [ ] **Step 1: Add the commands**

Append to `src-tauri/src/commands.rs` (the `use` block already imports `AppError`, `ensure_access_token`, `db`, `GmailClient`, `Result`):

```rust
// 🦀 Shared core for the label-toggle actions (read/star). `present` decides whether
//    the label is added or removed. We call Gmail FIRST; only on success do we take
//    the DB lock and persist the label set Gmail returns, so a network failure leaves
//    the local cache untouched (the frontend then rolls back its optimistic update).
//    The std MutexGuard is created AFTER every `.await`, never held across one.
async fn set_label(
    id: &str,
    label: &str,
    present: bool,
    state: &tauri::State<'_, Db>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 Pass the one-element slice directly as an argument so its temporary lives
    //    for the call (a `let` binding of `&[label]` would be dropped too early).
    let modified = if present {
        client.modify_message(id, &[label], &[]).await?
    } else {
        client.modify_message(id, &[], &[label]).await?
    };
    let csv = modified.label_ids.join(",");
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::update_message_labels(&conn, id, &csv)?;
    Ok(())
}

/// Mark a message read (`read = true` → remove UNREAD) or unread (`read = false` → add UNREAD).
#[tauri::command]
pub async fn set_message_read(
    id: String,
    read: bool,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    // 🦀 read == true means the UNREAD label should be ABSENT, so `present = !read`.
    set_label(&id, "UNREAD", !read, &state).await
}

/// Star (`starred = true`) or unstar (`starred = false`) a message via the STARRED label.
#[tauri::command]
pub async fn set_message_starred(
    id: String,
    starred: bool,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    set_label(&id, "STARRED", starred, &state).await
}

/// Archive: remove the INBOX label so the message leaves the inbox, then drop it from
/// the local cache (the next sync's history delta would remove it too — delete is idempotent).
#[tauri::command]
pub async fn archive_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.modify_message(&id, &[], &["INBOX"]).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 `std::slice::from_ref(&id)` makes a one-element `&[String]` without allocating.
    db::delete_messages(&conn, std::slice::from_ref(&id))?;
    Ok(())
}

/// Move a message to Trash and drop it from the local cache.
#[tauri::command]
pub async fn trash_message(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.trash_message(&id).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, std::slice::from_ref(&id))?;
    Ok(())
}
```

- [ ] **Step 2: Register the commands**

In `src-tauri/src/lib.rs`, extend the `tauri::generate_handler![...]` list (currently ends at `commands::fetch_message_body,` near line 87):

```rust
        .invoke_handler(tauri::generate_handler![
            commands::connect_gmail,
            commands::get_connected_account,
            commands::fetch_inbox_preview,
            commands::sync_inbox,
            commands::fetch_message_body,
            commands::set_message_read,
            commands::set_message_starred,
            commands::archive_message,
            commands::trash_message,
        ])
```

- [ ] **Step 3: Build + lint**

Run: `cd src-tauri && cargo build && cargo clippy --all-targets -- -D warnings`
Expected: compiles clean, no clippy warnings.

- [ ] **Step 4: Run the full backend test suite (no regressions)**

Run: `cd src-tauri && cargo test`
Expected: PASS — existing suite + the Task 1–3 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add read/star/archive/trash actions and register them"
```

**Rust recap:** factoring shared async logic into a private helper that borrows `&State`; `std::slice::from_ref`; why Gmail-then-DB ordering makes rollback clean; the rule that a std `MutexGuard` must not be held across `.await`.

---

## Task 5: Frontend API wrappers + pure label helpers

**Files:**
- Modify: `src/lib/api.ts`
- Create: `src/lib/labels.ts`

- [ ] **Step 1: Extend `MessagePreview` and add the action wrappers**

In `src/lib/api.ts`, add `label_ids` to the interface (after `category`):

```ts
export interface MessagePreview {
  id: string;
  thread_id: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
  internal_date: number;
  /** Smart-inbox stream from the backend scorer: "people" | "notifications" | "newsletters". */
  category: string;
  /** Raw Gmail label ids (e.g. "INBOX", "UNREAD", "STARRED"). Drives read/star state. */
  label_ids: string[];
}
```

Add the four wrappers at the end of the file:

```ts
export const setMessageRead = (id: string, read: boolean): Promise<void> =>
  invoke<void>("set_message_read", { id, read });
export const setMessageStarred = (id: string, starred: boolean): Promise<void> =>
  invoke<void>("set_message_starred", { id, starred });
export const archiveMessage = (id: string): Promise<void> =>
  invoke<void>("archive_message", { id });
export const trashMessage = (id: string): Promise<void> =>
  invoke<void>("trash_message", { id });
```

- [ ] **Step 2: Create the pure label helpers**

Create `src/lib/labels.ts`:

```ts
import type { MessagePreview } from "./api";

// Gmail's system label ids that map to read/star state.
export const UNREAD = "UNREAD";
export const STARRED = "STARRED";

export const isUnread = (m: MessagePreview): boolean =>
  m.label_ids.includes(UNREAD);
export const isStarred = (m: MessagePreview): boolean =>
  m.label_ids.includes(STARRED);

/**
 * Return a copy of `m` with `label` present or absent. Pure — never mutates `m`,
 * so it is safe to use for React optimistic state. Returns the same reference
 * when nothing would change (lets callers skip a no-op render).
 */
export function withLabel(
  m: MessagePreview,
  label: string,
  present: boolean,
): MessagePreview {
  const has = m.label_ids.includes(label);
  if (has === present) return m;
  const label_ids = present
    ? [...m.label_ids, label]
    : m.label_ids.filter((l) => l !== label);
  return { ...m, label_ids };
}
```

- [ ] **Step 3: Type-check**

Run: `npm run build`
Expected: `tsc` passes (no new type errors) and Vite builds. (`label_ids` is already serialized by the Rust `MessagePreview`, so the new field matches the wire data.)

- [ ] **Step 4: Commit**

```bash
git add src/lib/api.ts src/lib/labels.ts
git commit -m "feat(ui): add action API wrappers and pure label helpers"
```

---

## Task 6: App.tsx — optimistic action handlers

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add imports**

In `src/App.tsx`, extend the `./lib/api` import to include the four wrappers, and add imports for the label helpers and `filterByStream`:

```tsx
import {
  archiveMessage,
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  setMessageRead,
  setMessageStarred,
  syncInbox,
  trashMessage,
  type MessagePreview,
} from "./lib/api";
import { filterByStream, type Stream } from "./lib/streams";
import { isStarred, isUnread, UNREAD, STARRED, withLabel } from "./lib/labels";
```

(Remove the now-duplicated `import type { Stream } from "./lib/streams";` line — `Stream` is imported above.)

- [ ] **Step 2: Add the handlers**

Inside `App()`, after the `selected` `useMemo` (line ~65), add:

```tsx
  // Pick the row to select after the current one is removed (archive/trash):
  // the next visible row, else the previous, else nothing. Uses the active stream's
  // ordering so selection lands on something the user can actually see.
  function nextSelectedId(removedId: string): string | null {
    const visible = filterByStream(messages, stream);
    const idx = visible.findIndex((m) => m.id === removedId);
    if (idx === -1) return selectedId;
    const next = visible[idx + 1] ?? visible[idx - 1] ?? null;
    return next ? next.id : null;
  }

  // Roll back to `snapshot` and surface the error if the backend call rejects.
  // Captures explicit snapshots (not functional updates) — fine for single-user
  // clicks; rapid concurrent actions may roll back to a slightly stale list.
  async function withMessagesRollback(
    snapshot: MessagePreview[],
    call: () => Promise<void>,
  ) {
    setError(null);
    try {
      await call();
    } catch (e) {
      setMessages(snapshot);
      setError(String(e));
    }
  }

  function toggleRead(m: MessagePreview, read: boolean) {
    const snapshot = messages;
    setMessages(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, UNREAD, !read) : x)),
    );
    void withMessagesRollback(snapshot, () => setMessageRead(m.id, read));
  }

  function toggleStar(m: MessagePreview) {
    const starred = !isStarred(m);
    const snapshot = messages;
    setMessages(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, STARRED, starred) : x)),
    );
    void withMessagesRollback(snapshot, () => setMessageStarred(m.id, starred));
  }

  function removeWithAction(m: MessagePreview, call: () => Promise<void>) {
    const msgsSnap = messages;
    const selSnap = selectedId;
    setMessages(msgsSnap.filter((x) => x.id !== m.id));
    if (selectedId === m.id) setSelectedId(nextSelectedId(m.id));
    setError(null);
    call().catch((e) => {
      setMessages(msgsSnap);
      setSelectedId(selSnap);
      setError(String(e));
    });
  }

  const handleArchive = (m: MessagePreview) =>
    removeWithAction(m, () => archiveMessage(m.id));
  const handleTrash = (m: MessagePreview) =>
    removeWithAction(m, () => trashMessage(m.id));

  // Selecting a message opens it and (if unread) marks it read — like every mail client.
  function handleSelect(id: string) {
    setSelectedId(id);
    const m = messages.find((x) => x.id === id);
    if (m && isUnread(m)) toggleRead(m, true);
  }
```

- [ ] **Step 3: Wire the handlers into the JSX**

In the returned `<SplitView>`, pass `handleSelect` to the list and the action handlers to both panes:

```tsx
      <SplitView
        left={
          <MessageList
            messages={messages}
            stream={stream}
            selectedId={selectedId}
            onSelect={handleSelect}
            onArchive={handleArchive}
            onStar={toggleStar}
          />
        }
        right={
          <ReadingPane
            msg={selected}
            onArchive={handleArchive}
            onTrash={handleTrash}
            onToggleStar={toggleStar}
            onMarkUnread={(m) => toggleRead(m, false)}
          />
        }
      />
```

- [ ] **Step 4: Type-check (will fail until Tasks 7–8 add the new props)**

Run: `npm run build`
Expected: `tsc` ERRORS — `MessageList`/`ReadingPane` don't yet accept `onArchive`/`onStar`/`onTrash`/`onToggleStar`/`onMarkUnread`. That's expected; Tasks 7 and 8 add them. (Do not commit a broken build — commit at the end of Task 8 once the tree type-checks. If you prefer green commits, do Tasks 6–8 as one unit and commit once at the end of Task 8.)

- [ ] **Step 5: (Deferred commit)**

Commit happens at the end of Task 8 (the frontend wiring must type-check as a unit). Proceed to Task 7.

---

## Task 7: MessageItem + MessageList — unread/star state + hover actions

The row changes from a single `<button>` to a flex container (`.msg-item`) holding a main click button (`.msg-item-main`) plus an actions group (`.msg-actions`) — nested buttons are invalid HTML, so the row can no longer itself be a button.

**Files:**
- Modify: `src/components/MessageItem.tsx` (rewrite)
- Modify: `src/components/MessageList.tsx` (thread new props)
- Modify: `src/styles/app.css` (row layout + actions)

- [ ] **Step 1: Rewrite `MessageItem.tsx`**

Replace the entire contents of `src/components/MessageItem.tsx`:

```tsx
import type { MessagePreview } from "../lib/api";
import { isStarred, isUnread } from "../lib/labels";
import { relativeTime } from "../lib/time";
import { Archive, Star } from "lucide-react";

export function MessageItem({
  msg,
  selected,
  onSelect,
  onArchive,
  onStar,
}: {
  msg: MessagePreview;
  selected: boolean;
  onSelect: (id: string) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
}) {
  const unread = isUnread(msg);
  const starred = isStarred(msg);
  const cls = ["msg-item", selected && "selected", unread && "unread"]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={cls}>
      <button className="msg-item-main" onClick={() => onSelect(msg.id)}>
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
        <span className="msg-subject">{msg.subject || "(no subject)"}</span>
        <span className="msg-snippet">{msg.snippet}</span>
      </button>
      <div className="msg-actions">
        <button
          className={starred ? "row-act starred" : "row-act"}
          aria-label={starred ? "Unstar" : "Star"}
          onClick={(e) => {
            e.stopPropagation();
            onStar(msg);
          }}
        >
          <Star size={14} fill={starred ? "currentColor" : "none"} />
        </button>
        <button
          className="row-act"
          aria-label="Archive"
          onClick={(e) => {
            e.stopPropagation();
            onArchive(msg);
          }}
        >
          <Archive size={14} />
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Thread the props through `MessageList.tsx`**

In `src/components/MessageList.tsx`, add `onArchive`/`onStar` to the props type and pass them to every `<MessageItem>`.

Props type (extend the destructured params + type):

```tsx
export function MessageList({
  messages,
  stream,
  selectedId,
  onSelect,
  onArchive,
  onStar,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
}) {
```

Grouped branch `<MessageItem>` (inside `group.messages.map`):

```tsx
                <MessageItem
                  key={m.id}
                  msg={m}
                  selected={m.id === selectedId}
                  onSelect={onSelect}
                  onArchive={onArchive}
                  onStar={onStar}
                />
```

Flat branch `<MessageItem>` (inside `visible.map`):

```tsx
            <MessageItem
              key={m.id}
              msg={m}
              selected={m.id === selectedId}
              onSelect={onSelect}
              onArchive={onArchive}
              onStar={onStar}
            />
```

- [ ] **Step 3: Update the CSS**

In `src/styles/app.css`, REPLACE the existing `.msg-item` rule (line 47) and its `:hover`/`.selected` rules (lines 47–49) with:

```css
.msg-item { display: flex; align-items: stretch; width: 100%; border-bottom: 1px solid var(--border); border-left: 2px solid transparent; cursor: pointer; color: var(--text); }
.msg-item:hover:not(.selected) { background: var(--surface-2); }
.msg-item.selected { background: var(--accent-weak); border-left-color: var(--accent); }
.msg-item-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; padding: 10px 14px; border: none; background: transparent; text-align: left; cursor: pointer; color: inherit; }
/* Unread: bolder sender + subject */
.msg-item.unread .msg-sender { font-weight: 700; }
.msg-item.unread .msg-subject { font-weight: 600; }
```

Then append, at the end of the file, the row-action styles:

```css
/* M7 actions — list-row hover actions */
.msg-actions { display: flex; align-items: center; gap: 2px; padding: 0 8px; flex-shrink: 0; }
.row-act { display: inline-flex; align-items: center; justify-content: center; width: 26px; height: 26px; border: none; border-radius: 6px; background: transparent; color: var(--text-muted); cursor: pointer; opacity: 0; }
.msg-item:hover .row-act, .msg-item:focus-within .row-act { opacity: 1; }
.row-act:hover { background: var(--surface); color: var(--text); }
/* A starred message keeps its star lit even when the row isn't hovered. */
.row-act.starred { opacity: 1; color: var(--accent); }
```

- [ ] **Step 4: Type-check**

Run: `npm run build`
Expected: `MessageItem`/`MessageList` errors are gone. `ReadingPane` errors remain (fixed in Task 8).

- [ ] **Step 5: (Deferred commit)** — proceed to Task 8.

---

## Task 8: ReadingPane — wire the toolbar

**Files:**
- Modify: `src/components/ReadingPane.tsx`
- Modify: `src/styles/app.css` (add `.icon-btn.active`)

- [ ] **Step 1: Update `ReadingPane.tsx`**

In `src/components/ReadingPane.tsx`, change the icon import and add label-helper import:

```tsx
import { Mail, MailOpen, Archive, Trash2, Star, CornerUpLeft } from "lucide-react";
import { isStarred } from "../lib/labels";
```

Change the component signature to accept the action props:

```tsx
export function ReadingPane({
  msg,
  onArchive,
  onTrash,
  onToggleStar,
  onMarkUnread,
}: {
  msg: MessagePreview | null;
  onArchive: (m: MessagePreview) => void;
  onTrash: (m: MessagePreview) => void;
  onToggleStar: (m: MessagePreview) => void;
  onMarkUnread: (m: MessagePreview) => void;
}) {
```

Replace the `reading-toolbar` block (lines 59–69) with wired buttons (this code runs only in the `msg`-present branch, so `msg` is non-null here):

```tsx
      <div className="reading-toolbar">
        <button className="icon-btn" disabled aria-label="Reply (coming soon)">
          <CornerUpLeft size={15} />
        </button>
        <button
          className={isStarred(msg) ? "icon-btn active" : "icon-btn"}
          aria-label={isStarred(msg) ? "Unstar" : "Star"}
          onClick={() => onToggleStar(msg)}
        >
          <Star size={15} fill={isStarred(msg) ? "currentColor" : "none"} />
        </button>
        <button
          className="icon-btn"
          aria-label="Mark as unread"
          onClick={() => onMarkUnread(msg)}
        >
          <MailOpen size={15} />
        </button>
        <button
          className="icon-btn"
          aria-label="Archive"
          onClick={() => onArchive(msg)}
        >
          <Archive size={15} />
        </button>
        <button
          className="icon-btn"
          aria-label="Move to trash"
          onClick={() => onTrash(msg)}
        >
          <Trash2 size={15} />
        </button>
      </div>
```

(The empty-state early return at the top still uses `<Mail .../>`, so keep the `Mail` import.)

- [ ] **Step 2: Add the active-icon CSS**

Append to `src/styles/app.css`:

```css
/* M7 actions — active (e.g. starred) reading-pane toolbar button */
.icon-btn.active { color: var(--accent); border-color: var(--accent); }
```

- [ ] **Step 3: Type-check the whole frontend**

Run: `npm run build`
Expected: `tsc` passes (no errors) and Vite builds — the full Task 6–8 wiring now type-checks as a unit.

- [ ] **Step 4: Commit the frontend wiring (Tasks 6–8)**

```bash
git add src/App.tsx src/components/MessageItem.tsx src/components/MessageList.tsx src/components/ReadingPane.tsx src/styles/app.css
git commit -m "feat(ui): wire read/star/archive/trash actions with optimistic updates"
```

---

## Task 9: Full verification + docs/memory update

**Files:**
- Modify: `wiki/entities/ember.md` (M7 status), `wiki/log.md` (one line)
- (Memory) `~/.claude/projects/-Users-makar-dev-ownmail/memory/ember-project.md`

- [ ] **Step 1: Backend — full suite, lint, format**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all green. (If `cargo fmt --check` reports diffs, run `cargo fmt` and amend.)

- [ ] **Step 2: Frontend — type-check + build**

Run: `npm run build`
Expected: clean.

- [ ] **Step 3: Manual E2E against live Gmail**

Run: `npm run tauri dev`

Verify each, confirming the change in Gmail web too:
- Open an unread message → it becomes read (bold styling clears); Gmail shows it read.
- Toolbar **Mark as unread** → row goes bold again; Gmail shows unread.
- **Star** from the row hover and from the toolbar → star lights; Gmail shows it starred. Unstar reverses it.
- **Archive** (row hover + toolbar) → row disappears, selection advances to the next message; the message leaves INBOX in Gmail (still in All Mail).
- **Trash** (toolbar) → row disappears immediately; the message is in Gmail's Trash.
- **Rollback:** turn off network (or sign out the token), click Archive → the row reappears and the error bar shows the failure.
- Run **Sync** afterward → no duplicates or resurrected rows; archived/trashed messages stay gone.

- [ ] **Step 4: Update the wiki**

In `wiki/entities/ember.md`, update the M7 line (around line 53) from the roadmap entry to a done/state entry, e.g.:

```markdown
- **M7 — Actions (lean core)** — read/unread, star, archive, trash via Gmail label
  modify + the trash endpoint, optimistic UI with rollback. Reuses `label_ids` (no
  migration). Deferred: arbitrary labels, pin, snooze, multi-select, undo.
```

Append one line to `wiki/log.md` recording the M7 ingest/update (follow the existing format in that file).

- [ ] **Step 5: Update project memory**

Update `~/.claude/projects/-Users-makar-dev-ownmail/memory/ember-project.md`: mark M7 done in the milestone roadmap with the merge details, and set "Next: M8 compose". Keep the deferred-backlog note about cross-client read/star reconciliation.

- [ ] **Step 6: Commit docs**

```bash
git add wiki/entities/ember.md wiki/log.md
git commit -m "docs(m7): record Actions milestone in wiki"
```

- [ ] **Step 7: Finish the branch**

Use the `superpowers:finishing-a-development-branch` skill to decide merge/PR/cleanup for `m7-actions`.

---

## Self-review (completed during planning)

- **Spec coverage:** Gmail write client → Tasks 2–3; commands → Task 4; DB helper → Task 1; api/labels → Task 5; optimistic engine + auto-read + selection advance → Task 6; row hover actions + unread/star state → Task 7; toolbar wiring → Task 8; error handling/rollback → Tasks 6–8; testing → Tasks 1–4 + Task 9 manual E2E; deferrals → recorded in this plan and the spec. All spec sections map to a task.
- **Type consistency:** `ModifiedMessage { id, label_ids }`, `modify_message(id, &[&str], &[&str])`, `trash_message(id)`, `update_message_labels(conn, id, csv)`, and the JS wrappers (`setMessageRead`/`setMessageStarred`/`archiveMessage`/`trashMessage`) are used identically wherever referenced. Frontend prop names (`onArchive`/`onStar`/`onTrash`/`onToggleStar`/`onMarkUnread`) match between `App.tsx`, `MessageList.tsx`, `MessageItem.tsx`, and `ReadingPane.tsx`.
- **Placeholder scan:** no TBD/TODO; every code step shows full code.
- **Known caveat (intentional):** the frontend builds green only at the end of Task 8 — Task 6 deliberately introduces props that Tasks 7–8 satisfy. This is called out in Tasks 6–8.
