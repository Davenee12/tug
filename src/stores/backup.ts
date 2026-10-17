// Bring in your full message history from an iPhone backup: the wizard's state, what's in tug
// from a backup, group chats, whole-conversation history and photo previews. The work happens in
// Rust (`backup/`); this follows its `backup-import` status and keeps the window's lists in step.

import { defineStore } from "pinia";
import { ref } from "vue";
import { api, errorMessage, on } from "../lib/ipc";
import { BACKUP_SOURCE } from "../lib/backupImport";
import { byArrival, missingMessages } from "../lib/messageSync";
import { useTugStore } from "./tug";
import type { AttachmentPreview, BackupImported, BackupInfo, BackupStatus, MessageGroup, SmsMessage } from "../types/protocol";

/** How many conversations' newest texts the window lists beyond its recent page. */
const HEADS = 2000;

const IDLE: BackupStatus = {
  rev: 0,
  phase: "idle",
  backup: null,
  progress: null,
  stage: null,
  done: 0,
  total: 0,
  scan: null,
  summary: null,
  problem: null,
};

export const useBackupStore = defineStore("backup", () => {
  const tug = useTugStore();

  /** Settings › Messages › Bring in your full history… is open. */
  const wizardOpen = ref(false);
  const status = ref<BackupStatus>({ ...IDLE });
  const backups = ref<BackupInfo[] | null>(null);
  const listing = ref(false);
  const listError = ref<string | null>(null);
  /** The backup chosen in the picker (it may need a password next). */
  const chosen = ref<BackupInfo | null>(null);
  const imported = ref<BackupImported | null>(null);
  const groups = ref<MessageGroup[]>([]);
  /** A HEIC photo couldn't be shown (no HEIF Image Extensions); the hint says why, once. */
  const needsHeif = ref(false);
  const heifHintDismissed = ref(true);

  function applyStatus(s: BackupStatus) {
    if (s.rev < status.value.rev && s.phase === status.value.phase) return;
    const before = status.value.phase;
    status.value = s;
    // An import that ended (done, stopped or failed partway) changed the history: catch the
    // window up, once.
    if (before === "importing" && s.phase !== "importing") void refreshAfterImport();
  }

  let listening: Promise<unknown> | null = null;
  function listen() {
    listening ??= on("backup-import", applyStatus).catch(() => undefined);
  }

  let loaded: Promise<void> | null = null;
  /** Group chats, every conversation's newest text, and what's imported: once, when Messages opens. */
  function ensureLoaded(): Promise<void> {
    listen();
    loaded ??= (async () => {
      const [g, heads, i, settings] = await Promise.all([
        api.listMessageGroups().catch(() => [] as MessageGroup[]),
        api.conversationHeads(HEADS).catch(() => [] as SmsMessage[]),
        api.backupImported().catch(() => null),
        api.getSettings().catch(() => ({}) as Record<string, string>),
      ]);
      groups.value = g;
      imported.value = i;
      heifHintDismissed.value = settings["ui.heifHintDismissed"] === "true";
      mergeMessages(heads);
    })();
    return loaded;
  }

  function mergeMessages(list: SmsMessage[]) {
    const add = missingMessages(tug.messages, list);
    if (add.length) tug.messages = [...tug.messages, ...add].sort(byArrival);
  }

  const historyLoaded = new Set<string>();
  /** A conversation's whole history, when it's opened (the window keeps only recent texts). */
  async function loadHistory(c: { key: string; addresses: string[]; group?: string }) {
    if (historyLoaded.has(c.key) || (!c.group && !c.addresses.length)) return;
    historyLoaded.add(c.key);
    try {
      mergeMessages(await api.conversationMessages(c.addresses, c.group ?? null));
    } catch {
      historyLoaded.delete(c.key);
    }
  }

  async function refreshAfterImport() {
    historyLoaded.clear();
    const [recent, heads, g, i] = await Promise.all([
      api.listMessages(2000).catch(() => [] as SmsMessage[]),
      api.conversationHeads(HEADS).catch(() => [] as SmsMessage[]),
      api.listMessageGroups().catch(() => groups.value),
      api.backupImported().catch(() => imported.value),
    ]);
    groups.value = g;
    imported.value = i;
    // A Bluetooth text the import matched now carries its photos or its group: take the new copy.
    const fresh = new Map([...recent, ...heads].map((m) => [m.id, m]));
    tug.messages = tug.messages.map((m) => fresh.get(m.id) ?? m);
    mergeMessages([...recent, ...heads]);
  }

  async function openWizard() {
    listen();
    wizardOpen.value = true;
    chosen.value = null;
    const s = await api.backupStatus().catch(() => null);
    if (s) applyStatus(s);
    // A finished or failed run from before starts over; one still running is shown as it is.
    if (s && ["done", "cancelled", "failed", "ready"].includes(s.phase)) await api.backupCancel().catch(() => undefined);
    if (s && s.phase !== "unlocking" && s.phase !== "reading" && s.phase !== "importing") {
      status.value = { ...IDLE, rev: s.rev };
    }
    await refreshList();
  }

  async function refreshList() {
    listing.value = true;
    listError.value = null;
    try {
      backups.value = await api.backupList();
    } catch (e) {
      backups.value = [];
      listError.value = errorMessage(e);
    } finally {
      listing.value = false;
    }
  }

  async function pickFolder() {
    listError.value = null;
    try {
      const b = await api.backupPickFolder();
      if (!b) return;
      if (!backups.value?.some((x) => x.id === b.id)) backups.value = [b, ...(backups.value ?? [])];
      await choose(b);
    } catch (e) {
      listError.value = errorMessage(e);
    }
  }

  /** Choose a backup: an encrypted one asks for its password first. */
  async function choose(b: BackupInfo) {
    chosen.value = b;
    if (b.encrypted) return;
    await open(null);
  }

  async function open(password: string | null) {
    const b = chosen.value;
    if (!b) return;
    try {
      await api.backupOpen(b.id, password);
    } catch (e) {
      listError.value = errorMessage(e);
    }
  }

  async function start(texts: boolean, photos: boolean) {
    try {
      await api.backupImport(texts, photos);
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  /** Cancel what's running, or close a backup that was read. */
  async function cancel() {
    await api.backupCancel().catch(() => undefined);
  }

  /** Close the wizard. Anything still reading is cancelled; a running import is too. */
  async function closeWizard() {
    wizardOpen.value = false;
    chosen.value = null;
    if (status.value.phase !== "idle") await cancel();
  }

  /** Settings › Messages › Remove imported history. */
  async function removeImported(): Promise<boolean> {
    try {
      await api.backupRemoveImported();
    } catch (e) {
      tug.notify("error", errorMessage(e));
      return false;
    }
    tug.messages = tug.messages
      .filter((m) => m.source !== BACKUP_SOURCE)
      .map((m) => (m.attachments?.length || m.groupId ? { ...m, attachments: [], groupId: null } : m));
    groups.value = [];
    imported.value = { texts: 0, photos: 0, backupDate: null };
    historyLoaded.clear();
    previews.clear();
    tug.notify("info", "Removed the history from your iPhone backup.");
    return true;
  }

  async function refreshImported() {
    imported.value = await api.backupImported().catch(() => imported.value);
  }

  const previews = new Map<number, Promise<AttachmentPreview>>();
  /** A stored photo's preview (made once, then cached for the session). */
  function preview(id: number): Promise<AttachmentPreview> {
    let p = previews.get(id);
    if (!p) {
      p = api.attachmentPreview(id).catch(() => ({ dataUri: null, needsHeif: false }));
      previews.set(id, p);
      void p.then((r) => {
        if (r.needsHeif) needsHeif.value = true;
      });
    }
    return p;
  }

  async function openAttachment(id: number) {
    try {
      await api.openAttachment(id);
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  function dismissHeifHint() {
    heifHintDismissed.value = true;
    void api.setSetting("ui.heifHintDismissed", "true").catch(() => undefined);
  }

  return {
    wizardOpen,
    status,
    backups,
    listing,
    listError,
    chosen,
    imported,
    groups,
    needsHeif,
    heifHintDismissed,
    ensureLoaded,
    loadHistory,
    openWizard,
    refreshList,
    pickFolder,
    choose,
    open,
    start,
    cancel,
    closeWizard,
    removeImported,
    refreshImported,
    preview,
    openAttachment,
    dismissHeifHint,
  };
});
