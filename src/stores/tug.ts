import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import {
  appLabel,
  canClear,
  cleanName,
  formatAddress,
  groupConversations,
  groupThreads,
  isConversation,
  missedCallFor,
  newestUnreadThread,
  threadKey,
  type Conversation,
  type Thread,
} from "../lib/format";
import { toastSpec } from "../lib/toastSpec";
import { applyZoom, installZoomShortcuts } from "../lib/zoom";
import { ToastLimiter } from "../lib/toastLimiter";
import { findCode } from "../lib/codes";
import { batteryAlert } from "../lib/battery";
import { copyText } from "../lib/clipboard";
import { normalizeAddress } from "../lib/address";
import { isKnownConversation, outgoingAddresses, senderIndex, senderMayToast, threadCounts } from "../lib/senders";
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
  ToastPressed,
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
  lastErrorAt: null,
  pairingStale: false,
  awaitingPhoneAllow: false,
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
  /** The backend's status has arrived; until then `status` is a placeholder that says "noDevice". */
  const statusKnown = ref(false);
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
  const settings = ref<UiSettings>({
    toasts: true,
    doNotDisturb: false,
    mutedApps: [],
    closeToTray: true,
    appIcons: true,
    lowBattery: true,
    dialing: false,
    filterUnknown: true,
    knownSenders: [],
  });

  /** Who counts as a known sender (contacts, numbers you've texted, ones you moved), for Filter unknown senders. */
  const senders = computed(() => senderIndex(contacts.value, outgoingAddresses(messages.value), settings.value.knownSenders));
  /** With the filter off, everyone is treated as known, exactly as before it existed. */
  function isKnown(c: Pick<Conversation, "appId" | "contact" | "addresses">): boolean {
    return !settings.value.filterUnknown || isKnownConversation(c, senders.value);
  }
  const countsTowardUnread = (t: Thread) => !settings.value.filterUnknown || threadCounts(t, senders.value);

  /** App icons as data URIs by app id; null = the App Store has none (initials instead). */
  const appIcons = ref<Record<string, string | null>>({});
  const iconRequests = new Set<string>();
  /** App websites by app id (from the App Store), for "Open" on apps without a known page. */
  const appWebsites = ref<Record<string, string | null>>({});
  const websiteRequests = new Set<string>();
  function websiteFor(appId: string): string | null {
    if (!settings.value.appIcons) return null;
    if (!(appId in appWebsites.value) && !websiteRequests.has(appId)) {
      websiteRequests.add(appId);
      api
        .appWebsite(appId)
        .then((url) => (appWebsites.value = { ...appWebsites.value, [appId]: url }))
        .catch(() => undefined);
    }
    return appWebsites.value[appId] ?? null;
  }

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
  /** Start with Windows. Mirrors the real autostart registry entry, not a stored setting. */
  const autostartEnabled = ref(false);
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
  watch(
    [onboarded, () => status.value.device],
    ([done, device]) => {
      if (!done && device == null) setupActive.value = true;
    },
    { immediate: true },
  );
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
  let watchRenew: number | undefined;
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
    window.clearInterval(watchRenew);
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
    (on) => {
      void api.setWatching(on).catch(() => undefined);
      // The backend lets fast checks lapse after 90 s unless renewed, so a missed "off"
      // can't leave the phone polled every 2 s; renew while they're wanted.
      window.clearInterval(watchRenew);
      if (on) watchRenew = window.setInterval(() => void api.setWatching(true).catch(() => undefined), 45_000);
    },
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
  /** Windows pop-ups are on, not held by Do not disturb, and allowed by Windows. */
  async function canToast(): Promise<boolean> {
    const s = settings.value;
    if (!s.toasts || s.doNotDisturb) return false;
    if (toastPermission === null) {
      toastPermission = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    }
    return toastPermission;
  }

  // Low phone battery: one pop-up at 20% and one at 10% per discharge (see lib/battery).
  let batteryAlerted: number | null = null;
  async function checkBattery(level: number | null) {
    const r = batteryAlert(level, batteryAlerted);
    // An alert that couldn't be shown (switch off, Do not disturb…) isn't used up: it comes
    // when it can. Only a shown alert (or a charge, which resets it) moves the mark.
    if (r.alert == null) {
      batteryAlerted = r.alerted;
      return;
    }
    if (!settings.value.lowBattery || !(await canToast())) return;
    batteryAlerted = r.alerted;
    sendNotification({ title: "iPhone battery low", body: `${level}% left. Time to charge it.` });
  }

  async function maybeToast(n: PhoneNotification) {
    if (n.flags.silent || n.flags.preExisting || settings.value.mutedApps.includes(n.appId)) return;
    // Unknown senders wait quietly in their own list, unless the text carries a one-time code.
    if (settings.value.filterUnknown && !senderMayToast(n, senders.value)) return;
    if (!(await canToast())) return;
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
    // With buttons for what applies (reply, mark read, copy code, call back, clear); the
    // backend falls back to a plain pop-up itself if Windows won't take that one.
    const spec = toastSpec(n, messages.value, contacts.value);
    api.showToast(spec).catch(() => sendNotification({ title: spec.title, body: spec.body }));
  }

  /**
   * A pop-up's button or body was pressed and the backend has done its part (sent the
   * reply, copied the code, asked the phone to call back). Bring tug's own state along,
   * the same way the in-app buttons do.
   */
  function onToastPressed({ kind, id }: ToastPressed) {
    const n = notifications.value.find((x) => x.id === id);
    if (!n) return;
    if (kind === "open") {
      // tug is already in front; show the notification that was clicked.
      searchOpen.value = false;
      pickerOpen.value = false;
      closeSettings();
      if (isConversation(n)) {
        openThread(threadKey(n));
      } else if (n.category !== "incomingCall") {
        view.value = "feed";
        focusItem.value = `n${n.id}`;
      }
    } else if (kind === "read" || kind === "replied") {
      // As if the conversation had been opened: seen here, read and cleared on the phone.
      const key = threadKey(n);
      const c = groupConversations(notifications.value, messages.value, contacts.value).find((x) => x.key === key);
      markSeen(key);
      const ids = c ? messages.value.filter((m) => m.direction === "in" && c.addresses.includes(m.address)).map((m) => m.id) : [];
      readConversation(c?.notifications ?? [n], ids);
    } else if (kind === "copied") {
      // Same as Copy in tug: the code's notification has done its job on the phone.
      void clearItems([n], { quiet: true });
    } else if (kind === "calledBack") {
      // The phone logs the call a moment later; show it in Calls without waiting.
      window.setTimeout(() => void api.refreshCalls().catch(() => undefined), 6000);
    }
  }

  function isNew(key: string, n: PhoneNotification): boolean {
    // Cleared on the phone or watch means it was already read there.
    return n.removedAt == null && n.receivedAt > (seen.value[key] ?? seenSince.value);
  }

  /** New texts across conversations: the Messages tab badge, the tray and the taskbar dot. Unknown senders don't count. */
  const unreadTexts = computed(() =>
    groupThreads(notifications.value).reduce((sum, t) => sum + (countsTowardUnread(t) ? newCount(t.key, t.items) : 0), 0),
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
    const key = newestUnreadThread(notifications.value, newCount, countsTowardUnread);
    if (!key) return;
    searchOpen.value = false;
    pickerOpen.value = false;
    closeSettings();
    openThread(key);
  }

  async function loadSettings() {
    const raw = await api.getSettings();
    // Set this first: on a fresh install `ui.onboarded` is absent, and if a later await in here
    // threw, onboarded stayed at its optimistic default (true) and the wizard never opened.
    onboarded.value = raw["ui.onboarded"] === "1";
    zoom.value = Number(raw["ui.zoom"]) || 1;
    applyZoom(zoom.value);
    seen.value = raw["ui.seen"] ? (JSON.parse(raw["ui.seen"]) as Record<string, number>) : {};
    if (raw["ui.seenSince"]) {
      seenSince.value = Number(raw["ui.seenSince"]);
    } else {
      await api.setSetting("ui.seenSince", String(seenSince.value));
    }
    advertiseEnabled.value = raw.advertise !== "false";
    // The autostart registry entry is the source of truth, not a stored setting.
    autostartEnabled.value = (await attempt(api.getAutostart)) ?? false;
    settings.value = {
      toasts: raw["ui.toasts"] !== "false",
      doNotDisturb: raw["ui.doNotDisturb"] === "true",
      mutedApps: raw["ui.mutedApps"] ? (JSON.parse(raw["ui.mutedApps"]) as string[]) : [],
      closeToTray: raw["ui.closeToTray"] !== "false",
      lowBattery: raw["ui.lowBattery"] !== "false",
      appIcons: raw["ui.appIcons"] !== "false",
      dialing: raw["ui.dialing"] === "true",
      filterUnknown: raw["ui.filterUnknown"] !== "false",
      knownSenders: raw["ui.knownSenders"] ? (JSON.parse(raw["ui.knownSenders"]) as string[]) : [],
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
          if (s.battery !== status.value.battery) void checkBattery(s.battery);
          status.value = s;
          // Notification UIDs die with the connection, so nothing stays actionable.
          if (wasConnected && s.connection !== "connected") {
            for (const n of notifications.value) n.live = false;
          }
          statusKnown.value = true;
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
        on("toast-pressed", onToastPressed),
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
    statusKnown.value = true;
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

  /** "Move to conversations": these numbers count as known from now on (saved in `ui.knownSenders`). */
  function moveToConversations(c: Pick<Conversation, "contact" | "addresses">) {
    const add = c.addresses.length ? c.addresses : [c.contact];
    const next = [...new Set([...settings.value.knownSenders, ...add.map(normalizeAddress)])];
    void setSetting("knownSenders", next);
    notify("info", `Moved ${c.contact} to your conversations`);
  }

  function toggleMuted(appId: string) {
    const muted = settings.value.mutedApps;
    void setSetting("mutedApps", muted.includes(appId) ? muted.filter((a) => a !== appId) : [...muted, appId]);
  }

  return {
    setupSharingShown,
    status,
    statusKnown,
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
    websiteFor,
    advertiseEnabled,
    autostartEnabled,
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
    isKnown,
    moveToConversations,
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
    /** Pair the iPhone's Classic (texts) side from inside tug; true on success. */
    async pairTexts(): Promise<boolean> {
      const ok = await attempt(() => api.pairTexts().then(() => true));
      return ok === true;
    },
    async setAdvertising(enabled: boolean) {
      advertiseEnabled.value = enabled;
      await attempt(() => api.setAdvertising(enabled));
    },
    async setAutostart(enabled: boolean) {
      // Show the change at once, then confirm against what the registry actually holds, so a
      // failure doesn't leave the switch lying about whether tug starts with Windows.
      autostartEnabled.value = enabled;
      const ok = await attempt(() => api.setAutostart(enabled).then(() => true));
      if (ok !== true) {
        autostartEnabled.value = (await attempt(api.getAutostart)) ?? false;
      }
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
