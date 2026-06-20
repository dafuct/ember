import type { Label } from "../lib/api";

// Render a message's user labels as small chips. Uses the label's Gmail color when set,
// else a uniform accent chip. Pure/presentational.
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
