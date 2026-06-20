# Ember M15 — Batch actions + undo (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Multi-select messages and Archive/Trash/Mark-read/Star them in one Gmail `batchModify` call, with an Undo toast after any archive/trash (single or batch).

**Architecture:** Selection is a frontend `Set<string>` over the M11/M12 active list, surfaced as row checkboxes + a batch action bar. Every batch op is one `batchModify` call. Archive/trash (single + batch) are unified onto `batch_modify` so Undo is the symmetric inverse; the M7 single `archive_message`/`trash_message` are removed. Cache reconciles like M7 (archive/trash delete cached rows; read/star apply a label delta).

**Tech Stack:** Rust (reqwest, serde, rusqlite, Tauri 2; wiremock), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT):** owner is learning Rust — every Rust edit carries a concise `// 🦀` comment on the *language* concept (match `gmail/mod.rs`/`db/mod.rs` voice). Plain-English Rust recap after each Rust task. TS/React uses normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Reviewer note (process):** reviewers are READ-ONLY — their prompts must forbid Edit/Write/git-state changes, and the controller checks `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m15-batch-undo-design.md`

**Ordering:** the new `batch_modify` path is added (T1–T4) and wired into the UI (T5–T8) BEFORE the old M7 `archive_message`/`trash_message` are removed (T9), so every task's build stays green. The dead frontend `archiveMessage`/`trashMessage` wrappers are removed in T8 (when App stops importing them); the dead backend commands in T9.

---

## Task 1: Backend — `GmailClient::batch_modify` + `post_json_no_response`

**Files:** Modify `src-tauri/src/gmail/mod.rs`; Test `src-tauri/tests/gmail_test.rs`.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/tests/gmail_test.rs`:
```rust
#[tokio::test(flavor = "multi_thread")]
async fn batch_modify_posts_ids_and_labels() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/batchModify"))
        .and(body_json(json!({
            "ids": ["a", "b"],
            "addLabelIds": ["TRASH"],
            "removeLabelIds": []
        })))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client
        .batch_modify(&["a".to_string(), "b".to_string()], &["TRASH"], &[])
        .await
        .unwrap();
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test batch_modify`
Expected: FAIL — `batch_modify` not found.

- [ ] **Step 3: Add the helper + method**

In `src-tauri/src/gmail/mod.rs`, add a helper right after `post_json` (~line 163):
```rust
    // 🦀 POST a JSON body but expect NO response body (Gmail's batchModify returns 204).
    //    Like post_no_body, but carries a JSON payload; we only check the status, never
    //    parse — post_json would error trying to deserialize an empty body.
    async fn post_json_no_response<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<()> {
        self.http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
```
Add the method at the end of `impl GmailClient` (after `modify_message`, ~line 524):
```rust
    /// Add and/or remove labels on MANY messages in one call (`messages.batchModify`,
    /// up to 1000 ids; returns 204 with no body). Used by the M15 batch actions and undo.
    pub async fn batch_modify(&self, ids: &[String], add: &[&str], remove: &[&str]) -> Result<()> {
        // 🦀 A short-lived request struct; serde field names match Gmail's JSON. The `<'a>`
        //    ties the borrowed slices to the struct so we serialize without cloning.
        #[derive(serde::Serialize)]
        struct BatchModifyRequest<'a> {
            ids: &'a [String],
            #[serde(rename = "addLabelIds")]
            add_label_ids: &'a [&'a str],
            #[serde(rename = "removeLabelIds")]
            remove_label_ids: &'a [&'a str],
        }
        let url = format!("{}/gmail/v1/users/me/messages/batchModify", self.base_url);
        let body = BatchModifyRequest { ids, add_label_ids: add, remove_label_ids: remove };
        self.post_json_no_response(&url, &body).await
    }
```

- [ ] **Step 4: Run tests + clippy**

Run: `cd src-tauri && cargo test --test gmail_test batch_modify && cargo clippy --lib --tests`
Expected: PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m15): GmailClient batch_modify + post_json_no_response"
```

**🦀 Recap:** `batchModify` returns 204 (no body), so it needs a POST helper that checks status but never parses a response — `post_json` would choke on the empty body.

---

## Task 2: Backend — `db::apply_label_delta`

**Files:** Modify `src-tauri/src/db/mod.rs` (helper + a test).

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/db/mod.rs`, inside the existing `#[cfg(test)] mod tests { … }`, add (use the existing `msg(...)`/`upsert_messages`/`recent_previews`/`conn()` test helpers — match how other db tests build rows; the helper that sets `label_ids` is used by the M6/M7 tests, e.g. `m.label_ids = "INBOX,UNREAD".into()`):
```rust
    #[test]
    fn apply_label_delta_adds_and_removes_on_cached_rows() {
        let c = conn();
        let mut m = msg("x", 1);
        m.label_ids = "INBOX,UNREAD".into();
        upsert_messages(&c, &[m]).unwrap();

        // remove UNREAD, add STARRED
        apply_label_delta(&c, &["x".to_string()], &["STARRED".to_string()], &["UNREAD".to_string()]).unwrap();
        let labels: Vec<String> = recent_previews(&c, 10).unwrap()[0]
            .label_ids
            .split(',')
            .map(String::from)
            .collect();
        assert!(labels.contains(&"INBOX".to_string()));
        assert!(labels.contains(&"STARRED".to_string()));
        assert!(!labels.contains(&"UNREAD".to_string()));

        // idempotent: applying the same delta again changes nothing
        apply_label_delta(&c, &["x".to_string()], &["STARRED".to_string()], &["UNREAD".to_string()]).unwrap();
        let again: Vec<String> = recent_previews(&c, 10).unwrap()[0].label_ids.split(',').map(String::from).collect();
        assert_eq!(again.iter().filter(|l| *l == "STARRED").count(), 1);

        // an uncached id is skipped without error
        apply_label_delta(&c, &["nope".to_string()], &[], &["INBOX".to_string()]).unwrap();
    }
```
(If the test helpers `msg`/`upsert_messages` have different names, match the existing tests in this file — read them first.)

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --lib db::tests::apply_label_delta`
Expected: FAIL — `apply_label_delta` not found.

- [ ] **Step 3: Implement**

In `src-tauri/src/db/mod.rs`: ensure the rusqlite import includes `OptionalExtension` (change the top `use rusqlite::{params, Connection};` to `use rusqlite::{params, Connection, OptionalExtension};`). Add the function near `update_message_labels` (~line 236):
```rust
/// Apply a label add/remove delta to each cached row in `ids` (in place). Used by the
/// batch mark-read/star path — Gmail's batchModify returns no labels, so we update the
/// cache from the known delta. Idempotent; ids not in the cache (search/folder results)
/// are silently skipped. One transaction.
pub fn apply_label_delta(conn: &Connection, ids: &[String], add: &[String], remove: &[String]) -> Result<()> {
    // 🦀 `unchecked_transaction` borrows &Connection (no &mut) — safe here because we're
    //    not already inside another transaction (same pattern as apply_delta).
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        // 🦀 `.optional()` (from OptionalExtension) turns "no rows" into `Ok(None)` instead
        //    of an error, so an uncached id just falls through the `else { continue }`.
        let current: Option<String> = tx
            .query_row("SELECT label_ids FROM messages WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?;
        let Some(csv) = current else { continue };
        // 🦀 Parse the comma-joined labels into an owned Vec, dropping empties.
        let mut labels: Vec<String> = csv.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
        labels.retain(|l| !remove.contains(l));
        for a in add {
            if !labels.contains(a) {
                labels.push(a.clone());
            }
        }
        tx.execute("UPDATE messages SET label_ids = ?1 WHERE id = ?2", params![labels.join(","), id])?;
    }
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib db:: && cargo clippy --lib --tests`
Expected: PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(m15): db::apply_label_delta (in-place label add/remove on cached rows)"
```

**🦀 Recap:** `OptionalExtension::optional()` is the idiom for "this query may match no row" — it maps the no-rows error to `None`, letting an uncached id be skipped cleanly.

---

## Task 3: Backend — `batch_modify_messages` command (additive)

**Files:** Modify `src-tauri/src/commands.rs`; Modify `src-tauri/src/lib.rs`.

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, add after the existing `trash_message` command (~line 330, right before `send_email`):
```rust
/// Add/remove labels on many messages in one Gmail call, then reconcile the local cache.
/// Archive (remove INBOX) / trash (add TRASH) drop the rows from the inbox cache like the
/// M7 single actions; everything else (read/star) applies the delta in place. DB-aware,
/// but a no-op on the DB for ids that aren't cached (search/folder results).
#[tauri::command]
pub async fn batch_modify_messages(
    ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
    state: tauri::State<'_, Db>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 `&[&str]` is what batch_modify wants; map the owned Strings to borrowed &str.
    let add_refs: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove_refs: Vec<&str> = remove.iter().map(String::as_str).collect();
    client.batch_modify(&ids, &add_refs, &remove_refs).await?;

    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    // 🦀 `iter().any(|l| l == "TRASH")` — does the slice contain this label? Archiving
    //    (remove INBOX) or trashing (add TRASH) means the row leaves the inbox cache.
    if add.iter().any(|l| l == "TRASH") || remove.iter().any(|l| l == "INBOX") {
        db::delete_messages(&conn, &ids)?;
    } else {
        db::apply_label_delta(&conn, &ids, &add, &remove)?;
    }
    Ok(())
}
```

- [ ] **Step 2: Register it**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]`, after `commands::trash_message,` (~line 102), add:
```rust
            commands::batch_modify_messages,
```

- [ ] **Step 3: Verify**

Run: `cd src-tauri && cargo build && cargo clippy --all-targets && cargo test`
Expected: builds; clippy clean; all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(m15): batch_modify_messages command (cache reconcile: delete vs label-delta)"
```

**🦀 Recap:** the command picks the cache reconcile by inspecting the label delta — INBOX-removal / TRASH-add means "gone from the inbox" (delete the row), otherwise update labels in place.

---

## Task 4: Frontend — `batchModifyMessages` wrapper + mock (additive)

**Files:** Modify `src/lib/api.ts`; Modify `src/lib/mock.ts` (only if a mock seam is needed — see below).

- [ ] **Step 1: Add the wrapper**

In `src/lib/api.ts`, after the `trashMessage` export (~line 70), add:
```ts
export const batchModifyMessages = (
  ids: string[],
  add: string[],
  remove: string[],
): Promise<void> =>
  isTauri() ? invoke<void>("batch_modify_messages", { ids, add, remove }) : Promise.resolve();
```
(Do NOT remove `archiveMessage`/`trashMessage` yet — their callers move in Task 8.)

- [ ] **Step 2: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. (The wrapper isn't used yet.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/api.ts
git commit -m "feat(m15): batchModifyMessages api wrapper (isTauri-gated)"
```

---

## Task 5: Frontend — `MessageItem` selection checkbox

**Files:** Modify `src/components/MessageItem.tsx`; Modify `src/styles/app.css`.

- [ ] **Step 1: Add the checkbox + props**

In `src/components/MessageItem.tsx`, extend the props and render a leading checkbox. The new props are **optional** so `MessageList` keeps compiling until Task 6 wires them (keeps the build green). Change the signature to add `checked?`/`onToggleSelect?`:
```tsx
export function MessageItem({
  msg,
  selected,
  checked = false,
  onSelect,
  onToggleSelect,
  onArchive,
  onStar,
  showRecipient = false,
}: {
  msg: MessagePreview;
  selected: boolean;
  checked?: boolean;
  onSelect: (id: string) => void;
  onToggleSelect?: (id: string) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  showRecipient?: boolean;
}) {
```
Change the `cls` to also reflect batch-selection:
```tsx
  const cls = ["msg-item", selected && "selected", checked && "checked", unread && "unread"]
    .filter(Boolean)
    .join(" ");
```
Add the checkbox as the FIRST child inside the `<div className={cls}>` (before the `msg-item-main` button):
```tsx
      <input
        type="checkbox"
        className="msg-check"
        checked={checked}
        onChange={(e) => {
          e.stopPropagation();
          onToggleSelect?.(msg.id);
        }}
        aria-label="Select message"
      />
```

- [ ] **Step 2: Add CSS**

In `src/styles/app.css`, append:
```css
.msg-check { margin: 0 2px 0 10px; flex: 0 0 auto; cursor: pointer; }
.msg-item.checked { background: var(--accent-soft, rgba(22,163,74,0.10)); }
```
(If `--accent-soft` isn't defined in the theme, the `rgba(...)` fallback applies.)

- [ ] **Step 3: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. The new props are optional, so `MessageList` (which doesn't pass them yet) still compiles; the checkbox renders but is inert (`onToggleSelect` undefined) until Task 6/8 wire it.

- [ ] **Step 4: Commit**

```bash
git add src/components/MessageItem.tsx src/styles/app.css
git commit -m "feat(m15): MessageItem selection checkbox + styles"
```

---

## Task 6: Frontend — `MessageList` batch action bar + wiring

**Files:** Modify `src/components/MessageList.tsx`; Modify `src/styles/app.css`.

- [ ] **Step 1: Extend props**

In `src/components/MessageList.tsx`, add the selection props to the destructure + type. They are **optional** (with an empty-set default for `selectedIds`) so App keeps compiling until Task 8 wires them — the bar only renders when `selectedIds.size > 0`, which can't happen until App passes a real set:
```tsx
export function MessageList({
  messages,
  stream,
  selectedId,
  selectedIds = new Set<string>(),
  onSelect,
  onToggleSelect,
  onSelectAllVisible,
  onClearSelection,
  onBatchArchive,
  onBatchTrash,
  onBatchMarkRead,
  onBatchStar,
  onArchive,
  onStar,
  flat = false,
  title,
  emptyText,
  showRecipient = false,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  selectedIds?: Set<string>;
  onSelect: (id: string) => void;
  onToggleSelect?: (id: string) => void;
  onSelectAllVisible?: (ids: string[]) => void;
  onClearSelection?: () => void;
  onBatchArchive?: () => void;
  onBatchTrash?: () => void;
  onBatchMarkRead?: () => void;
  onBatchStar?: () => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  flat?: boolean;
  title?: string;
  emptyText?: string;
  showRecipient?: boolean;
}) {
```

- [ ] **Step 2: Compute visible ids + replace the header**

After the existing `const empty = …;` line, add:
```tsx
  const visibleIds = (groups ? groups.flatMap((g) => g.messages) : visible).map((m) => m.id);
  const allVisibleSelected = visibleIds.length > 0 && visibleIds.every((id) => selectedIds.has(id));
```
Replace the `<div className="msglist-header"> … </div>` block with a two-state header:
```tsx
      {selectedIds.size > 0 ? (
        <div className="msglist-header batch-bar">
          <input
            type="checkbox"
            className="batch-check"
            checked={allVisibleSelected}
            onChange={() => onSelectAllVisible?.(visibleIds)}
            aria-label="Select all visible"
          />
          <span className="batch-count">{selectedIds.size} selected</span>
          <div className="batch-actions">
            <button className="batch-btn" onClick={() => onBatchArchive?.()}>Archive</button>
            <button className="batch-btn" onClick={() => onBatchTrash?.()}>Trash</button>
            <button className="batch-btn" onClick={() => onBatchMarkRead?.()}>Mark read</button>
            <button className="batch-btn" onClick={() => onBatchStar?.()}>Star</button>
          </div>
          <button className="batch-clear" aria-label="Clear selection" onClick={() => onClearSelection?.()}>
            ✕
          </button>
        </div>
      ) : (
        <div className="msglist-header">
          <span className="msglist-title">{headerTitle}</span>
          <span className="msglist-count">{count} messages</span>
        </div>
      )}
```

- [ ] **Step 3: Pass `checked`/`onToggleSelect` to each MessageItem**

In BOTH `<MessageItem … />` call sites (the grouped map and the flat map), add:
```tsx
                  checked={selectedIds.has(m.id)}
                  onToggleSelect={onToggleSelect}
```
(right after `onSelect={onSelect}`).

- [ ] **Step 4: Add CSS**

In `src/styles/app.css`, append:
```css
.batch-bar { display: flex; align-items: center; gap: 12px; }
.batch-check { cursor: pointer; }
.batch-count { font-weight: 600; font-size: 13px; }
.batch-actions { display: flex; gap: 6px; margin-left: auto; }
.batch-btn { font-size: 12px; padding: 3px 8px; border: 1px solid var(--border, #ddd); border-radius: 6px; background: transparent; cursor: pointer; }
.batch-btn:hover { background: var(--accent-soft, rgba(22,163,74,0.10)); }
.batch-clear { background: transparent; border: none; cursor: pointer; font-size: 13px; padding: 2px 6px; }
```

- [ ] **Step 5: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. The new props are optional with an empty-set default, so App (not passing them yet) still compiles and the batch bar stays hidden (`selectedIds.size === 0`) until Task 8 wires real selection.

- [ ] **Step 6: Commit**

```bash
git add src/components/MessageList.tsx src/styles/app.css
git commit -m "feat(m15): MessageList batch action bar + select-all + checkbox wiring"
```

---

## Task 7: Frontend — `UndoToast` component

**Files:** Create `src/components/UndoToast.tsx`; Modify `src/styles/app.css`.

- [ ] **Step 1: Create the component**

Create `src/components/UndoToast.tsx`:
```tsx
// A transient bottom-center toast offering to undo the last archive/trash. Auto-dismiss
// is managed by the parent (App) timer; this is a pure presentational component.
export function UndoToast({
  verb,
  count,
  onUndo,
  onDismiss,
}: {
  verb: string;
  count: number;
  onUndo: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="undo-toast" role="status">
      <span className="undo-text">
        {verb} {count} {count === 1 ? "message" : "messages"}
      </span>
      <button className="undo-btn" onClick={onUndo}>Undo</button>
      <button className="undo-close" aria-label="Dismiss" onClick={onDismiss}>✕</button>
    </div>
  );
}
```

- [ ] **Step 2: Add CSS**

In `src/styles/app.css`, append:
```css
.undo-toast {
  position: fixed; left: 50%; bottom: 28px; transform: translateX(-50%);
  display: flex; align-items: center; gap: 14px;
  background: var(--toast-bg, #2b2b2b); color: #fff;
  padding: 10px 16px; border-radius: 8px; box-shadow: 0 4px 16px rgba(0,0,0,0.25);
  z-index: 1000; font-size: 13px;
}
.undo-text { white-space: nowrap; }
.undo-btn { background: transparent; color: var(--accent, #4ade80); border: none; font-weight: 700; cursor: pointer; font-size: 13px; }
.undo-close { background: transparent; color: #bbb; border: none; cursor: pointer; font-size: 12px; }
```

- [ ] **Step 3: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. `UndoToast.tsx` compiles (it's unused until Task 8 renders it).

- [ ] **Step 4: Commit**

```bash
git add src/components/UndoToast.tsx src/styles/app.css
git commit -m "feat(m15): UndoToast component + styles"
```

---

## Task 8: Frontend — App.tsx selection state, batch actions, undo, wiring

**Files:** Modify `src/App.tsx`; Modify `src/lib/api.ts` (remove dead wrappers).

- [ ] **Step 1: Imports**

In `src/App.tsx`: in the `./lib/api` import block, **remove** `archiveMessage` and `trashMessage`, and **add** `batchModifyMessages`. Add the toast import near the other component imports:
```tsx
import { UndoToast } from "./components/UndoToast";
```
Confirm `useMemo`, `useRef`, `useState` are imported (they are) and `withLabel, UNREAD, STARRED` are imported from `./lib/labels` (they are).

- [ ] **Step 2: Selection + undo state**

Add near the other state (after the M13 refs / folder state, e.g. ~line 80):
```tsx
  // M15 batch selection (over the active list) + a single-level undo for archive/trash.
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [undo, setUndo] = useState<{ verb: string; count: number; onUndo: () => void } | null>(null);
  const undoTimer = useRef<number | null>(null);
```

- [ ] **Step 3: Selection + undo helpers**

Add after the `selected` useMemo (~line 165, after `activeList`/`activeSelectedId` are defined):
```tsx
  const selectedMsgs = useMemo(
    () => activeList.filter((m) => selectedIds.has(m.id)),
    [activeList, selectedIds],
  );
  function toggleSelect(id: string) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }
  function clearSelection() {
    setSelectedIds(new Set());
  }
  function selectAllVisible(ids: string[]) {
    setSelectedIds((prev) => {
      const allSelected = ids.length > 0 && ids.every((id) => prev.has(id));
      return allSelected ? new Set() : new Set(ids);
    });
  }
  function clearUndo() {
    if (undoTimer.current) clearTimeout(undoTimer.current);
    undoTimer.current = null;
    setUndo(null);
  }
  function registerUndo(
    verb: string,
    rows: MessagePreview[],
    ids: string[],
    inverse: { add: string[]; remove: string[] },
  ) {
    if (undoTimer.current) clearTimeout(undoTimer.current);
    const onUndo = () => {
      clearUndo();
      // Restore the removed rows into the (originating) active list, deduped + recency-sorted.
      setActiveList((cur) => {
        const have = new Set(cur.map((m) => m.id));
        const merged = [...cur, ...rows.filter((r) => !have.has(r.id))];
        merged.sort((a, b) => b.internal_date - a.internal_date);
        return merged;
      });
      batchModifyMessages(ids, inverse.add, inverse.remove).catch((e) => setError(String(e)));
    };
    setUndo({ verb, count: ids.length, onUndo });
    undoTimer.current = window.setTimeout(() => setUndo(null), 6000);
  }
```

- [ ] **Step 4: The unified remove helper + batch handlers; reroute single archive/trash**

Replace the existing `const handleArchive = …` / `const handleTrash = …` lines (~308–311) with:
```tsx
  // Optimistically remove `msgs` from the active list, batch-modify on the server, and
  // register an Undo (inverse labels). Powers single (reading-pane) AND batch archive/trash.
  function removeMessages(
    msgs: MessagePreview[],
    op: { add: string[]; remove: string[]; verb: string },
  ) {
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const listSnap = activeList;
    const selSnap = activeSelectedId;
    setActiveList(listSnap.filter((m) => !idSet.has(m.id)));
    if (activeSelectedId && idSet.has(activeSelectedId)) {
      // single removal advances to the next message (M7 UX); a batch closes the pane.
      setActiveSelectedId(ids.length === 1 ? nextSelectedId(activeSelectedId) : null);
    }
    clearSelection();
    setError(null);
    batchModifyMessages(ids, op.add, op.remove)
      .then(() => registerUndo(op.verb, msgs, ids, { add: op.remove, remove: op.add }))
      .catch((e) => {
        setActiveList(listSnap);
        setActiveSelectedId(selSnap);
        setError(String(e));
      });
  }

  const handleArchive = (m: MessagePreview) =>
    removeMessages([m], { add: [], remove: ["INBOX"], verb: "Archived" });
  const handleTrash = (m: MessagePreview) =>
    removeMessages([m], { add: ["TRASH"], remove: [], verb: "Trashed" });

  const batchArchive = () =>
    removeMessages(selectedMsgs, { add: [], remove: ["INBOX"], verb: "Archived" });
  const batchTrash = () =>
    removeMessages(selectedMsgs, { add: ["TRASH"], remove: [], verb: "Trashed" });

  // Read/star: in-place label toggle, no undo toast (reversible via the row controls).
  function batchMarkRead() {
    const msgs = selectedMsgs;
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const snap = activeList;
    setActiveList(snap.map((m) => (idSet.has(m.id) ? withLabel(m, UNREAD, false) : m)));
    clearSelection();
    setError(null);
    batchModifyMessages(ids, [], ["UNREAD"]).catch((e) => {
      setActiveList(snap);
      setError(String(e));
    });
  }
  function batchStar() {
    const msgs = selectedMsgs;
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const snap = activeList;
    setActiveList(snap.map((m) => (idSet.has(m.id) ? withLabel(m, STARRED, true) : m)));
    clearSelection();
    setError(null);
    batchModifyMessages(ids, ["STARRED"], []).catch((e) => {
      setActiveList(snap);
      setError(String(e));
    });
  }
```

- [ ] **Step 5: Clear selection + undo on every list switch**

Add `clearSelection(); clearUndo();` as the FIRST lines inside `handleSelectFolder` (~439), `handleSearch` (~413), and `handleClearSearch` (~431). And in the Header `onSelectStream` callback (~487), which currently is `(s) => { setStream(s); setSelectedId(null); }`, add the two clears:
```tsx
        onSelectStream={(s) => {
          setStream(s);
          setSelectedId(null);
          clearSelection();
          clearUndo();
        }}
```

- [ ] **Step 6: Wire MessageList + render UndoToast**

In the `<MessageList … />` render, add these props (alongside the existing ones):
```tsx
                selectedIds={selectedIds}
                onToggleSelect={toggleSelect}
                onSelectAllVisible={selectAllVisible}
                onClearSelection={clearSelection}
                onBatchArchive={batchArchive}
                onBatchTrash={batchTrash}
                onBatchMarkRead={batchMarkRead}
                onBatchStar={batchStar}
```
At the end of the main return (e.g. right after the `{compose && (…)}` block, before the closing `</div>` of `.app`), add:
```tsx
      {undo && (
        <UndoToast
          verb={undo.verb}
          count={undo.count}
          onUndo={undo.onUndo}
          onDismiss={clearUndo}
        />
      )}
```

- [ ] **Step 7: Remove the now-dead api wrappers**

In `src/lib/api.ts`, delete the `archiveMessage` and `trashMessage` exports (they're no longer imported anywhere — `removeMessages` uses `batchModifyMessages`). Leave `setMessageRead`/`setMessageStarred` (still used by single read/star) and `restoreMessage`/`deleteMessageForever` (M12).

- [ ] **Step 8: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS — App now supplies all `<MessageList>` props, imports `batchModifyMessages`, and no longer imports the removed wrappers. If tsc reports `archiveMessage`/`trashMessage` still referenced, find and reroute that caller (there should be none beyond the two handlers just rewritten).

- [ ] **Step 9: Commit**

```bash
git add src/App.tsx src/lib/api.ts
git commit -m "feat(m15): selection state, batch actions, undo toast; unify single archive/trash"
```

---

## Task 9: Backend — remove the M7 single archive/trash commands

**Files:** Modify `src-tauri/src/commands.rs`; Modify `src-tauri/src/lib.rs`; Modify `src-tauri/src/gmail/mod.rs`.

- [ ] **Step 1: Remove the commands**

In `src-tauri/src/commands.rs`, delete the entire `archive_message` and `trash_message` command functions (the two `#[tauri::command] pub async fn …` blocks, ~lines 302–330) — they're replaced by `batch_modify_messages`.

- [ ] **Step 2: Remove the registrations**

In `src-tauri/src/lib.rs`, delete the `commands::archive_message,` and `commands::trash_message,` lines from `generate_handler!`.

- [ ] **Step 3: Remove the now-unused client method**

In `src-tauri/src/gmail/mod.rs`, delete the `pub async fn trash_message(&self, id: &str) -> Result<()> { … }` method (~line 500). Keep `untrash_message` (used by M12 folder restore) and `modify_message` (used by single read/star). If a wiremock test in `tests/gmail_test.rs` exercises `trash_message` specifically, delete that test too.

- [ ] **Step 4: Verify**

Run: `cd src-tauri && cargo build && cargo clippy --all-targets && cargo test`
Expected: builds with NO "unused function" warnings; clippy clean; all tests pass. If clippy flags any other now-unused helper, report it (do not delete beyond the three items above without noting it).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/gmail/mod.rs
git commit -m "refactor(m15): remove M7 single archive_message/trash_message (subsumed by batch_modify)"
```

**🦀 Recap:** unifying onto `batch_modify` means the single archive/trash commands + the `trash_message` client method became dead code — the compiler/clippy confirm nothing else referenced them.

---

## Task 10: Verification, roadmap & wiki

**Files:** Modify `wiki/entities/ember.md`, `wiki/log.md` (local-only, gitignored — edits live on disk, not committed).

- [ ] **Step 1: Full verification**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test && cargo clippy --all-targets`
Expected: all pass (prior + the 2 new tests: `batch_modify_posts_ids_and_labels`, `apply_label_delta_adds_and_removes_on_cached_rows`); clippy clean. Report the total count.
Run: `cd /Users/makar/dev/ownmail && npm run build` → PASS.

- [ ] **Step 2: Maket check (screenshot)**

Run `npm run dev`; in the browser, Mail view → check a couple of inbox rows → confirm the batch action bar appears ("N selected · Archive · Trash · Mark read · Star · ✕") → click **Archive** → rows vanish and the **Undo toast** appears → click **Undo** → rows return. Screenshot the action bar and the undo toast.

- [ ] **Step 3: Update the wiki roadmap**

In `wiki/entities/ember.md`: bump `updated:` to `2026-06-20`; mark M14 as merged if not already; add an M15 bullet after M14; update the closing "As of M14…" paragraph to "As of M15…" mentioning batch actions + undo. M15 bullet:
```
- **M15 — Batch actions + undo (lean v1)** — *implemented on branch `m15-batch-undo`, pending merge.*
  Second of the M14→M17 arc. Multi-select via per-row checkboxes over the M11/M12 active list +
  a batch action bar (replaces the list header when ≥1 selected): **Archive · Trash · Mark read ·
  Star**, each one Gmail `batchModify` call (`GmailClient::batch_modify` + a new `post_json_no_response`
  helper; 204 no body). **Unified archive/trash (single + batch) onto `batch_modify`** — removed the M7
  single `archive_message`/`trash_message` commands + the `trash_message` client method — so reading-pane
  single actions also gain undo, via one `removeMessages(msgs, {add, remove, verb})` helper. An **Undo
  toast** (`UndoToast`, ~6s, single-level) after any archive/trash restores the rows + issues the inverse
  `batchModify`. Cache reconcile: archive/trash → `db::delete_messages`; batch read/star → new
  `db::apply_label_delta` (in-place label add/remove on cached rows, idempotent, skips uncached). Selection
  is a frontend `Set<string>`, cleared on every list switch. **No DB migration, no new OAuth scope.** N tests
  (2 new: batch_modify wiremock, apply_label_delta unit), clippy clean. Maket verified by screenshot.
  **Live Gmail E2E pending owner** (real batch ops + undo, esp. the trash-via-`add TRASH` equivalence).
  **Deferred:** mark-unread/unstar in the bar, multi-level undo, undo for read/star, select-all-across-
  unloaded, keyboard shortcuts.
```
(Replace `N` with the count from Step 1.) Append a one-line `wiki/log.md` entry in the file's format.

- [ ] **Step 4: (No git commit — `wiki/` is gitignored.)**

---

## Self-review (completed by plan author)

**Spec coverage:** multi-select checkboxes (T5) ✓; batch action bar Archive/Trash/Mark-read/Star (T6) ✓; one `batchModify` per op (T1 client, T3 command) ✓; unify archive/trash + remove M7 singles (T8 reroute, T9 removal) ✓; Undo toast for archive/trash single+batch, ~6s, inverse batchModify + row restore (T7 component, T8 `registerUndo`/`removeMessages`) ✓; cache reconcile delete vs apply_label_delta (T2 helper, T3 command) ✓; selection cleared on list switch (T8 Step 5) ✓; no migration / no new scope / isTauri maket (throughout, T4) ✓; verification + wiki (T10) ✓; Rust learning comments (T1–T3, T9) ✓; wiremock + db unit tests (T1, T2) ✓.

**Placeholder scan:** no TBD/TODO; every code step shows full code; `N` in the wiki bullet is explicitly "replace with the Step-1 count".

**Type/name consistency:** `batch_modify(ids, add: &[&str], remove: &[&str])` (T1) ↔ command `batch_modify_messages(ids, add: Vec<String>, remove)` (T3) ↔ TS `batchModifyMessages(ids, add, remove)` (T4); `apply_label_delta(conn, ids, add, remove)` (T2/T3); `removeMessages(msgs, {add, remove, verb})`, `registerUndo(verb, rows, ids, inverse)`, `selectedIds`/`toggleSelect`/`selectAllVisible`/`clearSelection`/`clearUndo` consistent across T6/T7/T8; `MessageItem` props `checked`/`onToggleSelect` (T5) match `MessageList`'s pass-through (T6) match App's `selectedIds`/`toggleSelect` (T8); `UndoToast` props `verb`/`count`/`onUndo`/`onDismiss` (T7) match App's render (T8). **Every task's build is green:** the bridging `MessageItem`/`MessageList` selection props are optional (with defaults), so consumers compile before their wiring lands (T5/T6/T7 all pass `npm run build`); they're always supplied once App wires them in T8. The M7-command removal (T9) happens only after the frontend stops referencing them (T8).
