import { useState } from "react";
import { snoozePresets } from "../lib/snooze";

export function SnoozeMenu({
  anchor, onPick, onClose,
}: {
  anchor: { x: number; y: number };
  onPick: (wakeAt: number) => void;
  onClose: () => void;
}) {
  const [custom, setCustom] = useState("");
  const presets = snoozePresets();
  const fmt = (ms: number) =>
    new Date(ms).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" });
  // Min for the custom picker = local "now" as YYYY-MM-DDTHH:mm, so a past wake (which would
  // archive then immediately un-snooze on the next wake tick) can't be picked.
  const localNow = (() => {
    const d = new Date();
    d.setMinutes(d.getMinutes() - d.getTimezoneOffset());
    return d.toISOString().slice(0, 16);
  })();
  return (
    <>
      <div className="snooze-backdrop" onClick={onClose} />
      <div className="snooze-menu" style={{ left: anchor.x, top: anchor.y }} role="menu">
        {presets.map((p) => (
          <button key={p.label} className="snooze-item" onClick={() => onPick(p.wakeAt)}>
            <span>{p.label}</span><span className="snooze-when">{fmt(p.wakeAt)}</span>
          </button>
        ))}
        <div className="snooze-custom">
          <input type="datetime-local" min={localNow} value={custom} onChange={(e) => setCustom(e.target.value)} aria-label="Custom snooze time" />
          <button className="snooze-go" disabled={!custom} onClick={() => { const t = new Date(custom).getTime(); if (!Number.isNaN(t)) onPick(t); }}>Snooze</button>
        </div>
      </div>
    </>
  );
}
