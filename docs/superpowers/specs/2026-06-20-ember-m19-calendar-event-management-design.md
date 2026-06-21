# Ember — Milestone 19: Calendar event management (lean v1) — Design Spec

**Status:** Approved design (2026-06-20). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Turn Ember's read-only calendar (M10) into a writable one: **create, edit, and delete** events, and create real **meetings** with **guests** (email invitations) and an auto **Google Meet** link. First step of a larger "calendar & meeting notes" feature area (M19 calendar write → later: meeting notes storage → local Ollama summarization → meeting transcription capture). **No DB migration** (calendar stays live-fetched). **One added OAuth scope** (`calendar.events`); a reconnect re-grants it (the M10 pattern).

**Architecture in one paragraph:** M10's `CalendarClient` (read-only `list_calendars`/`list_events`) gains `create_event`/`update_event`/`delete_event` against the Google Calendar API. A new `EventWrite` input (title, start/end, all-day, location, description, attendee emails) maps to Google's event JSON; **create** optionally attaches a Meet link via `conferenceData.createRequest` (`conferenceDataVersion=1`, a `SystemTime`-derived unique `requestId`) and invites guests via `attendees` + `sendUpdates=all`. The read shape `CalendarEvent` gains `description`/`meet_link`/`html_link`/`attendees`. Three DB-free commands sit beside `fetch_calendar_week`; 401/403 still maps to the existing "reconnect" `AppError::Auth`. The frontend adds an `EventModal` (create/edit form), an event-detail popover (Edit/Delete + Meet link/guests), and entry points (a header "New event" button + click-empty-slot-to-create + click-event-to-open); every mutation refetches the visible week.

**Tech Stack:** Rust (reqwest, serde, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite, lucide-react.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each task, give a short plain-English Rust recap. TypeScript/React gets normal comments. **Do NOT run `cargo fmt`** (this repo hand-formats).

**Process note (reviewers):** reviewers are READ-ONLY — their prompts MUST forbid Edit/Write and any git state change ("REPORT ONLY"); the controller runs `git status -s` after each review (a prior milestone had a reviewer leave a rogue uncommitted edit).

---

## Milestone context

M1–M18 are merged to `main`. Ember reads/classifies/mutates/sends/forwards mail, has settings + a **read-only** Google Calendar week view (M10) + search + folders + notifications + drafts + batch + labels + attachments + forward/reply-all. M10 added the `calendar.readonly` scope and a `CalendarClient` (`list_calendars`, `list_events`, a pure `map_event`, a DB-free `fetch_calendar_week`); it explicitly **deferred create/edit/delete and the event-detail popover**. M19 delivers exactly those, plus meeting features (guests + Meet). It is the **foundation** of the user's "calendar & meeting notes" feature area; the harder pieces (notes storage, Ollama summarization, Zoom/Meet transcription capture) come in later milestones.

**Reuse map:** the M10 `CalendarClient` + its status→`AppError::Auth` mapping + `map_event` + the `connect()` scope-add pattern (`prompt=consent` → reconnect re-grants); the M10 frontend (`CalendarView`, `WeekGrid`, `lib/calendar.ts` week math + `toTimeMinMax`, the `isTauri()` mock seam); the M8 modal pattern (role=dialog, window-Esc, no-backdrop-close); `lib/compose.ts` `parseRecipients`/`isPlausibleEmail` for guest emails; the `SystemTime`-unique-id trick from M17's boundary.

---

## Scope

**In scope (lean v1):**
- **Create** an event/meeting: title, date, start/end time (or **all-day**), location, description, **guest emails** (invited via `attendees` + `sendUpdates=all`), and an **"Add Google Meet"** toggle (auto-creates a Meet link).
- **Edit** an existing event (title/time/all-day/location/description/guests) via PATCH — preserves an existing Meet link.
- **Delete** an event (`sendUpdates=all`, so guests get the cancellation).
- A **calendar picker** in the form — defaults to the **primary** calendar; lists writable calendars.
- An **event-detail popover** (click an event) showing time/location/description/guests/Meet link + **Edit**/**Delete**; a **New event** button + **click-empty-slot** to create at that time.
- Browser-maket mocks for create/edit/delete so the form is demoable offline.

**Explicitly deferred (not in M19):**
- **Recurring-event creation** (RRULE) — v1 creates **single** events. Editing/deleting a recurring **instance** (M10 expands them via `singleEvents`) affects **just that instance** (no "this and following / all events" choice).
- **Adding/removing a Meet link on an existing event** — the toggle is **create-only**; edit preserves whatever `conferenceData` exists.
- Per-event **color** picker, **attachments**, **reminders/notifications** config, RSVP-response display beyond the guest email list, **drag-to-move/resize**, a **timezone picker** (new events use the **local** timezone).

---

## Components

### Backend — OAuth (`src-tauri/src/auth/mod.rs`)
- Add `const SCOPE_CALENDAR_EVENTS = "https://www.googleapis.com/auth/calendar.events";` and a third `.add_scope(...)` in `connect()` (alongside `gmail.modify` + `calendar.readonly`). `calendar.readonly` **stays** (it backs `calendarList` + event reads); `calendar.events` adds write. Reconnect re-grants all three (`connect()` already forces `prompt=consent`). **No migration.** A pre-M19 token lacking `calendar.events` → write calls return 403 → the existing reconnect UI.

### Backend — `calendar/types.rs`
- `GEvent` (read) gains: `description: Option<String>`, `html_link` (`#[serde(rename="htmlLink")]`), `hangout_link` (`#[serde(rename="hangoutLink")]`), `attendees: Option<Vec<GAttendee>>` where `GAttendee { email: String, response_status: Option<String> (rename "responseStatus") }`.
- `CalendarEvent` (the normalized Serialize shape → frontend) gains: `description: Option<String>`, `meet_link: Option<String>`, `html_link: Option<String>`, `attendees: Vec<String>` (guest emails). (Existing fields unchanged → additive for the M10 week grid.)
- New **write** types (Serialize → Google):
  - `EventWrite` (Deserialize from JS, snake_case): `{ title: String, start: String, end: String, all_day: bool, description: Option<String>, location: Option<String>, attendees: Vec<String> }` (start/end are RFC3339 dateTime for timed events, or `YYYY-MM-DD` for all-day).
  - A private request body struct mapping to Google's event resource: `summary`, `start`/`end` as `{ "dateTime": ... }` (timed) or `{ "date": ... }` (all-day, when `all_day`), `description`, `location`, `attendees: [{ "email": ... }]`, optional `conferenceData.createRequest { requestId, conferenceSolutionKey: { type: "hangoutsMeet" } }`. The backend maps `EventWrite.start`/`.end` **verbatim** (no date arithmetic) — Google's all-day exclusive-end convention is the **frontend's** responsibility (see `lib/calendar.ts` below), so `EventWrite.end` for an all-day event already holds the exclusive date.

### Backend — `calendar/mod.rs`
- `create_event(&self, calendar_id, ev: &EventWrite, add_meet: bool) -> Result<CalendarEvent>` — `POST {base}/calendar/v3/calendars/{calendar_id}/events?conferenceDataVersion=1&sendUpdates=all`. Build the body from `EventWrite`; when `add_meet`, add `conferenceData.createRequest` with a unique `requestId` (`format!("ember-meet-{nanos}")` from `SystemTime`). Parse the returned event → `map_event`.
- `update_event(&self, calendar_id, event_id, ev: &EventWrite) -> Result<CalendarEvent>` — `PATCH {base}/.../events/{event_id}?sendUpdates=all` with the same body **minus** `conferenceData` (PATCH preserves the existing Meet link; sending `attendees` **replaces** the guest list — fine, the form is prefilled).
- `delete_event(&self, calendar_id, event_id) -> Result<()>` — `DELETE {base}/.../events/{event_id}?sendUpdates=all`.
- All three reuse the existing status-peek → 401/403 `AppError::Auth("…reconnect…")` mapping (extract the helper if needed so writes share it with the read path).
- `map_event` (and `GEvent`) extended to carry `description`/`meet_link` (from `hangoutLink`)/`html_link`/`attendees`.

### Backend — commands (`src-tauri/src/commands.rs`, registered in `lib.rs`)
All DB-free (calendar is live-fetched; no cache, no migration).
- `create_calendar_event(calendar_id: String, event: EventWrite, add_meet: bool) -> Result<CalendarEvent>`.
- `update_calendar_event(calendar_id: String, event_id: String, event: EventWrite) -> Result<CalendarEvent>`.
- `delete_calendar_event(calendar_id: String, event_id: String) -> Result<()>`.

### Frontend — api + helpers
- `lib/api.ts`: `EventWrite` interface; `createCalendarEvent(calendarId, event, addMeet)`, `updateCalendarEvent(calendarId, eventId, event)`, `deleteCalendarEvent(calendarId, eventId)` wrappers (`isTauri()`-gated with mocks); extend the `CalendarEvent` interface with `description?`/`meet_link?`/`html_link?`/`attendees`.
- `lib/calendar.ts`: pure helpers — build a local RFC3339 `dateTime` from a `YYYY-MM-DD` date + `HH:MM` time (with the local UTC offset, mirroring `toTimeMinMax`); **for an all-day event, compute the exclusive end date (the user-picked end date + 1 day)** so the backend can pass it through verbatim — this `+1` lives **only here**; reuse `parseRecipients`/`isPlausibleEmail` (from `lib/compose.ts`) for guest emails; a default-end helper (start + 1h).

### Frontend — components
- **`EventModal.tsx`** (new): the create/edit form — title, date, start time, end time, **all-day** toggle (hides the time inputs), location, description, **guests** (comma-separated emails), a **calendar** dropdown (from `list_calendars`, default `primary`), and an **"Add Google Meet"** toggle (**create only**; on edit, show the existing Meet link read-only). Buttons: **Save** (create or update), **Delete** (edit only, with a confirm). Reuses the M8 modal pattern. Validates title required, end-after-start, valid guest emails before calling.
- **`CalendarView.tsx`:** a header **New event** button (opens `EventModal` blank, default start = next hour today / the visible week). After any create/edit/delete → bump a `reloadKey` so the existing week-fetch effect refetches.
- **`WeekGrid.tsx`:** click an **empty slot** → open `EventModal` prefilled with that day + hour (1h default). Click an **event** → an **event-detail popover** (title/time/location/description/guests/Meet-link-as-link + **Edit** → prefilled `EventModal`, **Delete** → confirm → `deleteCalendarEvent`).
- **`lib/mock.ts`:** `mockCreateEvent`/`mockUpdateEvent` (echo a `CalendarEvent` built from the input, with a fake id + a mock Meet link when `addMeet`) and `mockDeleteEvent` (no-op), so the form + grid work in the maket.

### Data flow
`New event / click slot → EventModal → createCalendarEvent(calId, EventWrite, addMeet) → CalendarClient POST (+conferenceData when addMeet, +attendees, sendUpdates=all) → refetch week`. `Click event → detail popover → Edit (prefill EventModal) / Delete → update/delete command → refetch`.

---

## Error handling

- **401/403 on any write** → `AppError::Auth` → the existing calendar **"Reconnect"** empty-state (a pre-M19 token lacking `calendar.events` hits this until the user reconnects).
- **Form validation** (title required, end after start, valid guest emails) runs before the call; the title/time errors block submit. Other write failures surface inline in the modal.
- A create/edit that succeeds but whose Meet link wasn't provisioned (rare Google delay) → the event still saves; the Meet link simply appears on the next week refetch.

---

## Testing

- **Rust (wiremock):** `create_event` — POST body carries `summary`/`start`/`end`/`attendees`; `conferenceData.createRequest` present **iff** `add_meet`; the response (with `hangoutLink`) maps to a `CalendarEvent` with `meet_link` set. `update_event` — PATCH to the right path with the body (no `conferenceData`). `delete_event` — a DELETE is issued. Extend the `map_event`/`GEvent` parse tests for `description`/`hangoutLink`/`attendees`. All-day vs timed body shape (`date` vs `dateTime`, exclusive end).
- **Frontend:** no TS harness (consistent through M18). The pure `lib/calendar.ts` helpers (RFC3339 build, exclusive-end, default-end) are specified precisely. Maket-verified by screenshot: the create form (with the Meet toggle + guests), a created event on the grid, and the event-detail popover showing a Meet link.
- `cargo test` + `cargo clippy --all-targets` green; `npm run build` clean. **Live Google E2E** (real create → guest invite email → Meet link → edit → delete, after reconnecting for the new scope) is **owner-pending**, consistent with M10–M18.

---

## Known risks & decisions

- **`calendar.events` added, `calendar.readonly` kept** — readonly still backs `calendarList`/event reads; events adds write. The additive pair guarantees every call works; one reconnect grants all three scopes. (Chosen over the single broad `calendar` scope to stay closer to least-privilege while still avoiding re-consent churn.)
- **`sendUpdates=all`** — real email invitations / updates / cancellations are sent to guests. A genuine outward side effect, deliberately chosen (consistent with the app already sending mail); confirmed in design.
- **Meet link via `conferenceData.createRequest` on create** (`conferenceDataVersion=1`, a `SystemTime` `requestId` — unique, mirroring the M17 boundary). Edit **preserves** an existing Meet link (PATCH without `conferenceData`) but does **not** add/remove one (deferred).
- **Single (non-recurring) events in v1**; editing/deleting a recurring **instance** affects just that instance (Google's expanded-instance id); RRULE creation + "all occurrences" editing deferred.
- **Local timezone** for new events — RFC3339 with the local UTC offset (matching M10's `toTimeMinMax`); Google infers the zone from the offset. No timezone picker.
- **No caching / no migration** — events are created/edited live and the week is refetched; nothing is cached (consistent with M10).

---

## Non-goals / constraints

- **One new OAuth scope** (`calendar.events`); a reconnect re-grants — **no migration**, no special handling beyond the existing 403→reconnect path.
- **No DB migration, no new local storage** — meeting-notes/transcript storage is a **later** milestone in this feature area, not M19.
- **Tauri build unchanged for the maket** — every new wrapper is `isTauri()`-gated; the form/popover are frontend over mock data.
- **Plain create/edit** — no rich event description editor; description is plain text.
