# Ember — Milestone 22: Transcript on a note (import/paste → summarize) — Design Spec

**Status:** Approved design (2026-06-21). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Make a meeting's **transcript** a first-class field on its note — filled by **pasting** or **importing a `.txt`/`.vtt`** caption export (the `.vtt` timestamps stripped to plain text in Rust) — and feed it (combined with the user's freeform notes) into the existing M21 local-Ollama summarize pipeline. This is the **first of three sub-milestones** the "meeting transcription capture" step was decomposed into (M22 transcript import → **M23** local Whisper STT of an audio/video file → **M24** live macOS meeting-audio capture). M22 ships the transcript→summary spine with **no new dependency, no new OAuth scope, no OS permission** — and is the hedge that keeps the feature useful even if M24's live capture proves infeasible.

**Why decomposed:** "transcription capture" bundles three increasingly-risky subsystems — (1) transcript→summary (≈90% built by M21), (2) local Whisper STT (a `whisper.cpp` sidecar + model), (3) live macOS system-audio capture (ScreenCaptureKit / BlackHole — high effort *and* user-friction, the genuinely uncertain piece). Stacking them in one milestone risks shipping nothing. M22 is the cheapest valuable slice and proves the spine; M23 de-risks Whisper without capture; M24 (the risky capstone) comes last, by which point M22+M23 already deliver value.

**Architecture in one paragraph:** `meeting_notes` gains **`transcript TEXT NOT NULL DEFAULT ''`** (additive — `CREATE TABLE` literal for fresh DBs + `add_column_if_missing` for existing ones; the M6 pattern, **no migration framework**). `transcript` joins `body` in `MeetingNoteWrite` + `upsert_meeting_note` (insert + `ON CONFLICT DO UPDATE SET`), so the existing **Save** persists body **and** transcript and bumps `updated_at` — correctly marking the M21 summary stale when a transcript is edited/imported. A new **pure** module `src-tauri/src/transcript.rs` holds `vtt_to_text(raw)` (WebVTT → plain spoken text) and `build_summary_input(body, transcript)` (labeled combined input; fallback to whichever is non-empty). A DB-free command `read_transcript_file(path)` reads a user-picked file (`std::fs`) and parses `.vtt`. The M21 `summarize_meeting_note` changes to summarize `build_summary_input(body, transcript)` instead of just the body. The frontend `NotesModal` gains a Transcript section (textarea + **Import…** button using the M17 `tauri-plugin-dialog` `open()`). Local-only throughout.

**Tech Stack:** Rust (rusqlite, serde, Tauri 2, `tauri-plugin-dialog` — all already present), React 19 + TypeScript + Vite. **No new dependency, no new OAuth scope.**

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept (here: a line-by-line string parser, `ON CONFLICT DO UPDATE` with an added column, `std::fs::read_to_string` + error mapping, pure functions for unit-testing). After each Rust task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M21 are merged to `main`. Ember has local meeting notes (M20: a `meeting_notes` table — one note per calendar event, `UNIQUE(calendar_id, event_id)` + a title/start snapshot + `created_at`/`updated_at`; `NotesModal` editor; a Notes drawer + has-notes dot) and **local-Ollama summarization** (M21: a `summary`/`summary_updated_at` pair, an `OllamaClient` that summarizes via `POST /api/generate`, a `summarize_meeting_note` command that reads the saved body → Ollama → persists the summary without bumping `updated_at`, and a NotesModal Summarize/Regenerate button + summary panel + staleness hint). Both M20 and M21 explicitly pointed at a future `transcript` field; M22 delivers it.

**Reuse map:** the M20/M21 `meeting_notes` CRUD + `MeetingNote`/`MeetingNoteWrite`/`NOTE_COLS`/`row_to_note`/`upsert_meeting_note`; the M6 additive-migration pattern (`add_column_if_missing` + `CREATE TABLE IF NOT EXISTS`, no migration framework); the M21 `summarize_meeting_note` command (locked-block → drop guard → Ollama `await` → re-lock to persist) — M22 only changes what it feeds Ollama; the M17 file pattern (frontend `tauri-plugin-dialog` `open()` picks the path, a Rust `std::fs` command does the byte read — **no `fs` capability**); the M21 NotesModal (modal pattern, load-on-open, busy/summarizing interlocks, the `savedBody`-dirty check); the `isTauri()` mock seam (`lib/notes.ts` + `lib/mock.ts`); the pure-module + inline-test style of `scorer.rs`/`mime.rs`.

---

## Scope

**In scope (lean v1):**
- `meeting_notes` += `transcript TEXT NOT NULL DEFAULT ''` (additive); `MeetingNote`/`MeetingNoteWrite`/`NOTE_COLS`/`row_to_note`/`upsert_meeting_note` carry it (body + transcript save together).
- Pure `src-tauri/src/transcript.rs`: `vtt_to_text` (WebVTT → plain text) + `build_summary_input` (combined/fallback).
- Command `read_transcript_file(path) -> Result<String>` (`.txt` passthrough, `.vtt` parsed; DB-free, `std::fs`).
- `summarize_meeting_note` summarizes `build_summary_input(body, transcript)`; enabled when body **or** transcript is non-empty.
- NotesModal: a Transcript section (textarea + Import button); `readTranscriptFile` wrapper + mock; `MeetingNote` TS + mock seeds/save carry `transcript`.

**Explicitly deferred (later sub-milestones / not M22):**
- **M23** — local Whisper STT of an audio/video file (a `whisper.cpp` Tauri sidecar + a model; writes this **same** `transcript` field). **M24** — live macOS meeting-audio capture (ScreenCaptureKit or a BlackHole virtual device → Whisper). ⚠️ M24 is the hard/uncertain piece (Meet transcript API is Workspace-only, Zoom needs paid; local audio + Whisper is the path).
- `.srt` / other caption formats (`.txt` + `.vtt` only in v1).
- Speaker-label / timestamp display; transcript search; rich formatting (plain text only).
- A separate "summarize transcript-only" toggle (combined is the rule); auto-summarize-on-import.

---

## Components

### Backend — `db/mod.rs`
- `meeting_notes` `CREATE TABLE` literal gains `transcript TEXT NOT NULL DEFAULT ''` (after `summary_updated_at`, before `UNIQUE`); plus `add_column_if_missing(conn, "meeting_notes", "transcript", "TEXT NOT NULL DEFAULT ''")?` beside the M21 summary-column migration (existing DBs). No cache wipe.
- `MeetingNote` (Serialize) += `transcript: String`; `MeetingNoteWrite` (Deserialize) += `transcript: String`; `NOTE_COLS` appends `, transcript`; `row_to_note` adds `transcript: row.get(10)?` (column index 10).
- `upsert_meeting_note` carries `transcript` (timestamps move to `?7`, used twice for created/updated on insert):
  ```sql
  INSERT INTO meeting_notes
      (calendar_id, event_id, event_title, event_start, body, transcript, created_at, updated_at)
   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
   ON CONFLICT(calendar_id, event_id) DO UPDATE SET
      event_title = excluded.event_title, event_start = excluded.event_start,
      body = excluded.body, transcript = excluded.transcript, updated_at = excluded.updated_at
  ```
  `summary`/`summary_updated_at` stay **out** of the upsert (preserved on save, exactly as M21). The `#[cfg(test)]` `note_write` helper gains `transcript: "".into()` so the M20/M21 tests still compile (they don't assert it).

### Backend — new pure module `src-tauri/src/transcript.rs` (declared `pub mod transcript;` in `lib.rs`)
- `pub fn vtt_to_text(raw: &str) -> String`: iterate `raw.lines()`, trim each; **skip** the `WEBVTT` header, lines starting `NOTE`/`STYLE`/`REGION`/`Kind:`/`Language:`, `-->` timestamp lines, numeric-only cue ids, and blank lines; **strip** inline `<…>` tags (a small depth-counting scan, so `<v Speaker>` / `<00:00:00.000>` vanish); **collapse** consecutive duplicate lines (rolling captions repeat); return `join("\n")`. Plain `.txt` (no cue structure) effectively passes through.
- `pub fn build_summary_input(body: &str, transcript: &str) -> String`: trim both; both non-empty → `"Meeting notes:\n{body}\n\nTranscript:\n{transcript}"`; only one non-empty → that one; both empty → `""`.
- Inline `#[cfg(test)]` tests for both.

### Backend — commands (`src-tauri/src/commands.rs`, registered in `lib.rs`)
- New `read_transcript_file(path: String) -> Result<String>` (DB-free): `std::fs::read_to_string(&path)` (io err → `AppError::Other("could not read transcript file: …")`); if the lowercased path ends with `.vtt` → `crate::transcript::vtt_to_text(&raw)`, else passthrough; return `.trim().to_string()`. The byte read is Rust `std::fs` → **no `fs` capability** (M17 pattern); the path comes from the frontend `tauri-plugin-dialog` `open()`.
- `summarize_meeting_note` (M21) changes its input: in the locked block, fetch the full note (`get_meeting_note` → `AppError::Other("Save the note before summarizing.")` if absent) and compute `crate::transcript::build_summary_input(&note.body, &note.transcript)`; drop the lock; empty-guard message becomes `"Nothing to summarize — add notes or a transcript first."`; `OllamaClient::new().summarize(&input).await?`; re-lock; `set_meeting_note_summary(…, now_millis())`. Lock-drop-before-`.await` discipline and persistence unchanged.

### Frontend — `src/lib/notes.ts`
- `MeetingNote` interface += `transcript: string`; `MeetingNoteWrite` interface += `transcript: string`.
- `readTranscriptFile(path: string): Promise<string>` → `isTauri() ? invoke("read_transcript_file", { path }) : mockReadTranscriptFile(path)`.

### Frontend — `src/lib/mock.ts`
- Seeded notes gain `transcript` (e2 a short demo transcript; e6 `""`); `mockSaveMeetingNote` carries `transcript` from the write. New `mockReadTranscriptFile(path)` returns a canned multi-line transcript (simulating a parsed `.vtt`) so Import demos offline. `mockSummarizeMeetingNote` unchanged.

### Frontend — `src/components/NotesModal.tsx`
- Imports add `open` from `@tauri-apps/plugin-dialog`, `isTauri` from `@tauri-apps/api/core`, and `readTranscriptFile`.
- State += `transcript`, `savedTranscript` (both loaded from `n?.transcript ?? ""`), `importing`.
- A **Transcript section** between the body textarea and the summary panel: a head row (`Transcript` label + an **Import…** button) + a `<textarea>` (rows ~6, editable).
- `handleImport`: in Tauri, `open({ filters:[{ name:"Transcript", extensions:["txt","vtt"] }] })` → if a (non-array) path was picked, `readTranscriptFile(path)` → `setTranscript(text)`; in the maket (`!isTauri()`), skip the dialog and call `readTranscriptFile("/mock/transcript.vtt")`; errors → inline; `importing` spinner; the button is disabled while busy/summarizing/importing.
- `handleSave` sends `transcript` too. `hasContent = body.trim() !== "" || transcript.trim() !== ""` → **Save and Summarize enabled when body OR transcript is non-empty** (and disabled while importing).
- `handleSummarize`: persist first if `body !== savedBody || transcript !== savedTranscript` (update both `saved*`), then `summarizeMeetingNote` (which now summarizes the combined input server-side). Staleness/summary/Regenerate behavior unchanged.

### Frontend — `src/styles/app.css`
- A `.note-transcript-head` (flex row: label + Import button), reusing `compose-body` for the textarea and the existing button classes.

### Data flow
`Open note → load body + transcript + summary + timestamps`. `Import → open() dialog → read_transcript_file (.vtt stripped) → fill transcript textarea (editable) → Save persists body+transcript (updated_at bumps)`. `Summarize → save if dirty → summarize_meeting_note → build_summary_input(body, transcript) → Ollama → summary; a later body/transcript edit + Save → updated_at advances → stale hint`.

---

## Error handling

- **File read failure** (missing/unreadable path) → `AppError::Other("could not read transcript file: …")` → inline modal error.
- **`open()` cancelled** (no path / array) → no-op (no error).
- **Empty content** → Summarize guarded in the command (`"Nothing to summarize — add notes or a transcript first."`) and the button is disabled when both body and transcript are empty.
- **Ollama errors** (not running / model missing) → the M21 friendly messages, surfaced inline (unchanged).
- A malformed `.vtt` degrades gracefully: `vtt_to_text` keeps whatever text lines it finds (worst case the raw text, which is still summarizable) — never errors.

---

## Testing

- **Rust — `transcript.rs` inline tests:** `vtt_to_text` on a realistic sample (a `WEBVTT` header + a `NOTE` + numbered cues + `-->` timestamp lines + a `<v Speaker>` inline tag + a duplicated rolling line) → asserts the output is the dedup'd plain spoken text with the structure stripped; a plain-text input passes through unchanged; `build_summary_input` all four cases (both → labeled, body-only, transcript-only, both-empty → `""`).
- **Rust — `db/mod.rs` tests:** `upsert_meeting_note` round-trips `transcript`; a body+transcript re-save **preserves** an existing `summary`/`summary_updated_at` (still out of the upsert) while bumping `updated_at` and updating both texts; `init` adds the `transcript` column to a pre-M22-shaped `meeting_notes` table (additive migration) + idempotent.
- **Frontend:** no TS test harness (consistent through M21). Maket-verified by screenshot: the Transcript section with **Import** filling the textarea with the canned transcript, and **Summarize** producing a summary (combined input).
- **Gates:** `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean. Genuinely testable locally (paste/import a real `.txt`/`.vtt` → summarize with Ollama running); the live-Ollama leg stays owner-pending as in M21.

---

## Known risks & decisions

- **Transcript is a first-class column, not folded into `body`** — keeps the user's own notes separate from the meeting transcript, and gives M23/M24's Whisper output a clean field to write. The deferred column the M20/M21 specs always pointed at.
- **Transcript saves with the body (in `upsert_meeting_note`), not via a separate setter** — it's user-editable text like the body, so it belongs in the normal Save; this also makes a transcript edit bump `updated_at`, which correctly marks the summary stale (the same staleness contract M21 built). The summary keeps its dedicated no-bump setter.
- **Summarize feeds combined `body` + `transcript`** (labeled), falling back to whichever is non-empty — richest result, minimal extra prompt logic; the only change to the M21 command is the input it builds.
- **`.vtt` stripped to plain text in Rust** via a small pure `vtt_to_text` — `.txt` passes through; `.srt`/other formats deferred. Malformed input degrades to "keep the text," never errors.
- **File read is a Rust `std::fs` command, path from the frontend dialog** — no `fs` capability needed (the M17 pattern); `tauri-plugin-dialog` is already a dependency.
- **No new dependency, no new OAuth scope, no migration framework** — one additive `ADD COLUMN` with a safe default; idempotent `init`.
- **Decomposition is the headline risk-management decision** — M22 deliberately defers the hard live-capture (M24) and the Whisper integration (M23), shipping the immediately-useful transcript→summary spine first.

---

## Non-goals / constraints

- **No audio capture and no speech-to-text in M22** — those are M23 (Whisper file STT) and M24 (live capture); M22 only handles a transcript that already exists as text.
- **No new dependency, no new OAuth scope, no `ALTER` beyond one additive `ADD COLUMN`.**
- **Plain-text transcript** — no speaker/timestamp UI, no rich formatting.
- **Tauri build unchanged for the maket** — `readTranscriptFile` is `isTauri()`-gated; the Import button uses a mock path offline; the transcript section is frontend over mock data.
