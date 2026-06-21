# Ember — Milestone 21: Local-Ollama meeting-note summarization (lean v1) — Design Spec

**Status:** Approved design (2026-06-21). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Summarize a meeting note's body with a **local Ollama** model into a structured **Summary + Action items**, store the result, and show a **staleness hint** when the note is edited afterward. This is the **3rd step** of the "calendar & meeting notes" feature area (M19 calendar write → M20 notes storage → **M21 Ollama summarization** → later: M22 transcription capture). M21 proves the local-AI pipeline end-to-end on data the owner already has (their typed notes); M22 will feed a transcript through the **same** `summarize` path. **No new OAuth scope, no new dependency, no Settings change** — the model + endpoint are hardcoded. Everything stays **local-only** (Ollama runs on the user's machine; notes/summaries never leave it).

**Architecture in one paragraph:** A new single-file Rust module `src-tauri/src/ollama.rs` holds an **`OllamaClient`** mirroring `GmailClient`/`CalendarClient` — a swappable `base_url` (default `http://localhost:11434`, overridable for wiremock tests) + a reusable `reqwest::Client` with a ~120s timeout. Its `summarize(notes) -> Result<String>` POSTs `/api/generate` with `{ model: "llama3.2", prompt: <structured-summary prompt embedding the notes>, stream: false }`, parses `{ response }`, and maps failures to friendly `AppError::Other` messages. The M20 `meeting_notes` table gains **`summary`** + **`summary_updated_at`** (additive — `CREATE TABLE` literal for fresh DBs + `add_column_if_missing` for existing M20 DBs; the M6 pattern, no migration framework). An orchestrating command **`summarize_meeting_note(calendar_id, event_id)`** reads the *saved* note body in a locked block, drops the lock, `await`s the Ollama call, then re-locks to persist `summary` + `summary_updated_at` via a new `db::set_meeting_note_summary` (which does **not** bump the body's `updated_at`), and returns the updated `MeetingNote`. The NotesModal gains a Summarize/Regenerate button, a read-only summary panel, and a staleness hint; summarizing first persists the body if dirty, then calls the command. An `isTauri()`-gated mock returns a canned summary so the maket demos it.

**Tech Stack:** Rust (reqwest, serde, rusqlite, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite. **No new dependency** (reqwest already present). Requires a local **Ollama** install with the `llama3.2` model pulled (an optional runtime dependency, handled gracefully when absent).

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept (here: a reqwest POST with a JSON body, `reqwest::Error::is_connect()` discrimination, the borrow-then-drop-guard-before-`.await` pattern, additive `ALTER TABLE ADD COLUMN`). After each Rust task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M20 are merged to `main`. Ember reads/classifies/mutates/sends/forwards mail, has settings + onboarding, a writable Google Calendar (M19), and **local per-event meeting notes** (M20: a `meeting_notes` SQLite table — one plain-text note per calendar event, `UNIQUE(calendar_id, event_id)` + a title/start snapshot + `created_at`/`updated_at` ms timestamps; four DB-only commands `get`/`save`-upsert/`delete`/`list`; a `NotesModal` editor opened from the event-detail popover + a Notes drawer in the calendar; a has-notes dot; `isTauri()`-gated mocks). M20 deliberately **deferred** the `summary` column to this milestone. M21 adds it plus the local-AI integration.

**Reuse map:** the `GmailClient`/`CalendarClient` shape (`base_url` + `with_base_url` for tests, a reusable `reqwest::Client`, status handling) for the new `OllamaClient`; the **M6 additive-migration pattern** (`add_column_if_missing` + `CREATE TABLE IF NOT EXISTS`, no migration framework, no cache wipe) for the two new columns; the M20 `meeting_notes` CRUD + `MeetingNote`/`row_to_note`/`NOTE_COLS` + `now_millis()` + the locked-block/drop-before-`.await` command discipline; the M20 `NotesModal` (modal pattern, load-on-open) for the summary UI; the `isTauri()` mock seam (`lib/notes.ts` + `lib/mock.ts`); the wiremock integration-test style of `tests/calendar_test.rs`.

---

## Scope

**In scope (lean v1):**
- **`OllamaClient`** (`src-tauri/src/ollama.rs`): blocking `summarize(notes) -> Result<String>` via `POST /api/generate` (`model:"llama3.2"`, `stream:false`); a pure `build_prompt`; friendly error mapping (connection-refused, model-not-found, empty).
- **`meeting_notes`** += `summary TEXT NOT NULL DEFAULT ''`, `summary_updated_at INTEGER NOT NULL DEFAULT 0` (additive); `MeetingNote` (Serialize) + `NOTE_COLS` + `row_to_note` extended; new `db::set_meeting_note_summary` (no `updated_at` bump).
- Command **`summarize_meeting_note(calendar_id, event_id) -> MeetingNote`** (reads saved body → Ollama → persists summary → returns row), registered in `lib.rs`.
- **NotesModal**: Summarize/Regenerate button, "Summarizing…" spinner, read-only summary panel (raw markdown text), staleness hint, inline error.
- **`summarizeMeetingNote`** wrapper + mock; `MeetingNote` TS interface += `summary`/`summary_updated_at`.

**Explicitly deferred (later milestones / not M21):**
- **Transcription input** (M22 — meeting transcript capture; it feeds the *same* `summarize` pipeline). ⚠️ Still the hard piece: Google Meet's transcript API is Workspace-only (a personal `@gmail.com` can't use it), Zoom needs a paid plan → local system-audio + Whisper is the local-first path; Ollama does summarization, **not** speech-to-text.
- **Streaming tokens** (chose blocking + spinner).
- **Configurable model / endpoint** (Settings dropdown from `/api/tags`, base-URL field) — model + endpoint are hardcoded in v1.
- **Markdown *rendering*** of the summary (shown as raw markdown text; rendering can come with a later rich-text pass).
- A summary marker in the browse-list rows; auto-summarize-on-save; temperature/prompt UI; summarize-across-multiple-notes; regenerate-all.

---

## Components

### Backend — new module `src-tauri/src/ollama.rs` (declared `pub mod ollama;` in `lib.rs`)
- `const DEFAULT_BASE: &str = "http://localhost:11434";` `const MODEL: &str = "llama3.2";`
- `pub struct OllamaClient { base_url: String, http: reqwest::Client }` with `new()` (builds the client with `.timeout(Duration::from_secs(120))` — local CPU generation is slow) and `with_base_url(base_url)` (tests).
- A **pure** `build_prompt(notes: &str) -> String`: a fixed preamble instructing a concise markdown summary with a `## Summary` section (2–4 bullets) and an `## Action items` section (checkbox bullets; omit if none), "be factual, do not invent content," followed by the notes. Unit-testable.
- `pub async fn summarize(&self, notes: &str) -> Result<String>`: POST `{base}/api/generate` with a private `#[derive(Serialize)] GenerateRequest<'a> { model: &'a str, prompt: String, stream: bool }`. On `send()` error: map `e.is_connect()` → `AppError::Other("Ollama isn't running at localhost:11434 — install it from ollama.com and run \`ollama serve\`.")`, else the `#[from]` `AppError::Http`. If the response status is **404** → `AppError::Other("Ollama model 'llama3.2' not found. Run: ollama pull llama3.2")`; else `error_for_status()?`. Parse a private `#[derive(Deserialize)] GenerateResponse { response: String }`, `trim()`, and error (`AppError::Other`) if empty.

### Backend — `db/mod.rs`
- `meeting_notes` `CREATE TABLE` literal gains `summary TEXT NOT NULL DEFAULT ''` and `summary_updated_at INTEGER NOT NULL DEFAULT 0` (fresh DBs); plus two unconditional `add_column_if_missing(conn, "meeting_notes", "summary", "TEXT NOT NULL DEFAULT ''")?` / `add_column_if_missing(conn, "meeting_notes", "summary_updated_at", "INTEGER NOT NULL DEFAULT 0")?` near the existing migration block (existing M20 DBs). No cache wipe.
- `MeetingNote` (Serialize) += `summary: String`, `summary_updated_at: i64`; `NOTE_COLS` + `row_to_note` extended (indices 8, 9).
- `upsert_meeting_note` SQL is **unchanged** — it omits `summary`/`summary_updated_at`, so a fresh insert defaults them (`''`/`0`) and a body re-save **preserves** an existing summary (they are not in the `ON CONFLICT DO UPDATE SET`). This is the "saving the body never touches the summary" contract.
- New `set_meeting_note_summary(conn, calendar_id: &str, event_id: &str, summary: &str, now_ms: i64) -> Result<MeetingNote>`: `UPDATE meeting_notes SET summary=?1, summary_updated_at=?2 WHERE calendar_id=?3 AND event_id=?4` (**does not touch `updated_at`**), then re-read via `get_meeting_note` and return; `None` → `AppError::Other("note not found")`.

### Backend — command (`src-tauri/src/commands.rs`, registered in `lib.rs`)
- `summarize_meeting_note(calendar_id: String, event_id: String, state: tauri::State<'_, Db>) -> Result<db::MeetingNote>`: read the body in a locked block (`get_meeting_note` → `.body`; `None` → `AppError::Other("Save the note before summarizing.")`); guard empty/whitespace body (`AppError::Other("Nothing to summarize — the note is empty.")`); **drop the lock**; `OllamaClient::new().summarize(&body).await?`; **re-lock**; `db::set_meeting_note_summary(&conn, &calendar_id, &event_id, &summary, now_millis())`. Honors the std-MutexGuard-not-across-`.await` rule. Reuses M20 `now_millis()`.

### Frontend — `src/lib/notes.ts`
- `MeetingNote` interface += `summary: string; summary_updated_at: number`.
- `summarizeMeetingNote(calendarId, eventId): Promise<MeetingNote>` → `isTauri() ? invoke("summarize_meeting_note", { calendarId, eventId }) : mockSummarizeMeetingNote(calendarId, eventId)`.

### Frontend — `src/lib/mock.ts`
- Seeded notes + `mockSaveMeetingNote` gain `summary: ""`, `summary_updated_at: 0`. One seeded note carries a pre-filled summary with `summary_updated_at < updated_at` so a **stale** example renders. `mockSummarizeMeetingNote(cal, ev)` sets a canned structured summary on the stored mock note with `summary_updated_at` ≥ its `updated_at` (fresh) and returns the updated note.

### Frontend — `src/components/NotesModal.tsx`
- On open (`getMeetingNote`), load `summary`, `summary_updated_at`, the note's `updated_at`, and track `savedBody`.
- Summary panel below the textarea: when `summary` is non-empty, render it read-only (a `<pre>`/`div`, raw markdown text). A staleness hint ("Notes changed since this summary — Regenerate") when `summary && noteUpdatedAt > summaryUpdatedAt`.
- A Summarize/Regenerate button (label = `summary ? "Regenerate" : "Summarize"`; disabled when body empty/whitespace or while summarizing). On click: (1) `setSummarizing(true)`, clear error; (2) if `body !== savedBody`, `await saveMeetingNote({…body…})` + update `savedBody`; (3) `await summarizeMeetingNote(calId, eventId)`; (4) set `summary`/`summaryUpdatedAt`/`noteUpdatedAt`/`exists` from the returned row; (5) on error, show inline. "Summarizing…" spinner while running.
- M20 behavior unchanged (Save persists body; Delete/Cancel/Esc).

### Frontend — `src/styles/app.css`
- `.note-summary` (read-only panel, muted bg, whitespace-preserving), `.note-summary-stale` (subtle warning-toned hint). Reuse existing button classes.

### Data flow
`Open note → getMeetingNote (body + summary + timestamps)`. `Summarize → (save body if dirty) → summarize_meeting_note → reads saved body → OllamaClient /api/generate → set_meeting_note_summary → returns row → panel shows summary, hint cleared`. `Edit body + Save → updated_at advances → reopen shows the stale hint`.

---

## Error handling

- **Ollama not running** (connection refused) → `AppError::Other("Ollama isn't running…")` → inline modal error. **Model not pulled** (HTTP 404) → `AppError::Other("…run: ollama pull llama3.2")`. **Empty response** → `AppError::Other`. No reconnect path (Ollama isn't Google); no crash — the feature degrades to a clear, actionable message.
- **Note not saved / empty body** → guarded in the command with a clear message; the button is also disabled on an empty body.
- A long generation is bounded by the 120s client timeout (a hung Ollama surfaces a timeout error rather than hanging forever).

---

## Testing

- **Rust — `tests/ollama_test.rs` (new wiremock integration crate, mirrors `tests/calendar_test.rs`):** happy path — assert the captured POST body carries `model:"llama3.2"`/`stream:false` and a prompt that **contains the notes + the `## Summary`/`## Action items` instructions** (this covers `build_prompt` indirectly, so it can stay a private helper), and `{response}` → trimmed summary; **404 → friendly "ollama pull" message**; **connection-refused** (`with_base_url("http://127.0.0.1:1")` → assert the "Ollama isn't running" message).
- **Rust — `db/mod.rs` inline tests:** `set_meeting_note_summary` sets `summary` + `summary_updated_at` and **leaves `updated_at` unchanged**; a body re-save via `upsert_meeting_note` **preserves** an existing summary; `init` is idempotent with the new columns and **migrates an M20-shaped table** (create a `meeting_notes` table without the two columns, run `init`, confirm the columns are added and notes still work).
- **Frontend:** no TS harness (consistent through M20). Maket-verified by screenshot: the summary panel with a canned summary + the staleness hint on the seeded edited-since note.
- **Gates:** `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean. Genuinely testable locally with Ollama running (`ollama pull llama3.2`); otherwise the friendly error path is exercised. (A real-generation E2E with the owner's Ollama is owner-pending, like the Google E2E paths.)

---

## Known risks & decisions

- **Hardcoded model `llama3.2` + endpoint `localhost:11434`** (owner's choice over a Settings field). Brittle if the model isn't pulled — mitigated by the explicit 404 → "run `ollama pull llama3.2`" message. The model name is a single `const` (trivial to change); a Settings dropdown from `/api/tags` is the deferred upgrade.
- **Blocking generation** (`stream:false`) with a spinner — summaries are short, so streaming's benefit is marginal; the 120s timeout bounds a hung server.
- **Structured summary stored in one `summary` field** (markdown with `## Summary` / `## Action items`) — the structure lives in the prompt + the model's output, not in extra columns. Displayed as raw markdown text (rendering deferred).
- **Staleness via two timestamps** — `set_meeting_note_summary` sets `summary_updated_at` without bumping `updated_at`; saving the body bumps `updated_at` without touching `summary_updated_at`; `stale = summary != "" && updated_at > summary_updated_at`. Robust because both timestamps are written server-side in dedicated paths.
- **Local-only, optional dependency** — Ollama runs on the user's machine; its absence is a graceful, actionable message, never a crash. No data leaves the device.
- **No migration framework / no cache concerns** — two additive `ADD COLUMN`s with safe defaults; idempotent `init`; an M20-shaped DB upgrades transparently.

---

## Non-goals / constraints

- **No new OAuth scope, no new dependency, no Settings change.**
- **No transcription in M21** — M22 feeds a transcript through the same `summarize`.
- **No `ALTER` beyond two additive `ADD COLUMN`s; no data migration.**
- **Tauri build unchanged for the maket** — `summarizeMeetingNote` is `isTauri()`-gated; the panel/hint are frontend over mock data.
