import { useState } from "react";
import { Flame } from "lucide-react";
import { setGoogleCredentials } from "../lib/api";

export function CredentialsSetup({
  onSaved,
  onBack,
}: {
  onSaved: () => void;
  onBack?: () => void;
}) {
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [showHelp, setShowHelp] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSave() {
    if (!clientId.trim() || !clientSecret.trim()) {
      setError("Enter both the Client ID and the Client secret.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await setGoogleCredentials(clientId.trim(), clientSecret.trim());
      onSaved();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <div className="app">
      <div className="connect-screen">
        <Flame size={40} className="brand-icon" />
        <h1 className="connect-title">Set up Google access</h1>
        <p className="connect-sub">
          Ember uses your own Google API credentials. Paste your OAuth Client ID and secret
          to continue — they're stored securely in your Mac's Keychain.
        </p>
        <input
          className="creds-input"
          placeholder="Client ID (…apps.googleusercontent.com)"
          value={clientId}
          onChange={(e) => setClientId(e.target.value)}
        />
        <input
          className="creds-input"
          type="password"
          placeholder="Client secret"
          value={clientSecret}
          onChange={(e) => setClientSecret(e.target.value)}
        />
        {error && <div className="compose-error">{error}</div>}
        <button className="btn btn-accent" onClick={handleSave} disabled={busy}>
          {busy ? "Saving…" : "Save & continue"}
        </button>
        <button className="creds-help-toggle" onClick={() => setShowHelp((s) => !s)}>
          {showHelp ? "Hide setup steps" : "How do I get these?"}
        </button>
        {onBack && (
          <button className="creds-help-toggle" onClick={onBack}>
            ← Back
          </button>
        )}
        {showHelp && (
          <ol className="creds-help">
            <li>Open the <a href="https://console.cloud.google.com/" target="_blank" rel="noreferrer">Google Cloud Console</a> and create a project.</li>
            <li>APIs &amp; Services → enable <b>Gmail API</b> and <b>Google Calendar API</b>.</li>
            <li>Credentials → Create credentials → OAuth client ID → <b>Desktop app</b>. Copy the Client ID + secret.</li>
            <li>OAuth consent screen → add your Google account as a <b>Test user</b>.</li>
          </ol>
        )}
      </div>
    </div>
  );
}
