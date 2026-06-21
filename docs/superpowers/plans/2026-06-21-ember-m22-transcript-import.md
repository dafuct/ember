# Ember — M22 Transcript on a Note (import/paste → summarize) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a meeting's transcript a first-class field on its note (paste or import a `.txt`/`.vtt`), saved alongside the body, and feed it (combined with the freeform notes) into the existing M21 Ollama summarize pipeline.

**Architecture:** `meeting_notes` gains a `transcript` column (additive, M6 pattern), carried in `MeetingNoteWrite` + `upsert_meeting_note` so the existing Save persists body + transcript and bumps `updated_at` (marking the summary stale). A new pure `transcript.rs` holds `vtt_to_text` (WebVTT→plain text) + `build_summary_input` (combined notes+transcript, fallback). A DB-free `read_transcript_file` command reads a picked file (`std::fs`, `.vtt` parsed). `summarize_meeting_note` summarizes the combined input. NotesModal gains a Transcript section (textarea + Import button via the M17 dialog).

**Tech Stack:** Rust (rusqlite, serde, Tauri 2, `tauri-plugin-dialog` — all present), React 19 + TypeScript + Vite. No new dependency, no new OAuth scope.

**Learning mode (IMPORTANT):** every Rust block below carries `// 🦀` teaching comments — keep them verbatim. After each Rust task give a 2–3 sentence plain-English recap. TS/React gets normal comments. **Do NOT run `cargo fmt`** (hand-formatted repo). Commit messages use `feat(m22:)`/`test(m22:)` style and end with the `Co-Authored-By` trailer shown in each commit step.

**Reference (read before starting):** spec at `docs/superpowers/specs/2026-06-21-ember-m22-transcript-import-design.md`. Patterns mirrored: `db/mod.rs` (M20/M21 `meeting_notes` CRUD, `add_column_if_missing`, the `#[cfg(test)]` `conn()`/`note_write` helpers), `commands.rs` (M21 `summarize_meeting_note` lock-drop discipline; the M17 `download_attachment` `std::fs` command), `lib.rs` (`pub mod` + `invoke_handler`), `scorer.rs`/`mime.rs` (pure module + inline tests), `ComposeModal.tsx` (M17 `open()` dialog), `NotesModal.tsx`/`lib/notes.ts`/`lib/mock.ts` (M21 frontend).

---

## File structure

**Backend**
- `src-tauri/src/db/mod.rs` — *modify*: `transcript` column + `MeetingNote`/`MeetingNoteWrite`/`NOTE_COLS`/`row_to_note`/`upsert_meeting_note` + `note_write` helper + tests.
- `src-tauri/src/transcript.rs` — *create*: pure `vtt_to_text` + `build_summary_input` + inline tests.
- `src-tauri/src/lib.rs` — *modify*: `pub mod transcript;` + register `read_transcript_file`.
- `src-tauri/src/commands.rs` — *modify*: add `read_transcript_file`; change `summarize_meeting_note`'s input.

**Frontend**
- `src/lib/notes.ts` — *modify*: `transcript` on both interfaces + `readTranscriptFile` wrapper.
- `src/lib/mock.ts` — *modify*: seeds + save carry `transcript`; `mockReadTranscriptFile`.
- `src/components/NotesModal.tsx` — *modify*: Transcript section (textarea + Import) + save/summarize carry transcript.
- `src/styles/app.css` — *modify*: `.note-transcript-head`.

---

## Task 1: DB — `transcript` column + upsert (Rust, TDD)

**Files:** Modify `src-tauri/src/db/mod.rs`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/db/mod.rs`, inside `mod tests` (after the last M21 test `init_adds_summary_columns_to_an_m20_shaped_table`, before the closing `}`), add:

```rust
    #[test]
    fn upsert_meeting_note_round_trips_transcript() {
        let c = conn();
        let mut w = note_write("primary", "e1", "body");
        w.transcript = "line one\nline two".into();
        let n = upsert_meeting_note(&c, &w, 1000).unwrap();
        assert_eq!(n.transcript, "line one\nline two");
        let got = get_meeting_note(&c, "primary", "e1").unwrap().unwrap();
        assert_eq!(got.transcript, "line one\nline two"); // persisted
    }

    #[test]
    fn body_transcript_resave_preserves_summary() {
        let c = conn();
        let mut w = note_write("primary", "e1", "body1");
        w.transcript = "t1".into();
        upsert_meeting_note(&c, &w, 1000).unwrap();
        set_meeting_note_summary(&c, "primary", "e1", "sum", 1200).unwrap();
        // edit body + transcript, re-save later
        let mut w2 = note_write("primary", "e1", "body2");
        w2.transcript = "t2".into();
        let after = upsert_meeting_note(&c, &w2, 3000).unwrap();
        assert_eq!(after.body, "body2");
        assert_eq!(after.transcript, "t2");
        assert_eq!(after.updated_at, 3000); // bumped by the body/transcript save
        assert_eq!(after.summary, "sum"); // summary preserved (stays out of the upsert)
        assert_eq!(after.summary_updated_at, 1200);
    }

    #[test]
    fn init_adds_transcript_column_to_a_pre_m22_table() {
        // 🦀 An M21-shaped table (has the summary columns, but NO transcript) + a row.
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE meeting_notes (
                id INTEGER PRIMARY KEY, calendar_id TEXT NOT NULL, event_id TEXT NOT NULL,
                event_title TEXT NOT NULL DEFAULT '', event_start TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                summary TEXT NOT NULL DEFAULT '', summary_updated_at INTEGER NOT NULL DEFAULT 0,
                UNIQUE(calendar_id, event_id));
             INSERT INTO meeting_notes
                (calendar_id, event_id, event_title, event_start, body, created_at, updated_at)
                VALUES ('primary','e1','T','2026-01-01','b',1,1);",
        )
        .unwrap();
        init(&c).unwrap();
        let n = get_meeting_note(&c, "primary", "e1").unwrap().unwrap();
        assert_eq!(n.transcript, ""); // backfilled default
        assert_eq!(n.body, "b");
        init(&c).unwrap(); // idempotent
        assert!(get_meeting_note(&c, "primary", "e1").unwrap().is_some());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib db::tests 2>&1 | tail -20`
Expected: FAIL — compile errors (`MeetingNoteWrite` has no field `transcript`; `MeetingNote` has no field `transcript`).

- [ ] **Step 3: Add `transcript` to both structs**

In `MeetingNote` (after `pub summary_updated_at: i64,`):

```rust
    pub summary: String,
    pub summary_updated_at: i64,
    // 🦀 M22: the meeting transcript (plain text; pasted or imported). Empty = none.
    pub transcript: String,
}
```

In `MeetingNoteWrite` (after `pub body: String,`):

```rust
    pub body: String,
    // 🦀 M22: the transcript sent from the frontend. #[serde(default)] → an absent key
    //    deserializes to "" (defensive; the frontend always sends the current transcript).
    #[serde(default)]
    pub transcript: String,
}
```

- [ ] **Step 4: Add the column to the schema (fresh DBs + migration)**

In the `CREATE TABLE IF NOT EXISTS meeting_notes (...)` literal, add `transcript` after the `summary_updated_at` line:

```rust
            summary     TEXT NOT NULL DEFAULT '',
            summary_updated_at INTEGER NOT NULL DEFAULT 0,
            transcript  TEXT NOT NULL DEFAULT '',
            UNIQUE(calendar_id, event_id)
        );",
```

Then add the migration for existing DBs right after the two M21 `add_column_if_missing(conn, "meeting_notes", "summary"…)` / `"summary_updated_at"` lines:

```rust
    // 🦀 M22 additive migration: existing M20/M21 DBs get the transcript column here.
    add_column_if_missing(conn, "meeting_notes", "transcript", "TEXT NOT NULL DEFAULT ''")?;
```

- [ ] **Step 5: Extend `NOTE_COLS` + `row_to_note`**

Change `NOTE_COLS` to append `, transcript`:

```rust
const NOTE_COLS: &str = "id, calendar_id, event_id, event_title, event_start, body, created_at, updated_at, summary, summary_updated_at, transcript";
```

In `row_to_note`, add after `summary_updated_at: row.get(9)?,`:

```rust
        summary: row.get(8)?,
        summary_updated_at: row.get(9)?,
        transcript: row.get(10)?,
    })
```

- [ ] **Step 6: Carry `transcript` through `upsert_meeting_note`**

Replace the `conn.execute(...)` call in `upsert_meeting_note` (the comment's `?6` reference becomes `?7`):

```rust
    // 🦀 `?7` is reused for BOTH created_at and updated_at on insert. ON CONFLICT updates
    //    updated_at (= excluded.updated_at = ?7) but NOT created_at — so created_at keeps
    //    its first-insert value while updated_at moves forward. `summary`/`summary_updated_at`
    //    stay OUT of this statement, so a body/transcript save never clobbers the summary.
    conn.execute(
        "INSERT INTO meeting_notes
            (calendar_id, event_id, event_title, event_start, body, transcript, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(calendar_id, event_id) DO UPDATE SET
            event_title = excluded.event_title,
            event_start = excluded.event_start,
            body = excluded.body,
            transcript = excluded.transcript,
            updated_at = excluded.updated_at",
        params![w.calendar_id, w.event_id, w.event_title, w.event_start, w.body, w.transcript, now_ms],
    )?;
```

- [ ] **Step 7: Update the `note_write` test helper**

In `mod tests`, add `transcript` to the `note_write` helper so it builds a valid `MeetingNoteWrite`:

```rust
    fn note_write(cal: &str, ev: &str, body: &str) -> MeetingNoteWrite {
        MeetingNoteWrite {
            calendar_id: cal.into(),
            event_id: ev.into(),
            event_title: "Standup".into(),
            event_start: "2026-06-22T09:00:00-07:00".into(),
            body: body.into(),
            transcript: "".into(),
        }
    }
```

- [ ] **Step 8: Run the tests + lint**

Run: `cd src-tauri && cargo test --lib db::tests 2>&1 | tail -20`
Expected: PASS — the 3 new tests + all prior M20/M21 `meeting_note` tests. Then:
Run: `cd src-tauri && cargo test 2>&1 | grep -E "test result" && cargo clippy --all-targets 2>&1 | tail -8`
Expected: every binary green; clippy clean. Do NOT run `cargo fmt`.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "$(cat <<'EOF'
feat(m22): meeting_notes transcript column (saved with body, summary preserved)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** adding a column to the upsert's `ON CONFLICT DO UPDATE SET` is what makes a body/transcript save round-trip both fields, while *omitting* `summary` there is what preserves the AI summary across saves; `#[serde(default)]` makes the new wire field optional so an older caller can't fail to deserialize.

---

## Task 2: Pure `transcript.rs` (vtt_to_text + build_summary_input) (Rust, TDD)

**Files:** Create `src-tauri/src/transcript.rs`; modify `src-tauri/src/lib.rs` (`pub mod transcript;`).

- [ ] **Step 1: Create `src-tauri/src/transcript.rs` with the tests + (empty) functions**

Create the file:

```rust
// src-tauri/src/transcript.rs — pure transcript helpers (no I/O, fully unit-testable, M22).

/// Convert a WebVTT caption file to plain spoken text: drop the WEBVTT header, NOTE/STYLE/REGION
/// blocks, metadata lines, "-->" timestamp lines, and numeric cue ids; strip inline `<…>` tags;
/// collapse consecutive duplicate lines (rolling captions repeat). Plain `.txt` passes through.
pub fn vtt_to_text(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // 🦀 Skip the WebVTT header + block markers + metadata.
        if t.starts_with("WEBVTT")
            || t.starts_with("NOTE")
            || t.starts_with("STYLE")
            || t.starts_with("REGION")
            || t.starts_with("Kind:")
            || t.starts_with("Language:")
        {
            continue;
        }
        // 🦀 Timestamp cue lines contain the "-->" arrow.
        if t.contains("-->") {
            continue;
        }
        // 🦀 Numeric-only lines are cue identifiers (e.g. "1", "2").
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let cleaned = strip_tags(t);
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        // 🦀 Collapse a line identical to the previous kept line (rolling captions repeat).
        if out.last().map(|p| p == cleaned).unwrap_or(false) {
            continue;
        }
        out.push(cleaned.to_string());
    }
    out.join("\n")
}

// 🦀 Remove `<…>` segments (e.g. `<v Dana>`, `</v>`, `<00:00:00.000>`). A depth counter handles
//    a stray '>' gracefully and avoids pulling in a regex dependency.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth: u32 = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ => {
                if depth == 0 {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Build the text fed to the summarizer from the user's notes + the transcript. Both present →
/// labeled sections; only one → that one; neither → "" (the caller guards empty).
pub fn build_summary_input(body: &str, transcript: &str) -> String {
    let b = body.trim();
    let t = transcript.trim();
    match (b.is_empty(), t.is_empty()) {
        (false, false) => format!("Meeting notes:\n{b}\n\nTranscript:\n{t}"),
        (false, true) => b.to_string(),
        (true, false) => t.to_string(),
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtt_to_text_strips_structure_and_dedupes() {
        let raw = "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE recording\n\n\
                   1\n00:00:01.000 --> 00:00:03.000\n<v Dana>Hello everyone</v>\n\n\
                   2\n00:00:03.000 --> 00:00:05.000\nHello everyone\n\n\
                   3\n00:00:05.000 --> 00:00:07.000\nLet's start the review";
        assert_eq!(vtt_to_text(raw), "Hello everyone\nLet's start the review");
    }

    #[test]
    fn vtt_to_text_passes_plain_text_through() {
        let raw = "Just some notes\nwith two lines";
        assert_eq!(vtt_to_text(raw), "Just some notes\nwith two lines");
    }

    #[test]
    fn build_summary_input_combines_or_falls_back() {
        assert_eq!(build_summary_input("notes", "tr"), "Meeting notes:\nnotes\n\nTranscript:\ntr");
        assert_eq!(build_summary_input("notes", "   "), "notes"); // transcript blank → body only
        assert_eq!(build_summary_input("", "tr"), "tr"); // body blank → transcript only
        assert_eq!(build_summary_input("  ", ""), ""); // both blank → empty
    }
}
```

- [ ] **Step 2: Declare the module in `lib.rs`**

In `src-tauri/src/lib.rs`, after `pub mod ollama;`, add:

```rust
// 🦀 Pure transcript helpers (WebVTT→text, summary-input builder) — no I/O, unit-tested (M22).
pub mod transcript;
```

- [ ] **Step 3: Run the tests + lint**

Run: `cd src-tauri && cargo test --lib transcript 2>&1 | tail -15`
Expected: PASS — the 3 transcript tests. Then:
Run: `cd src-tauri && cargo clippy --all-targets 2>&1 | tail -8`
Expected: clippy clean. Do NOT run `cargo fmt`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transcript.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(m22): pure transcript module — vtt_to_text + build_summary_input

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** a pure function (no I/O) like `vtt_to_text` is the easiest thing to unit-test — feed a string, assert a string; `&str` slices + `chars()` let us strip tags without allocating a regex engine; `match` on a tuple of two bools is a tidy way to cover the four notes/transcript combinations.

---

## Task 3: `read_transcript_file` command + `summarize_meeting_note` change (Rust)

**Files:** Modify `src-tauri/src/commands.rs`; modify `src-tauri/src/lib.rs` (register the command).

No new unit tests (the parser is tested in Task 2; the DB in Task 1; `read_transcript_file` is thin `std::fs` glue). Verification is compile + clippy + existing tests green.

- [ ] **Step 1: Add `read_transcript_file`**

In `src-tauri/src/commands.rs`, after the `summarize_meeting_note` command (near the end, before `#[cfg(test)]`), add:

```rust
/// Read a user-picked transcript file (.txt or .vtt) into plain text (M22). DB-free; `.vtt` is
/// stripped to spoken text. The byte read happens here in Rust (std::fs) so no fs capability is
/// needed — the frontend supplies the path from the native open dialog.
#[tauri::command]
pub async fn read_transcript_file(path: String) -> Result<String> {
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Other(format!("could not read transcript file: {e}")))?;
    // 🦀 `.ends_with` on the lowercased path picks the parser; .txt (and anything else) passes through.
    let text = if path.to_lowercase().ends_with(".vtt") {
        crate::transcript::vtt_to_text(&raw)
    } else {
        raw
    };
    Ok(text.trim().to_string())
}
```

- [ ] **Step 2: Change `summarize_meeting_note`'s input to notes + transcript combined**

In `src-tauri/src/commands.rs`, replace the body of `summarize_meeting_note` — specifically the `let body = { … };` block, the empty-guard, and the `summarize(&body)` call — so the function reads (keep the signature + doc comment, but update the comment's "body" wording):

```rust
/// Summarize a meeting note with local Ollama (M21/M22). Reads the SAVED note, combines the
/// freeform body + transcript, calls Ollama OUTSIDE the DB lock, then persists the summary.
/// Requires the note to be saved first.
#[tauri::command]
pub async fn summarize_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<db::MeetingNote> {
    // 🦀 Build the summary input from the SAVED note (notes + transcript combined) in a short
    //    locked block, then DROP the guard before the network await.
    let input = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        let note = db::get_meeting_note(&conn, &calendar_id, &event_id)?
            .ok_or_else(|| AppError::Other("Save the note before summarizing.".into()))?;
        crate::transcript::build_summary_input(&note.body, &note.transcript)
    };
    if input.trim().is_empty() {
        return Err(AppError::Other(
            "Nothing to summarize — add notes or a transcript first.".into(),
        ));
    }
    // 🦀 The slow part: a local HTTP call to Ollama. No DB lock is held across this await.
    let summary = crate::ollama::OllamaClient::new().summarize(&input).await?;
    // 🦀 Re-lock to persist. This UPDATE does NOT bump the body's updated_at (staleness logic).
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::set_meeting_note_summary(&conn, &calendar_id, &event_id, &summary, now_millis())
}
```

- [ ] **Step 3: Register `read_transcript_file` in `lib.rs`**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]`, add after `commands::summarize_meeting_note,`:

```rust
            commands::read_transcript_file,
```

- [ ] **Step 4: Verify compile + lint + tests**

Run: `cd src-tauri && cargo test 2>&1 | grep -E "test result" && cargo clippy --all-targets 2>&1 | tail -8`
Expected: builds, all tests pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(m22): read_transcript_file command + summarize combined notes+transcript

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** `read_transcript_file` mirrors M17's download — the native file dialog runs in the frontend, but the actual disk read is a Rust command using `std::fs`, so no extra Tauri capability is required; `summarize_meeting_note` changed only in *what* it feeds Ollama (the combined input), keeping the same lock-drop-before-await structure.

---

## Task 4: Frontend wrapper + types + mock (TypeScript)

**Files:** Modify `src/lib/notes.ts`; modify `src/lib/mock.ts`.

- [ ] **Step 1: Add `transcript` to both interfaces + the wrapper in `notes.ts`**

In `src/lib/notes.ts`, add to `MeetingNote` (after `summary_updated_at`):

```ts
  /** Unix milliseconds the summary was generated (0 = never). */
  summary_updated_at: number;
  /** M22: the meeting transcript (plain text). Empty = none. */
  transcript: string;
}
```

Add to `MeetingNoteWrite` (after `body`):

```ts
  body: string;
  transcript: string;
}
```

Update the mock import block to also import `mockReadTranscriptFile`:

```ts
import {
  mockGetMeetingNote,
  mockSaveMeetingNote,
  mockDeleteMeetingNote,
  mockListMeetingNotes,
  mockSummarizeMeetingNote,
  mockReadTranscriptFile,
} from "./mock";
```

Add the wrapper at the end of the file:

```ts
export const readTranscriptFile = (path: string): Promise<string> =>
  isTauri()
    ? invoke<string>("read_transcript_file", { path })
    : Promise.resolve(mockReadTranscriptFile(path));
```

- [ ] **Step 2: Update `src/lib/mock.ts`**

Give the seeded notes a `transcript`. e2 ("1:1 with Dana") gets a short demo transcript; e6 ("Roadmap") gets `""`. Add the field to each seed object:

```ts
      summary: "## Summary\n- Career growth + Q3 priorities discussed\n\n## Action items\n- [ ] Share the roadmap doc",
      summary_updated_at: 1_750_000_100_000,
      transcript: "Dana: How's the quarter going?\nYou: On track — shipping the roadmap doc Friday.",
    },
```

```ts
      summary: "", summary_updated_at: 0,
      transcript: "",
    },
```

In `mockSaveMeetingNote`, add `transcript` to the constructed note (the frontend sends the current transcript, mirroring the real upsert):

```ts
    summary: existing?.summary ?? "",
    summary_updated_at: existing?.summary_updated_at ?? 0,
    transcript: w.transcript,
  };
```

In `mockSummarizeMeetingNote`, add `transcript: ""` to the synthesized `base` fallback object so it remains a valid `MeetingNote` (the `existing` branch already carries it):

```ts
  const base: MeetingNote = existing ?? {
    id: mockNoteId++, calendar_id: calendarId, event_id: eventId,
    event_title: "", event_start: "", body: "",
    created_at: 1_750_000_500_000, updated_at: 1_750_000_500_000,
    summary: "", summary_updated_at: 0, transcript: "",
  };
```

Append `mockReadTranscriptFile` at the end of the meeting-notes block:

```ts
// Browser-maket: pretend a .vtt was picked + parsed to plain text.
export function mockReadTranscriptFile(_path: string): string {
  return "Dana: Welcome everyone.\nYou: Let's review the Q3 priorities.\nDana: Action — share the roadmap doc by Friday.";
}
```

- [ ] **Step 3: Verify the build**

Run: `npm run build 2>&1 | tail -12`
Expected: clean (TypeScript compiles; no errors about a missing `transcript` on `MeetingNote` — every seed + `mockSaveMeetingNote` + the `base` fallback now supply it).

- [ ] **Step 4: Commit**

```bash
git add src/lib/notes.ts src/lib/mock.ts
git commit -m "$(cat <<'EOF'
feat(m22): transcript on MeetingNote + readTranscriptFile wrapper + mock

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: NotesModal Transcript section + Import + styles (React/TypeScript)

**Files:** Modify `src/components/NotesModal.tsx` (replace wholesale); modify `src/styles/app.css`.

- [ ] **Step 1: Replace the entire contents of `src/components/NotesModal.tsx` with:**

```tsx
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getMeetingNote,
  saveMeetingNote,
  deleteMeetingNote,
  summarizeMeetingNote,
  readTranscriptFile,
} from "../lib/notes";

// What the editor needs to open: the event identity + a title/start snapshot to store.
export interface NoteTarget {
  calendarId: string;
  eventId: string;
  eventTitle: string;
  eventStart: string;
}

export function NotesModal({
  target,
  onClose,
  onSaved,
}: {
  target: NoteTarget;
  onClose: () => void;
  onSaved: () => void; // reload the panel + dots
}) {
  const [body, setBody] = useState("");
  const [savedBody, setSavedBody] = useState(""); // the body currently persisted
  const [transcript, setTranscript] = useState("");
  const [savedTranscript, setSavedTranscript] = useState(""); // the transcript currently persisted
  const [exists, setExists] = useState(false); // a note already stored → show Delete
  const [summary, setSummary] = useState("");
  const [summaryUpdatedAt, setSummaryUpdatedAt] = useState(0);
  const [noteUpdatedAt, setNoteUpdatedAt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false); // save/delete in flight
  const [summarizing, setSummarizing] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Esc closes (matches EventModal/ComposeModal — window listener, no backdrop close).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Load any existing note for this event on open.
  useEffect(() => {
    let cancelled = false;
    getMeetingNote(target.calendarId, target.eventId)
      .then((n) => {
        if (cancelled) return;
        setBody(n?.body ?? "");
        setSavedBody(n?.body ?? "");
        setTranscript(n?.transcript ?? "");
        setSavedTranscript(n?.transcript ?? "");
        setExists(!!n);
        setSummary(n?.summary ?? "");
        setSummaryUpdatedAt(n?.summary_updated_at ?? 0);
        setNoteUpdatedAt(n?.updated_at ?? 0);
        setLoading(false);
      })
      .catch((e) => {
        if (!cancelled) {
          setError(String(e));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [target.calendarId, target.eventId]);

  // Enough to save/summarize: any notes OR any transcript.
  const hasContent = body.trim() !== "" || transcript.trim() !== "";
  // A stored summary is stale if the note has been edited since it was generated.
  const stale = summary !== "" && noteUpdatedAt > summaryUpdatedAt;

  function writePayload() {
    return {
      calendar_id: target.calendarId,
      event_id: target.eventId,
      event_title: target.eventTitle,
      event_start: target.eventStart,
      body,
      transcript,
    };
  }

  async function handleSave() {
    if (!hasContent) return; // Save is disabled when empty; guard regardless
    setBusy(true);
    setError(null);
    try {
      await saveMeetingNote(writePayload());
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (!window.confirm("Delete this note?")) return;
    setBusy(true);
    setError(null);
    try {
      await deleteMeetingNote(target.calendarId, target.eventId);
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleImport() {
    setImporting(true);
    setError(null);
    try {
      let path: string | null;
      if (isTauri()) {
        const sel = await open({ filters: [{ name: "Transcript", extensions: ["txt", "vtt"] }] });
        path = typeof sel === "string" ? sel : null; // null if cancelled (or a multi-array)
      } else {
        path = "/mock/transcript.vtt"; // maket: skip the native dialog
      }
      if (!path) return; // cancelled
      const text = await readTranscriptFile(path);
      setTranscript(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function handleSummarize() {
    if (!hasContent) return;
    setSummarizing(true);
    setError(null);
    try {
      // Persist the current notes/transcript first so the summary reflects the latest text.
      if (body !== savedBody || transcript !== savedTranscript) {
        await saveMeetingNote(writePayload());
        setSavedBody(body);
        setSavedTranscript(transcript);
      }
      const n = await summarizeMeetingNote(target.calendarId, target.eventId);
      setSummary(n.summary);
      setSummaryUpdatedAt(n.summary_updated_at);
      setNoteUpdatedAt(n.updated_at);
      setExists(true);
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSummarizing(false);
    }
  }

  const blocked = busy || summarizing || importing;

  return (
    <div className="compose-overlay">
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="note-title">
        <div className="compose-head">
          <span className="compose-title" id="note-title">Notes — {target.eventTitle}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="note-when">{new Date(target.eventStart).toLocaleString()}</div>
        {loading ? (
          <div className="cal-loading">Loading…</div>
        ) : (
          <>
            <textarea
              className="compose-body"
              placeholder="Write meeting notes…"
              value={body}
              onChange={(e) => setBody(e.target.value)}
              rows={8}
              autoFocus
            />
            <div className="note-transcript-head">
              <span>Transcript</span>
              <button className="btn" onClick={handleImport} disabled={blocked}>
                {importing ? "Importing…" : "Import…"}
              </button>
            </div>
            <textarea
              className="compose-body"
              placeholder="Paste a transcript, or Import a .txt / .vtt…"
              value={transcript}
              onChange={(e) => setTranscript(e.target.value)}
              rows={6}
            />
            <div className="note-summary-section">
              <div className="note-summary-head">
                <span>Summary</span>
                <button className="btn" onClick={handleSummarize} disabled={blocked || !hasContent}>
                  {summarizing ? "Summarizing…" : summary ? "Regenerate" : "Summarize"}
                </button>
              </div>
              {stale && (
                <div className="note-summary-stale">Notes changed since this summary — Regenerate.</div>
              )}
              {summary ? (
                <pre className="note-summary">{summary}</pre>
              ) : (
                <div className="note-summary-empty">
                  No summary yet. Click Summarize to generate one with local Ollama.
                </div>
              )}
            </div>
          </>
        )}
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {exists && (
            <button className="btn btn-danger-outline" onClick={handleDelete} disabled={blocked}>
              Delete
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={blocked}>
            Cancel
          </button>
          <button className="btn btn-accent" onClick={handleSave} disabled={blocked || !hasContent}>
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add the style to `src/styles/app.css`**

Append to the end of `src/styles/app.css`:

```css
/* M22 transcript section */
.note-transcript-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
  font-size: 13px;
  margin: 10px 0 6px;
}
```

- [ ] **Step 3: Verify the build**

Run: `npm run build 2>&1 | tail -12`
Expected: clean build.

- [ ] **Step 4: Maket visual check**

Open the maket (a vite dev server is typically already on `http://localhost:1420`; else `npm run dev`). In the Calendar view:
1. Open **"1:1 with Dana"** → popover → **Notes** → the editor shows the body, a **Transcript** section with the seeded transcript ("Dana: How's the quarter going?…") and an **Import…** button, and the Summary panel.
2. Click **Import…** → the transcript textarea fills with the canned `mockReadTranscriptFile` text ("Dana: Welcome everyone.…").
3. Click **Summarize** → after a moment the demo summary appears (the maket mock).
4. Open **"Roadmap"** (empty transcript) → Transcript section is empty with the placeholder; Save/Summarize still enabled because the body has content.
If a browser can't be launched, verify the wiring by inspection and report that the visual check is deferred.

- [ ] **Step 5: Commit**

```bash
git add src/components/NotesModal.tsx src/styles/app.css
git commit -m "$(cat <<'EOF'
feat(m22): NotesModal transcript section + Import + save/summarize carry transcript

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Final verification gates

**Files:** none (verification only)

- [ ] **Step 1: Full backend gate**

Run: `cd src-tauri && cargo test 2>&1 | grep -E "test result|Running" && cargo clippy --all-targets 2>&1 | tail -10`
Expected: every test binary green (lib unit incl. the 3 new db tests + the 3 new transcript tests; `tests/ollama_test.rs` + calendar + gmail unchanged); clippy clean. Do NOT run `cargo fmt`.

- [ ] **Step 2: Full frontend gate**

Run: `npm run build 2>&1 | tail -12`
Expected: TypeScript type-checks and Vite builds with no errors.

- [ ] **Step 3: Confirm clean tree + review the branch**

Run: `git status -s && git log --oneline main..HEAD`
Expected: working tree clean; the M22 commits (Tasks 1–5) on `m22-transcript-import` atop the spec commit.

- [ ] **Step 4 (optional, owner): live check**

In the Tauri dev build (`npm run tauri dev`): open a note, **Import…** a real `.vtt`/`.txt` (or paste a transcript), **Save**, then **Summarize** with Ollama running (`ollama serve` + `ollama pull llama3.2`) → confirm a combined summary appears; edit the transcript + Save → reopen → the staleness hint shows.

---

## Notes for the executor

- **No DB migration framework** — one additive `ADD COLUMN transcript` with a safe default + the `CREATE TABLE` literal; idempotent `init` (pinned by `init_adds_transcript_column_to_a_pre_m22_table`).
- **No new OAuth scope, no new dependency** (`tauri-plugin-dialog` was added in M17). The file read is a Rust `std::fs` command (no `fs` capability).
- **The summary stays out of `upsert_meeting_note`** — saving body/transcript preserves it (pinned by `body_transcript_resave_preserves_summary`); only `set_meeting_note_summary` writes it.
- **Reviewers are READ-ONLY** — forbid Edit/Write and any git change ("REPORT ONLY"); run `git status -s` after each review.
- **Deferred (later sub-milestones):** M23 (local Whisper STT of an audio/video file — writes this same `transcript` field) and M24 (live macOS audio capture); `.srt`/other formats; speaker/timestamp display.
```
