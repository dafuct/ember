import { useEffect, useState } from "react";
import {
  fetchMessageBody,
  type MessageBody,
  type MessagePreview,
} from "../lib/api";
import { Mail, Archive, Trash2, Star, CornerUpLeft } from "lucide-react";
import { isStarred } from "../lib/labels";

export function ReadingPane({
  msg,
  onArchive,
  onTrash,
  onToggleStar,
  onMarkUnread,
}: {
  msg: MessagePreview | null;
  onArchive: (m: MessagePreview) => void;
  onTrash: (m: MessagePreview) => void;
  onToggleStar: (m: MessagePreview) => void;
  onMarkUnread: (m: MessagePreview) => void;
}) {
  const [body, setBody] = useState<MessageBody | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!msg) {
      setBody(null);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    setBody(null);
    fetchMessageBody(msg.id, true)
      .then((b) => {
        if (!cancelled) setBody(b);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [msg?.id]);

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
        <button
          className={isStarred(msg) ? "icon-btn active" : "icon-btn"}
          aria-label={isStarred(msg) ? "Unstar" : "Star"}
          onClick={() => onToggleStar(msg)}
        >
          <Star size={15} fill={isStarred(msg) ? "currentColor" : "none"} />
        </button>
        <button
          className="icon-btn"
          aria-label="Mark as unread"
          onClick={() => onMarkUnread(msg)}
        >
          <Mail size={15} />
        </button>
        <button
          className="icon-btn"
          aria-label="Archive"
          onClick={() => onArchive(msg)}
        >
          <Archive size={15} />
        </button>
        <button
          className="icon-btn"
          aria-label="Move to trash"
          onClick={() => onTrash(msg)}
        >
          <Trash2 size={15} />
        </button>
      </div>
      <div className="reading-head">
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
      </div>
      <div className="reading-body-area">
        {loading ? (
          <div className="reading-status">Loading…</div>
        ) : error ? (
          <pre className="error-text">{error}</pre>
        ) : body?.is_html ? (
          <iframe
            className="reading-frame"
            sandbox=""
            srcDoc={body.html}
            title="Message body"
          />
        ) : body ? (
          <pre className="reading-text">{body.html}</pre>
        ) : null}
      </div>
    </section>
  );
}
