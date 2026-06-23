# Multi-Account Support — Design Spec

**Date:** 2026-06-23
**Status:** Approved (brainstorming) — pending implementation plan
**Author:** Brainstorming session (Ember / ownmail)

## Summary

Add the ability to connect several Google accounts and switch the active one. The
switch applies to the whole app — Gmail (inbox, folders, labels, snoozed) and Google
Calendar (week view, events) and local meeting notes. Exactly one account is "active"
at a time and the entire UI reflects that account. New mail for **all** connected
accounts is polled in the background and surfaces native notifications tagged by
account, even while another account is active.

## Decisions (from brainstorming)

1. **Core model: switch one at a time.** One active account; the whole app shows it.
   No unified/combined inbox.
2. **Notifications: all accounts in background.** Background new-mail polling and
   notifications run for every connected account, not just the active one.
3. **Switcher UI: avatar popover.** Clicking the bottom avatar in the icon rail opens
   an account popover (list + active marker + unread + "Add account" + "Manage in
   Settings"). Full add/remove management also lives in Settings.
4. **Storage strategy: account-scoped shared cache (Approach A).** Tokens keyed by
   email; one shared SQLite DB with an `account` column on the cache tables; UI filters
   to the active account. Chosen over per-account DB files (fights the single-connection
   state model, awkward background polling) and over wipe-and-resync-on-switch (can't do
   all-accounts background notifications; destroys local snoozed/notes; ruled out).

## Current architecture (single account)

- **Tokens:** macOS Keychain, service `dev.ember.oauth`, stored under a fixed key
  `PRIMARY_ACCOUNT = "primary"` (`src-tauri/src/auth/tokens.rs`,
  `src-tauri/src/auth/mod.rs`). `StoredToken` already carries `email`.
- **Token chokepoint:** `ensure_access_token()` (`auth/mod.rs`) loads the `"primary"`
  token, refreshes it if expired, and is called by ~30 commands — every Gmail and
  Calendar network command flows through it.
- **Local cache (`src-tauri/src/db/mod.rs`):** `messages`, `snoozed`, `meeting_notes`
  have **no account column** (single-account assumption). `sync_state` is keyed by an
  account string but that string is always `"primary"`. `settings` is a key/value table.
- **Connect/disconnect:** `connect_gmail` saves under `"primary"`; `disconnect` deletes
  the `"primary"` token and calls `clear_account_data` (wipes `messages` + `sync_state`;
  keeps `settings` and `meeting_notes`; never touches `snoozed`).
- **Frontend:** `App.tsx` holds `account: string | null`. The bottom avatar opens
  `SettingsModal`, which shows the account and a single "Disconnect account" button.
- **Maket:** `src/lib/mock.ts` exports a hardcoded `MOCK_ACCOUNT`; `getConnectedAccount`
  is `isTauri`-gated to return it in the browser.

## Design

### 1. Per-account token storage

- Store tokens in the Keychain keyed by **email** (`save_token(&email, …)`,
  `load_token(&email)`, `delete_token(&email)`), replacing the fixed `"primary"` key.
  `StoredToken` is unchanged.
- The Keychain cannot enumerate entries, so maintain an explicit index in the existing
  `settings` table:
  - `accounts` → JSON array of connected emails, e.g. `["a@gmail.com","b@gmail.com"]`.
  - `active_account` → the active email string.
- New db helpers (in `db/mod.rs`, on top of the existing settings key/value store):
  `get_accounts() -> Vec<String>`, `add_account(email)`, `remove_account(email)`,
  `get_active_account() -> Option<String>`, `set_active_account(email)`.

### 2. Token resolution chokepoint

Split `ensure_access_token()` into:

- `ensure_active_token() -> Result<StoredToken>` — resolves the active email from the
  `active_account` pointer, then loads + refreshes that account's token. This single
  change makes **all** Gmail and Calendar network commands switch accounts
  automatically; they need no per-command edits beyond calling this.
- `ensure_token_for(email: &str) -> Result<StoredToken>` — load + refresh a *specific*
  account's token. Used by the all-accounts background loop.

DB-touching commands already receive the active email for free: `ensure_active_token()`
returns the `StoredToken`, and `stored.email` is passed straight into the scoped db
calls — no extra lookup.

### 3. Local cache scoping (additive SQLite migration)

This project uses manual, additive SQLite migrations (`add_column_if_missing`), not
Flyway. Safe-DDL spirit still applies: add columns NOT NULL **with** a DEFAULT.

- Add `account TEXT NOT NULL DEFAULT ''` to `messages`, `snoozed`, `meeting_notes`.
- **Backfill (one-time):** read the existing `"primary"` token's email during
  migration and `UPDATE … SET account = <email> WHERE account = ''` on each table, so
  existing users' cached data is stamped with their single current account.
- `sync_state` is already account-keyed; its key changes from `"primary"` to the email.
- All reads on these tables gain `WHERE account = ?1`; all writes stamp the column.
- Replace `idx_messages_internal_date` usage with a composite
  `idx_messages_account_internal_date ON messages(account, internal_date DESC)` so the
  per-account inbox query stays index-covered.
- `clear_account_data` is generalized into per-account removal (see §8).

### 4. Add-account flow

- "Add account" reuses the existing `connect_gmail` OAuth flow (PKCE + loopback;
  already sends `access_type=offline` + `prompt=consent`, so Google shows the account
  chooser and returns a fresh refresh token).
- After fetching the email via `get_profile`, the flow:
  `save_token(email)` → `add_account(email)` (index) → `set_active_account(email)`.
  A newly added account becomes active.
- Idempotent: re-adding an already-connected email refreshes its token and re-activates
  it without duplicating the index entry.

### 5. Account switching

- New command `set_active_account(email)` — validates the email is in the index, flips
  the `active_account` pointer. Instant: no network, no cache wipe.
- `get_connected_account()` now returns the **active** email (from the pointer),
  preserving the existing single-string contract so most of the app is unchanged.
- New command `list_accounts() -> Vec<AccountInfo>` where
  `AccountInfo { email, active, unread }` (`unread` = count of unread cached rows for
  that account) — feeds the switcher popover.
- Frontend switch sequence: `set_active_account` → bump a global **account-epoch** key
  (mirrors the existing `folderReloadKey` pattern) → mail list, labels, and the calendar
  view re-fetch. Calendar/event data is live (DB-free), so it switches purely via the
  new active token.

### 6. All-accounts background sync + notifications

- New command `sync_all_accounts() -> Vec<AccountSyncSummary>` — loops every email in
  the index, runs the **existing** history-delta / full-resync logic per account into
  the scoped cache (stamping `account = email`), and returns per-account summaries
  including the newly-added previews.
- **Notification source changes from frontend-diff to backend delta.** Today the
  frontend diffs the visible list to detect new mail, which only works for the active
  account. Instead, notifications key off each account's sync **delta `added_ids`**
  (already computed by the backend), which works uniformly for every account.
- The frontend's existing ~60s background timer calls `sync_all_accounts()` instead of
  single-account sync. For each account's new previews it fires a native notification
  **tagged with the account** (e.g. title `New mail · b@gmail.com`). For the active
  account it also refreshes the visible list, as today.
- Per-account `sync_state` (historyId) keeps each account's delta cursor independent, so
  the loop stays cheap (deltas, not full pulls) after each account's first sync.
- **Baseline suppression:** an account's first sync (no stored historyId yet) only
  establishes the baseline `sync_state` and must **not** emit notifications — otherwise
  adding an account would fire ~30 days of "new mail" alerts. Notifications fire only for
  deltas computed against an existing historyId. `sync_all_accounts` flags whether each
  per-account result came from a baseline run so the frontend suppresses it.
- Gating unchanged: the loop runs only while notifications are enabled in Settings; each
  account's token is refreshed on demand inside the loop via `ensure_token_for`.
- A newly added account performs a one-time full ~30-day resync in the background; a
  subtle per-account spinner in the switcher indicates this (non-blocking).

### 7. Avatar popover (switcher UI)

- New component `AccountSwitcher` — a popover anchored to the bottom avatar in
  `IconRail`. Clicking the avatar opens it (instead of opening Settings directly).
- Contents:
  - One row per connected account: initials/email, a **✓ on the active one**, an unread
    badge, and a per-account spinner during first resync. Clicking a non-active row →
    `set_active_account` + account-epoch reload.
  - **+ Add account** → runs `connect_gmail`.
  - **Manage in Settings** → opens the existing `SettingsModal`.
- The avatar's initials reflect the **active** account.

### 8. App state & Settings

- `App.tsx`: keep `account` as the active email; add `accounts: AccountInfo[]` state.
  Load `list_accounts()` on mount and after any add/switch/remove. The account-epoch key
  drives mail + calendar reloads on switch.
- `SettingsModal`: the Account section becomes a **list** of connected accounts, each
  with a **Remove** button (replacing the single "Disconnect account").
  - New command `remove_account(email)` (generalizes today's `disconnect`): delete the
    account's Keychain token, delete its scoped cache rows
    (`messages`/`snoozed`/`meeting_notes`/`sync_state` WHERE `account = email`), and drop
    it from the index.
  - Removing the **active** account → auto-switch to another connected account.
  - Removing the **last** account → return to the connect/onboarding screen.
- Connect/onboarding screen (zero accounts): unchanged.

### 9. Maket / mock & testing

- Mock (`src/lib/mock.ts`): `MOCK_ACCOUNT` → `MOCK_ACCOUNTS` (2–3 emails) with mock
  `list_accounts` / `set_active_account` and per-account mock messages, all `isTauri`-
  gated in `api.ts`. Switching in the browser maket flips the mock active account and
  returns that account's messages, keeping the feature browser-testable.
- Rust tests: new db helpers (accounts index round-trip, active pointer, scoped
  reads/writes, migration backfill stamps the existing account), per-email token
  save/load/delete.
- Frontend: verify live in the maket — popover lists accounts; switching flips the
  inbox + calendar; add-account; remove-account (including removing the active account
  and the last account).
- The pre-existing maket gap (ungated `invoke` in a few action wrappers) stays out of
  scope.

## Components and boundaries

| Unit | Responsibility | Depends on |
| --- | --- | --- |
| `auth/tokens.rs` | Keychain save/load/delete keyed by email | keyring |
| `auth/mod.rs` | OAuth connect; `ensure_active_token` / `ensure_token_for` | tokens, settings index |
| `db/mod.rs` (accounts) | accounts index + active pointer helpers | settings table |
| `db/mod.rs` (cache) | account-scoped queries + migration/backfill | sqlite |
| `commands.rs` | `connect_gmail`, `list_accounts`, `set_active_account`, `remove_account`, `sync_all_accounts`, `get_connected_account` | auth, db, gmail, calendar |
| `AccountSwitcher.tsx` | popover: list, switch, add, manage | api (`list_accounts`, `set_active_account`, `connect_gmail`) |
| `App.tsx` | active account + accounts state; account-epoch reload; all-accounts notify loop | api, AccountSwitcher, SettingsModal |
| `SettingsModal.tsx` | per-account list + remove | api (`remove_account`) |
| `mock.ts` / `api.ts` | multi-account maket | — |

## Out of scope

- Unified/combined inbox.
- Non-Google accounts (IMAP, Outlook, etc.).
- Per-account settings/signatures (signature stays a single global setting for now).
- Fixing the pre-existing ungated-`invoke` maket gap.

## Risks / notes

- **Migration backfill** must run before any scoped query reads, and must be idempotent
  (`WHERE account = ''`). Existing single-account users must land seamlessly with their
  account stamped and active.
- **Notification model change** (frontend-diff → backend-delta) is the subtlest part;
  must avoid duplicate or missed notifications across the loop. Per-account `sync_state`
  is the cursor of record.
- **Background loop cost** scales with account count; deltas keep it cheap after first
  sync. First-sync-per-account is the only heavy step.
