# Ember — Milestone 18: Forward + Reply-all (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Add the two missing compose entry modes. **Reply-all** seeds To = the original sender and Cc = the original To+Cc deduped, minus your own address (and the sender). **Forward** prefixes "Fwd:", inserts a plain-text forwarded-header block + the original body, starts a **fresh thread**, and **re-attaches the original's attachments** (re-fetched from Gmail). First milestone after the M14→M17 arc. **No new OAuth scope, no DB migration.**

**Architecture in one paragraph:** Both modes reuse **one enriched context fetch** and **one send path**. `get_reply_context` already does a `format=full` fetch for Message-ID / References / quoted-text; it is extended to also return the original's **To, Cc, and attachment list** (reusing the M17 `collect_attachments` walk) — so `ReplyContext` gains `to`, `cc`, `attachments` with **no extra round-trip and no new command**. Reply-all computes recipients in a pure frontend helper (self-excluded via the connected account). Forward builds a plain-text forwarded block and carries the original's attachments as **refs** (`{message_id, attachment_id, filename, mime_type}`) rather than bytes; the M17 `send_email` command gains a `forwarded_attachments` param and, before building the multipart, **re-fetches each ref via the M17 `get_attachment`** and merges them with any disk files the user attached — the existing `build_multipart_rfc822` + the 25 MB cap (now over the combined total) handle the rest. The new modes are just different `ComposeInitial` values built in `App.tsx`; the compose modal renders forwarded attachments as removable chips alongside M17's file chips. `getReplyContext` is also `isTauri()`-gated (with a mock) so Reply / Reply-all / Forward all work in the browser maket.

**Tech Stack:** Rust (reqwest, serde, base64, Tauri 2; wiremock tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M17 are merged to `main`. Ember reads/classifies/mutates/sends mail, has settings + calendar + search + folders + notifications + drafts + batch actions/undo + arbitrary labels + attachments (receive + send). Compose today supports **new message** and **reply** (plain text, M8) — but there is **no reply-all and no forward** (both deferred since M8). M18 adds them, building directly on the M17 attachment plumbing (forward re-attaches the original's files).

**Reuse map:** the M8 reply path (`get_reply_context`, `ComposeInitial`, `ComposeModal`, `replySubject`/`quoteBody`/`parseAddress`/`parseRecipients`); the M17 attachment plumbing (`collect_attachments`, `AttachmentMeta`, `GmailClient::get_attachment`, `build_multipart_rfc822`, `MAX_ATTACHMENT_BYTES`, the M17 compose attach chips + `send_email(attachment_paths)`); the `isTauri()` mock seam (`lib/api.ts` + `lib/mock.ts`).

---

## Scope

**In scope (lean v1):**
- **Reply-all** — a toolbar button → To = the sender, Cc = (original To + Cc) deduped, **minus your own address and the sender**. Threads into the conversation exactly like Reply.
- **Forward** — a toolbar button → subject "Fwd: …", a `---------- Forwarded message ---------` block (From / Date / Subject / To) + the original plain-text body, **empty To/Cc**, a **fresh thread** (no `threadId` / `In-Reply-To` / `References`), and the **original's attachments re-attached** (shown as removable chips before send).
- Reading-pane toolbar gains **Reply-all** + **Forward** next to Reply; the compose modal title reflects the mode ("Reply all" / "Forward").
- `getReplyContext` is `isTauri()`-gated + a `mockReplyContext`, so Reply / Reply-all / Forward all work in the browser maket.

**Explicitly deferred (not in M18):**
- **HTML / rich-text forward** — plain-text only; the original's `text/plain` part is quoted (HTML formatting is lost). Inline `cid:` images travel as regular attachments (consistent with M17).
- **"Forward as `.eml` attachment"** (the whole original message as a single attachment).
- **Recipient-chip editing UI** — recipients stay the existing comma-separated To/Cc text fields.
- **Alias-aware self-exclusion** — only the single connected-account address is excluded from reply-all (a "+alias" or secondary address is not).
- **The `OutgoingFields` flat-args refactor** — still deferred; `send_email` keeps flat args under its existing `#[allow(clippy::too_many_arguments)]`, gaining one `forwarded_attachments` param.

---

## Components

### Backend — `gmail/types.rs`
- `ReplyContext` gains three fields:
  ```rust
  pub to: String,                          // raw To header (may hold several addresses)
  pub cc: String,                          // raw Cc header ("" when absent)
  pub attachments: Vec<AttachmentMeta>,    // reuses the M17 type
  ```
  (Existing fields `message_id`, `references`, `quoted_text` unchanged.)

### Backend — `gmail/mod.rs` `get_reply_context`
- Extract the `To` and `Cc` headers via the **existing case-insensitive `header(name)` closure** already used for `Message-ID`/`References`.
- Call the M17 `collect_attachments(&full.payload, &mut attachments)` on the **same `format=full` payload** (no extra round-trip).
- Return them in the enriched `ReplyContext`.

### Backend — `commands.rs` `send_email`
- New param `forwarded_attachments: Vec<ForwardedAttachmentRef>` (8th→9th flat arg, under the existing `#[allow(clippy::too_many_arguments)]`).
- New type (Deserialize; JS passes snake_case keys, matching the M17 `Attachment` shape + a `message_id`):
  ```rust
  #[derive(serde::Deserialize)]
  pub struct ForwardedAttachmentRef {
      pub message_id: String,
      pub attachment_id: String,
      pub filename: String,
      pub mime_type: String,
  }
  ```
- Logic: the no-attachment early-return guard becomes `attachment_paths.is_empty() && forwarded_attachments.is_empty()` → unchanged `build_rfc822` path. Otherwise: read disk files into `OutgoingAttachment` (M17), **then** for each forwarded ref `client.get_attachment(&message_id, &attachment_id).await?` → an `OutgoingAttachment { filename, mime_type, bytes }`, push into the same vec; accumulate `total` across **both** sources with `saturating_add`; the **25 MB cap fires on the combined total before building**; then `build_multipart_rfc822`. (Drafts untouched — still text-only.)
- No new command, no new client method (reuses `get_attachment` + `build_multipart_rfc822`).

### Frontend — `lib/compose.ts` (pure helpers)
- `forwardSubject(subject) -> string` — prefixes "Fwd: " unless already present (case-insensitive), mirroring `replySubject`.
- `replyAllRecipients(from, to, cc, self) -> { to: string; cc: string }` — To = `parseAddress(from)`; Cc = the deduped union of the addresses parsed from the `to` and `cc` header strings, **dropping** `self` and the To-sender (case-insensitive); returns comma-joined strings.
- `forwardBlock(from, dateLabel, subject, to) -> string` — the plain-text forwarded-message header block:
  ```
  ---------- Forwarded message ---------
  From: <from>
  Date: <dateLabel>
  Subject: <subject>
  To: <to>

  ```

### Frontend — `lib/api.ts`
- `ReplyContext` interface gains `to: string`, `cc: string`, `attachments: Attachment[]`.
- New `interface ForwardedAttachmentRef { message_id: string; attachment_id: string; filename: string; mime_type: string }`.
- `SendEmailPayload` gains `forwarded_attachments: ForwardedAttachmentRef[]`; `sendEmail` forwards it as `forwardedAttachments`.
- `getReplyContext` becomes `isTauri()`-gated → `mockReplyContext(id)` in the maket.

### Frontend — `components/ComposeModal.tsx`
- Track `forwardedAttachments: ForwardedAttachmentRef[]` state, seeded from `initial.forwardedAttachments ?? []`.
- Render them as **removable chips** alongside the M17 file chips (a paperclip + `filename` + a remove ✕; a small visual cue they came from the forwarded message is optional).
- Include the remaining forwarded refs in the send payload (`fields()` returns `forwarded_attachments: forwardedAttachments`).
- `dirty` also true when `forwardedAttachments.length > 0`.
- Title resolves from a new `initial.mode` (`"forward"` → "Forward", `"replyAll"` → "Reply all", else the existing draft/threadId logic).

### Frontend — `components/ReadingPane.tsx`
- Add **Reply-all** (`ReplyAll` icon) and **Forward** (`Forward` icon) buttons to the toolbar next to the existing Reply, wired to new `onReplyAll(m)` / `onForward(m)` props.

### Frontend — `components/ComposeModal.tsx` type + `App.tsx`
- `ComposeInitial` gains `mode?: "new" | "reply" | "replyAll" | "forward" | "draft"` and `forwardedAttachments?: ForwardedAttachmentRef[]`.
- `App.tsx`:
  - `handleReplyAll(m)` — `getReplyContext(m.id)` → `replyAllRecipients(m.from, ctx.to, ctx.cc, account)` → `ComposeInitial` like Reply but with the computed Cc, `mode: "replyAll"` (threads in: `inReplyTo`/`references`/`threadId` as Reply does).
  - `handleForward(m)` — `getReplyContext(m.id)` → `dateLabel` computed exactly as in `handleReply` (`m.internal_date ? new Date(m.internal_date).toLocaleString() : m.date`); `body = appendSignature(forwardBlock(m.from, dateLabel, m.subject, ctx.to) + ctx.quoted_text, settings.signature)` (signature appended at the end, matching the reply convention); `subject = forwardSubject(m.subject)`; **empty To/Cc**; `inReplyTo: null, references: null, threadId: null`; `mode: "forward"`; `forwardedAttachments = ctx.attachments.map(a => ({ message_id: m.id, attachment_id: a.attachment_id, filename: a.filename, mime_type: a.mime_type }))`.
  - Pass `onReplyAll`/`onForward` to `ReadingPane`. `account` (connected address) is already loaded in `App`.

### Frontend — `lib/mock.ts`
- `mockReplyContext(id)` — returns `{ message_id, references, quoted_text, to, cc, attachments }`; for `m1` include the two M17 mock attachments so a forward demos the re-attach chips, and a Cc so reply-all demos a prefilled Cc.

### Data flow
- **Reply-all:** `Reply-all → getReplyContext → replyAllRecipients(self=account) → ComposeInitial (mode replyAll, threads in) → send`.
- **Forward:** `Forward → getReplyContext → forwardBlock + body + forwardedAttachments → ComposeInitial (mode forward, fresh thread) → Send → send_email re-fetches each ref via get_attachment → merged multipart`.

---

## Error handling

- A forwarded-attachment re-fetch failure during send → `AppError` surfaced in the compose error line (the message stays open); the M17 failed-send outbox still saves text to Drafts (attachments dropped, with the existing note).
- Combined disk + forwarded attachments over 25 MB → the M17 cap error, before any send.
- A `getReplyContext` failure (reply-all / forward) → the existing global error bar (same as Reply today).

---

## Testing

- **Rust:** extend the `get_reply_context` wiremock test so the `format=full` response carries `To`/`Cc` headers + an attachment part, and assert the enriched `ReplyContext` parses `to`/`cc`/`attachments`. (The `send_email` forwarded-merge is command glue over the already-tested `get_attachment` + `build_multipart_rfc822`; verified by build + E2E, consistent with the repo's command-test boundary.)
- **Frontend:** the new pure helpers (`forwardSubject`, `replyAllRecipients`, `forwardBlock`) are simple and pure; no TS harness (consistent through M17). Maket-verified by screenshot: Reply-all opens with a **prefilled Cc**; Forward opens with the **forwarded block** + the **original attachment chips**.
- `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean. **Live Gmail E2E** (a real reply-all reaching all recipients; a real forward arriving with the original attachments intact) is **owner-pending**, consistent with M10–M17.

---

## Known risks & decisions

- **One enriched context fetch** — `get_reply_context` + To/Cc/attachments serves reply, reply-all, and forward; no duplicate `format=full` calls and no new command. The type is named `ReplyContext` but now serves forward too (a rename to `MessageContext` is a noted, non-blocking follow-up — kept out to minimize churn).
- **Forwarded attachments travel as refs, re-fetched at send** — not downloaded to disk; reuses M17's `get_attachment`. The user can remove any forwarded chip before sending. Trade-off: a re-fetch happens at send time (acceptable; same bytes the recipient would get).
- **Forward starts a fresh thread** (no `threadId` / `In-Reply-To`) — matches Gmail; reply-all threads in like Reply.
- **Self-exclusion is single-address** — only the connected account is dropped from reply-all; aliases/secondary addresses are not (deferred).
- **`getReplyContext` gated for the maket** — closes the same class of gap M17 closed for `fetchMessageBody` (Reply already threw in the browser maket today); a `mockReplyContext` makes all three modes demoable offline. (The broader action-invoke maket gating remains a separate, already-filed follow-up.)
- **`OutgoingFields` refactor still deferred** — one new flat param under the existing allow, keeping M18 focused (consistent with M14–M17).

---

## Non-goals / constraints

- **No new OAuth scope** — `gmail.modify` already permits `messages.get` (headers + attachments), `messages.attachments.get`, and `messages.send` (multipart).
- **No DB migration** — context + forwarded attachments are fetched live; nothing cached or schema-bound.
- **Tauri build unchanged for the maket** — the new wrapper is `isTauri()`-gated; the compose chips, toolbar buttons, and pure helpers are frontend over mock data.
- **Plain-text only** — consistent with M8 compose; HTML forward is out of scope.
