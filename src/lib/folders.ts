export type Folder = "inbox" | "sent" | "drafts" | "starred" | "archive" | "trash" | "spam";

export interface FolderDef {
  key: Folder;
  label: string;
}

export const FOLDERS: FolderDef[] = [
  { key: "inbox", label: "Inbox" },
  { key: "sent", label: "Sent" },
  { key: "drafts", label: "Drafts" },
  { key: "starred", label: "Starred" },
  { key: "archive", label: "Archive" },
  { key: "trash", label: "Trash" },
  { key: "spam", label: "Spam" },
];
