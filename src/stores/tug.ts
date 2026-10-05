import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { api, errorMessage, on } from "../lib/ipc";
import { appLabel, threadKey } from "../lib/format";
import { applyZoom, installZoomShortcuts } from "../lib/zoom";
import type {
  Contact,
  DeviceStatus,
  SmsMessage,
  DiscoveredDevice,
  MediaCommand,
  NowPlaying,
  PairingRequest,
  PhoneNotification,
  UiSettings,
} from "../types/protocol";

const PAGE = 100;

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
  /** When the last now-playing update arrived, to advance the progress bar locally. */
  const nowPlayingAt = ref(Date.now());
  const notifications = ref<PhoneNotification[]>([]);
  /** Messages from message access (MAP), oldest first. */
  const messages = ref<SmsMessage[]>([]);
  const contacts = ref<Contact[]>([]);
  const hasMore = ref(true);
  const searchQuery = ref("");
  const searchResults = ref<PhoneNotification[] | null>(null);
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

  /** When each feed entry (conversation or app stack) was last looked at. */
  const seen = ref<Record<string, number>>({});
  /** History from before tug started tracking "seen" never counts as new. */
  const seenSince = ref(Date.now());

  const connected = computed(() => status.value.connection === "connected");
  const visible = computed(() => searchResults.value ?? notifications.value);

  function notify(kind: "error" | "info", text: string) {
    flash.value = { kind, text };
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => (flash.value = null), 5000);
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

  function patch(id: number, change: Partial<PhoneNotification>) {
    for (const list of [notifications.value, searchResults.value ?? []]) {
      const n = list.find((x) => x.id === id);
      if (n) Object.assign(n, change);
    }
  }

  async function maybeToast(n: PhoneNotification) {
    const s = settings.value;
    if (!s.toasts || s.doNotDisturb || n.flags.silent || n.flags.preExisting) return;
    if (s.mutedApps.includes(n.appId)) return;
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (!granted) return;
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
    seen.value = { ...seen.value, [key]: Date.now() };
    void attempt(() => api.setSetting("ui.seen", JSON.stringify(seen.value)));
  }

  /** Clear every notification in a row that's still on the phone and clearable. */
  async function clearItems(items: PhoneNotification[]) {
    const clearable = items.filter((n) => n.live && n.removedAt == null && n.flags.negativeAction);
    for (const n of clearable) {
      try {
        await api.performAction(n.id, false);
      } catch (e) {
        notify("error", errorMessage(e));
        return;
      }
    }
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

  async function init() {
    installZoomShortcuts(
      () => zoom.value,
      (z) => {
        zoom.value = z;
        notify("info", `Zoom ${Math.round(z * 100)}%`);
        void attempt(() => api.setSetting("ui.zoom", String(z)));
      },
    );
    await Promise.all([
      on("device-status", (s) => {
        const wasConnected = status.value.connection === "connected";
        status.value = s;
        // Notification UIDs die with the connection, so nothing stays actionable.
        if (wasConnected && s.connection !== "connected") {
          for (const n of notifications.value) n.live = false;
          for (const n of searchResults.value ?? []) n.live = false;
        }
      }),
      on("now-playing", (np) => {
        nowPlaying.value = np;
        nowPlayingAt.value = Date.now();
      }),
      on("notification", (n) => {
        const added = upsert(notifications.value, n);
        if (searchResults.value) upsert(searchResults.value, n);
        if (view.value === "messages" && selectedThread.value === threadKey(n)) markSeen(threadKey(n));
        if (added) void maybeToast(n);
      }),
      on("notification-removed", (id) => patch(id, { removedAt: Date.now(), live: false })),
      on("app-name", ({ appId, appName }) => {
        for (const list of [notifications.value, searchResults.value ?? []]) {
          for (const n of list) if (n.appId === appId) n.appName = appName;
        }
      }),
      on("discovered-devices", (list) => (discovered.value = list)),
      on("message", (m) => {
        const i = messages.value.findIndex((x) => x.id === m.id);
        if (i >= 0) messages.value[i] = m;
        else messages.value.push(m);
      }),
      on("contacts", (list) => {
        contacts.value = list;
        // Names are joined into messages server-side; apply them to what's loaded.
        const byAddress = new Map(list.map((c) => [c.address, c.name]));
        for (const m of messages.value) m.contactName = byAddress.get(m.address) ?? m.contactName;
      }),
      on("pairing-request", (req) => (pairingRequest.value = req)),
      on("pairing-request-closed", () => (pairingRequest.value = null)),
    ]);
    const [s, np, first, msgs, people] = await Promise.all([
      api.getStatus(),
      api.getNowPlaying(),
      api.listNotifications(PAGE),
      api.listMessages(2000),
      api.getContacts(),
    ]);
    messages.value = msgs;
    contacts.value = people;
    status.value = s;
    nowPlaying.value = np;
    notifications.value = first;
    hasMore.value = first.length === PAGE;
    await attempt(loadSettings);
  }

  async function loadMore() {
    if (!hasMore.value || searchResults.value) return;
    const last = notifications.value.at(-1);
    const page = await attempt(() => api.listNotifications(PAGE, last?.id));
    if (!page) return;
    for (const n of page) upsert(notifications.value, n);
    hasMore.value = page.length === PAGE;
  }

  let searchSeq = 0;
  async function search(q: string) {
    searchQuery.value = q;
    const seq = ++searchSeq;
    if (!q.trim()) {
      searchResults.value = null;
      return;
    }
    const results = await attempt(() => api.searchNotifications(q, 300));
    if (results && seq === searchSeq) searchResults.value = results;
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
    nowPlayingAt,
    notifications,
    messages,
    contacts,
    hasMore,
    searchQuery,
    searchResults,
    discovered,
    pairingRequest,
    settings,
    advertiseEnabled,
    flash,
    view,
    selectedThread,
    composeTo,
    pickerOpen,
    seen,
    connected,
    visible,
    init,
    loadMore,
    search,
    setSetting,
    toggleMuted,
    notify,
    isNew,
    newCount,
    markSeen,
    openThread,
    startConversation,
    clearItems,
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
      searchResults.value = searchResults.value ? [] : null;
      hasMore.value = false;
    },
  };
});
