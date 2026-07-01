import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CalendarEvent } from "./calendar";
import { MOCK_SYNC, mockCalendarWeek, mockSearch, mockFolder, mockGetDraft, mockSaveDraft, MOCK_LABELS, mockFetchLabel, mockMessageBody, mockReplyContext, mockCreateEvent, mockUpdateEvent, mockListCalendars, mockListAccounts, mockSetActiveAccount, mockRemoveAccount, mockGetActive, mockInboxForActive, mockCredentialStatus, mockSetCredentials, mockClearCredentials, mockRespondEvent, mockSearchPeople, mockFindMeetingTimes, mockZoomStatus, mockZoomConnect, mockZoomDisconnect } from "./mock";

export type { CalendarEvent };

export interface MessagePreview {
  id: string;
  thread_id: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
  internal_date: number;
  category: string;
  label_ids: string[];
  to_addr: string;
  draft_id?: string;
}

export const connectGmail = (): Promise<string> =>
  invoke<string>("connect_gmail");
export const getConnectedAccount = (): Promise<string | null> =>
  isTauri() ? invoke<string | null>("get_connected_account") : Promise.resolve(mockGetActive());

export const fetchInboxPreview = (max = 20): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("fetch_inbox_preview", { max }) : Promise.resolve(mockInboxForActive().slice(0, max));

export interface AccountInfo { email: string; active: boolean; unread: number }

export const listAccounts = (): Promise<AccountInfo[]> =>
  isTauri() ? invoke<AccountInfo[]>("list_accounts") : Promise.resolve(mockListAccounts());

export const setActiveAccount = (email: string): Promise<void> =>
  isTauri() ? invoke<void>("set_active_account", { email }) : (mockSetActiveAccount(email), Promise.resolve());

export const removeAccount = (email: string): Promise<string | null> =>
  isTauri() ? invoke<string | null>("remove_account", { email }) : Promise.resolve(mockRemoveAccount(email));

export const searchMessages = (query: string, max = 50): Promise<MessagePreview[]> =>
  isTauri()
    ? invoke<MessagePreview[]>("search_messages", { query, max })
    : Promise.resolve(mockSearch(query));
export const fetchFolder = (folder: string, max = 50): Promise<MessagePreview[]> =>
  isTauri()
    ? invoke<MessagePreview[]>("fetch_folder", { folder, max })
    : Promise.resolve(mockFolder(folder));
export const restoreMessage = (id: string): Promise<void> =>
  invoke<void>("restore_message", { id });
export const deleteMessageForever = (id: string): Promise<void> =>
  invoke<void>("delete_message_forever", { id });
export interface SyncSummary {
  added: number;
  removed: number;
}

export const syncInbox = (): Promise<SyncSummary> =>
  isTauri() ? invoke<SyncSummary>("sync_inbox") : Promise.resolve(MOCK_SYNC);

export interface AccountSyncSummary {
  account: string;
  added: number;
  removed: number;
  baseline: boolean;
  new_previews: MessagePreview[];
}
export const syncAllAccounts = (): Promise<AccountSyncSummary[]> =>
  isTauri() ? invoke<AccountSyncSummary[]>("sync_all_accounts") : Promise.resolve([]);

export interface Attachment {
  filename: string;
  mime_type: string;
  size: number;
  attachment_id: string;
}

export interface MessageBody {
  html: string;
  is_html: boolean;
  blocked_images: boolean;
  attachments: Attachment[];
}

export const fetchMessageBody = (
  id: string,
  loadImages = false,
): Promise<MessageBody> =>
  isTauri()
    ? invoke<MessageBody>("fetch_message_body", { id, loadImages })
    : Promise.resolve(mockMessageBody(id));

export const downloadAttachment = (
  messageId: string,
  attachmentId: string,
  destPath: string,
): Promise<void> =>
  invoke<void>("download_attachment", { messageId, attachmentId, destPath });

export const setMessageRead = (id: string, read: boolean): Promise<void> =>
  invoke<void>("set_message_read", { id, read });
export const setMessageStarred = (id: string, starred: boolean): Promise<void> =>
  invoke<void>("set_message_starred", { id, starred });

export const batchModifyMessages = (
  ids: string[],
  add: string[],
  remove: string[],
): Promise<void> =>
  isTauri() ? invoke<void>("batch_modify_messages", { ids, add, remove }) : Promise.resolve();

export const batchRestoreMessages = (ids: string[]): Promise<void> =>
  isTauri() ? invoke<void>("batch_restore_messages", { ids }) : Promise.resolve();
export const batchDeleteMessages = (ids: string[]): Promise<void> =>
  isTauri() ? invoke<void>("batch_delete_messages", { ids }) : Promise.resolve();

export interface ReplyContext {
  message_id: string;
  references: string;
  quoted_text: string;
  to: string;
  cc: string;
  attachments: Attachment[];
}

export interface ForwardedAttachmentRef {
  message_id: string;
  attachment_id: string;
  filename: string;
  mime_type: string;
}

export interface SendEmailPayload {
  to: string[];
  cc: string[];
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
  attachment_paths: string[];
  forwarded_attachments: ForwardedAttachmentRef[];
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
    attachmentPaths: p.attachment_paths,
    forwardedAttachments: p.forwarded_attachments,
  });

export const getReplyContext = (id: string): Promise<ReplyContext> =>
  isTauri()
    ? invoke<ReplyContext>("get_reply_context", { id })
    : Promise.resolve(mockReplyContext(id));

export interface DraftContent {
  draft_id: string;
  to: string;
  cc: string;
  subject: string;
  body: string;
  in_reply_to: string | null;
  references: string | null;
  thread_id: string | null;
}

export const getDraft = (id: string): Promise<DraftContent> =>
  isTauri() ? invoke<DraftContent>("get_draft", { draftId: id }) : Promise.resolve(mockGetDraft(id));

export const saveDraft = (p: SendEmailPayload & { draft_id: string | null }): Promise<string> =>
  isTauri()
    ? invoke<string>("save_draft", {
        draftId: p.draft_id,
        to: p.to,
        cc: p.cc,
        subject: p.subject,
        body: p.body,
        inReplyTo: p.in_reply_to,
        references: p.references,
        threadId: p.thread_id,
      })
    : Promise.resolve(mockSaveDraft());

export const sendDraft = (p: SendEmailPayload & { draft_id: string }): Promise<void> =>
  isTauri()
    ? invoke<void>("send_draft", {
        draftId: p.draft_id,
        to: p.to,
        cc: p.cc,
        subject: p.subject,
        body: p.body,
        inReplyTo: p.in_reply_to,
        references: p.references,
        threadId: p.thread_id,
      })
    : Promise.resolve();

export const deleteDraft = (id: string): Promise<void> =>
  isTauri() ? invoke<void>("delete_draft", { draftId: id }) : Promise.resolve();

export interface LabelColor {
  text: string;
  background: string;
}
export interface Label {
  id: string;
  name: string;
  color?: LabelColor;
}

export const listLabels = (): Promise<Label[]> =>
  isTauri() ? invoke<Label[]>("list_labels") : Promise.resolve(MOCK_LABELS);
export const createLabel = (name: string): Promise<Label> =>
  isTauri()
    ? invoke<Label>("create_label", { name })
    : Promise.resolve({ id: "Label_mock", name });
export const fetchLabel = (id: string, max = 50): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("fetch_label", { labelId: id, max }) : Promise.resolve(mockFetchLabel(id));

export interface Settings {
  signature: string;
  remote_images: boolean;
  notifications: boolean;
}

export const getSettings = (): Promise<Settings> =>
  invoke<Settings>("get_settings");
export const setSettings = (settings: Settings): Promise<void> =>
  invoke<void>("set_settings", { settings });

export interface CredentialStatus { configured: boolean; source: string }

export const googleCredentialsStatus = (): Promise<CredentialStatus> =>
  isTauri()
    ? invoke<CredentialStatus>("google_credentials_status")
    : Promise.resolve(mockCredentialStatus());

export const setGoogleCredentials = (clientId: string, clientSecret: string): Promise<void> =>
  isTauri()
    ? invoke<void>("set_google_credentials", { clientId, clientSecret })
    : (mockSetCredentials(), Promise.resolve());

export const clearGoogleCredentials = (): Promise<void> =>
  isTauri()
    ? invoke<void>("clear_google_credentials")
    : (mockClearCredentials(), Promise.resolve());

export const fetchCalendarWeek = (timeMin: string, timeMax: string): Promise<CalendarEvent[]> =>
  isTauri()
    ? invoke<CalendarEvent[]>("fetch_calendar_week", { timeMin, timeMax })
    : Promise.resolve(mockCalendarWeek(timeMin, timeMax));

export interface EventWrite {
  title: string;
  start: string;
  end: string;
  all_day: boolean;
  description: string | null;
  location: string | null;
  attendees: string[];
}

export interface CalendarSummary {
  id: string;
  summary: string;
  primary: boolean;
  writable: boolean;
}

export const listCalendars = (): Promise<CalendarSummary[]> =>
  isTauri() ? invoke<CalendarSummary[]>("list_calendars") : Promise.resolve(mockListCalendars());

export type Conferencing = "none" | "meet" | "zoom";
export interface ZoomAccount { email: string; account_id: string }

export const createCalendarEvent = (
  calendarId: string,
  event: EventWrite,
  conferencing: Conferencing,
): Promise<CalendarEvent> =>
  isTauri()
    ? invoke<CalendarEvent>("create_calendar_event", { calendarId, event, conferencing })
    : Promise.resolve(mockCreateEvent(calendarId, event, conferencing));

export const updateCalendarEvent = (
  calendarId: string,
  eventId: string,
  event: EventWrite,
): Promise<CalendarEvent> =>
  isTauri()
    ? invoke<CalendarEvent>("update_calendar_event", { calendarId, eventId, event })
    : Promise.resolve(mockUpdateEvent(calendarId, eventId, event));

export const deleteCalendarEvent = (calendarId: string, eventId: string): Promise<void> =>
  isTauri() ? invoke<void>("delete_calendar_event", { calendarId, eventId }) : Promise.resolve();

export const openExternal = (url: string): Promise<void> => {
  // Mirror the Rust `is_safe_url` check (http/https only) so the browser-maket
  // fallback can't open a non-web scheme (javascript:, file:, …). The Tauri
  // command re-checks server-side; this is defense-in-depth on the client.
  if (!/^https?:\/\//i.test(url.trim())) {
    return Promise.reject(new Error(`Refusing to open non-web URL: ${url}`));
  }
  return isTauri()
    ? invoke<void>("open_external", { url })
    : Promise.resolve(void window.open(url, "_blank", "noopener,noreferrer"));
};

export const respondToEvent = (
  calendarId: string,
  eventId: string,
  responseStatus: string,
): Promise<CalendarEvent> =>
  isTauri()
    ? invoke<CalendarEvent>("respond_to_event", { calendarId, eventId, responseStatus })
    : Promise.resolve(mockRespondEvent(calendarId, eventId, responseStatus));

export interface PersonHit { name: string; email: string; photo_url: string | null }
export interface BusySpan { start: string; end: string }
export interface PersonBusy { email: string; busy: BusySpan[]; error: string | null }
export interface Slot { start: string; end: string }
export interface FindTimesResult { grid: PersonBusy[]; suggestions: Slot[]; unavailable: string[] }

export const searchPeople = (query: string): Promise<PersonHit[]> =>
  isTauri() ? invoke<PersonHit[]>("search_people", { query }) : Promise.resolve(mockSearchPeople(query));

export const findMeetingTimes = (
  attendees: string[],
  timeMin: string,
  timeMax: string,
  durationMin: number,
): Promise<FindTimesResult> =>
  isTauri()
    ? invoke<FindTimesResult>("find_meeting_times", { attendees, timeMin, timeMax, durationMin })
    : Promise.resolve(mockFindMeetingTimes(attendees, timeMin, timeMax, durationMin));

export const zoomStatus = (): Promise<ZoomAccount | null> =>
  isTauri() ? invoke<ZoomAccount | null>("zoom_status") : Promise.resolve(mockZoomStatus());
export const zoomConnect = (): Promise<ZoomAccount> =>
  isTauri() ? invoke<ZoomAccount>("zoom_connect") : Promise.resolve(mockZoomConnect());
export const zoomDisconnect = (): Promise<void> =>
  isTauri() ? invoke<void>("zoom_disconnect") : Promise.resolve(mockZoomDisconnect());
export const zoomCredentialsStatus = (): Promise<string> =>
  isTauri() ? invoke<string>("zoom_credentials_status") : Promise.resolve("baked");
export const setZoomCredentials = (clientId: string, clientSecret: string): Promise<void> =>
  isTauri() ? invoke<void>("set_zoom_credentials", { clientId, clientSecret }) : Promise.resolve();
export const clearZoomCredentials = (): Promise<void> =>
  isTauri() ? invoke<void>("clear_zoom_credentials") : Promise.resolve();
