# Ember — Milestone 9: Settings & Onboarding (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Settings surface (account + disconnect, theme, remote-images, signature) and light onboarding (auto-sync on first connect), unlocking the M5 remote-images and M8 signature deferrals and filling the no-way-to-sign-out gap.

**Architecture:** A new additive SQLite `settings` key-value table (no migration) holds signature + remote_images; typed `db::get_settings`/`save_settings` apply defaults. Three Tauri commands (`get_settings`, `set_settings`, `disconnect`) sit on top; `disconnect` also calls a new `auth::tokens::delete_token` + `db::clear_account_data`. The React frontend adds a `SettingsModal` (reusing the M8 compose-modal pattern), a header gear button, and wires settings into existing behavior (ReadingPane reads `remote_images`; compose seeds the signature). Theme stays in localStorage; the UI wires the existing `setTheme`.

**Tech Stack:** Rust (rusqlite, keyring 3, serde, Tauri 2), React 19 + TypeScript + Vite, lucide-react.

**Design source:** `docs/superpowers/specs/2026-06-19-ember-m9-settings-onboarding-design.md` (approved).

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

**Environment:** `cargo` at `/opt/homebrew/bin/cargo`; backend commands run from `src-tauri/`. Frontend `npm run build` from the repo ROOT. **`cargo fmt` is NOT used in this repo** (no config/CI, hand-formatted) — do not run it; not a gate. **Do NOT run `git checkout`/`switch`/`restore`** — stay on the `m9-settings-onboarding` branch (reviewers: inspect with read-only `git show <sha>` only).

---

## Milestone context

M1–M8 are merged. The app reads/classifies/mutates/sends mail; theme works (localStorage) but there's no settings surface, no sign-out, and no backend settings storage. M9 adds settings + light onboarding. **Additive `settings` table only — no destructive migration / no cache wipe.** Next: M10 calendar.

---

## File structure

**Backend (`src-tauri/`):**
- `src/db/mod.rs` — `Settings` struct; `settings` table in `init()`; `get_settings`/`save_settings`/`clear_account_data` + tests.
- `src/auth/tokens.rs` — `delete_token(account)`.
- `src/commands.rs` — `get_settings`/`set_settings`/`disconnect` commands.
- `src/lib.rs` — register the three commands.

**Frontend (`src/`):**
- `lib/api.ts` — `Settings` type + `getSettings`/`setSettings`/`disconnect` wrappers.
- `lib/compose.ts` — pure `appendSignature`.
- `components/SettingsModal.tsx` — **NEW.** The settings modal.
- `App.tsx` — settings state, modal, gear wiring, disconnect, signature seeding, auto-sync on connect.
- `components/Header.tsx` — gear Settings button.
- `components/ReadingPane.tsx` — `loadImages` prop.
- `styles/app.css` — settings + toggle styles.

---

## Task 1: DB settings store + `clear_account_data`

**Files:**
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: Write the failing tests.** Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/db/mod.rs`:

```rust
    #[test]
    fn get_settings_returns_defaults_when_empty() {
        let c = conn();
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "");
        assert!(s.remote_images); // default: load images (preserves pre-M9 behavior)
    }

    #[test]
    fn save_then_get_settings_round_trips() {
        let c = conn();
        save_settings(&c, &Settings { signature: "Cheers,\nDmytro".into(), remote_images: false }).unwrap();
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "Cheers,\nDmytro");
        assert!(!s.remote_images);
    }

    #[test]
    fn clear_account_data_wipes_cache_but_keeps_settings() {
        let c = conn();
        upsert_messages(&c, &[msg("a", 1)]).unwrap();
        set_sync_state(&c, "primary", Some(7), 1).unwrap();
        save_settings(&c, &Settings { signature: "sig".into(), remote_images: false }).unwrap();

        clear_account_data(&c).unwrap();

        assert_eq!(recent_previews(&c, 10).unwrap().len(), 0);
        assert_eq!(get_sync_state(&c, "primary").unwrap(), None);
        // settings survive a disconnect (they're user prefs, not account cache)
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "sig");
        assert!(!s.remote_images);
    }
```

- [ ] **Step 2: Run them, verify they FAIL** (`cannot find ... Settings` / functions):

Run: `cd src-tauri && cargo test --lib get_settings_returns_defaults_when_empty save_then_get_settings_round_trips clear_account_data_wipes_cache_but_keeps_settings`
Expected: FAIL (unresolved `Settings`/`get_settings`/`save_settings`/`clear_account_data`).

- [ ] **Step 3a: Add the `Settings` struct** near the top of `src-tauri/src/db/mod.rs` (after the `use` lines, beside `StoredMessage`):

```rust
// 🦀 App settings. `#[derive(Serialize, Deserialize)]` lets this cross the Tauri IPC
//    boundary directly (the frontend reads/writes the same shape). Stored in the
//    key-value `settings` table, one row per field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub signature: String,
    pub remote_images: bool,
}
```

- [ ] **Step 3b: Create the `settings` table** in `init()`. In the `conn.execute_batch("...")` call that creates `messages`/`sync_state`, add this table to the batch (append before the closing `"`, after the `sync_state` CREATE):

```sql
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
```

This is additive (`IF NOT EXISTS`) — no migration, no cache wipe.

- [ ] **Step 3c: Add the accessors** after `set_sync_state` (near the end of the non-test code):

```rust
// 🦀 Read one settings row's value, if present. Private helper for get_settings.
fn get_setting_raw(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

// 🦀 UPSERT one settings row (INSERT, or overwrite the value on key conflict).
fn set_setting_raw(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Read app settings, applying defaults for absent keys: empty signature, and
/// remote_images = true (which preserves the pre-M9 always-load-images behavior).
pub fn get_settings(conn: &Connection) -> Result<Settings> {
    let signature = get_setting_raw(conn, "signature")?.unwrap_or_default();
    // 🦀 Stored as "1"/"0"; default to true when the key was never written.
    let remote_images = get_setting_raw(conn, "remote_images")?
        .map(|v| v == "1")
        .unwrap_or(true);
    Ok(Settings { signature, remote_images })
}

/// Persist both settings in one transaction.
pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    set_setting_raw(&tx, "signature", &s.signature)?;
    set_setting_raw(&tx, "remote_images", if s.remote_images { "1" } else { "0" })?;
    tx.commit()?;
    Ok(())
}

/// Clear the local mail cache on disconnect: all `messages` and `sync_state` rows.
/// `settings` (user prefs) are intentionally kept.
pub fn clear_account_data(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM messages", [])?;
    tx.execute("DELETE FROM sync_state", [])?;
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4: Run the tests, verify they PASS**, then the whole lib suite:

Run: `cd src-tauri && cargo test --lib get_settings_returns_defaults_when_empty save_then_get_settings_round_trips clear_account_data_wipes_cache_but_keeps_settings`
Run: `cd src-tauri && cargo test --lib`
Expected: PASS, no regressions.

- [ ] **Step 5: Lint + commit.**

```bash
cd src-tauri && cargo clippy --lib --all-targets -- -D warnings
```
```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(db): add settings store + clear_account_data

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

**Rust recap:** a serde-derived struct crossing the IPC boundary; key-value persistence with `ON CONFLICT DO UPDATE`; `Option::map(...).unwrap_or(default)` for a defaulted bool; one-transaction multi-write.

---

## Task 2: `delete_token` + the three commands + registration

No automated test (keychain + live state, like the other I/O commands). The DB layer is tested in Task 1. Gate: `cargo build` + `cargo clippy --all-targets -- -D warnings` clean + `cargo test` green.

**Files:**
- Modify: `src-tauri/src/auth/tokens.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `delete_token` to `src-tauri/src/auth/tokens.rs`** (after `load_token`):

```rust
/// Remove the stored token for `account` from the Keychain. A missing entry is treated
/// as success (idempotent), mirroring `load_token`'s NoEntry handling.
pub fn delete_token(account: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)?;
    // 🦀 `delete_credential()` (keyring 3) removes the secret. We `match` so that a
    //    NoEntry (nothing stored) is `Ok(())`, not an error — disconnect should be
    //    idempotent. Any other error converts to AppError via `?`-style `e.into()`.
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
```

(If `delete_credential` doesn't resolve, the keyring 3 method is `delete_credential`; should compile. If the toolchain surfaces a deprecation, keep `delete_credential`.)

- [ ] **Step 2: Add the commands to `src-tauri/src/commands.rs`.** The file already has `use crate::auth::tokens::load_token;`, `use crate::auth::{ensure_access_token, GoogleOAuth, PRIMARY_ACCOUNT};`, `use crate::db;`, `use crate::error::{AppError, Result};`. Add `delete_token` to the tokens import:

```rust
use crate::auth::tokens::{delete_token, load_token};
```

Append the three commands at the end of the file:

```rust
/// Read persisted app settings (signature, remote-images), with defaults for first run.
#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, Db>) -> Result<db::Settings> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::get_settings(&conn)
}

/// Persist app settings.
#[tauri::command]
pub async fn set_settings(settings: db::Settings, state: tauri::State<'_, Db>) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::save_settings(&conn, &settings)
}

/// Sign out: delete the Keychain token and clear the local mail cache (messages +
/// sync_state). Settings (user prefs) are kept. After this, the app returns to the
/// connect screen.
#[tauri::command]
pub async fn disconnect(state: tauri::State<'_, Db>) -> Result<()> {
    // 🦀 `delete_token` is synchronous (keyring), so there's no `.await` between it and
    //    taking the DB lock — no MutexGuard-across-await concern.
    delete_token(PRIMARY_ACCOUNT)?;
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::clear_account_data(&conn)
}
```

- [ ] **Step 3: Register in `src-tauri/src/lib.rs`.** The `tauri::generate_handler![...]` list currently ends with `commands::get_reply_context,`. Append:

```rust
            commands::send_email,
            commands::get_reply_context,
            commands::get_settings,
            commands::set_settings,
            commands::disconnect,
        ])
```
(Keep the earlier entries; just add the three new lines before the closing `])`.)

- [ ] **Step 4: Build + lint + full test suite.**

Run: `cd src-tauri && cargo build`
Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings`
Run: `cd src-tauri && cargo test`
Expected: clean/green.

- [ ] **Step 5: Commit.**

```bash
git add src-tauri/src/auth/tokens.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add settings get/set + disconnect (delete_token + clear cache)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

**Rust recap:** matching a specific error variant (`keyring::Error::NoEntry`) to make an op idempotent; a Tauri command taking a serde struct arg (`db::Settings`); why `disconnect` needs no await-held lock.

---

## Task 3: Frontend API wrappers + `appendSignature`

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/lib/compose.ts`

- [ ] **Step 1: Append to `src/lib/api.ts`:**

```ts
export interface Settings {
  signature: string;
  remote_images: boolean;
}

export const getSettings = (): Promise<Settings> =>
  invoke<Settings>("get_settings");
export const setSettings = (settings: Settings): Promise<void> =>
  invoke<void>("set_settings", { settings });
export const disconnect = (): Promise<void> => invoke<void>("disconnect");
```

(The nested struct uses snake_case `remote_images` — serde field names aren't camelCase-converted, only the top-level arg key `settings` is. Same convention as `ReplyContext`.)

- [ ] **Step 2: Append `appendSignature` to `src/lib/compose.ts`:**

```ts
// Append a plain-text signature block to a composed body. Empty/whitespace signature
// → body unchanged. The "-- " line is the standard signature delimiter.
export function appendSignature(body: string, signature: string): string {
  if (!signature.trim()) return body;
  return `${body}\n\n-- \n${signature}`;
}
```

- [ ] **Step 3: Type-check.**

Run: `npm run build`
Expected: clean (new exports unused for now — fine for `tsc`).

- [ ] **Step 4: Commit.**

```bash
git add src/lib/api.ts src/lib/compose.ts
git commit -m "feat(ui): add settings/disconnect API wrappers and appendSignature

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: `SettingsModal.tsx` + styles

**Files:**
- Create: `src/components/SettingsModal.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Create `src/components/SettingsModal.tsx`:**

```tsx
import { useEffect, useState } from "react";
import { setSettings, disconnect, type Settings } from "../lib/api";
import { useTheme, type Theme } from "../theme";
import { X } from "lucide-react";

export function SettingsModal({
  account,
  initial,
  onClose,
  onSaved,
  onDisconnected,
}: {
  account: string;
  initial: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  onDisconnected: () => void;
}) {
  const { theme, setTheme } = useTheme();
  const [signature, setSignature] = useState(initial.signature);
  const [remoteImages, setRemoteImages] = useState(initial.remote_images);
  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Close on Esc from anywhere in the modal.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function handleSave() {
    const next: Settings = { signature, remote_images: remoteImages };
    setBusy(true);
    setError(null);
    try {
      await setSettings(next);
      onSaved(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDisconnect() {
    setBusy(true);
    setError(null);
    try {
      await disconnect();
      onDisconnected();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <div className="compose-overlay">
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="settings-title">
        <div className="compose-head">
          <span className="compose-title" id="settings-title">Settings</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="settings-row">
          <span className="settings-label">Account</span>
          <span className="settings-value">{account}</span>
        </div>

        <div className="settings-row">
          <span className="settings-label">Theme</span>
          <div className="settings-control">
            {(["light", "dark"] as Theme[]).map((t) => (
              <button
                key={t}
                className={theme === t ? "seg-btn active" : "seg-btn"}
                onClick={() => setTheme(t)}
              >
                {t === "light" ? "Light" : "Dark"}
              </button>
            ))}
          </div>
        </div>

        <div className="settings-row">
          <span className="settings-label">Remote images</span>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={remoteImages}
              onChange={(e) => setRemoteImages(e.target.checked)}
            />
            <span>{remoteImages ? "Load automatically" : "Blocked"}</span>
          </label>
        </div>

        <div className="settings-field">
          <span className="settings-label">Signature</span>
          <textarea
            className="compose-body settings-signature"
            placeholder="Added to the bottom of messages you compose"
            value={signature}
            onChange={(e) => setSignature(e.target.value)}
            rows={4}
          />
        </div>

        {error && <div className="compose-error">{error}</div>}

        <div className="settings-disconnect">
          {confirmingDisconnect ? (
            <div className="settings-confirm">
              <span>Disconnect? This signs out and clears the local cache.</span>
              <button className="btn btn-danger" onClick={handleDisconnect} disabled={busy}>
                {busy ? "Disconnecting…" : "Disconnect"}
              </button>
              <button className="btn" onClick={() => setConfirmingDisconnect(false)} disabled={busy}>
                Keep connected
              </button>
            </div>
          ) : (
            <button className="btn btn-danger-outline" onClick={() => setConfirmingDisconnect(true)} disabled={busy}>
              Disconnect account
            </button>
          )}
        </div>

        <div className="compose-actions">
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn btn-accent" onClick={handleSave} disabled={busy}>
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Append styles to `src/styles/app.css`** (reuses the M8 `.compose-overlay`/`.compose-card`/`.compose-error`/`.compose-actions`/`.compose-body` classes):

```css
/* M9 — settings modal */
.settings-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 4px 0; }
.settings-field { display: flex; flex-direction: column; gap: 6px; padding: 4px 0; }
.settings-label { font-size: 13px; font-weight: 500; color: var(--text); }
.settings-value { font-size: 13px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.settings-control { display: flex; gap: 4px; }
.seg-btn { padding: 5px 12px; border: 1px solid var(--border); border-radius: 7px; background: var(--bg); color: var(--text-muted); font-size: 12px; cursor: pointer; }
.seg-btn.active { background: var(--accent-weak); border-color: var(--accent); color: var(--accent-text); }
.settings-toggle { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; color: var(--text-muted); cursor: pointer; }
.settings-signature { min-height: 80px; }
.settings-disconnect { border-top: 1px solid var(--border); padding-top: 10px; margin-top: 2px; }
.settings-confirm { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 13px; color: var(--text); }
.btn-danger { background: var(--danger); border-color: var(--danger); color: #fff; }
.btn-danger-outline { border-color: var(--danger); color: var(--danger); background: transparent; }
```

- [ ] **Step 3: Type-check.**

Run: `npm run build`
Expected: clean (the modal is exported but not yet rendered — fine for `tsc`).

- [ ] **Step 4: Commit.**

```bash
git add src/components/SettingsModal.tsx src/styles/app.css
git commit -m "feat(ui): add SettingsModal (account/disconnect, theme, images, signature)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Wire settings into App, Header, ReadingPane

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/components/Header.tsx`
- Modify: `src/components/ReadingPane.tsx`

> Read all three files first to confirm exact insertion points (they've evolved through M7/M8).

- [ ] **Step 1: `src/components/ReadingPane.tsx` — `loadImages` prop.** Add `loadImages: boolean` to the props type + destructure. Change the body-fetch call and its effect dependency:
  - In the props object type, add: `loadImages: boolean;`
  - In the destructured params, add `loadImages`.
  - Change `fetchMessageBody(msg.id, true)` to `fetchMessageBody(msg.id, loadImages)`.
  - Change the effect dependency array from `[msg?.id]` to `[msg?.id, loadImages]` (so toggling the setting re-fetches the open message with the new preference).

- [ ] **Step 2: `src/components/Header.tsx` — gear Settings button.** Add `Settings as SettingsIcon` to the lucide import; add optional `onSettings?: () => void` to the props type + destructure; render a gear button just before the theme toggle button (the one calling `cycleTheme`):

```tsx
import {
  Flame,
  RefreshCw,
  Sun,
  Moon,
  Inbox,
  Users,
  Bell,
  Newspaper,
  Pencil,
  Settings as SettingsIcon,
  type LucideIcon,
} from "lucide-react";
```
Add `onSettings?: () => void;` to the props type and `onSettings` to the destructure. Then render before the theme `<button>`:
```tsx
      {account && onSettings && (
        <button className="icon-btn" onClick={onSettings} aria-label="Settings">
          <SettingsIcon size={16} />
        </button>
      )}
```

- [ ] **Step 3: `src/App.tsx` — imports.** Add to the `./lib/api` import: `getSettings`, `disconnect as disconnectAccount`, and `type Settings`. Add the modal + appendSignature imports:

```tsx
import { appendSignature, parseAddress, replySubject, quoteBody } from "./lib/compose";
import { SettingsModal } from "./components/SettingsModal";
```
(Keep the existing `ComposeModal`/`ComposeInitial` import. The `./lib/api` import block gains `getSettings`, `type Settings`, and — only if you use it directly in App — `disconnect`; note the SettingsModal calls `disconnect` itself, so App does NOT need to import it. Import `getSettings` and `type Settings`.)

- [ ] **Step 4: `src/App.tsx` — state.** Add next to the other `useState` hooks:

```tsx
  const [settings, setSettings] = useState<Settings>({ signature: "", remote_images: true });
  const [settingsOpen, setSettingsOpen] = useState(false);
```
(Name the setter `setSettings` — it's the React state setter for the `settings` object, distinct from the `setSettings` API wrapper which is NOT imported into App.)

- [ ] **Step 5: `src/App.tsx` — load settings on mount.** In the existing mount `useEffect` (the one calling `getConnectedAccount()` + `fetchInboxPreview(50)`), add a settings fetch that falls back to defaults on error:

```tsx
    getSettings()
      .then(setSettings)
      .catch(() => {}); // keep defaults { signature: "", remote_images: true }
```

- [ ] **Step 6: `src/App.tsx` — onboarding auto-sync on connect + disconnect handler.** Replace the existing `handleConnect` body so that after a successful connect it auto-runs the first sync, and add `handleDisconnect`:

```tsx
  async function handleConnect() {
    setBusy(true);
    setError(null);
    try {
      const acct = await connectGmail();
      setAccount(acct);
      // Onboarding: pull the inbox right away so the user doesn't land on an empty list.
      setStatus("Syncing your inbox…");
      const s = await syncInbox();
      setStatus(`${s.added} new, ${s.removed} removed`);
      setMessages(await fetchInboxPreview(50));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function handleDisconnected() {
    // Called by SettingsModal after the disconnect command succeeds.
    setSettingsOpen(false);
    setAccount(null);
    setMessages([]);
    setSelectedId(null);
    setStatus(null);
  }
```

- [ ] **Step 7: `src/App.tsx` — seed the signature into compose bodies.** In `openNewCompose`, set `body: appendSignature("", settings.signature)`. In `handleReply`, change the `body` to `appendSignature(quoteBody(m.from, dateLabel, ctx.quoted_text), settings.signature)`.

- [ ] **Step 8: `src/App.tsx` — JSX wiring.** Add `onSettings={() => setSettingsOpen(true)}` to the signed-in `<Header ... />`. Add `loadImages={settings.remote_images}` to `<ReadingPane ... />`. Render the settings modal alongside the compose modal (before the closing `</div>` of the signed-in return):

```tsx
      {settingsOpen && (
        <SettingsModal
          account={account}
          initial={settings}
          onClose={() => setSettingsOpen(false)}
          onSaved={(s) => {
            setSettings(s);
            setSettingsOpen(false);
          }}
          onDisconnected={handleDisconnected}
        />
      )}
```
(`account` is non-null in the signed-in branch.)

- [ ] **Step 9: `src/App.tsx` — connect-screen explainer.** In the `if (!account)` connect screen, under the existing `<p className="connect-sub">Connect your Gmail to get started.</p>`, the copy is fine; ensure the connect button shows the busy state during connect+sync. The existing button already uses `{busy ? "Connecting…" : "Connect Gmail"}` — leave it (the `status` "Syncing your inbox…" appears in the header once connected). No structural change needed beyond what Step 6 added.

- [ ] **Step 10: Type-check the whole frontend.**

Run: `npm run build`
Expected: `tsc` + Vite clean. All wiring lines up (`onSettings`, `loadImages`, SettingsModal props, the new imports/state).

- [ ] **Step 11: Commit.**

```bash
git add src/App.tsx src/components/Header.tsx src/components/ReadingPane.tsx
git commit -m "feat(ui): wire settings modal, disconnect, signature, images + onboarding auto-sync

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Full verification + docs + finish

**Files:**
- Modify: `wiki/entities/ember.md`, `wiki/log.md` (gitignored, local)
- (Memory) `~/.claude/projects/-Users-makar-dev-ownmail/memory/ember-project.md` + `MEMORY.md`

- [ ] **Step 1: Backend — full suite + lint.**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: green/clean. (Do NOT run `cargo fmt`.)

- [ ] **Step 2: Frontend — build.**

Run: `npm run build`
Expected: clean.

- [ ] **Step 3: Manual E2E** (`npm run tauri dev` from repo root; if port 1420 busy: `lsof -ti tcp:1420 | xargs kill` first):
- Open **Settings** (gear) → switch **Theme** light/dark (applies immediately).
- Toggle **Remote images** off → Save → open a message with remote images → confirm they're **blocked** (and on → loaded).
- Set a **Signature** → Save → click **Compose** → confirm the signature appears at the bottom of the body; do a **Reply** → confirm it's appended after the quote.
- **Disconnect** (gear → Disconnect account → confirm) → app returns to the **connect screen**; relaunch and confirm it still asks to connect (token gone) and the inbox cache is empty.
- **Reconnect** → confirm it **auto-syncs** ("Syncing your inbox…") and the inbox populates without hitting Sync.

- [ ] **Step 4: Update the wiki.** In `wiki/entities/ember.md`, change the M9 roadmap line to a done/state entry; update the "As of M8…" capability sentence to mention settings/disconnect; append a one-line entry to `wiki/log.md`.

- [ ] **Step 5: Update project memory.** Add an M9 milestone entry (done + merge SHA) to `ember-project.md`; update the `MEMORY.md` index line to "M1–M9 merged … next M10 calendar".

- [ ] **Step 6: Commit docs** (wiki is gitignored, so this mainly no-ops):
```bash
git add -A && git commit -m "docs(m9): record Settings & Onboarding milestone

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>" || echo "(nothing tracked to commit — wiki is gitignored)"
```

- [ ] **Step 7: Finish the branch.** Use `superpowers:finishing-a-development-branch` to merge `m9-settings-onboarding` to `main` (verify tests on the merged result, then delete the branch).

---

## Self-review (completed during planning)

- **Spec coverage:** settings store + clear_account_data → Task 1; delete_token + commands → Task 2; api/appendSignature → Task 3; SettingsModal → Task 4; wiring (theme/images/signature/disconnect/onboarding auto-sync) → Task 5; manual E2E + docs + finish → Task 6. All spec sections map to a task.
- **Type consistency:** `db::Settings { signature: String, remote_images: bool }` ↔ TS `Settings { signature, remote_images }`; commands `get_settings`/`set_settings(settings)`/`disconnect`; wrappers `getSettings`/`setSettings({settings})`/`disconnect`; `appendSignature(body, signature)`; SettingsModal props (`account`/`initial`/`onClose`/`onSaved`/`onDisconnected`) match App's render; `loadImages` prop matches App↔ReadingPane; `onSettings` matches App↔Header. The App state setter `setSettings` (React) is intentionally distinct from the un-imported `setSettings` API wrapper (the SettingsModal owns the API call), avoiding a name clash.
- **Placeholder scan:** no TBD/TODO; every code step shows complete code.
- **Intentional caveat:** the frontend builds green at Tasks 3, 4, 5 individually (the modal is committed before it's rendered — a valid unused export under `tsc`).
