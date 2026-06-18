# Ember — Gmail daily-driver mail client (v1 design)

**Date:** 2026-06-18
**Status:** approved — design finalized from the imported `Ember Mail` Claude Design.

## Goal & motivation

A native-feeling macOS Gmail client the user switches to day-to-day, built to replace a
paid Spark subscription. Everything runs locally and talks directly to Google; there is
no backend we host, so there is nothing to subscribe to.

A second product — a meeting transcriber/summarizer for Zoom/Google Meet — is
deliberately deferred to **Phase 2** with its own spec. It shares no technical core with
the mail client and is out of scope here.

## Design source

The visual design is the imported `Ember Mail` Claude Design (`docs/Ember Mail.html`, a
self-contained "Design Component" bundle). It was extracted to
`docs/_ember_extracted/` (a `dc-runtime` engine `*.js`, the `template.html` markup +
logic, bundled fonts, and three SVG illustrations).

We do **not** ship the `dc-runtime`. The extracted `template.html` is the **reference**:
we reimplement its markup/logic as real React + TypeScript components, preserving the
exact visuals (palettes, fonts, icons, layout, spacing, animations) and wiring them to
real Gmail/Calendar data via Tauri commands instead of the prototype's mock array.

## Visual design system (source of truth = Ember Mail)

- **App identity:** "Ember." Flame motif (`ph-fill ph-flame`), warm dark UI, tagline
  *"a calmer inbox that sorts itself."*
- **Themes ("directions"):** three full dark palettes, user-switchable; default **Bloom**.
  Persisted to local prefs. Token keys: `bg0–bg3, hover, border, borderFaint, textHi,
  textMid, textLo, accent, accentSoft, onAccent, people, notif, news, radius, item,
  rowPad, avatar, tint, gap`. Exact values (carry verbatim):
  - **Ember** (warm): `accent #f2a65a`, `bg0 #181410 / bg1 #1f1a15 / bg2 #271f19`,
    people `#f0916f`, notif `#e6c167`, news `#6fb7c2`, radius 18, square-ish avatars 50%.
  - **Slate** (dense): `accent #ff7a66`, `bg0 #131312 / bg1 #191917 / bg2 #1f1f1d`,
    tight `rowPad 9px 12px`, radius 12, avatar 9px.
  - **Bloom** (vivid, default): `accent #ff8f5e`, `bg0 #16131c / bg1 #1c1824 / bg2 #241f2e`,
    people `#ff7a9c`, notif `#ffc25c`, news `#74d0bd`, radius 22, tinted rows, avatar 42%.
  - Optional `accent` prop overrides the palette accent (with a derived `accentSoft`).
- **Typography:** body `Hanken Grotesk`; display/headings `Bricolage Grotesque`. Both
  fonts are bundled locally (extracted woff2/woff/ttf) — no CDN at runtime.
- **Icons:** Phosphor Icons (`ph`, `ph-fill`, `ph-bold`), vendored locally
  (`@phosphor-icons/web` or `react`) — no CDN.
- **Layout (1280×840 reference):**
  - **Title bar** (46px): macOS traffic lights · centered title · theme-switch chips.
  - **Icon rail** (62px): flame logo · Mail/Calendar/Settings nav · add-account (+) ·
    account avatar.
  - **Mail view:** **sidebar** (236px) Compose + folder sections (Smart Inbox:
    All/People/Notifications/Newsletters · Saved: Pinned/Snoozed · Folders:
    Sent/Drafts/Archive); **list** (382px) title + search + smart-grouped rows (avatar,
    unread dot, from, pin, time, subject, preview, star/snooze actions, empty states);
    **reading pane** (flex) category chip + subject + sender + body + reply bar, with a
    top action toolbar (Reply/Forward/Snooze/Pin/Star/Archive).
  - **Calendar view:** week grid (day columns, hour gutter, events, "now" line).
  - **Settings view:** account card · appearance (3 theme cards) · Smart Inbox &
    Notifications toggles · signature textarea.
  - **Overlays:** Compose modal · Snooze popover · 4-step onboarding (Welcome →
    Providers → Connecting → Done).

## Tech stack

- **Tauri** desktop app.
- **Frontend:** React + TypeScript (Vite). Renders the UI above; email bodies render in a
  sandboxed webview.
- **Backend:** Rust (Tauri commands). Owns OAuth, Gmail + Calendar API calls, the local
  cache, the sync engine, and smart-inbox scoring. Exposes typed commands + events.
- **Storage:** local SQLite in the app data dir. OAuth tokens in the macOS Keychain.

## Architecture & modules

Each module has one purpose, a defined interface, and is independently testable.

- `auth` — Google OAuth 2.0 Desktop loopback flow; token storage/refresh via Keychain.
- `gmail` — typed Gmail REST client (list, get, history, send, modify labels, drafts).
  HTTP layer mockable for tests.
- `calendar` — typed Google Calendar client (list events, incremental sync). Read-only.
- `store` — SQLite schema + queries.
- `sync` — initial backfill + History-API incremental mail sync; calendar event sync;
  applies deltas to `store`.
- `scorer` — smart-inbox classifier: message → People | Notifications | Newsletters.
  Pure, table-tested; clean interface so a Phase-2 LLM scorer can slot in.
- `compose` — RFC822 builder (signature-aware); drafts + send; local outbox + retry.
- `snooze` — local snooze store + scheduler that re-surfaces mail at the wake time.
- `prefs` — theme, signature, smart-inbox toggles (local).
- UI (`commands` + React) — thin layer over the above; reimplements the Ember markup.

## Auth & scopes

- Google Cloud project with Gmail API + Calendar API enabled; OAuth 2.0 **Desktop**
  client; loopback (localhost) redirect. Refresh + access tokens in Keychain.
- Scopes: `gmail.modify` (read, archive, label, mark-read, send) +
  `calendar.readonly` (week view).
- **People heuristic without an extra scope:** derive "People" from Gmail's
  `CATEGORY_PERSONAL` + whether the user has sent to / replied to the sender (from the
  Sent mailbox) — avoids requesting a contacts scope. (`contacts.readonly` is a later
  option if needed.)
- Personal use: the user is a **test user** on the consent screen — no Google
  verification needed. (Verification only matters for public distribution.)

## Sync engine

- **Initial mail sync:** backfill recent mail per account (default: last 30 days, matching
  the onboarding copy) — bodies, threads, labels — into SQLite.
- **Incremental mail sync:** Gmail **History API** (`historyId`) deltas, polled every
  ~60s plus on manual refresh and window focus. New mail, label changes, deletions.
- **Calendar sync:** list events for the visible week via incremental sync tokens; refresh
  on view open and on the same poll tick. Read-only.
- **Polling, not push:** Gmail push needs a public HTTPS webhook — wrong shape for a
  local app. Minute-level polling is plenty. Push is a clean later upgrade.
- **Sending:** build RFC822 locally (append signature) → `messages.send`; drafts via the
  drafts API; a local outbox holds failed sends and retries them.

## Smart inbox — three streams

The signature feature. Every inbox message is scored into exactly one of:

- **People** — real humans. Signals: Gmail `CATEGORY_PERSONAL`, sender the user has
  sent to / replied to, direct-to-you (you in `To`), not bulk.
- **Notifications** — apps, receipts, alerts. Signals: `CATEGORY_UPDATES` /
  `CATEGORY_SOCIAL`, `no-reply`/`notifications@` senders, automated `List-Id` without
  marketing markers.
- **Newsletters** — read-when-you-have-time. Signals: `CATEGORY_PROMOTIONS` /
  `CATEGORY_FORUMS`, `List-Unsubscribe` present, bulk/marketing senders.

Transparent, tweakable rule weights; classification is instant, local, and nothing leaves
the machine. The "All" smart view shows the three groups with headers (as in the design);
per-stream views filter to one. Multiple accounts unify into these streams.

## Features wired in v1

Mail: read, threaded reading pane, compose/reply/forward, send (signature appended),
archive, delete, mark read/unread, apply/remove labels, **star** (Gmail `STARRED`),
**pin** (local), **snooze** (local + scheduler), search. Smart inbox (3 streams) +
per-stream + Pinned/Snoozed/Sent/Drafts/Archive folders. Unified inbox across accounts.
Settings: theme switch (Ember/Slate/Bloom), signature, smart-inbox/notification toggles.
**Calendar:** read-only week view from Google Calendar. **Onboarding:** the provider
screen wires the **Gmail** path to the real OAuth flow; Outlook/iCloud/IMAP are shown as
"coming soon" (disabled) in v1.

## Local state & DB schema (sketch)

`accounts(id, email, …)` · `threads(id, account_id, …)` ·
`messages(id, thread_id, account_id, from_name, from_email, to, subject, snippet,
body_html, body_text, internal_date, unread, starred, label_ids, category)` ·
`labels(account_id, id, name)` · `sync_state(account_id, history_id, calendar_sync_token)`
· `local_flags(message_id, pinned, snoozed_until, snooze_label)` ·
`calendar_events(account_id, id, title, start, end, color, …)` ·
`prefs(key, value)` (theme, signature, toggles).

## Safety & privacy

- Tokens in Keychain; DB local only; no telemetry; nothing sent anywhere except Google.
- Email HTML sanitized; scripts stripped; rendered in a sandboxed webview with a strict
  CSP. **Remote images blocked by default** (kills tracking pixels) with a per-message
  "load images" action.

## Error handling

- Token-refresh failure → re-auth prompt (re-enter onboarding for that account).
- Network failure → serve cache + "last synced X" indicator + exponential backoff.
- Send failure → keep in local outbox and retry.
- Gmail/Calendar rate limits → backoff and request batching.

## Testing

- **Rust unit tests:** sync/history-diff application, calendar sync-token handling, the
  RFC822 builder, snooze scheduler, and the smart-inbox scorer (table-driven cases for
  People/Notifications/Newsletters). Gmail/Calendar mocked at the HTTP layer
  (e.g. `wiremock`) — no real network in unit tests.
- **Frontend:** component tests (Vitest + Testing Library) for list, reading pane,
  compose, theme switching, with mocked Tauri commands.
- **Integration:** one opt-in test against a throwaway Gmail account.

## Out of scope (v1)

Send-later, templates, reminders; calendar **write/editing** (read-only only); non-Gmail
providers (shown disabled); multi-device state sync; code-signing/notarization for
distribution; the Phase 2 meeting assistant.

## Open items

1. Confirm number of Gmail accounts and the 30-day initial backfill window.
2. Snooze fidelity: local-only hide vs. also removing/re-adding the Gmail `INBOX` label at
   wake time (decide during planning).
