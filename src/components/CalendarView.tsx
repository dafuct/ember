import { useEffect, useMemo, useState } from "react";
import { type CalendarEvent, toTimeMinMax } from "../lib/calendar";
import { fetchCalendarWeek, connectGmail, listCalendars, type CalendarSummary } from "../lib/api";
import { listMeetingNotes, noteKey, type MeetingNote } from "../lib/notes";
import { WeekGrid } from "./WeekGrid";
import { EventModal, type EventInitial } from "./EventModal";
import { NotesModal, type NoteTarget } from "./NotesModal";
import { NotebookPen } from "lucide-react";

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

  // Meeting notes (M20): local-only, separate from the live calendar fetch.
  const [notes, setNotes] = useState<MeetingNote[]>([]);
  const [notesReloadKey, setNotesReloadKey] = useState(0);
  const [noteTarget, setNoteTarget] = useState<NoteTarget | null>(null);
  const [notesPanelOpen, setNotesPanelOpen] = useState(false);

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

  // Load all notes (on mount + after any save/delete). Silent on failure.
  useEffect(() => {
    listMeetingNotes().then(setNotes).catch(() => setNotes([]));
  }, [notesReloadKey]);

  // Set of `${calendar_id}|${event_id}` keys for the week grid's has-notes dot.
  const notesByKey = useMemo(
    () => new Set(notes.map((n) => noteKey(n.calendar_id, n.event_id))),
    [notes],
  );

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
  const reloadNotes = () => setNotesReloadKey((k) => k + 1);
  const openNew = (startAt?: Date) => setModal({ calendars, startAt });
  const openEdit = (ev: CalendarEvent) => { setDetail(null); setModal({ calendars, event: ev }); };
  const openNotesForEvent = (ev: CalendarEvent) => {
    setDetail(null);
    setNoteTarget({ calendarId: ev.calendar_id, eventId: ev.id, eventTitle: ev.title, eventStart: ev.start });
  };
  const openNotesForNote = (n: MeetingNote) =>
    setNoteTarget({ calendarId: n.calendar_id, eventId: n.event_id, eventTitle: n.event_title, eventStart: n.event_start });

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
        <button
          className={notesPanelOpen ? "btn btn-toggle active" : "btn btn-toggle"}
          aria-pressed={notesPanelOpen}
          onClick={() => setNotesPanelOpen((o) => !o)}
        >
          <NotebookPen size={15} /> Notes
        </button>
      </div>
      <div className="cal-stage">
        {loading ? (
          <div className="cal-loading">Loading your week…</div>
        ) : (
          <WeekGrid
            weekStart={weekStart}
            events={events}
            now={now}
            notesByKey={notesByKey}
            onSlotClick={openNew}
            onEventClick={setDetail}
          />
        )}

        {notesPanelOpen && (
          <aside className="notes-drawer" aria-label="Meeting notes">
            <div className="notes-drawer-head">Meeting notes</div>
            {notes.length === 0 ? (
              <div className="notes-empty">No meeting notes yet.</div>
            ) : (
              <ul className="notes-list">
                {notes.map((n) => (
                  <li key={n.id}>
                    <button className="notes-row" onClick={() => openNotesForNote(n)}>
                      <span className="notes-row-title">{n.event_title || "(untitled event)"}</span>
                      <span className="notes-row-date">{new Date(n.event_start).toLocaleDateString()}</span>
                      <span className="notes-row-snippet">{n.body.split("\n")[0]}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </aside>
        )}
      </div>

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
              <button className="btn" onClick={() => openNotesForEvent(detail)}>
                <NotebookPen size={15} /> Notes
              </button>
              <button className="btn btn-accent" onClick={() => openEdit(detail)}>Edit</button>
            </div>
          </div>
        </div>
      )}

      {modal && (
        <EventModal initial={modal} onClose={() => setModal(null)} onSaved={refetch} />
      )}

      {noteTarget && (
        <NotesModal target={noteTarget} onClose={() => setNoteTarget(null)} onSaved={reloadNotes} />
      )}
    </div>
  );
}
