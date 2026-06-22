# Ember — Local stable code-signing — Design Spec

**Status:** Approved design (2026-06-22). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Give the `tauri build` output (`Ember.app`) a **stable code signature** via a local self-signed certificate, so macOS keys its TCC permissions (Microphone, Screen & System Audio Recording) to a constant identity instead of resetting them on every rebuild. This removes the dev-build friction hit during the M24 live-capture E2E (each unsigned/ad-hoc rebuild looked like a new app to TCC and re-prompted). **No Apple Developer account, no cost, no notarization, not distributable** — purely a local developer-experience fix.

**Why it works:** macOS identifies an app for TCC by its code signature's **designated requirement**. Ad-hoc signing (what `cargo tauri dev` and unsigned builds use) has no stable identity, so TCC falls back to the **cdhash**, which changes on every rebuild → the OS treats each build as a new app and re-prompts. Signing with a **named self-signed certificate** yields a constant designated requirement, so a permission granted once **persists across rebuilds** of the signed app.

**Architecture in one paragraph:** A committed, idempotent script `scripts/create-signing-cert.sh` generates (via `openssl`) a 10-year self-signed certificate named **"Ember Dev"** carrying the `codeSigning` extended-key-usage, bundles it into a passwordless `.p12`, and `security import`s it into the login keychain with `-T /usr/bin/codesign` (granting `codesign` access). The generated key/cert/`.p12` files are git-ignored (local secrets). `tauri.conf.json` gains `bundle.macOS.signingIdentity: "Ember Dev"`, so Tauri's bundler runs `codesign --sign "Ember Dev"` on `Ember.app` during `tauri build`. On the **first** signed build macOS shows a one-time *"codesign wants to use a key in your keychain"* prompt → the developer clicks **Always Allow** → subsequent builds sign silently (this sidesteps needing the login-keychain password in the script). A short `docs/` README documents the create → build → Always-Allow → verify flow.

**Tech Stack:** `openssl` + macOS `security`/`codesign` (all present; Xcode + `notarytool` already installed), Tauri 2 bundler config (`tauri.conf.json`). **No new project dependency, no app code change, no Apple account.**

**Scope caveat (explicit):** this signs the **`tauri build`** bundle only — **not** `cargo tauri dev`, which runs the bare binary ad-hoc. To get persistent TCC permissions, run the **built `Ember.app`**, not dev mode. Stably signing the dev binary is a larger, messier change and is out of scope.

---

## Context

Ember is an unsigned local Tauri 2 dev app. The M24 live-capture E2E (2026-06-22) confirmed the dev-build TCC pain: capturing from BlackHole needs Microphone permission, and an unsigned/ad-hoc rebuild resets it. `tauri.conf.json` currently has a `bundle` block with **no** `macOS.signingIdentity`. `security find-identity -v -p codesigning` shows **zero** identities. This milestone adds a stable local identity so permission-dependent features (M24 capture, and ScreenCaptureKit later) are testable against a built app without re-granting every time.

---

## Scope

**In scope:**
- `scripts/create-signing-cert.sh` — idempotent self-signed "Ember Dev" code-signing cert creation + keychain import (`openssl` + `security`).
- `tauri.conf.json` → `bundle.macOS.signingIdentity: "Ember Dev"`.
- `.gitignore` entries for the generated cert artifacts (`*.p12`, the key/cert PEMs under the script's output dir).
- `docs/local-code-signing.md` — the create → build → Always-Allow → verify runbook, incl. the dev-vs-build caveat.

**Out of scope (non-goals):**
- Notarization, Developer ID, distribution to other Macs, Gatekeeper acceptance.
- Hardened runtime + sandbox entitlements (only needed for notarization; the existing `NSMicrophoneUsageDescription` covers mic use unsandboxed).
- Signing the `cargo tauri dev` binary (dev mode stays ad-hoc).
- CI integration.

---

## Components

### `scripts/create-signing-cert.sh`
- Idempotent: if `security find-identity -v -p codesigning` already lists **"Ember Dev"**, print a message and exit 0 (don't duplicate).
- Generate into a gitignored working dir (e.g. `scripts/.signing/`):
  - `openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 3650 -nodes -subj "/CN=Ember Dev" -addext "extendedKeyUsage=codeSigning" -addext "basicConstraints=critical,CA:false"`
  - `openssl pkcs12 -export -inkey key.pem -in cert.pem -out ember-dev.p12 -passout pass:`
- Import: `security import ember-dev.p12 -k "$HOME/Library/Keychains/login.keychain-db" -P "" -T /usr/bin/codesign`
- Verify + report: `security find-identity -v -p codesigning | grep "Ember Dev"`; echo next steps (run `npm run tauri build`, click Always Allow on the first prompt).
- `set -euo pipefail`; clear echos; safe to re-run.

### `tauri.conf.json`
- In `bundle`, add a `macOS` object with `"signingIdentity": "Ember Dev"`. No other bundle changes.

### `.gitignore`
- Add `scripts/.signing/` (the cert/key/`.p12` working dir) so secrets are never committed.

### `docs/local-code-signing.md`
- One-page runbook: purpose (TCC persistence), `bash scripts/create-signing-cert.sh`, `npm run tauri build`, the one-time **Always Allow** prompt, verification (`codesign -dv --verbose=4`), and the **dev-mode-is-still-ad-hoc** caveat. Note the cert is self-signed → Gatekeeper will still warn on other machines (local-only by design).

---

## Verification

- `bash scripts/create-signing-cert.sh` → `security find-identity -v -p codesigning` lists **"Ember Dev"**; re-running the script is a no-op (idempotent).
- `npm run tauri build` succeeds; after the one-time Always-Allow, `codesign -dv --verbose=4 src-tauri/target/release/bundle/macos/Ember.app` shows `Authority=Ember Dev` and a **Designated Requirement** that is identical across two consecutive builds (the stability proof).
- **Owner-verified (manual):** grant Microphone to the built `Ember.app` once, rebuild, relaunch, confirm the permission is still granted and live capture works without re-prompting.

No automated test harness — this is build/keychain configuration verified by the commands above (consistent with how the project verifies infra; the app's Rust/TS test suites are unaffected and must stay green).

---

## Known risks & decisions

- **Self-signed, named identity (not ad-hoc, not Developer ID)** — the minimum that yields a *stable* designated requirement for TCC persistence without an Apple account. Ad-hoc wouldn't be stable; Developer ID needs the paid account.
- **First-build keychain prompt instead of `set-key-partition-list`** — avoids needing the login-keychain password in the committed script; the one-time **Always Allow** is a clean, well-understood step.
- **Only the built app is signed** — `tauri dev` stays ad-hoc (documented). Acceptable: permission-dependent features are tested against the built app.
- **Secrets stay local** — the key/`.p12` are gitignored; only the *creation script* is committed, so any contributor can regenerate their own "Ember Dev" cert.
- **No app code, no dependency, no migration, no Apple account** — pure build configuration + a script + docs.
- **`openssl -addext` may be unsupported on the system `openssl`** — macOS ships LibreSSL, where `req -addext` can fail. The script must add the `codeSigning` EKU via a generated OpenSSL **config file** (`-config` / `-extensions`) rather than `-addext`, OR prefer Homebrew `openssl@3` if present. The plan picks the portable config-file approach so it works on the stock toolchain.

---

## Non-goals / constraints

- **No notarization / Developer ID / distribution** — the built app is local-only and Gatekeeper-warned elsewhere by design.
- **No hardened runtime / entitlements changes** beyond what already exists.
- **No `cargo tauri dev` signing.**
- **The Rust + TypeScript test suites and `cargo clippy` must remain green** (this change should not touch them).
