import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  createCalendarEvent,
  updateCalendarEvent,
  deleteCalendarEvent,
  openExternal,
  type EventWrite,
  type CalendarSummary,
  type CalendarEvent,
} from "../lib/api";
import { rfc3339Local, allDayEndExclusive } from "../lib/calendar";
import { isPlausibleEmail } from "../lib/compose";
import { GuestField } from "./GuestField";

export interface EventInitial {
  calendars: CalendarSummary[];
  event?: CalendarEvent;
  startAt?: Date;
}

const fmtDate = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const fmtTime = (d: Date) =>
  `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

export function EventModal({
  initial,
  onClose,
  onSaved,
}: {
  initial: EventInitial;
  onClose: () => void;
  onSaved: () => void;
}) {
  const editing = initial.event;
  const seedStart = editing ? new Date(editing.start) : (initial.startAt ?? new Date());
  const seedEnd = editing ? new Date(editing.end) : new Date(seedStart.getTime() + 60 * 60 * 1000);
  const writableCals = initial.calendars.filter((c) => c.writable);

  const [title, setTitle] = useState(editing?.title ?? "");
  const [allDay, setAllDay] = useState(editing?.all_day ?? false);
  const [date, setDate] = useState(fmtDate(seedStart));
  const [endDate, setEndDate] = useState(fmtDate(seedEnd));
  const [startTime, setStartTime] = useState(fmtTime(seedStart));
  const [endTime, setEndTime] = useState(fmtTime(seedEnd));
  const [location, setLocation] = useState(editing?.location ?? "");
  const [description, setDescription] = useState(editing?.description ?? "");
  const [guests, setGuests] = useState<string[]>((editing?.attendees ?? []).map((a) => a.email));
  const [tab, setTab] = useState<"details" | "find">("details");
  const [calendarId, setCalendarId] = useState(
    editing?.calendar_id ?? writableCals.find((c) => c.primary)?.id ?? writableCals[0]?.id ?? "primary",
  );
  const [addMeet, setAddMeet] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function buildWrite(): EventWrite | string {
    if (title.trim() === "") return "A title is required.";
    const emails = guests;
    if (emails.length > 0 && !emails.every(isPlausibleEmail)) return "One of the guest emails looks invalid.";
    let start: string;
    let end: string;
    if (allDay) {
      if (endDate < date) return "End date is before the start date.";
      start = date;
      end = allDayEndExclusive(endDate);
    } else {
      start = rfc3339Local(date, startTime);
      end = rfc3339Local(endDate, endTime);
      if (new Date(end).getTime() <= new Date(start).getTime()) return "End must be after start.";
    }
    return {
      title: title.trim(),
      start,
      end,
      all_day: allDay,
      description: description.trim() || null,
      location: location.trim() || null,
      attendees: emails,
    };
  }

  async function handleSave() {
    const w = buildWrite();
    if (typeof w === "string") {
      setError(w);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (editing) await updateCalendarEvent(editing.calendar_id, editing.id, w);
      else await createCalendarEvent(calendarId, w, addMeet);
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (!editing) return;
    setBusy(true);
    setError(null);
    try {
      await deleteCalendarEvent(editing.calendar_id, editing.id);
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="compose-overlay">
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="event-title">
        <div className="compose-head">
          <span className="compose-title" id="event-title">{editing ? "Edit event" : "New event"}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}><X size={16} /></button>
        </div>
        <div className="event-tabs">
          <button type="button" className={tab === "details" ? "event-tab on" : "event-tab"} onClick={() => setTab("details")}>Details</button>
          <button type="button" className={tab === "find" ? "event-tab on" : "event-tab"} onClick={() => setTab("find")} disabled={allDay}>Find a time</button>
        </div>
        {tab === "details" && (
          <>
            <input className="compose-field" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus />
            <label className="event-row">
              <input type="checkbox" checked={allDay} onChange={(e) => setAllDay(e.target.checked)} /> All day
            </label>
            <div className="event-row">
              <input type="date" className="compose-field" value={date} onChange={(e) => setDate(e.target.value)} />
              {!allDay && <input type="time" className="compose-field" value={startTime} onChange={(e) => setStartTime(e.target.value)} />}
            </div>
            <div className="event-row">
              <input type="date" className="compose-field" value={endDate} onChange={(e) => setEndDate(e.target.value)} />
              {!allDay && <input type="time" className="compose-field" value={endTime} onChange={(e) => setEndTime(e.target.value)} />}
            </div>
            <input className="compose-field" placeholder="Location" value={location} onChange={(e) => setLocation(e.target.value)} />
            <GuestField value={guests} onChange={setGuests} />
            <textarea className="compose-body" placeholder="Description" value={description} onChange={(e) => setDescription(e.target.value)} rows={4} />
            <div className="event-row">
              <select className="compose-field" value={calendarId} onChange={(e) => setCalendarId(e.target.value)} disabled={!!editing}>
                {writableCals.map((c) => (
                  <option key={c.id} value={c.id}>{c.summary}{c.primary ? " (primary)" : ""}</option>
                ))}
              </select>
            </div>
            {editing ? (
              editing.meet_link ? <button type="button" className="event-meet event-meet-btn" onClick={() => openExternal(editing.meet_link!)}>Join Google Meet</button> : null
            ) : (
              <label className="event-row">
                <input type="checkbox" checked={addMeet} onChange={(e) => setAddMeet(e.target.checked)} /> Add Google Meet
              </label>
            )}
          </>
        )}
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {editing && (
            <button className="btn btn-danger-outline" onClick={handleDelete} disabled={busy}>Delete</button>
          )}
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn btn-accent" onClick={handleSave} disabled={busy}>{busy ? "Saving…" : "Save"}</button>
        </div>
      </div>
    </div>
  );
}
