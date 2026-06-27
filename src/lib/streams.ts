import type { MessagePreview } from "./api";

export type Stream = "all" | "people" | "notifications" | "newsletters";

export const STREAMS: { key: Stream; label: string }[] = [
  { key: "all", label: "All" },
  { key: "people", label: "People" },
  { key: "notifications", label: "Notifications" },
  { key: "newsletters", label: "Newsletters" },
];

export const CATEGORY_LABEL: Record<string, string> = {
  people: "People",
  notifications: "Notifications",
  newsletters: "Newsletters",
};

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

export function groupByStream(msgs: MessagePreview[]): StreamGroup[] {
  return STREAM_ORDER.map((cat) => ({
    category: cat,
    label: CATEGORY_LABEL[cat],
    messages: msgs.filter((m) => m.category === cat),
  })).filter((g) => g.messages.length > 0);
}

export function orderedForStream(
  msgs: MessagePreview[],
  stream: Stream,
): MessagePreview[] {
  if (stream === "all") {
    return groupByStream(msgs).flatMap((g) => g.messages);
  }
  return filterByStream(msgs, stream);
}
