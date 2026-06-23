import { Flame, Inbox, CalendarDays, Plus, Sun, Moon } from "lucide-react";
import { useTheme } from "../theme";

type View = "mail" | "calendar";

export function IconRail({
  view,
  onSelectView,
  onCompose,
  onSettings,
  account,
}: {
  view: View;
  onSelectView: (v: View) => void;
  onCompose: () => void;
  onSettings: () => void;
  account: string | null;
}) {
  const { theme, cycleTheme } = useTheme();
  const initials = (account ?? "?").slice(0, 2).toUpperCase();
  return (
    <nav className="icon-rail" aria-label="Primary">
      <div className="rail-brand" aria-hidden><Flame size={20} /></div>
      <button className={`rail-item${view === "mail" ? " active" : ""}`} aria-label="Mail" aria-current={view === "mail"} onClick={() => onSelectView("mail")}><Inbox size={20} /></button>
      <button className={`rail-item${view === "calendar" ? " active" : ""}`} aria-label="Calendar" aria-current={view === "calendar"} onClick={() => onSelectView("calendar")}><CalendarDays size={20} /></button>
      <div className="rail-spacer" />
      <button className="rail-item" aria-label="Theme" onClick={cycleTheme}>{theme === "light" ? <Moon size={18} /> : <Sun size={18} />}</button>
      <button className="rail-item rail-compose" aria-label="Compose" onClick={onCompose}><Plus size={20} /></button>
      <button className="rail-avatar" aria-label="Account" onClick={onSettings}>{initials}</button>
    </nav>
  );
}
