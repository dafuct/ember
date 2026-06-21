import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  getMeetingNote,
  saveMeetingNote,
  deleteMeetingNote,
  summarizeMeetingNote,
} from "../lib/notes";

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
  const [savedBody, setSavedBody] = useState(""); // the body currently persisted
  const [exists, setExists] = useState(false); // a note already stored → show Delete
  const [transcript, setTranscript] = useState(""); // M22: persisted transcript (round-tripped through save)
  const [summary, setSummary] = useState("");
  const [summaryUpdatedAt, setSummaryUpdatedAt] = useState(0);
  const [noteUpdatedAt, setNoteUpdatedAt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false); // save/delete in flight
  const [summarizing, setSummarizing] = useState(false);
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
        setSavedBody(n?.body ?? "");
        setExists(!!n);
        setTranscript(n?.transcript ?? "");
        setSummary(n?.summary ?? "");
        setSummaryUpdatedAt(n?.summary_updated_at ?? 0);
        setNoteUpdatedAt(n?.updated_at ?? 0);
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

  // A stored summary is stale if the body has been edited since it was generated.
  const stale = summary !== "" && noteUpdatedAt > summaryUpdatedAt;

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
        transcript,
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

  async function handleSummarize() {
    if (body.trim() === "") return;
    setSummarizing(true);
    setError(null);
    try {
      // Persist the current body first so the summary reflects the latest text.
      if (body !== savedBody) {
        await saveMeetingNote({
          calendar_id: target.calendarId,
          event_id: target.eventId,
          event_title: target.eventTitle,
          event_start: target.eventStart,
          body,
          transcript,
        });
        setSavedBody(body);
      }
      const n = await summarizeMeetingNote(target.calendarId, target.eventId);
      setSummary(n.summary);
      setSummaryUpdatedAt(n.summary_updated_at);
      setNoteUpdatedAt(n.updated_at);
      setExists(true);
      onSaved(); // a note now exists / changed → refresh the calendar dots + drawer
    } catch (e) {
      setError(String(e));
    } finally {
      setSummarizing(false);
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
          <>
            <textarea
              className="compose-body"
              placeholder="Write meeting notes…"
              value={body}
              onChange={(e) => setBody(e.target.value)}
              rows={10}
              autoFocus
            />
            <div className="note-summary-section">
              <div className="note-summary-head">
                <span>Summary</span>
                <button
                  className="btn"
                  onClick={handleSummarize}
                  disabled={summarizing || busy || body.trim() === ""}
                >
                  {summarizing ? "Summarizing…" : summary ? "Regenerate" : "Summarize"}
                </button>
              </div>
              {stale && (
                <div className="note-summary-stale">Notes changed since this summary — Regenerate.</div>
              )}
              {summary ? (
                <pre className="note-summary">{summary}</pre>
              ) : (
                <div className="note-summary-empty">
                  No summary yet. Click Summarize to generate one with local Ollama.
                </div>
              )}
            </div>
          </>
        )}
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {exists && (
            <button
              className="btn btn-danger-outline"
              onClick={handleDelete}
              disabled={busy || summarizing}
            >
              Delete
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={busy || summarizing}>
            Cancel
          </button>
          <button
            className="btn btn-accent"
            onClick={handleSave}
            disabled={busy || summarizing || body.trim() === ""}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
