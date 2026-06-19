# Ember — Milestone 10: Calendar (read-only Google Calendar week view, lean v1) — Design Spec

**Status:** Approved design (2026-06-19). Implementation plan to follow via `superpowers:writing-plans`.

**Goal:** Add a **Calendar** view to Ember — a read-only, time-grid **week view** of the user's
Google Calendar(s), with a `Mail | Calendar` toggle in the header and `‹ Today ›` week navigation.
This is the first non-mail surface and the start of Ember being a fuller daily driver.

**Architecture in one paragraph:** The split mirrors the rest of Ember — **Rust is a dumb pipe**
(OAuth token reuse + HTTP + JSON + merge), **JavaScript owns all timezone and layout math** (where
`Date` is easy and the browser's local tz is correct by definition). A new `calendar.readonly`
OAuth scope is added alongside `gmail.modify`; because `connect()` already forces `prompt=consent`,
a **reconnect re-grants both scopes** with no migration. A new `CalendarClient` (mirroring
`GmailClient`) hits `calendar/v3`: it lists the user's *selected* calendars and fetches each one's
events for a `[timeMin, timeMax)` window concurrently, merges them, and returns a flat
`Vec<CalendarEvent>` (raw RFC3339 / `YYYY-MM-DD` strings + an `all_day` flag + the calendar's
color). One DB-free command, `fetch_calendar_week(time_min, time_max)`, orchestrates this. The React
side adds a pure `lib/calendar.ts` (week math + overlap lane-packing), a pure `WeekGrid` presentational
component, and a `CalendarView` container that owns the visible week and fetch state. A new
`isTauri()` seam in `lib/api.ts` returns **mock data** in a plain browser so the whole app — calendar
included — runs via `npm run dev` (the "maket"); in the real Tauri build behavior is unchanged.

**Tech Stack:** Rust (reqwest, serde, futures, Tauri 2, wiremock for tests), React 19 + TypeScript +
Vite, `@tauri-apps/api` v2.11 (`isTauri`, `invoke`), lucide-react icons.

**Learning mode (IMPORTANT — every implementer):** the repo owner is learning Rust. All Rust code
MUST carry concise `// 🦀` teaching comments on the *language* concept, not just intent. After each
task, give a short plain-English Rust recap. TypeScript/React gets normal comments.

---

## Milestone context

M1–M9 are merged to `main`. The app reads, classifies, mutates, sends mail, and has settings +
disconnect. OAuth uses a single restricted scope `gmail.modify` (`auth/mod.rs`). M10 adds the first
non-mail feature: a read-only calendar week view. It introduces the **first additional OAuth scope**
(`calendar.readonly`) and the **second Google API client** (`CalendarClient` alongside
`GmailClient`). The roadmap does not yet define M11 — that is decided after M10 ships.

---

## Scope

**In scope (lean v1):**
- A **time-grid week view** (Mon–Sun): hour gutter, 7 day columns, all-day strip, current-time line.
- **Mail | Calendar** toggle in the header; `‹ Today ›` + a date-range label for week navigation.
- A **`CalendarClient`** that reads `calendarList` + `events.list` (with recurring-event expansion).
- Fetch from **all *selected* calendars**, merged and sorted; events tinted by their calendar color.
- **`calendar.readonly`** scope added; reconnect re-grants it (with a reconnect empty-state for
  already-connected users whose token predates the scope).
- A **browser mock path** (`isTauri()` seam) so the full app + calendar runs in a plain browser.

**Explicitly deferred (not in M10):**
- Offline calendar cache (events are fetched live per week navigation — no SQLite, no migration).
- Creating / editing / deleting events (read-only).
- Day / month / agenda views (week only).
- Event-detail popover (hover `title` shows full text); drag-to-reschedule.
- Per-event `colorId` palette (we use the per-**calendar** `backgroundColor` only).
- A timezone picker (uses the browser's local timezone).
- Multi-week prefetch / infinite scroll; calendar selection UI (we honor Google's `selected` flag).
- A JS test runner (`vitest`) — calendar math is verified in-browser for v1; vitest is a follow-up.

---

## Components & contracts

### Backend — `src/auth/mod.rs`
Add the read-only Calendar scope next to the existing Gmail scope:
```rust
const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";
// in connect()'s authorize_url builder, alongside the existing add_scope:
.add_scope(Scope::new(SCOPE_GMAIL_MODIFY.into()))
.add_scope(Scope::new(SCOPE_CALENDAR_READONLY.into()))
```
No other auth change: `connect()` already sets `access_type=offline` + `prompt=consent`, so the next
connect re-prompts and grants both scopes; `ensure_access_token()` refresh path is unchanged.

### Backend — `src/calendar/types.rs` (NEW)
Google-shaped deserialize types + the normalized type sent to the frontend:
```rust
// Google calendarList entry (only the fields we use)
#[derive(Deserialize)] struct CalendarListEntry {
    id: String, summary: Option<String>,
    #[serde(rename = "backgroundColor")] background_color: Option<String>,
    selected: Option<bool>, primary: Option<bool>,
}
#[derive(Deserialize)] struct CalendarListResponse { items: Vec<CalendarListEntry>,
    #[serde(rename = "nextPageToken")] next_page_token: Option<String> }

// Google event start/end: either dateTime (timed) or date (all-day)
#[derive(Deserialize)] struct GEventDateTime {
    #[serde(rename = "dateTime")] date_time: Option<String>, date: Option<String> }
#[derive(Deserialize)] struct GEvent { id: String, summary: Option<String>,
    start: Option<GEventDateTime>, end: Option<GEventDateTime>,
    location: Option<String>, status: Option<String> }
#[derive(Deserialize)] struct EventsResponse { items: Vec<GEvent>,
    #[serde(rename = "nextPageToken")] next_page_token: Option<String> }

// Normalized, sent to the frontend (Serialize)
#[derive(Serialize)] pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,        // summary, or "(no title)"
    pub start: String,        // RFC3339 (timed) or "YYYY-MM-DD" (all-day)
    pub end: String,
    pub all_day: bool,        // true when start.date is set (no dateTime)
    pub location: Option<String>,
    pub color: Option<String>,// the calendar's backgroundColor (hex), if any
}
```

### Backend — `src/calendar/mod.rs` (NEW)
`CalendarClient` mirrors `GmailClient`'s shape and helpers:
```rust
const DEFAULT_BASE: &str = "https://www.googleapis.com";
pub struct CalendarClient { base_url: String, access_token: String, http: reqwest::Client }
impl CalendarClient {
    pub fn new(access_token: String) -> Self;                       // DEFAULT_BASE
    pub fn with_base_url(access_token: String, base_url: String) -> Self;  // tests
    // private async get_json::<T>(&self, url) -> Result<T>  (bearer_auth + error_for_status, but
    //   maps 401/403 to AppError::Auth("Calendar access not granted — reconnect Google …") by
    //   checking status BEFORE error_for_status, like GmailClient::list_history handles 404)

    /// All calendars in the user's list (paginated).
    pub async fn list_calendars(&self) -> Result<Vec<CalendarListEntry>>;
    /// Events in [time_min, time_max) for one calendar: singleEvents=true (expands recurring),
    /// orderBy=startTime, paginated. time_min/time_max are RFC3339 strings.
    pub async fn list_events(&self, calendar_id: &str, time_min: &str, time_max: &str)
        -> Result<Vec<GEvent>>;
}
```
- `list_calendars` GET `…/calendar/v3/users/me/calendarList` (follow `nextPageToken`).
- `list_events` GET `…/calendar/v3/calendars/{id}/events?singleEvents=true&orderBy=startTime
  &timeMin=…&timeMax=…&maxResults=250` (calendar id and query values percent-encoded via
  `url::form_urlencoded`, as `list_inbox_message_ids_paged` does); follow `nextPageToken`.

### Backend — `src/commands.rs` + `src/lib.rs`
```rust
#[tauri::command]
pub async fn fetch_calendar_week(time_min: String, time_max: String) -> Result<Vec<CalendarEvent>>;
```
- `ensure_access_token().await?` → `CalendarClient::new(stored.access_token)`.
- `list_calendars()` → keep entries where `selected != Some(false)` (Google omits `selected` on
  primary; treat absent as shown).
- For each kept calendar, `list_events(id, &time_min, &time_max)` concurrently with
  `buffer_unordered(CALENDAR_CONCURRENCY)` (reuse the `futures::stream` pattern from
  `get_message_previews`); per-calendar failures are skipped (one broken calendar ≠ whole-week fail),
  **except** a scope/auth error (401/403) which propagates so the UI can show Reconnect.
- Map each `GEvent` → `CalendarEvent`, attaching the owning calendar's `background_color`; skip
  `status == Some("cancelled")`; title falls back to `"(no title)"`; `all_day = start.date.is_some()`.
- Sort by `start` as a best-effort convenience (lexicographic). Exact ordering here is **not
  load-bearing**: `WeekGrid`/`layoutDay` re-sort and position by the parsed local time, so mixed
  UTC offsets in RFC3339 strings don't cause misplacement.
- DB-free (no `State<Db>` param). Registered in `lib.rs` `generate_handler!` + `pub mod calendar;`.
- A `const CALENDAR_CONCURRENCY: usize = 6;` near the existing `PREVIEW_CONCURRENCY`.

### Frontend — `src/lib/calendar.ts` (NEW, pure)
```ts
export interface CalendarEvent { id: string; calendar_id: string; title: string;
  start: string; end: string; all_day: boolean; location: string | null; color: string | null }

export function startOfWeek(d: Date): Date;          // local Monday 00:00
export function addWeeks(d: Date, n: number): Date;
export function weekDays(weekStart: Date): Date[];   // 7 local dates Mon..Sun
export function weekRangeLabel(weekStart: Date): string;          // "Jun 15 – 21, 2026"
export function toTimeMinMax(weekStart: Date): { timeMin: string; timeMax: string }; // RFC3339, local
export function splitAllDay(evts: CalendarEvent[]): { allDay: CalendarEvent[]; timed: CalendarEvent[] };
export function eventsForDay(timed: CalendarEvent[], day: Date): CalendarEvent[];    // by local day
// Overlap lane-packing: assign each day's timed events to equal-width lanes so concurrent
// events sit side-by-side. Returns geometry for WeekGrid to position blocks.
export interface PositionedEvent { ev: CalendarEvent; topMin: number; heightMin: number;
  lane: number; lanes: number }
export function layoutDay(timed: CalendarEvent[], day: Date): PositionedEvent[];
```
- `topMin`/`heightMin` are minutes-from-local-midnight; `WeekGrid` multiplies by px/min.
- `layoutDay`: sort by start; greedy interval grouping into overlap clusters; within a cluster assign
  lanes (first free lane); `lanes` = cluster width. Min visual height clamp (e.g. 24 min) lives in
  `WeekGrid`, not here.

### Frontend — `src/components/WeekGrid.tsx` (NEW, pure presentational)
Props: `{ weekStart: Date; events: CalendarEvent[]; now: Date }`. Renders:
- A day-header row (Mon 15 … Sun 21; today's date pill in accent).
- An **all-day strip** under the headers (all-day events as bars; multi-day spans clamped to the week).
- A scrollable time grid: hour gutter (12 AM–11 PM) + 7 `position:relative` day columns with hour
  gridlines; timed events absolutely positioned from `layoutDay` (top/height in px; left/width from
  `lane`/`lanes`); event block tinted by `color` (subtle left-border + faint fill; fallback to
  `--accent`); `title` attribute = full title + time.
- A **current-time line** spanning the columns at `now`'s minute, **only when `now` is in the visible
  week**; parent re-renders it on a 60s tick.
- On mount, the scroll area scrolls so ~7 AM is at the top (and to `now` if today is in view).
- Grid metrics: `PX_PER_MIN = 0.8` → 48px/hour, 1152px/day scroll height.

### Frontend — `src/components/CalendarView.tsx` (NEW, container)
- Props: `weekStart: Date` (owned by `App`, see below). State: `events`, `loading`, `error`, `now`.
- On `weekStart` change: `toTimeMinMax` → `fetchCalendarWeek(timeMin, timeMax)` → set events.
- Renders `<WeekGrid weekStart events now />`; shows loading and (non-scope) error states.
- **Reconnect empty-state:** if the error indicates missing calendar access, show "Ember needs
  permission to read your Google Calendar" + a **Reconnect Google** button → `connectGmail()` →
  refetch. (Detected by matching the backend's auth-error message.)
- 60s `setInterval` updates `now` (drives the current-time line).

### Frontend — `src/lib/api.ts` (the mock seam)
```ts
import { invoke, isTauri } from "@tauri-apps/api/core";
// New wrapper:
export const fetchCalendarWeek = (timeMin: string, timeMax: string): Promise<CalendarEvent[]> =>
  isTauri() ? invoke("fetch_calendar_week", { timeMin, timeMax })
            : mockCalendarWeek(timeMin, timeMax);
```
And, so the app renders past the connect screen in a browser, the existing **read** wrappers gain a mock
branch when `!isTauri()`: `getConnectedAccount()` → a mock email; `fetchInboxPreview()` → a few mock
previews; `syncInbox()` → `{added:0,removed:0}`. Mail **mutations** (read/star/archive/trash/send)
are *not* mocked — they aren't exercised by the calendar maket; calling one in the browser rejects as
today. **Guard:** every mock branch is `!isTauri()`, so the Tauri build (where `isTauri()` is true)
is byte-for-byte unchanged.

### Frontend — `src/lib/mock.ts` (NEW, dev-only data)
`mockCalendarWeek(timeMin, timeMax)` generates a plausible week of events relative to the requested
window (a daily standup, a couple of meetings, an all-day span, a weekend event) so the maket looks
real for any navigated week. Plus `MOCK_ACCOUNT` and a small `MOCK_MESSAGES` array.

### Frontend — `src/components/Header.tsx`
- Always-visible **`Mail | Calendar`** segmented toggle (new `view` + `onSelectView` props).
- When `view === "calendar"`: render week-nav (`‹`, `Today`, `›` + range label) **in place of** the
  stream nav; hide Compose/Sync (mail-only). New optional props:
  `calendar?: { rangeLabel: string; onPrev(): void; onToday(): void; onNext(): void }`.
- Account avatar, Settings gear, theme toggle: unchanged, always visible.

### Frontend — `src/App.tsx`
- New `view: "mail" | "calendar"` state (defaults to `"calendar"` in browser mock mode so the maket
  shows immediately; `"mail"` in the Tauri app).
- **`App` owns `weekStart`** (`useState(startOfWeek(now))`) and the nav handlers (`prev`/`today`/
  `next` via `addWeeks`/`startOfWeek`). It passes `weekStart` to `<CalendarView>` (for fetch) and a
  `calendar={{ rangeLabel: weekRangeLabel(weekStart), onPrev, onToday, onNext }}` object to
  `<Header>` — so the header's week-nav and the grid stay in sync from a single source of truth.
- When `view === "calendar"`, render `<CalendarView weekStart>` instead of `<SplitView>`; the error
  bar / modals still mount.
- The connect gate is unchanged for Tauri; in browser mock mode `account` is the mock email so the
  app renders.

### Frontend — `src/styles/app.css`
Calendar styles: header toggle (segmented control) + week-nav pills; day-header row; all-day strip;
time grid (gutter, gridlines via `repeating-linear-gradient`), event block, current-time line;
reconnect empty-state. Reuse existing tokens (`--surface`, `--border`, `--accent`, `--accent-weak`).

---

## Data flow

**Open calendar:** Header toggle → `view="calendar"` → `CalendarView` mounts with
`weekStart = startOfWeek(now)` → `toTimeMinMax` → `fetchCalendarWeek` → `WeekGrid` renders.

**Navigate weeks:** `‹`/`›` → `weekStart = addWeeks(±1)`; `Today` → `startOfWeek(now)` → refetch.

**Real fetch (Tauri):** `invoke("fetch_calendar_week", {timeMin,timeMax})` → command →
`ensure_access_token` → `list_calendars` (selected) → concurrent `list_events` → merge/map/sort →
`CalendarEvent[]`.

**Maket (browser):** `!isTauri()` → `mockCalendarWeek` returns generated events for the window; the
app also serves a mock account + messages so the full shell renders via `npm run dev`.

**Reconnect:** a connected user whose token predates the scope → first calendar fetch returns the
auth error → `CalendarView` shows Reconnect → `connectGmail()` re-runs consent (now both scopes) →
refetch succeeds.

---

## Error handling

- **Scope/auth (401/403)** from any calendar call → `AppError::Auth("Calendar access not granted —
  reconnect Google to enable it.")`; `CalendarView` renders the Reconnect empty-state.
- **Per-calendar failure** (one calendar 404s / rate-limits) is skipped in the merge — the rest of
  the week still renders. (Auth errors are the exception and propagate.)
- **Network / other** errors → `CalendarView` shows an inline error with a Retry that refetches.
- **Malformed event** (missing start/end) → skipped defensively; never panics.
- No `MutexGuard` concerns — the command is DB-free.

---

## Testing strategy

- **Rust `CalendarClient`** (`tests/calendar_test.rs`, wiremock + `with_base_url`, mirroring
  `gmail_test.rs`):
  - `list_calendars` parses items and follows `nextPageToken`.
  - `list_events` sends `singleEvents=true`, `orderBy=startTime`, `timeMin`/`timeMax`; parses both a
    **timed** event (`dateTime`) and an **all-day** event (`date`); follows pagination.
  - 403 response → `AppError::Auth` (the friendly reconnect message), not a generic HTTP error.
- **Command-level mapping** (cancelled-skip, `all_day` detection, color attach, title fallback) is
  covered either by a small unit test on a pure `map_event` helper or via the client tests — the
  implementer keeps the mapping in a pure, testable function.
- **Frontend `lib/calendar.ts`**: pure and the riskiest math (week boundaries across midnight,
  `layoutDay` overlap lanes). **No JS test runner today** — verified in-browser via the maket for v1;
  `vitest` for these helpers is a noted follow-up.
- **Manual E2E (Tauri):** connect (grants both scopes) → open Calendar → current week renders real
  events at correct times; navigate weeks; all-day events in the strip; current-time line on today;
  overlapping events sit side-by-side; a pre-scope token shows Reconnect, and reconnecting works.
- **Maket E2E (browser):** `npm run dev` → app opens on the Calendar view with mock events;
  week navigation works; screenshot for review.

---

## Definition of done

- A `Mail | Calendar` toggle switches views; the calendar shows a Mon–Sun time-grid week with the
  user's real events (timed + all-day), a current-time line, week navigation, and side-by-side
  overlaps.
- `calendar.readonly` scope added; a fresh connect grants it; pre-scope tokens get a working
  Reconnect path.
- The app (calendar included) runs in a plain browser via `npm run dev` on mock data; the Tauri
  build is unchanged (`isTauri()`-guarded).
- New Rust code carries `// 🦀` comments; a plain-English Rust recap accompanies each task.
- `cargo test` green (existing + new calendar tests); `cargo clippy --all-targets -D warnings` clean;
  `npm run build` clean. (`cargo fmt` is **not** used in this repo — not a gate.)
- **No DB migration / no schema change** (calendar is fetched live, DB-free).

---

## Known limitations (carried as deferrals)

- Events are fetched live per week with no offline cache; navigating weeks re-hits the API.
- Read-only: no create/edit/delete, no day/month views, no event-detail popover, no drag.
- Coloring is per-**calendar** (`backgroundColor`), not per-event `colorId`.
- Uses the browser's local timezone; no timezone picker and no per-calendar tz handling beyond what
  Google returns in the event's `dateTime` offset.
- Honors Google's `selected` flag for which calendars to show; no in-app calendar picker.
- Overlap layout is equal-width lanes, not Google's fractional/cascading packing.
- The browser mock path is dev-only scaffolding for the maket, not a supported runtime.
