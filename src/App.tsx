import { useEffect, useMemo, useState } from "react";
import { Flame } from "lucide-react";
import "./styles/app.css";
import {
  archiveMessage,
  connectGmail,
  fetchFolder,
  fetchInboxPreview,
  getConnectedAccount,
  getReplyContext,
  getSettings,
  restoreMessage,
  deleteMessageForever,
  searchMessages,
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
import { isTauri } from "@tauri-apps/api/core";
import { startOfWeek, addWeeks, weekRangeLabel } from "./lib/calendar";
import { CalendarView } from "./components/CalendarView";
import type { View } from "./components/Header";
import { ComposeModal, type ComposeInitial } from "./components/ComposeModal";
import { SettingsModal } from "./components/SettingsModal";
import { Header } from "./components/Header";
import { MessageList } from "./components/MessageList";
import { ReadingPane } from "./components/ReadingPane";
import { SplitView } from "./components/SplitView";
import { FolderRail } from "./components/FolderRail";
import { FOLDERS, type Folder } from "./lib/folders";

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [stream, setStream] = useState<Stream>("all");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [compose, setCompose] = useState<ComposeInitial | null>(null);
  const [settings, setSettings] = useState<Settings>({ signature: "", remote_images: true, notifications: true });
  const [settingsOpen, setSettingsOpen] = useState(false);
  // M10: top-level Mail/Calendar view. Default to Calendar in browser mock mode so the
  // maket shows immediately; the Tauri app opens on Mail.
  const [view, setView] = useState<View>(isTauri() ? "mail" : "calendar");
  const [weekStart, setWeekStart] = useState<Date>(() => startOfWeek(new Date()));

  // M11 search. `inSearch` (a boolean, not array-nullability) marks search mode so both lists stay
  // the same non-null MessagePreview[] type and their setters unify in the `setActiveList` ternary.
  const [inSearch, setInSearch] = useState(false);
  const [searchResults, setSearchResults] = useState<MessagePreview[]>([]);
  const [searchSelectedId, setSearchSelectedId] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  // M12 folders. `folder === "inbox"` means the cached smart inbox; any other value is a live-
  // fetched mailbox. `folderReloadKey` lets re-clicking a folder (or the same one) refetch.
  const [folder, setFolder] = useState<Folder>("inbox");
  const [folderResults, setFolderResults] = useState<MessagePreview[]>([]);
  const [folderSelectedId, setFolderSelectedId] = useState<string | null>(null);
  const [folderLoading, setFolderLoading] = useState(false);
  const [folderReloadKey, setFolderReloadKey] = useState(0);
  const inFolder = folder !== "inbox";

  // Live-fetch the selected folder (non-inbox). Re-runs when the folder or reload key changes.
  useEffect(() => {
    if (folder === "inbox") return;
    let cancelled = false;
    setFolderLoading(true);
    setError(null);
    fetchFolder(folder, 50)
      .then((r) => {
        if (!cancelled) {
          setFolderResults(r);
          setFolderLoading(false);
        }
      })
      .catch((e) => {
        if (!cancelled) {
          setFolderResults([]);
          setError(String(e));
          setFolderLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [folder, folderReloadKey]);

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

  // Active list: search results > a live folder > the cached inbox. All selection + action
  // handlers operate on it, so they work identically across inbox, search, and folders.
  const activeList = inSearch ? searchResults : inFolder ? folderResults : messages;
  const setActiveList = inSearch ? setSearchResults : inFolder ? setFolderResults : setMessages;
  const activeSelectedId = inSearch ? searchSelectedId : inFolder ? folderSelectedId : selectedId;
  const setActiveSelectedId = inSearch ? setSearchSelectedId : inFolder ? setFolderSelectedId : setSelectedId;

  const selected = useMemo(
    () => activeList.find((m) => m.id === activeSelectedId) ?? null,
    [activeList, activeSelectedId],
  );

  // Pick the row to select after the current one is removed (archive/trash): next visible, else
  // previous, else nothing. Inbox uses the stream ordering; search results are already a flat list.
  function nextSelectedId(removedId: string): string | null {
    const visible = inSearch || inFolder ? activeList : orderedForStream(messages, stream);
    const idx = visible.findIndex((m) => m.id === removedId);
    if (idx === -1) return activeSelectedId;
    const next = visible[idx + 1] ?? visible[idx - 1] ?? null;
    return next ? next.id : null;
  }

  // Roll back to `snapshot` on the ACTIVE list and surface the error if the backend call rejects.
  async function withActiveRollback(
    snapshot: MessagePreview[],
    call: () => Promise<void>,
  ) {
    setError(null);
    try {
      await call();
    } catch (e) {
      setActiveList(snapshot);
      setError(String(e));
    }
  }

  function toggleRead(m: MessagePreview, read: boolean) {
    const snapshot = activeList;
    setActiveList(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, UNREAD, !read) : x)),
    );
    void withActiveRollback(snapshot, () => setMessageRead(m.id, read));
  }

  function toggleStar(m: MessagePreview) {
    const starred = !isStarred(m);
    const snapshot = activeList;
    setActiveList(
      snapshot.map((x) => (x.id === m.id ? withLabel(x, STARRED, starred) : x)),
    );
    void withActiveRollback(snapshot, () => setMessageStarred(m.id, starred));
  }

  function removeWithAction(m: MessagePreview, call: () => Promise<void>) {
    const listSnap = activeList;
    const selSnap = activeSelectedId;
    setActiveList(listSnap.filter((x) => x.id !== m.id));
    if (activeSelectedId === m.id) setActiveSelectedId(nextSelectedId(m.id));
    setError(null);
    call().catch((e) => {
      setActiveList(listSnap);
      setActiveSelectedId(selSnap);
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
    setActiveSelectedId(id);
    const m = activeList.find((x) => x.id === id);
    if (m && isUnread(m)) toggleRead(m, true);
  }

  async function handleSearch(q: string) {
    const query = q.trim();
    if (!query) return;
    setInSearch(true);
    setSearchQuery(query);
    setSearchSelectedId(null);
    setSearching(true);
    setError(null);
    try {
      setSearchResults(await searchMessages(query, 50));
    } catch (e) {
      setSearchResults([]);
      setError(String(e));
    } finally {
      setSearching(false);
    }
  }

  function handleClearSearch() {
    setInSearch(false);
    setSearchResults([]);
    setSearchSelectedId(null);
    setSearchQuery("");
    setError(null);
  }

  function handleSelectFolder(f: Folder) {
    // Switching mailbox leaves any active search; bumping the key refetches even on re-click.
    setInSearch(false);
    setSearchResults([]);
    setSearchSelectedId(null);
    setSearchQuery("");
    setFolderSelectedId(null);
    setFolder(f);
    setFolderReloadKey((k) => k + 1);
  }

  const handleRestore = (m: MessagePreview) =>
    removeWithAction(m, () => restoreMessage(m.id));
  const handleDeleteForever = (m: MessagePreview) =>
    removeWithAction(m, () => deleteMessageForever(m.id));

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
        view={view}
        onSelectView={setView}
        calendar={{
          rangeLabel: weekRangeLabel(weekStart),
          onPrev: () => setWeekStart((w) => addWeeks(w, -1)),
          onToday: () => setWeekStart(startOfWeek(new Date())),
          onNext: () => setWeekStart((w) => addWeeks(w, 1)),
        }}
        onSearch={handleSearch}
        onClearSearch={handleClearSearch}
        inSearch={inSearch}
        inFolder={inFolder}
        searching={searching}
      />
      {error && <div className="error-bar">{error}</div>}
      {view === "calendar" ? (
        <CalendarView weekStart={weekStart} />
      ) : (
        <div className="mail-body">
          <FolderRail folder={folder} onSelectFolder={handleSelectFolder} />
          <SplitView
            left={
              <MessageList
                messages={activeList}
                stream={stream}
                selectedId={activeSelectedId}
                onSelect={handleSelect}
                onArchive={handleArchive}
                onStar={toggleStar}
                flat={inSearch || inFolder}
                title={
                  inSearch
                    ? "Results"
                    : inFolder
                      ? FOLDERS.find((f) => f.key === folder)?.label
                      : undefined
                }
                emptyText={
                  inSearch
                    ? searching
                      ? "Searching…"
                      : `No results for "${searchQuery}".`
                    : inFolder
                      ? folderLoading
                        ? "Loading…"
                        : "Nothing here."
                      : undefined
                }
                showRecipient={folder === "sent"}
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
                folder={folder}
                onRestore={handleRestore}
                onDeleteForever={handleDeleteForever}
              />
            }
          />
        </div>
      )}
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
