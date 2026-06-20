import { useEffect, useState } from "react";
import {
  fetchMessageBody,
  downloadAttachment,
  type MessageBody,
  type MessagePreview,
  type Attachment,
} from "../lib/api";
import { isTauri } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { formatBytes } from "../lib/attachments";
import { Mail, Archive, Trash2, Star, CornerUpLeft, RotateCcw, Tag, Paperclip } from "lucide-react";
import { isStarred, userLabelChips } from "../lib/labels";
import { LabelChips } from "./LabelChips";
import type { Label } from "../lib/api";

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
  labelsById,
  onOpenLabels,
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
  labelsById?: Map<string, Label>;
  onOpenLabels?: (m: MessagePreview) => void;
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

  const [dlStatus, setDlStatus] = useState<Record<string, string>>({});

  // Reset the delete confirmation whenever the open message changes.
  useEffect(() => setConfirmDelete(false), [msg?.id]);
  useEffect(() => setDlStatus({}), [msg?.id]);

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

  async function handleDownload(att: Attachment) {
    if (!msg) return;
    if (!isTauri()) {
      setDlStatus((s) => ({ ...s, [att.attachment_id]: "(maket: no download)" }));
      return;
    }
    const dest = await save({ defaultPath: att.filename });
    if (!dest) return;
    setDlStatus((s) => ({ ...s, [att.attachment_id]: "Saving…" }));
    try {
      await downloadAttachment(msg.id, att.attachment_id, dest);
      setDlStatus((s) => ({ ...s, [att.attachment_id]: "Saved ✓" }));
    } catch {
      setDlStatus((s) => ({ ...s, [att.attachment_id]: "Failed" }));
    }
  }

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
            {onOpenLabels && (
              <button className="icon-btn" aria-label="Labels" onClick={() => onOpenLabels(msg)}>
                <Tag size={15} />
              </button>
            )}
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
        {labelsById && <LabelChips labels={userLabelChips(msg, labelsById)} />}
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
      {body && body.attachments.length > 0 && (
        <div className="attachments-strip">
          {body.attachments.map((att) => (
            <button
              key={att.attachment_id}
              className="attach-chip"
              onClick={() => handleDownload(att)}
              title={`Save ${att.filename}`}
            >
              <Paperclip size={13} />
              <span className="attach-name">{att.filename}</span>
              <span className="attach-size">{formatBytes(att.size)}</span>
              {dlStatus[att.attachment_id] && (
                <span className="attach-status">{dlStatus[att.attachment_id]}</span>
              )}
            </button>
          ))}
        </div>
      )}
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
