# Ember — Milestone 14: Drafts & outbox (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Save, edit, send, and discard **real Gmail drafts**; surface them in a **Drafts** folder in the left rail; and make a **failed send fall back to a saved draft** (the "minimal outbox"). First of a new four-milestone arc — **M14 drafts/outbox → M15 batch actions + undo → M16 arbitrary labels → M17 attachments** — that builds on M8 compose, M7 actions, and the M12 folder rail. **No new OAuth scope** (`gmail.modify` already permits drafts); **no DB migration** (drafts are live-fetched like folders/search, never cached).

**Architecture in one paragraph:** The Gmail client gains a small set of `users.drafts.*` methods that reuse the existing JSON helpers and the M8 `build_rfc822` RFC822 builder. Drafts are **never cached** — `fetch_folder` grows a `"drafts"` branch that lists drafts, hydrates each via the existing concurrent `get_message_previews`, and stamps a new additive `draft_id` onto each preview (so the M11/M12 "active list" plumbing carries drafts unchanged). Four DB-free commands — `get_draft`, `save_draft`, `send_draft`, `delete_draft` — drive the compose lifecycle. The `ComposeModal` gains a **Save as draft** button, optional `draftId` editing, a **send-from-draft** path (`drafts.send`), a **dirty-close prompt**, and a **failed-send → save-draft fallback**. The Drafts folder is the one folder whose rows open the **compose editor** instead of the reading pane. The `isTauri()` maket seam keeps the browser build working via mock drafts.

**Tech Stack:** Rust (reqwest, serde, Tauri 2; wiremock for tests; base64), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M13 are merged to `main`. Ember reads/classifies/mutates/sends mail (M1–M8), has settings + disconnect (M9), a read-only calendar (M10), server-side search (M11), folder/Sent/Trash views (M12), and new-mail notifications (M13). M8 added compose + reply (plain text) via a pure `mime.rs` RFC822 builder + `messages.send`, but there are **no drafts** — closing a half-written message loses it, and a failed send only shows an error. M12 added the folder rail but **explicitly deferred a Drafts slot** ("Ember has no drafts feature yet — the folder would be empty"). M14 fills both gaps. It is the first of a new arc the owner requested: **M14 drafts/outbox → M15 batch actions + undo → M16 arbitrary labels → M17 attachments.**

**Reuse map:** M8 `mime::build_rfc822` + `OutgoingMessage` (raw builder); M8 `get_reply_context` parsing pattern (for `get_draft`); the Gmail client's `get_json`/`post_json`/`post_no_body` helpers and `get_message_previews` (concurrent hydration); M12's `fetch_folder` shape + `to_addr`/`showRecipient` (drafts show the recipient, since the sender is always the user); M11/M12's list-aware "active list".

---

## Scope

**In scope (lean v1):**
- **Gmail drafts** via `users.drafts.*`: create, update, get, list, send, delete.
- A **Drafts folder** in the rail (live-fetched; rows open the compose editor, not the reading pane).
- **Save as draft** from the compose modal (explicit button), plus a **dirty-close prompt**.
- **Send from a draft** (`drafts.send` — sends and removes from Drafts in one step).
- **Discard a draft** (delete) from the compose editor.
- **Minimal outbox:** a failed `sendEmail` (network/transient) saves the message as a draft and tells the user; "retry" = reopen the draft and Send again.
- A **browser mock** so drafts + compose-from-draft work in the maket.

**Explicitly deferred (not in M14):**
- **Auto-save** drafts as you type (chose explicit Save + close-prompt).
- A **full background outbox** with auto-retry on reconnect / a local queue / send-dedup (chose the draft fallback).
- **Cross-device draft conflict** resolution UI (last-write-wins; Gmail owns the draft).
- **HTML / rich-text** drafts (plain text only, consistent with M8).
- **Attachments** in drafts (→ M17).
- **Scheduled send**; multiple-draft bulk discard; a dedicated draft reading-pane preview.

---

## Components

### Backend — `GmailClient` draft methods (`src-tauri/src/gmail/mod.rs`, types in `gmail/types.rs`)
Gmail base is `…/gmail/v1/users/me`. All methods reuse existing helpers; `raw` is the base64url-no-pad encoding of `build_rfc822` output (same as `send_message`); `threadId` is omitted (not null) when absent.
- `create_draft(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<DraftRef>` — `POST /drafts` body `{ "message": { "raw", ["threadId"] } }` → `{ id, message: { id, threadId } }`.
- `update_draft(&self, draft_id: &str, raw_rfc822: &str, thread_id: Option<&str>) -> Result<DraftRef>` — `PUT /drafts/{id}` same body.
- `get_draft(&self, draft_id: &str) -> Result<DraftContent>` — `GET /drafts/{id}?format=full` → parse the message headers/body into editable fields (To, Cc, Subject, plain-text body, In-Reply-To, References, threadId), mirroring `get_reply_context`'s header/MIME walk.
- `list_drafts(&self, max: u32) -> Result<Vec<DraftRef>>` — `GET /drafts?maxResults={max}` → `{ drafts: [{ id, message: { id, threadId } }] }` (empty when none).
- `send_draft(&self, draft_id: &str, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()>` — `POST /drafts/send` body `{ id, message: { raw, ["threadId"] } }` (captures any edits **and** sends in one call). *Plan task 1 verifies this single-call form against the Gmail docs; fallback is `update_draft` then `POST /drafts/send` with `{ id }`.*
- `delete_draft(&self, draft_id: &str) -> Result<()>` — `DELETE /drafts/{id}` (reuse/extend the `delete_no_body` helper added in M12).

New serde types: `DraftRef { id: String, message_id: String, thread_id: Option<String> }` and `DraftContent { draft_id, to, cc, subject, body, in_reply_to, references, thread_id }` (snake_case to the frontend).

### Backend — `MessagePreview.draft_id` (additive, no migration)
Add `pub draft_id: Option<String>` to `MessagePreview` (`gmail/types.rs`). It is populated **only** on the drafts path. Every other constructor sets `None`: the DB `recent_previews` mapper, `get_message_preview`, search, and the other folders. No SQLite column, no migration.

### Backend — commands (`src-tauri/src/commands.rs`, registered in `lib.rs`)
All DB-free (drafts are never cached), mirroring M12/M11.
- **`fetch_folder` gains a `"drafts"` arm:** `list_drafts(max)` → build a `HashMap<message_id, draft_id>` → `get_message_previews(message_ids, …)` → set each preview's `draft_id` from the map, leave `category` empty → recency-sort. (The generic label/query arm is unchanged for the other folders.)
- `get_draft(draft_id: String) -> Result<DraftContent>`.
- `save_draft(draft_id: Option<String>, to, cc, subject, body, in_reply_to, references, thread_id) -> Result<String>` — build `OutgoingMessage` → `build_rfc822` → `create_draft` (when `draft_id` is `None`) or `update_draft` (when `Some`) → return the draft id.
- `send_draft(draft_id: String, to, cc, subject, body, in_reply_to, references, thread_id) -> Result<()>` — build raw → `send_draft`.
- `delete_draft(draft_id: String) -> Result<()>`.

### Frontend
- **`lib/folders.ts`:** add `"drafts"` to `Folder` and a `{ key: "drafts", label: "Drafts" }` entry positioned **after Sent**.
- **`lib/api.ts`:** add `draft_id?: string` to `MessagePreview`; a `DraftContent` interface; wrappers `getDraft(id)`, `saveDraft(payload)`, `sendDraft(payload)`, `deleteDraft(id)` (all `isTauri()`-gated with mock fallbacks). `fetchFolder("drafts")` already returns `MessagePreview[]` (with `draft_id`) — no new fetch wrapper needed.
- **`lib/mock.ts`:** `MOCK_DRAFTS` (a couple of previews carrying `draft_id`, `to_addr` set, `from` = the mock account) returned by `mockFolder("drafts")`; `mockGetDraft`/`mockSaveDraft`/`mockSendDraft`/`mockDeleteDraft` (return fake ids / resolve) so the maket exercises the full flow.
- **`ComposeModal.tsx`:** accept an optional `draftId` (and the fields it implies via the existing `ComposeInitial`); add a **Save as draft** button (between Cancel and Send); **Send** calls `sendDraft` when `draftId` is set, else `sendEmail`; track a `dirty`/`savedDraftId` state; the **close path** (Cancel / Esc) prompts when dirty — new compose → "Save as draft / Discard"; existing-draft-with-edits → "Save changes / Discard changes"; unchanged → close silently. **Failed-send fallback** lives here: on `sendEmail` rejection, call `saveDraft`, surface "Couldn't send — saved to Drafts," and keep the modal closeable; if the fallback `saveDraft` *also* fails (fully offline), keep the modal open with the text intact and say so.
- **`App.tsx`:** the **Drafts folder** special-cases row selection — `onSelect` for `folder === "drafts"` calls `getDraft(draftId)` and opens `ComposeModal` (pre-filled, with `draftId`) instead of selecting into the reading pane. After a draft is saved / sent / discarded, bump `folderReloadKey` so the Drafts list refetches. The reading pane shows an empty/placeholder state while in the Drafts folder. New-mail compose and reply (M8) are unchanged.

### Data flow
`Save as draft` → `saveDraft(draftId?, fields)` → create/update → returns id (modal remembers it for subsequent saves). `Open draft` → row click → `getDraft(id)` → modal pre-filled with `draftId`. `Send from draft` → `sendDraft(id, fields)` → `drafts.send` → draft removed + mail sent → modal closes, Drafts list refetches. `Send new` (no draftId) → `sendEmail` → on failure → `saveDraft` fallback. `Discard` → `deleteDraft(id)` → list refetches.

---

## Error handling

- Draft API failures surface in the compose modal's error area (or the global error bar for list fetches), same pattern as M8/M12.
- A `send_draft` failure keeps the draft intact (nothing is lost) and shows the error.
- The minimal-outbox fallback: `sendEmail` fails → `saveDraft`. If `saveDraft` also fails (no network at all), the modal stays open with the user's text and an explicit "offline — couldn't send or save; your text is here" message.
- "Dirty" = any of To/Cc/Subject non-empty, or the body differs from the initially-seeded body (signature/quoted reply). A brand-new compose containing only the seeded signature is **not** dirty.

---

## Testing

- **Rust:** wiremock unit/integration tests for the six client methods (`create`/`update`/`get`/`list`/`send`/`delete`), asserting request shape (base64url `raw`, `threadId` omitted when `None`, correct verbs/paths) and response parsing (`DraftRef`/`DraftContent`), mirroring M12's untrash/permanent-delete tests. A `fetch_folder("drafts")` test that the `draft_id` map is attached to the right preview.
- **Frontend:** no TS test harness exists (deferred since M10; consistent through M13) — the new logic is thin glue over typed wrappers; verified via the **browser maket** (Drafts folder lists mock drafts; clicking opens compose pre-filled; Save-as-draft and the close-prompt behave) and a screenshot.
- `cargo test` + `cargo clippy --all-targets` stay green; `npm run build` clean. **Live Gmail E2E** (real draft create/edit/send round-trip, the failed-send fallback) is **owner-pending**, consistent with M10–M13.

---

## Known risks & decisions

- **`drafts.send` single-call vs update-then-send (the one API detail to confirm):** the plan's first task verifies whether `POST /drafts/send` with `{ id, message: { raw } }` both updates and sends. If it does → one call. If not → `update_draft` then `POST /drafts/send` with `{ id }`. Either way the command signature (`send_draft(draft_id, …fields)`) is unchanged.
- **`MessagePreview.draft_id` on a shared struct:** drafts reuse the cached-inbox preview type rather than a parallel `DraftPreview`, so the M11/M12 list + action plumbing carries them with zero new branches. The cost is one additive `Option<String>` field that is `None` everywhere except the drafts path — accepted (same call M12 made with `to_addr`).
- **Drafts are the one folder that opens an editor, not a reader** — a deliberate special-case in `App.tsx`'s selection handler, isolated to `folder === "drafts"`.
- **Last-write-wins across devices** — if the same draft is edited elsewhere, Ember's save overwrites; no conflict UI in v1.
- **Send-then-removed:** `drafts.send` atomically sends and drops the draft from Drafts; the local list refetch reflects it. (Avoids the send-then-delete race of `messages.send` + `delete_draft`.)

---

## Non-goals / constraints

- **No new OAuth scope** — `gmail.modify` already permits `drafts.*`.
- **No DB migration** — drafts are live-fetched; `draft_id` is a non-persisted struct field.
- **Tauri build unchanged for the maket** — all new wrappers are `isTauri()`-gated with mocks.
- **Plain text only** — drafts use the same `build_rfc822` plain-text path as M8; HTML/attachments are later milestones.
