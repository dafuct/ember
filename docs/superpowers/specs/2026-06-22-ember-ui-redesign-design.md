# Ember UI Redesign — "Smart Inbox" dark shell (design)

**Date:** 2026-06-22
**Status:** Approved (brainstorming) → ready for implementation plan

## Goal

Restyle and restructure Ember's mail UI to match the provided "Smart Inbox" mockup: a deep-dark, rounded, card-based layout with a far-left **icon rail**, a **merged left sidebar** (Compose + Smart Inbox + Saved + Folders), a **grouped message-card list**, and a **restyled reading pane**. Adopt the mockup's *structure and polish* while keeping Ember's **green** brand accent (the mockup's orange is replaced by green everywhere) and **keeping the light/dark toggle** (dark becomes the default).

This is a **presentation + information-architecture** change. No mail/calendar behavior changes; existing data flows, commands, and component logic are reused.

## Decisions (from brainstorming)

- **Scope:** Look + layout. Reuse existing features; introduce no backend work.
- **Accent:** **Green** (Ember's existing brand), not the mockup's orange. Every orange touchpoint → green.
- **Theme:** **Dark is the new default**; the light/dark toggle stays, both driven by CSS tokens. Light gets a matching restyle.
- **App icon:** Redo the flame in **green** (green gradient squircle + dark flame) via the existing `tauri icon` pipeline.
- **Pinned = Starred** (the "Saved" section's Pinned item maps to the existing Starred folder).
- **Deferred (NOT built now):** Snooze (the per-row clock + a "Snoozed" view), and a custom centered titlebar. Both are clean follow-ups.
- **Two resolved calls:** omit the "Snoozed" sidebar item and the per-row clock icon (no dead controls); keep standard macOS window chrome this pass.

## Approach

**Restructure the shell + retheme via tokens, reusing existing component logic.** Keep the working internals (message data, reading-pane actions, compose, calendar, folders/streams) and change *presentation*: new layout containers and card styling plus a rewritten token palette. Add new components only where structure demands (`IconRail`, `Sidebar`); re-skin everything else in place.

*Alternatives considered:* a ground-up new component tree (more fidelity, much more churn/risk) and a CSS-only retint (too little to produce the icon rail / merged sidebar). The middle path fits the codebase and the scope.

## Architecture — the 4-zone shell

Replace today's `Header` + 3-pane `SplitView` with a horizontal shell. `App.tsx` owns the same state (account, folder, stream, selection, search, compose, calendar week); only the rendered structure changes.

```
.app (column)
  .shell (row, flex:1)
    <IconRail/>     ~64px  fixed
    <Sidebar/>      ~280px fixed (internally scrollable)
    <MessageList/>  ~380px  resizable (SplitView retained for list↔reading)
    <ReadingPane/>  flex:1  resizable
```

When **Calendar** is selected in the rail, the content area (sidebar/list/reading region) is replaced by the existing `CalendarView` (unchanged logic, restyled to the dark tokens).

### IconRail (new) — `src/components/IconRail.tsx`
Vertical 64px strip. Top→bottom: **green flame brand mark**, **Mail** (inbox icon, active by default), **Calendar**, **Settings** (opens existing `SettingsModal`), spacer, **⊕ Compose** shortcut, **account avatar** (bottom; opens account/settings). Mail/Calendar replace today's header tabs; active item shows a green tint + rounded highlight. Props: `view` ("mail"|"calendar"), `onSelectView`, `onOpenSettings`, `onCompose`, `account`.

### Sidebar (new) — `src/components/Sidebar.tsx`
Absorbs today's `Header` stream-nav + `FolderRail`. Top: full-width green **Compose** button. Then a scrollable column of labeled sections:
- **SMART INBOX** — All / People / Notifications / Newsletters (today's `STREAMS` from `lib/streams.ts`), each with a count derived from the loaded `messages` (unread-or-total per stream; exact count rule fixed during implementation — default: unread count, hidden when 0).
- **SAVED** — **Pinned** → the Starred folder.
- **FOLDERS** — Sent / Drafts / Archive / Trash / Spam (today's `FOLDERS` from `lib/folders.ts`).
- **LABELS** — existing user labels (unchanged data).
Selecting a stream filters the inbox; selecting a folder/label loads that folder (existing handlers `setStream` / `handleSelectFolder`). `FolderRail.tsx` is folded into `Sidebar` and removed.

### MessageList / MessageItem — restyle to cards
Reuse the existing grouping (`groupByStream`) and selection/batch logic. Restyle:
- Section headers (PEOPLE / NOTIFICATIONS / …) get a colored icon + small-caps label (matches mockup).
- Each row becomes a **rounded card**: avatar, name + time row, **bold subject**, muted snippet, trailing **star** toggle. Selected card = green left-border + subtle tint.
- Search input moves into the list column header (out of the global header), with a "Smart Inbox" title + filter button.
- The per-row **clock/snooze** icon is omitted (deferred).

### ReadingPane — restyle
Same actions/logic, new chrome: a green category chip, green **Reply** + reply-all/forward, top-right **icon-button cluster** (star, archive, mark-unread, labels), large subject, sender row (avatar, name, email, time, "to me"), divider, body, and a styled inline **"Reply to <name>…"** affordance that triggers the existing reply flow. Trash-folder variant (Restore / Delete forever) is preserved.

## Theme tokens — `src/styles/theme.css`

Rewrite the palette. **Dark default** (deep charcoal-navy), **green accent kept**, plus new radius/spacing tokens. Indicative dark values (final hues tuned during implementation against the mockup):

```
--bg:#16161b  --surface:#1d1d24  --surface-2:#26262f
--text:#f0ece4  --text-muted:#a7a39c  --text-faint:#6f6a63
--border:#2a2a33  --border-strong:#3a3a45
--accent:#3fbf6f  --accent-weak:#16271c  --accent-text:#7ee7a3  --accent-contrast:#0e160f
--radius-card:14px  --radius-control:10px
```

The light theme keeps the same token names with a light, green-accented set so the toggle still works. `data-theme` switching is unchanged.

## App icon — green

Update `src-tauri/icons/source/ember-icon.svg`: a **green** diagonal gradient squircle with the existing dark flame + inner-flame highlight (same geometry as the current icon, green instead of coral/pink). Regenerate the set with `npm run tauri icon src-tauri/icons/source/ember-icon.svg`; remove the generator's `android/`/`ios/` output (desktop-only, matching prior icon commits). No `tauri.conf.json` change.

## Files touched

| File | Change |
|---|---|
| `src/styles/theme.css` | Rewrite palette: dark default + green accent + light variant + radius/spacing tokens |
| `src/styles/app.css` | Major rewrite: shell layout, icon rail, sidebar sections, message cards, reading-pane chrome |
| `src/App.tsx` | Render the 4-zone shell; wire rail view-switch + sidebar (stream/folder) in place of `Header` tabs |
| `src/components/IconRail.tsx` | **New** — left icon rail |
| `src/components/Sidebar.tsx` | **New** — Compose + Smart Inbox + Saved + Folders + Labels |
| `src/components/Header.tsx` | Removed/retired (its nav/account/settings move to rail + sidebar) |
| `src/components/FolderRail.tsx` | Removed (folded into `Sidebar`) |
| `src/components/MessageList.tsx`, `MessageItem.tsx` | Restyle to grouped cards; move search into the list header |
| `src/components/ReadingPane.tsx` | Restyle chrome (chip, icon-button cluster, inline reply affordance) |
| `src/components/CalendarView.tsx` & calendar styles | Re-skin to dark tokens (no logic change) |
| `src-tauri/icons/source/ember-icon.svg` + `src-tauri/icons/*` | Green icon + regenerated set |

## Out of scope

Snooze (feature, per-row clock, Snoozed view); a custom centered titlebar; any "Pin" concept distinct from Star; mail/calendar/compose behavior changes; new backend commands or migrations.

## Verification

No automated tests for visual work; the Rust/TS suites stay green and untouched. Verify by:
- `npx tsc --noEmit` clean; `cargo test` unaffected.
- Drive the **browser maket** (`npm run dev`, port-5190 `ember-maket` launch config): confirm the icon rail, sidebar sections, grouped cards, selected-card green accent, and restyled reading pane render; toggle light/dark; switch Mail↔Calendar; open Compose; no console errors.
- Visually compare against the mockup (green where the mockup is orange).
- Re-view the regenerated `icon.png` / `32x32.png` (green flame).

## Risks / notes

- **Largest change is `app.css` + `App.tsx` structure.** Mitigate by phasing the plan: (1) theme tokens, (2) shell + IconRail + Sidebar, (3) message cards, (4) reading pane, (5) green icon — each independently maket-verifiable.
- Per-stream **counts** require deriving from loaded messages; if a count is expensive or noisy, fall back to showing it only for non-zero unread.
- `Header.tsx`/`FolderRail.tsx` removal must not orphan props/handlers in `App.tsx` — rewire to the rail/sidebar.
