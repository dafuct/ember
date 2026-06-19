# Ember M10 — Calendar (read-only week view, lean v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only, Monday-start, time-grid **week view** of the user's Google Calendar(s) to Ember, with a `Mail | Calendar` header toggle and `‹ Today ›` week navigation — runnable in a plain browser on mock data (the "maket") and against live data in the Tauri app.

**Architecture:** Rust is a dumb pipe (reuse the OAuth token, list *selected* calendars, fetch each calendar's events for a `[timeMin, timeMax)` window concurrently, merge → `Vec<CalendarEvent>`). JavaScript owns all timezone/layout math (`lib/calendar.ts`) and rendering (`WeekGrid`). A new `calendar.readonly` scope is added; reconnect re-grants it. An `isTauri()` seam in `lib/api.ts` serves mock data in the browser so the whole app runs via `npm run dev`.

**Tech Stack:** Rust (reqwest, serde, futures, Tauri 2; wiremock for tests), React 19 + TypeScript + Vite, `@tauri-apps/api` v2.11 (`isTauri`, `invoke`), lucide-react.

**Design spec:** `docs/superpowers/specs/2026-06-19-ember-m10-calendar-design.md`.

**Conventions for every task:**
- New Rust code carries concise `// 🦀` teaching comments on the *language* concept (owner is learning Rust). Give a one-paragraph plain-English Rust recap after each Rust task.
- Every commit message ends with the trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` (shown via a second `-m`).
- Branch is already `m10-calendar`. Work there.
- `cargo fmt` is **not** used in this repo (not a gate). Gates are: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `npm run build`.

---

## File structure

**Frontend (build order first — gets the maket on screen):**
- Create `src/lib/calendar.ts` — pure date + layout math (week boundaries, RFC3339 bounds, overlap lane-packing). No I/O.
- Create `src/lib/mock.ts` — dev-only mock data (account, messages, generated week of events).
- Modify `src/lib/api.ts` — add `fetchCalendarWeek`; add `!isTauri()` mock branches to the read wrappers.
- Create `src/components/WeekGrid.tsx` — pure presentational grid (headers, all-day strip, time grid, now-line).
- Create `src/components/CalendarView.tsx` — container: fetch state, 60s tick, reconnect empty-state.
- Modify `src/components/Header.tsx` — `Mail | Calendar` toggle + week-nav (replaces stream nav in calendar view).
- Modify `src/App.tsx` — `view` + `weekStart` state, render `CalendarView`, pass nav to Header, browser-mock defaults.
- Modify `src/styles/app.css` — calendar grid + header toggle/nav styles.

**Backend:**
- Modify `src-tauri/src/auth/mod.rs` — add `calendar.readonly` scope.
- Create `src-tauri/src/calendar/types.rs` — Google + normalized serde types.
- Create `src-tauri/src/calendar/mod.rs` — `CalendarClient` (+ `map_event` pure fn).
- Create `src-tauri/tests/calendar_test.rs` — wiremock integration tests.
- Modify `src-tauri/src/commands.rs` — `fetch_calendar_week` command.
- Modify `src-tauri/src/lib.rs` — `pub mod calendar;` + register the command.

---

## Task 1: Pure calendar date + layout helpers

**Files:**
- Create: `src/lib/calendar.ts`

No JS test runner exists in this repo (vitest deferred per spec); this module is verified by `npm run build` (type-check) and exercised in-browser at Task 6. Keep it pure.

- [ ] **Step 1: Create `src/lib/calendar.ts` with the full module below**

```ts
// src/lib/calendar.ts — pure date + layout helpers for the week view (no I/O, no React).
// All math is in the browser's LOCAL timezone, which is correct for the user by definition.

export interface CalendarEvent {
  id: string;
  calendar_id: string;
  title: string;
  /** RFC3339 with offset (timed) or "YYYY-MM-DD" (all-day). */
  start: string;
  end: string;
  all_day: boolean;
  location: string | null;
  /** The owning calendar's background color (hex), if any. */
  color: string | null;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Local Monday 00:00 of the week containing `d`. */
export function startOfWeek(d: Date): Date {
  const x = new Date(d.getFullYear(), d.getMonth(), d.getDate()); // local midnight
  const dow = (x.getDay() + 6) % 7; // Mon=0 … Sun=6
  x.setDate(x.getDate() - dow);
  return x;
}

export function addWeeks(d: Date, n: number): Date {
  const x = new Date(d);
  x.setDate(x.getDate() + n * 7);
  return x;
}

/** The 7 local day-dates Mon..Sun for the week starting at `weekStart`. */
export function weekDays(weekStart: Date): Date[] {
  return Array.from({ length: 7 }, (_, i) => {
    const x = new Date(weekStart);
    x.setDate(x.getDate() + i);
    return x;
  });
}

/** "Jun 15 – 21, 2026", or "Jun 29 – Jul 5, 2026" when the week crosses a month. */
export function weekRangeLabel(weekStart: Date): string {
  const days = weekDays(weekStart);
  const first = days[0];
  const last = days[6];
  const mFirst = first.toLocaleString("en-US", { month: "short" });
  const mLast = last.toLocaleString("en-US", { month: "short" });
  const y = last.getFullYear();
  return mFirst === mLast
    ? `${mFirst} ${first.getDate()} – ${last.getDate()}, ${y}`
    : `${mFirst} ${first.getDate()} – ${mLast} ${last.getDate()}, ${y}`;
}

/** RFC3339 local-time string, e.g. 2026-06-15T00:00:00-07:00. */
function toRfc3339Local(d: Date): string {
  const off = -d.getTimezoneOffset(); // minutes east of UTC
  const sign = off >= 0 ? "+" : "-";
  const oh = pad(Math.floor(Math.abs(off) / 60));
  const om = pad(Math.abs(off) % 60);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}${sign}${oh}:${om}`
  );
}

/** RFC3339 bounds [Mon 00:00, next Mon 00:00) for the Google API. */
export function toTimeMinMax(weekStart: Date): { timeMin: string; timeMax: string } {
  return { timeMin: toRfc3339Local(weekStart), timeMax: toRfc3339Local(addWeeks(weekStart, 1)) };
}

/** Local "YYYY-MM-DD" for a Date. */
export function toYmd(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function splitAllDay(evts: CalendarEvent[]): { allDay: CalendarEvent[]; timed: CalendarEvent[] } {
  const allDay: CalendarEvent[] = [];
  const timed: CalendarEvent[] = [];
  for (const e of evts) (e.all_day ? allDay : timed).push(e);
  return { allDay, timed };
}

function sameLocalDay(a: Date, day: Date): boolean {
  return (
    a.getFullYear() === day.getFullYear() &&
    a.getMonth() === day.getMonth() &&
    a.getDate() === day.getDate()
  );
}

/** Whether an all-day event (start/end are "YYYY-MM-DD", end exclusive) covers local day `d`. */
export function allDayOnDay(e: CalendarEvent, d: Date): boolean {
  const ymd = toYmd(d);
  return e.start <= ymd && ymd < e.end;
}

/** Timed events whose local start falls on `day` (v1 assumes events stay within one day). */
export function eventsForDay(timed: CalendarEvent[], day: Date): CalendarEvent[] {
  return timed.filter((e) => sameLocalDay(new Date(e.start), day));
}

export interface PositionedEvent {
  ev: CalendarEvent;
  topMin: number;    // minutes from local midnight
  heightMin: number; // duration in minutes (min 15 enforced here)
  lane: number;      // 0-based column within the overlap cluster
  lanes: number;     // total columns in the cluster
}

/** Lay out one day's timed events into equal-width lanes so overlaps sit side-by-side. */
export function layoutDay(timed: CalendarEvent[], day: Date): PositionedEvent[] {
  const midnight = new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();
  const items = eventsForDay(timed, day)
    .map((ev) => {
      const s = new Date(ev.start).getTime();
      const e = new Date(ev.end).getTime();
      const topMin = Math.max(0, Math.round((s - midnight) / 60000));
      const rawDur = Math.round((e - s) / 60000);
      return { ev, topMin, heightMin: Math.max(15, Number.isFinite(rawDur) && rawDur > 0 ? rawDur : 15) };
    })
    .sort((a, b) => a.topMin - b.topMin || b.heightMin - a.heightMin);

  const out: PositionedEvent[] = [];
  let cluster: typeof items = [];
  let clusterEnd = -1;
  const flush = () => {
    if (cluster.length === 0) return;
    const laneEnds: number[] = [];
    const placed = cluster.map((it) => {
      let lane = laneEnds.findIndex((end) => end <= it.topMin);
      if (lane === -1) {
        lane = laneEnds.length;
        laneEnds.push(0);
      }
      laneEnds[lane] = it.topMin + it.heightMin;
      return { it, lane };
    });
    const lanes = laneEnds.length;
    for (const { it, lane } of placed) {
      out.push({ ev: it.ev, topMin: it.topMin, heightMin: it.heightMin, lane, lanes });
    }
    cluster = [];
    clusterEnd = -1;
  };
  for (const it of items) {
    if (cluster.length > 0 && it.topMin >= clusterEnd) flush();
    cluster.push(it);
    clusterEnd = Math.max(clusterEnd, it.topMin + it.heightMin);
  }
  flush();
  return out;
}
```

- [ ] **Step 2: Type-check**

Run: `npm run build`
Expected: PASS (no TypeScript errors). This compiles `tsc` then Vite; a clean exit means the module type-checks.

- [ ] **Step 3: Commit**

```bash
git add src/lib/calendar.ts
git commit -m "feat(calendar): pure week-view date + overlap-layout helpers" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Mock data + the `isTauri()` API seam

**Files:**
- Create: `src/lib/mock.ts`
- Modify: `src/lib/api.ts`

- [ ] **Step 1: Create `src/lib/mock.ts`**

```ts
// src/lib/mock.ts — DEV-ONLY data so the app renders in a plain browser (the "maket").
// Never used in the Tauri build: every call site is guarded by !isTauri().
import type { CalendarEvent } from "./calendar";
import { toYmd } from "./calendar";
import type { MessagePreview, SyncSummary } from "./api";

export const MOCK_ACCOUNT = "you@example.com (mock)";

export const MOCK_MESSAGES: MessagePreview[] = [
  {
    id: "m1", thread_id: "t1", from: "Maya <maya@studio.co>", subject: "Q3 roadmap",
    date: "Wed, 18 Jun 2026 09:42:00 -0700", snippet: "Here's the draft for review…",
    internal_date: 1750000000000, category: "people", label_ids: ["INBOX", "UNREAD"],
  },
  {
    id: "m2", thread_id: "t2", from: "GitHub <noreply@github.com>", subject: "[ember] CI passed",
    date: "Wed, 18 Jun 2026 08:10:00 -0700", snippet: "All checks have passed on m10-calendar.",
    internal_date: 1749990000000, category: "notifications", label_ids: ["INBOX"],
  },
];

export const MOCK_SYNC: SyncSummary = { added: 0, removed: 0 };

/** Generate a plausible week of events anchored to the requested window's Monday. */
export function mockCalendarWeek(timeMin: string, _timeMax: string): CalendarEvent[] {
  const mon = new Date(timeMin); // local Monday 00:00 from toTimeMinMax
  const day = (offset: number, h: number, m = 0) => {
    const d = new Date(mon);
    d.setDate(d.getDate() + offset);
    d.setHours(h, m, 0, 0);
    return d.toISOString();
  };
  const ymdAt = (offset: number) => {
    const d = new Date(mon);
    d.setDate(d.getDate() + offset);
    return toYmd(d);
  };
  const ACCENT = "#16a34a";
  const AMBER = "#b9722a";
  return [
    { id: "e1", calendar_id: "primary", title: "Standup", start: day(0, 9), end: day(0, 9, 30), all_day: false, location: null, color: ACCENT },
    { id: "e2", calendar_id: "primary", title: "1:1 with Dana", start: day(0, 14), end: day(0, 15), all_day: false, location: "Zoom", color: ACCENT },
    { id: "e3", calendar_id: "primary", title: "Design review", start: day(1, 11), end: day(1, 12, 30), all_day: false, location: null, color: ACCENT },
    { id: "e4", calendar_id: "personal", title: "Dentist", start: day(2, 8, 30), end: day(2, 9, 30), all_day: false, location: null, color: AMBER },
    { id: "e5", calendar_id: "primary", title: "Team sync", start: day(2, 15), end: day(2, 16), all_day: false, location: null, color: ACCENT },
    // overlap demo on Thu: two events 10:00–11:00 and 10:30–11:30
    { id: "e6", calendar_id: "primary", title: "Roadmap", start: day(3, 10), end: day(3, 11), all_day: false, location: null, color: ACCENT },
    { id: "e7", calendar_id: "personal", title: "Lunch w/ Sam", start: day(3, 10, 30), end: day(3, 11, 30), all_day: false, location: "Cafe", color: AMBER },
    { id: "e8", calendar_id: "primary", title: "Ship M9", start: day(4, 9), end: day(4, 10), all_day: false, location: null, color: ACCENT },
    { id: "e9", calendar_id: "primary", title: "Demo", start: day(4, 16), end: day(4, 17), all_day: false, location: null, color: ACCENT },
    { id: "e10", calendar_id: "personal", title: "Hike", start: day(6, 9, 30), end: day(6, 12), all_day: false, location: null, color: AMBER },
    // all-day span Wed→Thu (end exclusive = Fri)
    { id: "e11", calendar_id: "primary", title: "Q3 planning", start: ymdAt(2), end: ymdAt(4), all_day: true, location: null, color: AMBER },
  ];
}
```

- [ ] **Step 2: Modify `src/lib/api.ts` — add the import + `CalendarEvent` re-export + `fetchCalendarWeek`, and mock branches on the read wrappers**

At the top, change the import line and add mock imports:

```ts
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CalendarEvent } from "./calendar";
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek } from "./mock";

export type { CalendarEvent };
```

Replace the three read wrappers (`getConnectedAccount`, `fetchInboxPreview`, `syncInbox`) with mock-guarded versions:

```ts
export const getConnectedAccount = (): Promise<string | null> =>
  isTauri() ? invoke<string | null>("get_connected_account") : Promise.resolve(MOCK_ACCOUNT);

export const fetchInboxPreview = (max = 20): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("fetch_inbox_preview", { max }) : Promise.resolve(MOCK_MESSAGES);
```

```ts
export const syncInbox = (): Promise<SyncSummary> =>
  isTauri() ? invoke<SyncSummary>("sync_inbox") : Promise.resolve(MOCK_SYNC);
```

At the end of the file, add the calendar wrapper:

```ts
export const fetchCalendarWeek = (timeMin: string, timeMax: string): Promise<CalendarEvent[]> =>
  isTauri()
    ? invoke<CalendarEvent[]>("fetch_calendar_week", { timeMin, timeMax })
    : Promise.resolve(mockCalendarWeek(timeMin, timeMax));
```

Note: mail **mutations** (read/star/archive/trash/send) are intentionally NOT mocked — they aren't exercised by the calendar maket.

- [ ] **Step 3: Type-check**

Run: `npm run build`
Expected: PASS. (`SyncSummary` and `MessagePreview` are already exported from `api.ts`; `mock.ts` imports them as types.)

- [ ] **Step 4: Commit**

```bash
git add src/lib/mock.ts src/lib/api.ts
git commit -m "feat(calendar): isTauri() mock seam + fetchCalendarWeek wrapper" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: WeekGrid presentational component + grid styles

**Files:**
- Create: `src/components/WeekGrid.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Create `src/components/WeekGrid.tsx`**

```tsx
import { useEffect, useRef } from "react";
import {
  type CalendarEvent,
  weekDays,
  splitAllDay,
  layoutDay,
  allDayOnDay,
} from "../lib/calendar";

const PX_PER_MIN = 0.8;                       // 48px / hour
const GRID_HEIGHT = 24 * 60 * PX_PER_MIN;     // 1152px
const HOURS = Array.from({ length: 24 }, (_, h) => h);

function hourLabel(h: number): string {
  if (h === 0) return "12 AM";
  if (h === 12) return "12 PM";
  return h < 12 ? `${h} AM` : `${h - 12} PM`;
}

function fmtTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

function sameLocalDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** Use the calendar's color for the left border only; the faint fill comes from CSS. */
function tint(e: CalendarEvent): React.CSSProperties {
  return e.color ? { borderLeftColor: e.color } : {};
}

export function WeekGrid({
  weekStart,
  events,
  now,
}: {
  weekStart: Date;
  events: CalendarEvent[];
  now: Date;
}) {
  const days = weekDays(weekStart);
  const { allDay, timed } = splitAllDay(events);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Scroll so ~7 AM is at the top on mount / week change.
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 7 * 60 * PX_PER_MIN;
  }, [weekStart]);

  const weekEndExclusive = new Date(days[6].getFullYear(), days[6].getMonth(), days[6].getDate() + 1);
  const nowInWeek = now >= days[0] && now < weekEndExclusive;
  const nowTopMin = now.getHours() * 60 + now.getMinutes();

  return (
    <div className="cal-grid">
      <div className="cal-dayhead-row">
        <div className="cal-gutter-cell" />
        {days.map((d) => (
          <div key={d.toISOString()} className={sameLocalDay(d, now) ? "cal-dayhead today" : "cal-dayhead"}>
            <span className="cal-dow">{d.toLocaleString("en-US", { weekday: "short" })}</span>
            <span className="cal-daynum">{d.getDate()}</span>
          </div>
        ))}
      </div>

      {allDay.length > 0 && (
        <div className="cal-allday-row">
          <div className="cal-gutter-cell cal-allday-label">all-day</div>
          {days.map((d) => (
            <div key={d.toISOString()} className="cal-allday-cell">
              {allDay.filter((e) => allDayOnDay(e, d)).map((e) => (
                <div key={e.id} className="cal-allday-ev" style={tint(e)} title={e.title}>
                  {e.title}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}

      <div className="cal-scroll" ref={scrollRef}>
        <div className="cal-body" style={{ height: GRID_HEIGHT }}>
          <div className="cal-gutter">
            {HOURS.map((h) => (
              <div key={h} className="cal-hour" style={{ top: h * 60 * PX_PER_MIN }}>
                {hourLabel(h)}
              </div>
            ))}
          </div>
          {days.map((d) => {
            const positioned = layoutDay(timed, d);
            return (
              <div key={d.toISOString()} className="cal-col">
                {HOURS.map((h) => (
                  <div key={h} className="cal-hourline" style={{ top: h * 60 * PX_PER_MIN }} />
                ))}
                {positioned.map((p) => (
                  <div
                    key={p.ev.id}
                    className="cal-ev"
                    title={`${p.ev.title} · ${fmtTime(p.ev.start)}`}
                    style={{
                      top: p.topMin * PX_PER_MIN,
                      height: Math.max(14, p.heightMin * PX_PER_MIN - 2),
                      left: `calc(${(p.lane / p.lanes) * 100}% + 2px)`,
                      width: `calc(${100 / p.lanes}% - 4px)`,
                      ...tint(p.ev),
                    }}
                  >
                    <span className="cal-ev-title">{p.ev.title}</span>
                    <span className="cal-ev-time">{fmtTime(p.ev.start)}</span>
                  </div>
                ))}
                {nowInWeek && sameLocalDay(d, now) && (
                  <div className="cal-nowline" style={{ top: nowTopMin * PX_PER_MIN }} />
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Append calendar grid styles to `src/styles/app.css`**

```css
/* ===== M10 Calendar — week grid ===== */
.cal-view { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.cal-grid { flex: 1; min-height: 0; display: flex; flex-direction: column; background: var(--surface); }

.cal-dayhead-row,
.cal-allday-row { display: grid; grid-template-columns: 56px repeat(7, 1fr); border-bottom: 1px solid var(--border); }
.cal-gutter-cell { border-right: 1px solid var(--border); }
.cal-dayhead { padding: 6px 0 8px; text-align: center; border-right: 1px solid var(--border); color: var(--text-muted); }
.cal-dow { display: block; font-size: 11px; text-transform: uppercase; letter-spacing: .04em; }
.cal-daynum { display: block; font-size: 18px; font-weight: 600; color: var(--text); margin-top: 2px; }
.cal-dayhead.today .cal-dow { color: var(--accent-text); }
.cal-dayhead.today .cal-daynum {
  color: var(--accent-contrast); background: var(--accent); width: 28px; height: 28px;
  line-height: 28px; border-radius: 50%; margin: 2px auto 0;
}

.cal-allday-label { font-size: 10px; color: var(--text-faint); text-align: right; padding: 4px 6px 0 0; }
.cal-allday-cell { border-right: 1px solid var(--border); padding: 3px 4px; min-height: 22px; }
.cal-allday-ev {
  background: var(--accent-weak); color: var(--accent-text); border-left: 3px solid var(--accent);
  border-radius: 4px; padding: 1px 6px; font-size: 12px; font-weight: 600; margin-bottom: 2px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.cal-scroll { flex: 1; min-height: 0; overflow-y: auto; }
.cal-body { display: grid; grid-template-columns: 56px repeat(7, 1fr); position: relative; }
.cal-gutter { position: relative; border-right: 1px solid var(--border); }
.cal-hour { position: absolute; right: 6px; font-size: 10px; color: var(--text-faint); transform: translateY(-6px); }
.cal-col { position: relative; border-right: 1px solid var(--border); }
.cal-hourline { position: absolute; left: 0; right: 0; border-top: 1px solid var(--border); }
.cal-ev {
  position: absolute; overflow: hidden; border-radius: 5px; padding: 1px 5px;
  background: var(--accent-weak); color: var(--accent-text); border-left: 3px solid var(--accent);
  font-size: 11px; line-height: 1.25; cursor: default;
}
.cal-ev-title { font-weight: 600; display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cal-ev-time { opacity: .8; font-size: 10px; }
.cal-nowline { position: absolute; left: 0; right: 0; height: 2px; background: #e11d48; z-index: 2; }
.cal-nowline::before {
  content: ""; position: absolute; left: -4px; top: -3px; width: 8px; height: 8px;
  border-radius: 50%; background: #e11d48;
}

.cal-loading, .cal-empty {
  flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
  gap: 14px; color: var(--text-muted); padding: 40px;
}
```

- [ ] **Step 3: Type-check**

Run: `npm run build`
Expected: PASS. (WeekGrid isn't mounted yet; this only verifies it compiles.)

- [ ] **Step 4: Commit**

```bash
git add src/components/WeekGrid.tsx src/styles/app.css
git commit -m "feat(calendar): WeekGrid presentational time-grid + styles" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: CalendarView container

**Files:**
- Create: `src/components/CalendarView.tsx`

- [ ] **Step 1: Create `src/components/CalendarView.tsx`**

```tsx
import { useEffect, useState } from "react";
import { type CalendarEvent, toTimeMinMax } from "../lib/calendar";
import { fetchCalendarWeek, connectGmail } from "../lib/api";
import { WeekGrid } from "./WeekGrid";

// The backend maps missing calendar scope to a message containing "reconnect".
function isScopeError(msg: string): boolean {
  return /reconnect|calendar access|insufficient|permission/i.test(msg);
}

export function CalendarView({ weekStart }: { weekStart: Date }) {
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState<Date>(() => new Date());
  const [reloadKey, setReloadKey] = useState(0);

  // 60s tick drives the current-time line.
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 60_000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const { timeMin, timeMax } = toTimeMinMax(weekStart);
    fetchCalendarWeek(timeMin, timeMax)
      .then((evts) => {
        if (!cancelled) {
          setEvents(evts);
          setLoading(false);
        }
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
  }, [weekStart, reloadKey]);

  async function handleReconnect() {
    setError(null);
    setLoading(true);
    try {
      await connectGmail();
      setReloadKey((k) => k + 1); // triggers the fetch effect
    } catch (e) {
      setError(String(e));
      setLoading(false);
    }
  }

  if (error && isScopeError(error)) {
    return (
      <div className="cal-view">
        <div className="cal-empty">
          <p>Ember needs permission to read your Google Calendar.</p>
          <button className="btn btn-accent" onClick={handleReconnect}>
            Reconnect Google
          </button>
        </div>
      </div>
    );
  }
  if (error) {
    return (
      <div className="cal-view">
        <div className="cal-empty">
          <pre className="error-text">{error}</pre>
          <button className="btn" onClick={() => setReloadKey((k) => k + 1)}>
            Retry
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="cal-view">
      {loading ? (
        <div className="cal-loading">Loading your week…</div>
      ) : (
        <WeekGrid weekStart={weekStart} events={events} now={now} />
      )}
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/CalendarView.tsx
git commit -m "feat(calendar): CalendarView container (fetch, now-tick, reconnect)" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Header — Mail | Calendar toggle + week navigation

**Files:**
- Modify: `src/components/Header.tsx`
- Modify: `src/styles/app.css`

- [ ] **Step 1: Update `src/components/Header.tsx`**

Add `CalendarDays`, `Mail`, `ChevronLeft`, `ChevronRight` to the lucide import. Add the new props and conditional rendering. The full new component:

```tsx
import {
  Flame,
  Pencil,
  RefreshCw,
  Settings as SettingsIcon,
  Sun,
  Moon,
  Inbox,
  Users,
  Bell,
  Newspaper,
  Mail,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  type LucideIcon,
} from "lucide-react";
import { useTheme, type Theme } from "../theme";
import { STREAMS, type Stream } from "../lib/streams";

const THEME_ICON: Record<Theme, LucideIcon> = { light: Sun, dark: Moon };

const STREAM_ICON: Record<Stream, LucideIcon> = {
  all: Inbox,
  people: Users,
  notifications: Bell,
  newsletters: Newspaper,
};

export type View = "mail" | "calendar";

export interface CalendarNav {
  rangeLabel: string;
  onPrev: () => void;
  onToday: () => void;
  onNext: () => void;
}

export function Header({
  busy,
  onSync,
  onCompose,
  onSettings,
  status,
  account = null,
  stream = "all",
  onSelectStream,
  view = "mail",
  onSelectView,
  calendar,
}: {
  busy: boolean;
  onSync?: () => void;
  onCompose?: () => void;
  onSettings?: () => void;
  status: string | null;
  account?: string | null;
  stream?: Stream;
  onSelectStream?: (s: Stream) => void;
  view?: View;
  onSelectView?: (v: View) => void;
  calendar?: CalendarNav;
}) {
  const { theme, cycleTheme } = useTheme();
  const ThemeIcon = THEME_ICON[theme];
  const isCal = view === "calendar";
  return (
    <header className="app-header">
      <span className="brand">
        <Flame size={20} className="brand-icon" /> Ember
      </span>

      {account && onSelectView && (
        <div className="view-toggle" role="tablist" aria-label="Mail or Calendar">
          <button
            className={view === "mail" ? "view-tab active" : "view-tab"}
            aria-current={view === "mail" ? "page" : undefined}
            onClick={() => onSelectView("mail")}
          >
            <Mail size={14} /> <span className="nav-label">Mail</span>
          </button>
          <button
            className={view === "calendar" ? "view-tab active" : "view-tab"}
            aria-current={view === "calendar" ? "page" : undefined}
            onClick={() => onSelectView("calendar")}
          >
            <CalendarDays size={14} /> <span className="nav-label">Calendar</span>
          </button>
        </div>
      )}

      {account && !isCal && (
        <nav className="header-nav">
          {STREAMS.map((s) => {
            const Icon = STREAM_ICON[s.key];
            return (
              <button
                key={s.key}
                className={s.key === stream ? "header-nav-item active" : "header-nav-item"}
                title={s.label}
                aria-current={s.key === stream ? "page" : undefined}
                onClick={() => onSelectStream?.(s.key)}
              >
                <Icon size={15} /> <span className="nav-label">{s.label}</span>
              </button>
            );
          })}
        </nav>
      )}

      {account && isCal && calendar && (
        <nav className="week-nav">
          <button className="icon-btn" aria-label="Previous week" onClick={calendar.onPrev}>
            <ChevronLeft size={16} />
          </button>
          <button className="btn" onClick={calendar.onToday}>
            Today
          </button>
          <button className="icon-btn" aria-label="Next week" onClick={calendar.onNext}>
            <ChevronRight size={16} />
          </button>
          <span className="week-range">{calendar.rangeLabel}</span>
        </nav>
      )}

      <span className="spacer" />
      {status && <span className="status-text">{status}</span>}

      {!isCal && onCompose && (
        <button className="btn" onClick={onCompose}>
          <Pencil size={15} /> <span className="nav-label">Compose</span>
        </button>
      )}
      {!isCal && onSync && (
        <button className="btn btn-accent" onClick={onSync} disabled={busy}>
          <RefreshCw size={15} className={busy ? "spin" : undefined} />
          {busy ? "Syncing…" : "Sync"}
        </button>
      )}

      {account && (
        <div className="header-account" title={account}>
          <div className="avatar">{account.charAt(0).toUpperCase()}</div>
          <span className="account-email">{account}</span>
        </div>
      )}
      {account && onSettings && (
        <button className="icon-btn" onClick={onSettings} aria-label="Settings">
          <SettingsIcon size={16} />
        </button>
      )}
      <button
        className="icon-btn"
        onClick={cycleTheme}
        aria-label={`Theme: ${theme}. Click to switch.`}
      >
        <ThemeIcon size={16} />
      </button>
    </header>
  );
}
```

- [ ] **Step 2: Append header toggle + week-nav styles to `src/styles/app.css`**

```css
/* ===== M10 Calendar — header toggle + week nav ===== */
.view-toggle { display: inline-flex; border: 1px solid var(--border-strong); border-radius: 8px; overflow: hidden; }
.view-tab {
  display: inline-flex; align-items: center; gap: 5px; padding: 5px 12px; border: 0;
  background: transparent; color: var(--text-muted); cursor: pointer; font-size: 13px;
}
.view-tab.active { background: var(--accent); color: var(--accent-contrast); }
.week-nav { display: inline-flex; align-items: center; gap: 8px; }
.week-range { font-weight: 600; color: var(--text); margin-left: 4px; }
```

- [ ] **Step 3: Type-check**

Run: `npm run build`
Expected: PASS. (`lucide-react` exports `Mail`, `CalendarDays`, `ChevronLeft`, `ChevronRight`.)

- [ ] **Step 4: Commit**

```bash
git add src/components/Header.tsx src/styles/app.css
git commit -m "feat(calendar): header Mail/Calendar toggle + week navigation" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Wire into App and RUN THE MAKET in the browser

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Update imports in `src/App.tsx`**

Add to the top of the file:

```tsx
import { isTauri } from "@tauri-apps/api/core";
import { startOfWeek, addWeeks, weekRangeLabel } from "./lib/calendar";
import { CalendarView } from "./components/CalendarView";
import type { View } from "./components/Header";
```

- [ ] **Step 2: Add `view` + `weekStart` state**

Inside `App()`, after the existing `useState` declarations (e.g. after `settingsOpen`), add:

```tsx
  // M10: top-level Mail/Calendar view. Default to Calendar in browser mock mode so the
  // maket shows immediately; the Tauri app opens on Mail.
  const [view, setView] = useState<View>(isTauri() ? "mail" : "calendar");
  const [weekStart, setWeekStart] = useState<Date>(() => startOfWeek(new Date()));
```

- [ ] **Step 3: Pass the new props to the authenticated `<Header>`**

In the authenticated return (the `<Header ... />` that already has `onSettings`), add these props:

```tsx
        view={view}
        onSelectView={setView}
        calendar={{
          rangeLabel: weekRangeLabel(weekStart),
          onPrev: () => setWeekStart((w) => addWeeks(w, -1)),
          onToday: () => setWeekStart(startOfWeek(new Date())),
          onNext: () => setWeekStart((w) => addWeeks(w, 1)),
        }}
```

- [ ] **Step 4: Render `CalendarView` when in calendar view**

Replace the `<SplitView ... />` block in the authenticated return with a conditional:

```tsx
      {view === "calendar" ? (
        <CalendarView weekStart={weekStart} />
      ) : (
        <SplitView
          left={
            <MessageList
              messages={messages}
              stream={stream}
              selectedId={selectedId}
              onSelect={handleSelect}
              onArchive={handleArchive}
              onStar={toggleStar}
            />
          }
          right={
            <ReadingPane
              msg={selected}
              loadImages={settings.remote_images}
              onArchive={handleArchive}
              onTrash={handleTrash}
              onToggleStar={toggleStar}
              onMarkUnread={(m) => toggleRead(m, false)}
              onReply={handleReply}
            />
          }
        />
      )}
```

- [ ] **Step 5: Type-check**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 6: Run the maket in the browser**

Run (background): `npm run dev`
Then open `http://localhost:1420` in a browser (or drive it via the chrome-devtools MCP).

Expected (browser mock mode, `isTauri()` false):
- App renders past the connect screen (mock account in the header).
- Opens on the **Calendar** view (default in mock mode).
- The week grid shows the mock events at correct times; Thu shows the **Roadmap / Lunch w/ Sam overlap side-by-side**; the **Q3 planning** all-day bar spans Wed–Thu; the current-time line shows if today falls in the visible week.
- `‹` / `Today` / `›` change the week and the range label; events regenerate per week.
- The `Mail | Calendar` toggle switches to the mock inbox and back.

Take a screenshot for review. Fix any layout issues before committing (iterate on `WeekGrid.tsx` / CSS as needed).

- [ ] **Step 7: Commit**

```bash
git add src/App.tsx
git commit -m "feat(calendar): wire Mail/Calendar view + week nav into App (maket runs in browser)" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Backend — add the `calendar.readonly` OAuth scope

**Files:**
- Modify: `src-tauri/src/auth/mod.rs`

- [ ] **Step 1: Add the scope constant**

After the existing `SCOPE_GMAIL_MODIFY` const (line ~29), add:

```rust
// 🦀 A second OAuth scope. Adding it here means the next `connect()` requests BOTH scopes;
//    because connect() always sends `prompt=consent`, Google re-prompts and grants the new
//    scope — no migration needed for a user who reconnects.
const SCOPE_CALENDAR_READONLY: &str = "https://www.googleapis.com/auth/calendar.readonly";
```

- [ ] **Step 2: Request the scope in `connect()`**

In `connect()`, the `authorize_url(...)` builder chain currently has a single `.add_scope(...)`. Add the calendar scope right after it:

```rust
            .add_scope(Scope::new(SCOPE_GMAIL_MODIFY.into()))
            .add_scope(Scope::new(SCOPE_CALENDAR_READONLY.into()))
```

- [ ] **Step 3: Build**

Run: `cd src-tauri && cargo build`
Expected: PASS (clean build; no warnings about the new const since it's used).

- [ ] **Step 4: Commit + Rust recap**

```bash
git add src-tauri/src/auth/mod.rs
git commit -m "feat(auth): request calendar.readonly scope alongside gmail.modify" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: a `const` is a compile-time constant inlined at each use; `.add_scope()` is a builder method that appends to the OAuth request and returns the builder for chaining.

---

## Task 8: Backend — CalendarClient + calendarList (TDD)

**Files:**
- Create: `src-tauri/src/calendar/types.rs`
- Create: `src-tauri/src/calendar/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/calendar_test.rs`

- [ ] **Step 1: Register the module in `src-tauri/src/lib.rs`**

After the `pub mod gmail;` block (line ~16), add:

```rust
// 🦀 The read-only Google Calendar client, mirroring `gmail`. `pub` so integration
//    tests in `tests/calendar_test.rs` (a separate crate) can reach `ember_lib::calendar`.
pub mod calendar;
```

- [ ] **Step 2: Create `src-tauri/src/calendar/types.rs`**

```rust
// 🦀 serde "shapes": these structs mirror the JSON Google returns. `#[serde(rename = "...")]`
//    maps a camelCase JSON key to a snake_case Rust field. `Option<T>` means "the key may be
//    absent" — serde fills it with `None` instead of erroring.
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CalendarListEntry {
    pub id: String,
    pub summary: Option<String>,
    #[serde(rename = "backgroundColor")]
    pub background_color: Option<String>,
    pub selected: Option<bool>,
    pub primary: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CalendarListResponse {
    // 🦀 `#[serde(default)]` → if "items" is missing, use Vec::default() ([]) rather than failing.
    #[serde(default)]
    pub items: Vec<CalendarListEntry>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GEventDateTime {
    #[serde(rename = "dateTime")]
    pub date_time: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GEvent {
    pub id: String,
    pub summary: Option<String>,
    pub start: Option<GEventDateTime>,
    pub end: Option<GEventDateTime>,
    pub location: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EventsResponse {
    #[serde(default)]
    pub items: Vec<GEvent>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

// 🦀 The normalized event we send to the frontend. `Serialize` lets Tauri turn it into JSON.
//    `PartialEq` lets unit tests compare values with assert_eq!.
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
}
```

- [ ] **Step 3: Write the failing tests in `src-tauri/tests/calendar_test.rs`**

```rust
// 🦀 Integration tests: a separate crate, so the client is reached as `ember_lib::calendar`.
use ember_lib::calendar::CalendarClient;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
async fn list_calendars_parses_and_paginates() {
    let server = MockServer::start().await;
    // Page 1 → has nextPageToken; Page 2 → no token.
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": "personal@group", "summary": "Personal", "backgroundColor": "#b9722a", "selected": true }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{ "id": "primary", "summary": "Me", "backgroundColor": "#16a34a", "primary": true }],
            "nextPageToken": "p2"
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let cals = client.list_calendars().await.unwrap();
    assert_eq!(cals.len(), 2);
    assert_eq!(cals[0].id, "primary");
    assert_eq!(cals[1].id, "personal@group");
    assert_eq!(cals[0].background_color.as_deref(), Some("#16a34a"));
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_scope_maps_to_reconnect_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/users/me/calendarList"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let err = client.list_calendars().await.unwrap_err();
    // 🦀 We don't name the (private) AppError type — Display gives us the message string.
    assert!(err.to_string().to_lowercase().contains("reconnect"), "got: {err}");
}
```

- [ ] **Step 4: Run tests to verify they fail to compile**

Run: `cd src-tauri && cargo test --test calendar_test`
Expected: FAIL — `CalendarClient` / `list_calendars` not found.

- [ ] **Step 5: Create `src-tauri/src/calendar/mod.rs` (skeleton + get_json + list_calendars)**

```rust
// 🦀 `pub mod types;` exposes the sibling `types.rs` as `ember_lib::calendar::types`.
pub mod types;

use types::{CalendarListEntry, CalendarListResponse};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://www.googleapis.com";

// 🦀 Same shape as GmailClient: a base URL (swappable in tests), the bearer token, and a
//    reusable reqwest client (connection-pooled, cheap to hold).
pub struct CalendarClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl CalendarClient {
    pub fn new(access_token: String) -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), access_token, http: reqwest::Client::new() }
    }

    /// Point the client at a mock server in tests.
    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self { base_url, access_token, http: reqwest::Client::new() }
    }

    // 🦀 GET + bearer auth + JSON parse. We peek at the status BEFORE `error_for_status()` so a
    //    401/403 (no calendar scope) becomes a friendly, actionable AppError::Auth instead of a
    //    generic "http error: 403" — the same "inspect status first" trick GmailClient uses for 404.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.http.get(url).bearer_auth(&self.access_token).send().await?;
        if matches!(
            resp.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(AppError::Auth(
                "Calendar access not granted — reconnect Google to enable it.".into(),
            ));
        }
        let resp = resp.error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    /// All calendars in the user's list (following pagination).
    pub async fn list_calendars(&self) -> Result<Vec<CalendarListEntry>> {
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/calendar/v3/users/me/calendarList?maxResults=250",
                self.base_url
            );
            if let Some(t) = &page_token {
                url.push_str(&format!("&pageToken={t}"));
            }
            let page: CalendarListResponse = self.get_json(&url).await?;
            out.extend(page.items);
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --test calendar_test`
Expected: PASS (both `list_calendars_parses_and_paginates` and `missing_scope_maps_to_reconnect_error`).

- [ ] **Step 7: Commit + Rust recap**

```bash
git add src-tauri/src/calendar/types.rs src-tauri/src/calendar/mod.rs src-tauri/src/lib.rs src-tauri/tests/calendar_test.rs
git commit -m "feat(calendar): CalendarClient + list_calendars with reconnect-on-403" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: `matches!(value, A | B)` is a concise boolean pattern test; `#[serde(default)]` supplies a default when a JSON key is absent; integration tests in `tests/` consume the crate as an external dependency (`ember_lib::...`), which is why the module must be `pub`.

---

## Task 9: Backend — list_events + map_event (TDD)

**Files:**
- Modify: `src-tauri/src/calendar/mod.rs`
- Modify: `src-tauri/tests/calendar_test.rs`

- [ ] **Step 1: Add failing tests to `src-tauri/tests/calendar_test.rs`**

Append:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn list_events_parses_timed_and_all_day_with_query_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendar/v3/calendars/primary/events"))
        .and(query_param("singleEvents", "true"))
        .and(query_param("orderBy", "startTime"))
        .and(query_param("timeMin", "2026-06-15T00:00:00-07:00"))
        .and(query_param("timeMax", "2026-06-22T00:00:00-07:00"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "id": "t1", "summary": "Standup",
                  "start": { "dateTime": "2026-06-15T09:00:00-07:00" },
                  "end":   { "dateTime": "2026-06-15T09:30:00-07:00" } },
                { "id": "a1", "summary": "Q3 planning",
                  "start": { "date": "2026-06-17" }, "end": { "date": "2026-06-19" } }
            ]
        })))
        .mount(&server)
        .await;

    let client = CalendarClient::with_base_url("tok".into(), server.uri());
    let evts = client
        .list_events("primary", "2026-06-15T00:00:00-07:00", "2026-06-22T00:00:00-07:00")
        .await
        .unwrap();
    assert_eq!(evts.len(), 2);
    assert_eq!(evts[0].start.as_ref().unwrap().date_time.as_deref(), Some("2026-06-15T09:00:00-07:00"));
    assert_eq!(evts[1].start.as_ref().unwrap().date.as_deref(), Some("2026-06-17"));
}

#[test]
fn map_event_normalizes_and_skips_cancelled() {
    use ember_lib::calendar::map_event;
    use ember_lib::calendar::types::{GEvent, GEventDateTime};

    // timed event → all_day false, uses dateTime, color attaches, missing summary → "(no title)"
    let timed = GEvent {
        id: "t1".into(), summary: None,
        start: Some(GEventDateTime { date_time: Some("2026-06-15T09:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T09:30:00-07:00".into()), date: None }),
        location: None, status: None,
    };
    let m = map_event(timed, "primary", Some("#16a34a")).unwrap();
    assert!(!m.all_day);
    assert_eq!(m.title, "(no title)");
    assert_eq!(m.start, "2026-06-15T09:00:00-07:00");
    assert_eq!(m.color.as_deref(), Some("#16a34a"));
    assert_eq!(m.calendar_id, "primary");

    // all-day event → all_day true, uses date
    let allday = GEvent {
        id: "a1".into(), summary: Some("Q3 planning".into()),
        start: Some(GEventDateTime { date_time: None, date: Some("2026-06-17".into()) }),
        end: Some(GEventDateTime { date_time: None, date: Some("2026-06-19".into()) }),
        location: None, status: None,
    };
    let m2 = map_event(allday, "primary", None).unwrap();
    assert!(m2.all_day);
    assert_eq!(m2.start, "2026-06-17");
    assert_eq!(m2.title, "Q3 planning");

    // cancelled → None
    let cancelled = GEvent {
        id: "c1".into(), summary: Some("Old".into()),
        start: Some(GEventDateTime { date_time: Some("2026-06-15T09:00:00-07:00".into()), date: None }),
        end: Some(GEventDateTime { date_time: Some("2026-06-15T09:30:00-07:00".into()), date: None }),
        location: None, status: Some("cancelled".into()),
    };
    assert!(map_event(cancelled, "primary", None).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --test calendar_test`
Expected: FAIL — `list_events` and `map_event` not found.

- [ ] **Step 3: Implement `list_events` + `map_event` in `src-tauri/src/calendar/mod.rs`**

Update the `use types::{...}` line to include the event types and `CalendarEvent`:

```rust
use types::{CalendarEvent, CalendarListEntry, CalendarListResponse, EventsResponse, GEvent};
```

Add the `list_events` method inside `impl CalendarClient` (after `list_calendars`):

```rust
    /// Events in [time_min, time_max) for one calendar. `singleEvents=true` expands recurring
    /// events into individual instances; `orderBy=startTime` requires it. Follows pagination.
    pub async fn list_events(
        &self,
        calendar_id: &str,
        time_min: &str,
        time_max: &str,
    ) -> Result<Vec<GEvent>> {
        // 🦀 Percent-encode path + query values: calendar ids contain '@'/'#', and timeMin/Max
        //    contain ':' and '+', all of which must be escaped to stay URL-safe.
        let enc = |s: &str| -> String { url::form_urlencoded::byte_serialize(s.as_bytes()).collect() };
        let cal = enc(calendar_id);
        let (tmin, tmax) = (enc(time_min), enc(time_max));
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/calendar/v3/calendars/{}/events\
                 ?singleEvents=true&orderBy=startTime&maxResults=250&timeMin={}&timeMax={}",
                self.base_url, cal, tmin, tmax
            );
            if let Some(t) = &page_token {
                url.push_str(&format!("&pageToken={t}"));
            }
            let page: EventsResponse = self.get_json(&url).await?;
            out.extend(page.items);
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }
        Ok(out)
    }
```

Add the pure `map_event` function at the end of the file (outside `impl`):

```rust
// 🦀 Pure mapping (no I/O → trivially unit-testable). Returns None for cancelled or malformed
//    events. `all_day` is detected by the presence of `start.date` (vs `start.dateTime`). The
//    `?` on an Option returns None early when a field is missing.
pub fn map_event(ev: GEvent, calendar_id: &str, color: Option<&str>) -> Option<CalendarEvent> {
    if ev.status.as_deref() == Some("cancelled") {
        return None;
    }
    let start = ev.start?;
    let end = ev.end?;
    let all_day = start.date.is_some();
    let start_s = start.date_time.or(start.date)?;
    let end_s = end.date_time.or(end.date)?;
    Some(CalendarEvent {
        id: ev.id,
        calendar_id: calendar_id.to_string(),
        // 🦀 filter() drops an empty summary so it falls through to the default title.
        title: ev.summary.filter(|s| !s.is_empty()).unwrap_or_else(|| "(no title)".to_string()),
        start: start_s,
        end: end_s,
        all_day,
        location: ev.location,
        color: color.map(|c| c.to_string()),
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --test calendar_test`
Expected: PASS (all four tests).

- [ ] **Step 5: Commit + Rust recap**

```bash
git add src-tauri/src/calendar/mod.rs src-tauri/tests/calendar_test.rs
git commit -m "feat(calendar): list_events + pure map_event (all-day/cancelled handling)" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: `?` on `Option<T>` short-circuits to `None` (just like it does for `Result`/`Err`); `Option::or()` picks the first `Some`, letting `date_time.or(date)` mean "timed value, else all-day value"; a free `pub fn` (no `&self`) is the idiomatic home for pure logic you want to test without constructing a client.

---

## Task 10: Backend — the `fetch_calendar_week` command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

This command orchestrates over the network (auth + concurrent fetch); like the other I/O commands it's covered by the unit-tested pieces (`map_event`, the client) plus manual E2E in Task 11. No new automated test here.

- [ ] **Step 1: Add imports + concurrency const to `src-tauri/src/commands.rs`**

Near the existing `use crate::gmail::GmailClient;` add:

```rust
use crate::calendar::types::CalendarEvent;
use crate::calendar::{map_event, CalendarClient};
```

Near `const PREVIEW_CONCURRENCY: usize = 8;` add:

```rust
const CALENDAR_CONCURRENCY: usize = 6;
```

- [ ] **Step 2: Add the command (after `get_reply_context`, before the settings commands)**

```rust
/// Fetch the user's events for the week window [time_min, time_max) (RFC3339 strings from the
/// frontend, in local time). Reads all *selected* calendars concurrently, merges, and sorts.
/// DB-free — calendar data is fetched live, not cached.
#[tauri::command]
pub async fn fetch_calendar_week(time_min: String, time_max: String) -> Result<Vec<CalendarEvent>> {
    let stored = ensure_access_token().await?;
    let client = CalendarClient::new(stored.access_token);

    // 🦀 Google omits `selected` on the primary calendar; treat "absent" as shown. We only
    //    drop calendars the user has explicitly hidden (`selected == Some(false)`).
    let shown: Vec<_> = client
        .list_calendars()
        .await?
        .into_iter()
        .filter(|c| c.selected != Some(false))
        .collect();

    // 🦀 Borrow the client + window once; `async move` then copies these references (which are
    //    Copy) into each per-calendar future, so all futures can run concurrently.
    let client_ref = &client;
    let tmin: &str = &time_min;
    let tmax: &str = &time_max;

    use futures::stream::StreamExt;
    let results = futures::stream::iter(shown.into_iter())
        .map(|cal| async move {
            let color = cal.background_color.clone();
            let events = client_ref.list_events(&cal.id, tmin, tmax).await?;
            let mapped: Vec<CalendarEvent> = events
                .into_iter()
                .filter_map(|e| map_event(e, &cal.id, color.as_deref()))
                .collect();
            Ok::<Vec<CalendarEvent>, AppError>(mapped)
        })
        .buffer_unordered(CALENDAR_CONCURRENCY)
        .collect::<Vec<Result<Vec<CalendarEvent>>>>()
        .await;

    let mut all = Vec::new();
    for r in results {
        match r {
            Ok(evts) => all.extend(evts),
            // 🦀 An auth/scope error must surface so the UI can prompt reconnect; other
            //    per-calendar failures are skipped (one broken calendar ≠ whole-week failure).
            Err(AppError::Auth(m)) => return Err(AppError::Auth(m)),
            Err(_) => {}
        }
    }
    // Best-effort ordering; the frontend re-sorts/positions by parsed local time.
    all.sort_by(|a, b| a.start.cmp(&b.start));
    Ok(all)
}
```

- [ ] **Step 3: Register the command in `src-tauri/src/lib.rs`**

In the `tauri::generate_handler![...]` list, add after `commands::get_reply_context,`:

```rust
            commands::fetch_calendar_week,
```

- [ ] **Step 4: Build + full test + clippy**

Run: `cd src-tauri && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS — builds, all tests green, clippy clean.

- [ ] **Step 5: Commit + Rust recap**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): fetch_calendar_week (selected calendars, concurrent, merged)" \
  -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Rust recap: `futures::stream::iter(...).map(async closure).buffer_unordered(N)` runs up to N futures at once and yields each as it finishes — the same bounded-concurrency pattern as `get_message_previews`. `filter_map` keeps the `Some` results and drops `None`s in one pass.

---

## Task 11: Final verification + live E2E

**Files:** none (verification only).

- [ ] **Step 1: Full gate**

Run: `cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings`
Run: `npm run build`
Expected: all green.

- [ ] **Step 2: Live E2E in the Tauri app**

Run: `npm run tauri dev`
- If a previously-connected account exists (token predates the new scope): open **Calendar** → expect the **Reconnect Google** empty-state → click it → complete Google consent (now Gmail + Calendar) → the week populates with real events.
- If no account: **Connect Gmail** (consent now lists Calendar too) → open Calendar.
- Verify: current week shows real timed + all-day events at correct local times; `‹`/`Today`/`›` navigate weeks; the current-time line is on today; overlapping events render side-by-side; switching back to **Mail** still works (inbox, read/star/archive/compose unaffected).

- [ ] **Step 3: Manual maket re-check (optional)**

Run: `npm run dev` → browser → confirm mock calendar still renders (regression guard for the `isTauri()` branches).

- [ ] **Step 4: No commit needed** unless E2E surfaced a fix. If a fix was made, commit it with a clear `fix(calendar): …` message + the Co-Authored-By trailer.

---

## Post-implementation (handled outside this plan)

- Update the wiki roadmap (`wiki/entities/ember.md`) M10 entry from "current/next" to "done, merged", noting what shipped and the carried deferrals — via the `llm-wiki` ingest workflow.
- Use `superpowers:finishing-a-development-branch` to merge `m10-calendar` (the project squash-merges milestones, e.g. "Merge M9: …").
- Update auto-memory `MEMORY.md` ember entry: M10 merged.
```
