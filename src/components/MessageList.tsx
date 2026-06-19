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
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  const visible = filterByStream(messages, stream);
  const title = STREAMS.find((s) => s.key === stream)?.label ?? "Inbox";

  return (
    <section className="msglist">
      <div className="msglist-header">
        <span className="msglist-title">{title}</span>
        <span className="msglist-count">{visible.length} messages</span>
      </div>
      <div className="msglist-scroll">
        {visible.length === 0 ? (
          <div className="empty">No messages here — hit Sync.</div>
        ) : stream === "all" ? (
          groupByStream(messages).map((group) => (
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
            />
          ))
        )}
      </div>
    </section>
  );
}
