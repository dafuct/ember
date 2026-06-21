# M19 Calendar Event Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Ember's read-only calendar (M10) writable — create/edit/delete events and create meetings with guests (email invites) + an auto Google Meet link.

**Architecture:** M10's `CalendarClient` gains `create_event`/`update_event`/`delete_event`; a new `EventWrite` input maps to Google's event JSON (create optionally adds `conferenceData.createRequest` + `attendees`, `sendUpdates=all`). `CalendarEvent` gains `description`/`meet_link`/`html_link`/`attendees`. Three DB-free commands + a `list_calendars` command for the form's calendar picker. Frontend adds an `EventModal` (create/edit form), an event-detail popover, and entry points; mutations refetch the week. One added OAuth scope (`calendar.events`); reconnect re-grants. No DB migration.

**Tech Stack:** Rust (reqwest, serde, Tauri 2; wiremock tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (every task):** the owner is learning Rust — all Rust gets concise `// 🦀` teaching comments on the *language* concept + a 2-3 sentence recap per task. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (hand-formatted repo).

**Frontend testing note:** this repo has **no TS/React test harness** (consistent through M18). Frontend tasks are verified by `npm run build` (`tsc` + `vite build`) + a final maket screenshot. Backend tasks use TDD with `wiremock`.

**Spec:** `docs/superpowers/specs/2026-06-20-ember-m19-calendar-event-management-design.md`

---

## File structure

**Backend (`src-tauri/`):**
- `src/calendar/types.rs` — *modify*: extend `GEvent`/`CalendarEvent`/`CalendarListEntry`; add `GAttendee`, `EventWrite`.
- `src/calendar/mod.rs` — *modify*: extend `map_event`; factor an auth-status helper; add `create_event`/`update_event`/`delete_event` + private request-body structs.
- `src/auth/mod.rs` — *modify*: add the `calendar.events` scope.
- `src/commands.rs` — *modify*: add `create_calendar_event`/`update_calendar_event`/`delete_calendar_event`/`list_calendars`.
- `src/lib.rs` — *modify*: register the 4 commands.
- `tests/calendar_test.rs` — *modify*: extend `map_event` test for new fields; add create/update/delete wiremock tests.

**Frontend (`src/`):**
- `src/lib/calendar.ts` — *modify*: extend `CalendarEvent` interface; add pure form helpers.
- `src/lib/api.ts` — *modify*: `EventWrite`/`CalendarSummary` types + 4 wrappers (gated).
- `src/lib/mock.ts` — *modify*: mock create/update/delete/list-calendars.
- `src/components/EventModal.tsx` — *create*: the create/edit form.
- `src/components/CalendarView.tsx` — *modify*: New-event button, calendars load, modal/popover state, refetch-on-mutation.
- `src/components/WeekGrid.tsx` — *modify*: slot-click + event-click callbacks + the detail popover.
- `src/styles/app.css` — *modify*: event-modal + popover styles.

---

## Task 1: Backend — extend read types + `map_event`

**Files:**
- Modify: `src-tauri/src/calendar/types.rs`
- Modify: `src-tauri/src/calendar/mod.rs`
- Test: `src-tauri/tests/calendar_test.rs`

- [ ] **Step 1: Write/extend the failing test**

In `src-tauri/tests/calendar_test.rs`, the existing `map_event_normalizes_and_skips_cancelled` test constructs `GEvent { ... }` literals. Replace that test with this version (adds the new fields to the literals AND asserts they map through):

```rust
#[test]
fn map_event_normalizes_and_skips_cancelled() {
    use ember_lib::calendar::map_event;
    use ember_lib::calendar::types::{GAttendee, GEvent, GEventDateTime};

    let timed = GEvent {
        id: "e1".into(),
        summary: Some("Standup".into()),
        start: Some(GEventDateTime { date_time: Some("2026-06-15T09:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T09:30:00-07:00".into()), date: None }),
        location: Some("Zoom".into()),
        status: Some("confirmed".into()),
        description: Some("daily sync".into()),
        html_link: Some("https://cal/e1".into()),
        hangout_link: Some("https://meet.google.com/abc".into()),
        attendees: Some(vec![
            GAttendee { email: "a@x.com".into(), response_status: Some("accepted".into()) },
            GAttendee { email: "b@y.com".into(), response_status: None },
        ]),
    };
    let m = map_event(timed, "primary", Some("#16a34a")).unwrap();
    assert_eq!(m.title, "Standup");
    assert_eq!(m.start, "2026-06-15T09:00:00-07:00");
    assert!(!m.all_day);
    assert_eq!(m.description.as_deref(), Some("daily sync"));
    assert_eq!(m.meet_link.as_deref(), Some("https://meet.google.com/abc"));
    assert_eq!(m.html_link.as_deref(), Some("https://cal/e1"));
    assert_eq!(m.attendees, vec!["a@x.com".to_string(), "b@y.com".to_string()]);

    let allday = GEvent {
        id: "e2".into(),
        summary: None,
        start: Some(GEventDateTime { date_time: None, date: Some("2026-06-16".into()) }),
        end: Some(GEventDateTime { date_time: None, date: Some("2026-06-17".into()) }),
        location: None,
        status: None,
        description: None,
        html_link: None,
        hangout_link: None,
        attendees: None,
    };
    let m2 = map_event(allday, "primary", None).unwrap();
    assert_eq!(m2.title, "(no title)");
    assert!(m2.all_day);
    assert!(m2.attendees.is_empty());

    let cancelled = GEvent {
        id: "e3".into(),
        summary: Some("Gone".into()),
        start: Some(GEventDateTime { date_time: Some("2026-06-15T10:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T10:30:00-07:00".into()), date: None }),
        location: None,
        status: Some("cancelled".into()),
        description: None,
        html_link: None,
        hangout_link: None,
        attendees: None,
    };
    assert!(map_event(cancelled, "primary", None).is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test calendar_test map_event_normalizes_and_skips_cancelled`
Expected: FAIL to compile — `GEvent` has no `description`/`html_link`/`hangout_link`/`attendees`; `GAttendee` doesn't exist; `CalendarEvent` has no `description`/`meet_link`/`html_link`/`attendees`.

- [ ] **Step 3: Extend the types**

In `src-tauri/src/calendar/types.rs`, replace the `GEvent` struct and add `GAttendee`:

```rust
#[derive(Debug, Deserialize)]
pub struct GEvent {
    pub id: String,
    pub summary: Option<String>,
    pub start: Option<GEventDateTime>,
    pub end: Option<GEventDateTime>,
    pub location: Option<String>,
    pub status: Option<String>,
    // 🦀 New read fields. `#[serde(default)]` → absent key becomes None/empty, never an error.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "htmlLink", default)]
    pub html_link: Option<String>,
    #[serde(rename = "hangoutLink", default)]
    pub hangout_link: Option<String>,
    #[serde(default)]
    pub attendees: Option<Vec<GAttendee>>,
}

// 🦀 One guest on an event. We surface only the email to the frontend (responseStatus parsed
//    for completeness / future RSVP UI).
#[derive(Debug, Deserialize)]
pub struct GAttendee {
    pub email: String,
    #[serde(rename = "responseStatus", default)]
    pub response_status: Option<String>,
}
```

Replace the `CalendarEvent` struct:

```rust
#[derive(Debug, Serialize, PartialEq)]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub location: Option<String>,
    pub color: Option<String>,
    // 🦀 New fields for the detail popover + meeting features.
    pub description: Option<String>,
    pub meet_link: Option<String>,
    pub html_link: Option<String>,
    pub attendees: Vec<String>,
}
```

Add `access_role` to `CalendarListEntry` (for the writable-calendar filter in Task 4):

```rust
    #[serde(rename = "accessRole", default)]
    pub access_role: Option<String>,
```

- [ ] **Step 4: Fill the new fields in `map_event`**

In `src-tauri/src/calendar/mod.rs`, update the `CalendarEvent { ... }` constructed by `map_event` to include the new fields (add before the closing `})`):

```rust
        description: ev.description,
        meet_link: ev.hangout_link,
        html_link: ev.html_link,
        // 🦀 `map(...).unwrap_or_default()` turns Option<Vec<GAttendee>> into a Vec<String> of
        //    emails (empty when no attendees). `into_iter()` consumes the Vec so we can move emails.
        attendees: ev
            .attendees
            .map(|a| a.into_iter().map(|g| g.email).collect())
            .unwrap_or_default(),
```

(Note: `map_event` takes `ev: GEvent` by value, so moving `ev.description`/`ev.hangout_link`/`ev.html_link`/`ev.attendees` out is fine. Place these lines so they don't conflict with the existing `location: ev.location` move — they're distinct fields.)

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --test calendar_test` → all calendar tests pass (the extended `map_event` test + the existing list/scope tests). The `list_events` test still passes (new GEvent fields default when absent).

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo clippy --all-targets 2>&1 | tail -3
cd .. && git add src-tauri/src/calendar/types.rs src-tauri/src/calendar/mod.rs src-tauri/tests/calendar_test.rs
git commit -m "feat(m19): extend calendar read types + map_event (description/meet/attendees)"
```

**Rust recap:** how `Option<Vec<T>>::map(...).unwrap_or_default()` collapses "absent list" and "transform each" into one expression yielding an empty `Vec` for the None case.

---

## Task 2: Backend — write types + `create_event` + OAuth scope

**Files:**
- Modify: `src-tauri/src/calendar/types.rs`
- Modify: `src-tauri/src/calendar/mod.rs`
- Modify: `src-tauri/src/auth/mod.rs`
- Test: `src-tauri/tests/calendar_test.rs`

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/tests/calendar_test.rs` (it imports `method, path, query_param`; add `body_partial_json` to the `wiremock::matchers` use list at the top of the file):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn create_event_with_meet_posts_conference_and_attendees() {
    use ember_lib::calendar::types::EventWrite;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .and(query_param("conferenceDataVersion", "1"))
        .and(query_param("sendUpdates", "all"))
        .and(body_partial_json(json!({
            "summary": "Sync",
            "attendees": [{ "email": "a@x.com" }],
            "conferenceData": { "createRequest": { "conferenceSolutionKey": { "type": "hangoutsMeet" } } }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "new1",
            "summary": "Sync",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end": { "dateTime": "2026-06-21T11:00:00-07:00" },
            "hangoutLink": "https://meet.google.com/xyz"
        })))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let ev = EventWrite {
        title: "Sync".into(),
        start: "2026-06-21T10:00:00-07:00".into(),
        end: "2026-06-21T11:00:00-07:00".into(),
        all_day: false,
        description: None,
        location: None,
        attendees: vec!["a@x.com".into()],
    };
    let created = client.create_event("primary", &ev, true).await.unwrap();
    assert_eq!(created.id, "new1");
    assert_eq!(created.meet_link.as_deref(), Some("https://meet.google.com/xyz"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test calendar_test create_event_with_meet`
Expected: FAIL to compile — `EventWrite` and `create_event` don't exist.

- [ ] **Step 3: Add `EventWrite` (types.rs)**

Append to `src-tauri/src/calendar/types.rs`:

```rust
// 🦀 The create/edit input from the frontend. `Deserialize` so a Tauri command can accept it;
//    snake_case fields → JS passes snake_case object keys. `start`/`end` are RFC3339 dateTime
//    (timed) or "YYYY-MM-DD" (all-day, end already exclusive — the frontend handles the +1).
#[derive(Debug, Deserialize)]
pub struct EventWrite {
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub attendees: Vec<String>,
}
```

- [ ] **Step 4: Add the write request structs + `create_event` (mod.rs)**

In `src-tauri/src/calendar/mod.rs`, add `EventWrite` to the `use types::{...}` import. Add these private Serialize structs (e.g. above `map_event`):

```rust
// 🦀 Serialize-only shapes for the Google event-write body. Lifetimes (`'a`) let the body
//    borrow strings from the EventWrite instead of cloning. `skip_serializing_if` omits absent
//    optional keys entirely (Google rejects some explicit nulls).
#[derive(serde::Serialize)]
struct EventDateTimeBody<'a> {
    #[serde(rename = "dateTime", skip_serializing_if = "Option::is_none")]
    date_time: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<&'a str>,
}
#[derive(serde::Serialize)]
struct AttendeeBody<'a> {
    email: &'a str,
}
#[derive(serde::Serialize)]
struct ConferenceSolutionKey {
    #[serde(rename = "type")]
    type_: &'static str,
}
#[derive(serde::Serialize)]
struct CreateConferenceRequest {
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "conferenceSolutionKey")]
    conference_solution_key: ConferenceSolutionKey,
}
#[derive(serde::Serialize)]
struct ConferenceDataBody {
    #[serde(rename = "createRequest")]
    create_request: CreateConferenceRequest,
}
#[derive(serde::Serialize)]
struct EventBody<'a> {
    summary: &'a str,
    start: EventDateTimeBody<'a>,
    end: EventDateTimeBody<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<&'a str>,
    attendees: Vec<AttendeeBody<'a>>,
    #[serde(rename = "conferenceData", skip_serializing_if = "Option::is_none")]
    conference_data: Option<ConferenceDataBody>,
}

// 🦀 Build the start/end blocks from the EventWrite: all-day → `{date}`, timed → `{dateTime}`.
fn date_time_body(value: &str, all_day: bool) -> EventDateTimeBody<'_> {
    if all_day {
        EventDateTimeBody { date_time: None, date: Some(value) }
    } else {
        EventDateTimeBody { date_time: Some(value), date: None }
    }
}

// 🦀 The shared event body (everything except conferenceData, which only `create` adds).
fn event_body(ev: &types::EventWrite) -> EventBody<'_> {
    EventBody {
        summary: &ev.title,
        start: date_time_body(&ev.start, ev.all_day),
        end: date_time_body(&ev.end, ev.all_day),
        description: ev.description.as_deref(),
        location: ev.location.as_deref(),
        attendees: ev.attendees.iter().map(|e| AttendeeBody { email: e }).collect(),
        conference_data: None,
    }
}
```

Factor an auth-status helper out of `get_json` so writes share the 401/403→reconnect mapping. Replace the body of `get_json` to delegate, and add `check_auth_status`:

```rust
    // 🦀 Shared 401/403 handling (extracted from get_json so writes reuse it): map a missing-scope
    //    403 to AppError::Auth (reconnect helps) and any other 403 to the Google message.
    async fn check_auth_status(&self, resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            let body = resp.text().await.unwrap_or_default();
            let msg = google_error_message(&body);
            let lower = msg.to_lowercase();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || lower.contains("scope")
                || lower.contains("insufficient")
                || lower.contains("credential")
            {
                return Err(AppError::Auth(
                    "Calendar access not granted — reconnect Google to enable it.".into(),
                ));
            }
            return Err(AppError::Other(format!("Google Calendar API error: {msg}")));
        }
        Ok(resp.error_for_status()?)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.http.get(url).bearer_auth(&self.access_token).send().await?;
        let resp = self.check_auth_status(resp).await?;
        Ok(resp.json::<T>().await?)
    }
```

Add the `create_event` method to the `impl CalendarClient` block:

```rust
    /// Create an event (optionally a meeting with a Google Meet link). `sendUpdates=all` emails
    /// the guests. Returns the created event, normalized.
    pub async fn create_event(
        &self,
        calendar_id: &str,
        ev: &types::EventWrite,
        add_meet: bool,
    ) -> Result<CalendarEvent> {
        let url = format!(
            "{}/calendar/v3/calendars/{}/events?conferenceDataVersion=1&sendUpdates=all",
            self.base_url, calendar_id
        );
        let mut body = event_body(ev);
        if add_meet {
            // 🦀 A unique requestId per create (mirrors M17's boundary) so retries don't collide.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            body.conference_data = Some(ConferenceDataBody {
                create_request: CreateConferenceRequest {
                    request_id: format!("ember-meet-{nanos}"),
                    conference_solution_key: ConferenceSolutionKey { type_: "hangoutsMeet" },
                },
            });
        }
        let resp = self.http.post(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        // 🦀 Created events come back without the owning calendar's color; the next week refetch
        //    re-colors them. `ok_or_else` turns the None (e.g. a cancelled echo) into an error.
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }
```

- [ ] **Step 5: Add the OAuth scope (auth/mod.rs)**

In `src-tauri/src/auth/mod.rs`, add the constant next to the existing scope consts:

```rust
const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";
```

In `connect()`, add a third `.add_scope(...)` after the calendar-readonly one:

```rust
            .add_scope(Scope::new(SCOPE_CALENDAR_READONLY.into()))
            .add_scope(Scope::new(SCOPE_CALENDAR_EVENTS.into()))
```

- [ ] **Step 6: Run the test + full suite**

Run: `cd src-tauri && cargo test --test calendar_test create_event_with_meet` → PASS.
Then `cargo test` (all) → green (the get_json refactor keeps the existing calendar tests passing). `cargo clippy --all-targets 2>&1 | tail -3` → clean.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/calendar/types.rs src-tauri/src/calendar/mod.rs src-tauri/src/auth/mod.rs src-tauri/tests/calendar_test.rs
git commit -m "feat(m19): CalendarClient::create_event (+conferenceData/Meet) + calendar.events scope"
```

**Rust recap:** how lifetimes on the `EventBody<'a>` serialize structs let the request body borrow strings from `EventWrite` (zero clones), and how `skip_serializing_if = "Option::is_none"` omits absent keys so Google never sees an explicit null.

---

## Task 3: Backend — `update_event` + `delete_event`

**Files:**
- Modify: `src-tauri/src/calendar/mod.rs`
- Test: `src-tauri/tests/calendar_test.rs`

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/tests/calendar_test.rs`:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn update_event_patches_without_conference_data() {
    use ember_lib::calendar::types::EventWrite;
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .and(query_param("sendUpdates", "all"))
        .and(body_partial_json(json!({ "summary": "Renamed" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "e9",
            "summary": "Renamed",
            "start": { "dateTime": "2026-06-21T10:00:00-07:00" },
            "end": { "dateTime": "2026-06-21T11:00:00-07:00" }
        })))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let ev = EventWrite {
        title: "Renamed".into(),
        start: "2026-06-21T10:00:00-07:00".into(),
        end: "2026-06-21T11:00:00-07:00".into(),
        all_day: false,
        description: None,
        location: None,
        attendees: vec![],
    };
    let updated = client.update_event("primary", "e9", &ev).await.unwrap();
    assert_eq!(updated.title, "Renamed");
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_event_issues_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/calendar/v3/calendars/primary/events/e9"))
        .and(query_param("sendUpdates", "all"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    client.delete_event("primary", "e9").await.unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --test calendar_test update_event delete_event`
Expected: FAIL to compile — `update_event`/`delete_event` don't exist.

- [ ] **Step 3: Add the methods**

In `src-tauri/src/calendar/mod.rs`, add to the `impl CalendarClient` block:

```rust
    /// Edit an existing event (PATCH — partial update). Sending the body fields replaces them;
    /// omitting `conferenceData` PRESERVES any existing Meet link. `sendUpdates=all` notifies guests.
    pub async fn update_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        ev: &types::EventWrite,
    ) -> Result<CalendarEvent> {
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, calendar_id, event_id
        );
        let body = event_body(ev); // conference_data stays None → existing Meet link preserved
        let resp = self.http.patch(&url).bearer_auth(&self.access_token).json(&body).send().await?;
        let resp = self.check_auth_status(resp).await?;
        let g: GEvent = resp.json().await?;
        map_event(g, calendar_id, None)
            .ok_or_else(|| AppError::Other("calendar returned an unusable event".into()))
    }

    /// Delete an event. `sendUpdates=all` sends guests the cancellation.
    pub async fn delete_event(&self, calendar_id: &str, event_id: &str) -> Result<()> {
        let url = format!(
            "{}/calendar/v3/calendars/{}/events/{}?sendUpdates=all",
            self.base_url, calendar_id, event_id
        );
        let resp = self.http.delete(&url).bearer_auth(&self.access_token).send().await?;
        // 🦀 We don't need a body back; `check_auth_status` still maps 401/403 + `error_for_status`.
        self.check_auth_status(resp).await?;
        Ok(())
    }
```

- [ ] **Step 4: Run the tests + suite**

Run: `cd src-tauri && cargo test --test calendar_test` → all pass.
Run: `cargo clippy --all-targets 2>&1 | tail -3` → clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/calendar/mod.rs src-tauri/tests/calendar_test.rs
git commit -m "feat(m19): CalendarClient::update_event (PATCH) + delete_event"
```

**Rust recap:** why PATCH with a body that omits `conferenceData` preserves the Meet link (partial update only touches the keys you send), and how `check_auth_status` is reused on a no-body DELETE.

---

## Task 4: Backend — commands + `list_calendars`

**Files:**
- Modify: `src-tauri/src/calendar/types.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

(Command glue over the TDD'd client methods; verified by `cargo build`.)

- [ ] **Step 1: Add a `CalendarSummary` Serialize type (types.rs)**

Append to `src-tauri/src/calendar/types.rs`:

```rust
/// A calendar the user can write to (for the create-event picker).
#[derive(Debug, Serialize)]
pub struct CalendarSummary {
    pub id: String,
    pub summary: String,
    pub primary: bool,
    pub writable: bool,
}
```

- [ ] **Step 2: Add the commands (commands.rs)**

In `src-tauri/src/commands.rs`, add (near `fetch_calendar_week`; `EventWrite`/`CalendarEvent`/`CalendarSummary`/`CalendarClient` come from `crate::calendar`):

```rust
/// List the user's calendars (for the create-event calendar picker). DB-free.
#[tauri::command]
pub async fn list_calendars() -> Result<Vec<crate::calendar::types::CalendarSummary>> {
    let stored = ensure_access_token().await?;
    let client = crate::calendar::CalendarClient::new(stored.access_token);
    let entries = client.list_calendars().await?;
    // 🦀 Map the raw list to the frontend shape; writable = an owner/writer access role.
    Ok(entries
        .into_iter()
        .map(|c| crate::calendar::types::CalendarSummary {
            id: c.id,
            summary: c.summary.unwrap_or_else(|| "(unnamed)".to_string()),
            primary: c.primary.unwrap_or(false),
            writable: matches!(c.access_role.as_deref(), Some("owner") | Some("writer")),
        })
        .collect())
}

/// Create a calendar event (optionally a Meet meeting). DB-free.
#[tauri::command]
pub async fn create_calendar_event(
    calendar_id: String,
    event: crate::calendar::types::EventWrite,
    add_meet: bool,
) -> Result<crate::calendar::types::CalendarEvent> {
    let stored = ensure_access_token().await?;
    let client = crate::calendar::CalendarClient::new(stored.access_token);
    client.create_event(&calendar_id, &event, add_meet).await
}

/// Edit a calendar event. DB-free.
#[tauri::command]
pub async fn update_calendar_event(
    calendar_id: String,
    event_id: String,
    event: crate::calendar::types::EventWrite,
) -> Result<crate::calendar::types::CalendarEvent> {
    let stored = ensure_access_token().await?;
    let client = crate::calendar::CalendarClient::new(stored.access_token);
    client.update_event(&calendar_id, &event_id, &event).await
}

/// Delete a calendar event. DB-free.
#[tauri::command]
pub async fn delete_calendar_event(calendar_id: String, event_id: String) -> Result<()> {
    let stored = ensure_access_token().await?;
    let client = crate::calendar::CalendarClient::new(stored.access_token);
    client.delete_event(&calendar_id, &event_id).await
}
```

(If `commands.rs` doesn't already import `CalendarClient`, use the fully-qualified `crate::calendar::CalendarClient` as above — no new `use` needed.)

- [ ] **Step 3: Register the commands (lib.rs)**

In `src-tauri/src/lib.rs`, add to the `generate_handler![ ... ]` list (after `commands::fetch_calendar_week,`):

```rust
            commands::list_calendars,
            commands::create_calendar_event,
            commands::update_calendar_event,
            commands::delete_calendar_event,
```

- [ ] **Step 4: Build to verify**

Run: `cd src-tauri && cargo build 2>&1 | tail -6`
Expected: compiles clean — no "never used" warnings (the 4 commands are registered). Then `cargo clippy --all-targets 2>&1 | tail -3` + `cargo test 2>&1 | tail -5` — clean/green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/calendar/types.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(m19): calendar event commands (create/update/delete) + list_calendars"
```

**Rust recap:** how `matches!(c.access_role.as_deref(), Some("owner") | Some("writer"))` is a compact boolean test over an `Option<&str>` using an or-pattern.

---

## Task 5: Frontend — api wrappers, helpers, mocks

**Files:**
- Modify: `src/lib/calendar.ts`
- Modify: `src/lib/api.ts`
- Modify: `src/lib/mock.ts`

- [ ] **Step 1: Extend the `CalendarEvent` interface + add form helpers (calendar.ts)**

In `src/lib/calendar.ts`, extend the `CalendarEvent` interface (add the new OPTIONAL fields so existing `CalendarEvent` literals — e.g. `mockCalendarWeek` — don't break):

```ts
export interface CalendarEvent {
  id: string;
  calendar_id: string;
  title: string;
  start: string;
  end: string;
  all_day: boolean;
  location: string | null;
  color: string | null;
  description?: string | null;
  meet_link?: string | null;
  html_link?: string | null;
  attendees?: string[];
}
```

Append these pure helpers (reuse the existing `pad` + `toYmd`):

```ts
/** RFC3339 local-time string from a "YYYY-MM-DD" date + "HH:MM" time (with the local offset). */
export function rfc3339Local(ymd: string, hhmm: string): string {
  const [y, mo, d] = ymd.split("-").map(Number);
  const [h, mi] = hhmm.split(":").map(Number);
  const dt = new Date(y, (mo || 1) - 1, d || 1, h || 0, mi || 0, 0);
  const off = -dt.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "-";
  const oh = pad(Math.floor(Math.abs(off) / 60));
  const om = pad(Math.abs(off) % 60);
  return (
    `${dt.getFullYear()}-${pad(dt.getMonth() + 1)}-${pad(dt.getDate())}` +
    `T${pad(dt.getHours())}:${pad(dt.getMinutes())}:00${sign}${oh}:${om}`
  );
}

/** Google all-day end is EXCLUSIVE: a YYYY-MM-DD one day after the user-picked end date. */
export function allDayEndExclusive(ymd: string): string {
  const [y, mo, d] = ymd.split("-").map(Number);
  const dt = new Date(y, (mo || 1) - 1, (d || 1) + 1);
  return toYmd(dt);
}
```

- [ ] **Step 2: Add api types + wrappers (api.ts)**

In `src/lib/api.ts`, add the `mock*` imports to the existing `from "./mock"` line (`mockCreateEvent, mockUpdateEvent, mockDeleteEvent, mockListCalendars`), then add:

```ts
export interface EventWrite {
  title: string;
  start: string;
  end: string;
  all_day: boolean;
  description: string | null;
  location: string | null;
  attendees: string[];
}

export interface CalendarSummary {
  id: string;
  summary: string;
  primary: boolean;
  writable: boolean;
}

export const listCalendars = (): Promise<CalendarSummary[]> =>
  isTauri() ? invoke<CalendarSummary[]>("list_calendars") : Promise.resolve(mockListCalendars());

export const createCalendarEvent = (
  calendarId: string,
  event: EventWrite,
  addMeet: boolean,
): Promise<CalendarEvent> =>
  isTauri()
    ? invoke<CalendarEvent>("create_calendar_event", { calendarId, event, addMeet })
    : Promise.resolve(mockCreateEvent(calendarId, event, addMeet));

export const updateCalendarEvent = (
  calendarId: string,
  eventId: string,
  event: EventWrite,
): Promise<CalendarEvent> =>
  isTauri()
    ? invoke<CalendarEvent>("update_calendar_event", { calendarId, eventId, event })
    : Promise.resolve(mockUpdateEvent(calendarId, eventId, event));

export const deleteCalendarEvent = (calendarId: string, eventId: string): Promise<void> =>
  isTauri() ? invoke<void>("delete_calendar_event", { calendarId, eventId }) : Promise.resolve();
```

- [ ] **Step 3: Add mocks (mock.ts)**

In `src/lib/mock.ts`, add `EventWrite, CalendarSummary` to the type import from `./api`, and append:

```ts
/** Browser-maket: echo a created event (fake id, a mock Meet link when requested). */
export function mockCreateEvent(calendarId: string, ev: EventWrite, addMeet: boolean): CalendarEvent {
  return {
    id: `mock-${ev.title.replace(/\s+/g, "-")}`,
    calendar_id: calendarId,
    title: ev.title,
    start: ev.start,
    end: ev.end,
    all_day: ev.all_day,
    location: ev.location,
    color: "#16a34a",
    description: ev.description,
    meet_link: addMeet ? "https://meet.google.com/mock-abc" : null,
    html_link: null,
    attendees: ev.attendees,
  };
}
export function mockUpdateEvent(calendarId: string, eventId: string, ev: EventWrite): CalendarEvent {
  return { ...mockCreateEvent(calendarId, ev, false), id: eventId };
}
export function mockListCalendars(): CalendarSummary[] {
  return [
    { id: "primary", summary: "you@example.com", primary: true, writable: true },
    { id: "personal@group", summary: "Personal", primary: false, writable: true },
  ];
}
```

(`mockCalendarWeek`'s existing `CalendarEvent` literals are unaffected — the new interface fields are optional. `CalendarEvent` is imported via `./api` already in mock.ts; if not, it's `import type { ..., CalendarEvent }` — it's re-exported by api.ts.)

- [ ] **Step 4: Build to verify**

Run: `npm run build 2>&1 | tail -8`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 5: Commit**

```bash
git add src/lib/calendar.ts src/lib/api.ts src/lib/mock.ts
git commit -m "feat(m19): calendar event api wrappers + form helpers + mocks"
```

---

## Task 6: Frontend — the `EventModal` create/edit form

**Files:**
- Create: `src/components/EventModal.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Create `EventModal.tsx`**

Create `src/components/EventModal.tsx`:

```tsx
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  createCalendarEvent,
  updateCalendarEvent,
  deleteCalendarEvent,
  type EventWrite,
  type CalendarSummary,
  type CalendarEvent,
} from "../lib/api";
import { rfc3339Local, allDayEndExclusive } from "../lib/calendar";
import { parseRecipients, isPlausibleEmail } from "../lib/compose";

// Seed values for the form. For a new event the caller passes a start Date (e.g. a clicked slot);
// for an edit it passes the existing CalendarEvent.
export interface EventInitial {
  calendars: CalendarSummary[];
  event?: CalendarEvent; // present → edit mode
  startAt?: Date; // present → new-event default start (end = +1h)
}

const fmtDate = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const fmtTime = (d: Date) =>
  `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

export function EventModal({
  initial,
  onClose,
  onSaved,
}: {
  initial: EventInitial;
  onClose: () => void;
  onSaved: () => void; // refetch the week
}) {
  const editing = initial.event;
  const seedStart = editing ? new Date(editing.start) : (initial.startAt ?? new Date());
  const seedEnd = editing ? new Date(editing.end) : new Date(seedStart.getTime() + 60 * 60 * 1000);
  const writableCals = initial.calendars.filter((c) => c.writable);

  const [title, setTitle] = useState(editing?.title ?? "");
  const [allDay, setAllDay] = useState(editing?.all_day ?? false);
  const [date, setDate] = useState(fmtDate(seedStart));
  const [endDate, setEndDate] = useState(fmtDate(seedEnd));
  const [startTime, setStartTime] = useState(fmtTime(seedStart));
  const [endTime, setEndTime] = useState(fmtTime(seedEnd));
  const [location, setLocation] = useState(editing?.location ?? "");
  const [description, setDescription] = useState(editing?.description ?? "");
  const [guests, setGuests] = useState((editing?.attendees ?? []).join(", "));
  const [calendarId, setCalendarId] = useState(
    editing?.calendar_id ?? writableCals.find((c) => c.primary)?.id ?? writableCals[0]?.id ?? "primary",
  );
  const [addMeet, setAddMeet] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function buildWrite(): EventWrite | string {
    if (title.trim() === "") return "A title is required.";
    const emails = parseRecipients(guests);
    if (emails.length > 0 && !emails.every(isPlausibleEmail)) return "One of the guest emails looks invalid.";
    let start: string;
    let end: string;
    if (allDay) {
      if (endDate < date) return "End date is before the start date.";
      start = date;
      end = allDayEndExclusive(endDate);
    } else {
      start = rfc3339Local(date, startTime);
      end = rfc3339Local(endDate, endTime);
      if (new Date(end).getTime() <= new Date(start).getTime()) return "End must be after start.";
    }
    return {
      title: title.trim(),
      start,
      end,
      all_day: allDay,
      description: description.trim() || null,
      location: location.trim() || null,
      attendees: emails,
    };
  }

  async function handleSave() {
    const w = buildWrite();
    if (typeof w === "string") {
      setError(w);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (editing) await updateCalendarEvent(calendarId, editing.id, w);
      else await createCalendarEvent(calendarId, w, addMeet);
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (!editing) return;
    setBusy(true);
    setError(null);
    try {
      await deleteCalendarEvent(editing.calendar_id, editing.id);
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="compose-overlay">
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="event-title">
        <div className="compose-head">
          <span className="compose-title" id="event-title">{editing ? "Edit event" : "New event"}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}><X size={16} /></button>
        </div>
        <input className="compose-field" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus />
        <label className="event-row">
          <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} /> All day
        </label>
        <div className="event-row">
          <input type="date" className="compose-field" value={date} onChange={(e) => setDate(e.target.value)} />
          {!allDay && <input type="time" className="compose-field" value={startTime} onChange={(e) => setStartTime(e.target.value)} />}
        </div>
        <div className="event-row">
          <input type="date" className="compose-field" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
          {!allDay && <input type="time" className="compose-field" value={endTime} onChange={(e) => setEndTime(e.target.value)} />}
        </div>
        <input className="compose-field" placeholder="Location" value={location} onChange={(e) => setLocation(e.target.value)} />
        <input className="compose-field" placeholder="Guests (comma-separated emails)" value={guests} onChange={(e) => setGuests(e.target.value)} />
        <textarea className="compose-body" placeholder="Description" value={description} onChange={(e) => setDescription(e.target.value)} rows={4} />
        <div className="event-row">
          <select className="compose-field" value={calendarId} onChange={(e) => setCalendarId(e.target.value)}>
            {writableCals.map((c) => (
              <option key={c.id} value={c.id}>{c.summary}{c.primary ? " (primary)" : ""}</option>
            ))}
          </select>
        </div>
        {editing ? (
          editing.meet_link ? <a className="event-meet" href={editing.meet_link} target="_blank" rel="noreferrer">{editing.meet_link}</a> : null
        ) : (
          <label className="event-row">
            <input type="checkbox" checked={addMeet} onChange={(e) => setAddMeet(e.target.checked)} /> Add Google Meet
          </label>
        )}
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {editing && (
            <button className="btn btn-danger-outline" onClick={handleDelete} disabled={busy}>Delete</button>
          )}
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn btn-accent" onClick={handleSave} disabled={busy}>{busy ? "Saving…" : "Save"}</button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Styles**

Append to `src/styles/app.css` (reuse the compose-modal tokens; `--border`/`--text-muted`/`--accent` exist):

```css
.event-row { display: flex; gap: 8px; align-items: center; padding: 2px 0; }
.event-row .compose-field { flex: 1; }
.event-row input[type="checkbox"] { width: auto; }
.event-meet { font-size: 12px; color: var(--accent); word-break: break-all; padding: 2px 0; }
```

- [ ] **Step 3: Build to verify**

Run: `npm run build 2>&1 | tail -8`
Expected: `tsc` + `vite build` clean. (`EventModal` isn't rendered yet — Task 7 wires it — but it must typecheck.)

- [ ] **Step 4: Commit**

```bash
git add src/components/EventModal.tsx src/styles/app.css
git commit -m "feat(m19): EventModal create/edit form (guests + Meet toggle + calendar picker)"
```

---

## Task 7: Frontend — CalendarView/WeekGrid wiring + detail popover

**Files:**
- Modify: `src/components/CalendarView.tsx`
- Modify: `src/components/WeekGrid.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: WeekGrid — slot + event click callbacks**

In `src/components/WeekGrid.tsx`, add two optional props and wire them. Change the component signature/props type to include:

```tsx
  onSlotClick,
  onEventClick,
}: {
  weekStart: Date;
  events: CalendarEvent[];
  now: Date;
  onSlotClick?: (at: Date) => void;
  onEventClick?: (ev: CalendarEvent) => void;
}) {
```

Make each timed event clickable — change the event `<div>` (the `className="cal-ev"` one) to call `onEventClick`:

```tsx
                  <div
                    key={p.ev.id}
                    className="cal-ev"
                    onClick={(e) => { e.stopPropagation(); onEventClick?.(p.ev); }}
                    title={`${p.ev.title} · ${fmtTime(p.ev.start)}`}
                    style={{ /* unchanged */
                      top: p.topMin * PX_PER_MIN,
                      height: Math.max(14, p.heightMin * PX_PER_MIN - 2),
                      left: `calc(${(p.lane / p.lanes) * 100}% + 2px)`,
                      width: `calc(${100 / p.lanes}% - 4px)`,
                      ...tint(p.ev),
                    }}
                  >
```

Make each day column create-on-click. On the `<div className="cal-col">`, add an `onClick` that maps the click's Y offset to an hour and calls `onSlotClick` with that day+hour:

```tsx
              <div
                key={d.toISOString()}
                className="cal-col"
                onClick={(e) => {
                  if (!onSlotClick) return;
                  const rect = e.currentTarget.getBoundingClientRect();
                  const min = Math.max(0, Math.round((e.clientY - rect.top) / PX_PER_MIN));
                  const at = new Date(d.getFullYear(), d.getMonth(), d.getDate(), Math.floor(min / 60), 0, 0);
                  onSlotClick(at);
                }}
              >
```

(The event `stopPropagation` prevents a column-click from also firing when an event is clicked.)

Also make all-day events clickable (the `className="cal-allday-ev"` div) the same way:

```tsx
                <div key={e.id} className="cal-allday-ev" style={tint(e)} title={e.title}
                  onClick={() => onEventClick?.(e)}>
                  {e.title}
                </div>
```

- [ ] **Step 2: CalendarView — toolbar, calendars, modal + popover**

Replace `src/components/CalendarView.tsx` with this version (adds: a New-event button, a calendars load, an `EventModal` + a detail popover, and refetch-on-save):

```tsx
import { useEffect, useState } from "react";
import { type CalendarEvent, toTimeMinMax } from "../lib/calendar";
import { fetchCalendarWeek, connectGmail, listCalendars, type CalendarSummary } from "../lib/api";
import { WeekGrid } from "./WeekGrid";
import { EventModal, type EventInitial } from "./EventModal";

function isScopeError(msg: string): boolean {
  return /reconnect google|calendar access not granted/i.test(msg);
}

export function CalendarView({ weekStart }: { weekStart: Date }) {
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState<Date>(() => new Date());
  const [reloadKey, setReloadKey] = useState(0);
  const [calendars, setCalendars] = useState<CalendarSummary[]>([]);
  const [modal, setModal] = useState<EventInitial | null>(null);
  const [detail, setDetail] = useState<CalendarEvent | null>(null);

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(id);
  }, []);

  // Load writable calendars once (for the create form's picker). Silent on failure.
  useEffect(() => {
    listCalendars().then(setCalendars).catch(() => setCalendars([]));
  }, [reloadKey]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const { timeMin, timeMax } = toTimeMinMax(weekStart);
    fetchCalendarWeek(timeMin, timeMax)
      .then((evts) => { if (!cancelled) { setEvents(evts); setLoading(false); } })
      .catch((e) => { if (!cancelled) { setError(String(e)); setLoading(false); } });
    return () => { cancelled = true; };
  }, [weekStart, reloadKey]);

  async function handleReconnect() {
    setError(null);
    setLoading(true);
    try {
      await connectGmail();
      setReloadKey((k) => k + 1);
    } catch (e) {
      setError(String(e));
      setLoading(false);
    }
  }

  const refetch = () => setReloadKey((k) => k + 1);
  const openNew = (startAt?: Date) => setModal({ calendars, startAt });
  const openEdit = (ev: CalendarEvent) => { setDetail(null); setModal({ calendars, event: ev }); };

  if (error && isScopeError(error)) {
    return (
      <div className="cal-view">
        <div className="cal-empty">
          <p>Ember needs permission to manage your Google Calendar.</p>
          <button className="btn btn-accent" onClick={handleReconnect}>Reconnect Google</button>
        </div>
      </div>
    );
  }
  if (error) {
    return (
      <div className="cal-view">
        <div className="cal-empty">
          <pre className="error-text">{error}</pre>
          <button className="btn" onClick={refetch}>Retry</button>
        </div>
      </div>
    );
  }

  return (
    <div className="cal-view">
      <div className="cal-toolbar">
        <button className="btn btn-accent" onClick={() => openNew()}>New event</button>
      </div>
      {loading ? (
        <div className="cal-loading">Loading your week…</div>
      ) : (
        <WeekGrid weekStart={weekStart} events={events} now={now} onSlotClick={openNew} onEventClick={setDetail} />
      )}

      {detail && (
        <div className="event-detail-overlay" onClick={() => setDetail(null)}>
          <div className="event-detail" role="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>{detail.title}</h3>
            <div className="event-detail-when">
              {new Date(detail.start).toLocaleString()} – {new Date(detail.end).toLocaleString()}
            </div>
            {detail.location && <div>{detail.location}</div>}
            {detail.description && <p className="event-detail-desc">{detail.description}</p>}
            {detail.attendees && detail.attendees.length > 0 && (
              <div className="event-detail-guests">Guests: {detail.attendees.join(", ")}</div>
            )}
            {detail.meet_link && (
              <a className="event-meet" href={detail.meet_link} target="_blank" rel="noreferrer">{detail.meet_link}</a>
            )}
            <div className="compose-actions">
              <button className="btn" onClick={() => setDetail(null)}>Close</button>
              <button className="btn btn-accent" onClick={() => openEdit(detail)}>Edit</button>
            </div>
          </div>
        </div>
      )}

      {modal && (
        <EventModal initial={modal} onClose={() => setModal(null)} onSaved={refetch} />
      )}
    </div>
  );
}
```

- [ ] **Step 3: Styles**

Append to `src/styles/app.css`:

```css
.cal-toolbar { display: flex; justify-content: flex-end; padding: 6px 12px; }
.event-detail-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.25); display: flex; align-items: center; justify-content: center; z-index: 50; }
.event-detail { background: var(--surface); border: 1px solid var(--border); border-radius: 10px; padding: 16px; min-width: 320px; max-width: 460px; }
.event-detail h3 { margin: 0 0 6px; }
.event-detail-when { color: var(--text-muted); font-size: 13px; margin-bottom: 8px; }
.event-detail-desc { white-space: pre-wrap; }
.event-detail-guests { font-size: 13px; color: var(--text-muted); margin: 6px 0; }
```

- [ ] **Step 4: Build to verify**

Run: `npm run build 2>&1 | tail -8`
Expected: `tsc` + `vite build` clean.

- [ ] **Step 5: Commit**

```bash
git add src/components/CalendarView.tsx src/components/WeekGrid.tsx src/styles/app.css
git commit -m "feat(m19): calendar New-event button + slot/event clicks + detail popover"
```

---

## Task 8: Full verification + maket screenshots

**Files:** none (verification only)

- [ ] **Step 1: Backend suite + lint**

Run: `cd src-tauri && cargo test 2>&1 | tail -8` → all pass (the prior count + new calendar wiremock tests: create/update/delete + the extended map_event).
Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -4` → clean.

- [ ] **Step 2: Frontend build**

Run: `cd .. && npm run build 2>&1 | tail -5` → `tsc` + `vite build` clean.

- [ ] **Step 3: Maket verification**

Run `npm run dev`, open the local URL, switch to **Calendar**, and verify + screenshot:
1. **New event** button → the `EventModal` opens; fill a title, toggle **Add Google Meet**, add a guest email, pick a calendar, **Save** → the modal closes and a new event appears on the grid (the mock echoes it).
2. Click an **empty time slot** → the modal opens pre-filled at that day/hour.
3. Click the **created event** → the **detail popover** shows time/guests + a Meet link; **Edit** opens the form prefilled; **Delete** removes it.

- [ ] **Step 4: Confirm clean tree**

Run: `git status -s`
Expected: empty (all committed). If a reviewer left changes, discard them.

- [ ] **Step 5: Final summary**

Report: test count delta, clippy/build status, maket screenshots, and the **owner-pending live Google E2E** (a real create → guest invite email → Meet link → edit → delete, after reconnecting Google for the new `calendar.events` scope). The wiki `[[ember]]` + `wiki/log.md` update happens at merge time (controller).

---

## Self-review notes (for the controller)

- **Spec coverage:** read types + map_event (T1) · write types + create_event + scope (T2) · update/delete_event (T3) · commands + list_calendars (T4) · api/helpers/mocks (T5) · EventModal form (T6) · grid wiring + detail popover (T7) · verify (T8). Deferred items (recurring creation, Meet-on-edit, color/reminders, drag, timezone picker) intentionally absent.
- **Type consistency:** Rust `EventWrite {title, start, end, all_day, description, location, attendees}` (Deserialize, snake_case) ↔ TS `EventWrite` ↔ the command arg `event`; `CalendarEvent` gains `description/meet_link/html_link/attendees` in Rust (Serialize) ↔ TS interface (optional). `CalendarSummary {id, summary, primary, writable}` Rust↔TS. Invoke args: `create_calendar_event({calendarId, event, addMeet})` ↔ Rust `(calendar_id, event, add_meet)`; `update_calendar_event({calendarId, eventId, event})`; `delete_calendar_event({calendarId, eventId})`; `list_calendars()`.
- **Cross-task green:** the `CalendarEvent` interface gains only OPTIONAL fields → existing `mockCalendarWeek` literals don't break. The 4 new commands are registered in the same task they're written (T4). `EventModal` typechecks in T6 before T7 renders it.
- **One added OAuth scope (`calendar.events`), no DB migration, no new dependency.**
- **All-day exclusive-end `+1` lives only in `allDayEndExclusive` (frontend)** — the backend passes `EventWrite.start`/`.end` through verbatim.
- **Known v1 rough edge (acceptable):** editing an *all-day* event's end-date prefill (`new Date(editing.end)` where `end` is the exclusive `"YYYY-MM-DD"`) is timezone-dependent and may show off-by-one in positive-UTC-offset zones; it is correct in the owner's negative-offset zone. Creating events and editing *timed* events are unaffected. A local-parts date parse is the follow-up fix if it bites.
