import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CalendarEvent } from "./calendar";
import { MOCK_ACCOUNT, MOCK_MESSAGES, MOCK_SYNC, mockCalendarWeek, mockSearch, mockFolder, mockGetDraft, mockSaveDraft, MOCK_LABELS, mockFetchLabel, mockMessageBody } from "./mock";

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
  /** Recipient (To header). Shown instead of `from` in the Sent folder. */
  to_addr: string;
  draft_id?: string;
}

export const connectGmail = (): Promise<string> =>
  invoke<string>("connect_gmail");
export const getConnectedAccount = (): Promise<string | null> =>
  isTauri() ? invoke<string | null>("get_connected_account") : Promise.resolve(MOCK_ACCOUNT);

export const fetchInboxPreview = (max = 20): Promise<MessagePreview[]> =>
  isTauri() ? invoke<MessagePreview[]>("fetch_inbox_preview", { max }) : Promise.resolve(MOCK_MESSAGES);

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
  attachment_paths: string[];
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
  });

export const getReplyContext = (id: string): Promise<ReplyContext> =>
  invoke<ReplyContext>("get_reply_context", { id });

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

// A save payload is a send payload plus the draft id (null when creating a new draft).
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
export const disconnect = (): Promise<void> => invoke<void>("disconnect");

export const fetchCalendarWeek = (timeMin: string, timeMax: string): Promise<CalendarEvent[]> =>
  isTauri()
    ? invoke<CalendarEvent[]>("fetch_calendar_week", { timeMin, timeMax })
    : Promise.resolve(mockCalendarWeek(timeMin, timeMax));
