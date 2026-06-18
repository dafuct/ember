import {
  Flame,
  RefreshCw,
  Sun,
  Moon,
  Inbox,
  Star,
  Send,
  Archive,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useTheme, type Theme } from "../theme";

const THEME_ICON: Record<Theme, LucideIcon> = { light: Sun, dark: Moon };

const FOLDERS: { icon: LucideIcon; label: string }[] = [
  { icon: Star, label: "Starred" },
  { icon: Send, label: "Sent" },
  { icon: Archive, label: "Archive" },
  { icon: Trash2, label: "Trash" },
];

export function Header({
  busy,
  onSync,
  status,
  account = null,
}: {
  busy: boolean;
  onSync?: () => void;
  status: string | null;
  account?: string | null;
}) {
  const { theme, cycleTheme } = useTheme();
  const ThemeIcon = THEME_ICON[theme];
  return (
    <header className="app-header">
      <span className="brand">
        <Flame size={20} className="brand-icon" /> Ember
      </span>
      {account && (
        <nav className="header-nav">
          <span className="header-nav-item active" title="Inbox">
            <Inbox size={15} /> <span className="nav-label">Inbox</span>
          </span>
          {FOLDERS.map((f) => {
            const Icon = f.icon;
            return (
              <button
                key={f.label}
                className="header-nav-item"
                title={f.label}
                disabled
              >
                <Icon size={15} /> <span className="nav-label">{f.label}</span>
              </button>
            );
          })}
        </nav>
      )}
      <span className="spacer" />
      {status && <span className="status-text">{status}</span>}
      {onSync && (
        <button className="btn btn-accent" onClick={onSync} disabled={busy}>
          <RefreshCw size={15} className={busy ? "spin" : undefined} />
          {busy ? "Syncing…" : "Sync"}
        </button>
      )}
      {account && (
        <div className="header-account" title={account}>
          <div className="avatar">{account.charAt(0).toUpperCase()}</div>
          <span className="account-email">{account}</span>
        </div>
      )}
      <button
        className="icon-btn"
        onClick={cycleTheme}
        aria-label={`Theme: ${theme}. Click to switch.`}
      >
        <ThemeIcon size={16} />
      </button>
    </header>
  );
}
