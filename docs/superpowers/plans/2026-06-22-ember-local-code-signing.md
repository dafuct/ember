# Ember — Local stable code-signing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sign the `tauri build` output with a stable local self-signed certificate so macOS TCC permissions (Microphone, Screen & System Audio Recording) persist across rebuilds instead of resetting.

**Architecture:** A committed, idempotent script generates a self-signed "Ember Dev" code-signing cert (via `openssl` + an OpenSSL config file for portability across LibreSSL/OpenSSL) and imports it into the login keychain; `tauri.conf.json` points `bundle.macOS.signingIdentity` at it; a docs runbook covers the first-build "Always Allow" prompt and verification. No app code, no dependency, no Apple account.

**Tech Stack:** `openssl` + macOS `security`/`codesign`, Tauri 2 bundler config.

**Working directory:** repo root `/Users/makar/dev/ownmail` unless noted.

**Note on verification limits:** the cert-creation script modifies the login keychain (imports the cert) — running it is the intended, idempotent, reversible action. The full `npm run tauri build` signing check requires a multi-minute release build **and an interactive one-time "Always Allow" keychain click**, so that final step is **owner-verified/manual** (a non-interactive runner can't click it). Each task verifies what it can non-interactively.

---

## File Structure

| File | Create/Modify | Responsibility |
|---|---|---|
| `scripts/create-signing-cert.sh` | **Create** | Idempotent self-signed "Ember Dev" cert creation + keychain import |
| `.gitignore` | Modify | Ignore `scripts/.signing/` (cert/key/.p12 secrets) |
| `src-tauri/tauri.conf.json` | Modify | `bundle.macOS.signingIdentity: "Ember Dev"` |
| `docs/local-code-signing.md` | **Create** | Runbook: create → build → Always-Allow → verify; dev-vs-build caveat |

---

## Task 1: The cert-creation script + gitignore

**Files:**
- Create: `scripts/create-signing-cert.sh`
- Modify: `.gitignore`

- [ ] **Step 1: Add the gitignore entry first (so secrets are never staged)**

Append to `.gitignore` (after the existing last line `.superpowers/`):
```
# Local code-signing cert material (see scripts/create-signing-cert.sh)
scripts/.signing/
```

- [ ] **Step 2: Create the script**

Create `scripts/create-signing-cert.sh`:
```bash
#!/usr/bin/env bash
# Create a stable, self-signed "Ember Dev" code-signing identity in the login keychain so the
# `tauri build` output keeps a constant code-signature designated requirement across rebuilds —
# which lets macOS TCC permissions (Microphone, Screen Recording) persist instead of resetting.
# Local developer-experience only: NOT a Developer ID, NOT notarized, NOT for distribution.
set -euo pipefail

IDENTITY="Ember Dev"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

# Idempotent: do nothing if the identity already exists.
if security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  echo "✓ Code-signing identity \"$IDENTITY\" already present. Nothing to do."
  exit 0
fi

mkdir -p "$DIR"
CNF="$DIR/openssl-codesign.cnf"
# Use an OpenSSL config file (portable across the stock LibreSSL and Homebrew OpenSSL) to set the
# codeSigning extended-key-usage — `req -addext` is not reliable on LibreSSL.
cat > "$CNF" <<'EOF'
[req]
distinguished_name = dn
x509_extensions = v3_codesign
prompt = no
[dn]
CN = Ember Dev
[v3_codesign]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = codeSigning
EOF

echo "→ Generating self-signed code-signing certificate \"$IDENTITY\" (10 years)..."
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$DIR/ember-dev-key.pem" -out "$DIR/ember-dev-cert.pem" \
  -config "$CNF"
openssl pkcs12 -export -inkey "$DIR/ember-dev-key.pem" -in "$DIR/ember-dev-cert.pem" \
  -out "$DIR/ember-dev.p12" -passout pass: -name "$IDENTITY"

echo "→ Importing into the login keychain (grants /usr/bin/codesign access)..."
security import "$DIR/ember-dev.p12" -k "$KEYCHAIN" -P "" -T /usr/bin/codesign

echo
if security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  echo "✓ \"$IDENTITY\" is now a valid code-signing identity."
  echo "  Next: npm run tauri build  — click \"Always Allow\" on the first codesign keychain prompt."
else
  echo "✗ Import finished but \"$IDENTITY\" is not listed by find-identity — see output above." >&2
  exit 1
fi
```

- [ ] **Step 3: Make it executable**

Run: `chmod +x scripts/create-signing-cert.sh`

- [ ] **Step 4: Run it to verify it creates the identity**

Run: `bash scripts/create-signing-cert.sh`
Expected: prints `✓ "Ember Dev" is now a valid code-signing identity.` (On a machine where it already exists, prints the "already present" line and exits 0.)

- [ ] **Step 5: Verify idempotency + the identity is listed**

Run: `bash scripts/create-signing-cert.sh && echo "---" && security find-identity -v -p codesigning | grep "Ember Dev"`
Expected: second run prints `✓ … already present. Nothing to do.`, then the `grep` shows a line like `1) <HASH> "Ember Dev"`.

- [ ] **Step 6: Confirm secrets are NOT tracked**

Run: `git status --porcelain scripts/.signing/ ; echo "ignored-check:" ; git check-ignore scripts/.signing/openssl-codesign.cnf`
Expected: the first command prints nothing (untracked-but-ignored shows nothing in porcelain for ignored paths), and `git check-ignore` echoes `scripts/.signing/openssl-codesign.cnf` (proving it's ignored).

- [ ] **Step 7: Commit**

```bash
git add scripts/create-signing-cert.sh .gitignore
git commit -m "feat(signing): self-signed 'Ember Dev' code-signing cert script + gitignore"
```

---

## Task 2: Point Tauri at the signing identity

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Add the macOS signing identity to the bundle block**

In `src-tauri/tauri.conf.json`, the `bundle` block currently ends:
```json
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
```
Add a `macOS` key after the `icon` array (note the comma after the `icon` array's closing `]`):
```json
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "macOS": {
      "signingIdentity": "Ember Dev"
    }
  }
```

- [ ] **Step 2: Verify the JSON is valid**

Run: `python3 -c "import json; json.load(open('src-tauri/tauri.conf.json')); print('valid JSON')"`
Expected: `valid JSON`.

- [ ] **Step 3: Verify the key parsed where expected**

Run: `python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['bundle']['macOS']['signingIdentity'])"`
Expected: `Ember Dev`.

- [ ] **Step 4: Confirm the app still builds (config didn't break the toolchain)**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: `Finished` — the debug build is unaffected (signingIdentity only applies to the bundler at `tauri build` time, not `cargo build`/`tauri dev`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "feat(signing): set bundle.macOS.signingIdentity to 'Ember Dev'"
```

---

## Task 3: The runbook

**Files:**
- Create: `docs/local-code-signing.md`

- [ ] **Step 1: Write the runbook**

Create `docs/local-code-signing.md`:
```markdown
# Local stable code-signing (developer experience)

Ember's `tauri build` output is signed with a **self-signed** certificate named **"Ember Dev"** so
that macOS TCC permissions — **Microphone** and **Screen & System Audio Recording** — persist across
rebuilds. Unsigned / ad-hoc builds get a fresh code identity every rebuild, so macOS re-prompts for
those permissions each time (the pain hit during the M24 live-capture work). A stable named identity
fixes that.

This is **local developer experience only**: it is **not** a Developer ID certificate, the app is
**not** notarized, and it is **not** for distribution to other Macs (Gatekeeper will warn elsewhere).

## One-time setup

```bash
bash scripts/create-signing-cert.sh
```

Creates the "Ember Dev" code-signing identity in your login keychain (idempotent — safe to re-run).
The cert/key files live in the git-ignored `scripts/.signing/`.

## Build

```bash
npm run tauri build
```

On the **first** signed build, macOS shows a one-time prompt:
*"codesign wants to use a key in your keychain."* Click **Always Allow** — every later build signs
silently.

## Caveat: only the BUILT app is signed

`cargo tauri dev` / `npm run tauri dev` runs the bare binary **ad-hoc** — it is NOT stably signed.
To get persistent TCC permissions, run the **built app**
(`src-tauri/target/release/bundle/macos/Ember.app`), not dev mode.

## Verify the signature is stable

```bash
codesign -dv --verbose=4 src-tauri/target/release/bundle/macos/Ember.app
```

Look for `Authority=Ember Dev`. Build twice and compare the **Designated Requirement** line — it
should be identical across builds (that constancy is what lets TCC remember the app).

## Verify TCC persistence (the payoff)

1. Launch the built `Ember.app`, exercise a mic/recording feature, grant **Microphone** when prompted.
2. `npm run tauri build` again, relaunch the app.
3. The permission should still be granted — no re-prompt.
```

- [ ] **Step 2: Commit**

```bash
git add docs/local-code-signing.md
git commit -m "docs(signing): local code-signing runbook"
```

---

## Self-Review notes (already applied)

- **Spec coverage:** cert script with config-file EKU + idempotency + keychain import (Task 1) ✓; `.gitignore` for secrets (Task 1) ✓; `tauri.conf.json` `signingIdentity` (Task 2) ✓; runbook with create→build→Always-Allow→verify + dev-vs-build caveat + not-distributable note (Task 3) ✓; LibreSSL `-addext` avoided via config file (Task 1) ✓.
- **No placeholders:** every step has exact commands/content; the openssl/security commands are concrete.
- **Verification honesty:** Task 1 runs the script (real, idempotent keychain import) and asserts the identity is listed; Task 2 validates the JSON + that `cargo build` is unaffected; the interactive `tauri build` + TCC-persistence check is explicitly owner-verified (a non-interactive runner can't click "Always Allow").
- **Consistency:** the identity string `"Ember Dev"` is identical in the script, `tauri.conf.json`, and the docs.
