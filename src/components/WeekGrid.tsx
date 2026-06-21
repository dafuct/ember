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
  onSlotClick,
  onEventClick,
}: {
  weekStart: Date;
  events: CalendarEvent[];
  now: Date;
  onSlotClick?: (at: Date) => void;
  onEventClick?: (ev: CalendarEvent) => void;
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
                <div key={e.id} className="cal-allday-ev" style={tint(e)} title={e.title} onClick={() => onEventClick?.(e)}>
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
              <div key={d.toISOString()} className="cal-col"
                onClick={(e) => {
                  if (!onSlotClick) return;
                  const rect = e.currentTarget.getBoundingClientRect();
                  const min = Math.max(0, Math.round((e.clientY - rect.top) / PX_PER_MIN));
                  const at = new Date(d.getFullYear(), d.getMonth(), d.getDate(), Math.floor(min / 60), 0, 0);
                  onSlotClick(at);
                }}
              >
                {HOURS.map((h) => (
                  <div key={h} className="cal-hourline" style={{ top: h * 60 * PX_PER_MIN }} />
                ))}
                {positioned.map((p) => (
                  <div
                    key={p.ev.id}
                    className="cal-ev"
                    onClick={(e) => { e.stopPropagation(); onEventClick?.(p.ev); }}
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
