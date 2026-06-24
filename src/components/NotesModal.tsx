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
  listInputDevices,
  startCapture,
  stopCapture,
  transcriptionStatus,
  prepareTranscription,
  installBlackhole,
} from "../lib/notes";
import type { DeviceInfo, TranscriptionStatus } from "../lib/notes";

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
  const [transcript, setTranscript] = useState("");
  const [savedTranscript, setSavedTranscript] = useState(""); // the transcript currently persisted
  const [exists, setExists] = useState(false); // a note already stored → show Delete
  const [summary, setSummary] = useState("");
  const [summaryUpdatedAt, setSummaryUpdatedAt] = useState(0);
  const [noteUpdatedAt, setNoteUpdatedAt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false); // save/delete in flight
  const [summarizing, setSummarizing] = useState(false);
  const [importing, setImporting] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [prepMsg, setPrepMsg] = useState<string | null>(null); // transcription setup progress
  const [transStatus, setTransStatus] = useState<TranscriptionStatus | null>(null);
  const [installingBh, setInstallingBh] = useState(false); // BlackHole assisted install in flight
  const [bhMsg, setBhMsg] = useState<string | null>(null);

  // Esc closes (matches EventModal/ComposeModal — window listener, no backdrop close).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Load audio input devices for the capture picker (best-effort; empty list if none).
  useEffect(() => {
    listInputDevices()
      .then((ds) => {
        setDevices(ds);
        setSelectedDevice((prev) => prev || ds[0]?.name || "");
      })
      .catch(() => {});
    transcriptionStatus().then(setTransStatus).catch(() => {});
  }, []);

  // Track `recording` in a ref so the unmount cleanup sees the latest value without re-subscribing.
  const recordingRef = useRef(false);
  recordingRef.current = recording;
  // Stop any in-flight capture if the modal unmounts mid-recording (avoids a leaked worker/timer).
  useEffect(() => {
    return () => {
      if (recordingRef.current) void stopCapture();
    };
  }, []);

  // Load any existing note for this event on open.
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

  // Enough to save/summarize: any notes OR any transcript.
  const hasContent = body.trim() !== "" || transcript.trim() !== "";
  // A stored summary is stale if the note has been edited since it was generated.
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
    if (!hasContent) return; // Save is disabled when empty; guard regardless
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
        path = typeof sel === "string" ? sel : null; // null if cancelled (or a multi-array)
      } else {
        path = "/mock/transcript.vtt"; // maket: skip the native dialog
      }
      if (!path) return; // cancelled
      const text = await readTranscriptFile(path);
      setTranscript(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  }

  // Ensure the in-process transcriber is ready (downloads the model on first use), surfacing
  // progress. Returns true on success; on failure sets the error and returns false.
  async function ensureReady(): Promise<boolean> {
    setPrepMsg("Setting up transcription…");
    try {
      await prepareTranscription((p) => {
        if (p.type === "Downloading") setPrepMsg(`Downloading speech model… ${p.percent}%`);
        else if (p.type === "Loading") setPrepMsg("Loading model…");
        else if (p.type === "Ready") setPrepMsg(null);
        else if (p.type === "Error") setError(p.message);
      });
      setPrepMsg(null);
      setTransStatus(await transcriptionStatus());
      return true;
    } catch (e) {
      setPrepMsg(null);
      setError(String(e));
      return false;
    }
  }

  // Fetch + open the official BlackHole installer. On success the GUI installer takes over (asks
  // for the admin password); we re-check status so the hint clears once it's installed.
  async function handleInstallBlackhole() {
    setInstallingBh(true);
    setBhMsg(null);
    try {
      await installBlackhole();
      setBhMsg("Installer opened — follow the prompts (you'll be asked for your password), then re-select the device.");
      setTransStatus(await transcriptionStatus());
    } catch (e) {
      setBhMsg(`Couldn't fetch the installer (${String(e)}). Use the manual download link instead.`);
    } finally {
      setInstallingBh(false);
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
        path = typeof sel === "string" ? sel : null; // null if cancelled (or a multi-array)
      } else {
        path = "/mock/recording.m4a"; // maket: skip the native dialog
      }
      if (!path) return; // cancelled
      if (!(await ensureReady())) return; // download/load the model first
      const text = await transcribeRecording(path);
      setTranscript(text);
    } catch (e) {
      setError(String(e));
    } finally {
      setTranscribing(false);
    }
  }

  async function handleRecord() {
    setError(null);
    if (!(await ensureReady())) return; // download/load the model before capturing
    setRecording(true);
    try {
      await startCapture(selectedDevice, (e) => {
        if (e.type === "Chunk") {
          // Append each transcribed chunk to the transcript, newline-separated.
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
      await stopCapture();
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
      // Persist the current notes/transcript first so the summary reflects the latest text.
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
              <select
                className="note-device-select"
                aria-label="Audio input device"
                value={selectedDevice}
                onChange={(e) => setSelectedDevice(e.target.value)}
                disabled={blocked}
              >
                {devices.length === 0 && <option value="">No input devices</option>}
                {devices.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}
                  </option>
                ))}
              </select>
              {recording ? (
                <button className="btn btn-danger-outline" onClick={handleStop}>
                  Stop
                </button>
              ) : (
                <button className="btn" onClick={handleRecord} disabled={blocked || !selectedDevice}>
                  Record
                </button>
              )}
              {recording && <span className="note-capture-pulse">● listening…</span>}
              {prepMsg && <span className="note-capture-pulse">{prepMsg}</span>}
            </div>
            {transStatus && !transStatus.blackhole_present && (
              <div className="note-blackhole-hint">
                To capture the meeting's audio (not just your mic), install BlackHole and pick it as
                the input device.
                <div className="note-blackhole-actions">
                  <button className="btn" onClick={handleInstallBlackhole} disabled={installingBh}>
                    {installingBh ? "Opening installer…" : "Install BlackHole"}
                  </button>
                  <a
                    href="https://github.com/ExistentialAudio/BlackHole#installation"
                    target="_blank"
                    rel="noreferrer"
                  >
                    download manually
                  </a>
                </div>
                {bhMsg && <div className="note-blackhole-msg">{bhMsg}</div>}
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
