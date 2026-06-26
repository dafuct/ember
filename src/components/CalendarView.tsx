import { useEffect, useMemo, useState } from "react";
import { type CalendarEvent, toTimeMinMax } from "../lib/calendar";
import { fetchCalendarWeek, connectGmail, listCalendars, openExternal, respondToEvent, type CalendarSummary } from "../lib/api";
import { listMeetingNotes, noteKey, type MeetingNote } from "../lib/notes";
import { WeekGrid } from "./WeekGrid";
import { EventModal, type EventInitial } from "./EventModal";
import { NotesModal, type NoteTarget } from "./NotesModal";
import { NotebookPen, ChevronLeft, ChevronRight } from "lucide-react";

// The backend maps a missing calendar scope to the specific message
// "Calendar access not granted — reconnect Google to enable it." Match that phrasing
// precisely so an unrelated error that merely mentions "permission" isn't misrouted here.
function isScopeError(msg: string): boolean {
  return /reconnect google|calendar access not granted/i.test(msg);
}

// RSVP buttons (Google status → label).
const RSVP_CHOICES: { status: string; label: string }[] = [
  { status: "accepted", label: "Yes" },
  { status: "declined", label: "No" },
  { status: "tentative", label: "Maybe" },
];

// A guest's responseStatus → a small badge symbol + class.
function guestBadge(status?: string | null): { symbol: string; cls: string; label: string } {
  switch (status) {
    case "accepted": return { symbol: "✓", cls: "guest-status accepted", label: "accepted" };
    case "declined": return { symbol: "✗", cls: "guest-status declined", label: "declined" };
    case "tentative": return { symbol: "?", cls: "guest-status tentative", label: "maybe" };
    default: return { symbol: "–", cls: "guest-status pending", label: "no reply" };
  }
}

export function CalendarView({
  weekStart,
  onPrevWeek,
  onToday,
  onNextWeek,
  rangeLabel,
}: {
  weekStart: Date;
  onPrevWeek?: () => void;
  onToday?: () => void;
  onNextWeek?: () => void;
  rangeLabel?: string;
}) {
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState<Date>(() => new Date());
  const [reloadKey, setReloadKey] = useState(0);
  const [calendars, setCalendars] = useState<CalendarSummary[]>([]);
  const [modal, setModal] = useState<EventInitial | null>(null);
  const [detail, setDetail] = useState<CalendarEvent | null>(null);
  // RSVP state is popover-local: a failure must NOT trip the full-view `error` gate.
  const [rsvpBusy, setRsvpBusy] = useState(false);
  const [rsvpError, setRsvpError] = useState<string | null>(null);

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

  const handleRespond = async (status: string) => {
    if (!detail) return;
    setRsvpBusy(true);
    setRsvpError(null);
    try {
      const updated = await respondToEvent(detail.calendar_id, detail.id, status);
      // Merge only the fields that changed so the maket's stub event can't blank the popover.
      setDetail({ ...detail, attendees: updated.attendees, my_response_status: updated.my_response_status });
      refetch(); // re-pull the week so tiles reflect the new status
    } catch (e) {
      setRsvpError(e instanceof Error ? e.message : String(e));
    } finally {
      setRsvpBusy(false);
    }
  };

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
        {rangeLabel && (
          <nav className="week-nav" aria-label="Week navigation">
            <button className="icon-btn" aria-label="Previous week" onClick={onPrevWeek}>
              <ChevronLeft size={16} />
            </button>
            <button className="btn" onClick={onToday}>Today</button>
            <button className="icon-btn" aria-label="Next week" onClick={onNextWeek}>
              <ChevronRight size={16} />
            </button>
            <span className="week-range">{rangeLabel}</span>
          </nav>
        )}
        <span className="cal-toolbar-spacer" />
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
        <div className="event-detail-overlay" onClick={() => { setDetail(null); setRsvpError(null); }}>
          <div className="event-detail" role="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>{detail.title}</h3>
            <div className="event-detail-when">
              {new Date(detail.start).toLocaleString()} – {new Date(detail.end).toLocaleString()}
            </div>
            {detail.location && <div>{detail.location}</div>}
            {detail.description && <p className="event-detail-desc">{detail.description}</p>}
            {detail.attendees && detail.attendees.length > 0 && (
              <div className="event-detail-guests">
                {detail.attendees.map((a) => {
                  const b = guestBadge(a.response_status);
                  return (
                    <div className="guest-row" key={a.email}>
                      <span className={b.cls} title={b.label}>{b.symbol}</span>
                      <span className="guest-email">{a.email}{a.self ? " (you)" : ""}</span>
                    </div>
                  );
                })}
              </div>
            )}
            {detail.my_response_status != null && (
              <div className="event-rsvp">
                <span className="event-rsvp-label">Going?</span>
                {RSVP_CHOICES.map((c) => (
                  <button
                    key={c.status}
                    className={`rsvp-btn${detail.my_response_status === c.status ? " active" : ""}`}
                    disabled={rsvpBusy}
                    onClick={() => handleRespond(c.status)}
                  >
                    {c.label}
                  </button>
                ))}
              </div>
            )}
            {rsvpError && <div className="compose-error">{rsvpError}</div>}
            {detail.meet_link && (
              <button
                className="event-meet event-meet-btn"
                onClick={() => openExternal(detail.meet_link!)}
              >
                Join Google Meet
              </button>
            )}
            {detail.html_link && (
              <button
                className="event-open-link"
                onClick={() => openExternal(detail.html_link!)}
              >
                Open in Google Calendar
              </button>
            )}
            <div className="compose-actions">
              <button className="btn" onClick={() => { setDetail(null); setRsvpError(null); }}>Close</button>
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
