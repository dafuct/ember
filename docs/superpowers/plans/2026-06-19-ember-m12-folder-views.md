# Ember M12 — Folder & Sent views (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Browse Sent / Starred / Archive / Trash / Spam via a left folder rail, with Trash restore + permanent delete — reusing M11's `list_message_ids` helper and list-aware action handlers.

**Architecture:** A left `FolderRail` drives a `folder` state. M11's "active list" extends to 3 sources: search results → cached inbox (`folder==="inbox"`) → live-fetched `folderResults`. Each folder maps to a `(label, query, includeSpamTrash)` triple fed to `list_message_ids`; a DB-free `fetch_folder` command hydrates previews. Trash gains `untrash` + a permanent `DELETE`. Folders are live-fetched, DB-free, no migration.

**Tech Stack:** Rust (reqwest, serde, futures, Tauri 2; wiremock), React 19 + TypeScript + Vite, lucide-react.

**Design spec:** `docs/superpowers/specs/2026-06-19-ember-m12-folder-views-design.md`.

**Conventions for every task:**
- New Rust code carries `// 🦀` teaching comments on the *language* concept; give a one-paragraph plain-English Rust recap after each Rust task.
- Every commit ends with the trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` (second `-m`).
- Branch is `m12-folders`. Gates: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `npm run build`. (`cargo fmt` not used.)
- No JS test runner (consistent with M10–M11); frontend tasks gate on `npm run build` + the maket.

---

## File structure
**Backend:** `src-tauri/src/gmail/mod.rs` (extend `list_message_ids`; add `untrash_message`, `delete_message_forever`, `delete_no_body`), `tests/gmail_test.rs`, `src-tauri/src/commands.rs` (`fetch_folder`/`restore_message`/`delete_message_forever`), `src-tauri/src/lib.rs` (register).
**Frontend:** `src/lib/folders.ts` (new), `src/lib/api.ts` (+wrappers, `to_addr`), `src/lib/mock.ts` (`mockFolder`), `src/components/FolderRail.tsx` (new), `src/components/MessageList.tsx` + `MessageItem.tsx` (`showRecipient`), `src/components/ReadingPane.tsx` (Trash actions), `src/components/Header.tsx` (`inFolder`), `src/App.tsx` (3-way active list + wiring), `src/styles/app.css` (rail).

---

## Task 1: Backend — `include_spam_trash` flag on `list_message_ids` (TDD)

**Files:** `src-tauri/src/gmail/mod.rs`, `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Append failing tests to `src-tauri/tests/gmail_test.rs`**

```rust
#[tokio::test(flavor = "multi_thread")]
async fn list_message_ids_includes_spam_trash_flag_for_trash() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("labelIds", "TRASH"))
        .and(query_param("includeSpamTrash", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "messages": [{ "id": "t9" }] })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.list_message_ids(Some("TRASH"), "", 50, true).await.unwrap();
    assert_eq!(ids, vec!["t9".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_inbox_message_ids_paged_omits_spam_trash_flag() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("labelIds", "INBOX"))
        .and(query_param_is_missing("includeSpamTrash"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "messages": [{ "id": "i1" }] })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.list_inbox_message_ids_paged("newer_than:30d", 50).await.unwrap();
    assert_eq!(ids, vec!["i1".to_string()]);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --test gmail_test list_message_ids_includes_spam_trash_flag_for_trash`
Expected: FAIL — `list_message_ids` is private / wrong arity.

- [ ] **Step 3: Edit `src-tauri/src/gmail/mod.rs`** — make `list_message_ids` `pub`, add the `include_spam_trash` param + the query bit, and update the two delegating callers.

Replace the `async fn list_message_ids(...) -> Result<Vec<String>> { ... }` signature line + the URL-building region. The method becomes:

```rust
    /// Shared paging loop for `messages.list`. `label = Some("INBOX")` restricts to a label; `None`
    /// searches all mail. `include_spam_trash` adds `&includeSpamTrash=true` — Gmail omits Trash/Spam
    /// messages without it. Follows `nextPageToken` up to `max_total` ids.
    // 🦀 Now `pub` so the folder command can call it directly with a label + the spam/trash flag.
    pub async fn list_message_ids(
        &self,
        label: Option<&str>,
        query: &str,
        max_total: u32,
        include_spam_trash: bool,
    ) -> Result<Vec<String>> {
        let encoded_q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let mut ids = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/messages?maxResults=100&q={}",
                self.base_url, encoded_q
            );
            if let Some(l) = label {
                url.push_str(&format!("&labelIds={l}"));
            }
            // 🦀 Only add the flag when asked — keeps the inbox/search requests byte-identical.
            if include_spam_trash {
                url.push_str("&includeSpamTrash=true");
            }
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={token}"));
            }
            let list: MessageList = self.get_json(&url).await?;
            for m in list.messages {
                ids.push(m.id);
                if ids.len() >= max_total as usize {
                    return Ok(ids);
                }
            }
            match list.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }
        Ok(ids)
    }
```

And update the two delegating callers to pass `false`:

```rust
    pub async fn list_inbox_message_ids_paged(
        &self,
        query: &str,
        max_total: u32,
    ) -> Result<Vec<String>> {
        self.list_message_ids(Some("INBOX"), query, max_total, false).await
    }

    pub async fn search_message_ids(&self, query: &str, max_total: u32) -> Result<Vec<String>> {
        self.list_message_ids(None, query, max_total, false).await
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --test gmail_test`
Expected: PASS (new + existing, incl. the M11 search/inbox tests).

- [ ] **Step 5: Commit + Rust recap**

```bash
cd /Users/makar/dev/ownmail
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): include_spam_trash flag on list_message_ids (for Trash/Spam folders)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: adding a `bool` parameter and guarding a `push_str` with `if include_spam_trash` keeps existing callers byte-identical (they pass `false`) while the new folder path opts in — a small, backwards-compatible extension rather than a new method.

---

## Task 2: Backend — `untrash_message` + `delete_message_forever` (TDD)

**Files:** `src-tauri/src/gmail/mod.rs`, `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Append failing tests to `src-tauri/tests/gmail_test.rs`**

```rust
#[tokio::test(flavor = "multi_thread")]
async fn untrash_message_posts_to_untrash() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/messages/m1/untrash"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "m1" })))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client.untrash_message("m1").await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_message_forever_issues_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = GmailClient::with_base_url("tok".into(), server.uri());
    client.delete_message_forever("m1").await.unwrap();
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test --test gmail_test untrash_message_posts_to_untrash`
Expected: FAIL — `untrash_message` not found.

- [ ] **Step 3: Add the methods to `src-tauri/src/gmail/mod.rs`** — a `delete_no_body` helper next to `post_no_body`, and the two public methods next to the existing `trash_message`.

Add after the `post_no_body` method:

```rust
    // 🦀 DELETE with no body — Gmail's permanent-delete endpoint. Like post_no_body but the verb is
    //    DELETE. We only need success; there's no response body to read.
    async fn delete_no_body(&self, url: &str) -> Result<()> {
        self.http
            .delete(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
```

Add (next to `trash_message`):

```rust
    /// Restore a trashed message (removes the TRASH label). Gmail `messages/{id}/untrash`.
    pub async fn untrash_message(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}/untrash", self.base_url, id);
        self.post_no_body(&url).await
    }

    /// PERMANENTLY delete a message (bypasses Trash, irreversible). Gmail `DELETE messages/{id}`.
    pub async fn delete_message_forever(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}", self.base_url, id);
        self.delete_no_body(&url).await
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test --test gmail_test`
Expected: PASS (the two new + all existing).

- [ ] **Step 5: Commit + Rust recap**

```bash
cd /Users/makar/dev/ownmail
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): untrash_message + permanent delete_message_forever" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: `reqwest::Client` has a method per HTTP verb (`.get`/`.post`/`.delete`); `delete_no_body` mirrors `post_no_body` exactly but calls `.delete`, so the bearer-auth + `error_for_status` plumbing is reused with one word changed.

---

## Task 3: Backend — `fetch_folder` / `restore_message` / `delete_message_forever` commands

**Files:** `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`

Network-orchestration commands; covered by the client tests (Tasks 1–2) + manual E2E. No new automated test.

- [ ] **Step 1: Add the three commands to `src-tauri/src/commands.rs`** (after `search_messages`)

```rust
/// Fetch one mailbox's previews (live, DB-free). Maps the folder key to a Gmail label/query +
/// includeSpamTrash flag, lists ids, hydrates, recency-sorts. Folder results are NOT classified
/// (category dots are an inbox concept).
#[tauri::command]
pub async fn fetch_folder(folder: String, max: u32) -> Result<Vec<MessagePreview>> {
    let max = max.clamp(1, SEARCH_MAX);
    // 🦀 A match returning a tuple: each folder picks its (label, query, includeSpamTrash). The
    //    annotation pins the element types so `None` is inferred as `Option<&str>`, not `Option<_>`.
    let (label, query, include_spam_trash): (Option<&str>, &str, bool) = match folder.as_str() {
        "sent" => (Some("SENT"), "", false),
        "starred" => (Some("STARRED"), "", false),
        "trash" => (Some("TRASH"), "", true),
        "spam" => (Some("SPAM"), "", true),
        "archive" => (None, "-in:inbox -in:sent -in:trash -in:spam", false),
        other => return Err(AppError::Other(format!("unknown folder: {other}"))),
    };
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.list_message_ids(label, query, max, include_spam_trash).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    previews.sort_by_key(|p| std::cmp::Reverse(p.internal_date));
    Ok(previews)
}

/// Restore a trashed message (untrash). DB-free — the Trash folder isn't cached.
#[tauri::command]
pub async fn restore_message(id: String) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.untrash_message(&id).await
}

/// Permanently delete a message (irreversible) and drop it from the local cache if present.
#[tauri::command]
pub async fn delete_message_forever(id: String, state: tauri::State<'_, Db>) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    client.delete_message_forever(&id).await?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::delete_messages(&conn, std::slice::from_ref(&id))?;
    Ok(())
}
```

- [ ] **Step 2: Register in `src-tauri/src/lib.rs`** — add after `commands::search_messages,`:

```rust
            commands::fetch_folder,
            commands::restore_message,
            commands::delete_message_forever,
```

- [ ] **Step 3: Build + test + clippy**

Run: `cd src-tauri && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all PASS, clippy clean. (Fix any clippy lint in the new code minimally; if it flags pre-existing code, STOP and report.)

- [ ] **Step 4: Commit + Rust recap**

```bash
cd /Users/makar/dev/ownmail
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): fetch_folder + restore_message + delete_message_forever" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: a `match` that returns a tuple is a clean way to turn a string key into several related values at once; the `(Option<&str>, &str, bool)` type annotation guides inference so the `None` arm knows it's an `Option<&str>`.

---

## Task 4: Frontend — `folders.ts` + api wrappers + mock

**Files:** `src/lib/folders.ts` (new), `src/lib/api.ts`, `src/lib/mock.ts`

- [ ] **Step 1: Create `src/lib/folders.ts`**

```ts
// src/lib/folders.ts — the mailbox folders shown in the left rail (M12).
export type Folder = "inbox" | "sent" | "starred" | "archive" | "trash" | "spam";

export interface FolderDef {
  key: Folder;
  label: string;
}

export const FOLDERS: FolderDef[] = [
  { key: "inbox", label: "Inbox" },
  { key: "sent", label: "Sent" },
  { key: "starred", label: "Starred" },
  { key: "archive", label: "Archive" },
  { key: "trash", label: "Trash" },
  { key: "spam", label: "Spam" },
];
```

- [ ] **Step 2: Edit `src/lib/api.ts`** — add `to_addr` to `MessagePreview`, import `mockFolder`, add three wrappers.

Add `to_addr: string;` to the `MessagePreview` interface (e.g. after `label_ids: string[];`):

```ts
  /** Recipient (To header). Shown instead of `from` in the Sent folder. */
  to_addr: string;
```

Add `mockFolder` to the mock import:

```ts
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek, mockSearch, mockFolder } from "./mock";
```

Add the wrappers (near `searchMessages`):

```ts
export const fetchFolder = (folder: string, max = 50): Promise<MessagePreview[]> =>
  isTauri()
    ? invoke<MessagePreview[]>("fetch_folder", { folder, max })
    : Promise.resolve(mockFolder(folder));
export const restoreMessage = (id: string): Promise<void> =>
  invoke<void>("restore_message", { id });
export const deleteMessageForever = (id: string): Promise<void> =>
  invoke<void>("delete_message_forever", { id });
```

- [ ] **Step 3: Edit `src/lib/mock.ts`** — every existing `MOCK_MESSAGES` entry now needs `to_addr` (the interface field is required), and add `mockFolder`.

First add `to_addr` to each object in `MOCK_MESSAGES` (set `to_addr: "you@example.com"` on each). Then append:

```ts
/** Browser-maket folder contents: a small per-folder set so the rail is demoable offline. */
export function mockFolder(folder: string): MessagePreview[] {
  const base = (id: string, from: string, to_addr: string, subject: string, snippet: string): MessagePreview => ({
    id, thread_id: id, from, subject, snippet, to_addr,
    date: "Wed, 18 Jun 2026 09:00:00 -0700", internal_date: 1750000000000, category: "", label_ids: [],
  });
  switch (folder) {
    case "sent":
      return [
        base("s1", "you@example.com", "Maya <maya@studio.co>", "Re: Q3 roadmap", "Sounds good — shipping Friday."),
        base("s2", "you@example.com", "Sam, Dana", "Lunch Thursday?", "Works for me."),
      ];
    case "starred":
      return [base("st1", "Dana <dana@corp.io>", "you@example.com", "Offsite agenda", "Pinned for later.")];
    case "archive":
      return [base("a1", "Newsletter <news@weekly.dev>", "you@example.com", "Weekly digest", "Archived reading.")];
    case "trash":
      return [base("d1", "Spammer <promo@deals.biz>", "you@example.com", "50% OFF!!!", "Trashed.")];
    case "spam":
      return [base("sp1", "Prince <prince@scam.test>", "you@example.com", "Urgent transfer", "Definitely spam.")];
    default:
      return [];
  }
}
```

- [ ] **Step 4: Type-check** — `npm run build`; expect exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/lib/folders.ts src/lib/api.ts src/lib/mock.ts
git commit -m "feat(folders): folders.ts + fetchFolder/restore/delete wrappers + to_addr + mock" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Frontend — FolderRail component + styles

**Files:** `src/components/FolderRail.tsx` (new), `src/styles/app.css`

- [ ] **Step 1: Create `src/components/FolderRail.tsx`**

```tsx
import { Inbox, Send, Star, Archive, Trash2, ShieldAlert, type LucideIcon } from "lucide-react";
import { FOLDERS, type Folder } from "../lib/folders";

const ICON: Record<Folder, LucideIcon> = {
  inbox: Inbox,
  sent: Send,
  starred: Star,
  archive: Archive,
  trash: Trash2,
  spam: ShieldAlert,
};

export function FolderRail({
  folder,
  onSelectFolder,
}: {
  folder: Folder;
  onSelectFolder: (f: Folder) => void;
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
    </nav>
  );
}
```

- [ ] **Step 2: Append to `src/styles/app.css`**

```css
/* ===== M12 Folder rail ===== */
.mail-body { flex: 1; min-height: 0; display: flex; }
/* SplitView is the second child — make it fill the width left of the rail. */
.mail-body > :last-child { flex: 1; min-width: 0; }
.folder-rail { width: 78px; flex: none; border-right: 1px solid var(--border); background: var(--surface-2);
  padding: 8px 0; display: flex; flex-direction: column; gap: 2px; overflow-y: auto; }
.folder-item { display: flex; flex-direction: column; align-items: center; gap: 3px; padding: 8px 0;
  border: 0; background: transparent; color: var(--text-muted); cursor: pointer; font-size: 11px; }
.folder-item:hover { color: var(--text); }
.folder-item.active { color: var(--accent-text); }
.folder-item.active svg { background: var(--accent-weak); border-radius: 9px; padding: 5px; box-sizing: content-box; }
.folder-label { line-height: 1; }
```

- [ ] **Step 3: Type-check** — `npm run build`; expect exit 0. (Not mounted yet.)

- [ ] **Step 4: Commit**

```bash
git add src/components/FolderRail.tsx src/styles/app.css
git commit -m "feat(folders): FolderRail component + rail/mail-body styles" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Frontend — recipient display (Sent) in MessageList/MessageItem

**Files:** `src/components/MessageList.tsx`, `src/components/MessageItem.tsx`

- [ ] **Step 1: Edit `src/components/MessageList.tsx`** — add a `showRecipient?: boolean` prop and pass it to each `MessageItem`.

Add `showRecipient = false,` to the destructured params and `showRecipient?: boolean;` to the props type (next to `emptyText?`). Then add `showRecipient={showRecipient}` to BOTH `<MessageItem … />` usages (the grouped branch and the flat branch). For example the flat branch becomes:

```tsx
          visible.map((m) => (
            <MessageItem
              key={m.id}
              msg={m}
              selected={m.id === selectedId}
              onSelect={onSelect}
              onArchive={onArchive}
              onStar={onStar}
              showRecipient={showRecipient}
            />
          ))
```

(Do the same — add `showRecipient={showRecipient}` — inside the `groups.map(... group.messages.map((m) => <MessageItem … />))` branch.)

- [ ] **Step 2: Edit `src/components/MessageItem.tsx`** — accept `showRecipient` and render the recipient instead of the sender when set.

Add `showRecipient = false,` to the destructured params and `showRecipient?: boolean;` to the props type. Replace the sender line (`{msg.from || "(unknown sender)"}`) so it reads:

```tsx
            {showRecipient
              ? `To: ${msg.to_addr || "(no recipient)"}`
              : msg.from || "(unknown sender)"}
```

(The `cat-dot` stays as-is; folder/sent results have an empty `category`, so the dot simply doesn't render.)

- [ ] **Step 3: Type-check** — `npm run build`; expect exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/components/MessageList.tsx src/components/MessageItem.tsx
git commit -m "feat(folders): show recipient (To) in the Sent folder list" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Frontend — Trash actions (Restore + Delete-forever) in ReadingPane

**Files:** `src/components/ReadingPane.tsx`

Replace the **entire** contents of `src/components/ReadingPane.tsx` with the version below (adds `folder`/`onRestore`/`onDeleteForever` props; in Trash, swaps Archive/Trash for Restore + a two-step Delete-forever confirm).

- [ ] **Step 1: Replace `src/components/ReadingPane.tsx`**

```tsx
import { useEffect, useState } from "react";
import {
  fetchMessageBody,
  type MessageBody,
  type MessagePreview,
} from "../lib/api";
import { Mail, Archive, Trash2, Star, CornerUpLeft, RotateCcw } from "lucide-react";
import { isStarred } from "../lib/labels";
import type { Folder } from "../lib/folders";

export function ReadingPane({
  msg,
  loadImages,
  onArchive,
  onTrash,
  onToggleStar,
  onMarkUnread,
  onReply,
  folder = "inbox",
  onRestore,
  onDeleteForever,
}: {
  msg: MessagePreview | null;
  loadImages: boolean;
  onArchive: (m: MessagePreview) => void;
  onTrash: (m: MessagePreview) => void;
  onToggleStar: (m: MessagePreview) => void;
  onMarkUnread: (m: MessagePreview) => void;
  onReply: (m: MessagePreview) => void;
  folder?: Folder;
  onRestore?: (m: MessagePreview) => void;
  onDeleteForever?: (m: MessagePreview) => void;
}) {
  const [body, setBody] = useState<MessageBody | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Two-step confirm for the irreversible permanent delete.
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    if (!msg) {
      setBody(null);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    setBody(null);
    fetchMessageBody(msg.id, loadImages)
      .then((b) => {
        if (!cancelled) setBody(b);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [msg?.id, loadImages]);

  // Reset the delete confirmation whenever the open message changes.
  useEffect(() => setConfirmDelete(false), [msg?.id]);

  if (!msg) {
    return (
      <section className="reading">
        <div className="reading-empty">
          <Mail size={28} />
          <span>Select a message to read</span>
        </div>
      </section>
    );
  }

  const date = msg.internal_date
    ? new Date(msg.internal_date).toLocaleString([], {
        dateStyle: "medium",
        timeStyle: "short",
      })
    : msg.date;

  const inTrash = folder === "trash";

  return (
    <section className="reading">
      <div className="reading-toolbar">
        <button className="icon-btn" aria-label="Reply" onClick={() => onReply(msg)}>
          <CornerUpLeft size={15} />
        </button>
        <button
          className={isStarred(msg) ? "icon-btn active" : "icon-btn"}
          aria-label={isStarred(msg) ? "Unstar" : "Star"}
          onClick={() => onToggleStar(msg)}
        >
          <Star size={15} fill={isStarred(msg) ? "currentColor" : "none"} />
        </button>
        <button className="icon-btn" aria-label="Mark as unread" onClick={() => onMarkUnread(msg)}>
          <Mail size={15} />
        </button>
        {inTrash ? (
          <>
            <button className="icon-btn" aria-label="Restore" onClick={() => onRestore?.(msg)}>
              <RotateCcw size={15} />
            </button>
            {confirmDelete ? (
              <button
                className="btn btn-danger"
                onClick={() => {
                  setConfirmDelete(false);
                  onDeleteForever?.(msg);
                }}
              >
                Delete forever?
              </button>
            ) : (
              <button
                className="icon-btn"
                aria-label="Delete forever"
                onClick={() => setConfirmDelete(true)}
              >
                <Trash2 size={15} />
              </button>
            )}
          </>
        ) : (
          <>
            <button className="icon-btn" aria-label="Archive" onClick={() => onArchive(msg)}>
              <Archive size={15} />
            </button>
            <button className="icon-btn" aria-label="Move to trash" onClick={() => onTrash(msg)}>
              <Trash2 size={15} />
            </button>
          </>
        )}
      </div>
      <div className="reading-head">
        <h2 className="reading-subject">{msg.subject || "(no subject)"}</h2>
        <div className="reading-from">
          <div className="avatar avatar-lg">
            {(msg.from || "?").charAt(0).toUpperCase()}
          </div>
          <div className="reading-from-text">
            <div className="reading-name">{msg.from || "(unknown sender)"}</div>
          </div>
          <div className="reading-date">{date}</div>
        </div>
      </div>
      <div className="reading-body-area">
        {loading ? (
          <div className="reading-status">Loading…</div>
        ) : error ? (
          <pre className="error-text">{error}</pre>
        ) : body?.is_html ? (
          <iframe
            className="reading-frame"
            sandbox=""
            srcDoc={body.html}
            title="Message body"
          />
        ) : body ? (
          <pre className="reading-text">{body.html}</pre>
        ) : null}
      </div>
    </section>
  );
}
```

- [ ] **Step 2: Append a danger-button style to `src/styles/app.css`** (if `.btn-danger` doesn't already exist)

```css
/* ===== M12 — permanent-delete confirm button ===== */
.btn-danger { background: var(--danger); color: #fff; border: 0; border-radius: 8px;
  padding: 5px 10px; font: inherit; font-size: 13px; cursor: pointer; }
```

- [ ] **Step 3: Type-check** — `npm run build`; expect exit 0. (`folder` defaults to `"inbox"`, `onRestore`/`onDeleteForever` are optional, so the current App call site still compiles.)

- [ ] **Step 4: Commit**

```bash
git add src/components/ReadingPane.tsx src/styles/app.css
git commit -m "feat(folders): ReadingPane Trash actions (restore + delete-forever confirm)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Frontend — App wiring (3-way active list + rail) + Header

**Files:** `src/App.tsx`, `src/components/Header.tsx`

Apply these edits in order. **Read both files first.**

- [ ] **Step 1: `Header.tsx` — add an `inFolder` prop that hides the stream nav.**

Add `inFolder = false,` to the destructured params and `inFolder?: boolean;` to the props type (next to `inSearch?`). Change the stream-nav guard from `{account && !isCal && !inSearch && (` to:

```tsx
      {account && !isCal && !inSearch && !inFolder && (
```

- [ ] **Step 2: `App.tsx` — imports.** Add to the `from "./lib/api"` block:

```ts
  fetchFolder,
  restoreMessage,
  deleteMessageForever,
```

And add two imports after the existing component imports:

```tsx
import { FolderRail } from "./components/FolderRail";
import { FOLDERS, type Folder } from "./lib/folders";
```

- [ ] **Step 2b: `App.tsx` — folder state.** After the M11 search state block (after `const [searchQuery, setSearchQuery] = useState("");`), add:

```tsx
  // M12 folders. `folder === "inbox"` means the cached smart inbox; any other value is a live-
  // fetched mailbox. `folderReloadKey` lets re-clicking a folder (or the same one) refetch.
  const [folder, setFolder] = useState<Folder>("inbox");
  const [folderResults, setFolderResults] = useState<MessagePreview[]>([]);
  const [folderSelectedId, setFolderSelectedId] = useState<string | null>(null);
  const [folderLoading, setFolderLoading] = useState(false);
  const [folderReloadKey, setFolderReloadKey] = useState(0);
  const inFolder = folder !== "inbox";
```

- [ ] **Step 2c: `App.tsx` — folder fetch effect.** Add after that state block:

```tsx
  // Live-fetch the selected folder (non-inbox). Re-runs when the folder or reload key changes.
  useEffect(() => {
    if (folder === "inbox") return;
    let cancelled = false;
    setFolderLoading(true);
    setError(null);
    fetchFolder(folder, 50)
      .then((r) => {
        if (!cancelled) {
          setFolderResults(r);
          setFolderLoading(false);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setFolderResults([]);
          setError(String(e));
          setFolderLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [folder, folderReloadKey]);
```

- [ ] **Step 3: `App.tsx` — extend the active-list derivation to 3-way.** Replace the M11 block (`const activeList = inSearch ? searchResults : messages;` … through `const setActiveSelectedId = …;`) with:

```tsx
  // Active list: search results > a live folder > the cached inbox. All selection + action
  // handlers operate on it, so they work identically across inbox, search, and folders.
  const activeList = inSearch ? searchResults : inFolder ? folderResults : messages;
  const setActiveList = inSearch ? setSearchResults : inFolder ? setFolderResults : setMessages;
  const activeSelectedId = inSearch ? searchSelectedId : inFolder ? folderSelectedId : selectedId;
  const setActiveSelectedId = inSearch ? setSearchSelectedId : inFolder ? setFolderSelectedId : setSelectedId;
```

- [ ] **Step 4: `App.tsx` — `nextSelectedId` uses flat order for search OR folder.** Change its first line from `const visible = inSearch ? activeList : orderedForStream(messages, stream);` to:

```tsx
    const visible = inSearch || inFolder ? activeList : orderedForStream(messages, stream);
```

- [ ] **Step 5: `App.tsx` — folder + trash handlers.** Add right after `handleClearSearch`:

```tsx
  function handleSelectFolder(f: Folder) {
    // Switching mailbox leaves any active search; bumping the key refetches even on re-click.
    setInSearch(false);
    setSearchResults([]);
    setSearchSelectedId(null);
    setSearchQuery("");
    setFolderSelectedId(null);
    setFolder(f);
    setFolderReloadKey((k) => k + 1);
  }

  const handleRestore = (m: MessagePreview) =>
    removeWithAction(m, () => restoreMessage(m.id));
  const handleDeleteForever = (m: MessagePreview) =>
    removeWithAction(m, () => deleteMessageForever(m.id));
```

- [ ] **Step 6: `App.tsx` — pass `inFolder` to `<Header>`.** Add to the authenticated `<Header … />` props (after `searching={searching}`):

```tsx
        inFolder={inFolder}
```

- [ ] **Step 7: `App.tsx` — wrap the mail view in `mail-body` with the rail, and feed folder props.** Replace the whole `<SplitView … />` element (inside the `view === "calendar" ? … : ( … )` branch) with:

```tsx
        <div className="mail-body">
          <FolderRail folder={folder} onSelectFolder={handleSelectFolder} />
          <SplitView
            left={
              <MessageList
                messages={activeList}
                stream={stream}
                selectedId={activeSelectedId}
                onSelect={handleSelect}
                onArchive={handleArchive}
                onStar={toggleStar}
                flat={inSearch || inFolder}
                title={
                  inSearch
                    ? "Results"
                    : inFolder
                      ? FOLDERS.find((f) => f.key === folder)?.label
                      : undefined
                }
                emptyText={
                  inSearch
                    ? searching
                      ? "Searching…"
                      : `No results for "${searchQuery}".`
                    : inFolder
                      ? folderLoading
                        ? "Loading…"
                        : "Nothing here."
                      : undefined
                }
                showRecipient={folder === "sent"}
              />
            }
            right={
              <ReadingPane
                msg={selected}
                loadImages={settings.remote_images}
                onArchive={handleArchive}
                onTrash={handleTrash}
                onToggleStar={toggleStar}
                onMarkUnread={(m) => toggleRead(m, false)}
                onReply={handleReply}
                folder={folder}
                onRestore={handleRestore}
                onDeleteForever={handleDeleteForever}
              />
            }
          />
        </div>
```

- [ ] **Step 8: Type-check** — `npm run build`; expect exit 0.

- [ ] **Step 9: Run the maket**

Run (background): `npm run dev` → open `http://localhost:1420` → click **Mail**.
Verify:
- A left **rail** shows Inbox · Sent · Starred · Archive · Trash · Spam.
- Clicking **Sent** shows the mock sent list with **"To: …"** recipients; the stream tabs are hidden.
- **Inbox** restores the smart inbox + stream tabs.
- **Trash** shows the mock trashed message; opening it shows **Restore** + a **Delete-forever** button that asks "Delete forever?" on first click.
- Searching still overrides the folder; clearing returns to it.
Screenshot. Stop the dev server.

- [ ] **Step 10: Commit**

```bash
git add src/App.tsx src/components/Header.tsx
git commit -m "feat(folders): folder rail + 3-way active list + Trash actions wiring" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: Final verification + live E2E + review

**Files:** none.

- [ ] **Step 1: Full gate** — `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings` ; `npm run build`. Expect all green.

- [ ] **Step 2: Live E2E (Tauri)** — `npm run tauri dev`:
  - Click Sent/Starred/Archive/Trash/Spam → each shows real mail; Sent shows recipients.
  - In Trash: open a message → **Restore** returns it to Gmail (disappears from Trash); **Delete forever** (confirm) permanently removes it.
  - star/archive/trash/reply work from a folder; search overrides and clears back to the folder; Inbox + stream tabs unchanged.

- [ ] **Step 3: Final code review** over `git diff main..m12-folders` against the spec (the `include_spam_trash` wiring, the folder mapping, untrash/permanent-delete, the 3-way active-list extension, inbox/search behavior preserved, Tauri build unchanged when `isTauri()` is true).

- [ ] **Step 4:** No commit unless review/E2E surfaces a fix (`fix(folders): …` + trailer).

---

## Post-implementation (outside this plan)
- Update wiki (`wiki/entities/ember.md` + `wiki/log.md`) + auto-memory (`MEMORY.md`, `ember-project.md`): M12 merged; M13 notifications next.
- Use `superpowers:finishing-a-development-branch` to merge `m12-folders` (`Merge M12: folder & Sent views …`).
