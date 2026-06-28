import { useEffect, useMemo, useRef, useState } from "react";
import { Flame } from "lucide-react";
import "./styles/app.css";
import {
  batchModifyMessages,
  connectGmail,
  fetchFolder,
  fetchInboxPreview,
  getConnectedAccount,
  getDraft,
  getReplyContext,
  getSettings,
  googleCredentialsStatus,
  restoreMessage,
  deleteMessageForever,
  batchRestoreMessages,
  batchDeleteMessages,
  searchMessages,
  setMessageRead,
  setMessageStarred,
  syncInbox,
  listLabels,
  createLabel,
  fetchLabel,
  listAccounts,
  setActiveAccount,
  removeAccount,
  syncAllAccounts,
  type AccountInfo,
  type AccountSyncSummary,
  type Label,
  type MessagePreview,
  type Settings,
} from "./lib/api";
import { orderedForStream, type Stream } from "./lib/streams";
import { isStarred, isUnread, UNREAD, STARRED, withLabel } from "./lib/labels";
import { pickNewMail, notifyNewMail, ensureNotificationPermission } from "./lib/notify";
import { appendSignature, parseAddress, replySubject, quoteBody, forwardSubject, replyAllRecipients, forwardBlock } from "./lib/compose";
import { isTauri } from "@tauri-apps/api/core";
import { onAction } from "@tauri-apps/plugin-notification";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { startOfWeek, addWeeks, weekRangeLabel } from "./lib/calendar";
import { CalendarView } from "./components/CalendarView";
import { ComposeModal, type ComposeInitial } from "./components/ComposeModal";
import { SettingsModal } from "./components/SettingsModal";
import { CredentialsSetup } from "./components/CredentialsSetup";
import { AccountSwitcher } from "./components/AccountSwitcher";
import { IconRail } from "./components/IconRail";
import { Sidebar } from "./components/Sidebar";
import { MessageList } from "./components/MessageList";
import { UndoToast } from "./components/UndoToast";
import { ReadingPane } from "./components/ReadingPane";
import { SplitView } from "./components/SplitView";
import { LabelPicker } from "./components/LabelPicker";
import { SnoozeMenu } from "./components/SnoozeMenu";
import { SnoozedList } from "./components/SnoozedList";
import { snoozeMessage, listSnoozed, unsnoozeMessage, wakeDueSnoozes, type SnoozedRow } from "./lib/snooze";
import { FOLDERS } from "./lib/folders";

type View = "mail" | "calendar";

const POLL_MS = 60_000;
const MAX_NOTIFY_PER_SYNC = 5;
const INBOX_PAGE = 50;

export default function App() {
  const [account, setAccount] = useState<string | null>(null);
  const [messages, setMessages] = useState<MessagePreview[]>([]);
  const [inboxLimit, setInboxLimit] = useState(50);
  const [atEnd, setAtEnd] = useState(false);
  const loadingMoreRef = useRef(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [stream, setStream] = useState<Stream>("all");
  const [busy, setBusy] = useState(false);
  const [, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [compose, setCompose] = useState<ComposeInitial | null>(null);
  const [settings, setSettings] = useState<Settings>({ signature: "", remote_images: true, notifications: true });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [credsConfigured, setCredsConfigured] = useState<boolean | null>(null);
  const [accounts, setAccounts] = useState<AccountInfo[]>([]);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [accountEpoch, setAccountEpoch] = useState(0);
  const [view, setView] = useState<View>(isTauri() ? "mail" : "calendar");
  const [weekStart, setWeekStart] = useState<Date>(() => startOfWeek(new Date()));

  const [inSearch, setInSearch] = useState(false);
  const [searchResults, setSearchResults] = useState<MessagePreview[]>([]);
  const [searchSelectedId, setSearchSelectedId] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const [folder, setFolder] = useState<string>("inbox");
  const [folderResults, setFolderResults] = useState<MessagePreview[]>([]);
  const [folderSelectedId, setFolderSelectedId] = useState<string | null>(null);
  const [folderLoading, setFolderLoading] = useState(false);
  const [folderReloadKey, setFolderReloadKey] = useState(0);
  const inFolder = folder !== "inbox";

  const syncingRef = useRef(false);
  const knownIdsRef = useRef<Set<string>>(new Set());
  const notifyAllowedRef = useRef(false);
  const lastNotifiedIdRef = useRef<string | null>(null);
  function seedKnown(list: MessagePreview[]) {
    for (const m of list) knownIdsRef.current.add(m.id);
  }

  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [undo, setUndo] = useState<{ verb: string; count: number; onUndo: () => void } | null>(null);
  const undoTimer = useRef<number | null>(null);

  const [labels, setLabels] = useState<Label[]>([]);
  const labelsById = useMemo(() => new Map(labels.map((l) => [l.id, l])), [labels]);
  const [labelPicker, setLabelPicker] = useState<MessagePreview[] | null>(null);

  const [snoozeTarget, setSnoozeTarget] = useState<{ msg: MessagePreview; x: number; y: number } | null>(null);
  const [snoozedRows, setSnoozedRows] = useState<SnoozedRow[]>([]);
  const openSnoozeMenu = (msg: MessagePreview, e: { clientX: number; clientY: number }) =>
    setSnoozeTarget({ msg, x: e.clientX, y: e.clientY });
  const handleSnoozePick = (wakeAt: number) => {
    const t = snoozeTarget;
    if (!t) return;
    setSnoozeTarget(null);
    removeWithAction(t.msg, () => snoozeMessage(t.msg, wakeAt));
  };
  const handleUnsnooze = (id: string) => {
    setSnoozedRows((r) => r.filter((x) => x.message_id !== id));
    unsnoozeMessage(id).catch((e) => setError(String(e)));
  };

  useEffect(() => {
    if (folder === "inbox") return;
    if (folder === "snoozed") {
      listSnoozed().then(setSnoozedRows).catch((e) => setError(String(e)));
      return;
    }
    let cancelled = false;
    setFolderLoading(true);
    setError(null);
    const isSystem = FOLDERS.some((f) => f.key === folder);
    (isSystem ? fetchFolder(folder, 50) : fetchLabel(folder, 50))
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
    googleCredentialsStatus()
      .then((s) => setCredsConfigured(s.configured))
      .catch(() => setCredsConfigured(true));
    fetchInboxPreview(inboxLimit)
      .then((list) => {
        setMessages(list);
        setAtEnd(list.length < inboxLimit);
        seedKnown(list);
      })
      .catch(() => {});
    getSettings()
      .then(setSettings)
      .catch(() => {});
    listLabels()
      .then(setLabels)
      .catch(() => {});
    listAccounts()
      .then(setAccounts)
      .catch(() => {});
  }, []);

  async function handleConnect() {
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      const acct = await connectGmail();
      setAccount(acct);
      setStatus("Syncing your inbox…");
      const s = await syncInbox();
      setStatus(`${s.added} new, ${s.removed} removed`);
      const list = await fetchInboxPreview(inboxLimit);
      setMessages(list);
      setAtEnd(list.length < inboxLimit);
      seedKnown(list);
      setAccounts(await listAccounts());
      setAccountEpoch((e) => e + 1);
    } catch (e) {
      const msg = String(e);
      if (msg.includes("no Google credentials configured")) {
        setCredsConfigured(false);
      } else {
        setError(msg);
      }
    } finally {
      setBusy(false);
    }
  }

  async function handleSwitchAccount(email: string) {
    try {
      await setActiveAccount(email);
      setAccount(email);
      setSelectedId(null);
      setStream("all");
      setFolder("inbox");
      setInboxLimit(50);
      setAtEnd(false);
      setAccountEpoch((e) => e + 1);
      const [accs, list, labs] = await Promise.all([listAccounts(), fetchInboxPreview(50), listLabels()]);
      setAccounts(accs);
      setMessages(list);
      setAtEnd(list.length < 50);
      seedKnown(list);
      setLabels(labs);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRemoveAccount(email: string) {
    try {
      const next = await removeAccount(email);
      const accs = await listAccounts();
      setAccounts(accs);
      if (next) {
        setAccount(next);
        setSelectedId(null);
        setStream("all");
        setFolder("inbox");
        setInboxLimit(50);
        setAtEnd(false);
        setAccountEpoch((e) => e + 1);
        const [list, labs] = await Promise.all([fetchInboxPreview(50), listLabels()]);
        setMessages(list);
        setAtEnd(list.length < 50);
        seedKnown(list);
        setLabels(labs);
      } else {
        handleDisconnected();
      }
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    if (!account || !settings.notifications) {
      notifyAllowedRef.current = false;
      return;
    }
    ensureNotificationPermission().then((ok) => {
      notifyAllowedRef.current = ok;
    });
  }, [account, settings.notifications]);

  useEffect(() => {
    if (!account || !settings.notifications) return;
    const id = setInterval(() => void runBackgroundSyncRef.current(), POLL_MS);
    return () => clearInterval(id);
  }, [account, settings.notifications]);

  useEffect(() => {
    if (!isTauri()) return;
    let listener: Awaited<ReturnType<typeof onAction>> | undefined;
    onAction(() => {
      void getCurrentWindow().setFocus();
      const id = lastNotifiedIdRef.current;
      if (id) openMessageFromNotification(id);
    })
      .then((un) => {
        listener = un;
      })
      .catch((e) => console.warn("[ember] onAction subscribe failed:", e));
    return () => void listener?.unregister();
  }, []);

  function handleDisconnected() {
    setSettingsOpen(false);
    setAccount(null);
    setMessages([]);
    setSelectedId(null);
    setStatus(null);
    setStream("all");
    setCompose(null);
    setError(null);
  }

  async function runSync(surfaceErrors: boolean) {
    if (syncingRef.current) return;
    syncingRef.current = true;
    if (surfaceErrors) {
      setBusy(true);
      setError(null);
      setStatus(null);
    }
    try {
      const s = await syncInbox();
      const list = await fetchInboxPreview(inboxLimit);
      setMessages(list);
      setAtEnd(list.length < inboxLimit);
      if (surfaceErrors) setStatus(`${s.added} new, ${s.removed} removed`);

      const fresh = pickNewMail(knownIdsRef.current, list, MAX_NOTIFY_PER_SYNC);
      for (const m of list) knownIdsRef.current.add(m.id);
      if (fresh.length && notifyAllowedRef.current && !document.hasFocus()) {
        lastNotifiedIdRef.current = fresh[0].id;
        for (const m of fresh) void notifyNewMail(m);
      }
    } catch (e) {
      if (surfaceErrors) setError(String(e));
      else console.warn("[ember] background sync failed:", e);
    } finally {
      syncingRef.current = false;
      if (surfaceErrors) setBusy(false);
    }
  }

  const handleSync = () => runSync(true);

  async function handleLoadMore() {
    if (loadingMoreRef.current || atEnd) return;
    loadingMoreRef.current = true;
    try {
      const next = inboxLimit + INBOX_PAGE;
      const list = await fetchInboxPreview(next);
      setInboxLimit(next);
      setMessages(list);
      setAtEnd(list.length < next);
    } catch (e) {
      console.warn("[ember] load more failed:", e);
    } finally {
      loadingMoreRef.current = false;
    }
  }

  useEffect(() => {
    if (inSearch || inFolder || atEnd) return;
    if (messages.length === 0 || loadingMoreRef.current) return;
    void handleLoadMore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inSearch, inFolder, atEnd, messages.length]);

  async function runBackgroundSync() {
    if (syncingRef.current) return;
    syncingRef.current = true;
    try {
      const summaries: AccountSyncSummary[] = await syncAllAccounts();
      for (const s of summaries) {
        if (s.account === account) {
          const list = await fetchInboxPreview(inboxLimit);
          setMessages(list);
          setAtEnd(list.length < inboxLimit);
          for (const m of list) knownIdsRef.current.add(m.id);
        }
        if (!s.baseline && s.new_previews.length && notifyAllowedRef.current && !document.hasFocus()) {
          const fresh = s.new_previews
            .filter((m) => !knownIdsRef.current.has(m.id))
            .sort((a, b) => b.internal_date - a.internal_date)
            .slice(0, MAX_NOTIFY_PER_SYNC);
          for (const m of fresh) { knownIdsRef.current.add(m.id); void notifyNewMail(m, s.account); }
        }
      }
    } catch (e) {
      console.warn("[ember] background all-accounts sync failed:", e);
    } finally {
      syncingRef.current = false;
    }
  }

  const runSyncRef = useRef(runSync);
  runSyncRef.current = runSync;

  const runBackgroundSyncRef = useRef(runBackgroundSync);
  runBackgroundSyncRef.current = runBackgroundSync;

  useEffect(() => {
    if (!account) return;
    const tick = () => wakeDueSnoozes().then((woken) => { if (woken.length > 0) void runSyncRef.current(false); }).catch(() => {});
    tick();
    const id = setInterval(tick, 60_000);
    return () => clearInterval(id);
  }, [account]);

  const activeList = inSearch ? searchResults : inFolder ? folderResults : messages;
  const setActiveList = inSearch ? setSearchResults : inFolder ? setFolderResults : setMessages;
  const activeSelectedId = inSearch ? searchSelectedId : inFolder ? folderSelectedId : selectedId;
  const setActiveSelectedId = inSearch ? setSearchSelectedId : inFolder ? setFolderSelectedId : setSelectedId;

  const selected = useMemo(
    () => activeList.find((m) => m.id === activeSelectedId) ?? null,
    [activeList, activeSelectedId],
  );

  const selectedMsgs = useMemo(
    () => activeList.filter((m) => selectedIds.has(m.id)),
    [activeList, selectedIds],
  );
  function toggleSelect(id: string) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }
  function clearSelection() {
    setSelectedIds(new Set());
  }
  function selectAllVisible(ids: string[]) {
    setSelectedIds((prev) => {
      const allSelected = ids.length > 0 && ids.every((id) => prev.has(id));
      return allSelected ? new Set() : new Set(ids);
    });
  }
  function selectRange(ids: string[]) {
    setSelectedIds(new Set(ids));
  }
  function clearUndo() {
    if (undoTimer.current) clearTimeout(undoTimer.current);
    undoTimer.current = null;
    setUndo(null);
  }
  function registerUndo(
    verb: string,
    rows: MessagePreview[],
    ids: string[],
    inverse: { add: string[]; remove: string[] },
  ) {
    if (undoTimer.current) clearTimeout(undoTimer.current);
    const onUndo = () => {
      clearUndo();
      setActiveList((cur) => {
        const have = new Set(cur.map((m) => m.id));
        const merged = [...cur, ...rows.filter((r) => !have.has(r.id))];
        merged.sort((a, b) => b.internal_date - a.internal_date);
        return merged;
      });
      batchModifyMessages(ids, inverse.add, inverse.remove).catch((e) => setError(String(e)));
    };
    setUndo({ verb, count: ids.length, onUndo });
    undoTimer.current = window.setTimeout(() => setUndo(null), 6000);
  }

  function nextSelectedId(removedId: string): string | null {
    const visible = inSearch || inFolder ? activeList : orderedForStream(messages, stream);
    const idx = visible.findIndex((m) => m.id === removedId);
    if (idx === -1) return activeSelectedId;
    const next = visible[idx + 1] ?? visible[idx - 1] ?? null;
    return next ? next.id : null;
  }

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

  function removeMessages(
    msgs: MessagePreview[],
    op: { add: string[]; remove: string[]; verb: string },
  ) {
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const listSnap = activeList;
    const selSnap = activeSelectedId;
    setActiveList(listSnap.filter((m) => !idSet.has(m.id)));
    if (activeSelectedId && idSet.has(activeSelectedId)) {
      setActiveSelectedId(ids.length === 1 ? nextSelectedId(activeSelectedId) : null);
    }
    clearSelection();
    setError(null);
    batchModifyMessages(ids, op.add, op.remove)
      .then(() => registerUndo(op.verb, msgs, ids, { add: op.remove, remove: op.add }))
      .catch((e) => {
        setActiveList(listSnap);
        setActiveSelectedId(selSnap);
        setError(String(e));
      });
  }

  const handleArchive = (m: MessagePreview) =>
    removeMessages([m], { add: [], remove: ["INBOX"], verb: "Archived" });
  const handleTrash = (m: MessagePreview) =>
    removeMessages([m], { add: ["TRASH"], remove: [], verb: "Trashed" });

  const batchArchive = () =>
    removeMessages(selectedMsgs, { add: [], remove: ["INBOX"], verb: "Archived" });
  const batchTrash = () =>
    removeMessages(selectedMsgs, { add: ["TRASH"], remove: [], verb: "Trashed" });

  function batchMarkRead() {
    const msgs = selectedMsgs;
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const snap = activeList;
    setActiveList(snap.map((m) => (idSet.has(m.id) ? withLabel(m, UNREAD, false) : m)));
    clearSelection();
    setError(null);
    batchModifyMessages(ids, [], ["UNREAD"]).catch((e) => {
      setActiveList(snap);
      setError(String(e));
    });
  }
  function batchStar() {
    const msgs = selectedMsgs;
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const snap = activeList;
    setActiveList(snap.map((m) => (idSet.has(m.id) ? withLabel(m, STARRED, true) : m)));
    clearSelection();
    setError(null);
    batchModifyMessages(ids, ["STARRED"], []).catch((e) => {
      setActiveList(snap);
      setError(String(e));
    });
  }

  function applyLabel(targets: MessagePreview[], labelId: string, add: boolean) {
    if (targets.length === 0) return;
    const ids = targets.map((m) => m.id);
    const idSet = new Set(ids);
    const snap = activeList;
    setActiveList(snap.map((m) => (idSet.has(m.id) ? withLabel(m, labelId, add) : m)));
    setError(null);
    batchModifyMessages(ids, add ? [labelId] : [], add ? [] : [labelId]).catch((e) => {
      setActiveList(snap);
      setError(String(e));
    });
  }

  async function handleCreateLabel(name: string, targets: MessagePreview[]) {
    setError(null);
    try {
      const created = await createLabel(name);
      const next = await listLabels();
      setLabels(next);
      applyLabel(targets, created.id, true);
    } catch (e) {
      setError(String(e));
    }
  }

  function openNewCompose() {
    setCompose({
      to: "",
      cc: "",
      subject: "",
      body: appendSignature("", settings.signature),
      inReplyTo: null,
      references: null,
      threadId: null,
      draftId: null,
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
        draftId: null,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleReplyAll(m: MessagePreview) {
    setError(null);
    try {
      const ctx = await getReplyContext(m.id);
      const dateLabel = m.internal_date ? new Date(m.internal_date).toLocaleString() : m.date;
      const r = replyAllRecipients(m.from, ctx.to, ctx.cc, account ?? "");
      setCompose({
        to: r.to,
        cc: r.cc,
        subject: replySubject(m.subject),
        body: appendSignature(quoteBody(m.from, dateLabel, ctx.quoted_text), settings.signature),
        inReplyTo: ctx.message_id || null,
        references: ctx.references || ctx.message_id || null,
        threadId: m.thread_id || null,
        draftId: null,
        mode: "replyAll",
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleForward(m: MessagePreview) {
    setError(null);
    try {
      const ctx = await getReplyContext(m.id);
      const dateLabel = m.internal_date ? new Date(m.internal_date).toLocaleString() : m.date;
      setCompose({
        to: "",
        cc: "",
        subject: forwardSubject(m.subject),
        body: appendSignature(
          forwardBlock(m.from, dateLabel, m.subject, ctx.to) + ctx.quoted_text,
          settings.signature,
        ),
        inReplyTo: null,
        references: null,
        threadId: null,
        draftId: null,
        mode: "forward",
        forwardedAttachments: ctx.attachments.map((a) => ({
          message_id: m.id,
          attachment_id: a.attachment_id,
          filename: a.filename,
          mime_type: a.mime_type,
        })),
      });
    } catch (e) {
      setError(String(e));
    }
  }

  function handleSelect(id: string) {
    setActiveSelectedId(id);
    const m = activeList.find((x) => x.id === id);
    if (m && isUnread(m)) toggleRead(m, true);
  }

  async function handleOpenDraft(m: MessagePreview) {
    if (!m.draft_id) return;
    setError(null);
    try {
      const d = await getDraft(m.draft_id);
      setCompose({
        to: d.to,
        cc: d.cc,
        subject: d.subject,
        body: d.body,
        inReplyTo: d.in_reply_to,
        references: d.references,
        threadId: d.thread_id,
        draftId: d.draft_id,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  function handleRowSelect(id: string) {
    if (!inSearch && folder === "drafts") {
      const m = activeList.find((x) => x.id === id);
      if (m) void handleOpenDraft(m);
    } else {
      handleSelect(id);
    }
  }

  function openMessageFromNotification(id: string) {
    setView("mail");
    setInSearch(false);
    setSearchResults([]);
    setSearchQuery("");
    setSearchSelectedId(null);
    setFolder("inbox");
    setFolderSelectedId(null);
    setStream("all");
    setSelectedId(id);
    const m = messages.find((x) => x.id === id);
    if (m && isUnread(m)) {
      setMessages((prev) => prev.map((x) => (x.id === id ? withLabel(x, UNREAD, false) : x)));
      void setMessageRead(id, true).catch(() =>
        setMessages((prev) => prev.map((x) => (x.id === id ? withLabel(x, UNREAD, true) : x))),
      );
    }
  }

  async function handleSearch(q: string) {
    clearSelection();
    clearUndo();
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
    clearSelection();
    clearUndo();
    setInSearch(false);
    setSearchResults([]);
    setSearchSelectedId(null);
    setSearchQuery("");
    setError(null);
  }

  function handleSelectFolder(f: string) {
    clearSelection();
    clearUndo();
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

  function removeBatch(msgs: MessagePreview[], call: (ids: string[]) => Promise<void>) {
    if (msgs.length === 0) return;
    const ids = msgs.map((m) => m.id);
    const idSet = new Set(ids);
    const listSnap = activeList;
    const selSnap = activeSelectedId;
    setActiveList(listSnap.filter((m) => !idSet.has(m.id)));
    if (activeSelectedId && idSet.has(activeSelectedId)) {
      setActiveSelectedId(ids.length === 1 ? nextSelectedId(activeSelectedId) : null);
    }
    clearSelection();
    setError(null);
    call(ids).catch((e) => {
      setActiveList(listSnap);
      setActiveSelectedId(selSnap);
      setError(String(e));
    });
  }

  const batchRestore = () => removeBatch(selectedMsgs, batchRestoreMessages);
  const batchDeleteForever = () => {
    const n = selectedMsgs.length;
    if (n === 0) return;
    if (!window.confirm(`Permanently delete ${n} message${n === 1 ? "" : "s"}? This can't be undone.`)) {
      return;
    }
    removeBatch(selectedMsgs, batchDeleteMessages);
  };

  if (!account) {
    if (credsConfigured === false) {
      return (
        <CredentialsSetup
          onSaved={() => {
            setCredsConfigured(true);
            setError(null);
          }}
        />
      );
    }
    return (
      <div className="app">
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

  const keyboardEnabled =
    !compose && !settingsOpen && !labelPicker && !snoozeTarget && !switcherOpen;

  return (
    <div className="app">
      <div className="shell">
        <IconRail
          view={view}
          onSelectView={setView}
          onCompose={openNewCompose}
          onAvatar={() => setSwitcherOpen(true)}
          account={account}
        />
        {switcherOpen && (
          <AccountSwitcher
            accounts={accounts}
            onSwitch={handleSwitchAccount}
            onAdd={() => { setSwitcherOpen(false); void handleConnect(); }}
            onManage={() => { setSwitcherOpen(false); setSettingsOpen(true); }}
            onClose={() => setSwitcherOpen(false)}
          />
        )}
        {view === "mail" ? (
          <>
            <Sidebar
              messages={messages}
              stream={stream}
              onSelectStream={(s) => {
                setStream(s);
                setSelectedId(null);
                clearSelection();
                clearUndo();
              }}
              folder={folder}
              onSelectFolder={handleSelectFolder}
              labels={labels}
              onCompose={openNewCompose}
            />
            <SplitView
              left={
                folder === "snoozed" ? (
                <SnoozedList rows={snoozedRows} onUnsnooze={handleUnsnooze} />
                ) : (
                <MessageList
                  messages={activeList}
                  stream={stream}
                  selectedId={activeSelectedId}
                  onSelect={handleRowSelect}
                  onActivate={handleSelect}
                  onArchive={handleArchive}
                  onStar={toggleStar}
                  onSnooze={(msg, e) => openSnoozeMenu(msg, e)}
                  selectedIds={selectedIds}
                  onToggleSelect={toggleSelect}
                  onSelectAllVisible={selectAllVisible}
                  onSelectRange={selectRange}
                  onClearSelection={clearSelection}
                  onBatchArchive={batchArchive}
                  onBatchTrash={batchTrash}
                  onBatchMarkRead={batchMarkRead}
                  onBatchStar={batchStar}
                  onBatchLabel={() => setLabelPicker(selectedMsgs)}
                  folder={folder}
                  onBatchRestore={batchRestore}
                  onBatchDeleteForever={batchDeleteForever}
                  labelsById={labelsById}
                  onSearch={handleSearch}
                  onClearSearch={handleClearSearch}
                  searchQuery={searchQuery}
                  searching={searching}
                  onSync={handleSync}
                  busy={busy}
                  flat={inSearch || inFolder}
                  onLoadMore={!inSearch && !inFolder ? handleLoadMore : undefined}
                  canLoadMore={!inSearch && !inFolder && !atEnd}
                  keyboardEnabled={keyboardEnabled}
                  title={
                    inSearch
                      ? "Results"
                      : inFolder
                        ? FOLDERS.find((f) => f.key === folder)?.label ?? labelsById.get(folder)?.name ?? "Label"
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
                  showRecipient={folder === "sent" || folder === "drafts"}
                />
                )
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
                  onReplyAll={handleReplyAll}
                  onForward={handleForward}
                  onSnooze={(msg, e) => openSnoozeMenu(msg, e)}
                  folder={folder}
                  onRestore={handleRestore}
                  onDeleteForever={handleDeleteForever}
                  labelsById={labelsById}
                  onOpenLabels={(m) => setLabelPicker([m])}
                />
              }
            />
          </>
        ) : (
          <CalendarView
            key={`cal-${accountEpoch}`}
            weekStart={weekStart}
            onPrevWeek={() => setWeekStart((w) => addWeeks(w, -1))}
            onToday={() => setWeekStart(startOfWeek(new Date()))}
            onNextWeek={() => setWeekStart((w) => addWeeks(w, 1))}
            rangeLabel={weekRangeLabel(weekStart)}
          />
        )}
      </div>
      {error && <div className="error-bar">{error}</div>}
      {compose && (
        <ComposeModal
          initial={compose}
          onClose={() => setCompose(null)}
          onSent={() => {
            setCompose(null);
            setStatus("Sent ✓");
            if (folder === "drafts") setFolderReloadKey((k) => k + 1);
          }}
          onDraftsChanged={() => {
            if (folder === "drafts") setFolderReloadKey((k) => k + 1);
          }}
        />
      )}
      {settingsOpen && (
        <SettingsModal
          accounts={accounts}
          initial={settings}
          onClose={() => setSettingsOpen(false)}
          onSaved={(s) => {
            setSettings(s);
            setSettingsOpen(false);
          }}
          onRemove={handleRemoveAccount}
          onAdd={() => { setSettingsOpen(false); void handleConnect(); }}
        />
      )}
      {undo && (
        <UndoToast
          verb={undo.verb}
          count={undo.count}
          onUndo={undo.onUndo}
          onDismiss={clearUndo}
        />
      )}
      {labelPicker && (
        <LabelPicker
          labels={labels}
          targets={labelPicker}
          onApply={(labelId, add) => applyLabel(labelPicker, labelId, add)}
          onCreate={(name) => handleCreateLabel(name, labelPicker)}
          onClose={() => setLabelPicker(null)}
        />
      )}
      {snoozeTarget && (
        <SnoozeMenu
          anchor={{ x: snoozeTarget.x, y: snoozeTarget.y }}
          onPick={handleSnoozePick}
          onClose={() => setSnoozeTarget(null)}
        />
      )}
    </div>
  );
}
