// src/lib/folders.ts — the mailbox folders shown in the left rail (M12).
export type Folder = "inbox" | "sent" | "starred" | "archive" | "trash" | "spam";

export interface FolderDef {
  key: Folder;
  label: string;
}

export const FOLDERS: FolderDef[] = [
  { key: "inbox", label: "Inbox" },
  { key: "sent", label: "Sent" },
  { key: "starred", label: "Starred" },
  { key: "archive", label: "Archive" },
  { key: "trash", label: "Trash" },
  { key: "spam", label: "Spam" },
];
