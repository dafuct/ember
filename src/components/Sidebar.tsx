import { PenSquare, Inbox, Users, Bell, Newspaper, Star, Send, FileText, Archive, Trash2, AlertOctagon, Tag } from "lucide-react";
import { STREAMS, filterByStream, type Stream } from "../lib/streams";
import type { Label, MessagePreview } from "../lib/api";

const STREAM_ICON: Record<Stream, React.ReactNode> = {
  all: <Inbox size={16} />, people: <Users size={16} />, notifications: <Bell size={16} />, newsletters: <Newspaper size={16} />,
};
const FOLDER_ITEMS = [
  { key: "sent", label: "Sent", icon: <Send size={16} /> },
  { key: "drafts", label: "Drafts", icon: <FileText size={16} /> },
  { key: "archive", label: "Archive", icon: <Archive size={16} /> },
  { key: "trash", label: "Trash", icon: <Trash2 size={16} /> },
  { key: "spam", label: "Spam", icon: <AlertOctagon size={16} /> },
];

export function Sidebar({
  messages,
  stream,
  onSelectStream,
  folder,
  onSelectFolder,
  labels,
  onCompose,
}: {
  messages: MessagePreview[];
  stream: Stream;
  onSelectStream: (s: Stream) => void;
  folder: string;
  onSelectFolder: (f: string) => void;
  labels: Label[];
  onCompose: () => void;
}) {
  const inInbox = folder === "inbox";
  const unread = (s: Stream) => filterByStream(messages, s).filter((m) => m.label_ids.includes("UNREAD")).length;
  return (
    <aside className="sidebar">
      <button className="compose-btn" onClick={onCompose}><PenSquare size={16} /> Compose</button>
      <div className="sidebar-scroll">
        <div className="sb-section">Smart Inbox</div>
        {STREAMS.map((s) => {
          const n = unread(s.key);
          const active = inInbox && stream === s.key;
          return (
            <button key={s.key} className={`sb-item${active ? " active" : ""}`} onClick={() => { onSelectFolder("inbox"); onSelectStream(s.key); }}>
              <span className="sb-ic">{STREAM_ICON[s.key]}</span><span className="sb-label">{s.label}</span>
              {n > 0 && <span className="sb-count">{n}</span>}
            </button>
          );
        })}
        <div className="sb-section">Saved</div>
        <button className={`sb-item${folder === "starred" ? " active" : ""}`} onClick={() => onSelectFolder("starred")}>
          <span className="sb-ic"><Star size={16} /></span><span className="sb-label">Pinned</span>
        </button>
        <div className="sb-section">Folders</div>
        {FOLDER_ITEMS.map((f) => (
          <button key={f.key} className={`sb-item${folder === f.key ? " active" : ""}`} onClick={() => onSelectFolder(f.key)}>
            <span className="sb-ic">{f.icon}</span><span className="sb-label">{f.label}</span>
          </button>
        ))}
        {labels.length > 0 && <div className="sb-section">Labels</div>}
        {labels.map((l) => (
          <button key={l.id} className={`sb-item${folder === l.id ? " active" : ""}`} onClick={() => onSelectFolder(l.id)}>
            <span className="sb-ic"><Tag size={16} /></span><span className="sb-label">{l.name}</span>
          </button>
        ))}
      </div>
    </aside>
  );
}
