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
  created_at: number;
  updated_at: number;
  summary: string;
  summary_updated_at: number;
  transcript: string;
}

export interface MeetingNoteWrite {
  calendar_id: string;
  event_id: string;
  event_title: string;
  event_start: string;
  body: string;
  transcript: string;
}

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

export const transcribeRecording = (path: string, language: string): Promise<string> =>
  isTauri()
    ? invoke<string>("transcribe_recording", { path, language })
    : Promise.resolve(mockTranscribeRecording(path, language));

export type CaptureEvent =
  | { type: "Chunk"; text: string }
  | { type: "Error"; message: string }
  | { type: "Stopped" };

export const startSystemCapture = (
  captureMic: boolean,
  language: string,
  onEvent: (e: CaptureEvent) => void,
): Promise<void> => {
  if (!isTauri()) return mockStartCapture("system", onEvent);
  const ch = new Channel<CaptureEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("start_system_capture", { captureMic, language, onEvent: ch });
};

export const stopSystemCapture = (): Promise<void> =>
  isTauri() ? invoke<void>("stop_system_capture") : mockStopCapture();

export type PrepProgress =
  | { type: "Downloading"; percent: number }
  | { type: "Loading" }
  | { type: "Ready" }
  | { type: "Error"; message: string };

export const prepareTranscription = (
  model: string,
  onProgress: (p: PrepProgress) => void,
): Promise<void> => {
  if (!isTauri()) return Promise.resolve();
  const ch = new Channel<PrepProgress>();
  ch.onmessage = onProgress;
  return invoke<void>("prepare_transcription", { model, onProgress: ch });
};
