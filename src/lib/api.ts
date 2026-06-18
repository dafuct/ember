import { invoke } from "@tauri-apps/api/core";

export interface MessagePreview {
  id: string;
  thread_id: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
  internal_date: number;
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
