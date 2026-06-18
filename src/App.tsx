import { useEffect, useState } from "react";
import {
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  type MessagePreview,
} from "./lib/api";

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getConnectedAccount()
      .then(setAccount)
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

  async function handleLoad() {
    setBusy(true);
    setError(null);
    try {
      setMessages(await fetchInboxPreview(20));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main style={{ fontFamily: "system-ui", padding: 24, maxWidth: 720, margin: "0 auto" }}>
      <h1>Ember — M1</h1>
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
        <button onClick={handleLoad} disabled={busy} style={{ marginLeft: 8 }}>
          Load inbox preview
        </button>
      )}
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
