import { useEffect, useState } from "react";
import {
  setSettings,
  googleCredentialsStatus,
  setGoogleCredentials,
  clearGoogleCredentials,
  zoomStatus,
  zoomConnect,
  zoomDisconnect,
  zoomCredentialsStatus,
  setZoomCredentials,
  clearZoomCredentials,
  type AccountInfo,
  type Settings,
  type CredentialStatus,
  type ZoomAccount,
} from "../lib/api";
import { ensureNotificationPermission } from "../lib/notify";
import { isTauri } from "@tauri-apps/api/core";
import { useTheme, type Theme } from "../theme";
import { X } from "lucide-react";

export function SettingsModal({
  accounts,
  initial,
  onClose,
  onSaved,
  onRemove,
  onAdd,
}: {
  accounts: AccountInfo[];
  initial: Settings;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  onRemove: (email: string) => Promise<void>;
  onAdd: () => void;
}) {
  const { theme, setTheme } = useTheme();
  const [signature, setSignature] = useState(initial.signature);
  const [remoteImages, setRemoteImages] = useState(initial.remote_images);
  const [notifications, setNotifications] = useState(initial.notifications);
  const [permBlocked, setPermBlocked] = useState(false);
  const [confirmingEmail, setConfirmingEmail] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [credStatus, setCredStatus] = useState<CredentialStatus | null>(null);
  const [credId, setCredId] = useState("");
  const [credSecret, setCredSecret] = useState("");
  const [credBusy, setCredBusy] = useState(false);
  const [credError, setCredError] = useState<string | null>(null);

  const [zoomAccount, setZoomAccount] = useState<ZoomAccount | null>(null);
  const [zoomBusy, setZoomBusy] = useState(false);
  const [zoomError, setZoomError] = useState<string | null>(null);
  const [zoomCredsOpen, setZoomCredsOpen] = useState(false);
  const [zoomCredStatus, setZoomCredStatus] = useState<string | null>(null);
  const [zoomCredId, setZoomCredId] = useState("");
  const [zoomCredSecret, setZoomCredSecret] = useState("");
  const [zoomCredBusy, setZoomCredBusy] = useState(false);
  const [zoomCredError, setZoomCredError] = useState<string | null>(null);

  useEffect(() => {
    googleCredentialsStatus().then(setCredStatus).catch(() => {});
    zoomStatus().then(setZoomAccount).catch(() => {});
    zoomCredentialsStatus().then(setZoomCredStatus).catch(() => {});
  }, []);

  async function handleZoomConnect() {
    setZoomBusy(true);
    setZoomError(null);
    try {
      await zoomConnect();
      setZoomAccount(await zoomStatus());
    } catch (e) {
      setZoomError(String(e));
    } finally {
      setZoomBusy(false);
    }
  }

  async function handleZoomDisconnect() {
    setZoomBusy(true);
    setZoomError(null);
    try {
      await zoomDisconnect();
      setZoomAccount(await zoomStatus());
    } catch (e) {
      setZoomError(String(e));
    } finally {
      setZoomBusy(false);
    }
  }

  async function handleSaveZoomCreds() {
    setZoomCredBusy(true);
    setZoomCredError(null);
    try {
      await setZoomCredentials(zoomCredId.trim(), zoomCredSecret.trim());
      setZoomCredId("");
      setZoomCredSecret("");
      setZoomCredStatus(await zoomCredentialsStatus());
    } catch (e) {
      setZoomCredError(String(e));
    } finally {
      setZoomCredBusy(false);
    }
  }

  async function handleClearZoomCreds() {
    setZoomCredBusy(true);
    setZoomCredError(null);
    try {
      await clearZoomCredentials();
      setZoomCredId("");
      setZoomCredSecret("");
      setZoomCredStatus(await zoomCredentialsStatus());
    } catch (e) {
      setZoomCredError(String(e));
    } finally {
      setZoomCredBusy(false);
    }
  }

  async function handleSaveCreds() {
    setCredBusy(true);
    setCredError(null);
    try {
      await setGoogleCredentials(credId.trim(), credSecret.trim());
      setCredId("");
      setCredSecret("");
      setCredStatus(await googleCredentialsStatus());
    } catch (e) {
      setCredError(String(e));
    } finally {
      setCredBusy(false);
    }
  }

  async function handleClearCreds() {
    setCredBusy(true);
    setCredError(null);
    try {
      await clearGoogleCredentials();
      setCredId("");
      setCredSecret("");
      setCredStatus(await googleCredentialsStatus());
    } catch (e) {
      setCredError(String(e));
    } finally {
      setCredBusy(false);
    }
  }

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

  async function handleRemove(email: string) {
    setBusy(true);
    setError(null);
    try {
      await onRemove(email);
      setConfirmingEmail(null);
    } catch (e) {
      setError(String(e));
    } finally {
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

        <div className="settings-field">
          <span className="settings-label">Accounts</span>
          {accounts.map((a) => (
            <div className="settings-row" key={a.email}>
              <span className="settings-value">
                {a.email}
                {a.active && <span className="settings-chip">Active</span>}
              </span>
              {confirmingEmail === a.email ? (
                <div className="settings-confirm">
                  <span>Remove?</span>
                  <button
                    className="btn btn-danger"
                    onClick={() => handleRemove(a.email)}
                    disabled={busy}
                  >
                    {busy ? "Removing…" : "Remove"}
                  </button>
                  <button
                    className="btn"
                    onClick={() => setConfirmingEmail(null)}
                    disabled={busy}
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <button
                  className="btn btn-danger-outline"
                  onClick={() => setConfirmingEmail(a.email)}
                  disabled={busy}
                >
                  Remove
                </button>
              )}
            </div>
          ))}
          <div className="settings-disconnect">
            <button className="btn" onClick={onAdd} disabled={busy}>
              Add account
            </button>
          </div>
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

        <div className="settings-field">
          <span className="settings-label">Google API</span>
          <div className="settings-creds-status">
            {credStatus === null
              ? "Checking…"
              : credStatus.source === "stored"
              ? "Using your saved key"
              : credStatus.source === "none"
              ? "Not configured"
              : "Using built-in key"}
          </div>
          <input
            className="creds-input"
            placeholder="Client ID"
            value={credId}
            onChange={(e) => setCredId(e.target.value)}
          />
          <input
            className="creds-input"
            type="password"
            placeholder="Client secret"
            value={credSecret}
            onChange={(e) => setCredSecret(e.target.value)}
          />
          <div className="settings-creds-actions">
            <button
              className="btn btn-accent"
              disabled={credBusy || !credId.trim() || !credSecret.trim()}
              onClick={handleSaveCreds}
            >
              Save key
            </button>
            {credStatus?.source === "stored" && (
              <button className="btn btn-danger-outline" disabled={credBusy} onClick={handleClearCreds}>
                Clear saved key
              </button>
            )}
          </div>
          {credError && <div className="compose-error">{credError}</div>}
        </div>

        <div className="settings-field">
          <span className="settings-label">Zoom</span>
          <div className="settings-row">
            <span className="settings-value">
              {zoomAccount ? (
                <>
                  Connected as {zoomAccount.email}
                  <span className="settings-chip">Active</span>
                </>
              ) : (
                "Not connected"
              )}
            </span>
            {zoomAccount ? (
              <button className="btn btn-danger-outline" disabled={zoomBusy} onClick={handleZoomDisconnect}>
                {zoomBusy ? "Disconnecting…" : "Disconnect"}
              </button>
            ) : (
              <button className="btn btn-accent" disabled={zoomBusy} onClick={handleZoomConnect}>
                {zoomBusy ? "Connecting…" : "Connect Zoom"}
              </button>
            )}
          </div>
          {zoomError && <div className="compose-error">{zoomError}</div>}

          <button
            className="creds-help-toggle"
            onClick={() => setZoomCredsOpen((s) => !s)}
          >
            {zoomCredsOpen ? "Hide Zoom API credentials" : "Zoom API credentials"}
          </button>
          {zoomCredsOpen && (
            <>
              <div className="settings-creds-status">
                {zoomCredStatus === null
                  ? "Checking…"
                  : zoomCredStatus === "stored"
                  ? "Using your saved key"
                  : zoomCredStatus === "none"
                  ? "Not configured"
                  : "Using built-in key"}
              </div>
              <input
                className="creds-input"
                placeholder="Client ID"
                value={zoomCredId}
                onChange={(e) => setZoomCredId(e.target.value)}
              />
              <input
                className="creds-input"
                type="password"
                placeholder="Client secret"
                value={zoomCredSecret}
                onChange={(e) => setZoomCredSecret(e.target.value)}
              />
              <div className="settings-creds-actions">
                <button
                  className="btn btn-accent"
                  disabled={zoomCredBusy || !zoomCredId.trim() || !zoomCredSecret.trim()}
                  onClick={handleSaveZoomCreds}
                >
                  Save key
                </button>
                {zoomCredStatus === "stored" && (
                  <button className="btn btn-danger-outline" disabled={zoomCredBusy} onClick={handleClearZoomCreds}>
                    Clear saved key
                  </button>
                )}
              </div>
              {zoomCredError && <div className="compose-error">{zoomCredError}</div>}
            </>
          )}
        </div>

        {error && <div className="compose-error">{error}</div>}

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
