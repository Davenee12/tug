import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import { appLabel, threadKey } from "../lib/format";
import { applyZoom, installZoomShortcuts } from "../lib/zoom";
import { ToastLimiter } from "../lib/toastLimiter";
import type {
  Contact,
  DeviceStatus,
  SmsMessage,
  DiscoveredDevice,
  MediaCommand,
  NowPlaying,
  PairingRequest,
  PhoneNotification,
  SearchResults,
  UiSettings,
} from "../types/protocol";

const PAGE = 100;
const SEEN_KEEP = 300;

const EMPTY_STATUS: DeviceStatus = {
  radio: "unknown",
  peripheralSupported: null,
  advertising: "off",
  device: null,
  connection: "noDevice",
  battery: null,
  services: { notifications: false, media: false, battery: false, messages: false },
  lastError: null,
  messagesError: null,
  contactsError: null,
};

const EMPTY_NOW_PLAYING: NowPlaying = {
  player: null,
  state: "unknown",
  rate: null,
  elapsed: null,
  elapsedAt: null,
  volume: null,
  title: null,
  artist: null,
  album: null,
  duration: null,
  available: [],
};

export const useTugStore = defineStore("tug", () => {
  const status = ref<DeviceStatus>(EMPTY_STATUS);
  const nowPlaying = ref<NowPlaying>(EMPTY_NOW_PLAYING);
  /** Newest first. */
  const notifications = ref<PhoneNotification[]>([]);
  /** Messages from message access (MAP), oldest first. */
  const messages = ref<SmsMessage[]>([]);
  const contacts = ref<Contact[]>([]);
  const hasMore = ref(true);
  const discovered = ref<DiscoveredDevice[]>([]);
  const pairingRequest = ref<PairingRequest | null>(null);
  const settings = ref<UiSettings>({ toasts: true, doNotDisturb: false, mutedApps: [] });
  const advertiseEnabled = ref(true);
  const zoom = ref(1);
  const flash = ref<{ kind: "error" | "info"; text: string } | null>(null);
  let flashTimer: number | undefined;

  /** Middle-panel view, and the conversation open in Messages. */
  const view = ref<"feed" | "messages">("feed");
  const selectedThread = ref<string | null>(null);
  /** A new conversation being started from the + button, before any message exists. */
  const composeTo = ref<{ address: string; name: string } | null>(null);
  /** The New message picker (+ or Ctrl+N). */
  const pickerOpen = ref(false);
  /** Universal search (Ctrl+K or the search box). */
  const searchOpen = ref(false);
  /** The connection/settings panel sliding over the window (narrow layouts). */
  const panelOpen = ref(false);
  /** Something is covering the main view, so whatever is behind it isn't being looked at. */
  const overlayOpen = computed(() => searchOpen.value || pickerOpen.value || panelOpen.value || !!pairingRequest.value);
  /** An item to scroll to and highlight after navigating from search: `m<id>` or `n<id>`. */
  const focusItem = ref<string | null>(null);

  /** When each feed entry (conversation or app stack) was last looked at. */
  const seen = ref<Record<string, number>>({});
  /** History from before tug started tracking "seen" never counts as new. */
  const seenSince = ref(Date.now());

  const connected = computed(() => status.value.connection === "connected");

  function notify(kind: "error" | "info", text: string) {
    flash.value = { kind, text };
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => (flash.value = null), kind === "info" ? 2500 : 5000);
  }

  async function attempt<T>(fn: () => Promise<T>): Promise<T | undefined> {
    try {
      return await fn();
    } catch (e) {
      notify("error", errorMessage(e));
      return undefined;
    }
  }

  function upsert(list: PhoneNotification[], n: PhoneNotification): boolean {
    const i = list.findIndex((x) => x.id === n.id);
    if (i >= 0) {
      list[i] = n;
      return false;
    }
    // Ids are monotonic, so newest-first order is id-descending.
    const at = list.findIndex((x) => x.id < n.id);
    list.splice(at < 0 ? list.length : at, 0, n);
    return true;
  }

  function upsertMessage(m: SmsMessage) {
    const i = messages.value.findIndex((x) => x.id === m.id);
    if (i >= 0) messages.value[i] = m;
    else messages.value.push(m);
  }

  // Windows notification permission: ask once per launch, not on every notification.
  let toastPermission: boolean | null = null;
  const toasts = new ToastLimiter();
  let toastSummary: number | undefined;
  async function maybeToast(n: PhoneNotification) {
    const s = settings.value;
    if (!s.toasts || s.doNotDisturb || n.flags.silent || n.flags.preExisting) return;
    if (s.mutedApps.includes(n.appId)) return;
    if (toastPermission === null) {
      toastPermission = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    }
    if (!toastPermission) return;
    // Calls always ring through; everything else is rate-limited and summed up.
    if (n.category !== "incomingCall" && !toasts.admit(Date.now())) {
      if (toastSummary === undefined) {
        toastSummary = window.setTimeout(() => {
          toastSummary = undefined;
          const held = toasts.takeHeld();
          if (held > 0) sendNotification({ title: "tug", body: `${held} more notification${held === 1 ? "" : "s"}` });
        }, toasts.windowMs);
      }
      return;
    }
    const title = [appLabel(n), n.title].filter(Boolean).join(" · ");
    sendNotification({ title, body: [n.subtitle, n.message].filter(Boolean).join("\n") });
  }

  function isNew(key: string, n: PhoneNotification): boolean {
    // Cleared on the phone or watch means it was already read there.
    return n.removedAt == null && n.receivedAt > (seen.value[key] ?? seenSince.value);
  }

  function newCount(key: string, items: PhoneNotification[]): number {
    return items.reduce((c, n) => c + (isNew(key, n) ? 1 : 0), 0);
  }

  function markSeen(key: string) {
    // Keep the most recently opened conversations only, so the saved setting can't grow forever.
    const kept = Object.entries(seen.value)
      .filter(([k]) => k !== key)
      .sort((a, b) => b[1] - a[1])
      .slice(0, SEEN_KEEP - 1);
    seen.value = { ...Object.fromEntries(kept), [key]: Date.now() };
    void attempt(() => api.setSetting("ui.seen", JSON.stringify(seen.value)));
  }

  /** Clear every notification in a row that's still on the phone and clearable. */
  async function clearItems(items: PhoneNotification[], { quiet = false } = {}) {
    const clearable = items.filter((n) => n.live && n.removedAt == null && n.flags.negativeAction);
    for (const n of clearable) {
      try {
        await api.performAction(n.id, false);
      } catch (e) {
        // Automatic clears (opening a conversation) shouldn't nag; the ✕ button does.
        if (!quiet) notify("error", errorMessage(e));
        return;
      }
    }
  }

  /**
   * A conversation is open in front of the user: what's in it has been read. Clear its
   * notifications on the phone (which also takes them off the Feed) and mark its texts
   * read over message access. Both only touch what's still unread, so repeats are free.
   */
  function readConversation(notifications: PhoneNotification[], messageIds: number[]) {
    void clearItems(notifications, { quiet: true });
    if (messageIds.length) api.markRead(messageIds).catch(() => undefined);
  }

  /** Display name for a bundle id, from the history tug has seen. */
  function appNameFor(appId: string): string {
    const n = notifications.value.find((x) => x.appId === appId);
    return n ? appLabel(n) : appLabel({ appId, appName: null });
  }

  function startConversation(address: string, name: string) {
    composeTo.value = { address, name };
    selectedThread.value = threadKey({ appId: "com.apple.MobileSMS", title: name });
    view.value = "messages";
  }

  function openThread(key: string) {
    composeTo.value = null;
    selectedThread.value = key;
    view.value = "messages";
    markSeen(key);
  }

  async function loadSettings() {
    const raw = await api.getSettings();
    zoom.value = Number(raw["ui.zoom"]) || 1;
    applyZoom(zoom.value);
    seen.value = raw["ui.seen"] ? (JSON.parse(raw["ui.seen"]) as Record<string, number>) : {};
    if (raw["ui.seenSince"]) {
      seenSince.value = Number(raw["ui.seenSince"]);
    } else {
      await api.setSetting("ui.seenSince", String(seenSince.value));
    }
    advertiseEnabled.value = raw.advertise !== "false";
    settings.value = {
      toasts: raw["ui.toasts"] !== "false",
      doNotDisturb: raw["ui.doNotDisturb"] === "true",
      mutedApps: raw["ui.mutedApps"] ? (JSON.parse(raw["ui.mutedApps"]) as string[]) : [],
    };
  }

  // Listeners and shortcuts are installed once and torn down by dispose(), so a
  // remount (or dev hot-reload) can't double every toast and keypress.
  let teardown: Array<UnlistenFn | (() => void)> = [];
  let started = false;

  async function init() {
    if (started) return;
    started = true;
    teardown.push(
      installZoomShortcuts(
        () => zoom.value,
        (z) => {
          zoom.value = z;
          notify("info", `Zoom ${Math.round(z * 100)}%`);
          void attempt(() => api.setSetting("ui.zoom", String(z)));
        },
      ),
    );
    teardown.push(
      ...(await Promise.all([
        on("device-status", (s) => {
          const wasConnected = status.value.connection === "connected";
          status.value = s;
          // Notification UIDs die with the connection, so nothing stays actionable.
          if (wasConnected && s.connection !== "connected") {
            for (const n of notifications.value) n.live = false;
          }
        }),
        on("now-playing", (np) => {
          nowPlaying.value = np;
        }),
        on("notification", (n) => {
          const added = upsert(notifications.value, n);
          if (view.value === "messages" && selectedThread.value === threadKey(n)) markSeen(threadKey(n));
          if (added) void maybeToast(n);
        }),
        on("notification-removed", (id) => {
          const n = notifications.value.find((x) => x.id === id);
          if (n) Object.assign(n, { removedAt: Date.now(), live: false });
        }),
        on("app-name", ({ appId, appName }) => {
          for (const n of notifications.value) if (n.appId === appId) n.appName = appName;
        }),
        on("discovered-devices", (list) => (discovered.value = list)),
        on("message", upsertMessage),
        on("contacts", (list) => {
          contacts.value = list;
          // Names are joined into messages server-side; apply them to what's loaded.
          const byAddress = new Map(list.map((c) => [c.address, c.name]));
          for (const m of messages.value) m.contactName = byAddress.get(m.address) ?? m.contactName;
          // Notifications under a contact's old name come back under the new one.
          void refreshLoadedNotifications();
        }),
        on("pairing-request", (req) => (pairingRequest.value = req)),
        on("pairing-request-closed", () => (pairingRequest.value = null)),
      ])),
    );
    const [s, np, first, msgs, people] = await Promise.all([
      api.getStatus(),
      api.getNowPlaying(),
      api.listNotifications(PAGE),
      api.listMessages(2000),
      api.getContacts(),
    ]);
    // Merge rather than replace: events may have arrived while these loaded.
    for (const n of first) upsert(notifications.value, n);
    const live = new Set(messages.value.map((m) => m.id));
    messages.value = [...msgs.filter((m) => !live.has(m.id)), ...messages.value].sort(
      (a, b) => a.receivedAt - b.receivedAt || a.id - b.id,
    );
    if (contacts.value.length === 0) contacts.value = people;
    status.value = s;
    nowPlaying.value = np;
    hasMore.value = first.length === PAGE;
    await attempt(loadSettings);
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    window.clearTimeout(toastSummary);
    toastSummary = undefined;
    started = false;
  }

  async function loadMore() {
    if (!hasMore.value) return;
    const last = notifications.value.at(-1);
    const page = await attempt(() => api.listNotifications(PAGE, last?.id));
    if (!page) return;
    for (const n of page) upsert(notifications.value, n);
    hasMore.value = page.length === PAGE;
  }

  /** Re-read what's loaded (names are resolved server-side, e.g. after a contact is renamed). */
  async function refreshLoadedNotifications() {
    const count = Math.min(Math.max(notifications.value.length, PAGE), 500);
    const fresh = await api.listNotifications(count).catch(() => null);
    if (fresh) for (const n of fresh) upsert(notifications.value, n);
  }

  /** Universal search: people, texts and notifications. */
  async function searchAll(query: string): Promise<SearchResults | undefined> {
    return attempt(() => api.searchAll(query, 30));
  }

  async function setSetting<K extends keyof UiSettings>(key: K, value: UiSettings[K]) {
    settings.value = { ...settings.value, [key]: value };
    await attempt(() => api.setSetting(`ui.${key}`, typeof value === "string" ? value : JSON.stringify(value)));
  }

  function toggleMuted(appId: string) {
    const muted = settings.value.mutedApps;
    void setSetting("mutedApps", muted.includes(appId) ? muted.filter((a) => a !== appId) : [...muted, appId]);
  }

  return {
    status,
    nowPlaying,
    notifications,
    messages,
    contacts,
    hasMore,
    discovered,
    pairingRequest,
    settings,
    advertiseEnabled,
    flash,
    view,
    selectedThread,
    composeTo,
    pickerOpen,
    searchOpen,
    panelOpen,
    overlayOpen,
    focusItem,
    seen,
    connected,
    init,
    dispose,
    loadMore,
    searchAll,
    setSetting,
    toggleMuted,
    notify,
    isNew,
    newCount,
    markSeen,
    openThread,
    startConversation,
    clearItems,
    readConversation,
    appNameFor,
    /** Send through the iPhone. The pending message appears via the `message` event. */
    async sendMessage(address: string, text: string): Promise<boolean> {
      try {
        await api.sendMessage(address, text);
        return true;
      } catch (e) {
        notify("error", errorMessage(e));
        return false;
      }
    },
    performAction: (id: number, positive: boolean) => attempt(() => api.performAction(id, positive)),
    media: (command: MediaCommand) => attempt(() => api.mediaCommand(command)),
    startDiscovery: () => attempt(api.startDiscovery),
    stopDiscovery: () => {
      discovered.value = [];
      return attempt(api.stopDiscovery);
    },
    async pair(id: string) {
      const ok = await attempt(() => api.pairDevice(id).then(() => true));
      if (ok) notify("info", "Paired. Connecting to your iPhone…");
      return ok === true;
    },
    async useDevice(id: string) {
      const ok = await attempt(() => api.useDevice(id).then(() => true));
      return ok === true;
    },
    forget: () => attempt(api.forgetDevice),
    async setAdvertising(enabled: boolean) {
      advertiseEnabled.value = enabled;
      await attempt(() => api.setAdvertising(enabled));
    },
    async confirmPairing(accept: boolean) {
      pairingRequest.value = null;
      await attempt(() => api.confirmPairing(accept));
    },
    async clearHistory() {
      await attempt(api.clearHistory);
      notifications.value = [];
      hasMore.value = false;
    },
  };
});
