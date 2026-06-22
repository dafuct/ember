# Ember — App icon ("Ember & sparks" flame) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the default Tauri logo with a custom "Ember & sparks" flame app icon, delivered as a committed source SVG plus the regenerated platform icon set.

**Architecture:** Commit a 1024×1024 master `src-tauri/icons/source/ember-icon.svg` (the approved option B), then run `npm run tauri icon` on it to regenerate every file in `src-tauri/icons/` in place (same filenames, so `tauri.conf.json` is unchanged).

**Tech Stack:** the Tauri CLI icon generator via `npm run tauri icon` (`@tauri-apps/cli`, already installed; `cargo tauri` is NOT a cargo subcommand here — use the npm script). Hand-authored SVG.

**Working directory:** repo root `/Users/makar/dev/ownmail`.

---

## File Structure

| File | Create/Modify | Responsibility |
|---|---|---|
| `src-tauri/icons/source/ember-icon.svg` | **Create** | 1024×1024 editable master artwork (option B) |
| `src-tauri/icons/*` (`icon.png`, `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, `Square*Logo.png`, `StoreLogo.png`) | Regenerate | Platform icon set, overwritten by `tauri icon` |

---

## Task 1: Create the master source SVG

**Files:**
- Create: `src-tauri/icons/source/ember-icon.svg`

- [ ] **Step 1: Write the source SVG**

Create `src-tauri/icons/source/ember-icon.svg` with exactly this content (this is the approved option B — deep-warm-brown squircle, red→orange flame with amber core, three sparks — inset by a ~9% transparent margin via `translate(92,92) scale(4.666…)` so the rounded corners read natively):

```xml
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="gB" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#E8401F"/>
      <stop offset="1" stop-color="#FF9A33"/>
    </linearGradient>
    <linearGradient id="gBi" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFC24D"/>
      <stop offset="1" stop-color="#FFEFB0"/>
    </linearGradient>
  </defs>
  <g transform="translate(92,92) scale(4.6666667)">
    <rect x="0" y="0" width="180" height="180" rx="42" fill="#2A1410"/>
    <circle cx="120" cy="58" r="4.5" fill="#FFB347"/>
    <circle cx="64" cy="74" r="3" fill="#FF8A2B"/>
    <circle cx="108" cy="40" r="2.5" fill="#FFD27A"/>
    <path d="M90 44 C 108 72 118 88 112 114 C 107 134 99 146 90 148 C 81 146 73 134 68 114 C 64 96 78 90 86 98 C 78 78 82 62 90 44 Z" fill="url(#gB)"/>
    <path d="M90 88 C 100 104 106 114 102 128 C 99 139 95 144 90 145 C 85 144 81 139 78 128 C 75 116 84 110 88 116 C 83 102 86 96 90 88 Z" fill="url(#gBi)"/>
  </g>
</svg>
```

- [ ] **Step 2: Verify it is well-formed XML**

Run: `python3 -c "import xml.dom.minidom,sys; xml.dom.minidom.parse('src-tauri/icons/source/ember-icon.svg'); print('well-formed SVG')"`
Expected: `well-formed SVG`.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/icons/source/ember-icon.svg
git commit -m "feat(icon): add Ember & sparks flame master SVG"
```

---

## Task 2: Regenerate the icon set + verify

**Files:**
- Modify (regenerate): `src-tauri/icons/*`

- [ ] **Step 1: Regenerate all icons from the source SVG**

Run: `npm run tauri icon src-tauri/icons/source/ember-icon.svg 2>&1 | tail -20`
Expected: it prints the icons it wrote (e.g. `Appx logo`, `icon.icns`, `icon.ico`, the PNG sizes) and exits 0. The files land in `src-tauri/icons/` by default (the configured tauri dir). If it errors that the output dir is wrong, re-run with `-o src-tauri/icons` appended.

- [ ] **Step 2: Confirm the icon binaries changed**

Run: `git status --porcelain src-tauri/icons/ | grep -vE "source/" | head`
Expected: a list of modified icon files (`icon.png`, `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, the `Square*Logo.png`/`StoreLogo.png`).

- [ ] **Step 3: Visually verify the regenerated icon (full size)**

Use the Read tool on `src-tauri/icons/icon.png` (Read renders PNGs). Confirm: the deep-warm-brown rounded-square background, the red→orange flame with a brighter amber core, the small sparks above it, and transparent (rounded) corners — i.e. it matches the approved option B, not the old cyan/yellow Tauri logo.

- [ ] **Step 4: Visually verify the small size reads**

Use the Read tool on `src-tauri/icons/32x32.png`. Confirm the flame is still recognizable at 32 px and isn't clipped. (Sparks may visually merge at this size — acceptable.)

- [ ] **Step 5: Confirm tauri.conf.json still resolves (no change needed)**

Run: `python3 -c "import json,os; c=json.load(open('src-tauri/tauri.conf.json')); print([f for f in c['bundle']['icon'] if not os.path.exists('src-tauri/'+f)] or 'all bundle.icon paths exist')"`
Expected: `all bundle.icon paths exist`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/icons/
git commit -m "feat(icon): regenerate icon set from Ember flame source"
```

---

## Owner verification (manual, not a code step)

`npm run tauri build` → the built `Ember.app` (and its Dock icon) shows the new flame. Pairs with the local code-signing already set up. The Rust/TS test suites are untouched by this change and remain green.

---

## Self-Review notes (already applied)

- **Spec coverage:** master source SVG committed (Task 1) ✓; full icon set regenerated in place via `npm run tauri icon` (Task 2) ✓; `tauri.conf.json` unchanged + verified to still resolve (Task 2 Step 5) ✓; visual verification at full + small size (Task 2 Steps 3-4) ✓; in-app header flame and app code untouched (not in any task) ✓.
- **No placeholders:** the complete SVG is inline; every command is concrete with expected output.
- **Consistency:** the same source path `src-tauri/icons/source/ember-icon.svg` is created in Task 1 and consumed in Task 2; the SVG is the exact approved option B scaled into a 1024 canvas with a 92px (~9%) margin.
