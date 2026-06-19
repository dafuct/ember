# Ember — Milestone 9: Settings & Onboarding (lean v1) — Design Spec

**Status:** Approved design (2026-06-19). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Give the user a **Settings** surface (account + disconnect, theme, remote-images, signature)
and a smoother **first-run** (auto-sync after connecting), unlocking two earlier deferrals (M5
remote-images, M8 signature) and filling the real gap of having no way to sign out.

**Architecture in one paragraph:** A new SQLite `settings` key-value table (additive — no
migration) holds the two backend-relevant prefs (**signature**, **remote_images**); typed
`db::get_settings`/`save_settings` apply defaults (`signature=""`, `remote_images=true`, which
preserves today's always-load behavior). Three new DB-free-ish Tauri commands sit on top:
`get_settings`, `set_settings`, and `disconnect` (the latter calls a new
`auth::tokens::delete_token` + `db::clear_account_data`). The React frontend adds a `SettingsModal`
(reusing the M8 compose-modal pattern), a header gear button, and wires the settings into existing
behavior: `ReadingPane` passes `remote_images` to `fetch_message_body`, and the compose body is
seeded with the signature. **Theme stays in `localStorage`** (pure presentation, already works);
the settings UI just wires the existing `setTheme`. Onboarding is light: after a successful connect,
the app auto-runs the first sync so the inbox populates immediately.

**Tech Stack:** Rust (rusqlite, keyring, serde, Tauri 2), React 19 + TypeScript + Vite,
lucide-react icons.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust
code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent.
After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M8 are merged to `main`. The app reads, classifies, mutates, and sends mail. It has a theme
toggle (`localStorage`, header `cycleTheme`; an exposed-but-unused `setTheme`) but **no settings
surface, no way to sign out, and no backend settings storage**. Two features were deferred pending
settings: M5's **remote-images** toggle (`fetch_message_body` already takes `load_images`, but
`ReadingPane` hardcodes `true`) and M8's **signature**. M9 adds settings + light onboarding.
Re-sequenced roadmap after M9: **M10 calendar** (read-only Google Calendar week view).

---

## Scope

**In scope (lean v1):**
- A **Settings modal**: connected account + **Disconnect**; **Theme** (light/dark); **Remote
  images** on/off; **Signature** (plain text).
- A backend **settings store** (SQLite key-value) for signature + remote_images.
- **Disconnect**: delete the Keychain token + clear the local cache (messages + sync_state),
  returning to the connect screen.
- Wire the settings into behavior: ReadingPane respects `remote_images`; new compositions seed
  the signature.
- **Onboarding**: after a successful connect, auto-run the first sync (with a "Syncing your
  inbox…" state) + a one-line explainer on the connect screen.

**Explicitly deferred (not in M9):**
- Sync-window setting (stays hardcoded 30 days).
- Auto-sync on every launch (only the first connect auto-syncs; returning users sync manually).
- Theme "system/auto" option (light/dark only).
- Per-account / multi-account settings.
- Tunable smart-inbox scorer weights.
- A multi-step onboarding wizard.

---

## Components & contracts

### Backend — `src/db/mod.rs`

New additive table in `init()` (alongside the existing `CREATE TABLE IF NOT EXISTS` calls — **no
migration, no cache wipe**):
```sql
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```
Typed accessors over the key-value table:
```rust
pub struct Settings { pub signature: String, pub remote_images: bool }

/// Read settings, applying defaults for absent keys (signature="", remote_images=true).
pub fn get_settings(conn: &Connection) -> Result<Settings>;
/// Persist both settings (UPSERT each key). remote_images stored as "1"/"0".
pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()>;
/// Clear the local mail cache on disconnect: delete all rows from `messages` and
/// `sync_state`. Settings (user prefs) are intentionally kept.
pub fn clear_account_data(conn: &Connection) -> Result<()>;
```
`Settings` is defined here (or a small shared module) and is `Serialize + Deserialize` so it
crosses the Tauri boundary directly.

### Backend — `src/auth/tokens.rs`
```rust
/// Remove the stored token for `account` from the Keychain. A missing entry is not an error.
pub fn delete_token(account: &str) -> Result<()>;
```
Uses `keyring::Entry::new(...)?.delete_credential()`; treats `keyring::Error::NoEntry` as `Ok(())`
(idempotent), mirroring `load_token`'s NoEntry handling.

### Backend — `src/commands.rs` + `src/lib.rs`
```rust
#[tauri::command] pub async fn get_settings(state: State<'_, Db>) -> Result<Settings>;
#[tauri::command] pub async fn set_settings(settings: Settings, state: State<'_, Db>) -> Result<()>;
#[tauri::command] pub async fn disconnect(state: State<'_, Db>) -> Result<()>;
```
- `get_settings`/`set_settings` lock the DB and delegate to `db::get_settings`/`save_settings`.
- `disconnect`: `auth::tokens::delete_token(PRIMARY_ACCOUNT)?` then lock + `db::clear_account_data`.
- The `MutexGuard` is taken in an await-free block (no `.await` while held), per the existing
  convention. All three registered in `lib.rs`.

`send_email` and `fetch_message_body` are **unchanged** — the frontend consumes the settings.

### Frontend
- `lib/api.ts`: `interface Settings { signature: string; remote_images: boolean }` + wrappers
  `getSettings()`, `setSettings(s)` (`invoke("set_settings", { settings: s })`), `disconnect()`.
- `lib/compose.ts`: pure `appendSignature(body: string, signature: string): string` — returns
  `body` unchanged when signature is empty, else `body + "\n\n-- \n" + signature`.
- `components/SettingsModal.tsx` (NEW): modal (reuses `.compose-overlay`/card pattern, `role=dialog`,
  Esc/Close, no backdrop-close) with: **Account** row (the email + a **Disconnect** button that
  reveals an inline "Disconnect? This clears the local cache." confirm before calling
  `disconnect`); **Theme** (light/dark, via `useTheme().setTheme`); **Remote images** toggle;
  **Signature** `<textarea>`. **Save** persists via `setSettings` (errors keep the modal open);
  **Disconnect** calls a passed `onDisconnect`.
- `App.tsx`:
  - `settings` state (`{ signature, remote_images }`) fetched via `getSettings()` on mount (after
    account is known); passed to ReadingPane and used when seeding compose bodies.
  - `settingsOpen` state; a gear button (Header) opens the `SettingsModal`; on Save, refresh
    `settings` state.
  - `handleConnect`: after `setAccount(await connectGmail())`, **auto-run the first sync**
    (reuse `handleSync`) and fetch settings — the inbox populates without a manual Sync.
  - `handleDisconnect`: `await disconnect()` → `setAccount(null)`, `setMessages([])`,
    `setSelectedId(null)`, close the modal → connect screen.
  - `openNewCompose`/`handleReply` seed the body via `appendSignature` (new compose: signature
    block; reply: `quoteBody(...)` then `appendSignature`).
- `components/Header.tsx`: a gear **Settings** button (opens the modal); optional `onSettings`.
- `components/ReadingPane.tsx`: take a `loadImages: boolean` prop; call
  `fetchMessageBody(msg.id, loadImages)` instead of the hardcoded `true`. (Re-fetch is keyed on
  `[msg?.id, loadImages]` so toggling the setting re-renders the open message with the new pref.)
- Connect screen (App.tsx): a one-line explainer under the welcome; while connecting/auto-syncing,
  a "Connecting…" / "Syncing your inbox…" status.
- `styles/app.css`: settings rows + a toggle/switch style (reuse the compose modal overlay/card).

---

## Data flow

**Open settings:** gear → `SettingsModal` seeded from App's `settings` state + `useTheme`. **Save**
→ `setSettings({signature, remote_images})` → on success App refreshes `settings` (so ReadingPane
+ compose pick up the change). Theme changes apply immediately via `setTheme` (localStorage).

**Disconnect:** Disconnect → inline confirm → `disconnect()` (delete token + clear cache) → App
resets to the connect screen.

**First connect (onboarding):** Connect → `connectGmail()` succeeds → `setAccount` → auto
`handleSync()` (status "Syncing your inbox…") + `getSettings()` → inbox shows.

**Remote images:** ReadingPane fetches the body with `settings.remote_images`; with it off, the
sanitizer strips remote images (`blocked_images` true) — the existing M5 path.

**Signature:** with a signature set, opening compose seeds the body with the signature block; the
user sees and can edit it before sending. `send_email` is unchanged (the body already contains it).

---

## Error handling

- `set_settings` failure → the modal stays open with an inline error; fields preserved.
- `disconnect` failure → inline error in the modal; the user stays connected.
- Onboarding auto-sync failure → surfaced in the existing error bar, but the account stays
  connected (token saved) — the user can sync manually.
- `get_settings` failure on mount → fall back to defaults (`signature=""`, `remote_images=true`)
  so the app still works.
- The DB `MutexGuard` is never held across `.await` (existing convention).

---

## Testing strategy

- `db` tests: `get_settings` returns defaults when the table is empty; `save_settings` →
  `get_settings` round-trips signature + remote_images (incl. the "1"/"0" bool encoding);
  `clear_account_data` removes `messages` + `sync_state` rows but leaves `settings` intact.
- `compose.ts` `appendSignature`: empty signature → body unchanged; non-empty → appends the
  `-- ` block. Pure (testable once Vitest lands; none now — consistent with M4–M8).
- `auth::tokens::delete_token` + the three commands are covered by **manual E2E** (keychain +
  live state), consistent with the project's other I/O commands.
- **Manual E2E:** switch theme; toggle remote images and confirm ReadingPane re-fetches
  (images blocked when off); set a signature and confirm it appears in a new compose and a reply;
  Disconnect → returns to the connect screen and the cache is cleared; reconnect → auto-sync
  populates the inbox.

---

## Definition of done

- Settings modal works: account + Disconnect, theme, remote-images, signature; Save persists and
  takes effect (ReadingPane respects remote-images; compose seeds the signature).
- `disconnect` clears the Keychain token + local cache and returns to the connect screen (verified
  live); reconnecting works and auto-syncs.
- Onboarding: first connect auto-syncs; the connect screen has a one-line explainer.
- New Rust code carries `// 🦀` comments; a plain-English Rust recap accompanies each task.
- `cargo test` green (existing + new db tests); `cargo clippy --all-targets -D warnings` clean;
  `npm run build` clean. (`cargo fmt` is **not** used in this repo — not a gate.)
- **Additive `settings` table only — no destructive migration / no cache wipe.**

---

## Known limitations (carried as deferrals)

- Single-account; settings are global (not per-account). Disconnect keeps the prefs (signature/
  images) for the next account.
- Theme has no "system/auto" option and remains in `localStorage` (not in the backend store).
- Only the first connect auto-syncs; returning-user launches load the cached inbox and sync
  manually.
- Signature placement is a fixed `-- ` block appended to the seeded body; no rich/HTML signature.
