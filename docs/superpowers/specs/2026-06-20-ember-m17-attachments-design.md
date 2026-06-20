# Ember — Milestone 17: Attachments (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** **Receive** — list a message's attachments in the reading pane and save any to disk via a native Save-As dialog. **Send** — attach files to a new message or reply (`multipart/mixed`). Last of the M14→M15→M16→M17 arc. **No new OAuth scope** (`gmail.modify` already permits `messages.get`, `messages.attachments.get`, `messages.send`), **no DB migration**.

**Architecture in one paragraph:** Receiving rides the *existing* `format=full` fetch — `get_message_body` already walks the MIME tree for the body, so we add a sibling `collect_attachments` walk that gathers parts with a non-empty `filename` + an `attachmentId`, and surface that list on the `MessageBody` the reading pane already loads (zero extra round-trips for the *list*; bytes are fetched only on click, via a new `get_attachment` → a `download_attachment(message_id, attachment_id, dest_path)` command that writes with Rust `std::fs`). Sending adds a **new** pure builder `build_multipart_rfc822(msg, attachments, boundary)` *beside the unchanged `build_rfc822`* — drafts and attachment-free sends keep today's single-`text/plain` path completely untouched; only when files are attached does `send_email` pick the multipart builder (`multipart/mixed` = the text part + one base64 part per file), passing a `SystemTime`-derived boundary so `mime.rs` stays pure/clockless. The two builders share one extracted `outgoing_headers(msg)` helper for the From/To/Cc/Subject/threading lines. **Drafts stay text-only**: attachments work on Send and Reply only.

**Tech Stack:** Rust (reqwest, serde, base64, Tauri 2, **new: `tauri-plugin-dialog`**; wiremock for tests), React 19 + TypeScript + Vite, lucide-react, **new: `@tauri-apps/plugin-dialog`**.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M16 are merged to `main` (M16 pending-merge per the roadmap, but its plumbing is the baseline here). Ember reads/classifies/mutates/sends mail, has settings + calendar + search + folders + notifications + drafts + batch actions/undo + arbitrary labels. What it **cannot** do today: see or save an attachment someone sent (the MIME walk in `collect_body` extracts only `text/plain`/`text/html` and ignores attachment parts), and attach a file when composing (`build_rfc822` emits a single `text/plain` part). Both M8 (send) and M14 (drafts) explicitly punted "attachments → M17". M17 is the **last of the M14→M15→M16→M17 arc**.

**Reuse map:** the existing `format=full` fetch + the recursive `collect_body` MIME walk (sibling `collect_attachments`); the `RawBody`/`MessageBody` body path (`fetch_message_body` command); the pure `mime.rs` `build_rfc822` + `base64_body` + `sanitize_header` + `encode_subject` (RFC2047); the M8 compose/reply `ComposeModal` + `send_email` command + `SendEmailPayload`; the M14 failed-send outbox fallback; the M13 plugin-init pattern in `lib.rs` (notification → dialog); the `isTauri()` mock seam in `lib/api.ts` + `lib/mock.ts`.

---

## Scope

**In scope (lean v1):**
- **Receiving:** an attachment list (filename · size · type icon) in the reading pane; click a chip → native Save-As dialog → write the bytes to the chosen path.
- **Sending:** an "Attach" control in compose → native multi-file picker → attached-file chips with remove; files ride a `multipart/mixed` message on **Send and Reply**. Total size capped at **25 MB** (Gmail's UI limit).
- A **browser mock** so attachments render on a mock message and the compose attach UI is demoable in the maket (pickers stubbed under `!isTauri()`).

**Explicitly deferred (not in M17):**
- **Attachments in drafts** — drafts stay text-only. **Save-as-draft and the failed-send outbox fallback drop attachments** (preserving the text) with a clear inline message.
- **Inline `cid:` images** — they appear as attachments in the list (the sandboxed `srcDoc` iframe can't load them); no inline embedding or content-id rewriting.
- **List-level paperclip indicator** — the row preview uses `format=metadata`, which omits MIME parts, so "has attachment" isn't cheaply available; attachments are visible in the **reading pane only** in v1.
- Drag-and-drop attach, thumbnails/preview, per-file upload progress.
- **Resumable upload** for >25 MB (and the ~35 MB raw-`send` ceiling) → a clear hard error instead.
- The **`OutgoingFields` flat-args refactor** — kept as a noted follow-up; `send_email` keeps its flat args (one new `Vec<String>` param) under its existing `#[allow(clippy::too_many_arguments)]`.

---

## Components

### Backend — `GmailClient` (`src-tauri/src/gmail/mod.rs`, types in `gmail/types.rs`)

**`gmail/types.rs` — extend the existing MIME types (additive, all `#[serde(default)]`):**
- `PartBody { data, attachment_id: Option<String> (rename "attachmentId"), size: i64 }` — Gmail puts the attachment handle + byte size on the part's `body`.
- `MessagePart { …, filename: String }` — non-empty only on attachment parts.
- New public serde type (snake_case to the frontend):
  ```rust
  pub struct AttachmentMeta {
      pub filename: String,
      pub mime_type: String,
      pub size: i64,
      pub attachment_id: String,
  }
  ```

**`gmail/mod.rs`:**
- `collect_attachments(part: &MessagePart, out: &mut Vec<AttachmentMeta>)` — recursive walk mirroring `collect_body`; push a part when `!part.filename.is_empty()` **and** `part.body.attachment_id.is_some()`, mapping `mime_type`/`filename`/`body.size`/`body.attachment_id`.
- `get_message_body` → return `RawBody { html, text, attachments: Vec<AttachmentMeta> }` (add the field; same single `format=full` fetch — call `collect_attachments` alongside `collect_body`).
- `get_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>>` — `GET /gmail/v1/users/me/messages/{message_id}/attachments/{attachment_id}` → `{ data: base64url }` → decode **to bytes** via a new `decode_b64url_bytes(data) -> Option<Vec<u8>>` (the existing `decode_b64url` String helper delegates to it + `from_utf8_lossy`). Returns raw bytes (not a String — attachments are binary).

### Backend — commands (`src-tauri/src/commands.rs`, registered in `lib.rs`)
- `MessageBody` gains `pub attachments: Vec<AttachmentMeta>` — populated from `RawBody.attachments` in `fetch_message_body` (both the HTML and plain-text branches set it).
- `download_attachment(message_id: String, attachment_id: String, dest_path: String) -> Result<()>` — `get_attachment(...)` → `std::fs::write(&dest_path, &bytes)` (an `io::Error` maps to `AppError`). DB-free. **Register in `lib.rs`.**
- `send_email` gains `attachment_paths: Vec<String>` — for each path: `std::fs::read` the bytes (io error → `AppError`), mime via a small dep-free `mime_for_ext(path) -> &str` (common types: pdf/png/jpg/jpeg/gif/txt/csv/zip/doc(x)/xls(x)/ppt(x)/… with an `application/octet-stream` fallback), build `OutgoingAttachment`. **Enforce ≤ 25 MB total** (sum of byte lengths) → else `AppError` ("Attachments exceed the 25 MB limit"). When `attachment_paths` is **empty** → `build_rfc822(&msg)` (today's path, unchanged); else generate a boundary from `SystemTime::now()` nanos (e.g. `format!("ember_boundary_{nanos}")`) and call `build_multipart_rfc822(&msg, &attachments, &boundary)`. `save_draft`/`send_draft`/`create_draft` are **untouched** (drafts text-only).
- Init the dialog plugin in `run()`: `.plugin(tauri_plugin_dialog::init())` (mirrors the M13 notification-plugin init).

### Backend — pure MIME (`src-tauri/src/mime.rs`) — stays clockless/random-free
- New type:
  ```rust
  pub struct OutgoingAttachment { pub filename: String, pub mime_type: String, pub bytes: Vec<u8> }
  ```
- **`build_rfc822` and `OutgoingMessage` are left UNCHANGED** — drafts and attachment-free sends keep the exact single-part path (and its tests) they have today.
- Extract `fn outgoing_headers(msg: &OutgoingMessage) -> Vec<String>` — the shared From/To/Cc/Subject/In-Reply-To/References lines (CR/LF-sanitized, subject RFC2047). `build_rfc822` is refactored to call it (its emitted bytes stay identical — covered by the existing tests).
- New `pub fn build_multipart_rfc822(msg: &OutgoingMessage, attachments: &[OutgoingAttachment], boundary: &str) -> String`:
  - Headers: `outgoing_headers(msg)` + `MIME-Version: 1.0` + `Content-Type: multipart/mixed; boundary="<boundary>"`.
  - Body:
    ```
    --<boundary>
    Content-Type: text/plain; charset="utf-8"
    Content-Transfer-Encoding: base64

    <base64_body(msg.body)>
    --<boundary>
    Content-Type: <mime>; name="<filename>"
    Content-Disposition: attachment; filename="<filename>"
    Content-Transfer-Encoding: base64

    <base64_bytes(bytes)>
    --<boundary>--
    ```
    (one middle block per attachment; CRLF throughout.)
  - New `base64_bytes(&[u8]) -> String` (STANDARD, wrapped at 76 / CRLF — like `base64_body` but over raw bytes).
  - **Filename safety:** apply the existing `sanitize_header` (CR/LF → space) to the filename, and **RFC2047-encode a non-ASCII filename** by reusing the `encode_subject` pattern (factor a shared `encode_word` helper, or apply `encode_subject` to the filename value). ASCII filenames pass through.

### Frontend — api + helpers
- `lib/api.ts`:
  - `MessageBody` gains `attachments: Attachment[]`; new `interface Attachment { filename: string; mime_type: string; size: number; attachment_id: string }`.
  - `downloadAttachment(messageId, attachmentId, destPath): Promise<void>` (`isTauri()`-gated; no-op resolve in the maket).
  - `SendEmailPayload` gains `attachment_paths: string[]`; `sendEmail` passes `attachmentPaths`.
- `lib/attachments.ts` (new, pure): `formatBytes(n)` (e.g. `12 KB`, `3.4 MB`), `basename(path)` (last `/`-segment for display).

### Frontend — components
- **`ReadingPane.tsx`:** below the body, an **attachments strip** — one chip per `body.attachments` entry (paperclip icon + filename + `formatBytes(size)`). Click → `save({ defaultPath: filename })` from `@tauri-apps/plugin-dialog` (gated on `isTauri()`) → if a path is returned, `downloadAttachment(msg.id, att.attachment_id, path)` → inline per-chip "Saved"/error feedback. No strip when `attachments` is empty.
- **`ComposeModal.tsx`:** an **"Attach" button** → `open({ multiple: true })` (gated) → append to an `attachPaths: string[]` state → render attached-file **chips** (`basename` + a remove ✕). `handleSend` passes `attachment_paths: attachPaths`. **Save-as-draft and the outbox fallback ignore attachments** — show an inline note when attachments are present ("Drafts don't keep attachments yet — they'll send but won't be saved to a draft"). Reply reuses this modal, so reply attachments work for free.
- **`lib/mock.ts`:** a mock message carrying a couple of `AttachmentMeta`; stub the pickers (`open`/`save`) under `!isTauri()` so the maket demos both strips (e.g. `mockPickFiles()` returns sample paths so the compose chips render).

### Frontend — Tauri config / capabilities
- `package.json`: add `@tauri-apps/plugin-dialog`.
- `src-tauri/Cargo.toml`: add `tauri-plugin-dialog = "2"`.
- `src-tauri/capabilities/default.json`: add `"dialog:default"` (or the narrower `dialog:allow-open` + `dialog:allow-save`) to `permissions`. **No `fs:` capability is needed** — the byte *write* happens in Rust (`download_attachment`), which has direct OS access; the dialog plugin is used only to pick paths.

### Data flow
- **Receive list:** `open message → fetchMessageBody → MessageBody.attachments → reading-pane strip` (same single fetch as the body).
- **Receive save:** `chip click → dialog.save(defaultPath) → download_attachment(mid, aid, path) → get_attachment + std::fs::write`.
- **Send:** `Attach → dialog.open(multiple) → attachPaths → Send → send_email(attachmentPaths) → std::fs::read each + size-cap + mime → build_multipart_rfc822 → messages.send`.

---

## Error handling

- **Send:** an over-cap total or an unreadable file → `AppError` surfaced in the existing compose error line (before any network call). A network/send failure still falls back to **Save text to Drafts** (the M14 outbox), now with an explicit "attachments weren't saved — re-attach and retry" message.
- **Download:** a fetch or `std::fs::write` failure → an inline error on the attachment chip (the message stays open).
- **Empty/odd MIME:** a message with no attachment parts → no strip; an attachment part missing `attachmentId` is skipped (never rendered).

---

## Testing

- **Rust — `mime.rs`:** the existing `build_rfc822` tests stay green (proving the `outgoing_headers` extraction kept its bytes identical). New `build_multipart_rfc822` tests → assert `multipart/mixed; boundary=` present, a `text/plain` part, an attachment part with `Content-Disposition: attachment; filename="…"` + base64 that round-trips back to the original bytes, and the closing `--<boundary>--`; filename CR/LF sanitize; a non-ASCII filename → RFC2047 encoded-word.
- **Rust — `gmail` wiremock:** `get_message_body` over a `format=full` body that includes an attachment part → `attachments` has the right `filename`/`mime_type`/`size`/`attachment_id` **and** the text/html body is still extracted; `get_attachment` → base64url `data` decodes to the expected bytes.
- **Rust — commands:** `mime_for_ext` unit cases (a known ext → its type; unknown → `application/octet-stream`); the 25 MB size-cap rejection.
- **Frontend:** no TS harness (consistent through M16). Verified via the **browser maket** — the reading-pane attachment strip renders from the mock message and a chip is clickable; the compose "Attach" flow shows file chips with remove. Screenshot the reading-pane strip + the compose chips.
- `cargo test` + `cargo clippy --all-targets` stay green; `npm run build` clean. **Live Gmail E2E** (real download of a received attachment + a real `multipart/mixed` send that arrives intact) is **owner-pending**, consistent with M10–M16.

---

## Known risks & decisions

- **Boundary generated in the command, injected into `mime.rs`** — keeps the MIME builder pure/clockless/testable (tests pass a fixed boundary). A `SystemTime`-nanos suffix is unique enough; a content collision is negligible and Gmail re-encodes on its side anyway.
- **Rust `std::fs` write needs no capability** — Tauri commands run in the Rust process with full OS access, so only the **`dialog:`** capability is added (for the JS pickers). This keeps the new permission surface minimal and matches the privacy-respecting posture.
- **Received bytes fetched on demand, never cached** — consistent with message bodies (also not cached). No DB migration; nothing persisted.
- **25 MB cap** — Gmail's UI limit; the raw `messages.send` ceiling is ~35 MB after base64 inflation, so 25 MB of source bytes stays safely under. Larger files → a clear error; resumable upload is deferred.
- **Drafts text-only** — Gmail's `drafts.update` replaces the whole message, so editing-then-re-saving a draft with attachments would silently drop or require re-uploading them; v1 sidesteps the footgun entirely by keeping attachments on Send/Reply only.
- **No list paperclip** — `format=metadata` (the row-preview fetch) omits parts; a reliable indicator would need a per-row `format=full` fetch (expensive) or a `has:attachment` search. Deferred; attachments surface in the reading pane.
- **`send_email` stays flat-args** — one new `Vec<String>` param under the existing `#[allow(clippy::too_many_arguments)]`; the `OutgoingFields` struct refactor remains a noted follow-up to keep the M17 diff focused.

---

## Non-goals / constraints

- **No new OAuth scope** — `gmail.modify` already permits `messages.get` (attachment metadata), `messages.attachments.get` (bytes), and `messages.send` (multipart). No reconnect.
- **No DB migration** — attachments are fetched live (receive) / read from disk at send (outgoing); nothing is cached or schema-bound.
- **Tauri build unchanged for the maket** — every new wrapper + both pickers are `isTauri()`-gated; the reading-pane strip and compose chips are pure frontend over mock data.
- **`mime.rs` stays pure** — no clock, no randomness, no I/O; the boundary and file bytes are injected by the command layer.
