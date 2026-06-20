# Ember — Milestone 13: New-mail notifications (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** While Ember is open, periodically background-sync the INBOX and raise a **native macOS notification** for each genuinely-new inbox message (sender + subject). Clicking a notification focuses Ember and opens that message. Notifications are **suppressed while the window is focused**, **capped** per cycle, and **toggleable in Settings** (default on). Last of the M11→M12→M13 sequence. **No new OAuth scope, no DB migration.**

**Architecture in one paragraph:** A React `setInterval` (~60s) in `App.tsx` drives a **background poll** — active only when an account is connected AND notifications are enabled. Each tick reuses the existing sync path (`syncInbox()` → `fetchInboxPreview()`), then runs a **pure new-mail diff**: the set of currently-known inbox message ids is held in a ref, seeded on first load (so the initial/full-resync burst never notifies); any id not in that set is "fresh". Fresh messages raise one native banner each — **only when the window is unfocused**, newest-first, capped — via a thin `lib/notify.ts` wrapper over `tauri-plugin-notification`. Clicking a banner focuses the window and routes the message id back into App (Mail view → inbox → `all` stream → select). New-mail detection is **frontend-only** (an id-diff of the preview list) — `sync_inbox` is untouched. The only backend change is an **additive `notifications` settings field** (key-value table, default true, no migration) plus registering the notification plugin. The Tauri build and the `isTauri()` browser maket both keep working: the notify wrapper no-ops outside Tauri.

**Tech Stack:** Rust (Tauri 2 + `tauri-plugin-notification`; rusqlite), React 19 + TypeScript + Vite, `@tauri-apps/plugin-notification`, `@tauri-apps/api/window` (focus).

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M12 are merged to `main`. Ember reads/classifies/mutates/sends mail, has settings + disconnect (M9), a read-only calendar (M10), server-side search (M11), and folder/Sent views (M12). Today the inbox only updates when the user clicks **Sync** or on connect — there is **no background timer**, so nothing notifies the user of new mail. M13 adds that. It is the third of three sequenced milestones (**M11 search → M12 folders → M13 notifications**).

**Why poll, not push:** Gmail's real push (`users.watch` + Cloud Pub/Sub) requires a Pub/Sub topic and a publicly reachable delivery endpoint. Ember is **serverless by design** (the desktop app talks directly to Google; it has no server to receive webhooks), so push is architecturally precluded. Detection must be poll-based, and the natural vehicle is the existing M3 History-API delta sync, run on a timer.

---

## Scope

**In scope (lean v1):**
- A **background poll**: a ~60s `setInterval` that reuses the existing sync path while the app is open. Gated on *connected AND notifications enabled*.
- **Genuinely-new detection** via a pure frontend id-diff of the inbox preview list (seeded on first load so the initial backlog never notifies).
- **Native macOS notifications**: one per fresh message (sender + subject), newest-first, **capped at 5 per cycle**.
- **Focus suppression**: no banner while Ember's window is already focused.
- **Click-to-open**: clicking a banner focuses Ember and opens that message (with a focus-only fallback — see Known risks).
- A **Settings toggle** ("New-mail notifications", default on), additive to the settings table (no migration), requesting OS permission when enabled.
- The notify wrapper **no-ops outside Tauri** so the browser maket keeps working.

**Explicitly deferred (not in M13):**
- **Category filtering** — the owner chose to notify on *all* new inbox mail. The scorer category is on every preview, so restricting to People/Notifications later is a one-line filter; not built now.
- **Coalesced summaries** ("3 new messages") and **banner action buttons** (archive / mark-read from the notification).
- **Notifying while the app is fully closed** — needs a macOS launch agent / always-on helper; Ember only notifies while running.
- **Configurable cadence**; **per-account** notifications.
- **Decoupling background refresh from notifications** (today, turning notifications off also stops the background poll — see Decisions).
- **Gmail push (watch + Pub/Sub)** — architecturally precluded (no server).
- **Unread-only filtering** — new inbox arrivals are already effectively unread; redundant.

---

## Components & data flow

### 1. Background poll (`App.tsx`)
- Constants: `POLL_MS = 60_000`, `MAX_NOTIFY_PER_SYNC = 5`.
- A `useEffect` sets up a `setInterval` **only when** `account != null && settings.notifications`. The effect's cleanup clears the interval; it re-runs (tears down + rebuilds) when those deps change, so disconnect / toggle-off / unmount all stop the timer.
- Each tick calls a shared **`runSync()`** extracted from the current `handleSync` so the manual button and the timer share one code path. `runSync()` is guarded by a `syncingRef` (boolean ref): if a sync is already in flight, the tick is skipped (no overlapping syncs).
- **Error handling:** a *background* sync failure is logged (console) and otherwise swallowed — it must **not** hijack the global error bar (that would flash an error every 60s while offline). The manual Sync button keeps surfacing errors as today. `runSync()` therefore takes a flag (e.g. `{ surfaceErrors }`) — true for the button, false for the timer.

### 2. New-mail diff (pure function)
- `pickNewMail(known: Set<string>, list: MessagePreview[], cap: number): MessagePreview[]` — returns the messages in `list` whose id is not in `known`, newest-first (by `internal_date`), truncated to `cap`. Pure, no Tauri, unit-reasoned.
- `knownIdsRef: Set<string>` in `App.tsx`, **seeded** from the inbox on first load: the mount `fetchInboxPreview` effect and the `handleConnect` onboarding sync both call a `seedKnown(list)` that fills the set **without notifying**.
- After every `runSync()`'s `fetchInboxPreview`, compute `fresh = pickNewMail(known, list, MAX_NOTIFY_PER_SYNC)`, then **always** update `known` to the full current id set (even when suppressed/capped) so a message is never notified twice. If `document.hasFocus()` is false and notifications are enabled, raise a banner per `fresh`. Overflow beyond the cap is logged, not shown.

### 3. Notification wrapper (`src/lib/notify.ts`)
- `ensureNotificationPermission(): Promise<boolean>` — `isPermissionGranted()`, else `requestPermission()`, mapping the result to a bool. No-op `true`/`false` semantics outside Tauri.
- `notifyNewMail(m: MessagePreview): Promise<void>` — `sendNotification({ title: displayName(m.from), body: m.subject })`, carrying the message id for click routing. Wrapped in try/catch (one failure must not break the loop). **No-ops (console.debug) when `!isTauri()`** — the maket exercises the diff loop without raising real banners.
- **Click-to-open:** registered once at startup. On click: focus the main window (`getCurrentWindow().setFocus()`) and route the message id into App via a callback/handler that switches to Mail view + inbox folder + `all` stream and selects the message.

### 4. Native plugin wiring
- `src-tauri/Cargo.toml`: `tauri-plugin-notification = "2"`.
- `src-tauri/src/lib.rs`: `.plugin(tauri_plugin_notification::init())` in the builder chain.
- `src-tauri/capabilities/default.json`: add `"notification:default"` to permissions; add the window-focus permission (e.g. `core:window:allow-set-focus`) if it is not already covered by `core:default`.
- `package.json`: `@tauri-apps/plugin-notification`.

### 5. Settings toggle (additive, no migration)
- `db::Settings` (Rust) gains `pub notifications: bool`. `get_settings` defaults it to `true` when the key is absent (`get_setting_raw("notifications").map(|v| v == "1").unwrap_or(true)`); `save_settings` writes `"1"/"0"` under key `"notifications"` inside the existing transaction.
- TS `Settings` interface (`src/lib/api.ts`) gains `notifications: boolean`; `SettingsModal` gains a "New-mail notifications" toggle cloned from the remote-images toggle. Toggling it **on** calls `ensureNotificationPermission()`; a denied result is reflected (the toggle shows that the OS blocked it) and no banners fire.

### Data flow
`timer tick → runSync(surfaceErrors:false) → syncInbox() (Gmail history delta) → fetchInboxPreview(50) → setMessages → fresh = pickNewMail(known, list, 5) → (window unfocused?) notifyNewMail(each fresh) → known ← all current ids`.
`banner click → focus window → select message (Mail · inbox · all stream)`.

---

## Error handling

- **Background sync failure** (offline, auth expired): logged, swallowed; the error bar is untouched; the next tick retries. (Auth-expired still surfaces normally on the next *manual* action.)
- **Permission denied:** the Settings toggle reflects the blocked state; `notifyNewMail` no-ops; nothing crashes.
- **`notifyNewMail` throw:** caught per-message; the loop continues with the remaining fresh messages.
- **Overlapping ticks:** prevented by `syncingRef`.
- **Outside Tauri (maket):** every OS call no-ops; the diff loop still runs (console.debug), so the maket is exercisable.

---

## Testing

- **Rust:** extend the existing `get_settings`/`save_settings` unit test(s) in `db/mod.rs` for the new `notifications` field — defaults to `true` when unset, and round-trips `true`/`false`. This is the only meaningfully backend-testable surface (plugin registration and the JS glue are integration-only).
- **Frontend:** the new-mail logic lives entirely in the pure `pickNewMail`. The project has **no TS test harness** (vitest was deferred at M10 and never added), so — consistent with M10–M12 — `pickNewMail` is kept obviously-correct and pure, reasoned through rather than unit-tested, and the loop is verified via the **browser maket** (console.debug proof of seeding + diff + suppression) and a screenshot of the Settings toggle.
- **Live E2E** (a real macOS banner from real new mail, permission prompt, click-to-open) is **pending the owner's account** — same live-pending status as M10–M12.
- `cargo test` and `cargo clippy` stay green; the existing 65 tests keep passing.

---

## Known risks & decisions

- **Click-to-open API (the one real risk):** the exact Tauri v2 `tauri-plugin-notification` *click/action callback* behavior on macOS desktop must be verified. The implementation plan's **first task is a short spike** to confirm whether a notification click delivers our message-id payload to JS. If it does → full click-to-open. If it does not (or only on a build, not in dev) → **degrade to focus-only**: clicking activates Ember (OS default) and we do not deep-link. Focus-only is an acceptable lean v1 outcome; per-message routing is the bonus.
- **Background poll gated on the notifications toggle** (decision): turning notifications off stops the timer entirely, so with the feature off the app behaves exactly as it does today (manual sync only). This keeps M13 a clean opt-in and respects M9's deferral of "auto-sync every launch". Decoupling a silent background refresh from banners is deferred.
- **Un-archive / label-restore edge:** a message that re-enters the INBOX (e.g. user un-archives it elsewhere) has an id not in `known`, so it would count as "fresh". This is rare and accepted for v1; the cap bounds any surprise. (A timestamp high-water guard could suppress it later.)
- **Backlog after a long closure:** the first unfocused tick after the app was closed for a while can fire up to the cap for mail that arrived while closed. Accepted — it is genuinely unseen mail, and the cap (5) bounds the burst.
- **In-memory watermark:** `knownIdsRef` is per-session; on relaunch it is re-seeded from the current inbox, so already-present mail never re-notifies across restarts.

---

## Non-goals / constraints

- **No new OAuth scope** — reuses the existing sync.
- **No DB migration** — the `notifications` setting is an additive key in the existing key-value table.
- **Tauri build unchanged for the maket** — the notify wrapper is `isTauri()`-gated end to end.
- Desktop-only behavior: Ember notifies **only while running** (no closed-app notifications in v1).
