import { useEffect, useMemo, useState } from "react";
import { Flame } from "lucide-react";
import "./styles/app.css";
import {
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  syncInbox,
  type MessagePreview,
} from "./lib/api";
import { Header } from "./components/Header";
import { MessageList } from "./components/MessageList";
import { ReadingPane } from "./components/ReadingPane";

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getConnectedAccount()
      .then(setAccount)
      .catch(() => setAccount(null));
    fetchInboxPreview(50)
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
      const s = await syncInbox();
      setStatus(`${s.added} new, ${s.removed} removed`);
      setMessages(await fetchInboxPreview(50));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const selected = useMemo(
    () => messages.find((m) => m.id === selectedId) ?? null,
    [messages, selectedId],
  );

  if (!account) {
    return (
      <div className="app">
        <Header busy={busy} status={null} />
        <div className="connect-screen">
          <Flame size={40} className="brand-icon" />
          <h1 className="connect-title">Welcome to Ember</h1>
          <p className="connect-sub">Connect your Gmail to get started.</p>
          <button
            className="btn btn-accent"
            onClick={handleConnect}
            disabled={busy}
          >
            {busy ? "Connecting…" : "Connect Gmail"}
          </button>
          {error && <pre className="error-text">{error}</pre>}
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <Header busy={busy} onSync={handleSync} status={status} account={account} />
      {error && <div className="error-bar">{error}</div>}
      <div className="app-body">
        <MessageList
          messages={messages}
          selectedId={selectedId}
          onSelect={setSelectedId}
        />
        <ReadingPane msg={selected} />
      </div>
    </div>
  );
}
