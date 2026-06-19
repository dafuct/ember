# Ember — Milestone 11: Search (server-side Gmail search, lean v1) — Design Spec

**Status:** Approved design (2026-06-19). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Add **search** to Ember — a query box that runs a **server-side Gmail `q=` search across all
mail** (any age, any folder), shows the matches in the existing two-pane UI, and lets the user **act
on results** (open/read, star, archive, trash, reply) exactly as in the inbox. First of a three-step
sequence: **M11 Search → M12 Folder & Sent views → M13 New-mail notifications.**

**Architecture in one paragraph:** Reuse, not reinvention. The Gmail client already lists message ids
for a `q=` query (`list_inbox_message_ids_paged`); M11 factors out the shared paging loop and adds
`search_message_ids` — the same call **without** the `labelIds=INBOX` restriction — then reuses the
existing concurrent `get_message_previews` to hydrate results. One **DB-free** command
`search_messages(query, max)` orchestrates this and sorts by recency. The frontend adds a header
search box and a `searchResults` state; when a search is active, the **same** two-pane (MessageList +
ReadingPane) renders the flat results instead of the smart-inbox streams. The one structural change is
making the message-action handlers **list-aware** (Approach A): they operate on whichever list is
active (search results vs. inbox), so star/archive/trash/reply work on results with no duplicated
logic — and **M12 folder views reuse the same plumbing**. A browser mock path keeps search working in
the "maket". No new OAuth scope, no schema migration.

**Tech Stack:** Rust (reqwest, serde, futures, Tauri 2; wiremock for tests), React 19 + TypeScript +
Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST
carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task,
give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M10 are merged to `main`. Ember reads/classifies/mutates/sends mail, has settings + disconnect, and
a read-only calendar week view. The glaring daily-driver gap: **there is no search** — the only
navigation is the smart-inbox stream tabs over the ~30-day cached INBOX. M11 adds real search. It is
the first of three sequenced milestones (Search → Folders → Notifications); the **list-aware action
refactor** introduced here is deliberately shaped so **M12 folder/Sent views reuse it**.

---

## Scope

**In scope (lean v1):**
- A **header search box**: type a query, press Enter → results; a ✕ clears and returns to the inbox.
- **Server-side Gmail `q=` search across all mail** (Gmail's own operators work for free), results
  fetched live (not cached).
- Results shown as a **flat list** in the existing two-pane; selecting opens the full body (marks read).
- **Full actions on results** — star / archive / trash / reply / mark-unread — via the existing
  commands, with optimistic updates on the results list (**Approach A: list-aware handlers**).
- A **browser mock** so search works in the maket.

**Explicitly deferred (not in M11):**
- Pagination / load-more (fetch the first `max` = 50 results).
- Relevance ranking (we sort by `internal_date` descending).
- Search scoped to a folder/label (that arrives with **M12**).
- As-you-type / debounced search, a ⌘K palette, saved searches, search history.
- Match highlighting in results.
- Offline / local-cache search.
- Refreshing the cached inbox view to reflect an action taken on a result (the backend keeps the DB
  consistent; the in-memory inbox list updates on the next sync — see Known limitations).

---

## Components & contracts

### Backend — `src-tauri/src/gmail/mod.rs`
Factor the existing INBOX paging loop into a shared helper, then add the search variant:
```rust
/// List message ids for an optional label + `q=` query, following pagination up to `max_total`.
/// `label = Some("INBOX")` restricts to the inbox; `None` searches across all mail.
async fn list_message_ids(&self, label: Option<&str>, query: &str, max_total: u32)
    -> Result<Vec<String>>;

/// Inbox-restricted listing (unchanged behavior; now delegates to list_message_ids(Some("INBOX"),…)).
pub async fn list_inbox_message_ids_paged(&self, query: &str, max_total: u32) -> Result<Vec<String>>;

/// Search across ALL mail (no label restriction). `messages.list?q={query}` (Gmail excludes
/// Spam/Trash by default), paginated up to `max_total`.
pub async fn search_message_ids(&self, query: &str, max_total: u32) -> Result<Vec<String>>;
```
- `list_message_ids` builds `…/messages?maxResults=100&q={enc(query)}` and appends `&labelIds={label}`
  only when `label` is `Some`. The `q` value is percent-encoded (as today via `url::form_urlencoded`).
- Pagination (`nextPageToken`) and the `max_total` cap behave exactly as the current inbox method.
- `list_inbox_message_ids_paged` keeps its signature/behavior (sync path is untouched) — it just
  delegates, so there's no duplicated loop.

### Backend — `src-tauri/src/commands.rs` + `src-tauri/src/lib.rs`
```rust
#[tauri::command]
pub async fn search_messages(query: String, max: u32) -> Result<Vec<MessagePreview>>;
```
- `max` clamped to a sane range (1..=50).
- `ensure_access_token` → `GmailClient` → `search_message_ids(&query, max)` →
  `get_message_previews(&ids, PREVIEW_CONCURRENCY)` (existing concurrent fetch, skips per-message
  failures) → **sort by `internal_date` descending** (results come back unordered from
  `buffer_unordered`) → return.
- `category` is left as the empty string (the smart-inbox stream isn't shown for a flat result list);
  no scoring needed.
- **DB-free** (no `State<Db>`), reuses `gmail.modify`, no migration. Registered in `lib.rs`.

### Backend — tests (`src-tauri/tests/gmail_test.rs`)
- `search_message_ids` sends `q`, **omits `labelIds=INBOX`**, and follows pagination
  (`query_param`/`query_param_is_missing` assertions, mirroring existing tests).
- A regression assertion that `list_inbox_message_ids_paged` still sends `labelIds=INBOX` (guards the
  refactor).

### Frontend — `src/lib/api.ts`
```ts
export const searchMessages = (query: string, max = 50): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("search_messages", { query, max })
            : Promise.resolve(mockSearch(query));
```
`mockSearch(query)` (in `lib/mock.ts`) filters `MOCK_MESSAGES` by case-insensitive substring on
from/subject/snippet so the maket can demo search.

### Frontend — `src/components/Header.tsx`
- A **search `<input>`** shown when `account` and `view === "mail"`. Enter (or a search button) calls
  `onSearch(query)`; an inline ✕ (shown when a search is active) calls `onClearSearch()`.
- New optional props: `searchQuery?: string`, `onSearch?: (q: string) => void`,
  `onClearSearch?: () => void`, `searching?: boolean` (a spinner/disabled state). Existing props
  unchanged.
- When a search is active, the stream nav is replaced by a compact "Search: '…' ✕" indicator (the
  smart-inbox streams don't apply to flat results).

### Frontend — `src/App.tsx` (the Approach A refactor)
- New state: `inSearch: boolean` (default `false`), `searchResults: MessagePreview[]` (default `[]`),
  `searchSelectedId: string | null`, `searching: boolean`, `searchQuery: string`. Using a **boolean
  flag** (not array-nullability) to mark search mode keeps both lists the same non-null
  `MessagePreview[]` type, so their `useState` setters share one `Dispatch<SetStateAction<…>>` type and
  the ternaries below typecheck cleanly. "Searched, zero results" is `inSearch === true` with an empty
  array — distinct from "not searching".
- Derive the **active list** (both branches share a type):
  ```ts
  const activeList          = inSearch ? searchResults    : messages;
  const setActiveList       = inSearch ? setSearchResults  : setMessages;
  const activeSelectedId    = inSearch ? searchSelectedId  : selectedId;
  const setActiveSelectedId = inSearch ? setSearchSelectedId : setSelectedId;
  ```
- **Refactor the existing handlers to be list-aware**: `withMessagesRollback`, `toggleRead`,
  `toggleStar`, `removeWithAction`, `handleArchive`, `handleTrash`, `handleSelect`, `nextSelectedId`
  operate on `activeList`/`setActiveList`/`activeSelectedId` instead of hard-wiring
  `messages`/`setMessages`/`selectedId`. The optimistic-update + rollback logic is unchanged; only the
  target state is parameterized. (Inbox behavior is identical when not searching.)
  - `nextSelectedId` uses `orderedForStream(messages, stream)` for the inbox but the **flat order** of
    `searchResults` in search mode (no stream grouping).
- `handleSearch(q)`: ignore empty/whitespace; set `inSearch=true` + `searching`, call
  `searchMessages(q)`, set `searchResults` + clear `searchSelectedId`; on error set an error + empty
  results (still `inSearch=true`). `handleClearSearch`: `inSearch=false`, `searchResults=[]`,
  `searchSelectedId=null` → back to the inbox.
- Render: when `inSearch`, the `<SplitView>` is fed `activeList` (flat) + the search selection; the
  smart-inbox **stream tabs are hidden** (Header shows the search indicator). MessageList renders flat
  in search mode (see below). The reading pane is unchanged — its action buttons call the now
  list-aware handlers.

### Frontend — `src/components/MessageList.tsx`
- Add a **flat mode**: when rendering search results, list `messages` in given order without
  `orderedForStream` grouping (e.g. a `flat?: boolean` prop, or `stream={null}` meaning "no grouping").
  Inbox rendering is unchanged.

### Frontend — `src/styles/app.css`
Search input (header), the "Search: '…' ✕" indicator, results empty-state. Reuse existing tokens.

---

## Data flow

**Run a search:** type in the header box → Enter → `handleSearch(q)` → `searchMessages(q)` → command →
`search_message_ids` (all mail) → `get_message_previews` → recency-sorted `MessagePreview[]` →
`searchResults` (`inSearch=true`) → the two-pane shows the flat list.

**Open a result:** select → `fetch_message_body(id)` (live, works for any id) + mark read (list-aware
`toggleRead`). Reading-pane star/archive/trash/reply call the existing commands via the list-aware
handlers, updating `searchResults` optimistically (with rollback on failure).

**Exit search:** ✕ → `inSearch=false` (results cleared) → smart inbox returns (stream tabs reappear).

**Maket (browser):** `!isTauri()` → `mockSearch` filters `MOCK_MESSAGES`; actions are not exercised in
the maket (mutations stay invoke-only).

---

## Error handling

- Empty/whitespace query → no request (clear or keep current results; do not enter a broken state).
- Search request fails (network/Gmail) → an inline error in the results area + a Retry that re-runs the
  query; the inbox state is untouched.
- An action on a result fails → the optimistic change rolls back on the results list and the error
  surfaces (existing M7 rollback behavior, now list-aware).
- Per-message preview fetch failures inside `get_message_previews` are skipped (existing behavior) —
  search returns the previews it could fetch.
- No `MutexGuard`-across-`.await` concerns (the command is DB-free).

---

## Testing strategy

- **Rust** (`tests/gmail_test.rs`, wiremock): `search_message_ids` sends `q`, omits `labelIds=INBOX`,
  paginates; `list_inbox_message_ids_paged` still sends `labelIds=INBOX` (refactor regression guard).
- **Command** orchestration (recency sort, clamp) is covered by the client tests + manual E2E, matching
  the project's approach for I/O commands.
- **Frontend**: no JS test runner (consistent with M4–M10). The list-aware handler refactor is verified
  by (a) inbox behavior unchanged in the maket/app and (b) actions working on search results.
- **Maket E2E (browser):** `npm run dev` → search the mock set → results render → open one.
- **Live E2E (Tauri):** search a real term (and a Gmail operator like `from:`), open a result, then
  star/archive/trash/reply from results and confirm it takes effect in Gmail; ✕ returns to the inbox.

---

## Definition of done

- A header search box runs a server-side Gmail search across all mail; results render in the two-pane,
  most-recent-first; ✕ returns to the smart inbox.
- Opening a result shows its full body and marks it read; star/archive/trash/reply work on results.
- Inbox behavior is unchanged when not searching (the list-aware refactor is behavior-preserving there).
- The app (search included) runs in a plain browser via the maket; the Tauri build is unchanged.
- New Rust code carries `// 🦀` comments; a plain-English Rust recap accompanies each Rust task.
- `cargo test` green (existing + new search tests); `cargo clippy --all-targets -- -D warnings` clean;
  `npm run build` clean. No DB migration; no new OAuth scope.

---

## Known limitations (carried as deferrals)

- Results are not cached; each search re-hits Gmail, and only the first `max` (50) results are shown.
- Ordering is by recency, not Gmail relevance.
- Acting on a search result updates Gmail and the cached DB row (if that message happens to be cached),
  but the in-memory **inbox** list isn't live-refreshed — it reconciles on the next sync.
- No search-within-folder yet (M12), no as-you-type, no match highlighting, no offline search.
