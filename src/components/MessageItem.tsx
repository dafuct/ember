import type { MessagePreview, Label } from "../lib/api";
import { isStarred, isUnread, userLabelChips } from "../lib/labels";
import { relativeTime } from "../lib/time";
import { Star, Clock } from "lucide-react";
import { LabelChips } from "./LabelChips";

// Deterministic avatar tint from the sender string so the same correspondent
// keeps the same color across renders. Hues are spread evenly; lightness/sat are
// fixed so the chips read consistently in both themes.
const AVATAR_HUES = [4, 28, 48, 142, 168, 200, 224, 268, 292, 330];
function avatarHue(seed: string): number {
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) | 0;
  return AVATAR_HUES[Math.abs(h) % AVATAR_HUES.length];
}
function initial(name: string): string {
  const m = name.match(/[A-Za-z0-9]/);
  return m ? m[0].toUpperCase() : "?";
}

export function MessageItem({
  msg,
  selected,
  checked = false,
  onSelect,
  onToggleSelect,
  onArchive,
  onStar,
  onSnooze,
  showRecipient = false,
  labelsById,
}: {
  msg: MessagePreview;
  selected: boolean;
  checked?: boolean;
  onSelect: (id: string) => void;
  onToggleSelect?: (id: string, shiftKey?: boolean) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  onSnooze?: (msg: MessagePreview, e: { clientX: number; clientY: number }) => void;
  showRecipient?: boolean;
  labelsById?: Map<string, Label>;
}) {
  // onArchive is retained in the props contract (batch/keyboard paths still call it),
  // but the card omits a row-level archive control per the redesign — only star remains.
  void onArchive;

  const unread = isUnread(msg);
  const starred = isStarred(msg);
  const cls = ["msg-card", selected && "selected", checked && "checked", unread && "unread"]
    .filter(Boolean)
    .join(" ");

  const display = showRecipient
    ? `To: ${msg.to_addr || "(no recipient)"}`
    : msg.from || "(unknown sender)";
  const hue = avatarHue(display);

  return (
    <div
      className={cls}
      onClick={(e) => {
        if (e.shiftKey) onToggleSelect?.(msg.id, true);
        else onSelect(msg.id);
      }}
    >
      <div className="msg-lead">
        {/* Selection is driven by `checked` + the onClick handler (which carries shiftKey for
            range-select); the no-op onChange keeps this a valid controlled checkbox and lets
            keyboard Space toggle it (Space dispatches a click). */}
        <input
          type="checkbox"
          className="msg-check"
          checked={checked}
          onChange={() => {}}
          onClick={(e) => {
            e.stopPropagation();
            onToggleSelect?.(msg.id, e.shiftKey);
          }}
          aria-label="Select message"
        />
        <span
          className="msg-avatar"
          style={{
            background: `hsl(${hue} 55% 22%)`,
            color: `hsl(${hue} 70% 72%)`,
          }}
          aria-hidden
        >
          {initial(display)}
        </span>
      </div>
      <div className="msg-body">
        <div className="msg-top">
          <span className="name">{display}</span>
          <span className="when">{relativeTime(msg.internal_date)}</span>
        </div>
        <div className="subject">{msg.subject || "(no subject)"}</div>
        {labelsById && <LabelChips labels={userLabelChips(msg, labelsById)} />}
        <div className="snippet">{msg.snippet}</div>
      </div>
      {onSnooze && (
        <button
          className="msg-clock"
          aria-label="Snooze"
          onClick={(e) => {
            e.stopPropagation();
            onSnooze(msg, e);
          }}
        >
          <Clock size={16} />
        </button>
      )}
      <button
        className={starred ? "msg-star on" : "msg-star"}
        aria-label={starred ? "Unstar" : "Star"}
        onClick={(e) => {
          e.stopPropagation();
          onStar(msg);
        }}
      >
        <Star size={16} fill={starred ? "currentColor" : "none"} />
      </button>
    </div>
  );
}
