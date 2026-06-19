import type { MessagePreview } from "../lib/api";
import { MessageItem } from "./MessageItem";
import {
  STREAMS,
  filterByStream,
  groupByStream,
  type Stream,
} from "../lib/streams";

export function MessageList({
  messages,
  stream,
  selectedId,
  onSelect,
  onArchive,
  onStar,
  flat = false,
  title,
  emptyText,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  /** When true, render `messages` as a flat list (no stream filter/grouping) — used for search. */
  flat?: boolean;
  /** Header title override (used in flat/search mode). */
  title?: string;
  /** Empty-state text override (used in flat/search mode). */
  emptyText?: string;
}) {
  // Flat mode (search): render the given messages as-is. Stream mode (inbox): filter, and
  // group by category only in the "All" view.
  const visible = flat ? messages : filterByStream(messages, stream);
  const groups = !flat && stream === "all" ? groupByStream(visible) : null;
  const headerTitle = flat
    ? title ?? "Results"
    : STREAMS.find((s) => s.key === stream)?.label ?? "Inbox";
  const count = groups
    ? groups.reduce((n, g) => n + g.messages.length, 0)
    : visible.length;
  const empty = emptyText ?? "No messages here — hit Sync.";

  return (
    <section className="msglist">
      <div className="msglist-header">
        <span className="msglist-title">{headerTitle}</span>
        <span className="msglist-count">{count} messages</span>
      </div>
      <div className="msglist-scroll">
        {count === 0 ? (
          <div className="empty">{empty}</div>
        ) : groups ? (
          groups.map((group) => (
            <div key={group.category} className="msglist-group">
              <div className="msglist-group-header">
                <span>{group.label}</span>
                <span className="msglist-group-count">
                  {group.messages.length}
                </span>
              </div>
              {group.messages.map((m) => (
                <MessageItem
                  key={m.id}
                  msg={m}
                  selected={m.id === selectedId}
                  onSelect={onSelect}
                  onArchive={onArchive}
                  onStar={onStar}
                />
              ))}
            </div>
          ))
        ) : (
          visible.map((m) => (
            <MessageItem
              key={m.id}
              msg={m}
              selected={m.id === selectedId}
              onSelect={onSelect}
              onArchive={onArchive}
              onStar={onStar}
            />
          ))
        )}
      </div>
    </section>
  );
}
