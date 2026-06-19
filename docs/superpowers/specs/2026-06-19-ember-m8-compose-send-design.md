# Ember — Milestone 8: Compose & Send (lean v1) — Design Spec

**Status:** Approved design (2026-06-19). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Let the user **send** mail from Ember — both a fresh **compose** and a **reply** to an
open message — as plain text, from a focused modal, with a clear "Sent" confirmation and
error handling that never loses what they typed.

**Architecture in one paragraph:** A new pure Rust `mime` module assembles an RFC822 message
(headers + base64 plain-text body; RFC2047 encoded-word for non-ASCII subjects) from an
`OutgoingMessage`. The Gmail client — read + modify today — gains `send_message` (POST
`users.messages.send` with the base64url-encoded raw message + optional `threadId`) and a
`get_reply_context` fetch (the original's `Message-ID`/`References` headers + its plain-text
body, for threading and quoting). Two thin Tauri commands (`send_email`, `get_reply_context`)
sit on top; sending is **DB-free** (sent mail goes to Gmail's Sent folder, which Ember does not
cache). The React frontend adds a `ComposeModal` (To/Cc/Subject/body), a pure `compose` helper
(recipient parsing, `Re:` normalization, quote building, validation), a header **Compose**
button, and wires the reading-pane **Reply** button. No SQLite schema change.

**Tech Stack:** Rust (reqwest, serde, base64, wiremock for tests), Tauri 2, React 19 +
TypeScript + Vite, lucide-react icons.

**Learning mode (IMPORTANT — applies to every implementer):** The repo owner is learning Rust.
All Rust code MUST carry concise `// 🦀` teaching comments explaining the *language* concept
(ownership/borrowing, `Result`/`Option`/`?`, slices, iterators, traits, closures, derive macros),
not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets
normal comments — the owner knows JS/React.

---

## Milestone context

M1–M7 are merged to `main`. The app can read mail (sync + bodies), classify it into
People/Notifications/Newsletters streams (M6), and mutate it — read/unread, star, archive,
trash (M7). It cannot yet **send**. M8 adds compose & send. Re-sequenced roadmap after M8:
M9 settings/onboarding → M10 calendar.

**OAuth scope:** the existing `gmail.modify` scope already authorizes `users.messages.send`
(and drafts), per Google's scope definition ("Read, compose, and send messages…"). So **no
re-consent is required** — the current Keychain token should send. **This is verified live early
in E2E** (per the M7 lesson: confirm the live token actually permits the write before assuming).

---

## Scope

**In scope (lean v1):**
- A new **MIME/RFC822 builder** for a plain-text message.
- **New compose:** a modal with To, Cc (toggle), Subject, and a plain-text body; Send.
- **Reply:** wire the reading-pane Reply button — prefill recipient (original sender), `Re:`
  subject, a quoted original body, and thread the reply correctly (Gmail `threadId` +
  `In-Reply-To`/`References` headers).
- A clear **"Sent ✓"** confirmation, and **error handling that preserves the user's text**.

**Explicitly deferred (not in M8):**
- **Drafts** (`users.drafts.*`) and a Drafts view.
- **Outbox / retry queue** (offline send). Sending is immediate; failure surfaces in the modal.
- **Signature** (needs settings storage — M9 territory).
- **Attachments** (file picker + multipart MIME).
- **HTML / rich-text** composing (plain text only).
- **Reply-all** and **`Reply-To`** header handling (reply goes to the original `From`).
- **Display-name encoding** in To/Cc (plain `addr@host` only in v1).
- A **Sent view** (sent mail is not synced/shown locally; the "Sent ✓" confirmation stands in).

---

## Components & contracts

### Backend — `src/mime.rs` (NEW, pure)

```rust
pub struct OutgoingMessage {
    pub from: String,                  // the connected account email
    pub to: Vec<String>,               // bare addresses
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,                  // UTF-8 plain text
    pub in_reply_to: Option<String>,   // original Message-ID (reply only)
    pub references: Option<String>,    // References chain (reply only)
}

/// Build a complete RFC822 message: headers + blank line + base64 body, CRLF line endings.
pub fn build_rfc822(msg: &OutgoingMessage) -> String;
```

Headers emitted: `From`, `To`, `Cc` (omitted if empty), `Subject` (RFC2047 base64
encoded-word `=?UTF-8?B?…?=` when the subject contains non-ASCII, else literal),
`MIME-Version: 1.0`, `Content-Type: text/plain; charset="utf-8"`,
`Content-Transfer-Encoding: base64`, plus `In-Reply-To` and `References` when present.
`Date` and `Message-ID` are **omitted** — Gmail generates them. Body is base64-encoded
(standard base64, wrapped at 76 cols). Private helpers: `encode_subject` (RFC2047) and
`base64_body`. Fully unit-testable, no I/O, no clock.

### Backend — `src/gmail/mod.rs`

```rust
/// Send a raw RFC822 message. `thread_id` threads a reply into an existing conversation.
pub async fn send_message(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()>;

/// Fetch what a reply needs: the original's Message-ID + References (threading) and its
/// plain-text body (for quoting). One `format=full` fetch.
pub async fn get_reply_context(&self, id: &str) -> Result<ReplyContext>;
```

`send_message` base64url-encodes `raw_rfc822` (no padding, URL-safe — the same engine the
read path decodes with) and POSTs `{ "raw": "...", "threadId": "..."? }` to
`…/messages/send` via `post_json` (response ignored). `ReplyContext { message_id: String,
references: String, quoted_text: String }` — `quoted_text` is the decoded text/plain part
(reuse the existing `collect_body` walk); `message_id`/`references` are read from the payload
headers.

### Backend — `src/commands.rs` + `src/lib.rs`

```rust
#[tauri::command]
pub async fn send_email(
    to: Vec<String>, cc: Vec<String>, subject: String, body: String,
    in_reply_to: Option<String>, references: Option<String>, thread_id: Option<String>,
) -> Result<()>;

#[tauri::command]
pub async fn get_reply_context(id: String) -> Result<ReplyContext>;
```

`send_email`: `ensure_access_token` → build `GmailClient` → assemble `OutgoingMessage`
(`from` = the connected account email from the token) → `mime::build_rfc822` →
`client.send_message(&raw, thread_id.as_deref())`. **No DB access** (sent mail isn't cached).
`get_reply_context` delegates to the client method. Both registered in `lib.rs`.

### Frontend

- `src/lib/compose.ts` (NEW, pure): `parseAddress("Name <a@b>") -> "a@b"`; `replySubject(s)`
  (prefix `Re: ` unless already present, case-insensitive); `quoteBody(fromLabel, dateLabel,
  text)` → attribution line + `> `-prefixed lines; `parseRecipients(input) -> string[]`
  (split on comma/semicolon, trim, drop empties); `isPlausibleEmail(addr)` (non-empty, one `@`,
  a dot after it). No Vitest yet — kept pure for later testability.
- `src/lib/api.ts`: `sendEmail(payload)`, `getReplyContext(id)` wrappers + `ReplyContext` type.
- `src/components/ComposeModal.tsx` (NEW): centered modal over a dimmed backdrop; fields To,
  Cc (revealed by a toggle), Subject, and a plain-text body `<textarea>`; **Send** + **Cancel**;
  `Esc` closes; a "Sending…" disabled state; validation errors and send errors render inside
  the modal **without clearing the fields**. Props carry the initial values + reply metadata
  (`threadId`/`inReplyTo`/`references`) and an `onSent`/`onClose` pair.
- `src/App.tsx`: `compose` state (`null` | `{ mode, to, cc, subject, body, threadId?,
  inReplyTo?, references? }`); render `ComposeModal` when set; on success close it and set a
  transient **"Sent ✓"** status (reuse the Header `status` line); pass `onCompose` to the
  Header and `onReply` to the ReadingPane.
- `src/components/Header.tsx`: a **Compose** button (pencil icon) that opens an empty compose.
- `src/components/ReadingPane.tsx`: enable the **Reply** button → call `getReplyContext(msg.id)`
  → open the modal prefilled (To = `parseAddress(msg.from)`, Subject = `replySubject(...)`,
  body = `quoteBody(...)`, and `threadId`/`inReplyTo`/`references` from the context + preview).
- `src/styles/app.css`: modal overlay/card/field styles.

---

## Data flow

**New compose:** Header **Compose** → empty `ComposeModal` → user fills To/Subject/body →
**Send** → `send_email(to, cc, subject, body, None, None, None)` → builds RFC822 → Gmail
`messages.send` → success → modal closes, Header shows "Sent ✓".

**Reply:** open a message → **Reply** → `get_reply_context(id)` (Message-ID + References +
quoted text) → `ComposeModal` opens prefilled (recipient/subject/quote, threading carried) →
**Send** → `send_email(..., in_reply_to, references, thread_id)` → Gmail threads the reply →
"Sent ✓". The original message and the local cache are unchanged.

---

## Error handling

- **Send failure** (network/Gmail error): the modal stays open, all fields preserved, the error
  message renders in the modal. The user can retry without retyping.
- **Validation**: empty To, or a recipient failing `isPlausibleEmail`, blocks Send with an
  inline message.
- **401 / expired token**: handled by `ensure_access_token` before the send request.
- **Scope**: `gmail.modify` permits send; verified live in E2E. If a 403 ever appears, the
  message surfaces in the modal and the fix is re-consent (out of band).

---

## Testing strategy

- `mime.rs` table tests: ASCII subject (literal) vs Cyrillic subject (RFC2047 encoded-word);
  single vs multiple To and Cc; `Cc` omitted when empty; reply headers present
  (`In-Reply-To`/`References`); base64 body round-trips to the original UTF-8; CRLF line
  endings; `From` present; no `Date`/`Message-ID`.
- `tests/gmail_test.rs` (wiremock): `send_message` asserts `POST …/messages/send`, that the JSON
  body carries `raw` and (when threading) `threadId`, and that the base64url `raw` decodes back
  to the RFC822 we built; `get_reply_context` parses `Message-ID`/`References` and the text/plain
  body from a `format=full` fixture.
- `compose.ts` kept pure (address parsing, `Re:` normalization, quote building, validation) for
  unit testing once Vitest lands (none now — consistent with M4–M7).
- **Manual E2E** against live Gmail (project norm, run early to confirm the scope): send a new
  email to yourself and confirm it arrives; reply to a message and confirm it threads correctly
  in Gmail web; force a failure (offline) and confirm the modal keeps the typed content.

---

## Definition of done

- `mime::build_rfc822` produces a valid plain-text RFC822 message (unicode-safe subject), proven
  by table tests.
- `send_email` and `get_reply_context` commands implemented, registered, and reachable.
- Compose modal works for both new mail and replies; replies thread correctly in Gmail (verified
  live); a "Sent ✓" confirmation shows; send errors preserve the user's text.
- The Reply button is enabled and wired; a Compose button exists in the header.
- New Rust code carries `// 🦀` comments; a plain-English Rust recap accompanies each task.
- `cargo test` green (existing suite + new mime/gmail tests); `cargo clippy --all-targets -D
  warnings` clean; `npm run build` clean. (`cargo fmt` is **not** used in this repo — no config/CI,
  deliberate hand-style — so it is not a gate.)
- No SQLite schema migration introduced.

---

## Known limitations (carried as deferrals)

- Sent mail is not shown in-app (no Sent view) — the "Sent ✓" confirmation is the only local
  signal. Reconciles only if a Sent/folder view is built later.
- Reply targets the original `From` only (no reply-all, no `Reply-To` preference).
- Plain text only; HTML originals are quoted from their text/plain part.
- No drafts/outbox — an interrupted or failed compose is not persisted beyond the open modal.
