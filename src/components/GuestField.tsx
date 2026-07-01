import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { searchPeople, type PersonHit } from "../lib/api";
import { isPlausibleEmail } from "../lib/compose";

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
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    window.clearTimeout(timer.current);
    const q = text.trim();
    if (q.length < 1) {
      setHits([]);
      return;
    }
    timer.current = window.setTimeout(async () => {
      try {
        const found = await searchPeople(q);
        setHits(found.filter((h) => !value.includes(h.email)));
        setOpen(true);
      } catch {
        setHits([]);
      }
    }, 250);
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
    if ((e.key === "Enter" || e.key === ",") && text.trim()) {
      e.preventDefault();
      if (isPlausibleEmail(text.trim())) add(text.trim());
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
        />
      </div>
      {open && (
        <div className="guest-dropdown">
          {hits.length === 0 ? (
            <div className="guest-empty">No matches — type a full email address</div>
          ) : (
            hits.map((h) => (
              <button key={h.email} type="button" className="guest-option" onClick={() => add(h.email)}>
                <span className="guest-avatar">{h.name.slice(0, 2).toUpperCase()}</span>
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
