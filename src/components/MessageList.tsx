import { useEffect, useRef } from "react";
import type { Label, MessagePreview } from "../lib/api";
import { MessageItem } from "./MessageItem";
import {
  STREAMS,
  filterByStream,
  groupByStream,
  type Stream,
} from "../lib/streams";
import { Bell, Newspaper, RefreshCw, Search, Users } from "lucide-react";
import type { LucideIcon } from "lucide-react";

// Colored stream icon for the grouped "All" view section headers.
const GROUP_ICON: Record<string, { Icon: LucideIcon; hue: number }> = {
  people: { Icon: Users, hue: 200 },
  notifications: { Icon: Bell, hue: 38 },
  newsletters: { Icon: Newspaper, hue: 168 },
};

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
  labelsById,
  onBatchLabel,
  folder,
  onBatchRestore,
  onBatchDeleteForever,
  onArchive,
  onStar,
  onSnooze,
  flat = false,
  title,
  emptyText,
  showRecipient = false,
  onSearch,
  onClearSearch,
  searchQuery,
  searching,
  onSync,
  busy,
  onLoadMore,
  canLoadMore = false,
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
  labelsById?: Map<string, Label>;
  onBatchLabel?: () => void;
  /** Active folder key. In "trash" the batch bar swaps to Restore / Delete forever. */
  folder?: string;
  onBatchRestore?: () => void;
  onBatchDeleteForever?: () => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  onSnooze?: (msg: MessagePreview, e: { clientX: number; clientY: number }) => void;
  /** When true, render `messages` as a flat list (no stream filter/grouping) — used for search. */
  flat?: boolean;
  /** Header title override (used in flat/search mode). */
  title?: string;
  /** Empty-state text override (used in flat/search mode). */
  emptyText?: string;
  showRecipient?: boolean;
  /** Search input change — empty value clears, anything else runs a search. */
  onSearch: (q: string) => void;
  onClearSearch: () => void;
  searchQuery: string;
  searching: boolean;
  onSync: () => void;
  busy: boolean;
  /** Called when the bottom sentinel scrolls into view (inbox infinite scroll). */
  onLoadMore?: () => void;
  /** When true, the sentinel is active and may trigger onLoadMore. */
  canLoadMore?: boolean;
}) {
  // Flat mode (search): render the given messages as-is. Stream mode (inbox): filter, and
  // group by category only in the "All" view.
  const visible = flat ? messages : filterByStream(messages, stream);
  const groups = !flat && stream === "all" ? groupByStream(visible) : null;
  // List-column title: the override (search/folder), else the stream label, else Smart Inbox.
  const headerTitle = flat
    ? title ?? "Results"
    : stream === "all"
      ? "Smart Inbox"
      : STREAMS.find((s) => s.key === stream)?.label ?? "Inbox";
  const count = groups
    ? groups.reduce((n, g) => n + g.messages.length, 0)
    : visible.length;
  const empty = emptyText ?? "No messages here — hit Sync.";

  const visibleIds = (groups ? groups.flatMap((g) => g.messages) : visible).map((m) => m.id);
  const allVisibleSelected = visibleIds.length > 0 && visibleIds.every((id) => selectedIds.has(id));

  // Typing drives search; emptying the box exits search mode (handleSearch ignores empties).
  const onSearchChange = (value: string) => {
    if (value.trim() === "") onClearSearch();
    else onSearch(value);
  };

  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || !canLoadMore || !onLoadMore) return;
    // 🦀-style note: observe the 1px sentinel against the scroll container; fire onLoadMore
    //    when it scrolls into view (user reached near the bottom). rootMargin pre-loads a bit.
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) onLoadMore();
      },
      { root: el.parentElement, rootMargin: "200px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [canLoadMore, onLoadMore]);

  return (
    <section className="msglist">
      <div className="list-head">
        <div className="list-title">{headerTitle}</div>
        <button className="list-tool" aria-label="Sync" disabled={busy} onClick={onSync}>
          <RefreshCw size={16} className={busy ? "spin" : undefined} />
        </button>
      </div>
      <div className="list-search">
        <Search size={16} />
        <input
          value={searchQuery}
          placeholder="Search mail"
          onChange={(e) => onSearchChange(e.target.value)}
        />
        {searching && <span className="list-search-hint">…</span>}
      </div>

      {selectedIds.size > 0 && (
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
            {folder === "trash" ? (
              // In Trash, "Archive"/"Trash" are no-ops (the message is already trashed); the
              // real actions are untrash (Restore) and permanent delete (Delete forever).
              <>
                <button className="batch-btn" onClick={() => onBatchRestore?.()}>Restore</button>
                <button className="batch-btn batch-btn-danger" onClick={() => onBatchDeleteForever?.()}>
                  Delete forever
                </button>
              </>
            ) : (
              <>
                <button className="batch-btn" onClick={() => onBatchArchive?.()}>Archive</button>
                <button className="batch-btn" onClick={() => onBatchTrash?.()}>Trash</button>
                <button className="batch-btn" onClick={() => onBatchMarkRead?.()}>Mark read</button>
                <button className="batch-btn" onClick={() => onBatchStar?.()}>Star</button>
                <button className="batch-btn" onClick={() => onBatchLabel?.()}>Label</button>
              </>
            )}
          </div>
          <button className="batch-clear" aria-label="Clear selection" onClick={() => onClearSelection?.()}>
            ✕
          </button>
        </div>
      )}

      <div className="msglist-scroll">
        {count === 0 ? (
          <div className="empty">{empty}</div>
        ) : groups ? (
          groups.map((group) => {
            const meta = GROUP_ICON[group.category];
            return (
              <div key={group.category} className="msglist-group">
                <div className="group-head">
                  {meta && (
                    <span
                      className="group-ic"
                      style={{
                        background: `hsl(${meta.hue} 55% 20%)`,
                        color: `hsl(${meta.hue} 70% 70%)`,
                      }}
                      aria-hidden
                    >
                      <meta.Icon size={13} />
                    </span>
                  )}
                  <span>{group.label}</span>
                  <span className="group-count">{group.messages.length}</span>
                </div>
                {group.messages.map((m) => (
                  <MessageItem
                    key={m.id}
                    msg={m}
                    selected={m.id === selectedId}
                    onSelect={onSelect}
                    checked={selectedIds.has(m.id)}
                    onToggleSelect={onToggleSelect}
                    labelsById={labelsById}
                    onArchive={onArchive}
                    onStar={onStar}
                    onSnooze={onSnooze}
                    showRecipient={showRecipient}
                  />
                ))}
              </div>
            );
          })
        ) : (
          visible.map((m) => (
            <MessageItem
              key={m.id}
              msg={m}
              selected={m.id === selectedId}
              onSelect={onSelect}
              checked={selectedIds.has(m.id)}
              onToggleSelect={onToggleSelect}
              labelsById={labelsById}
              onArchive={onArchive}
              onStar={onStar}
              onSnooze={onSnooze}
              showRecipient={showRecipient}
            />
          ))
        )}
        {canLoadMore && count > 0 && (
          <div ref={sentinelRef} className="msglist-sentinel" aria-hidden />
        )}
      </div>
    </section>
  );
}
