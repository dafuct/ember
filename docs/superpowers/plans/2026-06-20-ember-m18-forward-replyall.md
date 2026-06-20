# M18 Forward + Reply-all Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add reply-all and forward (with the original's attachments re-attached) to Ember's compose modal.

**Architecture:** One enriched `get_reply_context` fetch (extended to return the original's To/Cc/attachments) feeds both modes. Reply-all computes recipients in a pure frontend helper (self-excluded). Forward carries the original's attachments as refs that the M17 `send_email` re-fetches via `get_attachment` and merges into `build_multipart_rfc822` under the combined 25 MB cap. New modes are just different `ComposeInitial` values built in `App.tsx`; `getReplyContext` is `isTauri()`-gated so all three reply modes work in the browser maket.

**Tech Stack:** Rust (reqwest, serde, base64, Tauri 2; wiremock tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (every task):** the owner is learning Rust — all Rust gets concise `// 🦀` teaching comments on the *language* concept, and each task ends with a 2-3 sentence plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (hand-formatted repo).

**Frontend testing note:** this repo has **no TypeScript/React test harness** (consistent through M17). Frontend tasks are verified by `npm run build` (`tsc` + `vite build`) + a final maket screenshot — not unit tests. Backend tasks use TDD with `wiremock`.

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m18-forward-replyall-design.md`

---

## File structure

**Backend (`src-tauri/`):**
- `src/gmail/types.rs` — *modify*: `ReplyContext` gains `to`/`cc`/`attachments`.
- `src/gmail/mod.rs` — *modify*: `get_reply_context` extracts To/Cc + `collect_attachments`.
- `tests/gmail_test.rs` — *modify*: one new wiremock test for the enriched context.
- `src/commands.rs` — *modify*: add `ForwardedAttachmentRef`; extend `send_email` to fetch + merge forwarded attachments.

**Frontend (`src/`):**
- `src/lib/compose.ts` — *modify*: add `forwardSubject`, `replyAllRecipients`, `forwardBlock`.
- `src/lib/api.ts` — *modify*: `ReplyContext` + `ForwardedAttachmentRef` + `SendEmailPayload.forwarded_attachments`; `sendEmail` forwards it; gate `getReplyContext`.
- `src/lib/mock.ts` — *modify*: `mockReplyContext`.
- `src/components/ComposeModal.tsx` — *modify*: `ComposeInitial` (+`mode`/`forwardedAttachments`); forwarded-attachment chips; mode title; (interim stub in Task 4).
- `src/components/ReadingPane.tsx` — *modify*: Reply-all + Forward toolbar buttons.
- `src/App.tsx` — *modify*: `handleReplyAll` + `handleForward`; wire the new ReadingPane props.

---

## Task 1: Backend — enrich `get_reply_context` (To / Cc / attachments)

**Files:**
- Modify: `src-tauri/src/gmail/types.rs`
- Modify: `src-tauri/src/gmail/mod.rs`
- Test: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Write the failing test**

Add to the end of `src-tauri/tests/gmail_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn get_reply_context_extracts_to_cc_and_attachments() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/r3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "r3",
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    {"name": "Message-ID", "value": "<orig@mail>"},
                    {"name": "To", "value": "alice@x.com, Bob <bob@y.com>"},
                    {"name": "Cc", "value": "carol@z.com"}
                ],
                "parts": [
                    {"mimeType": "text/plain", "body": {"data": b64url("hi")}},
                    {"mimeType": "application/pdf", "filename": "spec.pdf", "body": {"attachmentId": "aa1", "size": 99}}
                ]
            }
        })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let rc = client.get_reply_context("r3").await.unwrap();
    assert_eq!(rc.to, "alice@x.com, Bob <bob@y.com>");
    assert_eq!(rc.cc, "carol@z.com");
    assert_eq!(rc.attachments.len(), 1);
    assert_eq!(rc.attachments[0].filename, "spec.pdf");
    assert_eq!(rc.attachments[0].attachment_id, "aa1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test gmail_test get_reply_context_extracts_to_cc_and_attachments`
Expected: FAIL to compile — `ReplyContext` has no fields `to`/`cc`/`attachments`.

- [ ] **Step 3: Extend the `ReplyContext` type**

In `src-tauri/src/gmail/types.rs`, replace the `ReplyContext` struct with:

```rust
/// What a reply needs from the original message: threading headers + the quoted text,
/// plus (for reply-all / forward) the original recipients and attachment list.
#[derive(Debug, Serialize)]
pub struct ReplyContext {
    pub message_id: String,
    pub references: String,
    pub quoted_text: String,
    // 🦀 Raw header values (may hold several comma-separated addresses); "" when absent.
    pub to: String,
    pub cc: String,
    // 🦀 The original's attachments — reuses the M17 `AttachmentMeta` (same module).
    pub attachments: Vec<AttachmentMeta>,
}
```

(If the existing `ReplyContext` derives differ, keep whatever derives it already had and just add the three fields — it must remain `Serialize`. `AttachmentMeta` is defined in this same file, so no import is needed.)

- [ ] **Step 4: Extract To/Cc + attachments in `get_reply_context`**

In `src-tauri/src/gmail/mod.rs`, update `get_reply_context` to populate the new fields (the `header` closure and `collect_attachments` already exist):

```rust
        let message_id = header("Message-ID");
        let references = header("References");
        // 🦀 New: the original recipients (for reply-all) — same case-insensitive closure.
        let to = header("To");
        let cc = header("Cc");
        // 🦀 Reuse the existing recursive MIME walk to pull the text/plain part.
        let mut html = None;
        let mut text = None;
        collect_body(&full.payload, &mut html, &mut text);
        // 🦀 New: the original's attachments (for forward) — reuses the M17 walk on the same payload.
        let mut attachments = Vec::new();
        collect_attachments(&full.payload, &mut attachments);
        Ok(ReplyContext {
            message_id,
            references,
            quoted_text: text.unwrap_or_default(),
            to,
            cc,
            attachments,
        })
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --test gmail_test get_reply_context_extracts_to_cc_and_attachments` → PASS.
Then `cargo test --test gmail_test` — the two existing `get_reply_context_*` tests still PASS (they don't assert the new fields).

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo clippy --all-targets 2>&1 | tail -3
cd .. && git add src-tauri/src/gmail/types.rs src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(m18): get_reply_context returns original To/Cc + attachments"
```

**Rust recap:** how reusing the existing `header` closure + the M17 `collect_attachments` on the already-fetched `format=full` payload adds reply-all/forward context with zero extra HTTP round-trips.

---

## Task 2: Backend — `send_email` re-attaches forwarded attachments

**Files:**
- Modify: `src-tauri/src/commands.rs`

(Command glue over the already-tested `get_attachment` + `build_multipart_rfc822`; verified by `cargo build` + later E2E.)

- [ ] **Step 1: Add the `ForwardedAttachmentRef` type**

In `src-tauri/src/commands.rs`, add this near `send_email` (e.g. just above it):

```rust
/// A reference to an attachment on an existing message, for forwarding. The bytes are
/// NOT carried from JS — the backend re-fetches them via `get_attachment` at send time.
// 🦀 `Deserialize` so Tauri can build it from the JS object. Field names are snake_case,
//    so the JS side must pass `{ message_id, attachment_id, filename, mime_type }`.
#[derive(serde::Deserialize)]
pub struct ForwardedAttachmentRef {
    pub message_id: String,
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
}
```

- [ ] **Step 2: Extend `send_email`**

In `src-tauri/src/commands.rs`, replace the whole `send_email` command with this version (adds the `forwarded_attachments` param + a second fetch-and-merge loop; the 25 MB cap now covers the combined total):

```rust
/// Send a plain-text message, optionally with file attachments and/or forwarded attachments.
/// No attachments of either kind → the original single-part path; otherwise multipart/mixed.
// 🦀 `#[allow(clippy::too_many_arguments)]` — these flat args mirror the JS `invoke` payload;
//    a shared `OutgoingFields` struct is a noted follow-up (kept out of M18 to stay focused).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_email(
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    references: Option<String>,
    thread_id: Option<String>,
    attachment_paths: Vec<String>,
    forwarded_attachments: Vec<ForwardedAttachmentRef>,
) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let msg = crate::mime::OutgoingMessage {
        from: stored.email,
        to,
        cc,
        subject,
        body,
        in_reply_to,
        references,
    };
    // 🦀 Neither kind of attachment → the unchanged single-part path.
    if attachment_paths.is_empty() && forwarded_attachments.is_empty() {
        let raw = crate::mime::build_rfc822(&msg);
        return client.send_message(&raw, thread_id.as_deref()).await;
    }
    let mut attachments = Vec::new();
    let mut total = 0usize;
    // 🦀 (a) Files the user picked from disk (M17 path).
    for path in &attachment_paths {
        let bytes = std::fs::read(path)
            .map_err(|e| AppError::Other(format!("could not read attachment {path}: {e}")))?;
        total = total.saturating_add(bytes.len());
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let mime_type = crate::mime::mime_for_ext(&filename).to_string();
        attachments.push(crate::mime::OutgoingAttachment { filename, mime_type, bytes });
    }
    // 🦀 (b) Attachments forwarded from an existing message — re-fetched from Gmail by id.
    for fa in &forwarded_attachments {
        let bytes = client.get_attachment(&fa.message_id, &fa.attachment_id).await?;
        total = total.saturating_add(bytes.len());
        attachments.push(crate::mime::OutgoingAttachment {
            filename: fa.filename.clone(),
            mime_type: fa.mime_type.clone(),
            bytes,
        });
    }
    // 🦀 Cap the COMBINED total before base64 inflation pushes us past the send ceiling.
    if total > crate::mime::MAX_ATTACHMENT_BYTES {
        return Err(AppError::Other(format!(
            "attachments total {total} bytes exceed the {} MB limit",
            crate::mime::MAX_ATTACHMENT_BYTES / (1024 * 1024)
        )));
    }
    // 🦀 Unique-enough multipart boundary from the wall clock; mime.rs itself stays clock-free.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let boundary = format!("ember_boundary_{nanos}");
    let raw = crate::mime::build_multipart_rfc822(&msg, &attachments, &boundary);
    client.send_message(&raw, thread_id.as_deref()).await
}
```

- [ ] **Step 3: Build to verify**

Run: `cd src-tauri && cargo build 2>&1 | tail -6`
Expected: compiles clean. Then `cargo test 2>&1 | tail -5` + `cargo clippy --all-targets 2>&1 | tail -3` — green/clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(m18): send_email re-fetches + merges forwarded attachments"
```

**Rust recap:** how the second loop reuses the already-async `client.get_attachment(...).await?` to turn each forwarded ref into bytes, and why accumulating `total` across both loops keeps the one cap check honest.

---

## Task 3: Frontend — pure reply-all / forward helpers

**Files:**
- Modify: `src/lib/compose.ts`

(Pure functions. No TS test harness in this repo — verified by `npm run build` + the maket. The logic is specified precisely below; implement it exactly.)

- [ ] **Step 1: Add the helpers**

Append to `src/lib/compose.ts` (it already exports `parseAddress` and `parseRecipients`, which these reuse):

```ts
// Prefix "Fwd: " unless already present (case-insensitive). Mirrors replySubject.
export function forwardSubject(subject: string): string {
  return /^fwd:/i.test(subject.trim()) ? subject : `Fwd: ${subject}`;
}

// Compute reply-all recipients from the original message.
// To = the original sender (bare address). Cc = the original To + Cc addresses, deduped
// case-insensitively, with YOUR address and the sender removed. Display names are dropped
// (bare addresses only) — a deliberate lean-v1 simplification.
export function replyAllRecipients(
  from: string,
  to: string,
  cc: string,
  self: string,
): { to: string; cc: string } {
  const selfAddr = parseAddress(self).toLowerCase();
  const fromAddr = parseAddress(from).toLowerCase();
  const seen = new Set<string>([selfAddr, fromAddr].filter((a) => a.length > 0));
  const ccOut: string[] = [];
  for (const raw of [...parseRecipients(to), ...parseRecipients(cc)]) {
    const bare = parseAddress(raw);
    const key = bare.toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    ccOut.push(bare);
  }
  return { to: parseAddress(from), cc: ccOut.join(", ") };
}

// The plain-text forwarded-message header block (a blank line trails it, before the body).
export function forwardBlock(
  from: string,
  dateLabel: string,
  subject: string,
  to: string,
): string {
  return [
    "---------- Forwarded message ---------",
    `From: ${from}`,
    `Date: ${dateLabel}`,
    `Subject: ${subject}`,
    `To: ${to}`,
    "",
    "",
  ].join("\n");
}
```

- [ ] **Step 2: Build to verify**

Run: `npm run build 2>&1 | tail -6`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 3: Commit**

```bash
git add src/lib/compose.ts
git commit -m "feat(m18): pure forwardSubject/replyAllRecipients/forwardBlock helpers"
```

---

## Task 4: Frontend — api types, send wiring, gated context + mock

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/lib/mock.ts`
- Modify: `src/components/ComposeModal.tsx` (one-line stub only)

(Adding a required `forwarded_attachments` field to `SendEmailPayload` breaks `ComposeModal.fields()`; a one-line stub here keeps the build green. Task 5 wires the real state.)

- [ ] **Step 1: Add the mock context**

In `src/lib/mock.ts`, append (it already imports `MessageBody`/`Attachment`; ensure `ReplyContext` is added to the type import from `./api`):

```ts
/** Browser-maket reply/forward context: gives m1 a Cc (for reply-all) + attachments (for forward). */
export function mockReplyContext(id: string): ReplyContext {
  const attachments: Attachment[] =
    id === "m1"
      ? [
          { filename: "Q3-roadmap.pdf", mime_type: "application/pdf", size: 248000, attachment_id: "att1" },
          { filename: "budget.xlsx", mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", size: 18500, attachment_id: "att2" },
        ]
      : [];
  return {
    message_id: `<${id}@mock>`,
    references: "",
    quoted_text: "Here's the draft for review…",
    to: "you@example.com, Dana <dana@corp.io>",
    cc: id === "m1" ? "Sam <sam@team.io>" : "",
    attachments,
  };
}
```

Update the `mock.ts` type import line to include `ReplyContext`:

```ts
import type { MessagePreview, SyncSummary, DraftContent, Label, MessageBody, Attachment, ReplyContext } from "./api";
```

- [ ] **Step 2: Extend `api.ts` types + send + gate context**

In `src/lib/api.ts`:

(a) Replace the `ReplyContext` interface:

```ts
export interface ReplyContext {
  message_id: string;
  references: string;
  quoted_text: string;
  to: string;
  cc: string;
  attachments: Attachment[];
}
```

(b) Add the forwarded-ref interface (near `SendEmailPayload`):

```ts
export interface ForwardedAttachmentRef {
  message_id: string;
  attachment_id: string;
  filename: string;
  mime_type: string;
}
```

(c) Add `forwarded_attachments` to `SendEmailPayload` and forward it in `sendEmail`:

```ts
export interface SendEmailPayload {
  to: string[];
  cc: string[];
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
  attachment_paths: string[];
  forwarded_attachments: ForwardedAttachmentRef[];
}

export const sendEmail = (p: SendEmailPayload): Promise<void> =>
  invoke<void>("send_email", {
    to: p.to,
    cc: p.cc,
    subject: p.subject,
    body: p.body,
    inReplyTo: p.in_reply_to,
    references: p.references,
    threadId: p.thread_id,
    attachmentPaths: p.attachment_paths,
    forwardedAttachments: p.forwarded_attachments,
  });
```

(d) Gate `getReplyContext` (add `mockReplyContext` to the existing `from "./mock"` import):

```ts
export const getReplyContext = (id: string): Promise<ReplyContext> =>
  isTauri()
    ? invoke<ReplyContext>("get_reply_context", { id })
    : Promise.resolve(mockReplyContext(id));
```

- [ ] **Step 3: Add the stub to keep `ComposeModal` compiling**

In `src/components/ComposeModal.tsx`, in the `fields()` return object, add a stub line after `attachment_paths: attachPaths,`:

```ts
      forwarded_attachments: [], // wired to forwarded-attachment chips in the compose-modes task
```

- [ ] **Step 4: Build to verify**

Run: `npm run build 2>&1 | tail -8`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/lib/mock.ts src/components/ComposeModal.tsx
git commit -m "feat(m18): api ReplyContext/forwarded types + gated getReplyContext + mock"
```

---

## Task 5: Frontend — ComposeModal forward modes + forwarded chips

**Files:**
- Modify: `src/components/ComposeModal.tsx`

- [ ] **Step 1: Extend `ComposeInitial` + import the ref type**

In `src/components/ComposeModal.tsx`, add `ForwardedAttachmentRef` to the api import:

```tsx
import { sendEmail, saveDraft, sendDraft, deleteDraft, type SendEmailPayload, type ForwardedAttachmentRef } from "../lib/api";
```

Extend the `ComposeInitial` interface (add two optional fields):

```tsx
export interface ComposeInitial {
  to: string; // comma-separated text (prefilled for reply)
  cc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  threadId: string | null;
  draftId?: string | null; // set when editing an existing Gmail draft
  mode?: "new" | "reply" | "replyAll" | "forward" | "draft"; // drives the title
  forwardedAttachments?: ForwardedAttachmentRef[]; // original's attachments, for forward
}
```

- [ ] **Step 2: State, dirty, title, fields()**

Add state next to `attachPaths`:

```tsx
  const [forwardedAtts, setForwardedAtts] = useState<ForwardedAttachmentRef[]>(initial.forwardedAttachments ?? []);
```

Extend `dirty` to include forwarded attachments:

```tsx
  const dirty =
    to.trim() !== "" || cc.trim() !== "" || subject.trim() !== "" || body !== initial.body ||
    attachPaths.length > 0 || forwardedAtts.length > 0;
```

Replace the `title` line so the mode drives it:

```tsx
  const title =
    initial.mode === "forward" ? "Forward"
    : initial.mode === "replyAll" ? "Reply all"
    : draftId ? "Draft"
    : initial.threadId ? "Reply"
    : "New message";
```

Replace the stub in `fields()` (`forwarded_attachments: [],`) with the real state:

```tsx
      forwarded_attachments: forwardedAtts,
```

- [ ] **Step 3: A remove handler for forwarded chips**

Add next to `removeAttach`:

```tsx
  function removeForwarded(attachmentId: string) {
    setForwardedAtts((p) => p.filter((x) => x.attachment_id !== attachmentId));
  }
```

- [ ] **Step 4: Render the forwarded chips**

In the JSX, immediately after the existing `{attachPaths.length > 0 && ( ... )}` file-chips block (and before `{error && ...}`), add a forwarded-chips block:

```tsx
        {forwardedAtts.length > 0 && (
          <div className="attach-file-chips">
            {forwardedAtts.map((fa) => (
              <span key={fa.attachment_id} className="attach-file-chip">
                <Paperclip size={12} />
                {fa.filename}
                <button className="attach-remove" aria-label={`Remove ${fa.filename}`} onClick={() => removeForwarded(fa.attachment_id)} type="button">
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
```

- [ ] **Step 5: Build to verify**

Run: `npm run build 2>&1 | tail -6`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 6: Commit**

```bash
git add src/components/ComposeModal.tsx
git commit -m "feat(m18): ComposeModal forward/reply-all modes + forwarded-attachment chips"
```

---

## Task 6: Frontend — Reply-all + Forward buttons and App handlers

**Files:**
- Modify: `src/components/ReadingPane.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: ReadingPane — props + toolbar buttons**

In `src/components/ReadingPane.tsx`:

(a) Add `ReplyAll` and `Forward` to the lucide import:

```tsx
import { Mail, Archive, Trash2, Star, CornerUpLeft, RotateCcw, Tag, Paperclip, ReplyAll, Forward } from "lucide-react";
```

(b) Add the two props to the destructured params and the prop types:

```tsx
  onReplyAll,
  onForward,
```

```tsx
  onReplyAll: (m: MessagePreview) => void;
  onForward: (m: MessagePreview) => void;
```

(c) In the toolbar, immediately after the existing Reply button, add:

```tsx
        <button className="icon-btn" aria-label="Reply all" onClick={() => onReplyAll(msg)}>
          <ReplyAll size={15} />
        </button>
        <button className="icon-btn" aria-label="Forward" onClick={() => onForward(msg)}>
          <Forward size={15} />
        </button>
```

- [ ] **Step 2: App — `handleReplyAll` + `handleForward`**

In `src/App.tsx`, add `forwardSubject, replyAllRecipients, forwardBlock` to the `./lib/compose` import:

```tsx
import { appendSignature, parseAddress, replySubject, quoteBody, forwardSubject, replyAllRecipients, forwardBlock } from "./lib/compose";
```

Add these two handlers right after `handleReply` (mirroring its shape; `account` is the connected-address state, already present):

```tsx
  async function handleReplyAll(m: MessagePreview) {
    setError(null);
    try {
      const ctx = await getReplyContext(m.id);
      const dateLabel = m.internal_date ? new Date(m.internal_date).toLocaleString() : m.date;
      const r = replyAllRecipients(m.from, ctx.to, ctx.cc, account ?? "");
      setCompose({
        to: r.to,
        cc: r.cc,
        subject: replySubject(m.subject),
        body: appendSignature(quoteBody(m.from, dateLabel, ctx.quoted_text), settings.signature),
        inReplyTo: ctx.message_id || null,
        references: ctx.references || ctx.message_id || null,
        threadId: m.thread_id || null,
        draftId: null,
        mode: "replyAll",
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleForward(m: MessagePreview) {
    setError(null);
    try {
      const ctx = await getReplyContext(m.id);
      const dateLabel = m.internal_date ? new Date(m.internal_date).toLocaleString() : m.date;
      setCompose({
        to: "",
        cc: "",
        subject: forwardSubject(m.subject),
        body: appendSignature(
          forwardBlock(m.from, dateLabel, m.subject, ctx.to) + ctx.quoted_text,
          settings.signature,
        ),
        inReplyTo: null,
        references: null,
        threadId: null, // forward starts a fresh conversation
        draftId: null,
        mode: "forward",
        forwardedAttachments: ctx.attachments.map((a) => ({
          message_id: m.id,
          attachment_id: a.attachment_id,
          filename: a.filename,
          mime_type: a.mime_type,
        })),
      });
    } catch (e) {
      setError(String(e));
    }
  }
```

- [ ] **Step 3: Wire the new ReadingPane props**

In `src/App.tsx`, in the `<ReadingPane ... />` element (right after `onReply={handleReply}`), add:

```tsx
                onReplyAll={handleReplyAll}
                onForward={handleForward}
```

- [ ] **Step 4: Build to verify**

Run: `npm run build 2>&1 | tail -8`
Expected: `tsc` + `vite build` clean. (If `tsc` flags any other `<ReadingPane>` usage missing the new required props, there is only one — the block shown above; add the two props there.)

- [ ] **Step 5: Commit**

```bash
git add src/components/ReadingPane.tsx src/App.tsx
git commit -m "feat(m18): Reply-all + Forward buttons + App handlers"
```

---

## Task 7: Full verification + maket screenshots

**Files:** none (verification only)

- [ ] **Step 1: Backend suite + lint**

Run: `cd src-tauri && cargo test 2>&1 | tail -8`
Expected: all pass (the prior count + 1 new gmail wiremock test).
Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -4` → clean.

- [ ] **Step 2: Frontend build**

Run: `cd .. && npm run build 2>&1 | tail -5` → `tsc` + `vite build` clean.

- [ ] **Step 3: Maket verification**

Run `npm run dev`, open the local URL, switch to **Mail**, open the **Maya / "Q3 roadmap" (m1)** message, and verify + screenshot:
1. **Reply-all** (the `ReplyAll` toolbar button) → the compose modal opens titled **"Reply all"** with To = `maya@studio.co` and a **prefilled Cc** (from the mock To/Cc, minus you/sender).
2. **Forward** (the `Forward` toolbar button) → the modal opens titled **"Forward"**, subject `Fwd: Q3 roadmap`, the body showing the `---------- Forwarded message ---------` block, and two **forwarded-attachment chips** (`Q3-roadmap.pdf`, `budget.xlsx`) each with a removable ×; removing one drops the chip.

- [ ] **Step 4: Confirm clean tree**

Run: `git status -s`
Expected: empty (all committed). If a reviewer left changes, discard them.

- [ ] **Step 5: Final summary**

Report: test count delta, clippy/build status, maket screenshots, and the **owner-pending live Gmail E2E** (a real reply-all reaching all recipients; a real forward arriving with the original attachments intact). The wiki `[[ember]]` + `wiki/log.md` update happens at merge time (controller).

---

## Self-review notes (for the controller)

- **Spec coverage:** enriched context (T1) · forwarded re-attach in send (T2) · pure helpers (T3) · api/types/gating/mock + stub (T4) · ComposeModal modes + chips (T5) · ReadingPane buttons + App handlers (T6) · verification (T7). Deferred items (HTML forward, .eml, recipient chips, alias self-exclusion, OutgoingFields refactor) intentionally absent.
- **Type consistency:** Rust `ForwardedAttachmentRef {message_id, attachment_id, filename, mime_type}` (Deserialize, snake_case) ↔ TS `ForwardedAttachmentRef` (snake_case) ↔ the App-built object (snake_case keys) ↔ `sendEmail` forwards them under the camelCase arg `forwardedAttachments` → Rust param `forwarded_attachments`. `ReplyContext` gains `to`/`cc`/`attachments` identically in Rust + TS. `ComposeInitial.mode`/`forwardedAttachments` set by App, read by ComposeModal.
- **Cross-task green:** T4's required `forwarded_attachments` field is stubbed in `ComposeModal.fields()` in the same task (T4), so every task builds clean; T5 replaces the stub with real state.
- **No new OAuth scope, no DB migration, no new dependency.**
