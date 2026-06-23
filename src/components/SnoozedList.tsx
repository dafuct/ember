import { Clock, RotateCcw } from "lucide-react";
import type { SnoozedRow } from "../lib/snooze";

export function SnoozedList({ rows, onUnsnooze }: { rows: SnoozedRow[]; onUnsnooze: (id: string) => void }) {
  const fmt = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
  return (
    <section className="msglist">
      <div className="list-head"><div className="list-title">Snoozed</div></div>
      {rows.length === 0 ? (
        <div className="empty">Nothing snoozed.</div>
      ) : (
        <div className="msglist-scroll">
          {rows.map((r) => (
            <div key={r.message_id} className="msg-card snoozed-card">
              <div className="msg-body">
                <div className="msg-top"><span className="name">{r.from_addr}</span></div>
                <div className="subject">{r.subject || "(no subject)"}</div>
                <div className="snippet">{r.snippet}</div>
                <div className="snooze-wake"><Clock size={12} /> Wakes {fmt(r.wake_at)}</div>
              </div>
              <button className="batch-btn" onClick={() => onUnsnooze(r.message_id)}>
                <RotateCcw size={14} /> Un-snooze
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
