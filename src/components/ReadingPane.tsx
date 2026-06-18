import type { MessagePreview } from "../lib/api";
import { Mail, CornerUpLeft, Archive, Trash2 } from "lucide-react";

export function ReadingPane({ msg }: { msg: MessagePreview | null }) {
  if (!msg) {
    return (
      <section className="reading">
        <div className="reading-empty">
          <Mail size={28} />
          <span>Select a message to read</span>
        </div>
      </section>
    );
  }
  const date = msg.internal_date
    ? new Date(msg.internal_date).toLocaleString([], {
        dateStyle: "medium",
        timeStyle: "short",
      })
    : msg.date;
  return (
    <section className="reading">
      <div className="reading-toolbar">
        <button className="icon-btn" disabled aria-label="Reply (coming soon)">
          <CornerUpLeft size={15} />
        </button>
        <button className="icon-btn" disabled aria-label="Archive (coming soon)">
          <Archive size={15} />
        </button>
        <button className="icon-btn" disabled aria-label="Delete (coming soon)">
          <Trash2 size={15} />
        </button>
      </div>
      <div className="reading-content">
        <h2 className="reading-subject">{msg.subject || "(no subject)"}</h2>
        <div className="reading-from">
          <div className="avatar avatar-lg">
            {(msg.from || "?").charAt(0).toUpperCase()}
          </div>
          <div className="reading-from-text">
            <div className="reading-name">{msg.from || "(unknown sender)"}</div>
          </div>
          <div className="reading-date">{date}</div>
        </div>
        <p className="reading-body">{msg.snippet}</p>
        <div className="reading-note">
          <Mail size={16} /> Preview shown — full message body arrives in M5.
        </div>
      </div>
    </section>
  );
}
