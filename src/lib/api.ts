import { invoke } from "@tauri-apps/api/core";

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
  invoke<string | null>("get_connected_account");
export const fetchInboxPreview = (max = 20): Promise<MessagePreview[]> =>
  invoke<MessagePreview[]>("fetch_inbox_preview", { max });
export interface SyncSummary {
  added: number;
  removed: number;
}

export const syncInbox = (): Promise<SyncSummary> =>
  invoke<SyncSummary>("sync_inbox");

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
