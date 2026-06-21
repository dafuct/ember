// src/lib/notes.ts — meeting-note API wrappers + types. Notes are LOCAL-only (no Google).
// Every wrapper is isTauri()-gated so the browser maket runs against an in-memory mock store.
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  mockGetMeetingNote,
  mockSaveMeetingNote,
  mockDeleteMeetingNote,
  mockListMeetingNotes,
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
}

// The save payload — snake_case keys to match the Rust MeetingNoteWrite (serde default).
export interface MeetingNoteWrite {
  calendar_id: string;
  event_id: string;
  event_title: string;
  event_start: string;
  body: string;
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
