import type { MessagePreview } from "../lib/api";
import { relativeTime } from "../lib/time";

export function MessageItem({
  msg,
  selected,
  onSelect,
}: {
  msg: MessagePreview;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      className={selected ? "msg-item selected" : "msg-item"}
      onClick={() => onSelect(msg.id)}
    >
      <div className="msg-top">
        <span className="msg-sender">
          {msg.category && (
            <span
              className={`cat-dot cat-${msg.category}`}
              title={msg.category}
              aria-hidden
            />
          )}
          {msg.from || "(unknown sender)"}
        </span>
        <span className="msg-time">{relativeTime(msg.internal_date)}</span>
      </div>
      <span className="msg-subject">{msg.subject || "(no subject)"}</span>
      <span className="msg-snippet">{msg.snippet}</span>
    </button>
  );
}
