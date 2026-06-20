import { useEffect, useState } from "react";
import { type CalendarEvent, toTimeMinMax } from "../lib/calendar";
import { fetchCalendarWeek, connectGmail, listCalendars, type CalendarSummary } from "../lib/api";
import { WeekGrid } from "./WeekGrid";
import { EventModal, type EventInitial } from "./EventModal";

// The backend maps a missing calendar scope to the specific message
// "Calendar access not granted — reconnect Google to enable it." Match that phrasing
// precisely so an unrelated error that merely mentions "permission" isn't misrouted here.
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

  // Load the writable calendars for the create form's picker (on mount + after each
  // save/reconnect via reloadKey). Silent on failure — the form falls back to "primary".
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
