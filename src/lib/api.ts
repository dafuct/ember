import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CalendarEvent } from "./calendar";
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek } from "./mock";

export type { CalendarEvent };

export interface MessagePreview {
  id: string;
  thread_id: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
  internal_date: number;
  /** Smart-inbox stream from the backend scorer: "people" | "notifications" | "newsletters". */
  category: string;
  /** Raw Gmail label ids (e.g. "INBOX", "UNREAD", "STARRED"). Drives read/star state. */
  label_ids: string[];
}

export const connectGmail = (): Promise<string> =>
  invoke<string>("connect_gmail");
export const getConnectedAccount = (): Promise<string | null> =>
  isTauri() ? invoke<string | null>("get_connected_account") : Promise.resolve(MOCK_ACCOUNT);

export const fetchInboxPreview = (max = 20): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("fetch_inbox_preview", { max }) : Promise.resolve(MOCK_MESSAGES);
export interface SyncSummary {
  added: number;
  removed: number;
}

export const syncInbox = (): Promise<SyncSummary> =>
  isTauri() ? invoke<SyncSummary>("sync_inbox") : Promise.resolve(MOCK_SYNC);

export interface MessageBody {
  html: string;
  is_html: boolean;
  blocked_images: boolean;
}

export const fetchMessageBody = (
  id: string,
  loadImages = false,
): Promise<MessageBody> =>
  invoke<MessageBody>("fetch_message_body", { id, loadImages });

export const setMessageRead = (id: string, read: boolean): Promise<void> =>
  invoke<void>("set_message_read", { id, read });
export const setMessageStarred = (id: string, starred: boolean): Promise<void> =>
  invoke<void>("set_message_starred", { id, starred });
export const archiveMessage = (id: string): Promise<void> =>
  invoke<void>("archive_message", { id });
export const trashMessage = (id: string): Promise<void> =>
  invoke<void>("trash_message", { id });

export interface ReplyContext {
  message_id: string;
  references: string;
  quoted_text: string;
}

export interface SendEmailPayload {
  to: string[];
  cc: string[];
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
}

export const sendEmail = (p: SendEmailPayload): Promise<void> =>
  invoke<void>("send_email", {
    to: p.to,
    cc: p.cc,
    subject: p.subject,
    body: p.body,
    inReplyTo: p.in_reply_to,
    references: p.references,
    threadId: p.thread_id,
  });

export const getReplyContext = (id: string): Promise<ReplyContext> =>
  invoke<ReplyContext>("get_reply_context", { id });

export interface Settings {
  signature: string;
  remote_images: boolean;
}

export const getSettings = (): Promise<Settings> =>
  invoke<Settings>("get_settings");
export const setSettings = (settings: Settings): Promise<void> =>
  invoke<void>("set_settings", { settings });
export const disconnect = (): Promise<void> => invoke<void>("disconnect");

export const fetchCalendarWeek = (timeMin: string, timeMax: string): Promise<CalendarEvent[]> =>
  isTauri()
    ? invoke<CalendarEvent[]>("fetch_calendar_week", { timeMin, timeMax })
    : Promise.resolve(mockCalendarWeek(timeMin, timeMax));
