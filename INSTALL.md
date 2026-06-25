# Installing Ember on another Mac

A step-by-step guide to installing Ember from the `.dmg` on a second Mac.

**Requires an Apple-Silicon Mac** (M1/M2/M3…). The build is `aarch64` — it will not run on Intel.

The one step that trips people up is **Gatekeeper** (Step 3): Ember is self-signed ("Ember Dev"),
not notarized by Apple, so macOS blocks the first launch by design. The fix is below.

---

## 1. Copy the installer over

Get `Ember_0.1.0_aarch64.dmg` onto the other Mac — **AirDrop**, a USB stick, or any file transfer.
(After a build it lives at `src-tauri/target/release/bundle/dmg/Ember_0.1.0_aarch64.dmg`.)

## 2. Install the app

1. Double-click the `.dmg`.
2. Drag **Ember.app** into the **Applications** folder.
3. Eject the disk image.

## 3. First launch — get past Gatekeeper ⚠️

Because the app isn't notarized, macOS blocks the first launch with *"Ember cannot be opened because
the developer cannot be verified"* or *"Ember is damaged."* That's expected. Do this **once**:

**Easiest — Terminal (most reliable):**

```bash
xattr -dr com.apple.quarantine /Applications/Ember.app
open /Applications/Ember.app
```

This removes the download-quarantine flag and launches it. After that it opens normally.

**Or, without Terminal:**

1. In Finder, **right-click Ember.app → Open**, then click **Open** in the dialog.
2. If macOS still refuses, go to **System Settings → Privacy & Security**, scroll down, click
   **Open Anyway** next to the Ember message, then launch again.

## 4. Connect your Google account

1. On first run, click **Connect** — your browser opens Google sign-in.
2. Sign in and approve the requested permissions (Gmail + Calendar). The window redirects back and
   Ember finishes connecting.

⚠️ **Two account caveats:**

- If a **"Google API credentials" setup screen** appears instead of a normal connect button, this
  build wasn't shipped with a baked-in key — paste a **Client ID + Secret** from your own Google
  Cloud OAuth client (the in-app screen explains how).
- Ember uses Google's **restricted Gmail scope**, so the account you sign in with must be a **test
  user** on the Google Cloud project behind the credentials. Signing in with **your own account**
  (the one that owns the project) just works. A *different* person is blocked until they're added as a
  test user — or they use their own credentials.

## 5. (Optional) Meeting transcription

- The **first time you Record or Import** in a meeting note, Ember downloads the speech model
  (~142 MB, one time) automatically — no setup.
- macOS prompts for **Microphone** permission → **Allow** (or System Settings → Privacy & Security →
  Microphone → enable Ember).
- To capture **the meeting's audio** (not just your mic), the note shows an **Install BlackHole**
  button → click it → the official installer opens → enter your password. Then in **Audio MIDI Setup**
  create a **Multi-Output Device** (speakers + BlackHole) so you still hear the call, set that as the
  call's output, and pick **BlackHole** as the input device in the note.

## 6. (Optional) Local summaries

Summaries (separate from transcription) use [Ollama](https://ollama.com): install it, run
`ollama pull llama3.2`, and the **Summarize** button in a note works. Nothing leaves your machine.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| "Ember is damaged / can't be opened" | `xattr -dr com.apple.quarantine /Applications/Ember.app` (Step 3) |
| "Developer cannot be verified" | Right-click → Open, or **Open Anyway** in Privacy & Security |
| Won't launch at all, no dialog | It's Apple-Silicon-only — confirm it's not an Intel Mac |
| Google sign-in blocked / "not a test user" | Sign in with the project owner's account, or add that account as a test user in Google Cloud |
| Recording produces nothing | Grant Microphone permission; for the *other* participants you need BlackHole + a Multi-Output Device (Step 5) |
