// A transient bottom-center toast offering to undo the last archive/trash. Auto-dismiss
// is managed by the parent (App) timer; this is a pure presentational component.
export function UndoToast({
  verb,
  count,
  onUndo,
  onDismiss,
}: {
  verb: string;
  count: number;
  onUndo: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="undo-toast" role="status">
      <span className="undo-text">
        {verb} {count} {count === 1 ? "message" : "messages"}
      </span>
      <button className="undo-btn" onClick={onUndo}>Undo</button>
      <button className="undo-close" aria-label="Dismiss" onClick={onDismiss}>✕</button>
    </div>
  );
}
