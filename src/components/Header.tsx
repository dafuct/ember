import { Flame, RefreshCw, Sun, Moon, type LucideIcon } from "lucide-react";
import { useTheme, type Theme } from "../theme";

const THEME_ICON: Record<Theme, LucideIcon> = { light: Sun, dark: Moon, ember: Flame };

export function Header({
  busy,
  onSync,
  status,
}: {
  busy: boolean;
  onSync?: () => void;
  status: string | null;
}) {
  const { theme, cycleTheme } = useTheme();
  const ThemeIcon = THEME_ICON[theme];
  return (
    <header className="app-header">
      <span className="brand">
        <Flame size={20} className="brand-icon" /> Ember
      </span>
      <span className="spacer" />
      {status && <span className="status-text">{status}</span>}
      {onSync && (
        <button className="btn btn-accent" onClick={onSync} disabled={busy}>
          <RefreshCw size={15} className={busy ? "spin" : undefined} />
          {busy ? "Syncing…" : "Sync"}
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
