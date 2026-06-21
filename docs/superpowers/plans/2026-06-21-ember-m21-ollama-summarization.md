# Ember — M21 Local-Ollama Meeting-Note Summarization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Summarize a meeting note's body with a local Ollama model into a structured Summary + Action items, store it, and show a staleness hint when the note is edited afterward.

**Architecture:** A new `OllamaClient` (`src-tauri/src/ollama.rs`, mirrors Gmail/Calendar clients: swappable `base_url`, a `reqwest::Client` with a 120s timeout, wiremock-testable) does a blocking `POST /api/generate`. The M20 `meeting_notes` table gains `summary` + `summary_updated_at` (additive, M6 pattern). An orchestrating command `summarize_meeting_note` reads the saved body, calls Ollama (lock dropped during the await), and persists the summary via a `db::set_meeting_note_summary` that does **not** bump the body's `updated_at` (so staleness = `updated_at > summary_updated_at`). The NotesModal gains a Summarize/Regenerate button, a summary panel, and a staleness hint.

**Tech Stack:** Rust (reqwest 0.12, serde, rusqlite, Tauri 2; wiremock 0.6 for tests — all already in `Cargo.toml`), React 19 + TypeScript + Vite. No new dependency. Requires local Ollama with `llama3.2` pulled at runtime (handled gracefully when absent).

**Learning mode (IMPORTANT):** every Rust block below carries `// 🦀` teaching comments — keep them verbatim. After each Rust task, give a 2–3 sentence plain-English recap. TS/React gets normal comments. **Do NOT run `cargo fmt`** (hand-formatted repo). Commit messages use `feat(m21:)`/`test(m21:)` style and end with the `Co-Authored-By` trailer shown in each commit step.

**Reference (read before starting):** spec at `docs/superpowers/specs/2026-06-21-ember-m21-ollama-summarization-design.md`. Patterns this plan mirrors: `src-tauri/src/calendar/mod.rs` (`CalendarClient` shape + `with_base_url`), `src-tauri/tests/calendar_test.rs` (wiremock integration crate), `src-tauri/src/db/mod.rs` (M20 `meeting_notes` CRUD, `add_column_if_missing`, the `#[cfg(test)]` `conn()`/`note_write` helpers), `src-tauri/src/commands.rs` (M20 note commands + `now_millis`, the locked-block/drop-before-`.await` discipline), `src/lib/notes.ts` + `src/lib/mock.ts` + `src/components/NotesModal.tsx` (M20 frontend).

---

## File structure

**Backend**
- `src-tauri/src/ollama.rs` — *create*: `OllamaClient` + `summarize` + private `build_prompt`/request/response structs.
- `src-tauri/tests/ollama_test.rs` — *create*: wiremock tests (happy path, 404, connection-refused).
- `src-tauri/src/lib.rs` — *modify*: `pub mod ollama;` + register `summarize_meeting_note`.
- `src-tauri/src/db/mod.rs` — *modify*: `summary`/`summary_updated_at` columns + `MeetingNote`/`NOTE_COLS`/`row_to_note` + `set_meeting_note_summary` + tests.
- `src-tauri/src/commands.rs` — *modify*: `summarize_meeting_note` command.

**Frontend**
- `src/lib/notes.ts` — *modify*: `MeetingNote` += `summary`/`summary_updated_at`; `summarizeMeetingNote` wrapper.
- `src/lib/mock.ts` — *modify*: seed summary fields (one stale); preserve summary on save; `mockSummarizeMeetingNote`.
- `src/components/NotesModal.tsx` — *modify*: Summarize/Regenerate button, summary panel, staleness hint.
- `src/styles/app.css` — *modify*: summary panel + stale-hint styles.

---

## Task 1: `OllamaClient` module + wiremock tests (Rust, TDD)

**Files:**
- Create: `src-tauri/src/ollama.rs`
- Create: `src-tauri/tests/ollama_test.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod ollama;`)

- [ ] **Step 1: Write the failing integration tests**

Create `src-tauri/tests/ollama_test.rs`:

```rust
// 🦀 Integration tests: a separate crate, so the client is reached as `ember_lib::ollama`.
use ember_lib::ollama::OllamaClient;
use serde_json::json;
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn summarize_posts_generate_request_and_returns_trimmed_response() {
    let server = MockServer::start().await;
    // The mock only matches if the POST body has model/stream:false AND a prompt that carries
    // the section instruction + the notes — so matching it also verifies build_prompt's output.
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .and(body_partial_json(json!({ "model": "llama3.2", "stream": false })))
        .and(body_string_contains("## Action items"))
        .and(body_string_contains("Reviewed the Q3 roadmap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "response": "\n## Summary\n- Discussed Q3\n\n## Action items\n- [ ] Share doc\n",
            "done": true
        })))
        .mount(&server)
        .await;

    let client = OllamaClient::with_base_url(server.uri());
    let summary = client.summarize("Reviewed the Q3 roadmap and assigned the doc.").await.unwrap();
    assert!(summary.contains("## Summary"));
    assert!(summary.contains("- [ ] Share doc"));
    // trimmed: no leading/trailing whitespace
    assert_eq!(summary, summary.trim());
}

#[tokio::test(flavor = "multi_thread")]
async fn summarize_maps_404_to_pull_instruction() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": "model 'llama3.2' not found, try pulling it first"
        })))
        .mount(&server)
        .await;
    let client = OllamaClient::with_base_url(server.uri());
    let err = client.summarize("notes").await.unwrap_err().to_string().to_lowercase();
    assert!(err.contains("ollama pull"), "got: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn summarize_maps_connection_refused_to_friendly_message() {
    // 🦀 Port 1 has nothing listening → an immediate connection-refused (reqwest is_connect()).
    let client = OllamaClient::with_base_url("http://127.0.0.1:1".into());
    let err = client.summarize("notes").await.unwrap_err().to_string().to_lowercase();
    assert!(err.contains("isn't running") || err.contains("ollama serve"), "got: {err}");
}
```

- [ ] **Step 2: Run the tests to verify they fail (don't compile)**

Run: `cd src-tauri && cargo test --test ollama_test 2>&1 | tail -15`
Expected: FAIL — `unresolved import ember_lib::ollama` / `OllamaClient` not found.

- [ ] **Step 3: Create the `ollama.rs` module**

Create `src-tauri/src/ollama.rs`:

```rust
// src-tauri/src/ollama.rs — local Ollama client (meeting-note summarization, M21).
// Mirrors GmailClient/CalendarClient: a swappable base_url + a reusable reqwest::Client.
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "http://localhost:11434";
const MODEL: &str = "llama3.2";

pub struct OllamaClient {
    base_url: String,
    http: reqwest::Client,
}

// 🦀 `new()` takes no args, so clippy wants a matching `Default` impl — provide one that delegates.
impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaClient {
    pub fn new() -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), http: build_http() }
    }

    /// Point the client at a mock server in tests.
    pub fn with_base_url(base_url: String) -> Self {
        Self { base_url, http: build_http() }
    }

    /// Summarize meeting notes via Ollama's blocking /api/generate. Maps the two common
    /// local-setup failures (Ollama not running, model not pulled) to actionable messages.
    pub async fn summarize(&self, notes: &str) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let req = GenerateRequest { model: MODEL, prompt: build_prompt(notes), stream: false };
        // 🦀 `.send()` can fail before any HTTP status — e.g. connection refused. `is_connect()`
        //    tells "couldn't even reach the server" apart from other errors, so we can show a
        //    friendly setup hint instead of a raw reqwest message.
        let resp = self.http.post(&url).json(&req).send().await.map_err(|e| {
            if e.is_connect() {
                AppError::Other(format!(
                    "Ollama isn't running at {DEFAULT_BASE} — install it from https://ollama.com and run `ollama serve`."
                ))
            } else {
                AppError::Http(e)
            }
        })?;
        // 🦀 Ollama returns 404 when the requested model hasn't been pulled.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::Other(format!(
                "Ollama model '{MODEL}' not found. Run: ollama pull {MODEL}"
            )));
        }
        let resp = resp.error_for_status()?;
        let parsed: GenerateResponse = resp.json().await?;
        let summary = parsed.response.trim().to_string();
        if summary.is_empty() {
            return Err(AppError::Other("Ollama returned an empty summary.".into()));
        }
        Ok(summary)
    }
}

// 🦀 One place to build the HTTP client: a generous 120s timeout (local CPU generation is slow);
//    `.build()` returns a Result, and `.expect` mirrors what `reqwest::Client::new()` does internally.
fn build_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build reqwest client")
}

// 🦀 The /api/generate request body. `<'a>` lets `model` borrow the &'static str; `prompt` is owned.
#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
}

// 🦀 We only need `response` from Ollama's JSON; serde ignores the other fields (done, etc.).
#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

// 🦀 Pure prompt builder (no I/O), kept private — the wiremock happy-path test asserts its output
//    via the captured request body. Asks for a compact, factual markdown summary + action items.
fn build_prompt(notes: &str) -> String {
    format!(
        "You are a meeting-notes assistant. Summarize the meeting notes below into concise \
         GitHub-flavored markdown with exactly these two sections:\n\
         ## Summary\n- 2 to 4 short bullet points capturing the key points\n\
         ## Action items\n- one `- [ ]` checkbox per action item; if there are none, write \"_None_\"\n\n\
         Be factual and concise. Do not invent information that is not in the notes.\n\n\
         Notes:\n{notes}"
    )
}
```

- [ ] **Step 4: Declare the module in `lib.rs`**

In `src-tauri/src/lib.rs`, after the `pub mod mime;` declaration (~line 41), add:

```rust
// 🦀 Local Ollama client for meeting-note summarization (M21). `pub` so the wiremock
//    integration test in tests/ollama_test.rs (a separate crate) can reach it.
pub mod ollama;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --test ollama_test 2>&1 | tail -15`
Expected: PASS — all 3 tests green. Then lint:
Run: `cd src-tauri && cargo clippy --all-targets 2>&1 | tail -10`
Expected: clippy clean (no warnings). Do NOT run `cargo fmt`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/ollama.rs src-tauri/tests/ollama_test.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(m21): OllamaClient summarize (/api/generate) + wiremock tests

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** a reqwest POST with a JSON body is `client.post(url).json(&body).send().await`; `reqwest::Error::is_connect()` distinguishes "couldn't reach the server" from other failures so we can show a friendly message; and a `new()` with no args wants a `Default` impl to satisfy clippy.

---

## Task 2: DB — `summary` columns + `set_meeting_note_summary` (Rust, TDD)

**Files:**
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/db/mod.rs`, inside `mod tests` (after `init_creates_meeting_notes_table_idempotently`, before the closing `}`), add:

```rust
    #[test]
    fn set_meeting_note_summary_sets_summary_without_bumping_updated_at() {
        let c = conn();
        let n = upsert_meeting_note(&c, &note_write("primary", "e1", "body"), 1000).unwrap();
        assert_eq!(n.summary, ""); // default after insert
        assert_eq!(n.summary_updated_at, 0);
        assert_eq!(n.updated_at, 1000);

        let updated = set_meeting_note_summary(&c, "primary", "e1", "## Summary\n- ok", 2000).unwrap();
        assert_eq!(updated.summary, "## Summary\n- ok");
        assert_eq!(updated.summary_updated_at, 2000);
        assert_eq!(updated.updated_at, 1000); // body's updated_at must NOT move
        assert_eq!(updated.created_at, 1000);
    }

    #[test]
    fn body_resave_preserves_existing_summary() {
        let c = conn();
        upsert_meeting_note(&c, &note_write("primary", "e1", "body1"), 1000).unwrap();
        set_meeting_note_summary(&c, "primary", "e1", "the summary", 1500).unwrap();
        // Edit the body later (a fresh save with a newer clock).
        let mut w = note_write("primary", "e1", "body2");
        w.event_title = "1:1".into();
        let after = upsert_meeting_note(&c, &w, 3000).unwrap();
        assert_eq!(after.body, "body2");
        assert_eq!(after.updated_at, 3000); // body edit advanced updated_at
        assert_eq!(after.summary, "the summary"); // summary PRESERVED
        assert_eq!(after.summary_updated_at, 1500); // and its timestamp
        // → stale (updated_at 3000 > summary_updated_at 1500), which the UI will flag.
    }

    #[test]
    fn init_adds_summary_columns_to_an_m20_shaped_table() {
        // 🦀 Simulate a pre-M21 (M20) meeting_notes table WITHOUT the summary columns + a row.
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE meeting_notes (
                id INTEGER PRIMARY KEY, calendar_id TEXT NOT NULL, event_id TEXT NOT NULL,
                event_title TEXT NOT NULL DEFAULT '', event_start TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                UNIQUE(calendar_id, event_id));
             INSERT INTO meeting_notes
                (calendar_id, event_id, event_title, event_start, body, created_at, updated_at)
                VALUES ('primary','e1','T','2026-01-01','b',1,1);",
        )
        .unwrap();

        init(&c).unwrap();

        let n = get_meeting_note(&c, "primary", "e1").unwrap().unwrap();
        assert_eq!(n.summary, ""); // backfilled default
        assert_eq!(n.summary_updated_at, 0);
        assert_eq!(n.body, "b");

        // Idempotent: a second init must not error and the row survives.
        init(&c).unwrap();
        assert!(get_meeting_note(&c, "primary", "e1").unwrap().is_some());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib db::tests 2>&1 | tail -20`
Expected: FAIL — compile errors (`set_meeting_note_summary` not found; `MeetingNote` has no field `summary`).

- [ ] **Step 3: Add the two columns to `MeetingNote`**

In `src-tauri/src/db/mod.rs`, in the `MeetingNote` struct (~line 60), add the two fields after `updated_at`:

```rust
    pub created_at: i64,
    pub updated_at: i64,
    // 🦀 M21: the local-Ollama summary (markdown text) + when it was generated (Unix ms).
    //    Empty string / 0 mean "never summarized". Staleness = updated_at > summary_updated_at.
    pub summary: String,
    pub summary_updated_at: i64,
```

- [ ] **Step 4: Add the columns to the schema (fresh DBs + migration for M20 DBs)**

In the `CREATE TABLE IF NOT EXISTS meeting_notes (...)` literal (~line 122), add the two columns before `UNIQUE(...)`:

```rust
        CREATE TABLE IF NOT EXISTS meeting_notes (
            id          INTEGER PRIMARY KEY,
            calendar_id TEXT NOT NULL,
            event_id    TEXT NOT NULL,
            event_title TEXT NOT NULL DEFAULT '',
            event_start TEXT NOT NULL DEFAULT '',
            body        TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL,
            summary     TEXT NOT NULL DEFAULT '',
            summary_updated_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE(calendar_id, event_id)
        );",
```

Then, right after the existing `messages` `add_column_if_missing` block (after the `add_column_if_missing(conn, "messages", "category", ...)` line, ~line 146), add the meeting_notes migration for existing M20 DBs:

```rust
    // 🦀 M21 additive migration: existing M20 DBs already have the meeting_notes table (so the
    //    CREATE above is a no-op for them) — add the new summary columns here. NOT NULL + DEFAULT
    //    backfills existing rows. Independent of the messages `needs_migration` wipe above.
    add_column_if_missing(conn, "meeting_notes", "summary", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "meeting_notes", "summary_updated_at", "INTEGER NOT NULL DEFAULT 0")?;
```

- [ ] **Step 5: Extend `NOTE_COLS` + `row_to_note`**

Change `NOTE_COLS` (~line 469) to include the two columns (order matters — it must match `row_to_note`):

```rust
const NOTE_COLS: &str = "id, calendar_id, event_id, event_title, event_start, body, created_at, updated_at, summary, summary_updated_at";
```

In `row_to_note` (~line 474), add the two reads after `updated_at: row.get(7)?,`:

```rust
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        summary: row.get(8)?,
        summary_updated_at: row.get(9)?,
    })
```

(Leave `upsert_meeting_note` UNCHANGED — it omits `summary`/`summary_updated_at`, so a fresh insert defaults them and a body re-save preserves an existing summary. That's the contract the tests pin.)

- [ ] **Step 6: Add `set_meeting_note_summary`**

In `src-tauri/src/db/mod.rs`, after `delete_meeting_note` (~line 545, before `#[cfg(test)]`), add:

```rust
/// Set the AI summary (M21), stamping `summary_updated_at` but NOT touching the body's
/// `updated_at` — so staleness (`updated_at > summary_updated_at`) tracks body edits only.
/// Returns the updated row; errors if the note doesn't exist (it must be saved first).
pub fn set_meeting_note_summary(
    conn: &Connection,
    calendar_id: &str,
    event_id: &str,
    summary: &str,
    now_ms: i64,
) -> Result<MeetingNote> {
    conn.execute(
        "UPDATE meeting_notes SET summary = ?1, summary_updated_at = ?2
         WHERE calendar_id = ?3 AND event_id = ?4",
        params![summary, now_ms, calendar_id, event_id],
    )?;
    get_meeting_note(conn, calendar_id, event_id)?
        .ok_or_else(|| crate::error::AppError::Other("note not found".into()))
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib db::tests 2>&1 | tail -20`
Expected: PASS — including the 3 new tests + all prior M20 `meeting_note` tests. Then the full suite + lint:
Run: `cd src-tauri && cargo test 2>&1 | grep -E "test result" && cargo clippy --all-targets 2>&1 | tail -8`
Expected: every binary green; clippy clean.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/db/mod.rs
git commit -m "$(cat <<'EOF'
feat(m21): meeting_notes summary columns + set_meeting_note_summary

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** SQLite `ALTER TABLE ADD COLUMN ... NOT NULL DEFAULT x` backfills existing rows, which is why the M20→M21 upgrade is a safe two-line additive migration; and by leaving `summary` out of the body-upsert's column list, saving the body can never clobber the summary — the new column just keeps its stored value.

---

## Task 3: `summarize_meeting_note` command + registration (Rust)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add the command after `list_meeting_notes`, ~line 842)
- Modify: `src-tauri/src/lib.rs` (register it)

No new unit tests (DB logic is covered in Task 2; the OllamaClient in Task 1). Verification is compile + clippy + existing tests green.

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, after `list_meeting_notes` (the last meeting-note command, ~line 842), add:

```rust
/// Summarize a meeting note's body with local Ollama (M21). Reads the SAVED body, calls Ollama
/// OUTSIDE the DB lock, then persists the summary. Requires the note to be saved first.
#[tauri::command]
pub async fn summarize_meeting_note(
    calendar_id: String,
    event_id: String,
    state: tauri::State<'_, Db>,
) -> Result<db::MeetingNote> {
    // 🦀 Read the body in a short locked block, then DROP the guard before the network await
    //    (a std MutexGuard must never be held across `.await`).
    let body = {
        let conn = state
            .lock()
            .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
        db::get_meeting_note(&conn, &calendar_id, &event_id)?
            .map(|n| n.body)
            .ok_or_else(|| AppError::Other("Save the note before summarizing.".into()))?
    };
    if body.trim().is_empty() {
        return Err(AppError::Other("Nothing to summarize — the note is empty.".into()));
    }
    // 🦀 The slow part: a local HTTP call to Ollama. No DB lock is held across this await.
    let summary = crate::ollama::OllamaClient::new().summarize(&body).await?;
    // 🦀 Re-lock to persist. This UPDATE does NOT bump the body's updated_at (staleness logic).
    let conn = state
        .lock()
        .map_err(|_| AppError::Other("database lock was poisoned".into()))?;
    db::set_meeting_note_summary(&conn, &calendar_id, &event_id, &summary, now_millis())
}
```

- [ ] **Step 2: Register it in `lib.rs`**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![ … ]`, add after `commands::list_meeting_notes,` (~line 125):

```rust
            commands::summarize_meeting_note,
```

- [ ] **Step 3: Verify compile + lint + tests**

Run: `cd src-tauri && cargo test 2>&1 | grep -E "test result" && cargo clippy --all-targets 2>&1 | tail -8`
Expected: builds, all tests pass, clippy clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(m21): summarize_meeting_note command (read body -> Ollama -> persist)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

**Rust recap to give the owner:** the command orchestrates two subsystems — it reads from SQLite, releases the lock so it isn't held across the `.await` on Ollama, then re-locks to write — the same lock-drop-before-network discipline the other DB+network commands follow.

---

## Task 4: Frontend wrapper + types + mock (TypeScript)

**Files:**
- Modify: `src/lib/notes.ts`
- Modify: `src/lib/mock.ts`

- [ ] **Step 1: Extend `MeetingNote` + add the wrapper in `notes.ts`**

In `src/lib/notes.ts`, add the two fields to the `MeetingNote` interface (after `updated_at`):

```ts
  /** Unix milliseconds. */
  updated_at: number;
  /** M21: local-Ollama summary (markdown). Empty = never summarized. */
  summary: string;
  /** Unix milliseconds the summary was generated (0 = never). */
  summary_updated_at: number;
}
```

Update the mock import block (lines 4–9) to also import `mockSummarizeMeetingNote`:

```ts
import {
  mockGetMeetingNote,
  mockSaveMeetingNote,
  mockDeleteMeetingNote,
  mockListMeetingNotes,
  mockSummarizeMeetingNote,
} from "./mock";
```

Add the wrapper at the end of the file (after `listMeetingNotes`):

```ts
export const summarizeMeetingNote = (calendarId: string, eventId: string): Promise<MeetingNote> =>
  isTauri()
    ? invoke<MeetingNote>("summarize_meeting_note", { calendarId, eventId })
    : Promise.resolve(mockSummarizeMeetingNote(calendarId, eventId));
```

- [ ] **Step 2: Update `mock.ts` — seed summary fields (one stale), preserve on save, add summarize**

In `src/lib/mock.ts`, give the two seeded notes their new fields. For the `e2` ("1:1 with Dana") seed, add a **stale** summary (its `summary_updated_at` is *less* than its `updated_at` of `1_750_000_200_000`, so the staleness hint demos):

```ts
    {
      id: 1, calendar_id: "primary", event_id: "e2",
      event_title: "1:1 with Dana", event_start: "2026-06-22T14:00:00-07:00",
      body: "- Career growth check-in\n- Reviewed Q3 priorities\n- Action: share the roadmap doc",
      created_at: 1_750_000_000_000, updated_at: 1_750_000_200_000,
      summary: "## Summary\n- Career growth + Q3 priorities discussed\n\n## Action items\n- [ ] Share the roadmap doc",
      summary_updated_at: 1_750_000_100_000,
    },
```

For the `e6` ("Roadmap") seed, add empty summary fields:

```ts
    {
      id: 2, calendar_id: "primary", event_id: "e6",
      event_title: "Roadmap", event_start: "2026-06-25T10:00:00-07:00",
      body: "Draft milestones for H2. Decide M21 scope next.",
      created_at: 1_750_000_000_000, updated_at: 1_750_000_100_000,
      summary: "", summary_updated_at: 0,
    },
```

In `mockSaveMeetingNote`, preserve the existing summary fields (saving the body must not wipe the summary) — add the two lines to the constructed `note`:

```ts
  const note: MeetingNote = {
    id: existing?.id ?? mockNoteId++,
    calendar_id: w.calendar_id,
    event_id: w.event_id,
    event_title: w.event_title,
    event_start: w.event_start,
    body: w.body,
    created_at: existing?.created_at ?? now,
    updated_at: now,
    summary: existing?.summary ?? "",
    summary_updated_at: existing?.summary_updated_at ?? 0,
  };
```

Append `mockSummarizeMeetingNote` after `mockListMeetingNotes`:

```ts
// Browser-maket: set a canned structured summary on the stored note. summary_updated_at is
// >= the note's updated_at, so the result reads as FRESH (no staleness hint right after).
export function mockSummarizeMeetingNote(calendarId: string, eventId: string): MeetingNote {
  const key = mockNoteKey(calendarId, eventId);
  const existing = MOCK_NOTES.get(key);
  const base: MeetingNote = existing ?? {
    id: mockNoteId++, calendar_id: calendarId, event_id: eventId,
    event_title: "", event_start: "", body: "",
    created_at: 1_750_000_500_000, updated_at: 1_750_000_500_000,
    summary: "", summary_updated_at: 0,
  };
  const note: MeetingNote = {
    ...base,
    summary: "## Summary\n- (demo) Key points captured from the notes\n\n## Action items\n- [ ] (demo) Follow up with the team",
    summary_updated_at: Math.max(base.updated_at, 1_750_000_600_000),
  };
  MOCK_NOTES.set(key, note);
  return note;
}
```

- [ ] **Step 3: Verify the build**

Run: `npm run build 2>&1 | tail -12`
Expected: clean (TypeScript compiles, Vite bundles).

- [ ] **Step 4: Commit**

```bash
git add src/lib/notes.ts src/lib/mock.ts
git commit -m "$(cat <<'EOF'
feat(m21): summarizeMeetingNote wrapper + summary fields + mock

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: NotesModal summary UI + styles (React/TypeScript)

**Files:**
- Modify: `src/components/NotesModal.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Wire summarization into `NotesModal.tsx`**

Replace the entire contents of `src/components/NotesModal.tsx` with:

```tsx
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  getMeetingNote,
  saveMeetingNote,
  deleteMeetingNote,
  summarizeMeetingNote,
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
  const [exists, setExists] = useState(false); // a note already stored → show Delete
  const [summary, setSummary] = useState("");
  const [summaryUpdatedAt, setSummaryUpdatedAt] = useState(0);
  const [noteUpdatedAt, setNoteUpdatedAt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false); // save/delete in flight
  const [summarizing, setSummarizing] = useState(false);
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

  // A stored summary is stale if the body has been edited since it was generated.
  const stale = summary !== "" && noteUpdatedAt > summaryUpdatedAt;

  async function handleSave() {
    if (body.trim() === "") return; // Save is disabled when empty; guard regardless
    setBusy(true);
    setError(null);
    try {
      await saveMeetingNote({
        calendar_id: target.calendarId,
        event_id: target.eventId,
        event_title: target.eventTitle,
        event_start: target.eventStart,
        body,
      });
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

  async function handleSummarize() {
    if (body.trim() === "") return;
    setSummarizing(true);
    setError(null);
    try {
      // Persist the current body first so the summary reflects the latest text.
      if (body !== savedBody) {
        await saveMeetingNote({
          calendar_id: target.calendarId,
          event_id: target.eventId,
          event_title: target.eventTitle,
          event_start: target.eventStart,
          body,
        });
        setSavedBody(body);
      }
      const n = await summarizeMeetingNote(target.calendarId, target.eventId);
      setSummary(n.summary);
      setSummaryUpdatedAt(n.summary_updated_at);
      setNoteUpdatedAt(n.updated_at);
      setExists(true);
      onSaved(); // a note now exists / changed → refresh the calendar dots + drawer
    } catch (e) {
      setError(String(e));
    } finally {
      setSummarizing(false);
    }
  }

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
              rows={10}
              autoFocus
            />
            <div className="note-summary-section">
              <div className="note-summary-head">
                <span>Summary</span>
                <button
                  className="btn"
                  onClick={handleSummarize}
                  disabled={summarizing || busy || body.trim() === ""}
                >
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
            <button
              className="btn btn-danger-outline"
              onClick={handleDelete}
              disabled={busy || summarizing}
            >
              Delete
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={busy || summarizing}>
            Cancel
          </button>
          <button
            className="btn btn-accent"
            onClick={handleSave}
            disabled={busy || summarizing || body.trim() === ""}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Add the styles to `src/styles/app.css`**

Append to the end of `src/styles/app.css`:

```css
/* M21 meeting-note summary */
.note-summary-section {
  margin-top: 10px;
  border-top: 1px solid var(--border, #e8e4dc);
  padding-top: 8px;
}
.note-summary-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
  font-size: 13px;
  margin-bottom: 6px;
}
.note-summary {
  white-space: pre-wrap;
  font-family: inherit;
  font-size: 12px;
  margin: 0;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--surface-2, rgba(0, 0, 0, 0.04));
  max-height: 220px;
  overflow-y: auto;
}
.note-summary-empty {
  font-size: 12px;
  color: var(--text-muted, #6b6660);
}
.note-summary-stale {
  font-size: 12px;
  color: var(--danger, #c0392b);
  margin-bottom: 6px;
}
```

- [ ] **Step 3: Verify the build**

Run: `npm run build 2>&1 | tail -12`
Expected: clean build.

- [ ] **Step 4: Maket visual check**

Open the maket (a vite dev server is typically already running on `http://localhost:1420`; otherwise `npm run dev`). In the Calendar view:
1. Click the event **"1:1 with Dana"** → its detail popover → **Notes** → the editor shows the seeded body AND a **Summary** panel with the pre-filled summary plus the **"Notes changed since this summary — Regenerate."** hint (the e2 seed is intentionally stale).
2. Click the event **"Roadmap"** → Notes → the Summary panel shows "No summary yet." with a **Summarize** button; click it → after a moment the canned demo summary appears and the button reads **Regenerate**.
If a browser can't be launched, verify the wiring by inspection and report that the visual check is deferred.

- [ ] **Step 5: Commit**

```bash
git add src/components/NotesModal.tsx src/styles/app.css
git commit -m "$(cat <<'EOF'
feat(m21): NotesModal summarize button + summary panel + stale hint

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Final verification gates

**Files:** none (verification only)

- [ ] **Step 1: Full backend gate**

Run: `cd src-tauri && cargo test 2>&1 | grep -E "test result|Running" && cargo clippy --all-targets 2>&1 | tail -10`
Expected: every test binary green (lib unit incl. the 3 new db tests; `tests/ollama_test.rs` 3 tests; calendar + gmail unchanged); clippy clean. Do NOT run `cargo fmt`.

- [ ] **Step 2: Full frontend gate**

Run: `npm run build 2>&1 | tail -12`
Expected: TypeScript type-checks and Vite builds with no errors.

- [ ] **Step 3: Confirm clean tree + review the branch**

Run: `git status -s && git log --oneline main..HEAD`
Expected: working tree clean; the M21 commits (Tasks 1–5) on `m21-ollama-summarization` atop the spec commit.

- [ ] **Step 4 (optional, owner): live Ollama check**

With Ollama installed and the model pulled (`ollama pull llama3.2`, `ollama serve` running), open a real note in the Tauri dev build (`npm run tauri dev`), type some notes, click **Summarize**, and confirm a real summary appears and persists; then edit the body, Save, reopen → the staleness hint shows. With Ollama stopped, confirm the friendly "Ollama isn't running…" message.

---

## Notes for the executor

- **No DB migration framework** — two additive `ADD COLUMN`s with safe defaults + the `CREATE TABLE` literal; idempotent `init`; an M20-shaped DB upgrades transparently (pinned by `init_adds_summary_columns_to_an_m20_shaped_table`).
- **No new OAuth scope, no new dependency, no Settings change.** Model (`llama3.2`) + endpoint (`localhost:11434`) are `const`s in `ollama.rs`.
- **Reviewers are READ-ONLY** — any code-review subagent prompt must forbid Edit/Write and any git change ("REPORT ONLY"); run `git status -s` after each review.
- **Deferred (not this milestone):** transcription input (M22 — feeds the same `summarize`), streaming tokens, configurable model/endpoint, markdown *rendering* of the summary, summary marker in browse-list rows, auto-summarize-on-save.
```
