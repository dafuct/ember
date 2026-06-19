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
  Mail,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
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

export type View = "mail" | "calendar";

export interface CalendarNav {
  rangeLabel: string;
  onPrev: () => void;
  onToday: () => void;
  onNext: () => void;
}

export function Header({
  busy,
  onSync,
  onCompose,
  onSettings,
  status,
  account = null,
  stream = "all",
  onSelectStream,
  view = "mail",
  onSelectView,
  calendar,
}: {
  busy: boolean;
  onSync?: () => void;
  onCompose?: () => void;
  onSettings?: () => void;
  status: string | null;
  account?: string | null;
  stream?: Stream;
  onSelectStream?: (s: Stream) => void;
  view?: View;
  onSelectView?: (v: View) => void;
  calendar?: CalendarNav;
}) {
  const { theme, cycleTheme } = useTheme();
  const ThemeIcon = THEME_ICON[theme];
  const isCal = view === "calendar";
  return (
    <header className="app-header">
      <span className="brand">
        <Flame size={20} className="brand-icon" /> Ember
      </span>

      {account && onSelectView && (
        <div className="view-toggle" role="tablist" aria-label="Mail or Calendar">
          <button
            className={view === "mail" ? "view-tab active" : "view-tab"}
            aria-current={view === "mail" ? "page" : undefined}
            onClick={() => onSelectView("mail")}
          >
            <Mail size={14} /> <span className="nav-label">Mail</span>
          </button>
          <button
            className={view === "calendar" ? "view-tab active" : "view-tab"}
            aria-current={view === "calendar" ? "page" : undefined}
            onClick={() => onSelectView("calendar")}
          >
            <CalendarDays size={14} /> <span className="nav-label">Calendar</span>
          </button>
        </div>
      )}

      {account && !isCal && (
        <nav className="header-nav">
          {STREAMS.map((s) => {
            const Icon = STREAM_ICON[s.key];
            return (
              <button
                key={s.key}
                className={s.key === stream ? "header-nav-item active" : "header-nav-item"}
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

      {account && isCal && calendar && (
        <nav className="week-nav">
          <button className="icon-btn" aria-label="Previous week" onClick={calendar.onPrev}>
            <ChevronLeft size={16} />
          </button>
          <button className="btn" onClick={calendar.onToday}>
            Today
          </button>
          <button className="icon-btn" aria-label="Next week" onClick={calendar.onNext}>
            <ChevronRight size={16} />
          </button>
          <span className="week-range">{calendar.rangeLabel}</span>
        </nav>
      )}

      <span className="spacer" />
      {status && <span className="status-text">{status}</span>}

      {!isCal && onCompose && (
        <button className="btn" onClick={onCompose}>
          <Pencil size={15} /> <span className="nav-label">Compose</span>
        </button>
      )}
      {!isCal && onSync && (
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
