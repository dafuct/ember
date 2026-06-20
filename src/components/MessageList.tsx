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
  selectedIds = new Set<string>(),
  onSelect,
  onToggleSelect,
  onSelectAllVisible,
  onClearSelection,
  onBatchArchive,
  onBatchTrash,
  onBatchMarkRead,
  onBatchStar,
  onArchive,
  onStar,
  flat = false,
  title,
  emptyText,
  showRecipient = false,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  selectedIds?: Set<string>;
  onSelect: (id: string) => void;
  onToggleSelect?: (id: string) => void;
  onSelectAllVisible?: (ids: string[]) => void;
  onClearSelection?: () => void;
  onBatchArchive?: () => void;
  onBatchTrash?: () => void;
  onBatchMarkRead?: () => void;
  onBatchStar?: () => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  /** When true, render `messages` as a flat list (no stream filter/grouping) — used for search. */
  flat?: boolean;
  /** Header title override (used in flat/search mode). */
  title?: string;
  /** Empty-state text override (used in flat/search mode). */
  emptyText?: string;
  showRecipient?: boolean;
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

  const visibleIds = (groups ? groups.flatMap((g) => g.messages) : visible).map((m) => m.id);
  const allVisibleSelected = visibleIds.length > 0 && visibleIds.every((id) => selectedIds.has(id));

  return (
    <section className="msglist">
      {selectedIds.size > 0 ? (
        <div className="msglist-header batch-bar">
          <input
            type="checkbox"
            className="batch-check"
            checked={allVisibleSelected}
            onChange={() => onSelectAllVisible?.(visibleIds)}
            aria-label="Select all visible"
          />
          <span className="batch-count">{selectedIds.size} selected</span>
          <div className="batch-actions">
            <button className="batch-btn" onClick={() => onBatchArchive?.()}>Archive</button>
            <button className="batch-btn" onClick={() => onBatchTrash?.()}>Trash</button>
            <button className="batch-btn" onClick={() => onBatchMarkRead?.()}>Mark read</button>
            <button className="batch-btn" onClick={() => onBatchStar?.()}>Star</button>
          </div>
          <button className="batch-clear" aria-label="Clear selection" onClick={() => onClearSelection?.()}>
            ✕
          </button>
        </div>
      ) : (
        <div className="msglist-header">
          <span className="msglist-title">{headerTitle}</span>
          <span className="msglist-count">{count} messages</span>
        </div>
      )}
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
                  checked={selectedIds.has(m.id)}
                  onToggleSelect={onToggleSelect}
                  onArchive={onArchive}
                  onStar={onStar}
                  showRecipient={showRecipient}
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
              checked={selectedIds.has(m.id)}
              onToggleSelect={onToggleSelect}
              onArchive={onArchive}
              onStar={onStar}
              showRecipient={showRecipient}
            />
          ))
        )}
      </div>
    </section>
  );
}
