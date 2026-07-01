import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { searchPeople, type PersonHit } from "../lib/api";
import { isPlausibleEmail } from "../lib/compose";

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  const chars = parts.length === 1 ? parts[0].slice(0, 2) : parts[0][0] + parts[parts.length - 1][0];
  return chars.toUpperCase();
}

export function GuestField({
  value,
  onChange,
}: {
  value: string[];
  onChange: (emails: string[]) => void;
}) {
  const [text, setText] = useState("");
  const [hits, setHits] = useState<PersonHit[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const timer = useRef<number | undefined>(undefined);
  const gen = useRef(0);

  useEffect(() => {
    window.clearTimeout(timer.current);
    const q = text.trim();
    if (q.length < 1) {
      setHits([]);
      setOpen(false);
      return;
    }
    timer.current = window.setTimeout(async () => {
      const myGen = ++gen.current;
      try {
        const found = await searchPeople(q);
        if (gen.current !== myGen) return; // a newer query superseded this one
        setHits(found.filter((h) => !value.includes(h.email)));
        setActive(0);
        setOpen(true);
      } catch {
        if (gen.current !== myGen) return;
        setHits([]);
        setOpen(true);
      }
    }, 150);
    return () => window.clearTimeout(timer.current);
  }, [text, value]);

  function add(email: string) {
    const e = email.trim();
    if (!e || value.includes(e)) return;
    onChange([...value, e]);
    setText("");
    setHits([]);
    setOpen(false);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (open && hits.length > 0 && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      setActive((a) => {
        const next = e.key === "ArrowDown" ? a + 1 : a - 1;
        return Math.max(0, Math.min(hits.length - 1, next));
      });
    } else if (e.key === "Enter") {
      if (open && hits.length > 0 && hits[active]) {
        e.preventDefault();
        add(hits[active].email);
      } else if (text.trim() && isPlausibleEmail(text.trim())) {
        e.preventDefault();
        add(text.trim());
      }
    } else if (e.key === "," && text.trim()) {
      if (isPlausibleEmail(text.trim())) {
        e.preventDefault();
        add(text.trim());
      }
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      setOpen(false);
    } else if (e.key === "Backspace" && !text && value.length) {
      onChange(value.slice(0, -1));
    }
  }

  return (
    <div className="guest-field">
      <div className="guest-chips">
        {value.map((email) => (
          <span key={email} className="guest-chip">
            {email}
            <button type="button" aria-label={`Remove ${email}`} onClick={() => onChange(value.filter((v) => v !== email))}>
              <X size={12} />
            </button>
          </span>
        ))}
        <input
          className="guest-input"
          placeholder={value.length ? "" : "Add guests…"}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          onFocus={() => hits.length && setOpen(true)}
          onBlur={() => window.setTimeout(() => setOpen(false), 150)}
        />
      </div>
      {open && (
        <div className="guest-dropdown">
          {hits.length === 0 ? (
            <div className="guest-empty">No matches — type a full email address</div>
          ) : (
            hits.map((h, i) => (
              <button
                key={h.email}
                type="button"
                className={i === active ? "guest-option active" : "guest-option"}
                onMouseEnter={() => setActive(i)}
                onClick={() => add(h.email)}
              >
                <span className="guest-avatar">
                  {h.photo_url ? <img src={h.photo_url} alt="" /> : initials(h.name)}
                </span>
                <span className="guest-meta">
                  <span className="guest-name">{h.name}</span>
                  <span className="guest-email">{h.email}</span>
                </span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}
