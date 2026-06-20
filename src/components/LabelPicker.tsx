import { useEffect, useState } from "react";
import type { Label, MessagePreview } from "../lib/api";

// A small overlay popover for applying/removing user labels on `targets` (one message from
// the reading pane, or the multi-selection from the batch bar) + creating a new label.
// A label is "checked" only when EVERY target already has it (exact for one target).
export function LabelPicker({
  labels,
  targets,
  onApply,
  onCreate,
  onClose,
}: {
  labels: Label[];
  targets: MessagePreview[];
  onApply: (labelId: string, add: boolean) => void;
  onCreate: (name: string) => void;
  onClose: () => void;
}) {
  const [newName, setNewName] = useState("");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const appliedToAll = (id: string) =>
    targets.length > 0 && targets.every((m) => m.label_ids.includes(id));

  function handleCreate() {
    const name = newName.trim();
    if (!name) return;
    onCreate(name);
    setNewName("");
  }

  return (
    <div className="picker-overlay" onClick={onClose}>
      <div className="picker-card" role="dialog" aria-modal="true" aria-label="Labels" onClick={(e) => e.stopPropagation()}>
        <div className="picker-title">Label as</div>
        <div className="picker-list">
          {labels.length === 0 && <div className="picker-empty">No labels yet.</div>}
          {labels.map((l) => {
            const checked = appliedToAll(l.id);
            return (
              <label key={l.id} className="picker-row">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => onApply(l.id, !checked)}
                />
                <span
                  className="picker-dot"
                  style={l.color ? { background: l.color.background } : undefined}
                />
                <span className="picker-name">{l.name}</span>
              </label>
            );
          })}
        </div>
        <div className="picker-create">
          <input
            className="picker-input"
            placeholder="Create new label…"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleCreate();
            }}
          />
          <button className="btn" onClick={handleCreate} disabled={!newName.trim()}>
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
