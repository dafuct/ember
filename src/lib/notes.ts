// src/lib/notes.ts — meeting-note API wrappers + types. Notes are LOCAL-only (no Google).
// Every wrapper is isTauri()-gated so the browser maket runs against an in-memory mock store.
import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import {
  mockGetMeetingNote,
  mockSaveMeetingNote,
  mockDeleteMeetingNote,
  mockListMeetingNotes,
  mockSummarizeMeetingNote,
  mockReadTranscriptFile,
  mockTranscribeRecording,
  mockStartCapture,
  mockStopCapture,
} from "./mock";

export interface MeetingNote {
  id: number;
  calendar_id: string;
  event_id: string;
  event_title: string;
  event_start: string;
  body: string;
  /** Unix milliseconds. */
  created_at: number;
  /** Unix milliseconds. */
  updated_at: number;
  /** M21: local-Ollama summary (markdown). Empty = never summarized. */
  summary: string;
  /** Unix milliseconds the summary was generated (0 = never). */
  summary_updated_at: number;
  /** M22: the meeting transcript (plain text). Empty = none. */
  transcript: string;
}

// The save payload — snake_case keys to match the Rust MeetingNoteWrite (serde default).
export interface MeetingNoteWrite {
  calendar_id: string;
  event_id: string;
  event_title: string;
  event_start: string;
  body: string;
  transcript: string;
}

/** Stable composite key for the "has-notes" Set + lookups (a pipe never appears in calendar/event ids). */
export function noteKey(calendarId: string, eventId: string): string {
  return `${calendarId}|${eventId}`;
}

export const getMeetingNote = (calendarId: string, eventId: string): Promise<MeetingNote | null> =>
  isTauri()
    ? invoke<MeetingNote | null>("get_meeting_note", { calendarId, eventId })
    : Promise.resolve(mockGetMeetingNote(calendarId, eventId));

export const saveMeetingNote = (note: MeetingNoteWrite): Promise<MeetingNote> =>
  isTauri()
    ? invoke<MeetingNote>("save_meeting_note", { note })
    : Promise.resolve(mockSaveMeetingNote(note));

export const deleteMeetingNote = (calendarId: string, eventId: string): Promise<void> =>
  isTauri()
    ? invoke<void>("delete_meeting_note", { calendarId, eventId })
    : Promise.resolve(mockDeleteMeetingNote(calendarId, eventId));

export const listMeetingNotes = (): Promise<MeetingNote[]> =>
  isTauri() ? invoke<MeetingNote[]>("list_meeting_notes") : Promise.resolve(mockListMeetingNotes());

export const summarizeMeetingNote = (calendarId: string, eventId: string): Promise<MeetingNote> =>
  isTauri()
    ? invoke<MeetingNote>("summarize_meeting_note", { calendarId, eventId })
    : Promise.resolve(mockSummarizeMeetingNote(calendarId, eventId));

export const readTranscriptFile = (path: string): Promise<string> =>
  isTauri()
    ? invoke<string>("read_transcript_file", { path })
    : Promise.resolve(mockReadTranscriptFile(path));

export const transcribeRecording = (path: string): Promise<string> =>
  isTauri()
    ? invoke<string>("transcribe_recording", { path })
    : Promise.resolve(mockTranscribeRecording(path));

// M24: the streamed capture events (matches the Rust #[serde(tag = "type")] enum).
export type CaptureEvent =
  | { type: "Chunk"; text: string }
  | { type: "Error"; message: string }
  | { type: "Stopped" };

export const startCapture = (
  deviceName: string,
  onEvent: (e: CaptureEvent) => void,
): Promise<void> => {
  if (!isTauri()) return mockStartCapture(deviceName, onEvent);
  // The Tauri Channel streams CaptureEvent objects from the Rust worker to onEvent.
  const ch = new Channel<CaptureEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("start_capture", { deviceName, onEvent: ch });
};

export const stopCapture = (): Promise<void> =>
  isTauri() ? invoke<void>("stop_capture") : mockStopCapture();

// Zero-setup native capture (ScreenCaptureKit): grabs the system audio (the call) + optionally the
// mic, no BlackHole/aggregate devices. Transcript chunks stream over the same CaptureEvent channel.
export const startSystemCapture = (
  captureMic: boolean,
  onEvent: (e: CaptureEvent) => void,
): Promise<void> => {
  if (!isTauri()) return mockStartCapture("system", onEvent);
  const ch = new Channel<CaptureEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("start_system_capture", { captureMic, onEvent: ch });
};

export const stopSystemCapture = (): Promise<void> =>
  isTauri() ? invoke<void>("stop_system_capture") : mockStopCapture();

// M24+: zero-setup transcription. `prepare_transcription` downloads the speech model (first run)
// + loads the in-process Whisper engine, streaming progress over PrepProgress.
export type PrepProgress =
  | { type: "Downloading"; percent: number }
  | { type: "Loading" }
  | { type: "Ready" }
  | { type: "Error"; message: string };

export const prepareTranscription = (onProgress: (p: PrepProgress) => void): Promise<void> => {
  if (!isTauri()) return Promise.resolve();
  const ch = new Channel<PrepProgress>();
  ch.onmessage = onProgress;
  return invoke<void>("prepare_transcription", { onProgress: ch });
};
