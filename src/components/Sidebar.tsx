import { Inbox, Star, Send, Archive, Trash2, type LucideIcon } from "lucide-react";

const FOLDERS: { icon: LucideIcon; label: string }[] = [
  { icon: Star, label: "Starred" },
  { icon: Send, label: "Sent" },
  { icon: Archive, label: "Archive" },
  { icon: Trash2, label: "Trash" },
];

export function Sidebar({
  account,
  count,
}: {
  account: string | null;
  count: number;
}) {
  return (
    <aside className="sidebar">
      <button className="nav-item active">
        <Inbox size={16} /> Inbox <span className="nav-count">{count}</span>
      </button>
      {FOLDERS.map((f) => {
        const Icon = f.icon;
        return (
          <button key={f.label} className="nav-item" disabled>
            <Icon size={16} /> {f.label} <span className="soon-tag">soon</span>
          </button>
        );
      })}
      <span className="sidebar-spacer" />
      {account && (
        <div className="account">
          <div className="avatar">{account.charAt(0).toUpperCase()}</div>
          <span className="account-email">{account}</span>
        </div>
      )}
    </aside>
  );
}
