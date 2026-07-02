import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getMeetingNote,
  saveMeetingNote,
  deleteMeetingNote,
  summarizeMeetingNote,
  readTranscriptFile,
  transcribeRecording,
  startSystemCapture,
  stopSystemCapture,
  prepareTranscription,
} from "../lib/notes";

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
  onSaved: () => void;
}) {
  const [body, setBody] = useState("");
  const [savedBody, setSavedBody] = useState("");
  const [transcript, setTranscript] = useState("");
  const [savedTranscript, setSavedTranscript] = useState("");
  const [exists, setExists] = useState(false);
  const [summary, setSummary] = useState("");
  const [summaryUpdatedAt, setSummaryUpdatedAt] = useState(0);
  const [noteUpdatedAt, setNoteUpdatedAt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [summarizing, setSummarizing] = useState(false);
  const [importing, setImporting] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [recording, setRecording] = useState(false);
  const [captureMic, setCaptureMic] = useState(true);
  const [lang, setLang] = useState(
    () => localStorage.getItem("ember.transcribeLang") || "auto",
  );
  const [model, setModel] = useState(
    () => localStorage.getItem("ember.transcribeModel") || "medium",
  );
  const [error, setError] = useState<string | null>(null);
  const [prepMsg, setPrepMsg] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const recordingRef = useRef(false);
  recordingRef.current = recording;
  useEffect(() => {
    return () => {
      if (recordingRef.current) void stopSystemCapture();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    getMeetingNote(target.calendarId, target.eventId)
      .then((n) => {
        if (cancelled) return;
        setBody(n?.body ?? "");
        setSavedBody(n?.body ?? "");
        setTranscript(n?.transcript ?? "");
        setSavedTranscript(n?.transcript ?? "");
        setExists(!!n);
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

  const hasContent = body.trim() !== "" || transcript.trim() !== "";
  const stale = summary !== "" && noteUpdatedAt > summaryUpdatedAt;

  function writePayload() {
    return {
      calendar_id: target.calendarId,
      event_id: target.eventId,
      event_title: target.eventTitle,
      event_start: target.eventStart,
      body,
      transcript,
    };
  }

  async function handleSave() {
    if (!hasContent) return;
    setBusy(true);
    setError(null);
    try {
      await saveMeetingNote(writePayload());
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

  async function handleImport() {
    setImporting(true);
    setError(null);
    try {
      let path: string | null;
      if (isTauri()) {
        const sel = await open({ filters: [{ name: "Transcript", extensions: ["txt", "vtt"] }] });
        path = typeof sel === "string" ? sel : null;
      } else {
        path = "/mock/transcript.vtt";
      }
      if (!path) return;
      const text = await readTranscriptFile(path);
      setTranscript(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  async function ensureReady(): Promise<boolean> {
    setPrepMsg("Setting up transcription…");
    try {
      await prepareTranscription(model, (p) => {
        if (p.type === "Downloading") setPrepMsg(`Downloading speech model… ${p.percent}%`);
        else if (p.type === "Loading") setPrepMsg("Loading model…");
        else if (p.type === "Ready") setPrepMsg(null);
        else if (p.type === "Error") setError(p.message);
      });
      setPrepMsg(null);
      return true;
    } catch (e) {
      setPrepMsg(null);
      setError(String(e));
      return false;
    }
  }

  async function handleTranscribe() {
    setTranscribing(true);
    setError(null);
    try {
      let path: string | null;
      if (isTauri()) {
        const sel = await open({
          filters: [
            { name: "Recording", extensions: ["wav", "mp3", "m4a", "mp4", "mov", "webm", "ogg", "flac", "aac"] },
          ],
        });
        path = typeof sel === "string" ? sel : null;
      } else {
        path = "/mock/recording.m4a";
      }
      if (!path) return;
      if (!(await ensureReady())) return;
      const text = await transcribeRecording(path, lang);
      setTranscript(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setTranscribing(false);
    }
  }

  async function handleRecord() {
    setError(null);
    if (!(await ensureReady())) return;
    setRecording(true);
    try {
      await startSystemCapture(captureMic, lang, (e) => {
        if (e.type === "Chunk") {
          setTranscript((t) => (t ? t + "\n" : "") + e.text);
        } else if (e.type === "Error") {
          setError(e.message);
        } else if (e.type === "Stopped") {
          setRecording(false);
        }
      });
    } catch (err) {
      setError(String(err));
      setRecording(false);
    }
  }

  async function handleStop() {
    try {
      await stopSystemCapture();
    } catch (err) {
      setError(String(err));
    }
    setRecording(false);
  }

  async function handleSummarize() {
    if (!hasContent) return;
    setSummarizing(true);
    setError(null);
    try {
      if (body !== savedBody || transcript !== savedTranscript) {
        await saveMeetingNote(writePayload());
        setSavedBody(body);
        setSavedTranscript(transcript);
      }
      const n = await summarizeMeetingNote(target.calendarId, target.eventId);
      setSummary(n.summary);
      setSummaryUpdatedAt(n.summary_updated_at);
      setNoteUpdatedAt(n.updated_at);
      setExists(true);
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSummarizing(false);
    }
  }

  const blocked = busy || summarizing || importing || transcribing || recording || prepMsg !== null;

  return (
    <div className="compose-overlay">
      <div className="compose-card" role="dialog" aria-modal="true" aria-labelledby="note-title">
        <div className="compose-head">
          <span className="compose-title" id="note-title">Notes — {target.eventTitle}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <div className="note-scroll">
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
              rows={8}
              autoFocus
            />
            <div className="note-transcript-head">
              <span>Transcript</span>
              <div className="note-transcript-actions">
                <select
                  className="note-select"
                  aria-label="Transcription language"
                  value={lang}
                  disabled={blocked}
                  onChange={(e) => {
                    setLang(e.target.value);
                    localStorage.setItem("ember.transcribeLang", e.target.value);
                  }}
                >
                  <option value="auto">Auto-detect</option>
                  <option value="uk">Ukrainian</option>
                  <option value="en">English</option>
                  <option value="ru">Russian</option>
                  <option value="de">German</option>
                  <option value="es">Spanish</option>
                  <option value="fr">French</option>
                  <option value="pl">Polish</option>
                </select>
                <select
                  className="note-select"
                  aria-label="Transcription model"
                  value={model}
                  disabled={blocked}
                  onChange={(e) => {
                    setModel(e.target.value);
                    localStorage.setItem("ember.transcribeModel", e.target.value);
                  }}
                >
                  <option value="medium">Standard — medium (1.5 GB)</option>
                  <option value="large-v3-turbo">High accuracy — large-v3-turbo (1.6 GB)</option>
                </select>
                <button className="btn" onClick={handleImport} disabled={blocked}>
                  {importing ? "Importing…" : "Import…"}
                </button>
                <button className="btn" onClick={handleTranscribe} disabled={blocked}>
                  {transcribing ? "Transcribing…" : "Transcribe…"}
                </button>
              </div>
            </div>
            <textarea
              className="compose-body"
              placeholder="Paste a transcript, or Import a .txt / .vtt…"
              value={transcript}
              onChange={(e) => setTranscript(e.target.value)}
              rows={6}
            />
            <div className="note-capture-row">
              <label className="note-mic-toggle">
                <input
                  type="checkbox"
                  checked={captureMic}
                  onChange={(e) => setCaptureMic(e.target.checked)}
                  disabled={blocked}
                />
                Also capture my voice
              </label>
              {recording ? (
                <button className="btn btn-danger-outline" onClick={handleStop}>
                  Stop
                </button>
              ) : (
                <button className="btn" onClick={handleRecord} disabled={blocked}>
                  Record
                </button>
              )}
              {recording && <span className="note-capture-pulse">● listening…</span>}
              {prepMsg && <span className="note-capture-pulse">{prepMsg}</span>}
            </div>
            <div className="note-capture-help">
              Records the meeting's audio with no setup. macOS will ask for <b>Screen Recording</b>
              {captureMic ? " and Microphone" : ""} permission the first time — after enabling it
              in System Settings, quit and reopen Ember, then Record again.
            </div>
            {model === "large-v3-turbo" && (
              <div className="note-capture-help">
                High-accuracy model downloads ~1.6 GB the first time you use it.
              </div>
            )}
            <div className="note-summary-section">
              <div className="note-summary-head">
                <span>Summary</span>
                <button className="btn" onClick={handleSummarize} disabled={blocked || !hasContent}>
                  {summarizing ? "Summarizing…" : summary ? "Regenerate" : "Summarize"}
                </button>
              </div>
              {stale && (
                <div className="note-summary-stale">Notes or transcript changed since this summary — Regenerate.</div>
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
        </div>
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          {exists && (
            <button className="btn btn-danger-outline" onClick={handleDelete} disabled={blocked}>
              Delete
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={blocked}>
            Cancel
          </button>
          <button className="btn btn-accent" onClick={handleSave} disabled={blocked || !hasContent}>
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
