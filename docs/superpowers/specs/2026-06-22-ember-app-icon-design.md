# Ember — App icon ("Ember & sparks" flame) — Design Spec

**Status:** Approved design (2026-06-22). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Replace the default Tauri logo (the cyan/yellow interlocking mark currently in `src-tauri/icons/`) with a custom **"Ember & sparks" flame** icon that fits the app's name and its in-app flame motif. Deliver it as a committed, reproducible source SVG plus the regenerated platform icon set.

**Chosen design (option B from brainstorming):** a rounded-square icon with a **deep warm-brown background**, a **red→orange flame** with a brighter amber inner core, and a few **rising sparks** above it. Rounded corners + a small transparent margin are baked into the source so macOS renders it as a native-looking Dock icon.

**Architecture in one paragraph:** A single committed source file `src-tauri/icons/source/ember-icon.svg` (1024×1024, transparent canvas, the squircle inset with a margin) is the editable master. Running `npm run tauri icon src-tauri/icons/source/ember-icon.svg` rasterizes it and regenerates the **entire existing icon set in place** — `icon.png`, `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, and the Windows `Square*Logo.png` / `StoreLogo.png` tiles — keeping the same filenames. Because `tauri.conf.json`'s `bundle.icon` array already references those names, **no config change is needed**. The macOS Dock icon updates on the next `npm run tauri build`.

**Tech Stack:** the Tauri CLI's `tauri icon` generator (`@tauri-apps/cli`, already present, invoked via `npm run tauri icon`); a hand-authored SVG. **No app code change, no new dependency, no `tauri.conf.json` change.**

**Note:** the project launches the Tauri CLI via the npm script (`npm run tauri …`) — `cargo tauri` is NOT installed as a cargo subcommand on this machine.

---

## Scope

**In scope:**
- New `src-tauri/icons/source/ember-icon.svg` — the 1024×1024 master artwork (option B), committed.
- Regenerate the full icon set in `src-tauri/icons/` via `npm run tauri icon …` (overwrites the existing default-Tauri-logo files in place, same names).

**Out of scope (non-goals):**
- The **in-app header logo** (a separate `lucide` Flame component in the React UI) — left unchanged.
- `tauri.conf.json` (filenames are unchanged, so the `bundle.icon` list still resolves).
- App code, window title, product name, bundle identifier.
- A signed `tauri build` / live Dock verification — owner-verified (pairs with the local code-signing already set up).

---

## Components

### `src-tauri/icons/source/ember-icon.svg` (new master)
- `viewBox="0 0 1024 1024"`, transparent canvas.
- **Squircle background:** a rounded `<rect>` inset by a ~9% margin (≈ x/y 92 → 932, size ≈ 840) with corner radius ≈ 0.225 × side, filled deep warm-brown (`#2A1410`). The margin leaves transparent corners so the Dock icon sits at native size with rounded corners.
- **Flame:** the option-B flame paths (outer red→orange `linearGradient` `#E8401F`→`#FF9A33`, inner amber `#FFC24D`→`#FFEFB0`) scaled to fill the central area of the squircle, bold enough to read at 32 px.
- **Sparks:** 3 small circles (`#FFB347`, `#FF8A2B`, `#FFD27A`) above the flame.
- Pure SVG, no external refs; gradients inline in `<defs>` (fine for raster source art).

### `src-tauri/icons/` (regenerated, not hand-edited)
- All existing icon files overwritten by `tauri icon`. We do not hand-edit the binaries.

### `tauri.conf.json`
- Unchanged. (Verified: `bundle.icon` already lists `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`.)

---

## Verification

- `npm run tauri icon src-tauri/icons/source/ember-icon.svg` completes without error and rewrites the icon files (confirm via `git status` showing the icon binaries modified).
- Re-view the regenerated `src-tauri/icons/icon.png` and `src-tauri/icons/32x32.png`: the flame + sparks render, the background is the deep-brown squircle, and the corners are transparent (rounded), with no clipping of the flame at the small size.
- `npm run tauri build` (owner step) shows the new icon in the Dock / built `Ember.app`.

No automated test — this is asset generation, verified visually + by the `tauri icon` exit status. The Rust/TS test suites are untouched and must stay green.

---

## Known risks & decisions

- **Source SVG committed for reproducibility** — the icon can be re-tweaked and regenerated; the generated binaries are derived artifacts (kept in git as before, since `tauri.conf.json` references them directly).
- **Margin + rounded corners baked into the source** — macOS does not auto-round third-party `.icns`; baking the squircle + ~9% transparent margin yields a native-looking Dock icon at the right visual size.
- **Detail vs small sizes** — the flame is the dominant, bold element so it reads at 32 px; the sparks are decorative and may visually merge at 16 px, which is acceptable.
- **No `tauri.conf.json`/code change, no new dependency** — only a new source asset + regenerated icons.
- **In-app header flame intentionally left as-is** — matching it is a separate, optional follow-up.
