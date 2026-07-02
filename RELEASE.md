# Cutting a release

How to publish an Ember build to **GitHub Releases** for other people to download.

> ⚠️ **The one rule that matters:** a public release must be a **bring-your-own-credentials
> (BYO) build** — i.e. built **without** `src-tauri/.env`. You cannot add strangers as
> Test users of *your* Google Cloud project (you don't know their emails, and the cap is
> 100), and Google's **restricted** Gmail scope has no "continue anyway" bypass. A build
> with your baked-in key would let nobody else in — and worse, it hides the credentials
> screen (`source` is `"baked"`, not `"none"`), so they'd hit a dead end. Ship BYO.

## What a downloader has to do

Set expectations honestly: because Gmail is a restricted scope, a public build is only
practical for **technical users**. Each one must, once:

1. Get past Gatekeeper (the app is unsigned — see [INSTALL.md](INSTALL.md)).
2. Create their **own** Google Cloud project, enable the Gmail + Calendar APIs, make a
   Desktop OAuth client, and add themselves as a Test user.
3. Paste their **Client ID + secret** into Ember's first-run screen.

Steps 2–3 are the app's built-in BYO flow. Non-technical mass distribution isn't possible
without Google's formal verification (domain + privacy policy + annual CASA audit).

---

## Option A — automated (recommended)

The [`release.yml`](.github/workflows/release.yml) workflow builds the BYO `.dmg` on an
Apple-Silicon runner, computes a SHA-256, and creates the Release.

1. Bump the version in **`src-tauri/tauri.conf.json`** and **`package.json`** (keep them in
   sync).
2. Commit, then tag and push:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
3. The workflow runs and a Release appears with the `.dmg` + `.dmg.sha256` attached. Edit
   the notes if you want (template below).

CI never has `src-tauri/.env` (it's gitignored), so the build is BYO automatically. It
ad-hoc-signs (`APPLE_SIGNING_IDENTITY: "-"`) because the local "Ember Dev" cert doesn't
exist on the runner.

## Option B — manual (local build)

Run everything **from the repo root**.

1. **Make sure no credentials get baked in.** Temporarily move your env file aside:
   ```bash
   mv src-tauri/.env src-tauri/.env.local.bak   # skip if you have no .env
   ```
2. Build:
   ```bash
   npm ci
   npm run tauri build
   ```
3. Checksum the `.dmg`:
   ```bash
   dmg=$(find src-tauri/target/release/bundle/dmg -name '*.dmg')
   shasum -a 256 "$dmg" | tee "$dmg.sha256"
   ```
4. Create the Release:
   ```bash
   gh release create v0.1.0 "$dmg" "$dmg.sha256" \
     --title "Ember v0.1.0" \
     --notes "Apple-Silicon macOS build (unsigned). Setup: INSTALL.md. Verify: shasum -a 256 -c Ember_*_aarch64.dmg.sha256"
   ```
5. Restore your env file:
   ```bash
   mv src-tauri/.env.local.bak src-tauri/.env
   ```

> Verify you didn't ship your key: install the built `.dmg`, and on first launch the
> **"Set up Google access"** screen should appear (not a "Connect" button). If it goes
> straight to Connect, your `.env` leaked into the build — rebuild without it.

---

## Release-notes template

```markdown
**Ember vX.Y.Z** — local-first macOS Gmail client.

Apple-Silicon (arm64) only · macOS 13 (Ventura)+ · unsigned build.

### Install
1. Download `Ember_X.Y.Z_aarch64.dmg`, open it, drag **Ember** to Applications.
2. The app is unsigned, so macOS blocks the first launch. Once:
   ```
   xattr -dr com.apple.quarantine /Applications/Ember.app
   open /Applications/Ember.app
   ```
3. On first run you'll paste your **own** Google OAuth Client ID + secret.
   Full walkthrough: INSTALL.md.

### Verify your download
```
shasum -a 256 -c Ember_X.Y.Z_aarch64.dmg.sha256
```

> Ember is a personal project, not affiliated with Google, and is not notarized by
> Apple. It talks only to Google's APIs; your mail is cached locally on your Mac.
```
