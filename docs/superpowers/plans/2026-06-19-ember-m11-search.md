# Ember M11 — Search (server-side Gmail search, lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a header search box that runs a server-side Gmail `q=` search across all mail, shows results in the existing two-pane, and lets the user act on them (open/read, star, archive, trash, reply).

**Architecture:** Reuse, not reinvention. Factor the Gmail client's existing query-paging loop and add `search_message_ids` (same call without the `INBOX` filter); a DB-free `search_messages` command hydrates results via the existing concurrent preview fetch, classifies + sorts by recency. The frontend adds `searchResults` state and makes the M7 action handlers **list-aware** (Approach A) so they operate on whichever list is active (search vs inbox) — inbox behavior unchanged, and M12 folder views reuse the same plumbing.

**Tech Stack:** Rust (reqwest, serde, futures, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite, lucide-react.

**Design spec:** `docs/superpowers/specs/2026-06-19-ember-m11-search-design.md`.

**Conventions for every task:**
- New Rust code carries concise `// 🦀` teaching comments on the *language* concept (owner is learning Rust). Give a one-paragraph plain-English Rust recap after each Rust task.
- Every commit message ends with the trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` (via a second `-m`).
- Branch is already `m11-search`. Work there.
- Gates: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `npm run build`. (`cargo fmt` is NOT used here.)
- No JS test runner exists (vitest deferred since M10); frontend tasks gate on `npm run build` + the browser maket.

---

## File structure

**Backend:**
- Modify `src-tauri/src/gmail/mod.rs` — factor `list_message_ids(label, query, max)`; add `search_message_ids`; keep `list_inbox_message_ids_paged` (delegates).
- Modify `src-tauri/tests/gmail_test.rs` — search + inbox-regression tests.
- Modify `src-tauri/src/commands.rs` — `search_messages` command.
- Modify `src-tauri/src/lib.rs` — register the command.

**Frontend:**
- Modify `src/lib/api.ts` — `searchMessages` wrapper + mock branch.
- Modify `src/lib/mock.ts` — `mockSearch`.
- Modify `src/components/MessageList.tsx` — flat mode (`flat`/`title`/`emptyText`).
- Modify `src/components/Header.tsx` — search input + props.
- Modify `src/App.tsx` — search state + list-aware handler refactor + wiring.
- Modify `src/styles/app.css` — search input styling.

---

## Task 1: Backend — `search_message_ids` (TDD)

**Files:**
- Modify: `src-tauri/src/gmail/mod.rs`
- Modify: `src-tauri/tests/gmail_test.rs`

- [ ] **Step 1: Add failing tests to `src-tauri/tests/gmail_test.rs`** (append; `query_param_is_missing` is already imported there)

```rust
#[tokio::test(flavor = "multi_thread")]
async fn search_message_ids_searches_all_mail_without_inbox_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("q", "from:maya"))
        .and(query_param_is_missing("labelIds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{ "id": "s1" }, { "id": "s2" }]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.search_message_ids("from:maya", 50).await.unwrap();
    assert_eq!(ids, vec!["s1".to_string(), "s2".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_inbox_message_ids_paged_still_filters_to_inbox() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("labelIds", "INBOX"))
        .and(query_param("q", "newer_than:30d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{ "id": "i1" }]
        })))
        .mount(&server)
        .await;

    let client = GmailClient::with_base_url("tok".into(), server.uri());
    let ids = client.list_inbox_message_ids_paged("newer_than:30d", 50).await.unwrap();
    assert_eq!(ids, vec!["i1".to_string()]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --test gmail_test search_message_ids_searches_all_mail_without_inbox_filter`
Expected: FAIL — `search_message_ids` not found.

- [ ] **Step 3: Refactor + add the methods in `src-tauri/src/gmail/mod.rs`**

Replace the existing `list_inbox_message_ids_paged` method (the one whose body builds `?labelIds=INBOX&maxResults=100&q=…` in a pagination loop) with the following three items (a private shared helper + the two public methods):

```rust
    /// Shared paging loop for `messages.list`. `label = Some("INBOX")` restricts to a label;
    /// `None` searches across all mail. Follows `nextPageToken` up to `max_total` ids.
    // 🦀 `Option<&str>` is a borrowed, maybe-absent string: `Some("INBOX")` or `None`. Passing a
    //    reference (not `String`) means callers lend us the label without giving up ownership.
    async fn list_message_ids(
        &self,
        label: Option<&str>,
        query: &str,
        max_total: u32,
    ) -> Result<Vec<String>> {
        // 🦀 Percent-encode the query value so characters like ':' are URL-safe.
        let encoded_q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let mut ids = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/messages?maxResults=100&q={}",
                self.base_url, encoded_q
            );
            // 🦀 `if let Some(l) = label` runs the block only when a label was supplied, binding
            //    the inner `&str` to `l`. No label → search across all mail.
            if let Some(l) = label {
                url.push_str(&format!("&labelIds={l}"));
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

    /// List INBOX message ids matching `query` (e.g. "newer_than:30d"), following pagination up to
    /// `max_total` ids. (Sync path — behavior unchanged; now delegates to `list_message_ids`.)
    pub async fn list_inbox_message_ids_paged(
        &self,
        query: &str,
        max_total: u32,
    ) -> Result<Vec<String>> {
        self.list_message_ids(Some("INBOX"), query, max_total).await
    }

    /// Search across ALL mail (no label restriction) for `query`. Gmail excludes Spam/Trash by
    /// default. Follows pagination up to `max_total` ids.
    pub async fn search_message_ids(&self, query: &str, max_total: u32) -> Result<Vec<String>> {
        self.list_message_ids(None, query, max_total).await
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --test gmail_test`
Expected: PASS (the two new tests + all existing gmail tests).

- [ ] **Step 5: Commit + Rust recap**

```bash
cd /Users/makar/dev/ownmail
git add src-tauri/src/gmail/mod.rs src-tauri/tests/gmail_test.rs
git commit -m "feat(gmail): search_message_ids (all-mail q=) via shared paging helper" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: extracting the shared loop into `list_message_ids(label: Option<&str>, …)` is classic DRY — the two public methods are now one-liners that differ only by passing `Some("INBOX")` vs `None`. `Option<&str>` lets one parameter mean "a label, or nothing" without overloads.

---

## Task 2: Backend — `search_messages` command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

This command orchestrates over the network; like the other I/O commands it's covered by the client tests (Task 1) + manual E2E. No new automated test.

- [ ] **Step 1: Add a constant near `PREVIEW_CONCURRENCY` in `src-tauri/src/commands.rs`**

```rust
const SEARCH_MAX: u32 = 50;
```

- [ ] **Step 2: Add the command (place it after `get_reply_context`, before the settings commands)**

```rust
/// Search all mail with a Gmail `q=` query and return hydrated, recency-sorted previews. DB-free —
/// results are fetched live, not cached. Reuses the smart-inbox scorer to set each result's category
/// (for the category dot), exactly as the sync path does.
#[tauri::command]
pub async fn search_messages(query: String, max: u32) -> Result<Vec<MessagePreview>> {
    // 🦀 `clamp` keeps `max` within 1..=SEARCH_MAX regardless of what the frontend sends.
    let max = max.clamp(1, SEARCH_MAX);
    let stored = ensure_access_token().await?;
    let client = GmailClient::new(stored.access_token);
    let ids = client.search_message_ids(&query, max).await?;
    let mut previews = client.get_message_previews(&ids, PREVIEW_CONCURRENCY).await?;
    // 🦀 `for p in &mut previews` iterates by mutable reference so we can write `p.category` in place.
    for p in &mut previews {
        p.category = scorer::classify(&scorer::MessageFeatures {
            label_ids: &p.label_ids,
            from_addr: &p.from,
            has_list_unsubscribe: p.has_list_unsubscribe,
            has_list_id: p.has_list_id,
        })
        .as_str()
        .to_string();
    }
    // 🦀 `get_message_previews` returns results out of order (buffer_unordered); sort newest-first.
    previews.sort_by(|a, b| b.internal_date.cmp(&a.internal_date));
    Ok(previews)
}
```

(`MessagePreview`, `GmailClient`, `ensure_access_token`, `scorer`, and `PREVIEW_CONCURRENCY` are already imported/declared in `commands.rs`.)

- [ ] **Step 3: Register the command in `src-tauri/src/lib.rs`**

In the `tauri::generate_handler![...]` list, add after `commands::get_reply_context,`:

```rust
            commands::search_messages,
```

- [ ] **Step 4: Build + test + clippy**

Run: `cd src-tauri && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS — builds, all tests green, clippy clean.

- [ ] **Step 5: Commit + Rust recap**

```bash
cd /Users/makar/dev/ownmail
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): search_messages (all-mail, classified, recency-sorted)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: `&mut previews` + `for p in &mut previews` gives mutable borrows of each element so we can set `p.category` without rebuilding the vector; `clamp` is a tidy one-call replacement for min/max guarding.

---

## Task 3: Frontend — `searchMessages` API wrapper + mock

**Files:**
- Modify: `src/lib/mock.ts`
- Modify: `src/lib/api.ts`

- [ ] **Step 1: Add `mockSearch` to `src/lib/mock.ts`** (append at the end of the file)

```ts
/** Browser-maket search: case-insensitive substring match over the mock messages. */
export function mockSearch(query: string): MessagePreview[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return MOCK_MESSAGES.filter(
    (m) =>
      m.from.toLowerCase().includes(q) ||
      m.subject.toLowerCase().includes(q) ||
      m.snippet.toLowerCase().includes(q),
  );
}
```

(`MOCK_MESSAGES` and the `MessagePreview` type import already exist at the top of `mock.ts`.)

- [ ] **Step 2: Add the wrapper to `src/lib/api.ts`**

Add `mockSearch` to the existing mock import (the line `import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek } from "./mock";`):

```ts
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek, mockSearch } from "./mock";
```

Add the wrapper (next to `fetchInboxPreview`, or anywhere among the exports):

```ts
export const searchMessages = (query: string, max = 50): Promise<MessagePreview[]> =>
  isTauri()
    ? invoke<MessagePreview[]>("search_messages", { query, max })
    : Promise.resolve(mockSearch(query));
```

- [ ] **Step 3: Type-check**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/lib/mock.ts src/lib/api.ts
git commit -m "feat(search): searchMessages api wrapper + browser mock" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Frontend — MessageList flat mode

**Files:**
- Modify: `src/components/MessageList.tsx`

Add a flat-rendering mode (no stream filtering/grouping) with overridable title + empty text, used for search results. Replace the **entire** contents of `src/components/MessageList.tsx` with:

- [ ] **Step 1: Replace `src/components/MessageList.tsx`**

```tsx
import type { MessagePreview } from "../lib/api";
import { MessageItem } from "./MessageItem";
import {
  STREAMS,
  filterByStream,
  groupByStream,
  type Stream,
} from "../lib/streams";

export function MessageList({
  messages,
  stream,
  selectedId,
  onSelect,
  onArchive,
  onStar,
  flat = false,
  title,
  emptyText,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  /** When true, render `messages` as a flat list (no stream filter/grouping) — used for search. */
  flat?: boolean;
  /** Header title override (used in flat/search mode). */
  title?: string;
  /** Empty-state text override (used in flat/search mode). */
  emptyText?: string;
}) {
  // Flat mode (search): render the given messages as-is. Stream mode (inbox): filter, and
  // group by category only in the "All" view.
  const visible = flat ? messages : filterByStream(messages, stream);
  const groups = !flat && stream === "all" ? groupByStream(visible) : null;
  const headerTitle = flat
    ? title ?? "Results"
    : STREAMS.find((s) => s.key === stream)?.label ?? "Inbox";
  const count = groups
    ? groups.reduce((n, g) => n + g.messages.length, 0)
    : visible.length;
  const empty = emptyText ?? "No messages here — hit Sync.";

  return (
    <section className="msglist">
      <div className="msglist-header">
        <span className="msglist-title">{headerTitle}</span>
        <span className="msglist-count">{count} messages</span>
      </div>
      <div className="msglist-scroll">
        {count === 0 ? (
          <div className="empty">{empty}</div>
        ) : groups ? (
          groups.map((group) => (
            <div key={group.category} className="msglist-group">
              <div className="msglist-group-header">
                <span>{group.label}</span>
                <span className="msglist-group-count">
                  {group.messages.length}
                </span>
              </div>
              {group.messages.map((m) => (
                <MessageItem
                  key={m.id}
                  msg={m}
                  selected={m.id === selectedId}
                  onSelect={onSelect}
                  onArchive={onArchive}
                  onStar={onStar}
                />
              ))}
            </div>
          ))
        ) : (
          visible.map((m) => (
            <MessageItem
              key={m.id}
              msg={m}
              selected={m.id === selectedId}
              onSelect={onSelect}
              onArchive={onArchive}
              onStar={onStar}
            />
          ))
        )}
      </div>
    </section>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `npm run build`
Expected: PASS. (Existing callers pass no `flat`/`title`/`emptyText` → defaults preserve inbox behavior exactly.)

- [ ] **Step 3: Commit**

```bash
git add src/components/MessageList.tsx
git commit -m "feat(search): MessageList flat mode for results" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Frontend — Header search input

**Files:**
- Modify: `src/components/Header.tsx`
- Modify: `src/styles/app.css`

Add a search box (mail view only) and hide the stream nav while a search is active. Replace the **entire** contents of `src/components/Header.tsx` with:

- [ ] **Step 1: Replace `src/components/Header.tsx`**

```tsx
import { useState } from "react";
import {
  Flame,
  Pencil,
  RefreshCw,
  Settings as SettingsIcon,
  Sun,
  Moon,
  Inbox,
  Users,
  Bell,
  Newspaper,
  Mail,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Search,
  X,
  type LucideIcon,
} from "lucide-react";
import { useTheme, type Theme } from "../theme";
import { STREAMS, type Stream } from "../lib/streams";

const THEME_ICON: Record<Theme, LucideIcon> = { light: Sun, dark: Moon };

const STREAM_ICON: Record<Stream, LucideIcon> = {
  all: Inbox,
  people: Users,
  notifications: Bell,
  newsletters: Newspaper,
};

export type View = "mail" | "calendar";

export interface CalendarNav {
  rangeLabel: string;
  onPrev: () => void;
  onToday: () => void;
  onNext: () => void;
}

export function Header({
  busy,
  onSync,
  onCompose,
  onSettings,
  status,
  account = null,
  stream = "all",
  onSelectStream,
  view = "mail",
  onSelectView,
  calendar,
  onSearch,
  onClearSearch,
  inSearch = false,
  searching = false,
}: {
  busy: boolean;
  onSync?: () => void;
  onCompose?: () => void;
  onSettings?: () => void;
  status: string | null;
  account?: string | null;
  stream?: Stream;
  onSelectStream?: (s: Stream) => void;
  view?: View;
  onSelectView?: (v: View) => void;
  calendar?: CalendarNav;
  onSearch?: (q: string) => void;
  onClearSearch?: () => void;
  inSearch?: boolean;
  searching?: boolean;
}) {
  const { theme, cycleTheme } = useTheme();
  const ThemeIcon = THEME_ICON[theme];
  const isCal = view === "calendar";
  // Local input text; App owns the submitted query + results.
  const [q, setQ] = useState("");

  function submitSearch(e: React.FormEvent) {
    e.preventDefault();
    if (q.trim()) onSearch?.(q);
  }
  function clearSearch() {
    setQ("");
    onClearSearch?.();
  }

  return (
    <header className="app-header">
      <span className="brand">
        <Flame size={20} className="brand-icon" /> Ember
      </span>

      {account && onSelectView && (
        <div className="view-toggle" role="tablist" aria-label="Mail or Calendar">
          <button
            className={view === "mail" ? "view-tab active" : "view-tab"}
            aria-current={view === "mail" ? "page" : undefined}
            onClick={() => onSelectView("mail")}
          >
            <Mail size={14} /> <span className="nav-label">Mail</span>
          </button>
          <button
            className={view === "calendar" ? "view-tab active" : "view-tab"}
            aria-current={view === "calendar" ? "page" : undefined}
            onClick={() => onSelectView("calendar")}
          >
            <CalendarDays size={14} /> <span className="nav-label">Calendar</span>
          </button>
        </div>
      )}

      {account && !isCal && !inSearch && (
        <nav className="header-nav">
          {STREAMS.map((s) => {
            const Icon = STREAM_ICON[s.key];
            return (
              <button
                key={s.key}
                className={s.key === stream ? "header-nav-item active" : "header-nav-item"}
                title={s.label}
                aria-current={s.key === stream ? "page" : undefined}
                onClick={() => onSelectStream?.(s.key)}
              >
                <Icon size={15} /> <span className="nav-label">{s.label}</span>
              </button>
            );
          })}
        </nav>
      )}

      {account && isCal && calendar && (
        <nav className="week-nav">
          <button className="icon-btn" aria-label="Previous week" onClick={calendar.onPrev}>
            <ChevronLeft size={16} />
          </button>
          <button className="btn" onClick={calendar.onToday}>
            Today
          </button>
          <button className="icon-btn" aria-label="Next week" onClick={calendar.onNext}>
            <ChevronRight size={16} />
          </button>
          <span className="week-range">{calendar.rangeLabel}</span>
        </nav>
      )}

      <span className="spacer" />

      {account && !isCal && onSearch && (
        <form className="search-box" onSubmit={submitSearch} role="search">
          <Search size={14} className="search-icon" />
          <input
            className="search-input"
            type="search"
            placeholder="Search mail…"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            aria-label="Search mail"
          />
          {(inSearch || q) && (
            <button
              type="button"
              className="icon-btn search-clear"
              aria-label="Clear search"
              onClick={clearSearch}
            >
              <X size={14} />
            </button>
          )}
          {searching && <span className="search-spinner" aria-hidden="true" />}
        </form>
      )}

      {status && <span className="status-text">{status}</span>}

      {!isCal && onCompose && (
        <button className="btn" onClick={onCompose}>
          <Pencil size={15} /> <span className="nav-label">Compose</span>
        </button>
      )}
      {!isCal && onSync && (
        <button className="btn btn-accent" onClick={onSync} disabled={busy}>
          <RefreshCw size={15} className={busy ? "spin" : undefined} />
          {busy ? "Syncing…" : "Sync"}
        </button>
      )}

      {account && (
        <div className="header-account" title={account}>
          <div className="avatar">{account.charAt(0).toUpperCase()}</div>
          <span className="account-email">{account}</span>
        </div>
      )}
      {account && onSettings && (
        <button className="icon-btn" onClick={onSettings} aria-label="Settings">
          <SettingsIcon size={16} />
        </button>
      )}
      <button
        className="icon-btn"
        onClick={cycleTheme}
        aria-label={`Theme: ${theme}. Click to switch.`}
      >
        <ThemeIcon size={16} />
      </button>
    </header>
  );
}
```

- [ ] **Step 2: Append search styles to `src/styles/app.css`**

```css
/* ===== M11 Search — header search box ===== */
.search-box { display: inline-flex; align-items: center; gap: 6px; position: relative;
  border: 1px solid var(--border-strong); border-radius: 8px; padding: 3px 8px; background: var(--surface); }
.search-icon { color: var(--text-faint); flex: none; }
.search-input { border: 0; outline: none; background: transparent; color: var(--text);
  font: inherit; font-size: 13px; width: 180px; }
.search-input::placeholder { color: var(--text-faint); }
.search-clear { padding: 2px; }
.search-spinner { width: 12px; height: 12px; border: 2px solid var(--border-strong);
  border-top-color: var(--accent); border-radius: 50%; animation: spin 0.7s linear infinite; }
```

(The `spin` keyframes already exist — the Sync button reuses them.)

- [ ] **Step 3: Type-check**

Run: `npm run build`
Expected: PASS. (New Header props are optional; the unchanged `App.tsx` still compiles. `lucide-react` exports `Search` and `X`.)

- [ ] **Step 4: Commit**

```bash
git add src/components/Header.tsx src/styles/app.css
git commit -m "feat(search): header search input (mail view) + styles" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Frontend — App wiring + list-aware action refactor

**Files:**
- Modify: `src/App.tsx`

This is the Approach A core: make the action handlers operate on the active list (search results vs inbox). Apply these edits in order. **Read `src/App.tsx` first.**

- [ ] **Step 1: Add `searchMessages` to the api import**

In the `from "./lib/api"` import block, add `searchMessages,` (e.g. after `setMessageStarred,`):

```ts
  searchMessages,
```

- [ ] **Step 2: Add search state** — after the `const [weekStart, setWeekStart] = useState<Date>(() => startOfWeek(new Date()));` line, add:

```tsx
  // M11 search. `inSearch` (a boolean, not array-nullability) marks search mode so both lists stay
  // the same non-null MessagePreview[] type and their setters unify in the `setActiveList` ternary.
  const [inSearch, setInSearch] = useState(false);
  const [searchResults, setSearchResults] = useState<MessagePreview[]>([]);
  const [searchSelectedId, setSearchSelectedId] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
```

- [ ] **Step 3: Replace the `selected` useMemo** (the block `const selected = useMemo(() => messages.find(...) ?? null, [messages, selectedId]);`) with the active-list derivations + an active-list `selected`:

```tsx
  // The "active list" is the search results when searching, else the inbox. All selection +
  // action handlers operate on it, so they work identically for inbox and search.
  const activeList = inSearch ? searchResults : messages;
  const setActiveList = inSearch ? setSearchResults : setMessages;
  const activeSelectedId = inSearch ? searchSelectedId : selectedId;
  const setActiveSelectedId = inSearch ? setSearchSelectedId : setSelectedId;

  const selected = useMemo(
    () => activeList.find((m) => m.id === activeSelectedId) ?? null,
    [activeList, activeSelectedId],
  );
```

- [ ] **Step 4: Replace `nextSelectedId`** with:

```tsx
  // Pick the row to select after the current one is removed (archive/trash): next visible, else
  // previous, else nothing. Inbox uses the stream ordering; search results are already a flat list.
  function nextSelectedId(removedId: string): string | null {
    const visible = inSearch ? activeList : orderedForStream(messages, stream);
    const idx = visible.findIndex((m) => m.id === removedId);
    if (idx === -1) return activeSelectedId;
    const next = visible[idx + 1] ?? visible[idx - 1] ?? null;
    return next ? next.id : null;
  }
```

- [ ] **Step 5: Replace `withMessagesRollback`** with a list-aware version (rename to `withActiveRollback`):

```tsx
  // Roll back to `snapshot` on the ACTIVE list and surface the error if the backend call rejects.
  async function withActiveRollback(
    snapshot: MessagePreview[],
    call: () => Promise<void>,
  ) {
    setError(null);
    try {
      await call();
    } catch (e) {
      setActiveList(snapshot);
      setError(String(e));
    }
  }
```

- [ ] **Step 6: Replace `toggleRead` and `toggleStar`** with active-list versions:

```tsx
  function toggleRead(m: MessagePreview, read: boolean) {
    const snapshot = activeList;
    setActiveList(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, UNREAD, !read) : x)),
    );
    void withActiveRollback(snapshot, () => setMessageRead(m.id, read));
  }

  function toggleStar(m: MessagePreview) {
    const starred = !isStarred(m);
    const snapshot = activeList;
    setActiveList(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, STARRED, starred) : x)),
    );
    void withActiveRollback(snapshot, () => setMessageStarred(m.id, starred));
  }
```

- [ ] **Step 7: Replace `removeWithAction`** with the active-list version:

```tsx
  function removeWithAction(m: MessagePreview, call: () => Promise<void>) {
    const listSnap = activeList;
    const selSnap = activeSelectedId;
    setActiveList(listSnap.filter((x) => x.id !== m.id));
    if (activeSelectedId === m.id) setActiveSelectedId(nextSelectedId(m.id));
    setError(null);
    call().catch((e) => {
      setActiveList(listSnap);
      setActiveSelectedId(selSnap);
      setError(String(e));
    });
  }
```

- [ ] **Step 8: Replace `handleSelect`** with the active-list version, and add the search handlers right after it:

```tsx
  // Selecting a message opens it and (if unread) marks it read — like every mail client.
  function handleSelect(id: string) {
    setActiveSelectedId(id);
    const m = activeList.find((x) => x.id === id);
    if (m && isUnread(m)) toggleRead(m, true);
  }

  async function handleSearch(q: string) {
    const query = q.trim();
    if (!query) return;
    setInSearch(true);
    setSearchQuery(query);
    setSearchSelectedId(null);
    setSearching(true);
    setError(null);
    try {
      setSearchResults(await searchMessages(query, 50));
    } catch (e) {
      setSearchResults([]);
      setError(String(e));
    } finally {
      setSearching(false);
    }
  }

  function handleClearSearch() {
    setInSearch(false);
    setSearchResults([]);
    setSearchSelectedId(null);
    setSearchQuery("");
    setError(null);
  }
```

- [ ] **Step 9: Pass the search props to `<Header>`** — add these props to the authenticated `<Header ... />` (after the `calendar={{ … }}` prop):

```tsx
        onSearch={handleSearch}
        onClearSearch={handleClearSearch}
        inSearch={inSearch}
        searching={searching}
```

- [ ] **Step 10: Feed the active list to `<MessageList>`** — in the `view === "calendar" ? … : (<SplitView left={<MessageList … />} … />)` block, replace the `<MessageList … />` element with:

```tsx
            <MessageList
              messages={activeList}
              stream={stream}
              selectedId={activeSelectedId}
              onSelect={handleSelect}
              onArchive={handleArchive}
              onStar={toggleStar}
              flat={inSearch}
              title={inSearch ? "Results" : undefined}
              emptyText={
                inSearch
                  ? searching
                    ? "Searching…"
                    : `No results for "${searchQuery}".`
                  : undefined
              }
            />
```

(The `<ReadingPane … />` is unchanged — its `msg={selected}` and the action handlers are now list-aware.)

- [ ] **Step 11: Type-check**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 12: Run the maket**

Run (background): `npm run dev` → open `http://localhost:1420` (browser mock mode opens on Calendar; click **Mail**).
Verify:
- The header has a **Search mail…** box. Type `roadmap` → Enter → results list shows the matching mock message(s) as a flat list ("Results"); the stream tabs are hidden; a ✕ appears.
- Selecting a result opens its body in the reading pane.
- Type a non-matching term → "No results for …".
- Click ✕ → returns to the smart inbox with stream tabs.
Take a screenshot. Stop the dev server when done.

- [ ] **Step 13: Commit**

```bash
git add src/App.tsx
git commit -m "feat(search): wire header search + list-aware action handlers (Approach A)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Final verification + live E2E + final review

**Files:** none (verification only).

- [ ] **Step 1: Full gate**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings`
Run: `npm run build`
Expected: all green.

- [ ] **Step 2: Live E2E (Tauri)**

Run: `npm run tauri dev`
- In Mail view, search a real term and a Gmail operator (e.g. `from:`); confirm results render newest-first with correct category dots.
- Open a result → full body loads and it marks read.
- From a result: star, archive, trash, and reply — confirm each takes effect in Gmail and the results list updates (with rollback if a call fails).
- Clear search (✕) → returns to the smart inbox; inbox behavior (streams, actions) is unchanged.

- [ ] **Step 3: Final code review**

Dispatch a final code reviewer over `git diff main..m11-search` against the spec (correctness of the paging refactor, the command, and the list-aware handler refactor; confirm inbox behavior is preserved and the Tauri build is unchanged when `isTauri()` is true).

- [ ] **Step 4: No commit needed** unless review/E2E surfaced a fix (commit any fix as `fix(search): …` with the trailer).

---

## Post-implementation (outside this plan)

- Update wiki roadmap (`wiki/entities/ember.md`) + `wiki/log.md`: M11 search done; note M12 folders / M13 notifications are next.
- Update auto-memory (`MEMORY.md` + `ember-project.md`): M11 merged; M11→M13 sequence.
- Use `superpowers:finishing-a-development-branch` to merge `m11-search` (merge commit `Merge M11: search …`).
