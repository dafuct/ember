import type { MessagePreview } from "../lib/api";
import { MessageItem } from "./MessageItem";

export function MessageList({
  messages,
  selectedId,
  onSelect,
}: {
  messages: MessagePreview[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <section className="msglist">
      <div className="msglist-header">
        <span className="msglist-title">Inbox</span>
        <span className="msglist-count">{messages.length} messages</span>
      </div>
      <div className="msglist-scroll">
        {messages.length === 0 ? (
          <div className="empty">No messages yet — hit Sync.</div>
        ) : (
          messages.map((m) => (
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
