import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import { getVersion } from "@tauri-apps/api/app";
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
import { replyAddress, toastSpec } from "../lib/toastSpec";
import { shouldPopUp, type PopupEvent } from "../lib/popup";
import { isVip, vipIndex } from "../lib/vips";
import { applyZoom, installZoomShortcuts } from "../lib/zoom";
import { ToastLimiter } from "../lib/toastLimiter";
import { findCode } from "../lib/codes";
import { codeEntries, codeToastForMessage, newestCode, type CodeEntry } from "../lib/codeFeed";
import { MESSAGES_APP } from "../lib/format";
import { batteryAlert } from "../lib/battery";
import { copyText } from "../lib/clipboard";
import { isAddressLike, normalizeAddress } from "../lib/address";
import type { ToastSpec } from "../types/protocol";
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
  SpotifyDevice,
  SpotifyPlayer,
  SpotifyPlaylist,
  SpotifyStatus,
  SpotifyTrack,
  ToastPressed,
  UiSettings,
} from "../types/protocol";
import { bestTrack, nextRepeat, sameSong } from "../lib/spotify";
import { nextShowConnect, shouldWatchSwitches } from "../lib/connectFlow";
import { notesUpTo, RELEASE_NOTES, whatsNewToShow, type ReleaseNote } from "../lib/whatsNew";
import { useTugboatStore } from "./tugboat";

/** The Spotify panel's tabs. */
export type SpotifyTab = "search" | "playlists" | "recent" | "top" | "queue";

const PAGE = 100;

/** A fresh quiet-hours schedule, off, defaulting to a typical overnight window. */
const DEFAULT_QUIET_HOURS = { enabled: false, start: "22:00", end: "07:00", days: [] as number[] };

/** What the backend says when Spotify can't see the iPhone (mirrors `model::NO_PHONE`). */
const SPOTIFY_NO_PHONE = "Open Spotify on your iPhone.";
/** How long to wait for Spotify to open on the iPhone before giving up. */
const SPOTIFY_WAIT_MS = 60_000;

export type SettingsSection = "general" | "iphone" | "notifications" | "connectors" | "weather" | "privacy" | "about";
const SEEN_KEEP = 300;
/** How many cleared-code message ids to remember (they expire from the Feed in minutes anyway). */
const CLEARED_CODES_KEEP = 200;
/** A text and an ANCS notification carry the same code if they're this close: don't pop up twice. */
const CODE_TOAST_WINDOW_MS = 10 * 60 * 1000;
/**
 * Toast ids for code texts are their message id plus this base, so they never collide with a real
 * notification's id: a press on one finds no notification to act on (the code was copied by the
 * backend regardless), which is exactly right — a text has nothing to clear in the Feed.
 */
const CODE_TEXT_TOAST_BASE = 1_000_000_000;
/** The low-battery pop-up's toast id: outside notification and code-text ids, so a press only opens tug. */
const BATTERY_TOAST_ID = 2_000_000_000;

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
  awaitingUnlock: false,
  reconnecting: false,
  messagesError: null,
  contactsError: null,
  contactsShared: false,
  textsPairing: "unknown",
  textsDevice: null,
  liveTexts: "off",
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

/** A send the iPhone didn't take; the message stays in the conversation with Retry. */
const NOT_SENT = "Your iPhone didn't send that text. Use Retry when it's nearby.";

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
    quietHours: { ...DEFAULT_QUIET_HOURS },
    vips: [],
    muteCalls: false,
    closeToTray: true,
    appIcons: true,
    lowBattery: true,
    dialing: false,
    filterUnknown: true,
    knownSenders: [],
  });

  /** Who is a VIP (always let through), rebuilt when the list or the contacts change. */
  const vips = computed(() => vipIndex(settings.value.vips, contacts.value));
  /** The pop-up policy event for a notification: its app, whether it's a call, and VIP status. */
  function popupEventFor(n: PhoneNotification): PopupEvent {
    const address = n.appId === MESSAGES_APP ? replyAddress(n, messages.value, contacts.value) : null;
    return {
      appId: n.appId,
      isCall: n.category === "incomingCall",
      isVip: isVip(vips.value, { name: n.title, address }),
    };
  }

  // --- Spotify connector ---
  const spotify = ref<SpotifyStatus>({ connected: false, account: null });
  /** The Spotify playback snapshot (repeat/shuffle/like/art), read once per song (no polling). */
  const spotifyPlayer = ref<SpotifyPlayer | null>(null);
  /** The user's playlists, loaded on connect / first use and cached for Ctrl+K and the panel. */
  const playlists = ref<SpotifyPlaylist[]>([]);
  /** The Spotify panel (overlay) is open. */
  const spotifyPanelOpen = ref(false);
  /** Which tab the panel opens on next (Ctrl+K "spotify" vs the Now Playing button). */
  const spotifyPanelTab = ref<SpotifyTab>("search");
  /** Connect in progress (the browser is open waiting for sign-in). */
  const spotifyConnecting = ref(false);
  /** The "Play on" target: a chosen device, or null for the iPhone (the default). */
  const spotifyDevice = ref<SpotifyDevice | null>(null);

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

  /** Spotify cover art as data URIs by image URL (fetched through tug, cached on disk by the backend). */
  const spotifyCovers = ref<Record<string, string | null>>({});
  const coverRequests = new Set<string>();
  /** A Spotify cover for a row, asked the first time it's shown. Null until it arrives (placeholder meanwhile). */
  function spotifyCoverFor(url: string | null | undefined): string | null {
    if (!url) return null;
    if (!(url in spotifyCovers.value) && !coverRequests.has(url)) {
      coverRequests.add(url);
      api
        .spotifyCover(url)
        .then((uri) => (spotifyCovers.value = { ...spotifyCovers.value, [url]: uri }))
        .catch(() => undefined);
    }
    return spotifyCovers.value[url] ?? null;
  }

  /** Contact photos as data URIs, keyed by the lookup (a number or a name); null = no photo (initials). */
  const contactPhotos = ref<Record<string, string | null>>({});
  const photoRequests = new Set<string>();
  /**
   * A contact's photo for their avatar, asked the first time it's shown (then cached by the
   * backend). `key` is a phone number or the name on the avatar; the backend resolves either.
   * Returns null until it arrives, and whenever the iPhone shares no photo — the avatar shows
   * initials in the meantime.
   */
  function contactPhoto(key: string | null | undefined): string | null {
    if (!key) return null;
    if (!(key in contactPhotos.value) && !photoRequests.has(key)) {
      photoRequests.add(key);
      api
        .contactPhoto(key)
        .then((uri) => (contactPhotos.value = { ...contactPhotos.value, [key]: uri }))
        // Not shared yet, or the lookup failed: initials for now, asked again on the next sync.
        .catch(() => undefined);
    }
    return contactPhotos.value[key] ?? null;
  }
  const advertiseEnabled = ref(true);
  /** Start with Windows. Mirrors the real autostart registry entry, not a stored setting. */
  const autostartEnabled = ref(false);
  const zoom = ref(1);
  /**
   * The "Connect your iPhone" panel stands in for the Feed while the user sets up their iPhone, and
   * yields once notifications work (lib/connectFlow). The latch only engages when there's no device
   * after the real status is known, so an upgrader whose paired phone is merely reconnecting at
   * launch lands straight on the Feed and never sees it; Start over clears the device and brings it
   * back. Settings › iPhone shows the same panel component regardless of this flag.
   */
  const showConnect = ref(false);
  /** "Skip for now" on the panel's switches: let it yield with optional switches still off. */
  const connectSkipped = ref(false);
  watch(
    [statusKnown, status, connectSkipped],
    () => {
      showConnect.value = nextShowConnect(showConnect.value, statusKnown.value, status.value, connectSkipped.value);
      // A new setup (no phone) waits for the switches again.
      if (status.value.device == null) connectSkipped.value = false;
    },
    { immediate: true, deep: true },
  );
  const flash = ref<{ kind: "error" | "info"; text: string; action?: { label: string; run: () => void } } | null>(null);
  let flashTimer: number | undefined;
  /** The pending once-per-song Spotify read (see refreshForSong). */
  let spotifySongTimer: number | undefined;

  // --- What's new -----------------------------------------------------------------------
  /** The running app version (from getVersion), null until read / in a plain-browser dev build. */
  const appVersion = ref<string | null>(null);
  /** The newest version whose "What's new" card the user has seen; null until settings load, and
   *  null again means "fresh install" — the first launch records the version silently. */
  const lastSeenVersion = ref<string | null>(null);
  /** The "What's new" card is open. */
  const whatsNewOpen = ref(false);
  /** The release notes the open card is showing (newest first). */
  const whatsNewEntries = ref<ReleaseNote[]>([]);
  /** Notes waiting to be shown once the Connect panel is out of the way (see maybeShowWhatsNew). */
  const pendingWhatsNew = ref<ReleaseNote[]>([]);

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
  const overlayOpen = computed(
    () =>
      searchOpen.value ||
      pickerOpen.value ||
      spotifyPanelOpen.value ||
      whatsNewOpen.value ||
      useTugboatStore().open ||
      !!pairingRequest.value ||
      !!ringing.value,
  );

  function openSettings(section?: SettingsSection) {
    if (view.value !== "settings") viewBeforeSettings = view.value;
    if (section) settingsSection.value = section;
    view.value = "settings";
  }

  function closeSettings() {
    if (view.value === "settings") view.value = viewBeforeSettings;
  }
  let watchRenew: number | undefined;
  const pageVisible = ref(document.visibilityState === "visible");
  // Registered in init() and removed in dispose(), so it's torn down with the rest (see teardown).
  const onVisibilityChange = () => (pageVisible.value = document.visibilityState === "visible");
  // The switches get flipped on the phone, with tug on any screen or in the tray. So for the
  // first minutes after launch or pairing, check fast whenever one is still off, too.
  const FRESH_MS = 5 * 60 * 1000;
  const fresh = ref(true);
  let freshTimer = window.setTimeout(() => (fresh.value = false), FRESH_MS);
  watch(
    () => status.value.device?.id,
    (id) => {
      if (!id) return;
      // A fresh connection reopens the fast-check window; the renew interval below owns its own
      // lifecycle (set when watching turns on, cleared on re-run and in dispose), so leave it be.
      fresh.value = true;
      window.clearTimeout(freshTimer);
      freshTimer = window.setTimeout(() => (fresh.value = false), FRESH_MS);
    },
  );
  const switchesPending = computed(() => {
    const s = status.value;
    return !!s.device && (!s.services.notifications || !s.services.messages || contacts.value.length === 0);
  });
  /**
   * The Connect panel is on screen and tug is visible: the Feed stand-in (showConnect) outside
   * Settings, or Settings › iPhone. The same component shows in both places; watching keys off its
   * visibility so flipping a switch on the phone turns green within a couple of seconds.
   */
  const connectPanelVisible = computed(
    () =>
      pageVisible.value &&
      (view.value === "settings" ? settingsSection.value === "iphone" : showConnect.value),
  );
  watch(
    () =>
      shouldWatchSwitches({
        panelVisible: connectPanelVisible.value,
        fresh: fresh.value,
        switchesPending: switchesPending.value,
      }),
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
  /** Code-text rows the user cleared from the Feed (hidden, not deleted); persisted in `ui.clearedCodes`. */
  const clearedCodes = ref<number[]>([]);
  /** Ticks each minute so the code Feed drops rows once their texts age out of the recency window. */
  const clock = ref(Date.now());
  let clockTimer: number | undefined;

  /**
   * Verification codes that arrived as texts but never raised a notification, shown in the Feed with
   * a Copy code button. Derived from messages, de-duped against any notification that carried the
   * same code, and filtered by the cleared list — no stored state beyond the handful of cleared ids.
   */
  const codeFeed = computed<CodeEntry[]>(() =>
    codeEntries(messages.value, notifications.value, {
      now: clock.value,
      cleared: clearedCodes.value,
      contacts: contacts.value,
    }),
  );

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

  /** Returns true when the message is new (so a live code text can pop up; an update doesn't). */
  function upsertMessage(m: SmsMessage): boolean {
    const i = messages.value.findIndex((x) => x.id === m.id);
    if (i >= 0) {
      messages.value[i] = m;
      return false;
    }
    messages.value.push(m);
    return true;
  }

  // Windows notification permission: ask once per launch, not on every notification.
  let toastPermission: boolean | null = null;
  const toasts = new ToastLimiter();
  let toastSummary: number | undefined;
  /** Windows will show pop-ups (permission granted). The settings policy is `shouldPopUp`. */
  async function hasToastPermission(): Promise<boolean> {
    if (toastPermission === null) {
      toastPermission = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    }
    return toastPermission;
  }
  /** Whether the settings let a pop-up through right now, for a given event (see lib/popup). */
  const popupAllowed = (event: PopupEvent) => shouldPopUp(event, settings.value, new Date());

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
    // A low-battery alert is a system pop-up (no app, no VIP): held by Windows alerts off, DND or
    // quiet hours, like any other.
    if (!settings.value.lowBattery) return;
    if (!popupAllowed({ appId: "", isCall: false, isVip: false }) || !(await hasToastPermission())) return;
    batteryAlerted = r.alerted;
    // Through tug's own Windows toast like every other pop-up: the generic notification call
    // never showed on the test PC at 20%. Its id sits outside notification ids, so pressing it
    // just opens tug.
    const spec: ToastSpec = {
      id: BATTERY_TOAST_ID,
      title: "iPhone battery low",
      body: `${level}% left. Time to charge it.`,
      name: "",
      replyTo: null,
      markRead: false,
      code: null,
      callBack: false,
      clear: false,
    };
    api.showToast(spec).catch(() => sendNotification({ title: spec.title, body: spec.body }));
  }

  // A one-time code shouldn't pop up twice when it arrives on both an ANCS notification and a MAP
  // text: whichever fires first records the code here and the other stands down for the window.
  const codeToastedAt = new Map<string, number>();
  function recentlyCodeToasted(code: string, now = Date.now()): boolean {
    const last = codeToastedAt.get(code);
    return last !== undefined && now - last < CODE_TOAST_WINDOW_MS;
  }
  function markCodeToasted(code: string, now = Date.now()) {
    codeToastedAt.set(code, now);
    for (const [c, at] of codeToastedAt) if (now - at > CODE_TOAST_WINDOW_MS) codeToastedAt.delete(c);
  }

  /** Calls always ring through; everything else is rate-limited and the overflow summed up once. */
  function admitToast(isCall: boolean): boolean {
    if (isCall || toasts.admit(Date.now())) return true;
    if (toastSummary === undefined) {
      toastSummary = window.setTimeout(() => {
        toastSummary = undefined;
        const held = toasts.takeHeld();
        if (held > 0) sendNotification({ title: "tug", body: `${held} more notification${held === 1 ? "" : "s"}` });
      }, toasts.windowMs);
    }
    return false;
  }

  async function maybeToast(n: PhoneNotification) {
    if (n.flags.silent || n.flags.preExisting) return;
    // Unknown senders wait quietly in their own list, unless the text carries a one-time code.
    if (settings.value.filterUnknown && !senderMayToast(n, senders.value)) return;
    // Settings policy: muted apps, Do not disturb, quiet hours, VIP let-through, call handling.
    const event = popupEventFor(n);
    if (!popupAllowed(event)) return;
    const code = findCode(n.message || n.subtitle)?.code ?? null;
    if (code !== null && recentlyCodeToasted(code)) return; // a text pop-up already carried this code
    // Claim the code before awaiting: the notification and the text for one code can arrive in the
    // same tick, and both would pass the check above if the claim came after the await.
    if (code !== null) markCodeToasted(code);
    if (!(await hasToastPermission())) return;
    if (!admitToast(event.isCall)) return;
    // With buttons for what applies (reply, mark read, copy code, call back, clear); the
    // backend falls back to a plain pop-up itself if Windows won't take that one.
    const spec = toastSpec(n, messages.value, contacts.value);
    api.showToast(spec).catch(() => sendNotification({ title: spec.title, body: spec.body }));
  }

  /**
   * A text just arrived live (not startup backlog) carrying a code, and no ANCS notification popped
   * it: raise a pop-up with Copy code, respecting the toast settings, Do-not-disturb and the limiter.
   * The toast's id is the message id offset into its own range, so a press has no notification to act
   * on — the backend copies the code itself, and a code text has nothing to clear in the Feed.
   */
  async function maybeToastMessage(m: SmsMessage) {
    const code = codeToastForMessage(m, notifications.value, { contacts: contacts.value });
    if (code === null || recentlyCodeToasted(code)) return;
    // A code text obeys the same policy as a notification carrying a code (Messages app, no call).
    const event: PopupEvent = { appId: MESSAGES_APP, isCall: false, isVip: isVip(vips.value, { name: m.contactName, address: m.address }) };
    if (!popupAllowed(event)) return;
    markCodeToasted(code); // claimed before the await (see maybeToast)
    if (!(await hasToastPermission())) return;
    if (!admitToast(false)) return;
    const known = m.contactName ?? contacts.value.find((c) => c.address === m.address)?.name;
    const name = known && !isAddressLike(known) ? cleanName(known) : formatAddress(m.address);
    const spec: ToastSpec = {
      id: CODE_TEXT_TOAST_BASE + m.id,
      title: ["Messages", name].filter(Boolean).join(" · "),
      body: m.body,
      name,
      replyTo: null,
      markRead: false,
      code,
      callBack: false,
      clear: false,
    };
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
    return newestCode(notifications.value, messages.value, { maxAgeMs });
  }

  /** Hide a code-text row from the Feed (every text behind it); the texts stay in Messages. */
  function clearCode(entry: CodeEntry) {
    const next = [...new Set([...clearedCodes.value, ...entry.messageIds])].slice(-CLEARED_CODES_KEEP);
    clearedCodes.value = next;
    void attempt(() => api.setSetting("ui.clearedCodes", JSON.stringify(next)));
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
      quietHours: raw["ui.quietHours"]
        ? { ...DEFAULT_QUIET_HOURS, ...(JSON.parse(raw["ui.quietHours"]) as Partial<UiSettings["quietHours"]>) }
        : { ...DEFAULT_QUIET_HOURS },
      vips: raw["ui.vips"] ? (JSON.parse(raw["ui.vips"]) as string[]) : [],
      muteCalls: raw["ui.muteCalls"] === "true",
      closeToTray: raw["ui.closeToTray"] !== "false",
      lowBattery: raw["ui.lowBattery"] !== "false",
      appIcons: raw["ui.appIcons"] !== "false",
      dialing: raw["ui.dialing"] === "true",
      filterUnknown: raw["ui.filterUnknown"] !== "false",
      knownSenders: raw["ui.knownSenders"] ? (JSON.parse(raw["ui.knownSenders"]) as string[]) : [],
    };
    clearedCodes.value = raw["ui.clearedCodes"] ? (JSON.parse(raw["ui.clearedCodes"]) as number[]) : [];
    // A missing key is a fresh install: null (not ""), so the first launch records the version
    // silently instead of greeting a brand-new user with a card about changes they never saw.
    // Before 0.5.9 nothing recorded a seen version. Someone who finished the old setup wizard
    // ("ui.onboarded") is updating, not installing fresh: treat them as having seen 0.5.8 so
    // the 0.5.9 card greets them instead of being skipped as a first install.
    lastSeenVersion.value =
      "ui.lastSeenVersion" in raw ? raw["ui.lastSeenVersion"] : raw["ui.onboarded"] === "1" ? "0.5.8" : null;
  }

  /** Remember the version whose card the user has now seen, so it doesn't show again. */
  async function recordSeenVersion(version: string) {
    lastSeenVersion.value = version;
    await attempt(() => api.setSetting("ui.lastSeenVersion", version));
  }

  /** Open the card on these notes and mark the current version seen (so launch won't repeat it). */
  function showWhatsNew(entries: ReleaseNote[]) {
    if (entries.length === 0) return;
    whatsNewEntries.value = entries;
    whatsNewOpen.value = true;
    if (appVersion.value) void recordSeenVersion(appVersion.value);
  }

  /**
   * After an update, show the "What's new" card once — but not while the Connect panel owns the
   * main area (a fresh install or Start over). In that case the notes wait and the watcher below
   * shows them the moment setup is done, so the card never interrupts connecting. A fresh install
   * has no last-seen version, so it records silently here and shows nothing at all.
   */
  async function checkWhatsNew() {
    appVersion.value = await getVersion().catch(() => null);
    const version = appVersion.value;
    if (!version) return; // plain-browser dev build: no native version to compare against
    if (lastSeenVersion.value === null) {
      await recordSeenVersion(version); // fresh install: record quietly, don't greet with a card
      return;
    }
    const entries = whatsNewToShow(RELEASE_NOTES, version, lastSeenVersion.value);
    if (entries.length === 0) return;
    if (showConnect.value) pendingWhatsNew.value = entries;
    else showWhatsNew(entries);
  }

  // Held-back notes (fresh install / Start over) show once the Connect panel yields to the Feed.
  watch(showConnect, (connecting) => {
    if (!connecting && pendingWhatsNew.value.length && !whatsNewOpen.value) {
      const entries = pendingWhatsNew.value;
      pendingWhatsNew.value = [];
      showWhatsNew(entries);
    }
  });

  /** Settings › About: reopen the card any time, showing every release up to the current version. */
  function openWhatsNew() {
    const notes = appVersion.value ? notesUpTo(RELEASE_NOTES, appVersion.value) : RELEASE_NOTES;
    whatsNewEntries.value = notes.length ? notes : RELEASE_NOTES;
    whatsNewOpen.value = true;
  }

  // Listeners and shortcuts are installed once and torn down by dispose(), so a
  // remount (or dev hot-reload) can't double every toast and keypress.
  let teardown: Array<UnlistenFn | (() => void)> = [];
  let started = false;
  /** True once the startup backlog has loaded, so a live code text can pop up but backlog can't. */
  let messagesReady = false;

  async function init() {
    if (started) return;
    started = true;
    document.addEventListener("visibilitychange", onVisibilityChange);
    teardown.push(() => document.removeEventListener("visibilitychange", onVisibilityChange));
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
        on("message", (m) => {
          const added = upsertMessage(m);
          // Only live arrivals pop up; the startup backlog (loaded below) must stay quiet.
          if (added && messagesReady) void maybeToastMessage(m);
        }),
        on("contacts", (list) => {
          contacts.value = list;
          // A resync may have added, changed or removed photos: drop the cache so avatars re-ask.
          contactPhotos.value = {};
          photoRequests.clear();
          // Names are joined into messages server-side; re-read them for what's loaded.
          void refreshLoadedMessageNames();
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
    // The backlog is in; from here a newly delivered code text is a live arrival that may pop up.
    messagesReady = true;
    // Let code rows age out of the Feed's recency window even when nothing else changes.
    clockTimer = window.setInterval(() => (clock.value = Date.now()), 60_000);
    await attempt(loadSettings);
    // Settings are in (so lastSeenVersion is known): decide whether to greet with "What's new".
    void checkWhatsNew();
    void loadSpotify();
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    // Everything with a lifetime gets cleared here, so init() can be called again cleanly (a
    // remount or dev hot-reload) without a leaked timer firing or an interval double-polling.
    window.clearTimeout(toastSummary);
    spotifyWait++;
    window.clearTimeout(freshTimer);
    window.clearTimeout(spotifySongTimer);
    spotifySongTimer = undefined;
    window.clearTimeout(flashTimer);
    window.clearInterval(watchRenew);
    window.clearInterval(clockTimer);
    toastSummary = undefined;
    watchRenew = undefined;
    clockTimer = undefined;
    messagesReady = false;
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
  /**
   * Take each loaded text's name from the server, which knows the whole story: the contact's
   * name when there is one, else the name the iPhone sent with the text. Guessing from the
   * contacts list alone wiped phone-sent names and brought deleted contacts back on restart.
   */
  async function refreshLoadedMessageNames() {
    const fresh = await api.listMessages(Math.max(messages.value.length, 2000)).catch(() => null);
    if (!fresh) return;
    const names = new Map(fresh.map((m) => [m.id, m.contactName]));
    for (const m of messages.value) if (names.has(m.id)) m.contactName = names.get(m.id) ?? null;
  }

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

  /** Add someone to "Always let through" (VIPs), by their normalised address. No-op if already there. */
  function addVip(address: string) {
    const a = normalizeAddress(address);
    if (!a || settings.value.vips.includes(a)) return;
    void setSetting("vips", [...settings.value.vips, a]);
  }

  function removeVip(address: string) {
    void setSetting("vips", settings.value.vips.filter((v) => v !== normalizeAddress(address)));
  }

  // --- Spotify connector ---------------------------------------------------------------
  /** Whether to augment Now Playing with Spotify: connected, and Spotify is the AMS player. */
  const spotifyActive = computed(() => spotify.value.connected && nowPlaying.value.player === "Spotify");

  async function loadSpotify() {
    const s = await attempt(api.spotifyStatus);
    if (s) spotify.value = s;
    if (spotify.value.connected) void loadPlaylists();
  }

  /** Load the user's playlists once (cached); pass force to reload after a (re)connect. */
  async function loadPlaylists(force = false) {
    if (!spotify.value.connected || (playlists.value.length && !force)) return;
    const list = await attempt(api.spotifyPlaylists);
    if (list) playlists.value = list;
  }

  /** Run the OAuth flow (opens the browser). Resolves when the loopback redirect returns. */
  async function connectSpotify(): Promise<boolean> {
    spotifyConnecting.value = true;
    notify("info", "Finish signing in to Spotify in your browser…");
    try {
      spotify.value = await api.spotifyConnect();
      playlists.value = [];
      void loadPlaylists(true);
      notify("info", spotify.value.account ? `Connected to Spotify as ${spotify.value.account}.` : "Connected to Spotify.");
      return true;
    } catch (e) {
      notify("error", errorMessage(e));
      return false;
    } finally {
      spotifyConnecting.value = false;
    }
  }

  async function disconnectSpotify() {
    const s = await attempt(api.spotifyDisconnect);
    if (s) spotify.value = s;
    playlists.value = [];
    spotifyPlayer.value = null;
    spotifyDevice.value = null;
  }

  /** The current "Play on" target id for playback calls (null = the iPhone default). */
  const spotifyTargetId = computed(() => spotifyDevice.value?.id ?? null);
  /** The name of the current target, for messages ("Playing on <device>"). */
  const spotifyTargetName = computed(() => spotifyDevice.value?.name ?? status.value.device?.name ?? "your iPhone");

  /** Bumped to cancel a pending "start when Spotify opens on the iPhone". */
  let spotifyWait = 0;
  /**
   * Run a playback call, handling the iPhone's "open Spotify first" case. Spotify can only reach the
   * phone while its Spotify app is open, so when it isn't (`SPOTIFY_NO_PHONE`), ask the person to
   * open it and retry until it shows up (no second tap). An explicit non-phone device never waits.
   * Resolves true once playing or waiting, false on an error.
   */
  async function startPlayback(doPlay: () => Promise<void>, what: string): Promise<boolean> {
    const onDevice = spotifyDevice.value ? ` on ${spotifyTargetName.value}` : " on your iPhone";
    const playing = () => notify("info", `Playing ${what}${onDevice}.`);
    const tryPlay = async (): Promise<"ok" | "no-phone" | string> => {
      try {
        await doPlay();
        return "ok";
      } catch (e) {
        const msg = errorMessage(e);
        return msg === SPOTIFY_NO_PHONE ? "no-phone" : msg;
      }
    };
    const first = await tryPlay();
    if (first === "ok") {
      playing();
      void refreshSpotifyPlayer();
      return true;
    }
    if (first !== "no-phone") {
      notify("error", first);
      return false;
    }
    const waitId = ++spotifyWait;
    notify("info", "Open Spotify on your iPhone. It starts as soon as it's open.", {
      label: "Cancel",
      run: () => {
        if (spotifyWait === waitId) spotifyWait++;
      },
    });
    void (async () => {
      const deadline = Date.now() + SPOTIFY_WAIT_MS;
      while (spotifyWait === waitId && Date.now() < deadline) {
        await new Promise((r) => window.setTimeout(r, 2000));
        if (spotifyWait !== waitId) return;
        const result = await tryPlay();
        if (spotifyWait !== waitId) return;
        if (result === "ok") {
          void refreshSpotifyPlayer();
          return playing();
        }
        if (result !== "no-phone") return notify("error", result);
      }
      if (spotifyWait === waitId) notify("error", "Couldn't reach Spotify on your iPhone. Open it and try again.");
    })();
    return true;
  }

  /** Start a playlist/album/artist context (panel and Ctrl+K). Never fails silently. */
  function playContext(uri: string, name?: string): Promise<boolean> {
    return startPlayback(() => api.spotifyPlayContext(uri, spotifyTargetId.value), name ?? "it");
  }
  /** Backwards-compatible name (playlists are contexts). */
  const playPlaylist = playContext;

  /** Play one track; `contextUri` (its album/playlist) keeps the following songs going. */
  function playTrack(track: Pick<SpotifyTrack, "uri" | "name">, contextUri?: string | null): Promise<boolean> {
    return startPlayback(() => api.spotifyPlayTrack(track.uri, contextUri ?? null, spotifyTargetId.value), track.name);
  }

  /** Ctrl+K "play <song or artist>": search tracks and play the best match in its album. */
  async function playFromSearch(query: string): Promise<boolean> {
    const r = await attempt(() => api.spotifySearch(query, ["track"], 0));
    const best = r ? bestTrack(query, r.tracks) : null;
    if (!best) {
      notify("error", `No song on Spotify matches “${query}”.`);
      return false;
    }
    return playTrack(best, best.albumUri);
  }

  /** Add a track to the queue (panel and Ctrl+K "queue <song>"). */
  async function addToQueue(track: Pick<SpotifyTrack, "uri" | "name">): Promise<boolean> {
    const ok = await attempt(() => api.spotifyAddToQueue(track.uri).then(() => true));
    if (ok === true) notify("info", `Queued ${track.name}.`);
    return ok === true;
  }

  /** Ctrl+K "queue <song>": search and queue the best track match. */
  async function queueFromSearch(query: string): Promise<boolean> {
    const r = await attempt(() => api.spotifySearch(query, ["track"], 0));
    const best = r ? bestTrack(query, r.tracks) : null;
    if (!best) {
      notify("error", `No song on Spotify matches “${query}”.`);
      return false;
    }
    return addToQueue(best);
  }

  /** Add a track to one of the user's own playlists. */
  async function addToPlaylist(playlistId: string, playlistName: string, track: Pick<SpotifyTrack, "uri" | "name">): Promise<boolean> {
    const ok = await attempt(() => api.spotifyAddToPlaylist(playlistId, track.uri).then(() => true));
    if (ok === true) notify("info", `Added ${track.name} to ${playlistName}.`);
    return ok === true;
  }

  /** Like (save) a track to the library from search/results. */
  async function likeTrack(track: Pick<SpotifyTrack, "uri" | "name">): Promise<boolean> {
    const ok = await attempt(() => api.spotifySetSaved(track.uri, true).then(() => true));
    if (ok === true) notify("info", `Saved ${track.name} to your Liked Songs.`);
    return ok === true;
  }

  /** Seek the current Spotify track (the Now Playing bar, when Spotify is the player). */
  async function spotifySeek(positionMs: number) {
    const ok = await attempt(() => api.spotifySeek(Math.max(0, Math.round(positionMs))).then(() => true));
    if (ok === true) void refreshSpotifyPlayer();
  }

  /** Choose a "Play on" device and transfer playback to it (null = back to the iPhone). */
  async function chooseDevice(device: SpotifyDevice | null): Promise<boolean> {
    if (!device) {
      spotifyDevice.value = null;
      // Move what's playing back to the iPhone now, not just the next play (prefer the paired
      // phone by name among Spotify's smartphones).
      const devices = await api.spotifyDevices().catch(() => [] as SpotifyDevice[]);
      const phones = devices.filter((d) => d.kind.toLowerCase() === "smartphone");
      const name = status.value.device?.name?.toLowerCase();
      const phone = phones.find((d) => d.name.toLowerCase() === name) ?? phones[0];
      if (!phone) {
        notify("info", "Open Spotify on your iPhone to move the music there.");
        return true;
      }
      if (phone.isActive) {
        notify("info", "Playing on your iPhone.");
        return true;
      }
      const ok = await attempt(() => api.spotifyTransfer(phone.id).then(() => true));
      if (ok === true) {
        notify("info", "Playing on your iPhone.");
        void refreshSpotifyPlayer();
      }
      return ok === true;
    }
    const ok = await attempt(() => api.spotifyTransfer(device.id).then(() => true));
    if (ok === true) {
      spotifyDevice.value = device;
      notify("info", `Playing on ${device.name}.`);
      void refreshSpotifyPlayer();
    }
    return ok === true;
  }

  /** Open the Spotify panel on a given tab (Now Playing button, Ctrl+K "spotify"). */
  function openSpotifyPanel(tab: SpotifyTab = "search") {
    spotifyPanelTab.value = tab;
    spotifyPanelOpen.value = true;
  }

  async function refreshSpotifyPlayer() {
    if (!spotifyActive.value) return;
    // A transient read failure shouldn't nag; the next song or action reads again.
    try {
      spotifyPlayer.value = await api.spotifyPlayer();
    } catch {
      /* ignore */
    }
  }

  /** Cycle Spotify repeat (off → all → one → off), optimistic with revert on failure. */
  async function cycleSpotifyRepeat() {
    const p = spotifyPlayer.value;
    if (!p) return;
    const previous = p.repeat;
    const mode = nextRepeat(previous);
    p.repeat = mode;
    const ok = await attempt(() => api.spotifySetRepeat(mode).then(() => true));
    if (ok !== true && spotifyPlayer.value) spotifyPlayer.value.repeat = previous;
    else void refreshSpotifyPlayer();
  }

  async function toggleSpotifyShuffle() {
    const p = spotifyPlayer.value;
    if (!p) return;
    const previous = p.shuffle;
    p.shuffle = !previous;
    const ok = await attempt(() => api.spotifySetShuffle(!previous).then(() => true));
    if (ok !== true && spotifyPlayer.value) spotifyPlayer.value.shuffle = previous;
    else void refreshSpotifyPlayer();
  }

  async function toggleSpotifyLike() {
    const p = spotifyPlayer.value;
    // Never Like from a snapshot of another song (Spotify lagging the phone after a skip).
    if (!p?.trackUri || !spotifyTrackVerified.value) return;
    const previous = p.saved ?? false;
    const want = !previous;
    p.saved = want;
    const uri = p.trackUri;
    const ok = await attempt(() => api.spotifySetSaved(uri, want).then(() => true));
    if (ok !== true && spotifyPlayer.value) spotifyPlayer.value.saved = previous;
  }

  // No polling: Spotify rate-limits hard (testing hit a 19-hour timeout polling every 5 s, then
  // still while polling every 30 s). The iPhone already says when the song changes, so tug reads
  // Spotify's extras (art, Like, shuffle/repeat) once per song, and otherwise only when you act.
  // Spotify's API often lags the phone by about a second, so the read waits for the song to settle
  // (which also skips the songs you skip past), and is checked against the phone's title: a
  // snapshot of the previous song is read once more, and never shown as the current one.
  /** Wait after a song change before reading Spotify. */
  const SPOTIFY_SETTLE_MS = 2_500;
  /** Wait before the one re-read when Spotify still reports a different song. */
  const SPOTIFY_RECHECK_MS = 3_000;
  /** The snapshot is the song the phone is playing (so its art and Like belong to it). */
  const spotifyTrackVerified = computed(
    () => sameSong(spotifyPlayer.value?.trackName, nowPlaying.value.title) === true,
  );
  let spotifyReadFor = "";
  const songKey = () => `${nowPlaying.value.title}|${nowPlaying.value.artist}`;
  function cancelSongRead() {
    window.clearTimeout(spotifySongTimer);
    spotifySongTimer = undefined;
  }
  function refreshForSong() {
    cancelSongRead();
    if (!pageVisible.value || !spotifyActive.value) return;
    const key = songKey();
    // Already read for this song (e.g. window shown again), unless that read was of another song.
    if (key === spotifyReadFor && spotifyTrackVerified.value) return;
    spotifySongTimer = window.setTimeout(() => void readForSong(key, true), SPOTIFY_SETTLE_MS);
  }
  async function readForSong(key: string, recheck: boolean) {
    spotifySongTimer = undefined;
    if (key !== songKey() || !pageVisible.value || !spotifyActive.value) return;
    spotifyReadFor = key;
    await refreshSpotifyPlayer();
    // The song moved on while reading: its own settle timer takes over.
    if (key !== songKey() || !pageVisible.value || !spotifyActive.value) return;
    if (recheck && !spotifyTrackVerified.value) {
      spotifySongTimer = window.setTimeout(() => void readForSong(key, false), SPOTIFY_RECHECK_MS);
    }
  }
  watch(songKey, refreshForSong);
  watch(
    () => pageVisible.value && spotifyActive.value,
    (on) => {
      if (on) return refreshForSong();
      cancelSongRead();
      if (!spotifyActive.value) {
        spotifyPlayer.value = null;
        spotifyReadFor = "";
      }
    },
    { immediate: true },
  );

  return {
    status,
    statusKnown,
    nowPlaying,
    // Spotify connector
    spotify,
    spotifyPlayer,
    spotifyTrackVerified,
    spotifyActive,
    spotifyConnecting,
    playlists,
    spotifyPanelOpen,
    spotifyPanelTab,
    spotifyDevice,
    spotifyTargetName,
    loadSpotify,
    loadPlaylists,
    connectSpotify,
    disconnectSpotify,
    playPlaylist,
    playContext,
    playTrack,
    playFromSearch,
    addToQueue,
    queueFromSearch,
    addToPlaylist,
    likeTrack,
    spotifySeek,
    chooseDevice,
    openSpotifyPanel,
    refreshSpotifyPlayer,
    cycleSpotifyRepeat,
    toggleSpotifyShuffle,
    toggleSpotifyLike,
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
    contactPhoto,
    spotifyCoverFor,
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
    showConnect,
    connectSkipped,
    // What's new
    appVersion,
    whatsNewOpen,
    whatsNewEntries,
    openWhatsNew,
    focusItem,
    seen,
    connected,
    init,
    dispose,
    loadMore,
    searchAll,
    setSetting,
    toggleMuted,
    addVip,
    removeVip,
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
    codeFeed,
    clearCode,
    deleteConversation,
    appNameFor,
    /**
     * Send through the iPhone. The pending message appears via the `message` event. "failed" means
     * it's shown as "Not sent" with Retry (so the composer shouldn't keep the text too); "error"
     * means nothing was recorded, so the composer keeps the text.
     */
    async sendMessage(address: string, text: string): Promise<"sent" | "failed" | "error"> {
      try {
        const m = await api.sendMessage(address, text);
        if (m.status !== "failed") return "sent";
        notify("error", NOT_SENT);
        return "failed";
      } catch (e) {
        notify("error", errorMessage(e));
        return "error";
      }
    },
    /** Try a failed send again: the same message, to the number it was meant for. */
    async retryMessage(id: number): Promise<boolean> {
      try {
        const m = await api.retryMessage(id);
        if (m.status !== "failed") return true;
        notify("error", NOT_SENT);
      } catch (e) {
        notify("error", errorMessage(e));
      }
      return false;
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
    /**
     * Re-inquire for the iPhone (the find step calls this every ~15 s). An unpaired-Classic AEP
     * watcher only inquires once, so a phone made discoverable later never appears without this.
     * Quiet on failure — it's a background nicety, not something to toast about.
     */
    rescan: () => void api.rescanDiscovery().catch(() => undefined),
    /** Remove a leftover Windows pairing (both the LE and Classic bonds), then keep scanning. */
    removePairing: (id: string) => attempt(() => api.removePairing(id)),
    async pair(id: string) {
      const ok = await attempt(() => api.pairDevice(id).then(() => true));
      // The Connect panel shows this itself; a toast over it would just cover the screen.
      if (ok && !showConnect.value) notify("info", "Paired. Connecting to your iPhone…");
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
