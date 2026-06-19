import { useEffect, useState } from "react";
import { sendEmail, type SendEmailPayload } from "../lib/api";
import { parseRecipients, isPlausibleEmail } from "../lib/compose";
import { X } from "lucide-react";

export interface ComposeInitial {
  to: string; // comma-separated text (prefilled for reply)
  cc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  threadId: string | null;
}

export function ComposeModal({
  initial,
  onClose,
  onSent,
}: {
  initial: ComposeInitial;
  onClose: () => void;
  onSent: () => void;
}) {
  const [to, setTo] = useState(initial.to);
  const [cc, setCc] = useState(initial.cc);
  const [showCc, setShowCc] = useState(initial.cc.length > 0);
  const [subject, setSubject] = useState(initial.subject);
  const [body, setBody] = useState(initial.body);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Close on Esc from anywhere in the modal, not only when a field is focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const title = initial.threadId ? "Reply" : "New message";

  async function handleSend() {
    const toList = parseRecipients(to);
    const ccList = parseRecipients(cc);
    if (toList.length === 0 || !toList.every(isPlausibleEmail)) {
      setError("Enter at least one valid recipient address.");
      return;
    }
    if (ccList.length > 0 && !ccList.every(isPlausibleEmail)) {
      setError("One of the Cc addresses looks invalid.");
      return;
    }
    const payload: SendEmailPayload = {
      to: toList,
      cc: ccList,
      subject,
      body,
      in_reply_to: initial.inReplyTo,
      references: initial.references,
      thread_id: initial.threadId,
    };
    setSending(true);
    setError(null);
    try {
      await sendEmail(payload);
      onSent();
    } catch (e) {
      // Keep every field intact so the user can retry without retyping.
      setError(String(e));
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="compose-overlay">
      <div
        className="compose-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="compose-title"
      >
        <div className="compose-head">
          <span className="compose-title" id="compose-title">{title}</span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <input
          className="compose-field"
          placeholder="To"
          value={to}
          onChange={(e) => setTo(e.target.value)}
          autoFocus
        />
        {showCc ? (
          <input
            className="compose-field"
            placeholder="Cc"
            value={cc}
            onChange={(e) => setCc(e.target.value)}
          />
        ) : (
          <button className="compose-cc-toggle" onClick={() => setShowCc(true)}>
            Add Cc
          </button>
        )}
        <input
          className="compose-field"
          placeholder="Subject"
          value={subject}
          onChange={(e) => setSubject(e.target.value)}
        />
        <textarea
          className="compose-body"
          placeholder="Write your message…"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={12}
        />
        {error && <div className="compose-error">{error}</div>}
        <div className="compose-actions">
          <button className="btn" onClick={onClose} disabled={sending}>
            Cancel
          </button>
          <button
            className="btn btn-accent"
            onClick={handleSend}
            disabled={sending}
          >
            {sending ? "Sending…" : "Send"}
          </button>
        </div>
      </div>
    </div>
  );
}
