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

## Removing it later

To undo the local trust + identity (e.g. on cleanup):

```bash
# delete the trust setting + cert/key from the login keychain
sudo security delete-certificate -c "Ember Dev"   # removes the cert (and its trust)
rm -rf scripts/.signing                            # local cert material
```

Then drop `bundle.macOS.signingIdentity` from `src-tauri/tauri.conf.json` to return to unsigned
builds.
