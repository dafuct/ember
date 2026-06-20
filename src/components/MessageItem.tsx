import type { MessagePreview } from "../lib/api";
import { isStarred, isUnread } from "../lib/labels";
import { relativeTime } from "../lib/time";
import { Archive, Star } from "lucide-react";

export function MessageItem({
  msg,
  selected,
  checked = false,
  onSelect,
  onToggleSelect,
  onArchive,
  onStar,
  showRecipient = false,
}: {
  msg: MessagePreview;
  selected: boolean;
  checked?: boolean;
  onSelect: (id: string) => void;
  onToggleSelect?: (id: string) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  showRecipient?: boolean;
}) {
  const unread = isUnread(msg);
  const starred = isStarred(msg);
  const cls = ["msg-item", selected && "selected", checked && "checked", unread && "unread"]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={cls}>
      <input
        type="checkbox"
        className="msg-check"
        checked={checked}
        onChange={(e) => {
          e.stopPropagation();
          onToggleSelect?.(msg.id);
        }}
        aria-label="Select message"
      />
      <button className="msg-item-main" onClick={() => onSelect(msg.id)}>
        <div className="msg-top">
          <span className="msg-sender">
            {msg.category && (
              <span
                className={`cat-dot cat-${msg.category}`}
                title={msg.category}
                aria-hidden
              />
            )}
            {showRecipient
              ? `To: ${msg.to_addr || "(no recipient)"}`
              : msg.from || "(unknown sender)"}
          </span>
          <span className="msg-time">{relativeTime(msg.internal_date)}</span>
          {unread && <span className="unread-dot" title="Unread" aria-hidden />}
        </div>
        <span className="msg-subject">{msg.subject || "(no subject)"}</span>
        <span className="msg-snippet">{msg.snippet}</span>
      </button>
      <div className="msg-actions">
        <button
          className={starred ? "row-act starred" : "row-act"}
          aria-label={starred ? "Unstar" : "Star"}
          onClick={(e) => {
            e.stopPropagation();
            onStar(msg);
          }}
        >
          <Star size={14} fill={starred ? "currentColor" : "none"} />
        </button>
        <button
          className="row-act"
          aria-label="Archive"
          onClick={(e) => {
            e.stopPropagation();
            onArchive(msg);
          }}
        >
          <Archive size={14} />
        </button>
      </div>
    </div>
  );
}
