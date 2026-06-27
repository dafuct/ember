import type { MessagePreview, Label } from "./api";

export const UNREAD = "UNREAD";
export const STARRED = "STARRED";

export type LabelId = typeof UNREAD | typeof STARRED;

export const isUnread = (m: MessagePreview): boolean =>
  m.label_ids.includes(UNREAD);
export const isStarred = (m: MessagePreview): boolean =>
  m.label_ids.includes(STARRED);

export function withLabel(
  m: MessagePreview,
  label: string,
  present: boolean,
): MessagePreview {
  const has = m.label_ids.includes(label);
  if (has === present) return m;
  const label_ids = present
    ? [...m.label_ids, label]
    : m.label_ids.filter((l) => l !== label);
  return { ...m, label_ids };
}

export function userLabelChips(m: MessagePreview, labelsById: Map<string, Label>): Label[] {
  return m.label_ids
    .map((id) => labelsById.get(id))
    .filter((l): l is Label => l !== undefined);
}
