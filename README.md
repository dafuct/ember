# Ember

A **local-first macOS Gmail client** built with Tauri 2 (Rust) and React + TypeScript. Your mail is fetched from Gmail and cached **on your Mac** in a local SQLite database; OAuth tokens live in the macOS Keychain. Nothing is sent to any third‑party server — the app talks only to Google's APIs.

> Personal project. Ember is not affiliated with Google.

## Features

- **Multiple Google accounts** — add several accounts and switch the active one from the avatar menu; Gmail, Calendar, and notes all follow the active account. New mail for *all* connected accounts is polled in the background with native notifications.
- **Smart inbox** — messages are classified into *People / Notifications / Newsletters* streams.
- **Full mail workflow** — read, compose, reply / reply‑all / forward, drafts, attachments (send & receive), labels, and folder views (Sent, Starred, Archive, Trash, Spam, Drafts).
- **Batch actions + undo**, **snooze** (archive now, resurface later), and **server‑side search**.
- **Calendar** — week view plus create / edit / delete events (with optional Google Meet links).
- **Meeting notes** — local, per‑event notes; optional local‑LLM summarization via [Ollama](https://ollama.com) and transcript import.
- **Local‑first & private** — mail cache in SQLite, tokens in the Keychain, no external backend.

## Tech stack

- **Backend:** Rust, Tauri 2, `rusqlite` (SQLite), `keyring` (macOS Keychain), `oauth2` (PKCE + loopback).
- **Frontend:** React 19, TypeScript, Vite.

## Prerequisites

- **macOS** (Apple Silicon or Intel)
- **Node.js** 18+ and npm
- **Rust** toolchain — install via [rustup](https://rustup.rs)
- **Xcode Command Line Tools** — `xcode-select --install`
- A **Google Cloud OAuth client** (see below)

## Google OAuth setup (one‑time)

Ember needs your own Google OAuth credentials.

1. In the [Google Cloud Console](https://console.cloud.google.com/), create a project.
2. **APIs & Services → Enable APIs** → enable **Gmail API** and **Google Calendar API**.
3. **APIs & Services → Credentials → Create credentials → OAuth client ID** → application type **Desktop app**. Copy the **Client ID** and **Client secret**.
4. **OAuth consent screen / Audience** → add the Google account(s) you'll sign in with as **Test users**.
   - ⚠️ Ember requests the full Gmail scope (`https://mail.google.com/`), which Google classifies as **restricted**. For an unverified app, restricted scopes only work for accounts on the **Test users** list — there's no "continue anyway" bypass. Adding a test user is a one‑time step per account.
   - In *Testing* publishing status, refresh tokens expire after ~7 days (you'll re‑sign‑in periodically). Removing that entirely requires Google's formal app verification.
5. Create your env file:
   ```bash
   cp src-tauri/.env.example src-tauri/.env
   ```
   Then edit `src-tauri/.env` and paste your values:
   ```
   EMBER_GOOGLE_CLIENT_ID=your-client-id.apps.googleusercontent.com
   EMBER_GOOGLE_CLIENT_SECRET=your-client-secret
   ```
   `.env` is gitignored and never committed.

## Run in development

```bash
npm install
npm run tauri dev
```

This compiles the Rust backend (first run takes a few minutes), starts Vite, and opens the native window. The credentials are read from `src-tauri/.env` at runtime.

> There's also a browser‑only "maket" (`npm run dev`) that renders the UI with mock data for quick frontend work — it can't do real Google sign‑in.

## Build a distributable app

```bash
npm run tauri build
```

Produces a `.app` and `.dmg` under `src-tauri/target/release/bundle/`. The build embeds the credentials from `src-tauri/.env` into the binary (via `build.rs`), so the bundle is self‑contained and runs on another Mac without the source tree.

- **Apple Silicon → Intel (or both):** build a universal binary with
  `rustup target add x86_64-apple-darwin && npm run tauri build -- --target universal-apple-darwin`.

### Distributing to others (bring-your-own credentials)

Ember can be shared with other people, each using their **own** Google Cloud project:

- Build the `.dmg` **without** baking your credentials — just don't ship `src-tauri/.env`
  (an absent/empty `.env` means nothing is baked in).
- On first launch, each user is asked to paste **their own** Client ID + secret (stored in
  their Mac's Keychain). They follow the same Google OAuth setup as above and add
  themselves as a Test user of their own project.
- Credentials can be updated or cleared anytime in **Settings → Google API**.

(Your own personal build that includes `src-tauri/.env` keeps working with no entry — the
baked credentials are used automatically.)

## Install on another Mac

1. Copy the `.dmg` (e.g. `Ember_0.1.0_aarch64.dmg`) to the other Mac and drag **Ember** into **Applications**.
2. The app is **unsigned / not notarized**, so Gatekeeper blocks the first launch. Either:
   - **System Settings → Privacy & Security → "Open Anyway"**, or
   - `xattr -dr com.apple.quarantine /Applications/Ember.app` in Terminal, then open it.
3. On first sign‑in, allow Ember to use the login Keychain (it stores your OAuth tokens there).
4. The account you sign in with must be a **Test user** of your OAuth app (see above).

For a clean, no‑warning install you'd need an Apple Developer ID certificate + notarization (a paid Apple Developer account) — optional and only worth it for wider distribution.

## Using multiple accounts

- Click the **avatar** at the bottom of the icon rail → the account popover lists your accounts (active marked, with unread counts).
- **Add account** → runs Google sign‑in; the new account becomes active.
- Click any account to **switch** — the inbox, folders, and calendar follow it.
- **Manage in Settings** → remove accounts individually.

## Meeting transcription (zero‑setup)

Transcription runs **in‑process** — Whisper is compiled into Ember (no separate server, no manual install). The first time you **Record** in a meeting note (or **Import** a recording), Ember downloads the speech model (`base.en`, ~142 MB, one time) to its app‑data folder and loads it; after that it's instant and fully offline.

- To capture **the meeting's** audio (not just your mic), install [BlackHole](https://github.com/ExistentialAudio/BlackHole#installation), route the call's output to it (an Audio‑MIDI Multi‑Output Device lets you still hear it), and pick **BlackHole** as the input device in the note. Ember shows this hint when BlackHole isn't detected.
- Grant Ember **Microphone** permission (System Settings → Privacy & Security → Microphone).

## Optional: local meeting‑note summaries

*Summaries* (distinct from transcription) run locally via Ollama:

```bash
# install Ollama from https://ollama.com, then:
ollama pull llama3.2
```

With Ollama running, the **Summarize** button in a meeting note produces a summary from the note + transcript. No data leaves your machine.

## Known limitations

- **Shared‑event notes:** meeting notes are keyed by `(calendar_id, event_id)` without the account. If two connected accounts are both on the *same shared* calendar event and both take local notes, the second save overwrites the first (local data loss only — never a cross‑account leak).
- Live, real‑OAuth multi‑account flows are validated manually; the automated tests cover the Rust backend and the browser maket.
