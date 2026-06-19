import type { MessagePreview } from "./api";

// Gmail's system label ids that map to read/star state.
export const UNREAD = "UNREAD";
export const STARRED = "STARRED";

export type LabelId = typeof UNREAD | typeof STARRED;

export const isUnread = (m: MessagePreview): boolean =>
  m.label_ids.includes(UNREAD);
export const isStarred = (m: MessagePreview): boolean =>
  m.label_ids.includes(STARRED);

/**
 * Return a copy of `m` with `label` present or absent. Pure — never mutates `m`,
 * so it is safe to use for React optimistic state. Returns the same reference
 * when nothing would change (lets callers skip a no-op render).
 */
export function withLabel(
  m: MessagePreview,
  label: LabelId,
  present: boolean,
): MessagePreview {
  const has = m.label_ids.includes(label);
  if (has === present) return m;
  const label_ids = present
    ? [...m.label_ids, label]
    : m.label_ids.filter((l) => l !== label);
  return { ...m, label_ids };
}
