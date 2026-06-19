import { useEffect, useMemo, useState } from "react";
import { Flame } from "lucide-react";
import "./styles/app.css";
import {
  archiveMessage,
  connectGmail,
  fetchInboxPreview,
  getConnectedAccount,
  getReplyContext,
  getSettings,
  setMessageRead,
  setMessageStarred,
  syncInbox,
  trashMessage,
  type MessagePreview,
  type Settings,
} from "./lib/api";
import { orderedForStream, type Stream } from "./lib/streams";
import { isStarred, isUnread, UNREAD, STARRED, withLabel } from "./lib/labels";
import { appendSignature, parseAddress, replySubject, quoteBody } from "./lib/compose";
import { ComposeModal, type ComposeInitial } from "./components/ComposeModal";
import { SettingsModal } from "./components/SettingsModal";
import { Header } from "./components/Header";
import { MessageList } from "./components/MessageList";
import { ReadingPane } from "./components/ReadingPane";
import { SplitView } from "./components/SplitView";

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [stream, setStream] = useState<Stream>("all");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [compose, setCompose] = useState<ComposeInitial | null>(null);
  const [settings, setSettings] = useState<Settings>({ signature: "", remote_images: true });
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    getConnectedAccount()
      .then(setAccount)
      .catch(() => setAccount(null));
    fetchInboxPreview(50)
      .then(setMessages)
      .catch(() => {});
    getSettings()
      .then(setSettings)
      .catch(() => {}); // keep defaults on error
  }, []);

  async function handleConnect() {
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      const acct = await connectGmail();
      setAccount(acct);
      // Onboarding: pull the inbox right away so the user doesn't land on an empty list.
      setStatus("Syncing your inbox…");
      const s = await syncInbox();
      setStatus(`${s.added} new, ${s.removed} removed`);
      setMessages(await fetchInboxPreview(50));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function handleDisconnected() {
    // Called by SettingsModal after the disconnect command succeeds.
    setSettingsOpen(false);
    setAccount(null);
    setMessages([]);
    setSelectedId(null);
    setStatus(null);
    setStream("all");
    setCompose(null);
    setError(null);
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

  // Pick the row to select after the current one is removed (archive/trash):
  // the next visible row, else the previous, else nothing. Uses the active stream's
  // ordering so selection lands on something the user can actually see.
  function nextSelectedId(removedId: string): string | null {
    const visible = orderedForStream(messages, stream);
    const idx = visible.findIndex((m) => m.id === removedId);
    if (idx === -1) return selectedId;
    const next = visible[idx + 1] ?? visible[idx - 1] ?? null;
    return next ? next.id : null;
  }

  // Roll back to `snapshot` and surface the error if the backend call rejects.
  // Captures explicit snapshots (not functional updates) — fine for single-user
  // clicks; rapid concurrent actions may roll back to a slightly stale list.
  async function withMessagesRollback(
    snapshot: MessagePreview[],
    call: () => Promise<void>,
  ) {
    setError(null);
    try {
      await call();
    } catch (e) {
      setMessages(snapshot);
      setError(String(e));
    }
  }

  function toggleRead(m: MessagePreview, read: boolean) {
    const snapshot = messages;
    setMessages(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, UNREAD, !read) : x)),
    );
    void withMessagesRollback(snapshot, () => setMessageRead(m.id, read));
  }

  function toggleStar(m: MessagePreview) {
    const starred = !isStarred(m);
    const snapshot = messages;
    setMessages(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, STARRED, starred) : x)),
    );
    void withMessagesRollback(snapshot, () => setMessageStarred(m.id, starred));
  }

  function removeWithAction(m: MessagePreview, call: () => Promise<void>) {
    const msgsSnap = messages;
    const selSnap = selectedId;
    setMessages(msgsSnap.filter((x) => x.id !== m.id));
    if (selectedId === m.id) setSelectedId(nextSelectedId(m.id));
    setError(null);
    call().catch((e) => {
      setMessages(msgsSnap);
      setSelectedId(selSnap);
      setError(String(e));
    });
  }

  const handleArchive = (m: MessagePreview) =>
    removeWithAction(m, () => archiveMessage(m.id));
  const handleTrash = (m: MessagePreview) =>
    removeWithAction(m, () => trashMessage(m.id));

  function openNewCompose() {
    setCompose({
      to: "",
      cc: "",
      subject: "",
      body: appendSignature("", settings.signature),
      inReplyTo: null,
      references: null,
      threadId: null,
    });
  }

  async function handleReply(m: MessagePreview) {
    setError(null);
    try {
      const ctx = await getReplyContext(m.id);
      const dateLabel = m.internal_date
        ? new Date(m.internal_date).toLocaleString()
        : m.date;
      setCompose({
        to: parseAddress(m.from),
        cc: "",
        subject: replySubject(m.subject),
        body: appendSignature(quoteBody(m.from, dateLabel, ctx.quoted_text), settings.signature),
        inReplyTo: ctx.message_id || null,
        references: ctx.references || ctx.message_id || null,
        threadId: m.thread_id || null,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  // Selecting a message opens it and (if unread) marks it read — like every mail client.
  function handleSelect(id: string) {
    setSelectedId(id);
    const m = messages.find((x) => x.id === id);
    if (m && isUnread(m)) toggleRead(m, true);
  }

  if (!account) {
    return (
      <div className="app">
        <Header busy={busy} status={null} />
        <div className="connect-screen">
          <Flame size={40} className="brand-icon" />
          <h1 className="connect-title">Welcome to Ember</h1>
          <p className="connect-sub">
            A local-first Gmail client — your mail stays on your Mac. Connect to get
            started; your inbox syncs automatically.
          </p>
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
      <Header
        busy={busy}
        onSync={handleSync}
        status={status}
        account={account}
        stream={stream}
        onSelectStream={(s) => {
          setStream(s);
          setSelectedId(null);
        }}
        onCompose={openNewCompose}
        onSettings={() => setSettingsOpen(true)}
      />
      {error && <div className="error-bar">{error}</div>}
      <SplitView
        left={
          <MessageList
            messages={messages}
            stream={stream}
            selectedId={selectedId}
            onSelect={handleSelect}
            onArchive={handleArchive}
            onStar={toggleStar}
          />
        }
        right={
          <ReadingPane
            msg={selected}
            loadImages={settings.remote_images}
            onArchive={handleArchive}
            onTrash={handleTrash}
            onToggleStar={toggleStar}
            onMarkUnread={(m) => toggleRead(m, false)}
            onReply={handleReply}
          />
        }
      />
      {compose && (
        <ComposeModal
          initial={compose}
          onClose={() => setCompose(null)}
          onSent={() => {
            setCompose(null);
            setStatus("Sent ✓");
          }}
        />
      )}
      {settingsOpen && (
        <SettingsModal
          account={account}
          initial={settings}
          onClose={() => setSettingsOpen(false)}
          onSaved={(s) => {
            setSettings(s);
            setSettingsOpen(false);
          }}
          onDisconnected={handleDisconnected}
        />
      )}
    </div>
  );
}
