import { useEffect, useState } from "react";
import {
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  syncInbox,
  type MessagePreview,
} from "./lib/api";

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // On mount: find the connected account and show whatever is already cached
  // in the local DB (instant, works offline — no network needed).
  useEffect(() => {
    getConnectedAccount()
      .then(setAccount)
      .catch(() => setAccount(null));
    fetchInboxPreview(20)
      .then(setMessages)
      .catch(() => {});
  }, []);

  async function handleConnect() {
    setBusy(true);
    setError(null);
    try {
      setAccount(await connectGmail());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleSync() {
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      const count = await syncInbox();
      setStatus(`Synced ${count} messages from Gmail`);
      setMessages(await fetchInboxPreview(20));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main style={{ fontFamily: "system-ui", padding: 24, maxWidth: 720, margin: "0 auto" }}>
      <h1>Ember — M2</h1>
      {account ? (
        <p>
          Connected as <strong>{account}</strong>
        </p>
      ) : (
        <button onClick={handleConnect} disabled={busy}>
          Connect Gmail
        </button>
      )}
      {account && (
        <button onClick={handleSync} disabled={busy}>
          {busy ? "Working…" : "Sync inbox"}
        </button>
      )}
      {status && <p style={{ color: "#2563eb" }}>{status}</p>}
      {error && <pre style={{ color: "crimson", whiteSpace: "pre-wrap" }}>{error}</pre>}
      <ul style={{ listStyle: "none", padding: 0 }}>
        {messages.map((m) => (
          <li key={m.id} style={{ borderBottom: "1px solid #eee", padding: "10px 0" }}>
            <div style={{ fontWeight: 600 }}>{m.from}</div>
            <div>{m.subject}</div>
            <div style={{ color: "#666", fontSize: 13 }}>{m.snippet}</div>
          </li>
        ))}
      </ul>
    </main>
  );
}
