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

Transcription runs fully on‑device (Whisper is built into Ember; the model auto‑downloads ~142 MB
on first use). Notes live in the **Calendar** view: click a meeting's event → **Notes** in the
popover → the editor has the **Record** button + device picker.

### One‑time setup

1. **Grant Microphone permission.** System Settings → Privacy & Security → **Microphone** → turn
   **Ember** on. (If it isn't listed, click **Record** once and macOS will prompt → Allow.) Without
   this, capture records pure silence and the transcript fills with "you you you".
2. **Install BlackHole 2ch** — the **Install BlackHole** button in a note (or
   https://github.com/ExistentialAudio/BlackHole). It's a virtual audio device that carries the
   call's audio to Ember. Needs your admin password (a system audio driver can't install silently).
3. **Create a Multi‑Output Device** (so you *hear* the call AND BlackHole gets a copy). Open **Audio
   MIDI Setup** → ＋ → **Create Multi‑Output Device** → tick **your speakers/headphones** + **BlackHole
   2ch**; set the speakers as primary (top) and tick **Drift Correction** on BlackHole.
4. **(Both sides) Create an Aggregate Device** — to also capture **your own voice**. ＋ → **Create
   Aggregate Device** → tick **your mic** (e.g. the built‑in or USB mic) + **BlackHole 2ch**; clock
   source = your mic, tick **Drift Correction** on BlackHole.

### Before each meeting

| What | Set to | Why |
|---|---|---|
| Mac **Output** (System Settings → Sound) | **Multi‑Output Device** | you hear the call + feed BlackHole |
| **Ember** note input | **Aggregate Device** (both sides) or **BlackHole 2ch** (other person only) | what Ember transcribes |
| **Google Meet/Zoom** microphone | **your real mic** (not the aggregate) | so the others still hear you |

Then click **Record** in the note and wait ~10 seconds for the first chunk. **Stop** when done, then
**Save** / **Summarize**.

> Notes: a Multi‑Output Device disables the keyboard volume keys (adjust volume in the app instead).
> BlackHole captures only audio your Mac *plays* (the other people) — your own voice needs the
> Aggregate Device. To test quickly, just play a YouTube video while recording.

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
| Transcript shows only "you you you" | Capture is getting silence. Grant **Microphone** permission (Step 5.1); make sure audio is actually reaching the input — output = **Multi‑Output Device**, and for a real test play a YouTube video |
| Hear nothing during a call | Don't set output to **BlackHole** alone (it's silent) — use the **Multi‑Output Device** |
| Other person transcribes but not me | That's expected with **BlackHole** as input — switch the note's input to the **Aggregate Device** to capture both sides |
