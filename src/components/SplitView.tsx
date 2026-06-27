import {
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";

const STORAGE_KEY = "ember-list-width";
const MIN = 260;
const MAX = 640;
const DEFAULT = 360;

function initialWidth(): number {
  const saved = Number(localStorage.getItem(STORAGE_KEY));
  return saved >= MIN && saved <= MAX ? saved : DEFAULT;
}

export function SplitView({ left, right }: { left: ReactNode; right: ReactNode }) {
  const [width, setWidth] = useState(initialWidth);
  const widthRef = useRef(width);
  const dragging = useRef(false);
  const rootRef = useRef<HTMLDivElement>(null);

  function startDrag(e: ReactMouseEvent) {
    e.preventDefault();
    dragging.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }

  useEffect(() => {
    function onMove(e: MouseEvent) {
      if (!dragging.current || !rootRef.current) return;
      const rootLeft = rootRef.current.getBoundingClientRect().left;
      const next = Math.min(MAX, Math.max(MIN, e.clientX - rootLeft));
      widthRef.current = next;
      setWidth(next);
    }
    function onUp() {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      localStorage.setItem(STORAGE_KEY, String(Math.round(widthRef.current)));
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  return (
    <div className="split-root" ref={rootRef}>
      <div className="split-pane" style={{ width }}>
        {left}
      </div>
      <div
        className="resize-handle"
        onMouseDown={startDrag}
        role="separator"
        aria-orientation="vertical"
      />
      <div className="split-pane split-pane-grow">{right}</div>
    </div>
  );
}
