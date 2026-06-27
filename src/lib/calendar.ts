
export interface Attendee {
  email: string;
  response_status?: string | null;
  self?: boolean;
}

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
  attendees?: Attendee[];
  my_response_status?: string | null;
}

const pad = (n: number) => String(n).padStart(2, "0");

export function startOfWeek(d: Date): Date {
  const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const dow = (x.getDay() + 6) % 7;
  x.setDate(x.getDate() - dow);
  return x;
}

export function addWeeks(d: Date, n: number): Date {
  const x = new Date(d);
  x.setDate(x.getDate() + n * 7);
  return x;
}

export function weekDays(weekStart: Date): Date[] {
  return Array.from({ length: 7 }, (_, i) => {
    const x = new Date(weekStart);
    x.setDate(x.getDate() + i);
    return x;
  });
}

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

function toRfc3339Local(d: Date): string {
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "-";
  const oh = pad(Math.floor(Math.abs(off) / 60));
  const om = pad(Math.abs(off) % 60);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}${sign}${oh}:${om}`
  );
}

export function toTimeMinMax(weekStart: Date): { timeMin: string; timeMax: string } {
  return { timeMin: toRfc3339Local(weekStart), timeMax: toRfc3339Local(addWeeks(weekStart, 1)) };
}

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

export function allDayOnDay(e: CalendarEvent, d: Date): boolean {
  const ymd = toYmd(d);
  return e.start <= ymd && ymd < e.end;
}

export function eventsForDay(timed: CalendarEvent[], day: Date): CalendarEvent[] {
  return timed.filter((e) => sameLocalDay(new Date(e.start), day));
}

export interface PositionedEvent {
  ev: CalendarEvent;
  topMin: number;
  heightMin: number;
  lane: number;
  lanes: number;
}

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

export function allDayEndExclusive(ymd: string): string {
  const [y, mo, d] = ymd.split("-").map(Number);
  const dt = new Date(y, (mo || 1) - 1, (d || 1) + 1);
  return toYmd(dt);
}
