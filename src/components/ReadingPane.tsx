import { useEffect, useState } from "react";
import {
  fetchMessageBody,
  type MessageBody,
  type MessagePreview,
} from "../lib/api";
import { Mail, Archive, Trash2, Star, CornerUpLeft, RotateCcw } from "lucide-react";
import { isStarred } from "../lib/labels";

export function ReadingPane({
  msg,
  loadImages,
  onArchive,
  onTrash,
  onToggleStar,
  onMarkUnread,
  onReply,
  folder = "inbox",
  onRestore,
  onDeleteForever,
}: {
  msg: MessagePreview | null;
  loadImages: boolean;
  onArchive: (m: MessagePreview) => void;
  onTrash: (m: MessagePreview) => void;
  onToggleStar: (m: MessagePreview) => void;
  onMarkUnread: (m: MessagePreview) => void;
  onReply: (m: MessagePreview) => void;
  folder?: string;
  onRestore?: (m: MessagePreview) => void;
  onDeleteForever?: (m: MessagePreview) => void;
}) {
  const [body, setBody] = useState<MessageBody | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Two-step confirm for the irreversible permanent delete.
  const [confirmDelete, setConfirmDelete] = useState(false);

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
    fetchMessageBody(msg.id, loadImages)
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
  }, [msg?.id, loadImages]);

  // Reset the delete confirmation whenever the open message changes.
  useEffect(() => setConfirmDelete(false), [msg?.id]);

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

  const inTrash = folder === "trash";

  return (
    <section className="reading">
      <div className="reading-toolbar">
        <button className="icon-btn" aria-label="Reply" onClick={() => onReply(msg)}>
          <CornerUpLeft size={15} />
        </button>
        <button
          className={isStarred(msg) ? "icon-btn active" : "icon-btn"}
          aria-label={isStarred(msg) ? "Unstar" : "Star"}
          onClick={() => onToggleStar(msg)}
        >
          <Star size={15} fill={isStarred(msg) ? "currentColor" : "none"} />
        </button>
        <button className="icon-btn" aria-label="Mark as unread" onClick={() => onMarkUnread(msg)}>
          <Mail size={15} />
        </button>
        {inTrash ? (
          <>
            <button className="icon-btn" aria-label="Restore" onClick={() => onRestore?.(msg)}>
              <RotateCcw size={15} />
            </button>
            {confirmDelete ? (
              <button
                className="btn btn-danger"
                onClick={() => {
                  setConfirmDelete(false);
                  onDeleteForever?.(msg);
                }}
              >
                Delete forever?
              </button>
            ) : (
              <button
                className="icon-btn"
                aria-label="Delete forever"
                onClick={() => setConfirmDelete(true)}
              >
                <Trash2 size={15} />
              </button>
            )}
          </>
        ) : (
          <>
            <button className="icon-btn" aria-label="Archive" onClick={() => onArchive(msg)}>
              <Archive size={15} />
            </button>
            <button className="icon-btn" aria-label="Move to trash" onClick={() => onTrash(msg)}>
              <Trash2 size={15} />
            </button>
          </>
        )}
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
