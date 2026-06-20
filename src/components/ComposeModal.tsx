import { useEffect, useState } from "react";
import { sendEmail, saveDraft, sendDraft, deleteDraft, type SendEmailPayload } from "../lib/api";
import { parseRecipients, isPlausibleEmail } from "../lib/compose";
import { X, Paperclip } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { isTauri } from "@tauri-apps/api/core";
import { basename } from "../lib/attachments";
import { mockPickFiles } from "../lib/mock";

export interface ComposeInitial {
  to: string; // comma-separated text (prefilled for reply)
  cc: string;
  subject: string;
  body: string;
  inReplyTo: string | null;
  references: string | null;
  threadId: string | null;
  draftId?: string | null; // set when editing an existing Gmail draft
}

export function ComposeModal({
  initial,
  onClose,
  onSent,
  onDraftsChanged,
}: {
  initial: ComposeInitial;
  onClose: () => void;
  onSent: () => void;
  onDraftsChanged?: () => void; // called after a save/discard so the parent can refresh Drafts
}) {
  const [to, setTo] = useState(initial.to);
  const [cc, setCc] = useState(initial.cc);
  const [showCc, setShowCc] = useState(initial.cc.length > 0);
  const [subject, setSubject] = useState(initial.subject);
  const [body, setBody] = useState(initial.body);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false); // save/discard in flight
  const [confirmingClose, setConfirmingClose] = useState(false);
  const [draftId, setDraftId] = useState<string | null>(initial.draftId ?? null);
  const [attachPaths, setAttachPaths] = useState<string[]>([]);

  // "Dirty" = worth offering to save. A brand-new compose holding only its seeded body
  // (signature) is not dirty; editing a draft is dirty as soon as the body changes.
  const dirty =
    to.trim() !== "" || cc.trim() !== "" || subject.trim() !== "" || body !== initial.body || attachPaths.length > 0;

  function attemptClose() {
    if (dirty) setConfirmingClose(true);
    else onClose();
  }

  // Close on Esc from anywhere in the modal, not only when a field is focused.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") attemptClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, dirty]);

  const title = draftId ? "Draft" : initial.threadId ? "Reply" : "New message";

  function fields(): SendEmailPayload {
    return {
      to: parseRecipients(to),
      cc: parseRecipients(cc),
      subject,
      body,
      in_reply_to: initial.inReplyTo,
      references: initial.references,
      thread_id: initial.threadId,
      attachment_paths: attachPaths,
      forwarded_attachments: [], // wired to forwarded-attachment chips in the compose-modes task
    };
  }

  async function handleSend() {
    const f = fields();
    if (f.to.length === 0 || !f.to.every(isPlausibleEmail)) {
      setError("Enter at least one valid recipient address.");
      return;
    }
    if (f.cc.length > 0 && !f.cc.every(isPlausibleEmail)) {
      setError("One of the Cc addresses looks invalid.");
      return;
    }
    setSending(true);
    setError(null);
    try {
      if (draftId) await sendDraft({ ...f, draft_id: draftId });
      else await sendEmail(f);
      onSent();
    } catch (e) {
      // Minimal outbox: a failed send becomes a saved draft so nothing is lost.
      try {
        const id = await saveDraft({ ...f, draft_id: draftId });
        setDraftId(id);
        onDraftsChanged?.();
        setError(
          attachPaths.length > 0
            ? `Couldn't send (${String(e)}). Saved text to Drafts — attachments were NOT saved; re-attach and retry.`
            : `Couldn't send (${String(e)}). Saved to Drafts — retry from there.`,
        );
      } catch {
        setError("Couldn't send or save — you appear to be offline. Your message is still here.");
      }
    } finally {
      setSending(false);
    }
  }

  // Save without sending. No recipient validation — a draft can be incomplete.
  async function handleSaveDraft() {
    setBusy(true);
    setError(null);
    try {
      const id = await saveDraft({ ...fields(), draft_id: draftId });
      setDraftId(id);
      onDraftsChanged?.();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // Discard (permanently delete) the draft being edited.
  async function handleDeleteDraft() {
    if (!draftId) return;
    setBusy(true);
    setError(null);
    try {
      await deleteDraft(draftId);
      onDraftsChanged?.();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleAttach() {
    if (!isTauri()) {
      // Browser maket: stub a picked file so the chips render.
      setAttachPaths((p) => [...p, ...mockPickFiles()]);
      return;
    }
    const picked = await open({ multiple: true });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    setAttachPaths((p) => [...p, ...paths]);
  }

  function removeAttach(path: string) {
    setAttachPaths((p) => p.filter((x) => x !== path));
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
          <button className="icon-btn" aria-label="Close" onClick={attemptClose}>
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
        <div className="attach-row">
          <button className="compose-cc-toggle" onClick={handleAttach} type="button">
            <Paperclip size={13} /> Attach
          </button>
          {attachPaths.length > 0 && (
            <span className="attach-hint">Attachments send with the message but aren't saved to drafts yet.</span>
          )}
        </div>
        {attachPaths.length > 0 && (
          <div className="attach-file-chips">
            {attachPaths.map((p) => (
              <span key={p} className="attach-file-chip">
                <Paperclip size={12} />
                {basename(p)}
                <button className="attach-remove" aria-label={`Remove ${basename(p)}`} onClick={() => removeAttach(p)} type="button">
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        {error && <div className="compose-error">{error}</div>}
        {confirmingClose ? (
          <div className="compose-actions">
            <span className="settings-label">Save this draft before closing?</span>
            <button className="btn" onClick={() => setConfirmingClose(false)} disabled={busy}>
              Keep editing
            </button>
            <button className="btn" onClick={onClose} disabled={busy}>
              Discard
            </button>
            <button className="btn btn-accent" onClick={handleSaveDraft} disabled={busy}>
              Save draft
            </button>
          </div>
        ) : (
          <div className="compose-actions">
            {draftId && (
              <button className="btn btn-danger-outline" onClick={handleDeleteDraft} disabled={sending || busy}>
                Delete draft
              </button>
            )}
            <button className="btn" onClick={attemptClose} disabled={sending || busy}>
              Cancel
            </button>
            <button className="btn" onClick={handleSaveDraft} disabled={sending || busy}>
              Save as draft
            </button>
            <button className="btn btn-accent" onClick={handleSend} disabled={sending || busy}>
              {sending ? "Sending…" : "Send"}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
