# Ember M13 — New-mail notifications (lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While Ember is open, background-sync the INBOX on a ~60s timer and raise a native macOS banner for each genuinely-new inbox message (sender + subject); clicking opens it; suppressed while the window is focused; toggleable in Settings (default on).

**Architecture:** A React `setInterval` reuses the existing `sync_inbox` path. New mail is detected by a **pure frontend id-diff** of the inbox preview list against a session-seeded `knownIdsRef` (so the launch backlog never notifies). Banners are posted via a thin `lib/notify.ts` wrapper over `tauri-plugin-notification`, gated by `isTauri()` so the browser maket still runs. The only backend change is an **additive `notifications` settings field** (no migration) plus registering the plugin.

**Tech Stack:** Rust (Tauri 2 + `tauri-plugin-notification`, rusqlite), React 19 + TypeScript + Vite, `@tauri-apps/plugin-notification`, `@tauri-apps/api/window`.

**Learning mode (IMPORTANT):** the repo owner is learning Rust. Every Rust edit carries concise `// 🦀` teaching comments on the *language* concept; after each Rust task give a short plain-English recap. TS/React gets normal comments.

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m13-notifications-design.md`

**Ordering note:** the `notifications` field is added to the Rust `Settings` struct (Task 1) before the TS `Settings` type (Task 4). These are separate builds; the field lands on both sides within this branch before it ever merges, so no released build is ever mismatched. Do not run the live app's `set_settings` between Task 1 and Task 4 (the IPC payload would be missing the field).

---

## Task 1: Backend — add `notifications` to settings (additive, no migration)

**Files:**
- Modify: `src-tauri/src/db/mod.rs` (struct `Settings` ~47; `get_settings` ~354–361; `save_settings` ~364–370; tests ~570–597)

- [ ] **Step 1: Update the failing tests first**

In `src-tauri/src/db/mod.rs`, update the three existing settings tests to cover the new field. Replace the bodies of `get_settings_returns_defaults_when_empty`, `save_then_get_settings_round_trips`, and the `Settings { … }` literal in `clear_account_data_wipes_cache_but_keeps_settings`:

```rust
    #[test]
    fn get_settings_returns_defaults_when_empty() {
        let c = conn();
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "");
        assert!(s.remote_images); // default: load images (preserves pre-M9 behavior)
        assert!(s.notifications); // default: notifications on out of the box
    }

    #[test]
    fn save_then_get_settings_round_trips() {
        let c = conn();
        save_settings(
            &c,
            &Settings { signature: "Cheers,\nDmytro".into(), remote_images: false, notifications: false },
        )
        .unwrap();
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "Cheers,\nDmytro");
        assert!(!s.remote_images);
        assert!(!s.notifications);
        // also exercise the "1" → true decode path for both bools
        save_settings(
            &c,
            &Settings { signature: "Cheers,\nDmytro".into(), remote_images: true, notifications: true },
        )
        .unwrap();
        let s = get_settings(&c).unwrap();
        assert!(s.remote_images);
        assert!(s.notifications);
    }
```

And in `clear_account_data_wipes_cache_but_keeps_settings`, change the save line and add an assertion:

```rust
        save_settings(&c, &Settings { signature: "sig".into(), remote_images: false, notifications: false }).unwrap();
```
```rust
        let s = get_settings(&c).unwrap();
        assert_eq!(s.signature, "sig");
        assert!(!s.remote_images);
        assert!(!s.notifications);
```

- [ ] **Step 2: Run the tests to verify they fail (compile error)**

Run: `cd src-tauri && cargo test --lib db::`
Expected: FAIL — `Settings` has no field `notifications` (and the `assert!(s.notifications)` lines don't compile).

- [ ] **Step 3: Add the field + read/write it**

In the `Settings` struct (~47), add the field:

```rust
pub struct Settings {
    pub signature: String,
    pub remote_images: bool,
    // 🦀 A plain `bool` field — serde derives (above) serialize it to/from JSON for the
    //    Tauri IPC boundary automatically, same as `remote_images`.
    pub notifications: bool,
}
```

In `get_settings` (~354), decode the key with a default of `true`:

```rust
pub fn get_settings(conn: &Connection) -> Result<Settings> {
    let signature = get_setting_raw(conn, "signature")?.unwrap_or_default();
    let remote_images = get_setting_raw(conn, "remote_images")?
        .map(|v| v == "1")
        .unwrap_or(true);
    // 🦀 Same "1"/"0" decode as remote_images; `unwrap_or(true)` makes notifications
    //    default ON when the key was never written (existing installs included).
    let notifications = get_setting_raw(conn, "notifications")?
        .map(|v| v == "1")
        .unwrap_or(true);
    Ok(Settings { signature, remote_images, notifications })
}
```

In `save_settings` (~364), persist it inside the same transaction:

```rust
pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    set_setting_raw(&tx, "signature", &s.signature)?;
    set_setting_raw(&tx, "remote_images", if s.remote_images { "1" } else { "0" })?;
    // 🦀 `if cond { "1" } else { "0" }` is an expression (Rust ifs return values), so
    //    it slots straight into the function call as the encoded value.
    set_setting_raw(&tx, "notifications", if s.notifications { "1" } else { "0" })?;
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib db::`
Expected: PASS (all `db::` tests green).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "feat(m13): add notifications field to settings (additive, no migration)"
```

**🦀 Recap to give the owner:** the key-value `settings` table needs no migration to gain a field — `get_settings` simply reads one more key and applies a default when it's absent, so old databases keep working. `unwrap_or(true)` is the idiom for "use this value when the Option is None."

---

## Task 2: Backend — register the notification plugin + capability

**Files:**
- Modify: `src-tauri/Cargo.toml` (`[dependencies]`)
- Modify: `src-tauri/src/lib.rs` (builder chain ~75)
- Modify: `src-tauri/capabilities/default.json` (permissions)

- [ ] **Step 1: Add the Rust dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]` (e.g. after the `ammonia` line), add:

```toml
tauri-plugin-notification = "2"
```

- [ ] **Step 2: Register the plugin in the builder**

In `src-tauri/src/lib.rs`, add the plugin to the builder chain. Change:

```rust
    tauri::Builder::default()
        .setup(|app| {
```
to:

```rust
    tauri::Builder::default()
        // 🦀 `.plugin(...)` registers a Tauri plugin's commands + setup on the builder.
        //    `tauri_plugin_notification::init()` returns the plugin value; the JS side
        //    reaches it through `@tauri-apps/plugin-notification`.
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
```

- [ ] **Step 3: Grant the capability**

In `src-tauri/capabilities/default.json`, extend `permissions` so it reads:

```json
  "permissions": [
    "core:default",
    "core:window:allow-set-focus",
    "notification:default"
  ]
```

(`notification:default` grants send + permission checks; `core:window:allow-set-focus` lets the click handler foreground the window in Task 6.)

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo build`
Expected: PASS — `tauri-plugin-notification` downloads and the crate builds. (Capability JSON is fully validated later by `npm run tauri dev`; here confirm it is valid JSON / build is green.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "feat(m13): register tauri-plugin-notification + capability"
```

**🦀 Recap:** Tauri's capability system is allow-list security — a plugin's commands are inert until the window's capability file opts into them. Adding the dependency and `.plugin(init())` is not enough; the `notification:default` permission line is what actually lets the frontend call it.

---

## Task 3: Frontend — install the JS plugin + create `lib/notify.ts`

**Files:**
- Modify: `package.json` (dependencies)
- Create: `src/lib/notify.ts`

- [ ] **Step 1: Install the JS plugin**

Run: `npm install @tauri-apps/plugin-notification`
Expected: adds `@tauri-apps/plugin-notification` to `package.json` dependencies.

- [ ] **Step 2: Create the wrapper + pure diff**

Create `src/lib/notify.ts`:

```ts
import { isTauri } from "@tauri-apps/api/core";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { MessagePreview } from "./api";

/** Newest-first messages in `list` whose id is not in `known`, capped at `cap`. Pure. */
export function pickNewMail(
  known: Set<string>,
  list: MessagePreview[],
  cap: number,
): MessagePreview[] {
  const fresh = list.filter((m) => !known.has(m.id));
  fresh.sort((a, b) => b.internal_date - a.internal_date); // newest first
  return fresh.slice(0, cap);
}

/** '"Ada Lovelace" <ada@x.com>' -> "Ada Lovelace"; falls back to the raw address. */
export function displayName(from: string): string {
  const m = from.match(/^\s*"?([^"<]*?)"?\s*<.*>\s*$/);
  const name = m?.[1]?.trim();
  return name && name.length > 0 ? name : from.trim();
}

/**
 * True if we may post OS notifications; requests permission once if needed.
 * No-op `false` outside Tauri (the browser maket never posts real banners).
 */
export async function ensureNotificationPermission(): Promise<boolean> {
  if (!isTauri()) return false;
  try {
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === "granted";
  } catch (e) {
    console.warn("[ember] notification permission check failed:", e);
    return false;
  }
}

/** Post one native banner for a new message. No-ops (logs) outside Tauri or on failure. */
export async function notifyNewMail(m: MessagePreview): Promise<void> {
  if (!isTauri()) {
    console.debug("[ember] (maket) new mail:", displayName(m.from), "—", m.subject);
    return;
  }
  try {
    sendNotification({ title: displayName(m.from), body: m.subject });
  } catch (e) {
    console.warn("[ember] sendNotification failed:", e);
  }
}
```

- [ ] **Step 3: Verify it type-checks**

Run: `npm run build`
Expected: PASS (`tsc && vite build`). The new file is not imported yet; this just confirms it compiles and the plugin types resolve.

- [ ] **Step 4: Commit**

```bash
git add package.json package-lock.json src/lib/notify.ts
git commit -m "feat(m13): notify.ts wrapper (permission, sendNotification) + pure pickNewMail"
```

---

## Task 4: Frontend — `Settings` type field + Settings toggle

**Files:**
- Modify: `src/lib/api.ts` (`Settings` interface ~102–105)
- Modify: `src/App.tsx` (default settings literal ~47)
- Modify: `src/components/SettingsModal.tsx`

- [ ] **Step 1: Add the field to the TS type**

In `src/lib/api.ts`, extend the `Settings` interface:

```ts
export interface Settings {
  signature: string;
  remote_images: boolean;
  notifications: boolean;
}
```

- [ ] **Step 2: Fix the App default literal**

In `src/App.tsx` line ~47, change:

```tsx
  const [settings, setSettings] = useState<Settings>({ signature: "", remote_images: true });
```
to:

```tsx
  const [settings, setSettings] = useState<Settings>({ signature: "", remote_images: true, notifications: true });
```

- [ ] **Step 3: Add the toggle to SettingsModal**

In `src/components/SettingsModal.tsx`:

Add the import (after the api import on line 2):

```tsx
import { ensureNotificationPermission } from "../lib/notify";
```

Add state (after `const [remoteImages, …]` ~21):

```tsx
  const [notifications, setNotifications] = useState(initial.notifications);
```

Include it in `handleSave`'s `next` (~36):

```tsx
    const next: Settings = { signature, remote_images: remoteImages, notifications };
```

Add the toggle row immediately after the "Remote images" `settings-row` block (after line ~108):

```tsx
        <div className="settings-row">
          <span className="settings-label">New-mail notifications</span>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={notifications}
              onChange={(e) => {
                const on = e.target.checked;
                setNotifications(on);
                // Prompt for OS permission immediately when switching on.
                if (on) void ensureNotificationPermission();
              }}
            />
            <span>{notifications ? "On" : "Off"}</span>
          </label>
        </div>
```

- [ ] **Step 4: Verify it builds**

Run: `npm run build`
Expected: PASS — all three `Settings` construction sites now include `notifications`.

- [ ] **Step 5: Manually verify the toggle in the maket**

Run: `npm run dev`, open the local URL, open Settings (gear). Expected: a "New-mail notifications" row with an On/Off toggle, defaulting to On. (No OS prompt in the browser — `ensureNotificationPermission` returns false outside Tauri.)

- [ ] **Step 6: Commit**

```bash
git add src/lib/api.ts src/App.tsx src/components/SettingsModal.tsx
git commit -m "feat(m13): Settings notifications toggle + type field"
```

---

## Task 5: Frontend — background poll + new-mail diff wiring

**Files:**
- Modify: `src/App.tsx` (imports; constants; refs; `runSync`/`handleSync`; mount + connect seeding; permission + interval effects)

- [ ] **Step 1: Update imports**

In `src/App.tsx`, change the React import (line 1) to add `useRef`:

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
```

Add the notify import (after the `./lib/labels` import ~23):

```tsx
import { pickNewMail, notifyNewMail, ensureNotificationPermission } from "./lib/notify";
```

- [ ] **Step 2: Add module-level constants**

Above `export default function App() {` (line ~38), add:

```tsx
// M13 new-mail notifications.
const POLL_MS = 60_000; // background sync cadence while the app is open
const MAX_NOTIFY_PER_SYNC = 5; // cap banners per cycle so a backlog can't flood
```

- [ ] **Step 3: Add refs + seedKnown (after the M12 folder state, ~line 69)**

```tsx
  // M13: track inbox ids we've already seen so only genuinely-new mail notifies.
  const syncingRef = useRef(false); // guards against overlapping syncs
  const knownIdsRef = useRef<Set<string>>(new Set());
  const notifyAllowedRef = useRef(false); // OS permission granted AND feature enabled
  function seedKnown(list: MessagePreview[]) {
    for (const m of list) knownIdsRef.current.add(m.id);
  }
```

- [ ] **Step 4: Replace `handleSync` with the shared `runSync`**

Replace the whole `handleSync` function (~139–152) with:

```tsx
  // Shared sync path. surfaceErrors=true (manual Sync button): drive busy/status and
  // show failures in the error bar. surfaceErrors=false (background timer): stay silent
  // — a transient failure must NOT flash the error bar every POLL_MS. Guarded so a slow
  // sync is never overlapped by the next tick.
  async function runSync(surfaceErrors: boolean) {
    if (syncingRef.current) return;
    syncingRef.current = true;
    if (surfaceErrors) {
      setBusy(true);
      setError(null);
      setStatus(null);
    }
    try {
      const s = await syncInbox();
      const list = await fetchInboxPreview(50);
      setMessages(list);
      if (surfaceErrors) setStatus(`${s.added} new, ${s.removed} removed`);

      // New-mail notifications: one banner per fresh id (newest-first, capped) when the
      // window is unfocused. Always fold the current ids into `known` afterward so a
      // message never notifies twice. Manual syncs are naturally silent (window focused).
      const fresh = pickNewMail(knownIdsRef.current, list, MAX_NOTIFY_PER_SYNC);
      for (const m of list) knownIdsRef.current.add(m.id);
      if (fresh.length && notifyAllowedRef.current && !document.hasFocus()) {
        lastNotifiedIdRef.current = fresh[0].id; // newest — opened on banner click (Task 6)
        for (const m of fresh) void notifyNewMail(m);
      }
    } catch (e) {
      if (surfaceErrors) setError(String(e));
      else console.warn("[ember] background sync failed:", e);
    } finally {
      syncingRef.current = false;
      if (surfaceErrors) setBusy(false);
    }
  }

  const handleSync = () => runSync(true);

  // Live ref to runSync so the interval always calls the latest closure without
  // re-subscribing the timer on every render.
  const runSyncRef = useRef(runSync);
  runSyncRef.current = runSync;
```

> Note: `lastNotifiedIdRef` is declared in Task 6. To keep this task self-compiling, also add it now alongside the other refs in Step 3:
> ```tsx
>   const lastNotifiedIdRef = useRef<string | null>(null);
> ```

- [ ] **Step 5: Seed on mount and on connect (no notify)**

In the mount effect (~100), change the inbox fetch to seed:

```tsx
    fetchInboxPreview(50)
      .then((list) => {
        setMessages(list);
        seedKnown(list); // baseline: mail already present at launch never notifies
      })
      .catch(() => {});
```

In `handleConnect` (~119), change:

```tsx
      setMessages(await fetchInboxPreview(50));
```
to:

```tsx
      const list = await fetchInboxPreview(50);
      setMessages(list);
      seedKnown(list);
```

- [ ] **Step 6: Add the permission + interval effects**

After `handleConnect` (or near the other effects, ~125), add:

```tsx
  // Resolve OS notification permission once per session and whenever notifications are
  // switched on. Stored in a ref so runSync can read it without re-rendering.
  useEffect(() => {
    if (!account || !settings.notifications) {
      notifyAllowedRef.current = false;
      return;
    }
    ensureNotificationPermission().then((ok) => {
      notifyAllowedRef.current = ok;
    });
  }, [account, settings.notifications]);

  // Background poll: sync every POLL_MS while connected with notifications on. Tearing
  // down on dep change means disconnect / toggle-off / unmount all stop the timer.
  useEffect(() => {
    if (!account || !settings.notifications) return;
    const id = setInterval(() => void runSyncRef.current(false), POLL_MS);
    return () => clearInterval(id);
  }, [account, settings.notifications]);
```

- [ ] **Step 7: Verify it builds**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 8: Manually verify the loop in the maket**

Run: `npm run dev`. Open the console. Expected: no errors; the inbox renders. (In the maket `syncInbox`/`fetchInboxPreview` are static mocks, so after seeding the diff finds nothing fresh and stays silent — this confirms the loop runs cleanly and does **not** misfire on already-seen mail. Optional sanity check: temporarily make `mockSearch`/`MOCK_MESSAGES` return an extra id to see a `[ember] (maket) new mail:` debug line, then revert.)

- [ ] **Step 9: Commit**

```bash
git add src/App.tsx
git commit -m "feat(m13): background poll + new-mail id-diff + native banners"
```

---

## Task 6: Frontend — click a banner to open the message (best-effort + focus fallback)

**Files:**
- Modify: `src/App.tsx` (imports; `lastNotifiedIdRef` already added in Task 5; open helper; onAction effect)

- [ ] **Step 1: Add imports**

In `src/App.tsx`, add to the `@tauri-apps/plugin-notification` usage by importing `onAction`, and import `getCurrentWindow`. Add near the other imports:

```tsx
import { onAction } from "@tauri-apps/plugin-notification";
import { getCurrentWindow } from "@tauri-apps/api/window";
```

- [ ] **Step 2: Add the open helper (near the other handlers, ~line 263)**

```tsx
  // Open a specific inbox message (used when a notification banner is clicked): leave
  // search/folder, return to the smart inbox, and select it. The message is in `messages`
  // because the sync that notified just refreshed the list.
  function openMessageFromNotification(id: string) {
    setView("mail");
    setInSearch(false);
    setSearchResults([]);
    setSearchQuery("");
    setFolder("inbox");
    setStream("all");
    setSelectedId(id);
  }
```

- [ ] **Step 3: Register the click listener (effect, after the interval effect)**

```tsx
  // M13: when the user clicks a banner, foreground Ember and open the most-recently
  // notified message. onAction fires for a notification tap/action on desktop.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onAction(() => {
      void getCurrentWindow().setFocus();
      const id = lastNotifiedIdRef.current;
      if (id) openMessageFromNotification(id);
    })
      .then((un) => {
        unlisten = un;
      })
      .catch((e) => console.warn("[ember] onAction subscribe failed:", e));
    return () => unlisten?.();
  }, []);
```

- [ ] **Step 4: Verify it builds**

Run: `npm run build`
Expected: PASS.

> **If `onAction` is not exported by the installed plugin version, or `npm run build` fails on that import:** remove Step 1's two imports, the Step 2 `openMessageFromNotification` helper, and the entire Step 3 effect. No replacement code is needed — clicking a macOS banner already foregrounds the app via the OS, which is the **focus-only fallback** flagged in the spec as an acceptable lean-v1 outcome. Record which path shipped (full deep-link vs focus-only) in the commit message.

- [ ] **Step 5: Manually verify (best-effort)**

Run `npm run tauri dev` (real app), ensure notifications are On, unfocus Ember, and trigger a new mail (or temporarily lower `POLL_MS` and send yourself a test message). Click the banner. Expected (full path): Ember foregrounds and the message opens. If the click does nothing beyond foregrounding, that is the focus-only fallback — acceptable.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx
git commit -m "feat(m13): open message on banner click (focus-only fallback)"
```

---

## Task 7: Verification, roadmap & wiki

**Files:**
- Modify: `wiki/entities/ember.md` (roadmap + summary + `updated:`)
- Modify: `wiki/log.md` (one-line entry)

- [ ] **Step 1: Full backend verification**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets`
Expected: all tests PASS (existing 65 + updated settings tests); clippy clean (no warnings).

- [ ] **Step 2: Full frontend verification**

Run: `npm run build`
Expected: PASS (`tsc && vite build`).

- [ ] **Step 3: Maket screenshot**

Run: `npm run dev`; screenshot Settings showing the "New-mail notifications" toggle (default On). Confirm no console errors.

- [ ] **Step 4: Update the roadmap**

In `wiki/entities/ember.md`: bump `updated:` to `2026-06-20`; add an M13 bullet after the M12 bullet; update the "As of M12…" closing paragraph to "As of M13…" mentioning new-mail notifications. M13 bullet text:

```
- **M13 — New-mail notifications (lean v1)** — *current.* Last of the M11→M12→M13
  sequence. A ~60s React `setInterval` background-polls the existing `sync_inbox` while
  the app is open (gated on *connected AND a new Settings toggle*); a **pure frontend
  id-diff** (`pickNewMail`) of the inbox preview against a session-seeded `knownIdsRef`
  detects genuinely-new mail (the launch backlog is seeded, so it never notifies). Each
  fresh message raises a **native macOS banner** (sender + subject) via
  `tauri-plugin-notification` (new `lib/notify.ts` wrapper, `isTauri()`-gated so the maket
  no-ops), newest-first, **capped at 5/cycle**, **suppressed while the window is focused**;
  clicking opens the newest notified message (focus-only fallback if the plugin's click
  event isn't delivered). The `notifications` setting is **additive** to the key-value
  settings table (default on, **no migration**); **no new OAuth scope**. Background sync
  failures stay silent (no error-bar spam). Deferred: category filtering (all mail notifies
  for now), coalesced summaries, banner action buttons, closed-app notifications, configurable
  cadence. **Live banner E2E pending owner.**
```

- [ ] **Step 5: Append to the wiki log**

Add one line to `wiki/log.md` (match the existing format), e.g.:

```
- 2026-06-20 — M13 new-mail notifications (lean v1): background poll + id-diff + native banners + Settings toggle. Updated [[ember]].
```

- [ ] **Step 6: Commit**

```bash
git add wiki/entities/ember.md wiki/log.md
git commit -m "docs(m13): record new-mail notifications in the wiki roadmap"
```

---

## Self-review (completed by plan author)

**Spec coverage:** background poll (T5) ✓; pure id-diff w/ seeding (T3 `pickNewMail`, T5 wiring) ✓; native banners capped+sender/subject (T3 `notifyNewMail`, T5) ✓; focus suppression (T5 `document.hasFocus()`) ✓; click-to-open + focus-only fallback (T6) ✓; Settings toggle additive no-migration (T1 backend, T4 UI) ✓; permission request (T3 `ensureNotificationPermission`, T4 on-enable, T5 effect) ✓; maket no-op (T3 `isTauri()` guards) ✓; plugin wiring + capability (T2) ✓; background errors silent (T5) ✓; Rust learning comments (T1, T2) ✓; tests extended (T1) ✓; verification + wiki (T7) ✓.

**Placeholder scan:** no TBD/TODO; every code step shows full code.

**Type/name consistency:** `pickNewMail(known, list, cap)`, `notifyNewMail(m)`, `ensureNotificationPermission()`, `displayName(from)` consistent across T3/T5/T6; refs `syncingRef`/`knownIdsRef`/`notifyAllowedRef`/`runSyncRef`/`lastNotifiedIdRef` all declared in T5 (the last cross-referenced by T6); `Settings { signature, remote_images, notifications }` consistent across T1 (Rust) and T4 (TS); constants `POLL_MS`/`MAX_NOTIFY_PER_SYNC` defined T5. `lastNotifiedIdRef` is explicitly declared in T5 Step 4's note so T5 compiles before T6 uses it.
