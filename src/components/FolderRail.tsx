import { Inbox, Send, FileEdit, Star, Archive, Trash2, ShieldAlert, type LucideIcon } from "lucide-react";
import { FOLDERS, type Folder } from "../lib/folders";

const ICON: Record<Folder, LucideIcon> = {
  inbox: Inbox,
  sent: Send,
  drafts: FileEdit,
  starred: Star,
  archive: Archive,
  trash: Trash2,
  spam: ShieldAlert,
};

export function FolderRail({
  folder,
  onSelectFolder,
}: {
  folder: Folder;
  onSelectFolder: (f: Folder) => void;
}) {
  return (
    <nav className="folder-rail" aria-label="Mailboxes">
      {FOLDERS.map((f) => {
        const Icon = ICON[f.key];
        return (
          <button
            key={f.key}
            className={f.key === folder ? "folder-item active" : "folder-item"}
            aria-current={f.key === folder ? "page" : undefined}
            onClick={() => onSelectFolder(f.key)}
          >
            <Icon size={18} />
            <span className="folder-label">{f.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
