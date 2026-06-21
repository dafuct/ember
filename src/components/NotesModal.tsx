import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { getMeetingNote, saveMeetingNote, deleteMeetingNote } from "../lib/notes";

// What the editor needs to open: the event identity + a title/start snapshot to store.
export interface NoteTarget {
  calendarId: string;
  eventId: string;
  eventTitle: string;
  eventStart: string;
}

export function NotesModal({
  target,
  onClose,
  onSaved,
}: {
  target: NoteTarget;
  onClose: () => void;
  onSaved: () => void; // reload the panel + dots
}) {
  const [body, setBody] = useState("");
  const [exists, setExists] = useState(false); // a note already stored → show Delete
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Esc closes (matches EventModal/ComposeModal — window listener, no backdrop close).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Load any existing note for this event on open.
  useEffect(() => {
    let cancelled = false;
    getMeetingNote(target.calendarId, target.eventId)
      .then((n) => {
        if (cancelled) return;
        setBody(n?.body ?? "");
        setExists(!!n);
        setLoading(false);
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
  }, [target.calendarId, target.eventId]);

  async function handleSave() {
    if (body.trim() === "") return; // Save is disabled when empty; guard regardless
    setBusy(true);
    setError(null);
    try {
      await saveMeetingNote({
        calendar_id: target.calendarId,
        event_id: target.eventId,
        event_title: target.eventTitle,
        event_start: target.eventStart,
        body,
      });
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (!window.confirm("Delete this note?")) return;
    setBusy(true);
    setError(null);
    try {
      await deleteMeetingNote(target.calendarId, target.eventId);
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
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="note-title">
        <div className="compose-head">
          <span className="compose-title" id="note-title">Notes — {target.eventTitle}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="note-when">{new Date(target.eventStart).toLocaleString()}</div>
        {loading ? (
          <div className="cal-loading">Loading…</div>
        ) : (
          <textarea
            className="compose-body"
            placeholder="Write meeting notes…"
            value={body}
            onChange={(e) => setBody(e.target.value)}
            rows={12}
            autoFocus
          />
        )}
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {exists && (
            <button className="btn btn-danger-outline" onClick={handleDelete} disabled={busy}>
              Delete
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            className="btn btn-accent"
            onClick={handleSave}
            disabled={busy || body.trim() === ""}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
