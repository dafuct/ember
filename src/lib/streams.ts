import type { MessagePreview } from "./api";

// "all" is the grouped view; the other three are the scorer's category keys.
export type Stream = "all" | "people" | "notifications" | "newsletters";

export const STREAMS: { key: Stream; label: string }[] = [
  { key: "all", label: "All" },
  { key: "people", label: "People" },
  { key: "notifications", label: "Notifications" },
  { key: "newsletters", label: "Newsletters" },
];

// Display label for a category key (used for section headers and the dot title).
export const CATEGORY_LABEL: Record<string, string> = {
  people: "People",
  notifications: "Notifications",
  newsletters: "Newsletters",
};

// Order the grouped "All" view shows its sections in.
const STREAM_ORDER = ["people", "notifications", "newsletters"] as const;

export function filterByStream(
  msgs: MessagePreview[],
  stream: Stream,
): MessagePreview[] {
  if (stream === "all") return msgs;
  return msgs.filter((m) => m.category === stream);
}

export interface StreamGroup {
  category: string;
  label: string;
  messages: MessagePreview[];
}

// Group messages into the three streams, dropping empty groups.
export function groupByStream(msgs: MessagePreview[]): StreamGroup[] {
  return STREAM_ORDER.map((cat) => ({
    category: cat,
    label: CATEGORY_LABEL[cat],
    messages: msgs.filter((m) => m.category === cat),
  })).filter((g) => g.messages.length > 0);
}

// The messages in the exact order MessageList renders them: grouped
// (people → notifications → newsletters) for "all", else the filtered flat list.
// Shared with App's selection-advance so the two can't drift from what's on screen.
export function orderedForStream(
  msgs: MessagePreview[],
  stream: Stream,
): MessagePreview[] {
  if (stream === "all") {
    return groupByStream(msgs).flatMap((g) => g.messages);
  }
  return filterByStream(msgs, stream);
}
