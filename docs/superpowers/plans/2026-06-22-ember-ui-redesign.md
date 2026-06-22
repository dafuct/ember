# Ember UI Redesign — "Smart Inbox" dark shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle and restructure Ember's mail UI to match the "Smart Inbox" mockup — a far-left icon rail, a merged left sidebar, grouped message cards, and a restyled reading pane — in a deep-dark, rounded aesthetic, keeping Ember's **green** accent and the light/dark toggle (dark default).

**Architecture:** Presentation-only. Reuse all existing state/handlers in `App.tsx` (`stream/setStream`, `folder/handleSelectFolder`, `view/setView`, `compose/openNewCompose`, search, settings, `useTheme`). Replace the top `Header` + 3-pane split with a 4-zone shell (`IconRail` │ `Sidebar` │ `MessageList` │ `ReadingPane`). Add two new components (`IconRail`, `Sidebar`); retire `Header` and `FolderRail`; re-skin `MessageList`/`MessageItem`/`ReadingPane`/`CalendarView` via rewritten CSS tokens. No backend, no new deps, no `tauri.conf.json` change.

**Tech Stack:** React + TypeScript (Vite), CSS custom properties in `src/styles/theme.css` + `src/styles/app.css`, `lucide-react` icons, Tauri `tauri icon` for the app icon. Verification is `npx tsc --noEmit` + the browser maket (launch config `ember-maket`, port 5190) + `cargo test` staying green.

**Execution note:** Do this on a feature branch (`git checkout -b ui-redesign`) — `main` currently carries unrelated uncommitted work. Commit after each task. Visual/pixel tuning against the maket is expected; the spec fixes the structure and token names, not exact pixels.

**Verification pattern (every task):** after edits, run `npx tsc --noEmit` (must be clean), then in the maket (`mcp__Claude_Preview__preview_start` name `ember-maket`, then `preview_snapshot`/`preview_screenshot`/`preview_console_logs`) confirm the task's UI renders with no console errors. The maket sets `isTauri()=false` and defaults to the Calendar view — click Mail first.

---

## Task 1: Theme tokens — dark default + green + radius/spacing

**Files:**
- Modify: `src/styles/theme.css` (full rewrite of the token blocks; keep the `*`/`html`/`body`/`button` base rules)
- Modify: `src/theme.tsx` — change `initialTheme()` fallback from `"light"` to `"dark"` so a fresh user gets dark (a saved preference still wins). Required because `useTheme` sets `data-theme` explicitly, overriding the CSS `:root` default.

- [ ] **Step 1: Rewrite the token blocks.** Replace lines 1–32 (the `:root[data-theme]` blocks) with: dark as the default `:root`, a `[data-theme="light"]` variant, green accent kept, new radius/spacing tokens. Keep everything from `* { box-sizing }` onward unchanged.

```css
/* Dark is the default. Light is opt-in via [data-theme="light"]. */
:root,
:root[data-theme="dark"] {
  --bg: #16161b;
  --surface: #1d1d24;
  --surface-2: #26262f;
  --surface-3: #30303b;
  --text: #f1ede6;
  --text-muted: #a7a39c;
  --text-faint: #6f6a63;
  --border: #2a2a33;
  --border-strong: #3a3a45;
  --accent: #3fbf6f;
  --accent-hover: #4fce7e;
  --accent-weak: #16271c;
  --accent-text: #7ee7a3;
  --accent-contrast: #0e160f;
  --danger: #e06c5b;
  --star: #f5c451;
  --radius-card: 14px;
  --radius-control: 10px;
  --radius-pill: 999px;
  --rail-w: 64px;
  --sidebar-w: 280px;
}

:root[data-theme="light"] {
  --bg: #f5f4f1;
  --surface: #ffffff;
  --surface-2: #f0eee9;
  --surface-3: #e6e3dc;
  --text: #1c1a17;
  --text-muted: #6b6660;
  --text-faint: #9c958c;
  --border: #e6e2da;
  --border-strong: #d6cfc4;
  --accent: #16a34a;
  --accent-hover: #14903f;
  --accent-weak: #e7f5ec;
  --accent-text: #15803d;
  --accent-contrast: #ffffff;
  --danger: #c0392b;
  --star: #e0a92e;
  --radius-card: 14px;
  --radius-control: 10px;
  --radius-pill: 999px;
  --rail-w: 64px;
  --sidebar-w: 280px;
}
```

- [ ] **Step 2: Verify dark is default + nothing breaks.** Run `npx tsc --noEmit` (clean — CSS only, but confirms no import broke). Start the maket, click **Mail**: the existing layout should now render dark with green accents (cards still old-style — that's later tasks). Confirm via `preview_screenshot` and `preview_console_logs` (no errors).

- [ ] **Step 3: Commit.**
```bash
git add src/styles/theme.css
git commit -m "feat(ui): dark-default token palette + green accent + radius tokens"
```

---

## Task 2: The shell — IconRail, Sidebar, and App.tsx rewiring

This is the structural core: new left rail + merged sidebar, `Header`/`FolderRail` retired, calendar week-nav moved into `CalendarView`.

**Files:**
- Create: `src/components/IconRail.tsx`
- Create: `src/components/Sidebar.tsx`
- Modify: `src/App.tsx` (render the shell; remove `<Header>`/`<FolderRail>` usage; keep all handlers)
- Modify: `src/components/CalendarView.tsx` (add a week-nav header strip, since the global Header is gone)
- Delete: `src/components/Header.tsx`, `src/components/FolderRail.tsx`
- Modify: `src/styles/app.css` (shell/rail/sidebar layout)

- [ ] **Step 1: Create `IconRail.tsx`.** Vertical 64px rail. Reuses `useTheme` for the theme toggle and the existing view/compose/settings handlers.

```tsx
import { Flame, Inbox, CalendarDays, Settings as SettingsIcon, Plus, Sun, Moon } from "lucide-react";
import { useTheme } from "../theme";

type View = "mail" | "calendar";

export function IconRail({
  view,
  onSelectView,
  onCompose,
  onSettings,
  account,
}: {
  view: View;
  onSelectView: (v: View) => void;
  onCompose: () => void;
  onSettings: () => void;
  account: string | null;
}) {
  const { theme, cycleTheme } = useTheme();
  const initials = (account ?? "?").slice(0, 2).toUpperCase();
  return (
    <nav className="icon-rail" aria-label="Primary">
      <div className="rail-brand" aria-hidden><Flame size={20} /></div>
      <button className={`rail-item${view === "mail" ? " active" : ""}`} aria-label="Mail" aria-current={view === "mail"} onClick={() => onSelectView("mail")}><Inbox size={20} /></button>
      <button className={`rail-item${view === "calendar" ? " active" : ""}`} aria-label="Calendar" aria-current={view === "calendar"} onClick={() => onSelectView("calendar")}><CalendarDays size={20} /></button>
      <button className="rail-item" aria-label="Settings" onClick={onSettings}><SettingsIcon size={20} /></button>
      <div className="rail-spacer" />
      <button className="rail-item" aria-label="Theme" onClick={cycleTheme}>{theme === "light" ? <Moon size={18} /> : <Sun size={18} />}</button>
      <button className="rail-item rail-compose" aria-label="Compose" onClick={onCompose}><Plus size={20} /></button>
      <button className="rail-avatar" aria-label="Account" onClick={onSettings}>{initials}</button>
    </nav>
  );
}
```

- [ ] **Step 2: Create `Sidebar.tsx`.** Compose button + Smart Inbox (streams, with unread counts) + Saved (Pinned→starred) + Folders + Labels. Reuses `STREAMS`, `FOLDERS`, `filterByStream`.

```tsx
import { PenSquare, Inbox, Users, Bell, Newspaper, Star, Send, FileText, Archive, Trash2, AlertOctagon, Tag } from "lucide-react";
import { STREAMS, filterByStream, type Stream } from "../lib/streams";
import type { Label, MessagePreview } from "../lib/api";

const STREAM_ICON: Record<Stream, React.ReactNode> = {
  all: <Inbox size={16} />, people: <Users size={16} />, notifications: <Bell size={16} />, newsletters: <Newspaper size={16} />,
};
const FOLDER_ITEMS = [
  { key: "sent", label: "Sent", icon: <Send size={16} /> },
  { key: "drafts", label: "Drafts", icon: <FileText size={16} /> },
  { key: "archive", label: "Archive", icon: <Archive size={16} /> },
  { key: "trash", label: "Trash", icon: <Trash2 size={16} /> },
  { key: "spam", label: "Spam", icon: <AlertOctagon size={16} /> },
];

export function Sidebar({
  messages,
  stream,
  onSelectStream,
  folder,
  onSelectFolder,
  labels,
  onCompose,
}: {
  messages: MessagePreview[];
  stream: Stream;
  onSelectStream: (s: Stream) => void;
  folder: string;
  onSelectFolder: (f: string) => void;
  labels: Label[];
  onCompose: () => void;
}) {
  const inInbox = folder === "inbox";
  const unread = (s: Stream) => filterByStream(messages, s).filter((m) => m.label_ids.includes("UNREAD")).length;
  return (
    <aside className="sidebar">
      <button className="compose-btn" onClick={onCompose}><PenSquare size={16} /> Compose</button>
      <div className="sidebar-scroll">
        <div className="sb-section">Smart Inbox</div>
        {STREAMS.map((s) => {
          const n = unread(s.key);
          const active = inInbox && stream === s.key;
          return (
            <button key={s.key} className={`sb-item${active ? " active" : ""}`} onClick={() => { onSelectFolder("inbox"); onSelectStream(s.key); }}>
              <span className="sb-ic">{STREAM_ICON[s.key]}</span><span className="sb-label">{s.label}</span>
              {n > 0 && <span className="sb-count">{n}</span>}
            </button>
          );
        })}
        <div className="sb-section">Saved</div>
        <button className={`sb-item${folder === "starred" ? " active" : ""}`} onClick={() => onSelectFolder("starred")}>
          <span className="sb-ic"><Star size={16} /></span><span className="sb-label">Pinned</span>
        </button>
        <div className="sb-section">Folders</div>
        {FOLDER_ITEMS.map((f) => (
          <button key={f.key} className={`sb-item${folder === f.key ? " active" : ""}`} onClick={() => onSelectFolder(f.key)}>
            <span className="sb-ic">{f.icon}</span><span className="sb-label">{f.label}</span>
          </button>
        ))}
        {labels.length > 0 && <div className="sb-section">Labels</div>}
        {labels.map((l) => (
          <button key={l.id} className={`sb-item${folder === l.id ? " active" : ""}`} onClick={() => onSelectFolder(l.id)}>
            <span className="sb-ic"><Tag size={16} /></span><span className="sb-label">{l.name}</span>
          </button>
        ))}
      </div>
    </aside>
  );
}
```

- [ ] **Step 3: Rewire `App.tsx` render.** Remove the `import { Header }` and `import { FolderRail }` lines; add `import { IconRail } from "./components/IconRail"` and `import { Sidebar } from "./components/Sidebar"`. Replace the main authenticated render (`<div className="app"> <Header .../> ... </div>`, around lines 736–818) with the shell below. **Keep every existing handler/state** — only the JSX structure changes. Search moves into the `MessageList` header (Task 3); pass `onSearch`/`onClearSearch`/`searchQuery`/`searching`/`inSearch` into `MessageList`. Keep the `SplitView` for the list↔reading panes. Keep all modals (`ComposeModal`, `SettingsModal`, `NotesModal`, `LabelPicker`, `UndoToast`) exactly as they are.

```tsx
<div className="app">
  <div className="shell">
    <IconRail
      view={view}
      onSelectView={setView}
      onCompose={openNewCompose}
      onSettings={() => setSettingsOpen(true)}
      account={account}
    />
    {view === "mail" ? (
      <>
        <Sidebar
          messages={messages}
          stream={stream}
          onSelectStream={(s) => { setStream(s); setSelectedId(null); clearSelection(); clearUndo(); }}
          folder={folder}
          onSelectFolder={handleSelectFolder}
          labels={labels}
          onCompose={openNewCompose}
        />
        <SplitView
          /* keep the SAME left/right props the old SplitView used for MessageList | ReadingPane */
          left={ /* <MessageList .../> exactly as before, now also receiving search props (Task 3) */ }
          right={ /* <ReadingPane .../> exactly as before */ }
        />
      </>
    ) : (
      <CalendarView weekStart={weekStart} onPrevWeek={() => setWeekStart((w) => addWeeks(w, -1))} onToday={() => setWeekStart(startOfWeek(new Date()))} onNextWeek={() => setWeekStart((w) => addWeeks(w, 1))} rangeLabel={weekRangeLabel(weekStart)} />
    )}
  </div>
  {error && <div className="error-bar">…</div>}
  {/* modals unchanged */}
</div>
```

Also simplify the **unauthenticated** branch (around line 713): drop `<Header>`, keep a centered connect screen (`.connect-screen` with the `Flame` brand + connect button).

- [ ] **Step 4: Move calendar week-nav into `CalendarView`.** `CalendarView` previously relied on `Header.calendar` for prev/today/next + range label. Add those four optional props (`onPrevWeek`, `onToday`, `onNextWeek`, `rangeLabel`) and render a small toolbar strip at the top of the calendar (reuse its existing "New event"/"Notes" toolbar row — add the week-nav buttons + label to it). Existing calendar logic is unchanged.

- [ ] **Step 5: Delete the retired components.**
```bash
git rm src/components/Header.tsx src/components/FolderRail.tsx
```
Then `grep -rn "Header\|FolderRail" src/` and confirm no remaining imports/usages (other than unrelated words). Fix any stragglers.

- [ ] **Step 6: Add shell/rail/sidebar CSS** to `src/styles/app.css` (append; remove the now-dead `.app-header`/`.header-*`/`.folder-rail*` rules):

```css
.shell { flex: 1; min-height: 0; display: flex; background: var(--bg); }
.icon-rail { width: var(--rail-w); flex-shrink: 0; display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 14px 0; background: var(--bg); border-right: 1px solid var(--border); }
.rail-brand { display: flex; align-items: center; justify-content: center; width: 40px; height: 40px; border-radius: 12px; background: linear-gradient(135deg, var(--accent), var(--accent-hover)); color: var(--accent-contrast); margin-bottom: 6px; }
.rail-item { display: flex; align-items: center; justify-content: center; width: 40px; height: 40px; border: none; border-radius: 12px; background: transparent; color: var(--text-muted); cursor: pointer; }
.rail-item:hover { background: var(--surface-2); color: var(--text); }
.rail-item.active { background: var(--accent-weak); color: var(--accent-text); }
.rail-compose { color: var(--accent-text); }
.rail-spacer { flex: 1; }
.rail-avatar { width: 36px; height: 36px; border-radius: 50%; border: none; background: linear-gradient(135deg, var(--accent), var(--accent-hover)); color: var(--accent-contrast); font-size: 12px; font-weight: 600; cursor: pointer; }

.sidebar { width: var(--sidebar-w); flex-shrink: 0; display: flex; flex-direction: column; gap: 10px; padding: 14px; background: var(--bg); border-right: 1px solid var(--border); }
.compose-btn { display: flex; align-items: center; justify-content: center; gap: 8px; height: 44px; border: none; border-radius: var(--radius-control); background: linear-gradient(135deg, var(--accent), var(--accent-hover)); color: var(--accent-contrast); font-size: 14px; font-weight: 600; cursor: pointer; }
.compose-btn:hover { filter: brightness(1.05); }
.sidebar-scroll { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
.sb-section { padding: 14px 10px 4px; font-size: 11px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; color: var(--text-faint); }
.sb-item { display: flex; align-items: center; gap: 10px; height: 38px; padding: 0 10px; border: none; border-radius: var(--radius-control); background: transparent; color: var(--text-muted); font-size: 14px; cursor: pointer; text-align: left; }
.sb-item:hover { background: var(--surface-2); color: var(--text); }
.sb-item.active { background: var(--accent-weak); color: var(--accent-text); font-weight: 500; }
.sb-ic { display: flex; width: 18px; color: inherit; }
.sb-label { flex: 1; }
.sb-count { font-size: 12px; font-weight: 600; color: var(--text-muted); background: var(--surface-2); border-radius: var(--radius-pill); padding: 1px 8px; }
.sb-item.active .sb-count { background: var(--accent); color: var(--accent-contrast); }
```

- [ ] **Step 7: Verify.** `npx tsc --noEmit` clean. Maket → Mail: icon rail (green flame, Mail active), sidebar sections (Smart Inbox w/ counts, Saved→Pinned, Folders, Labels), Compose green. Click streams/folders → list updates. Click Calendar → calendar renders with its own week-nav. Toggle theme via rail. `preview_console_logs` clean.

- [ ] **Step 8: Commit.**
```bash
git add -A
git commit -m "feat(ui): icon rail + merged sidebar shell; retire Header/FolderRail"
```

---

## Task 3: Message list — grouped cards + in-list search header

**Files:**
- Modify: `src/components/MessageList.tsx` (add a list-column header with title + search + sync; restyle group headers)
- Modify: `src/components/MessageItem.tsx` (rounded card layout: avatar, name+time, bold subject, snippet, star)
- Modify: `src/styles/app.css` (`.msglist*`, `.msg-card*`, group-header rules)

- [ ] **Step 1: Add the list header to `MessageList`.** Above the existing list body, render a header: a title (`"Smart Inbox"` for inbox, else the folder/stream label), a search `<input>` wired to new props `onSearch(q)`, `onClearSearch()`, `searchQuery`, `searching`, and a Sync icon button wired to a new `onSync` prop. Thread these props from `App.tsx` (they already exist there: `handleSearch`, `handleClearSearch`, `searchQuery`, `searching`, `handleSync`, `busy`). Keep the existing batch-action bar and grouping logic.

```tsx
<div className="list-head">
  <div className="list-title">{title ?? "Smart Inbox"}</div>
  <button className="list-tool" aria-label="Sync" disabled={busy} onClick={onSync}><RefreshCw size={16} /></button>
</div>
<div className="list-search">
  <Search size={16} />
  <input value={searchQuery} placeholder="Search mail" onChange={(e) => onSearch(e.target.value)} />
</div>
```

- [ ] **Step 2: Restyle group headers** (the PEOPLE/NOTIFICATIONS rows) with a colored icon + small-caps label: class `.group-head` with an `.group-ic`. Keep the existing group iteration.

- [ ] **Step 3: Restyle `MessageItem` as a card.** Wrap each row in `.msg-card` (rounded, `var(--surface)` bg, hover lift, `.selected` gets a green left border + `--accent-weak` tint). Layout: avatar (colored initials), top row (name + time), subject (bold), snippet (muted, 1 line ellipsis), trailing star toggle (filled `var(--star)` when starred). Keep the checkbox for batch-select (show on hover/selection) and the existing `onSelect`/`onToggleSelect`/`onStar` handlers. **Omit** any clock/snooze control.

- [ ] **Step 4: Add card CSS** to `app.css`:
```css
.msglist { background: var(--bg); }
.list-head { display: flex; align-items: center; gap: 8px; padding: 14px 16px 8px; }
.list-title { font-size: 20px; font-weight: 700; flex: 1; }
.list-tool { display: flex; align-items: center; justify-content: center; width: 34px; height: 34px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--text-muted); cursor: pointer; }
.list-search { display: flex; align-items: center; gap: 8px; margin: 0 16px 8px; padding: 0 12px; height: 40px; border-radius: var(--radius-control); background: var(--surface); border: 1px solid var(--border); color: var(--text-faint); }
.list-search input { flex: 1; border: none; background: transparent; color: var(--text); font-size: 14px; outline: none; }
.group-head { display: flex; align-items: center; gap: 8px; padding: 14px 16px 6px; font-size: 11px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--text-faint); }
.msg-card { display: flex; gap: 12px; margin: 4px 12px; padding: 12px 14px; border-radius: var(--radius-card); background: var(--surface); border: 1px solid transparent; cursor: pointer; }
.msg-card:hover { background: var(--surface-2); }
.msg-card.selected { background: var(--accent-weak); border-left: 3px solid var(--accent); }
.msg-card .subject { font-weight: 600; color: var(--text); }
.msg-card .snippet { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.msg-card .when { color: var(--text-faint); font-size: 12px; }
.msg-star { background: none; border: none; color: var(--text-faint); cursor: pointer; }
.msg-star.on { color: var(--star); }
```

- [ ] **Step 5: Verify.** `npx tsc --noEmit` clean. Maket → Mail: grouped cards render (PEOPLE/NOTIFICATIONS headers, rounded cards, avatars, bold subjects, star toggles), selected card shows green border+tint, search box filters, Sync button present. `preview_console_logs` clean.

- [ ] **Step 6: Commit.**
```bash
git add -A
git commit -m "feat(ui): grouped message cards + in-list search/sync header"
```

---

## Task 4: Reading pane restyle

**Files:**
- Modify: `src/components/ReadingPane.tsx` (chrome only — actions/logic unchanged)
- Modify: `src/styles/app.css` (`.reading-*` rules)

- [ ] **Step 1: Restyle the action bar.** Top-left: a green **Reply** button (`.btn-accent` style) + reply-all + forward icon buttons. Top-right: an icon-button cluster (star, archive, mark-unread, labels) using a new `.read-tool` pill style. Keep the existing `onReply`/`onReplyAll`/`onForward`/`onArchive`/`onToggleStar`/`onMarkUnread`/`onOpenLabels` handlers and the **Trash-folder variant** (Restore / Delete forever) exactly as in the current `inTrash` branch.

- [ ] **Step 2: Restyle the header block.** A green category chip (derive from the message's stream/category, e.g. "People"), large subject (`28px/700`), sender row (avatar-lg, name, email, right-aligned time + "to me"), a divider, then the body. Keep the existing body rendering (`fetchMessageBody`, image-blocking, attachments strip) unchanged.

- [ ] **Step 3: Add a styled inline reply affordance** at the bottom: a rounded `.reply-affordance` bar reading `↩ Reply to <name> …` that calls the existing `onReply(msg)` on click. (No new compose logic — it just triggers the existing reply flow.)

- [ ] **Step 4: Add reading CSS** to `app.css`:
```css
.reading-pane { background: var(--bg); padding: 16px 24px; overflow-y: auto; }
.read-bar { display: flex; align-items: center; gap: 8px; margin-bottom: 20px; }
.read-bar .spacer { flex: 1; }
.read-tool { display: flex; align-items: center; justify-content: center; width: 38px; height: 38px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--text-muted); cursor: pointer; }
.read-tool:hover { background: var(--surface-2); color: var(--text); }
.read-tool.on { background: var(--accent-weak); color: var(--accent-text); border-color: var(--accent); }
.read-chip { display: inline-flex; align-items: center; gap: 6px; padding: 4px 12px; border-radius: var(--radius-pill); background: var(--accent-weak); color: var(--accent-text); font-size: 13px; font-weight: 500; }
.read-subject { font-size: 28px; font-weight: 700; margin: 14px 0 18px; line-height: 1.25; }
.read-from { display: flex; align-items: center; gap: 12px; }
.read-divider { height: 1px; background: var(--border); margin: 18px 0; }
.reply-affordance { display: flex; align-items: center; gap: 10px; margin-top: 24px; padding: 16px; border-radius: var(--radius-card); background: var(--surface); border: 1px solid var(--border); color: var(--text-muted); cursor: text; }
```

- [ ] **Step 5: Verify.** `npx tsc --noEmit` clean. Maket → Mail → open a message: green Reply, category chip, big subject, sender row, body, inline reply bar; clicking Reply opens compose; star/archive work; open a Trash message → Restore/Delete-forever still present. `preview_console_logs` clean.

- [ ] **Step 6: Commit.**
```bash
git add -A
git commit -m "feat(ui): restyle reading pane (chip, icon actions, inline reply)"
```

---

## Task 5: Green app icon

**Files:**
- Modify: `src-tauri/icons/source/ember-icon.svg`
- Modify (regenerate): `src-tauri/icons/*`

- [ ] **Step 1: Recolor the source SVG** — change the `#bg` gradient stops from coral/pink to green; keep the dark flame + inner highlight geometry. Replace the gradient stops in `src-tauri/icons/source/ember-icon.svg`:
```xml
<stop offset="0" stop-color="#3FBF6F"/>
<stop offset="1" stop-color="#1F8F52"/>
```
(Everything else in the file stays.)

- [ ] **Step 2: Regenerate the set.**
```bash
npm run tauri icon src-tauri/icons/source/ember-icon.svg 2>&1 | tail -5
rm -rf src-tauri/icons/android src-tauri/icons/ios
```

- [ ] **Step 3: Verify.** Use the Read tool on `src-tauri/icons/icon.png` and `src-tauri/icons/32x32.png`: green gradient squircle + dark flame, clean at small size. `git status -s src-tauri/icons/` shows the flat set modified, no android/ios.

- [ ] **Step 4: Commit.**
```bash
git add src-tauri/icons/
git commit -m "feat(icon): recolor app icon green to match the redesign"
```

---

## Final verification

- [ ] `npx tsc --noEmit` — clean.
- [ ] `cd src-tauri && cargo test` — unchanged/green (no Rust touched, but confirm).
- [ ] Maket end-to-end: rail (Mail/Calendar/Settings/Compose/theme/avatar), sidebar (streams+counts, Pinned, folders, labels), grouped cards + selection, search, reading pane + reply, Calendar view + week-nav, light/dark toggle both legible. No console errors (`preview_console_logs`).
- [ ] Compare side-by-side with the mockup; confirm green where the mockup is orange.
- [ ] `grep -rn "Header\|FolderRail" src/` — no dead references.

## Self-review notes (done while writing)

- **Spec coverage:** icon rail ✓ (T2), merged sidebar w/ Smart Inbox+Saved+Folders+Labels ✓ (T2), counts ✓ (T2, unread>0), grouped cards ✓ (T3), in-list search ✓ (T3), reading-pane chip/actions/inline-reply ✓ (T4), dark-default+green+toggle ✓ (T1), green icon ✓ (T5), calendar re-skin + week-nav relocation ✓ (T2). Deferred items (Snooze, per-row clock, custom titlebar) intentionally absent.
- **Type consistency:** `View`="mail"|"calendar"; `Stream` from `lib/streams`; `IconRail`/`Sidebar` prop names match the `App.tsx` wiring in T2; `onSync`/search props added to `MessageList` in T3 come from existing `App.tsx` handlers (`handleSync`, `handleSearch`, `handleClearSearch`, `searchQuery`, `searching`, `busy`).
- **Pixel tuning** (exact spacing/hues) is expected during maket verification per the spec; token names are fixed.
