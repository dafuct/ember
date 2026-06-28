export type Dir = 1 | -1;

export function nextId(ids: string[], currentId: string | null, dir: Dir): string | null {
  if (ids.length === 0) return null;
  const i = currentId ? ids.indexOf(currentId) : -1;
  if (i === -1) return dir === 1 ? ids[0] : ids[ids.length - 1];
  const j = i + dir;
  if (j < 0 || j >= ids.length) return ids[i];
  return ids[j];
}

export function rangeBetween(
  ids: string[],
  anchorId: string | null,
  leadId: string | null,
): string[] {
  if (!anchorId || !leadId) return [];
  const a = ids.indexOf(anchorId);
  const b = ids.indexOf(leadId);
  if (a === -1 || b === -1) return [];
  const [lo, hi] = a <= b ? [a, b] : [b, a];
  return ids.slice(lo, hi + 1);
}
