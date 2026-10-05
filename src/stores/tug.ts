import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import { appLabel, canClear, cleanName, formatAddress, groupConversations, groupThreads, missedCallFor, newestUnreadThread, threadKey } from "../lib/format";
import { applyZoom, installZoomShortcuts } from "../lib/zoom";
import { ToastLimiter } from "../lib/toastLimiter";
import { findCode } from "../lib/codes";
import { copyText } from "../lib/clipboard";
import type {
  CallRecord,
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

export type SettingsSection = "general" | "iphone" | "notifications" | "weather" | "privacy" | "about";
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
  pairingStale: false,
  messagesError: null,
  contactsError: null,
  textsPairing: "unknown",
  textsDevice: null,
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
  repeat: null,
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
  /** The phone's recent calls (PBAP call history), newest first. */
  const calls = ref<CallRecord[]>([]);
  const hasMore = ref(true);
  const discovered = ref<DiscoveredDevice[]>([]);
  const pairingRequest = ref<PairingRequest | null>(null);
  const settings = ref<UiSettings>({ toasts: true, doNotDisturb: false, mutedApps: [], closeToTray: true, appIcons: true, dialing: false });

  /** App icons as data URIs by app id; null = the App Store has none (initials instead). */
  const appIcons = ref<Record<string, string | null>>({});
  const iconRequests = new Set<string>();
  /** The app's real icon, asked for the first time it's needed (then cached on disk by the backend). */
  function iconFor(appId: string): string | null {
    if (!settings.value.appIcons) return null;
    if (!(appId in appIcons.value) && !iconRequests.has(appId)) {
      iconRequests.add(appId);
      api
        .appIcon(appId)
        .then((uri) => (appIcons.value = { ...appIcons.value, [appId]: uri }))
        // Offline or the lookup failed: initials for now, asked again next launch.
        .catch(() => undefined);
    }
    return appIcons.value[appId] ?? null;
  }
  const advertiseEnabled = ref(true);
  const zoom = ref(1);
  /** First-run setup finished (or skipped). Until then, with no iPhone paired, setup shows. */
  const onboarded = ref(true);
  /** Setup opened on purpose (Settings › iPhone › Run setup again). */
  const setupRequested = ref(false);
  /**
   * First run: nothing paired and setup never finished. Latched once it starts, because
   * pairing mid-setup gives us a device, and setup still has steps to go after that.
   */
  const setupActive = ref(false);
  watch([onboarded, () => status.value.device], ([done, device]) => {
    if (!done && device == null) setupActive.value = true;
  });
  const showSetup = computed(() => setupRequested.value || setupActive.value);
  const flash = ref<{ kind: "error" | "info"; text: string; action?: { label: string; run: () => void } } | null>(null);
  let flashTimer: number | undefined;

  /** Middle-panel view, and the conversation open in Messages. */
  const view = ref<"feed" | "messages" | "calls" | "settings">("feed");
  const settingsSection = ref<SettingsSection>("general");
  /** Where Settings returns to. */
  let viewBeforeSettings: "feed" | "messages" | "calls" = "feed";
  const selectedThread = ref<string | null>(null);
  /** A new conversation being started from the + button, before any message exists. */
  const composeTo = ref<{ address: string; name: string } | null>(null);
  /** The New message picker (+ or Ctrl+N). */
  const pickerOpen = ref(false);
  /** Universal search (Ctrl+K or the search box). */
  const searchOpen = ref(false);
  /** Ringing calls whose card was answered or hidden here; their Feed row stays until the phone drops them. */
  const callsHandled = ref<number[]>([]);
  /** The call ringing on the phone right now (newest first), for the incoming-call card. */
  const ringing = computed(
    () =>
      notifications.value.find(
        (n) => n.category === "incomingCall" && n.live && n.removedAt == null && !callsHandled.value.includes(n.id),
      ) ?? null,
  );
  /** Something is covering the main view, so whatever is behind it isn't being looked at. */
  const overlayOpen = computed(() => searchOpen.value || pickerOpen.value || !!pairingRequest.value || !!ringing.value);

  function openSettings(section?: SettingsSection) {
    if (view.value !== "settings") viewBeforeSettings = view.value;
    if (section) settingsSection.value = section;
    view.value = "settings";
  }

  function closeSettings() {
    if (view.value === "settings") view.value = viewBeforeSettings;
  }
  /**
   * The iPhone's switches are on screen (Settings, or setup's sharing step) and tug is visible:
   * the app checks them every couple of seconds so flipping one on the phone shows up at once.
   */
  const setupSharingShown = ref(false);
  const pageVisible = ref(document.visibilityState === "visible");
  document.addEventListener("visibilitychange", () => (pageVisible.value = document.visibilityState === "visible"));
  // The switches get flipped on the phone, with tug on any screen or in the tray. So for the
  // first minutes after launch or pairing, check fast whenever one is still off, too.
  const FRESH_MS = 5 * 60 * 1000;
  const fresh = ref(true);
  let freshTimer = window.setTimeout(() => (fresh.value = false), FRESH_MS);
  watch(
    () => status.value.device?.id,
    (id) => {
      if (!id) return;
      fresh.value = true;
      window.clearTimeout(freshTimer);
      freshTimer = window.setTimeout(() => (fresh.value = false), FRESH_MS);
    },
  );
  const switchesPending = computed(() => {
    const s = status.value;
    return !!s.device && (!s.services.notifications || !s.services.messages || contacts.value.length === 0);
  });
  watch(
    () =>
      (pageVisible.value && (view.value === "settings" || setupSharingShown.value)) ||
      (fresh.value && switchesPending.value),
    (on) => void api.setWatching(on).catch(() => undefined),
    { immediate: true },
  );

  /** An item to scroll to and highlight after navigating from search: `m<id>` or `n<id>`. */
  const focusItem = ref<string | null>(null);

  /** When each feed entry (conversation or app stack) was last looked at. */
  const seen = ref<Record<string, number>>({});
  /** History from before tug started tracking "seen" never counts as new. */
  const seenSince = ref(Date.now());

  const connected = computed(() => status.value.connection === "connected");

  /** A short message at the bottom; with an action (e.g. Undo) it stays a little longer. */
  function notify(kind: "error" | "info", text: string, action?: { label: string; run: () => void }) {
    flash.value = { kind, text, action };
    window.clearTimeout(flashTimer);
    const ms = action ? 8000 : kind === "info" ? 2500 : 5000;
    flashTimer = window.setTimeout(() => (flash.value = null), ms);
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

  /** New texts across conversations: the Messages tab badge, the tray and the taskbar dot. */
  const unreadTexts = computed(() =>
    groupThreads(notifications.value).reduce((sum, t) => sum + newCount(t.key, t.items), 0),
  );
  watch(unreadTexts, (n) => void api.setUnread(n).catch(() => undefined), { immediate: true });

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
    const clearable = items.filter(canClear);
    let failure: unknown = null;
    // One that's already gone from the phone mustn't stop the rest from clearing.
    for (const n of clearable) {
      try {
        await api.performAction(n.id, false);
      } catch (e) {
        failure ??= e;
      }
    }
    // Automatic clears (opening a conversation) shouldn't nag; the ✕ button does, once.
    if (failure && !quiet) notify("error", errorMessage(failure));
  }

  /**
   * Delete a conversation from tug: its notifications and texts disappear here (Feed,
   * Messages, search) but nothing changes on the phone. Undo is offered for a few seconds;
   * a new text from them starts the conversation again.
   */
  async function deleteConversation(c: { key: string; contact: string; addresses: string[]; notifications: PhoneNotification[] }) {
    const gone = new Set(c.notifications.map((n) => n.id));
    const removedNotifications = notifications.value.filter((n) => gone.has(n.id));
    const removedMessages = messages.value.filter((m) => c.addresses.includes(m.address));
    const nIds = removedNotifications.map((n) => n.id);
    const mIds = removedMessages.map((m) => m.id);
    try {
      await api.setHidden(nIds, mIds, true);
    } catch (e) {
      notify("error", errorMessage(e));
      return;
    }
    const hiddenMessages = new Set(mIds);
    notifications.value = notifications.value.filter((n) => !gone.has(n.id));
    messages.value = messages.value.filter((m) => !hiddenMessages.has(m.id));
    if (selectedThread.value === c.key) selectedThread.value = null;
    notify("info", `Deleted ${c.contact}`, {
      label: "Undo",
      run: () => {
        flash.value = null;
        void attempt(async () => {
          await api.setHidden(nIds, mIds, false);
          for (const n of removedNotifications) upsert(notifications.value, n);
          for (const m of removedMessages) upsertMessage(m);
          messages.value.sort((a, b) => a.receivedAt - b.receivedAt || a.id - b.id);
          selectedThread.value = c.key;
        });
      },
    });
  }

  /** Copy a one-time code; once it's used, its notification has done its job on the phone too. */
  async function copyCode(code: string, from: PhoneNotification[] = []): Promise<boolean> {
    if (!(await copyText(code))) {
      notify("error", "Couldn't copy to the clipboard.");
      return false;
    }
    notify("info", `Copied ${code}`);
    void clearItems(from, { quiet: true });
    return true;
  }

  /** The newest code that arrived in the last few minutes, from a notification or a text. */
  function latestCode(maxAgeMs = 10 * 60 * 1000): { code: string; from: PhoneNotification[] } | null {
    const since = Date.now() - maxAgeMs;
    type Hit = { code: string; at: number; from: PhoneNotification[] };
    let best: Hit | null = null;
    for (const n of notifications.value) {
      if (n.receivedAt < since) break; // newest first
      const found = findCode(n.message || n.subtitle);
      if (found && (!best || n.receivedAt > best.at)) best = { code: found.code, at: n.receivedAt, from: [n] };
    }
    for (const m of messages.value) {
      if (m.direction !== "in" || m.receivedAt < since) continue;
      const found = findCode(m.body);
      if (found && (!best || m.receivedAt > best.at)) best = { code: found.code, at: m.receivedAt, from: [] };
    }
    return best && { code: best.code, from: best.from };
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

  /**
   * Mark everything read at once: every conversation counts as seen in tug, and its texts
   * are marked read and its notifications cleared on the phone — exactly as opening each would.
   */
  function markAllRead() {
    for (const c of groupConversations(notifications.value, messages.value, contacts.value)) {
      markSeen(c.key);
      const ids = messages.value.filter((m) => m.direction === "in" && c.addresses.includes(m.address)).map((m) => m.id);
      readConversation(c.notifications, ids);
    }
  }

  /** Take the ringing card down here; the phone keeps ringing (and the Feed row stays). */
  function hideCall(id: number) {
    if (!callsHandled.value.includes(id)) callsHandled.value = [...callsHandled.value.slice(-20), id];
  }

  /**
   * Answer or decline on the phone (ANCS positive/negative action). The card goes once the
   * phone takes it; answering doesn't always remove the notification right away, so it's
   * taken down here too. The call's audio stays on the iPhone.
   */
  async function respondToCall(n: PhoneNotification, answer: boolean): Promise<boolean> {
    const ok = await attempt(() => api.performAction(n.id, answer).then(() => true));
    if (!ok) return false;
    hideCall(n.id);
    if (answer) notify("info", "Answered. Talk on your iPhone.");
    return true;
  }

  /** The number being dialed, while the iPhone sets the call up. */
  const calling = ref<string | null>(null);
  /**
   * Calling from tug can work here: the experimental hands-free check in Settings › iPhone
   * passed on this PC. Anything that offers a Call action (rows, Ctrl+K) shows it only then.
   */
  const canDial = computed(() => settings.value.dialing);

  /**
   * Experimental: place a call on the iPhone over its hands-free link; you talk on the phone.
   * `name` defaults to the contact's name for that number. Refuses (with a message) unless
   * `canDial`, and a failure always says why: it never fails silently.
   */
  async function call(number: string, name?: string): Promise<boolean> {
    name ??= contacts.value.find((c) => c.address === number)?.name ?? formatAddress(number);
    if (!canDial.value) {
      notify("error", "Calling from tug is off. Check it under Settings › iPhone › Calls.");
      return false;
    }
    if (calling.value) return false;
    calling.value = number;
    notify("info", `Calling ${name} on your iPhone…`);
    try {
      await api.dial(number);
      notify("info", `Calling ${name}. Talk on your iPhone.`);
      return true;
    } catch (e) {
      notify("error", `Couldn't call ${name}: ${errorMessage(e)}`);
      return false;
    } finally {
      calling.value = null;
    }
  }

  /** A missed call from this person still on the phone, to call back from (no hands-free needed). */
  function missedCallFrom(name: string | null, address: string | null): PhoneNotification | null {
    return missedCallFor(notifications.value, { name, address });
  }

  /** How tug can call this person right now: back from their missed call, by dialing, or not yet. */
  function callRoute(name: string | null, address: string | null): "back" | "dial" | null {
    if (missedCallFrom(name, address)) return "back";
    return canDial.value && address ? "dial" : null;
  }

  /**
   * Call someone from tug, the same way everywhere (Ctrl+K, Calls tab, Feed): press "Dial" on
   * their missed call if the phone still has one (works today, no hands-free link), else dial
   * over hands-free when that check has passed, else say why not. Never fails silently.
   */
  async function callPerson(name: string, address: string | null): Promise<boolean> {
    const missed = missedCallFrom(name, address);
    if (missed) {
      const ok = await attempt(() => api.performAction(missed.id, true).then(() => true));
      if (!ok) return false;
      notify("info", `Calling ${cleanName(name)} back on your iPhone.`);
      // The phone logs the call a moment later; show it in Calls without waiting.
      window.setTimeout(() => void api.refreshCalls().catch(() => undefined), 6000);
      return true;
    }
    if (canDial.value && address) return call(address, name);
    notify(
      "error",
      `tug can't call ${cleanName(name)} yet: Windows is holding your iPhone's calling connection. Missed calls still on your phone can be called back.`,
    );
    return false;
  }

  /** Settings › iPhone › Calls: open the hands-free link without calling. Only a pass turns Call buttons on. */
  async function checkDialing(): Promise<boolean> {
    try {
      await api.dial(null);
      await setSetting("dialing", true);
      notify("info", "The hands-free link works. Call buttons are on.");
      return true;
    } catch (e) {
      await setSetting("dialing", false);
      notify("error", `Calling anyone from tug isn't available here: ${errorMessage(e)}. Calling back missed calls still works.`);
      return false;
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
    composeTo.value = null;
    selectedThread.value = key;
    view.value = "messages";
    markSeen(key);
  }

  /**
   * Clicking the tray (or its Open) with unread texts: open the newest conversation that
   * has unread, as if it were clicked, so it's read and cleared normally. The backend only
   * emits this when there are unread; if nothing's unread by the time it arrives, do nothing
   * and leave the user where they were. An overlay or Settings would hide the conversation,
   * so close those first (but not a pairing dialog, which needs an answer).
   */
  function openLatestConversation() {
    const key = newestUnreadThread(notifications.value, newCount);
    if (!key) return;
    searchOpen.value = false;
    pickerOpen.value = false;
    closeSettings();
    openThread(key);
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
      closeToTray: raw["ui.closeToTray"] !== "false",
      appIcons: raw["ui.appIcons"] !== "false",
      dialing: raw["ui.dialing"] === "true",
    };
    onboarded.value = raw["ui.onboarded"] === "1";
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
        on("calls", (list) => (calls.value = list)),
        on("pairing-request", (req) => (pairingRequest.value = req)),
        on("pairing-request-closed", () => (pairingRequest.value = null)),
        on("open-latest-conversation", openLatestConversation),
      ])),
    );
    const [s, np, first, msgs, people, recent] = await Promise.all([
      api.getStatus(),
      api.getNowPlaying(),
      api.listNotifications(PAGE),
      api.listMessages(2000),
      api.getContacts(),
      api.getCalls(),
    ]);
    if (calls.value.length === 0) calls.value = recent;
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
    window.clearTimeout(freshTimer);
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

  /** Setup is done (or skipped): don't show it again on its own. */
  function finishSetup() {
    onboarded.value = true;
    setupRequested.value = false;
    setupActive.value = false;
    void attempt(() => api.setSetting("ui.onboarded", "1"));
  }

  /** App zoom from Settings (Ctrl +/−/0 does the same from anywhere). */
  function setZoom(factor: number) {
    zoom.value = factor;
    applyZoom(factor);
    void attempt(() => api.setSetting("ui.zoom", String(factor)));
  }

  function toggleMuted(appId: string) {
    const muted = settings.value.mutedApps;
    void setSetting("mutedApps", muted.includes(appId) ? muted.filter((a) => a !== appId) : [...muted, appId]);
  }

  return {
    setupSharingShown,
    status,
    nowPlaying,
    notifications,
    messages,
    contacts,
    calls,
    calling,
    canDial,
    call,
    checkDialing,
    /** Recent calls are on screen: ask the phone again (the backend throttles it). */
    refreshCalls: () => void api.refreshCalls().catch(() => undefined),
    missedCallFrom,
    callRoute,
    callPerson,
    hasMore,
    discovered,
    pairingRequest,
    settings,
    iconFor,
    advertiseEnabled,
    flash,
    view,
    selectedThread,
    composeTo,
    pickerOpen,
    searchOpen,
    unreadTexts,
    ringing,
    hideCall,
    respondToCall,
    overlayOpen,
    settingsSection,
    openSettings,
    closeSettings,
    zoom,
    setZoom,
    onboarded,
    setupRequested,
    showSetup,
    finishSetup,
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
    markAllRead,
    copyCode,
    latestCode,
    deleteConversation,
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
    /** Open a notification's web link in the default browser. Never clears the notification. */
    openUrl: (url: string) => attempt(() => api.openUrl(url)),
    /** True once the phone accepted the write (which isn't the player acting on it). */
    async media(command: MediaCommand): Promise<boolean> {
      return (await attempt(() => api.mediaCommand(command).then(() => true))) === true;
    },
    startDiscovery: () => attempt(api.startDiscovery),
    stopDiscovery: () => {
      discovered.value = [];
      return attempt(api.stopDiscovery);
    },
    async pair(id: string) {
      const ok = await attempt(() => api.pairDevice(id).then(() => true));
      // Setup shows this itself; a toast over it would just cover the screen.
      if (ok && !showSetup.value) notify("info", "Paired. Connecting to your iPhone…");
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
