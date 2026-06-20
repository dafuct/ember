import { useEffect, useState } from "react";
import { setSettings, disconnect, type Settings } from "../lib/api";
import { ensureNotificationPermission } from "../lib/notify";
import { isTauri } from "@tauri-apps/api/core";
import { useTheme, type Theme } from "../theme";
import { X } from "lucide-react";

export function SettingsModal({
  account,
  initial,
  onClose,
  onSaved,
  onDisconnected,
}: {
  account: string;
  initial: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  onDisconnected: () => void;
}) {
  const { theme, setTheme } = useTheme();
  const [signature, setSignature] = useState(initial.signature);
  const [remoteImages, setRemoteImages] = useState(initial.remote_images);
  const [notifications, setNotifications] = useState(initial.notifications);
  const [permBlocked, setPermBlocked] = useState(false);
  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Close on Esc from anywhere in the modal.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function handleSave() {
    const next: Settings = { signature, remote_images: remoteImages, notifications };
    setBusy(true);
    setError(null);
    try {
      await setSettings(next);
      onSaved(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDisconnect() {
    setBusy(true);
    setError(null);
    try {
      await disconnect();
      onDisconnected(); // unmounts the modal — intentionally no `finally` resetting busy
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <div className="compose-overlay">
      <div
        className="compose-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
      >
        <div className="compose-head">
          <span className="compose-title" id="settings-title">
            Settings
          </span>
          <button className="icon-btn" aria-label="Close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="settings-row">
          <span className="settings-label">Account</span>
          <span className="settings-value">{account}</span>
        </div>

        <div className="settings-row">
          <span className="settings-label">Theme</span>
          <div className="settings-control">
            {(["light", "dark"] as Theme[]).map((t) => (
              <button
                key={t}
                className={theme === t ? "seg-btn active" : "seg-btn"}
                onClick={() => setTheme(t)}
              >
                {t === "light" ? "Light" : "Dark"}
              </button>
            ))}
          </div>
        </div>

        <div className="settings-row">
          <span className="settings-label">Remote images</span>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={remoteImages}
              onChange={(e) => setRemoteImages(e.target.checked)}
            />
            <span>{remoteImages ? "Load automatically" : "Blocked"}</span>
          </label>
        </div>

        <div className="settings-row">
          <span className="settings-label">New-mail notifications</span>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={notifications}
              onChange={async (e) => {
                const on = e.target.checked;
                setNotifications(on);
                if (on) {
                  // Request OS permission immediately. A `false` result inside Tauri means
                  // the user denied it at the OS level; reflect that in the toggle. Outside
                  // Tauri (browser maket) the helper always returns false — not a denial.
                  const ok = await ensureNotificationPermission();
                  setPermBlocked(isTauri() && !ok);
                } else {
                  setPermBlocked(false);
                }
              }}
            />
            <span>{notifications ? (permBlocked ? "On — blocked in System Settings" : "On") : "Off"}</span>
          </label>
        </div>

        <div className="settings-field">
          <span className="settings-label">Signature</span>
          <textarea
            className="compose-body settings-signature"
            placeholder="Added to the bottom of messages you compose"
            value={signature}
            onChange={(e) => setSignature(e.target.value)}
            rows={4}
          />
        </div>

        {error && <div className="compose-error">{error}</div>}

        <div className="settings-disconnect">
          {confirmingDisconnect ? (
            <div className="settings-confirm">
              <span>Disconnect? This signs out and clears the local cache.</span>
              <button className="btn btn-danger" onClick={handleDisconnect} disabled={busy}>
                {busy ? "Disconnecting…" : "Disconnect"}
              </button>
              <button className="btn" onClick={() => setConfirmingDisconnect(false)} disabled={busy}>
                Keep connected
              </button>
            </div>
          ) : (
            <button
              className="btn btn-danger-outline"
              onClick={() => setConfirmingDisconnect(true)}
              disabled={busy}
            >
              Disconnect account
            </button>
          )}
        </div>

        <div className="compose-actions">
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn btn-accent" onClick={handleSave} disabled={busy}>
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
