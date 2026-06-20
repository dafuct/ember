# Ember — Milestone 16: Arbitrary labels (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** List the user's Gmail labels, **apply/remove** them on messages (single + batch), **browse** a label from the rail, see a message's labels as **chips**, and **create** a new label inline. Third of the M14→M15→M16→M17 arc. **No DB migration, no new OAuth scope** (`gmail.modify` already permits `users.labels.*` + `messages.modify`).

**Architecture in one paragraph:** Applying/removing a label is just a label-id add/remove, so it **reuses the M15 `batch_modify_messages` command** (a 1-element id list for single, the selection for batch) — whose `apply_label_delta` reconcile already keeps the cache consistent for non-INBOX/non-TRASH label changes. The genuinely-new backend is small: list user labels, create a label, and fetch one label's messages (a thin mirror of M12's `fetch_folder` generic path over `list_message_ids(Some(labelId), …)`). On the frontend, **browsing a label is modeled as a folder**: the `folder` state is relaxed from the fixed system-folder union to a `string`, the rail's user-label rows set `folder = <labelId>`, and the folder fetch effect branches (known system key → `fetchFolder`, else → `fetchLabel`) — reusing M11/M12's entire "active list" with no new list source. A `LabelPicker` popover (checkbox list + create-new input) opens from the reading pane (single message) and the M15 batch bar (selection); applied user labels render as small chips on rows + the reading-pane header.

**Tech Stack:** Rust (reqwest, serde, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M15 are merged to `main`. Ember reads/classifies/mutates/sends mail, has settings + calendar + search + folders + notifications + drafts + batch actions/undo. Labels are *applied* by id today (M7 `modify_message`, M15 `batch_modify`) and read/star are special-cased system labels, but there is **no way to list, create, apply, browse, or see arbitrary user labels**. M16 adds that. It is the third of the new arc (**M14 drafts → M15 batch+undo → M16 labels → M17 attachments**).

**Reuse map:** M15 `batch_modify_messages` command (+ `db::apply_label_delta`) for apply/remove; M12 `fetch_folder`/`list_message_ids(label, …)` + the FolderRail + the 3-way active list; M11/M12 active-list action handlers; `lib/labels.ts` `withLabel` (generalized here); the Gmail client JSON helpers; the M15 `MessageList` batch bar.

---

## Scope

**In scope (lean v1):**
- **List** the user's labels (`users.labels.list`, filtered to `type == "user"`), with name + Gmail color.
- **Apply/remove** a user label on the open message (reading pane) and on the multi-selection (batch bar), via a **`LabelPicker`** popover — reusing `batch_modify_messages`.
- **Create** a new label inline from the picker (`users.labels.create`).
- **Browse** a user label from the rail (a "Labels" section), reusing the folder/active-list machinery.
- **Label chips** on message rows + the reading-pane header (a message's user labels, name + color).
- A **browser mock** so labels (rail, picker, chips, browse) work in the maket.

**Explicitly deferred (not in M16):**
- **Rename / delete / recolor** labels (use Gmail web).
- **Nested labels** (treat `Parent/Child` names as flat strings).
- **Drag-to-label**, label-scoped search, per-label unread counts in the rail, label reordering.
- Applying **system** labels (INBOX/IMPORTANT/etc.) via the picker (only user labels are listed).
- A label *management* screen.

---

## Components

### Backend — `GmailClient` (`src-tauri/src/gmail/mod.rs`, types in `gmail/types.rs`)
- `list_labels(&self) -> Result<Vec<Label>>` — `GET /gmail/v1/users/me/labels`. Parse into a wire `RawLabel { id, name, type, color }`, **filter to `type == "user"`**, map to the public `Label { id, name, color }`. (System labels — INBOX/SENT/UNREAD/CATEGORY_*/etc. — are excluded; they're handled by the rail's fixed folders, the scorer, and read/star.)
- `create_label(&self, name: &str) -> Result<Label>` — `POST /gmail/v1/users/me/labels` body `{ "name", "labelListVisibility": "labelShow", "messageListVisibility": "show" }` → parse the returned label resource into `Label`.

New public serde types (snake_case to the frontend):
```rust
pub struct Label { pub id: String, pub name: String, pub color: Option<LabelColor> }
pub struct LabelColor { pub text: String, pub background: String }   // Gmail's textColor/backgroundColor (hex)
```
Wire structs (private): a `LabelsListResponse { labels: Vec<RawLabel> }` and `RawLabel { id, name, #[serde(rename="type")] label_type, color: Option<LabelColor> }`. `color` is `None` for labels with no custom color.

### Backend — commands (`src-tauri/src/commands.rs`, registered in `lib.rs`)
All DB-free.
- `list_labels() -> Result<Vec<Label>>`.
- `create_label(name: String) -> Result<Label>`.
- `fetch_label(label_id: String, max: u32) -> Result<Vec<MessagePreview>>` — `max.clamp(1, SEARCH_MAX)`; `list_message_ids(Some(&label_id), "", false)` → `get_message_previews` → recency-sort. (Identical shape to `fetch_folder`'s generic arm; a user label is just a label id.)
- **Apply/remove reuses the existing M15 `batch_modify_messages(ids, add, remove)`** — no new command.

### Frontend — api + labels helpers
- `lib/api.ts`: `Label`/`LabelColor` interfaces; `listLabels()`, `createLabel(name)`, `fetchLabel(id, max)` wrappers (`isTauri()`-gated with mocks).
- `lib/labels.ts`: **relax `withLabel(m, label: string, present)`** (was `LabelId`) so arbitrary label ids work (system callers `toggleRead`/`toggleStar` unchanged); add `userLabelChips(m, labelsById: Map<string, Label>): Label[]` — the message's `label_ids` intersected with the user-label map, for chip rendering.

### Frontend — components
- **`LabelPicker.tsx`** (new): a small popover anchored to its trigger. Renders the user labels as a checkbox list (checked = applied to **all** target messages → single is exact, batch checks only labels common to every selected message) + a "Create new label…" text input. Props: `labels: Label[]`, `targets: MessagePreview[]`, `onApply(labelId, add)`, `onCreate(name)`, `onClose`. Closes on outside-click / Esc.
- **`LabelChips.tsx`** (new, small): renders a list of `Label` as chips (name; background = `color.background` if set, else a uniform accent). Used by `MessageItem` (row) and `ReadingPane` (header).
- **`MessageItem.tsx`**: render `LabelChips` for the message's user labels (needs a `labelsById` prop threaded from `MessageList`).
- **`MessageList.tsx`**: thread `labelsById` to rows; add a **"Label" button** to the M15 batch bar that opens the `LabelPicker` for the selection.
- **`ReadingPane.tsx`**: a **"Labels" control** (button) opening the `LabelPicker` for the open message + chips in the header.
- **`FolderRail.tsx`**: a **"Labels" section** below the system folders — one row per user label (color dot + name), each setting the active folder to its label id; active-highlight like the system rows.

### Frontend — `App.tsx`
- Load labels on mount: `listLabels()` → `labels: Label[]` state + a derived `labelsById: Map<string, Label>`. Refetch after a successful `createLabel`.
- **Browse-as-folder:** relax the `folder` state type to `string` (system folders keep their keys: `inbox`/`sent`/…; user labels use their Gmail ids). The folder fetch `useEffect` branches: if `folder` is a known system key (`FOLDERS.some(f => f.key === folder)`) → `fetchFolder(folder)`, else → `fetchLabel(folder)`. The list header title resolves from `labelsById.get(folder)?.name` for label ids. `inFolder = folder !== "inbox"` is unchanged.
- **Apply wiring:** a shared `applyLabel(targets, labelId, add)` → optimistic `withLabel` on each target in the active list (+ the open message) → `batchModifyMessages(targets.map(id), add ? [labelId] : [], add ? [] : [labelId])` → roll back on error. Used by the reading-pane picker (target = `[selected]`) and the batch picker (target = `selectedMsgs`). `createLabel` → refetch labels → apply the new label to the current target.
- Thread `labelsById` to `MessageList` (chips) and the pickers.

### Data flow
`mount → listLabels → labelsById` (rail + picker + chips). `apply → picker toggle → applyLabel → optimistic withLabel + batchModifyMessages`. `browse → rail label click → folder = labelId → fetchLabel`. `create → picker input → createLabel → refetch labels → apply`.

---

## Error handling

- Label API failures (`list`/`create`/`fetch`) surface in the global error bar (existing pattern); a failed `fetchLabel` shows the folder error state.
- Apply rolls back the optimistic `withLabel` on the active list (the M7/M15 optimistic-rollback pattern) and surfaces the error; selection behavior matches M15.
- A failed `createLabel` keeps the picker open with the typed name + an inline error.
- Label browsing of an empty label shows the existing folder empty-state.

---

## Testing

- **Rust:** wiremock tests for `list_labels` (response with mixed system+user labels → only user labels returned; a label with a `color` parses, one without → `None`), `create_label` (POST body has `name`; response parsed to `Label`), and `fetch_label` (`labelIds=<id>` list → hydrated previews). Mirrors M12/M14/M15 wiremock style.
- **Frontend:** no TS harness (consistent through M15). Verified via the **browser maket**: mock labels appear in the rail "Labels" section; the picker applies a chip to a message; chips render on rows; clicking a rail label browses its (mock) messages; "create" adds a label. Screenshot the rail section + a chip + the picker.
- `cargo test` + `cargo clippy --all-targets` stay green; `npm run build` clean. **Live Gmail E2E** (real label list/create/apply/browse) is **owner-pending**, consistent with M10–M15.

---

## Known risks & decisions

- **Apply reuses `batch_modify_messages` (no new command)** — a single-message label change is `batch_modify_messages([id], [labelId], [])`, which lands in the `apply_label_delta` cache branch (not delete). Deliberate: one apply path for single + batch, and the cache stays consistent.
- **Browse-a-label = `folder: string`** — relaxing the `Folder` union to a string id makes user labels first-class "folders" reusing the M12 active-list, at the cost of a `FOLDERS.some(...)` branch in the fetch effect and title lookup. Gmail-idiomatic (folders *are* labels). Risk: a user label whose id collided with a system key — impossible, Gmail user-label ids are `Label_<n>`, distinct from the lowercase system keys Ember uses.
- **Batch picker "checked = on all targets"** — a label is shown checked only when every selected message has it; toggling on adds to all, off removes from all. Exact for a single target; a sensible, predictable batch rule (no tri-state in v1).
- **Chips show user labels only** — `userLabelChips` intersects `label_ids` with the user-label map, so INBOX/UNREAD/CATEGORY_*/STARRED never render as chips.
- **Label list staleness** — labels load on mount + refetch after create; a label created in Gmail web mid-session won't appear until relaunch (acceptable; no background label sync in v1).

---

## Non-goals / constraints

- **No new OAuth scope** — `gmail.modify` permits `labels.list`/`labels.create`/`messages.modify`.
- **No DB migration** — label application rides the existing `label_ids` column via `apply_label_delta`; labels themselves are fetched live, never cached.
- **Tauri build unchanged for the maket** — every new wrapper is `isTauri()`-gated; the picker/chips/rail are pure frontend over mock data.
- **User labels only** — system labels are out of the picker and chips.
