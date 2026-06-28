import { useEffect, useRef, useState } from "react";
import { nextId, rangeBetween } from "../lib/listnav";
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
  onActivate,
  onToggleSelect,
  onSelectRange,
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
  keyboardEnabled = true,
}: {
  messages: MessagePreview[];
  stream: Stream;
  selectedId: string | null;
  selectedIds?: Set<string>;
  onSelect: (id: string) => void;
  onActivate?: (id: string) => void;
  onToggleSelect?: (id: string) => void;
  onSelectRange?: (ids: string[]) => void;
  onSelectAllVisible?: (ids: string[]) => void;
  onClearSelection?: () => void;
  onBatchArchive?: () => void;
  onBatchTrash?: () => void;
  onBatchMarkRead?: () => void;
  onBatchStar?: () => void;
  labelsById?: Map<string, Label>;
  onBatchLabel?: () => void;
  folder?: string;
  onBatchRestore?: () => void;
  onBatchDeleteForever?: () => void;
  onArchive: (msg: MessagePreview) => void;
  onStar: (msg: MessagePreview) => void;
  onSnooze?: (msg: MessagePreview, e: { clientX: number; clientY: number }) => void;
  flat?: boolean;
  title?: string;
  emptyText?: string;
  showRecipient?: boolean;
  onSearch: (q: string) => void;
  onClearSearch: () => void;
  searchQuery: string;
  searching: boolean;
  onSync: () => void;
  busy: boolean;
  onLoadMore?: () => void;
  canLoadMore?: boolean;
  keyboardEnabled?: boolean;
}) {
  const visible = flat ? messages : filterByStream(messages, stream);
  const groups = !flat && stream === "all" ? groupByStream(visible) : null;
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

  const anchorRef = useRef<string | null>(null);
  const [leadId, setLeadId] = useState<string | null>(selectedId);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setLeadId(selectedId);
  }, [selectedId]);

  useEffect(() => {
    if (!keyboardEnabled) return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest("input, textarea, select, button, a, [contenteditable='true']")) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      const lead = leadId ?? selectedId;

      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const dir = e.key === "ArrowDown" ? 1 : -1;
        const next = nextId(visibleIds, lead, dir);
        if (!next) return;
        if (e.shiftKey) {
          if (!anchorRef.current) anchorRef.current = lead ?? next;
          setLeadId(next);
          onSelectRange?.(rangeBetween(visibleIds, anchorRef.current, next));
        } else {
          setLeadId(next);
          anchorRef.current = next;
          onActivate?.(next);
        }
        const el = scrollRef.current?.querySelector(`[data-id="${CSS.escape(next)}"]`);
        el?.scrollIntoView({ block: "nearest" });
      } else if (e.key === "Enter") {
        if (lead) {
          e.preventDefault();
          onSelect(lead);
        }
      } else if (e.key === "Escape") {
        if (selectedIds.size > 0) {
          e.preventDefault();
          onClearSelection?.();
          anchorRef.current = null;
        }
      } else if (e.key === "x" || e.key === "X") {
        if (lead) {
          e.preventDefault();
          onToggleSelect?.(lead);
          anchorRef.current = lead;
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    keyboardEnabled,
    leadId,
    selectedId,
    visibleIds,
    selectedIds,
    onActivate,
    onSelect,
    onSelectRange,
    onClearSelection,
    onToggleSelect,
  ]);

  const handleToggle = (id: string, shiftKey?: boolean) => {
    if (shiftKey && anchorRef.current && onSelectRange) {
      const a = visibleIds.indexOf(anchorRef.current);
      const b = visibleIds.indexOf(id);
      if (a !== -1 && b !== -1) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        onSelectRange(visibleIds.slice(lo, hi + 1));
        return;
      }
    }
    onToggleSelect?.(id);
    anchorRef.current = id;
  };

  const onSearchChange = (value: string) => {
    if (value.trim() === "") onClearSearch();
    else onSearch(value);
  };

  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || !canLoadMore || !onLoadMore) return;
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

      <div className="msglist-scroll" ref={scrollRef}>
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
                    lead={m.id === leadId}
                    onSelect={onSelect}
                    checked={selectedIds.has(m.id)}
                    onToggleSelect={handleToggle}
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
              lead={m.id === leadId}
              onSelect={onSelect}
              checked={selectedIds.has(m.id)}
              onToggleSelect={handleToggle}
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
