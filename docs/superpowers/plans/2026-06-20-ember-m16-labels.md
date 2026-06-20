# Ember M16 — Arbitrary labels (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** List the user's Gmail labels, apply/remove them on messages (single + batch), browse a label from the rail, see label chips, and create a label inline.

**Architecture:** Apply/remove reuses the M15 `batch_modify_messages` command (a 1-element id list for single). New backend is only list/create labels + `fetch_label` (a thin mirror of `fetch_folder` over `list_message_ids(Some(labelId))`). Browsing a label is modeled as a "folder": the active-mailbox `folder` value becomes a `string` (system key OR label id), and the folder fetch effect branches (known system key → `fetchFolder`, else → `fetchLabel`), reusing M11/M12's active list. A `LabelPicker` overlay (checkbox list + create) opens from the reading pane and the M15 batch bar; user labels render as chips.

**Tech Stack:** Rust (reqwest, serde, Tauri 2; wiremock), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT):** owner is learning Rust — every Rust edit carries a concise `// 🦀` comment on the *language* concept (match `gmail/mod.rs` voice). Plain-English Rust recap after each Rust task. TS/React uses normal comments. **Do NOT run `cargo fmt`.**

**Reviewer note (process):** reviewers are READ-ONLY — prompts MUST forbid Edit/Write/git-state ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m16-labels-design.md`

**Type note:** `Folder` (the system-folder union in `lib/folders.ts`) and `Record<Folder, LucideIcon>` (FolderRail's `ICON`) STAY as-is. Only the *active-mailbox value* (`folder` state + the props that carry it: App, FolderRail's `folder`/`onSelectFolder`, ReadingPane's `folder`, `handleSelectFolder`) relax to `string` — a system key OR a Gmail label id. System rail rows still iterate the typed `FOLDERS`.

**Ordering:** leaf components/props (T5–T9) take OPTIONAL new props (with safe defaults) so each builds green before App wires them in T10.

---

## Task 1: Backend — `GmailClient::list_labels` + types

**Files:** Modify `src-tauri/src/gmail/types.rs`, `src-tauri/src/gmail/mod.rs`; Test `src-tauri/tests/gmail_test.rs`.

- [ ] **Step 1: Write the failing test**

Append to `src-tauri/tests/gmail_test.rs`:
```rust
#[tokio::test(flavor = "multi_thread")]
async fn list_labels_returns_user_labels_only_with_color() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "labels": [
                { "id": "INBOX", "name": "INBOX", "type": "system" },
                { "id": "Label_1", "name": "Work", "type": "user",
                  "color": { "textColor": "#ffffff", "backgroundColor": "#16a34a" } },
                { "id": "Label_2", "name": "Personal", "type": "user" }
            ]
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let labels = client.list_labels().await.unwrap();
    assert_eq!(labels.len(), 2); // system INBOX excluded
    assert_eq!(labels[0].id, "Label_1");
    assert_eq!(labels[0].name, "Work");
    assert_eq!(labels[0].color.as_ref().unwrap().background, "#16a34a");
    assert_eq!(labels[1].id, "Label_2");
    assert!(labels[1].color.is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test list_labels`
Expected: FAIL — `list_labels` not found.

- [ ] **Step 3: Add the public types**

In `src-tauri/src/gmail/types.rs`, after the `ModifiedMessage` struct, add:
```rust
/// A user-created Gmail label (system labels are filtered out by `list_labels`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Label {
    pub id: String,
    pub name: String,
    // 🦀 `Option` — Gmail omits `color` for labels with no custom color (then `None`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<LabelColor>,
}

/// A label's Gmail color (hex). Both fields present when a label is colored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabelColor {
    #[serde(rename = "textColor", default)]
    pub text: String,
    #[serde(rename = "backgroundColor", default)]
    pub background: String,
}
```

- [ ] **Step 4: Add the method + wire structs**

In `src-tauri/src/gmail/mod.rs`, add to the end of `impl GmailClient` (after the M15 `batch_modify`):
```rust
    /// List the user's *user-created* labels (system labels like INBOX/UNREAD are dropped —
    /// they're handled by the rail's fixed folders + the scorer). Gmail `users.labels.list`.
    pub async fn list_labels(&self) -> Result<Vec<Label>> {
        let url = format!("{}/gmail/v1/users/me/labels", self.base_url);
        let resp: LabelsListResponse = self.get_json(&url).await?;
        // 🦀 `filter` keeps only user labels; `map` drops the wire-only `label_type` field.
        Ok(resp
            .labels
            .into_iter()
            .filter(|l| l.label_type == "user")
            .map(|l| Label { id: l.id, name: l.name, color: l.color })
            .collect())
    }
```
Add the private wire structs near the other response structs in `mod.rs` (module level):
```rust
// 🦀 users.labels.list shape. `RawLabel` carries `type` (a Rust keyword → `serde(rename)`
//    onto `label_type`) so we can filter to user labels.
#[derive(serde::Deserialize)]
struct LabelsListResponse {
    #[serde(default)]
    labels: Vec<RawLabel>,
}
#[derive(serde::Deserialize)]
struct RawLabel {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    label_type: String,
    #[serde(default)]
    color: Option<LabelColor>,
}
```
Ensure `Label`/`LabelColor` are in scope in `mod.rs` the same way `MessagePreview` is (the existing `use types::{…}` — add `Label`, `LabelColor`).

- [ ] **Step 5: Run tests + clippy**

Run: `cd src-tauri && cargo test --test gmail_test list_labels && cargo clippy --lib --tests`
Expected: PASS; clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m16): GmailClient list_labels (user labels only) + Label types"
```

**🦀 Recap:** serde `rename` lets a struct field named `label_type` deserialize Gmail's `type` key (a Rust keyword can't be a field name); `filter` then keeps only `type == "user"`.

---

## Task 2: Backend — `GmailClient::create_label`

**Files:** Modify `src-tauri/src/gmail/mod.rs`; Test `src-tauri/tests/gmail_test.rs`.

- [ ] **Step 1: Write the failing test**

Append to `tests/gmail_test.rs`:
```rust
#[tokio::test(flavor = "multi_thread")]
async fn create_label_posts_name_and_parses_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/labels"))
        .and(body_json(json!({
            "name": "Receipts",
            "labelListVisibility": "labelShow",
            "messageListVisibility": "show"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "Label_9", "name": "Receipts", "type": "user"
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let label = client.create_label("Receipts").await.unwrap();
    assert_eq!(label.id, "Label_9");
    assert_eq!(label.name, "Receipts");
    assert!(label.color.is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test create_label`
Expected: FAIL — `create_label` not found.

- [ ] **Step 3: Implement**

In `gmail/mod.rs` `impl GmailClient` (next to `list_labels`):
```rust
    /// Create a new user label. Gmail `users.labels.create`. Returns the created label.
    pub async fn create_label(&self, name: &str) -> Result<Label> {
        // 🦀 Short-lived request struct; the visibility fields make the label show in
        //    both the label list and the message-list label menu (Gmail's defaults).
        #[derive(serde::Serialize)]
        struct CreateLabelRequest<'a> {
            name: &'a str,
            #[serde(rename = "labelListVisibility")]
            label_list_visibility: &'a str,
            #[serde(rename = "messageListVisibility")]
            message_list_visibility: &'a str,
        }
        let url = format!("{}/gmail/v1/users/me/labels", self.base_url);
        let body = CreateLabelRequest {
            name,
            label_list_visibility: "labelShow",
            message_list_visibility: "show",
        };
        // 🦀 Reuse RawLabel for the response, then map to the public Label (drop label_type).
        let raw: RawLabel = self.post_json(&url, &body).await?;
        Ok(Label { id: raw.id, name: raw.name, color: raw.color })
    }
```

- [ ] **Step 4: Run tests + clippy**

Run: `cd src-tauri && cargo test --test gmail_test create_label && cargo clippy --lib --tests`
Expected: PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m16): GmailClient create_label"
```

---

## Task 3: Backend — commands (`list_labels`, `create_label`, `fetch_label`) + register

**Files:** Modify `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`.

- [ ] **Step 1: Add the commands**

In `src-tauri/src/commands.rs`, add after `fetch_folder` (and its draft commands), near the other DB-free fetch commands:
```rust
/// List the user's user-created labels (DB-free). Drives the rail labels section + picker + chips.
#[tauri::command]
pub async fn list_labels() -> Result<Vec<crate::gmail::types::Label>> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.list_labels().await
}

/// Create a new user label (DB-free). Returns the created label.
#[tauri::command]
pub async fn create_label(name: String) -> Result<crate::gmail::types::Label> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.create_label(&name).await
}

/// Fetch one label's messages (DB-free) — a user label is just a label id, so this mirrors
/// fetch_folder's generic arm over list_message_ids.
#[tauri::command]
pub async fn fetch_label(label_id: String, max: u32) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    // 🦀 `Some(label_id.as_str())` → Option<&str> to match list_message_ids' `label` param.
    let ids = client.list_message_ids(Some(label_id.as_str()), "", max, false).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
    Ok(previews)
}
```

- [ ] **Step 2: Register**

In `src-tauri/src/lib.rs`, inside `generate_handler![ … ]` (after `commands::fetch_folder,`), add:
```rust
            commands::list_labels,
            commands::create_label,
            commands::fetch_label,
```

- [ ] **Step 3: Verify**

Run: `cd src-tauri && cargo build && cargo clippy --all-targets && cargo test`
Expected: builds; clippy clean; all tests pass. Report count.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(m16): list_labels/create_label/fetch_label commands"
```

**🦀 Recap:** `fetch_label` composes already-tested client methods (`list_message_ids` + `get_message_previews`), so it needs no new wiremock test — a user label is just another label id fed to the same paging+hydrate path as folders.

---

## Task 4: Frontend — api wrappers + types + mock + `withLabel` relax + chip helper

**Files:** Modify `src/lib/api.ts`, `src/lib/mock.ts`, `src/lib/labels.ts`.

- [ ] **Step 1: api.ts — Label type + wrappers**

In `src/lib/api.ts`: update the mock import to add the new mock helpers:
```ts
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek, mockSearch, mockFolder, mockGetDraft, mockSaveDraft, MOCK_LABELS, mockFetchLabel } from "./mock";
```
Add after the `DraftContent` exports (~line 130):
```ts
export interface LabelColor {
  text: string;
  background: string;
}
export interface Label {
  id: string;
  name: string;
  color?: LabelColor;
}

export const listLabels = (): Promise<Label[]> =>
  isTauri() ? invoke<Label[]>("list_labels") : Promise.resolve(MOCK_LABELS);
export const createLabel = (name: string): Promise<Label> =>
  isTauri()
    ? invoke<Label>("create_label", { name })
    : Promise.resolve({ id: "Label_mock", name });
export const fetchLabel = (id: string, max = 50): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("fetch_label", { labelId: id, max }) : Promise.resolve(mockFetchLabel(id));
```

- [ ] **Step 2: labels.ts — relax `withLabel` + chip helper**

In `src/lib/labels.ts`: change `withLabel`'s param type from `label: LabelId` to `label: string` (line in the function signature) — the logic is unchanged and system callers still pass `UNREAD`/`STARRED`. Add the import + helper:
```ts
import type { MessagePreview, Label } from "./api";
```
(merge with the existing `import type { MessagePreview } from "./api";`). At the end of the file add:
```ts
/** A message's user labels (its label_ids that match the user-label map), for chips. */
export function userLabelChips(m: MessagePreview, labelsById: Map<string, Label>): Label[] {
  return m.label_ids
    .map((id) => labelsById.get(id))
    .filter((l): l is Label => l !== undefined);
}
```

- [ ] **Step 3: mock.ts — mock labels + a labeled message + mockFetchLabel**

In `src/lib/mock.ts`: add `Label` to the type import from `./api`. Add a user label to a mock message so chips show — change the `m1` entry's `label_ids` to include `"Label_1"`:
```ts
    internal_date: 1750000000000, category: "people", label_ids: ["INBOX", "UNREAD", "Label_1"],
```
Add near the other mock exports:
```ts
export const MOCK_LABELS: Label[] = [
  { id: "Label_1", name: "Work", color: { text: "#ffffff", background: "#16a34a" } },
  { id: "Label_2", name: "Personal" },
];

/** Browser-maket: messages "in" a label = the mock messages carrying that label id. */
export function mockFetchLabel(labelId: string): MessagePreview[] {
  return MOCK_MESSAGES.filter((m) => m.label_ids.includes(labelId));
}
```

- [ ] **Step 4: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. (Wrappers/helpers unused so far.)

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/lib/mock.ts src/lib/labels.ts
git commit -m "feat(m16): label api wrappers + types + mock + withLabel(string) + userLabelChips"
```

---

## Task 5: Frontend — `LabelChips` component + `MessageItem` chips

**Files:** Create `src/components/LabelChips.tsx`; Modify `src/components/MessageItem.tsx`, `src/styles/app.css`.

- [ ] **Step 1: Create LabelChips**

Create `src/components/LabelChips.tsx`:
```tsx
import type { Label } from "../lib/api";

// Render a message's user labels as small chips. Uses the label's Gmail color when set,
// else a uniform accent chip. Pure/presentational.
export function LabelChips({ labels }: { labels: Label[] }) {
  if (labels.length === 0) return null;
  return (
    <span className="label-chips">
      {labels.map((l) => (
        <span
          key={l.id}
          className="label-chip"
          style={l.color ? { background: l.color.background, color: l.color.text } : undefined}
        >
          {l.name}
        </span>
      ))}
    </span>
  );
}
```

- [ ] **Step 2: MessageItem — render chips (optional prop)**

In `src/components/MessageItem.tsx`: add the import:
```tsx
import { LabelChips } from "./LabelChips";
import { userLabelChips } from "../lib/labels";
import type { Label } from "../lib/api";
```
Add an OPTIONAL prop `labelsById` (so `MessageList` compiles until Task 8 passes it). In the destructure add `labelsById,` and in the prop type add:
```tsx
  labelsById?: Map<string, Label>;
```
Then render chips after the subject line (after the `<span className="msg-subject">…</span>` line, inside `msg-item-main`):
```tsx
        {labelsById && <LabelChips labels={userLabelChips(msg, labelsById)} />}
```

- [ ] **Step 3: CSS**

Append to `src/styles/app.css`:
```css
.label-chips { display: inline-flex; flex-wrap: wrap; gap: 4px; margin-left: 6px; }
.label-chip { font-size: 10px; line-height: 1.4; padding: 0 6px; border-radius: 8px; background: var(--accent-weak); color: var(--accent-text); white-space: nowrap; }
```

- [ ] **Step 4: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. `labelsById` is optional, so `MessageList` still compiles; no chips render until App passes the map (Task 10).

- [ ] **Step 5: Commit**

```bash
git add src/components/LabelChips.tsx src/components/MessageItem.tsx src/styles/app.css
git commit -m "feat(m16): LabelChips + MessageItem label chips (optional labelsById)"
```

---

## Task 6: Frontend — `LabelPicker` component

**Files:** Create `src/components/LabelPicker.tsx`; Modify `src/styles/app.css`.

- [ ] **Step 1: Create LabelPicker**

Create `src/components/LabelPicker.tsx`:
```tsx
import { useEffect, useState } from "react";
import type { Label, MessagePreview } from "../lib/api";

// A small overlay popover for applying/removing user labels on `targets` (one message from
// the reading pane, or the multi-selection from the batch bar) + creating a new label.
// A label is "checked" only when EVERY target already has it (exact for one target).
export function LabelPicker({
  labels,
  targets,
  onApply,
  onCreate,
  onClose,
}: {
  labels: Label[];
  targets: MessagePreview[];
  onApply: (labelId: string, add: boolean) => void;
  onCreate: (name: string) => void;
  onClose: () => void;
}) {
  const [newName, setNewName] = useState("");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const appliedToAll = (id: string) =>
    targets.length > 0 && targets.every((m) => m.label_ids.includes(id));

  function handleCreate() {
    const name = newName.trim();
    if (!name) return;
    onCreate(name);
    setNewName("");
  }

  return (
    <div className="picker-overlay" onClick={onClose}>
      <div className="picker-card" role="dialog" aria-label="Labels" onClick={(e) => e.stopPropagation()}>
        <div className="picker-title">Label as</div>
        <div className="picker-list">
          {labels.length === 0 && <div className="picker-empty">No labels yet.</div>}
          {labels.map((l) => {
            const checked = appliedToAll(l.id);
            return (
              <label key={l.id} className="picker-row">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => onApply(l.id, !checked)}
                />
                <span
                  className="picker-dot"
                  style={l.color ? { background: l.color.background } : undefined}
                />
                <span className="picker-name">{l.name}</span>
              </label>
            );
          })}
        </div>
        <div className="picker-create">
          <input
            className="picker-input"
            placeholder="Create new label…"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleCreate();
            }}
          />
          <button className="btn" onClick={handleCreate} disabled={!newName.trim()}>
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: CSS**

Append to `src/styles/app.css`:
```css
.picker-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.25); display: flex; align-items: center; justify-content: center; z-index: 1100; }
.picker-card { background: var(--bg); border: 1px solid var(--border); border-radius: 10px; padding: 14px; width: 280px; max-height: 70vh; display: flex; flex-direction: column; gap: 10px; box-shadow: 0 8px 28px rgba(0,0,0,0.25); }
.picker-title { font-weight: 600; font-size: 13px; }
.picker-list { overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
.picker-empty { font-size: 12px; color: var(--muted); padding: 4px 0; }
.picker-row { display: flex; align-items: center; gap: 8px; padding: 4px; border-radius: 6px; cursor: pointer; }
.picker-row:hover { background: var(--accent-weak); }
.picker-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--accent); flex: 0 0 auto; }
.picker-name { font-size: 13px; }
.picker-create { display: flex; gap: 6px; border-top: 1px solid var(--border); padding-top: 10px; }
.picker-input { flex: 1; min-width: 0; padding: 4px 8px; border: 1px solid var(--border); border-radius: 6px; background: transparent; font-size: 13px; }
```
(If `--bg`/`--muted` aren't defined in the theme, substitute the nearest existing tokens — check `app.css`/`theme` for the surface + muted-text vars and use those.)

- [ ] **Step 3: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. (Unused until Task 10.)

- [ ] **Step 4: Commit**

```bash
git add src/components/LabelPicker.tsx src/styles/app.css
git commit -m "feat(m16): LabelPicker overlay (checkbox list + create)"
```

---

## Task 7: Frontend — `FolderRail` labels section

**Files:** Modify `src/components/FolderRail.tsx`, `src/styles/app.css`.

- [ ] **Step 1: Add the labels section (optional prop)**

In `src/components/FolderRail.tsx`: add the import + relax `folder`/`onSelectFolder` to `string` and add an optional `labels` prop:
```tsx
import { Inbox, Send, FileEdit, Star, Archive, Trash2, ShieldAlert, type LucideIcon } from "lucide-react";
import { FOLDERS, type Folder } from "../lib/folders";
import type { Label } from "../lib/api";

const ICON: Record<Folder, LucideIcon> = {
  inbox: Inbox, sent: Send, drafts: FileEdit, starred: Star, archive: Archive, trash: Trash2, spam: ShieldAlert,
};

export function FolderRail({
  folder,
  labels = [],
  onSelectFolder,
}: {
  folder: string;
  labels?: Label[];
  onSelectFolder: (f: string) => void;
}) {
  return (
    <nav className="folder-rail" aria-label="Mailboxes">
      {FOLDERS.map((f) => {
        const Icon = ICON[f.key];
        return (
          <button
            key={f.key}
            className={f.key === folder ? "folder-item active" : "folder-item"}
            aria-current={f.key === folder ? "page" : undefined}
            onClick={() => onSelectFolder(f.key)}
          >
            <Icon size={18} />
            <span className="folder-label">{f.label}</span>
          </button>
        );
      })}
      {labels.length > 0 && (
        <>
          <div className="rail-section">Labels</div>
          {labels.map((l) => (
            <button
              key={l.id}
              className={l.id === folder ? "folder-item active" : "folder-item"}
              aria-current={l.id === folder ? "page" : undefined}
              onClick={() => onSelectFolder(l.id)}
            >
              <span className="rail-label-dot" style={l.color ? { background: l.color.background } : undefined} />
              <span className="folder-label">{l.name}</span>
            </button>
          ))}
        </>
      )}
    </nav>
  );
}
```

- [ ] **Step 2: Relax App's active-folder type (paired here to keep the build green)**

In `src/App.tsx`: change `const [folder, setFolder] = useState<Folder>("inbox");` to `const [folder, setFolder] = useState<string>("inbox");`, and change `function handleSelectFolder(f: Folder)` to `function handleSelectFolder(f: string)`. If `Folder` is now an unused import in App (only `FOLDERS` is still referenced), remove `Folder` from the `./lib/folders` import to avoid an unused-import error.

- [ ] **Step 3: CSS**

Append to `src/styles/app.css`:
```css
.rail-section { font-size: 10px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); padding: 12px 12px 4px; }
.rail-label-dot { width: 14px; height: 14px; border-radius: 50%; background: var(--accent); display: inline-block; }
```
(Use the existing muted-text token if `--muted` isn't defined — match what `.msglist-count` or similar uses.)

- [ ] **Step 4: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. FolderRail's `folder`/`onSelectFolder` and App's `folder`/`handleSelectFolder` are now both `string`; `labels` is optional so the (not-yet-passed) labels section stays empty until Task 10.

- [ ] **Step 5: Commit**

```bash
git add src/components/FolderRail.tsx src/App.tsx src/styles/app.css
git commit -m "feat(m16): FolderRail labels section + relax active folder to string"
```

---

## Task 8: Frontend — `MessageList` label chips + batch "Label" button

**Files:** Modify `src/components/MessageList.tsx`.

- [ ] **Step 1: Add optional props + pass chips + batch button**

In `src/components/MessageList.tsx`: add the import `import type { Label } from "../lib/api";`. Add two OPTIONAL props to the destructure + type:
```tsx
  labelsById,
  onBatchLabel,
```
```tsx
  labelsById?: Map<string, Label>;
  onBatchLabel?: () => void;
```
Pass `labelsById` to BOTH `<MessageItem>` call sites (add `labelsById={labelsById}` after `onToggleSelect={onToggleSelect}`). In the batch bar (`.batch-actions`), add a Label button after the Star button:
```tsx
            <button className="batch-btn" onClick={() => onBatchLabel?.()}>Label</button>
```

- [ ] **Step 2: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. Optional props → App still compiles; chips/label-button inert until Task 10.

- [ ] **Step 3: Commit**

```bash
git add src/components/MessageList.tsx
git commit -m "feat(m16): MessageList label chips passthrough + batch Label button"
```

---

## Task 9: Frontend — `ReadingPane` chips + "Labels" button

**Files:** Modify `src/components/ReadingPane.tsx`.

- [ ] **Step 1: Add optional props + chips + button**

In `src/components/ReadingPane.tsx`: add imports:
```tsx
import { LabelChips } from "./LabelChips";
import { userLabelChips } from "../lib/labels";
import type { Label } from "../lib/api";
```
Add OPTIONAL props to the destructure + type:
```tsx
  labelsById,
  onOpenLabels,
```
```tsx
  labelsById?: Map<string, Label>;
  onOpenLabels?: (m: MessagePreview) => void;
```
Relax the existing `folder?: Folder;` prop to `folder?: string;` (it's only compared with `=== "trash"` etc.). In the header action row (near the Archive button ~line 131), add a Labels button (only when the callback is provided):
```tsx
            {onOpenLabels && (
              <button className="icon-btn" aria-label="Labels" onClick={() => onOpenLabels(msg)}>
                <Tag size={16} />
              </button>
            )}
```
Import `Tag` from lucide-react (add to the existing lucide import). Render chips under the subject/header (after the subject element; find the reading-pane subject/title element and add):
```tsx
        {labelsById && <LabelChips labels={userLabelChips(msg, labelsById)} />}
```

- [ ] **Step 2: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS. Optional props → App still compiles (App passes `folder` as a string already after Task 7; the `folder?: string` relaxation matches).

- [ ] **Step 3: Commit**

```bash
git add src/components/ReadingPane.tsx
git commit -m "feat(m16): ReadingPane label chips + Labels button"
```

---

## Task 10: Frontend — App integration (labels state, browse-as-folder, picker, apply/create)

**Files:** Modify `src/App.tsx`.

- [ ] **Step 1: Imports + state**

In `src/App.tsx`: add to the `./lib/api` import block: `listLabels, createLabel, fetchLabel, type Label`. Add `import { LabelPicker } from "./components/LabelPicker";`. Add `useMemo` if not already imported (it is). Add state near the M15 selection state:
```tsx
  // M16 labels.
  const [labels, setLabels] = useState<Label[]>([]);
  const labelsById = useMemo(() => new Map(labels.map((l) => [l.id, l])), [labels]);
  const [labelPicker, setLabelPicker] = useState<MessagePreview[] | null>(null);
```

- [ ] **Step 2: Load labels on mount**

In the mount `useEffect` (the one that calls `getConnectedAccount`/`fetchInboxPreview`/`getSettings`, ~line 120s), add:
```tsx
    listLabels()
      .then(setLabels)
      .catch(() => {}); // labels are non-critical; keep [] on error
```

- [ ] **Step 3: Browse-as-folder fetch branch**

In the folder fetch `useEffect` (~line 99), replace `fetchFolder(folder, 50)` with a branch on whether `folder` is a system folder:
```tsx
    const isSystem = FOLDERS.some((f) => f.key === folder);
    (isSystem ? fetchFolder(folder, 50) : fetchLabel(folder, 50))
```
(keep the rest of the `.then/.catch/cleanup` unchanged).

- [ ] **Step 4: List header title for labels**

In the `<MessageList … title={…} />` prop, change the `inFolder` branch from `FOLDERS.find((f) => f.key === folder)?.label` to also handle label ids:
```tsx
                title={
                  inSearch
                    ? "Results"
                    : inFolder
                      ? FOLDERS.find((f) => f.key === folder)?.label ?? labelsById.get(folder)?.name ?? "Label"
                      : undefined
                }
```

- [ ] **Step 5: apply/create handlers**

Add near the M15 batch handlers:
```tsx
  // Apply or remove a user label on `targets` (1 message from the reading pane, or the
  // selection). Optimistic withLabel on the active list (+ the open message), then persist
  // via the M15 batch command; roll back on error.
  function applyLabel(targets: MessagePreview[], labelId: string, add: boolean) {
    if (targets.length === 0) return;
    const ids = targets.map((m) => m.id);
    const idSet = new Set(ids);
    const snap = activeList;
    setActiveList(snap.map((m) => (idSet.has(m.id) ? withLabel(m, labelId, add) : m)));
    setError(null);
    batchModifyMessages(ids, add ? [labelId] : [], add ? [] : [labelId]).catch((e) => {
      setActiveList(snap);
      setError(String(e));
    });
  }

  async function handleCreateLabel(name: string, targets: MessagePreview[]) {
    setError(null);
    try {
      const created = await createLabel(name);
      const next = await listLabels();
      setLabels(next);
      applyLabel(targets, created.id, true);
    } catch (e) {
      setError(String(e));
    }
  }
```

- [ ] **Step 6: Wire FolderRail / MessageList / ReadingPane / picker**

- `<FolderRail folder={folder} onSelectFolder={handleSelectFolder} />` → add `labels={labels}`.
- `<MessageList … />` → add `labelsById={labelsById}` and `onBatchLabel={() => setLabelPicker(selectedMsgs)}`.
- `<ReadingPane … />` → add `labelsById={labelsById}` and `onOpenLabels={(m) => setLabelPicker([m])}`.
- At the end of the main return (next to the `{undo && …}` / `{compose && …}` blocks), add:
```tsx
      {labelPicker && (
        <LabelPicker
          labels={labels}
          targets={labelPicker}
          onApply={(labelId, add) => applyLabel(labelPicker, labelId, add)}
          onCreate={(name) => handleCreateLabel(name, labelPicker)}
          onClose={() => setLabelPicker(null)}
        />
      )}
```

- [ ] **Step 7: Verify it builds**

Run: `cd /Users/makar/dev/ownmail && npm run build`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/App.tsx
git commit -m "feat(m16): App labels integration — state, browse-as-folder, picker, apply/create"
```

---

## Task 11: Verification, roadmap & wiki

**Files:** Modify `wiki/entities/ember.md`, `wiki/log.md` (local-only, gitignored — not committed).

- [ ] **Step 1: Full verification**

Run: `cd /Users/makar/dev/ownmail/src-tauri && cargo test && cargo clippy --all-targets`
Expected: all pass (prior + 2 new: `list_labels_returns_user_labels_only_with_color`, `create_label_posts_name_and_parses_result`); clippy clean. Report total count.
Run: `cd /Users/makar/dev/ownmail && npm run build` → PASS.

- [ ] **Step 2: Maket check (screenshot)**

Run `npm run dev`; Mail view → confirm a **"Labels" section** in the rail (Work, Personal) with a green "Work" dot; the inbox row for "Q3 roadmap" shows a **"Work" chip**; open a message → reading pane shows the chip + a **Labels (tag) button** → clicking it opens the **LabelPicker** (Work checked, Personal unchecked, a "Create new label…" input); select a row → batch bar shows a **Label** button. Click the "Work" rail label → it browses (shows the labeled message). Screenshot the rail section + a chip + the open picker.

- [ ] **Step 3: Update the wiki roadmap**

In `wiki/entities/ember.md`: bump `updated:`; mark M15 merged if not already; add an M16 bullet after M15; update the closing "As of M15…" paragraph to "As of M16…" mentioning labels. M16 bullet:
```
- **M16 — Arbitrary labels (lean v1)** — *implemented on branch `m16-labels`, pending merge.*
  Third of the M14→M17 arc. **List** user labels (`GmailClient::list_labels`, `type=="user"` only, with
  Gmail color) + **create** inline (`create_label`) + **fetch_label** (a label is just a label id →
  mirrors `fetch_folder` over `list_message_ids`). **Apply/remove reuses the M15 `batch_modify_messages`**
  command (no new apply command) for single (reading pane) + batch (M15 bar). **Browse-a-label = folder:**
  the active-mailbox value relaxed to `string` (system key OR label id); the folder fetch effect branches
  `fetchFolder` vs `fetchLabel` — reuses the whole M11/M12 active list (the `Folder` union + `Record<Folder>`
  ICON stay for system rows). New **`LabelPicker`** overlay (checkbox list, checked = on all targets, +
  create-new), a rail **"Labels" section**, and **label chips** (`LabelChips` + `userLabelChips`) on rows +
  the reading-pane header (Gmail color or uniform accent). `withLabel` relaxed to `string`. **No DB
  migration, no new OAuth scope.** N tests (2 new gmail wiremock), clippy clean, npm build clean. Maket
  verified by screenshot. **Live Gmail E2E pending owner.** **Deferred:** rename/delete/recolor labels,
  nested labels, drag-to-label, label-scoped search, per-label unread counts.
```
(Replace `N`.) Append a one-line `wiki/log.md` entry in the file's format.

- [ ] **Step 4: (No git commit — `wiki/` is gitignored.)**

---

## Self-review (completed by plan author)

**Spec coverage:** list user labels (T1) ✓; create inline (T2, T10 handleCreateLabel) ✓; fetch_label/browse (T3, T10 fetch branch) ✓; apply/remove via batch_modify_messages (T10 applyLabel) ✓; reading-pane + batch-bar picker (T6 LabelPicker, T8 batch button, T9 reading-pane button, T10 wiring) ✓; chips (T5 LabelChips + MessageItem, T9 ReadingPane) ✓; rail labels section (T7) ✓; withLabel(string) + userLabelChips (T4) ✓; no migration/scope, isTauri maket (T4 mocks) ✓; verify + wiki (T11) ✓; Rust learning comments (T1–T3) ✓; wiremock tests (T1, T2) ✓.

**Placeholder scan:** no TBD/TODO; full code in every step; `N` is explicitly "replace with the Step-1 count"; CSS token fallbacks noted where a var may be undefined.

**Type/name consistency:** `Label{id,name,color?:LabelColor{text,background}}` consistent Rust↔TS (T1/T4); `list_labels`/`create_label`/`fetch_label` (T1–T3) ↔ `listLabels`/`createLabel`/`fetchLabel` (T4); `withLabel(m, label: string, present)` (T4) used by `applyLabel` (T10); `userLabelChips(m, labelsById)` (T4) used by `LabelChips` consumers (T5/T9); `labelsById?: Map<string,Label>` consistent across MessageItem (T5)/MessageList (T8)/ReadingPane (T9); `onBatchLabel`/`onOpenLabels`/`onApply`/`onCreate`/`onClose` consistent (T6/T8/T9/T10); active `folder: string` consistent across App/FolderRail/ReadingPane (T7/T9/T10). **Every build green:** the new MessageItem/MessageList/ReadingPane/FolderRail props are optional (defaults) so consumers compile before T10 wires them; FolderRail's `folder: string` relaxation is paired with App's relaxation in the SAME task (T7, Step 1b) to avoid a red window.
