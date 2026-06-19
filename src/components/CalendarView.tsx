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
