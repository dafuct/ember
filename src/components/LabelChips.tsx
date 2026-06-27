import type { Label } from "../lib/api";

export function LabelChips({ labels }: { labels: Label[] }) {
  if (labels.length === 0) return null;
  return (
    <span className="label-chips">
      {labels.map((l) => (
        <span
          key={l.id}
          className="label-chip"
          style={l.color ? { background: l.color.background, color: l.color.text } : undefined}
        >
          {l.name}
        </span>
      ))}
    </span>
  );
}
