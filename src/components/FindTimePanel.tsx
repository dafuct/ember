import { useEffect, useState } from "react";
import { findMeetingTimes, type FindTimesResult, type Slot } from "../lib/api";
import { rfc3339Local } from "../lib/calendar";

const WORK_START = 9;
const WORK_END = 18;
const HOURS = Array.from({ length: WORK_END - WORK_START }, (_, i) => WORK_START + i);

function minutesFromStart(iso: string): number {
  const d = new Date(iso);
  return (d.getHours() - WORK_START) * 60 + d.getMinutes();
}
const TOTAL_MIN = (WORK_END - WORK_START) * 60;

export function FindTimePanel({
  attendees,
  day,
  durationMin,
  onPick,
}: {
  attendees: string[];
  day: string;
  durationMin: number;
  onPick: (slot: Slot) => void;
}) {
  const [data, setData] = useState<FindTimesResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dayOffset, setDayOffset] = useState(0);
  const shownDay = (() => {
    const d = new Date(`${day}T00:00:00`);
    d.setDate(d.getDate() + dayOffset);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  })();

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const timeMin = rfc3339Local(shownDay, "00:00");
    const timeMax = rfc3339Local(shownDay, "23:59");
    findMeetingTimes(attendees, timeMin, timeMax, durationMin)
      .then((r) => !cancelled && setData(r))
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [attendees, shownDay, durationMin]);

  if (loading) return <div className="find-time"><p className="subtitle">Checking availability…</p></div>;
  if (error) return <div className="find-time"><p className="compose-error">{error}</p></div>;
  if (!data) return null;

  const total = data.grid.length;
  const available = total - data.unavailable.length;

  return (
    <div className="find-time">
      <div className="ft-daypager">
        <button type="button" aria-label="Previous day" onClick={() => setDayOffset((o) => o - 1)}>‹</button>
        <span>{new Date(`${shownDay}T00:00:00`).toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" })}</span>
        <button type="button" aria-label="Next day" onClick={() => setDayOffset((o) => o + 1)}>›</button>
      </div>
      <div className="ft-axis">
        {HOURS.map((h) => <span key={h}>{h}</span>)}
      </div>
      {data.grid.map((row) => (
        <div key={row.email} className={row.error ? "ft-row ft-row-unavail" : "ft-row"}>
          <span className="ft-name" title={row.email}>{row.email.split("@")[0]}</span>
          <span className="ft-strip">
            {row.error ? (
              <span className="ft-nodata">no availability</span>
            ) : (
              row.busy.map((b, i) => {
                const startMin = Math.max(0, Math.min(TOTAL_MIN, minutesFromStart(b.start)));
                const endMin = Math.max(0, Math.min(TOTAL_MIN, minutesFromStart(b.end)));
                const leftPct = (startMin / TOTAL_MIN) * 100;
                const widthPct = ((endMin - startMin) / TOTAL_MIN) * 100;
                return (
                  <span
                    key={i}
                    className="ft-busy"
                    style={{ left: `${leftPct}%`, width: `${widthPct}%` }}
                  />
                );
              })
            )}
          </span>
        </div>
      ))}

      <div className="ft-suggest-label">
        Suggested times{data.unavailable.length ? ` · based on ${available} of ${total} guests` : ""}
      </div>
      {data.suggestions.length === 0 ? (
        <p className="subtitle">No common time — try another day.</p>
      ) : (
        data.suggestions.map((s) => (
          <button key={`${s.start}/${s.end}`} type="button" className="ft-slot" onClick={() => onPick(s)}>
            {new Date(s.start).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} –{" "}
            {new Date(s.end).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </button>
        ))
      )}
    </div>
  );
}
