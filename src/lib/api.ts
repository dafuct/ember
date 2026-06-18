import { invoke } from "@tauri-apps/api/core";

export interface MessagePreview {
  id: string;
  from: string;
  subject: string;
  date: string;
  snippet: string;
}

export const connectGmail = () => invoke<string>("connect_gmail");
export const getConnectedAccount = () =>
  invoke<string | null>("get_connected_account");
export const fetchInboxPreview = (max = 20) =>
  invoke<MessagePreview[]>("fetch_inbox_preview", { max });
