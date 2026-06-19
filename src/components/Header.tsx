import {
  Flame,
  Pencil,
  RefreshCw,
  Settings as SettingsIcon,
  Sun,
  Moon,
  Inbox,
  Users,
  Bell,
  Newspaper,
  type LucideIcon,
} from "lucide-react";
import { useTheme, type Theme } from "../theme";
import { STREAMS, type Stream } from "../lib/streams";

const THEME_ICON: Record<Theme, LucideIcon> = { light: Sun, dark: Moon };

const STREAM_ICON: Record<Stream, LucideIcon> = {
  all: Inbox,
  people: Users,
  notifications: Bell,
  newsletters: Newspaper,
};

export function Header({
  busy,
  onSync,
  onCompose,
  onSettings,
  status,
  account = null,
  stream = "all",
  onSelectStream,
}: {
  busy: boolean;
  onSync?: () => void;
  onCompose?: () => void;
  onSettings?: () => void;
  status: string | null;
  account?: string | null;
  stream?: Stream;
  onSelectStream?: (s: Stream) => void;
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
          {STREAMS.map((s) => {
            const Icon = STREAM_ICON[s.key];
            return (
              <button
                key={s.key}
                className={
                  s.key === stream
                    ? "header-nav-item active"
                    : "header-nav-item"
                }
                title={s.label}
                aria-current={s.key === stream ? "page" : undefined}
                onClick={() => onSelectStream?.(s.key)}
              >
                <Icon size={15} /> <span className="nav-label">{s.label}</span>
              </button>
            );
          })}
        </nav>
      )}
      <span className="spacer" />
      {status && <span className="status-text">{status}</span>}
      {onCompose && (
        <button className="btn" onClick={onCompose}>
          <Pencil size={15} /> <span className="nav-label">Compose</span>
        </button>
      )}
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
      {account && onSettings && (
        <button className="icon-btn" onClick={onSettings} aria-label="Settings">
          <SettingsIcon size={16} />
        </button>
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
