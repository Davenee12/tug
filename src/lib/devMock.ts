// Dev-only stand-in for the Rust backend, so the UI can be built and reviewed in
// a plain browser (`npm run dev`) without Bluetooth or an iPhone. Loaded from
// main.ts only when Tauri isn't present; never included in `tauri build`.
//
//   http://localhost:1420/            connected iPhone with sample history (Settings › iPhone shows
//                                     the Connect panel's paired state)
//   http://localhost:1420/?setup      first run, nothing paired: the Feed is replaced by the
//                                     "Connect your iPhone" panel (scripted: pair, code, Allow, the
//                                     switches tick green, it yields to the Feed, optional switches
//                                     then arrive as the Feed nudge, first notification lands)
//   http://localhost:1420/?leftover   fresh install, but Windows still has an iPhone paired from
//                                     before: the find step offers it as "paired before" with Use
//                                     and Remove (Remove unpairs it, then it reappears to pair fresh)
//   http://localhost:1420/?latephone  fresh install, no iPhone discoverable yet: it appears a rescan
//                                     later (the find step re-inquires every ~15 s)
//   http://localhost:1420/?forgotten  the iPhone forgot this PC while Windows still holds the bond:
//                                     the "Your iPhone has forgotten this PC" notice, with Remove
//   http://localhost:1420/?nudge      connected, notifications on, but texts/contacts off: the Feed's
//                                     dismissible "Get more from tug" nudge
//   http://localhost:1420/?setup&btoff   …starting with Bluetooth off
//   http://localhost:1420/?pairing    PIN confirmation dialog open
//   http://localhost:1420/?call       a call rings 1.5 s after load (rings out after 30 s, as a missed call)
//   http://localhost:1420/?nodial     Settings › iPhone › Calls check fails, like a blocked hands-free link
//   http://localhost:1420/?norepeat   player doesn't list AdvanceRepeatMode: no loop button
//   http://localhost:1420/?repeatignored   player lists it but ignores it: the "didn't change" toast

import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { CallRecord, Contact, DeviceStatus, DiscoveredDevice, NowPlaying, PhoneNotification, RepeatMode, SmsMessage, SpotifyAlbum, SpotifyArtist, SpotifyDevice, SpotifyPlayer, SpotifyPlaylist, SpotifyStatus, SpotifyTrack } from "../types/protocol";

const params = new URLSearchParams(location.search);
const setup = params.has("setup");
const leftover = params.has("leftover");
const latephone = params.has("latephone");
const forgotten = params.has("forgotten");
// Every first-run-like state: nothing (yet) connected, so the Feed is the Connect panel.
const noPhone = setup || leftover || latephone;
const noRepeat = params.has("norepeat");
const repeatIgnored = params.has("repeatignored");
const now = Date.now();
const min = 60_000;

const flags = (f: Partial<PhoneNotification["flags"]> = {}) => ({
  silent: false,
  important: false,
  preExisting: false,
  positiveAction: false,
  negativeAction: true,
  ...f,
});

let nextId = 100;
function n(appId: string, appName: string | null, title: string, message: string, agoMin: number, extra: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: nextId--,
    appId,
    appName,
    category: "social",
    title,
    subtitle: "",
    message,
    postedAt: null,
    receivedAt: now - agoMin * min,
    flags: flags(),
    positiveLabel: "",
    negativeLabel: "Clear",
    removedAt: null,
    live: true,
    ...extra,
  };
}

const history: PhoneNotification[] = noPhone
  ? []
  : [
      n("com.apple.mobilephone", "Phone", "Mum", "Missed Call", 3, { category: "missedCall", flags: flags({ positiveAction: true }), positiveLabel: "Dial" }),
      n("com.apple.MobileSMS", "Messages", "Tay", "omw, 10 mins 🚗", 1),
      n("com.apple.MobileSMS", "Messages", "Tay", "did you see the photos I sent?", 4),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Are we still meeting at 5? I can grab a table if you're running late.", 2),
      n("com.google.Gmail", "Gmail", "Google", "G-591204 is your Google verification code.", 6, { category: "email" }),
      n("net.whatsapp.WhatsApp", "WhatsApp", "Sam Okafor", "Sent you the slides, have a look before the call", 9),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Also bring the charger 🙏", 14),
      n("com.apple.mobilecal", "Calendar", "Design review", "In 15 minutes · Room 4", 18, { category: "schedule" }),
      n("com.hammerandchisel.discord", "Discord", "#release", "ci passed on main, tagging v0.4 now", 41),
      n("com.apple.mobilemail", "Mail", "Netlify", "Deploy succeeded for topcourt-prod", 66, { category: "email", removedAt: now - 30 * min, live: false }),
      n("net.whatsapp.WhatsApp", "WhatsApp", "Sam Okafor", "Running 5 late", 180, { live: false }),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Booked for Thursday", 60 * 26, { live: false }),
      n("com.apple.MobileSMS", "Messages", "Tay", "lol yes that's exactly what I meant", 60 * 25, { live: false, removedAt: now - 60 * 24 * min }),
      n("com.apple.Health", null, "Stand", "Time to stand! Stand and move for a minute.", 60 * 27, { category: "healthAndFitness", live: false }),
      n("com.apple.MobileSMS", "Messages", "Bank", "Your code is 482913. Don't share it with anyone.", 60 * 50, { live: false }),
      // Unknown senders (iOS titles them with the number): they land under Messages › Unknown
      // senders, without a badge. The short code's text carries a code, so it would still pop up.
      n("com.apple.MobileSMS", "Messages", "‪+1 (555) 013-2244‬", "Congrats! You've been selected for a $500 gift card. Reply YES to claim before midnight.", 7),
      n("com.apple.MobileSMS", "Messages", "72975", "Your Acme verification code is 731904. It expires in 10 minutes.", 12),
    ];

// Mirrors the backend's reconnect sweep: anything no longer on the phone is cleared.
for (const x of history) if (!x.live && x.removedAt == null) x.removedAt = x.receivedAt + min;

const status: DeviceStatus = noPhone
  ? {
      radio: "on",
      peripheralSupported: true,
      advertising: "on",
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
      contactsShared: false,
      textsPairing: "missing",
      textsDevice: null,
      liveTexts: "off",
    }
  : forgotten
  ? {
      // The bond lingers in Windows but the iPhone forgot this PC: disconnected, stale, no sharing.
      radio: "on",
      peripheralSupported: true,
      advertising: "on",
      device: { id: "mock", name: "Dave's iPhone" },
      connection: "disconnected",
      battery: null,
      services: { notifications: false, media: false, battery: false, messages: false },
      lastError: "Your iPhone isn't accepting this PC's pairing",
      lastErrorAt: now - min,
      pairingStale: true,
      awaitingPhoneAllow: false,
      messagesError: null,
      contactsError: null,
      contactsShared: false,
      textsPairing: "broken",
      textsDevice: "Dave's iPhone",
      liveTexts: "off",
    }
  : {
      radio: "on",
      peripheralSupported: true,
      advertising: "on",
      device: { id: "mock", name: "Dave's iPhone" },
      connection: "connected",
      battery: 76,
      // ?nudge: notifications work but the optional switches are off, so the Feed shows its
      // dismissible "Get more from tug" nudge (texts + names) without blocking the way in.
      services: { notifications: true, media: true, battery: true, messages: !params.has("nudge") },
      lastError: params.has("lasterror") ? "Couldn't advertise to the iPhone: the radio is busy" : null,
      lastErrorAt: params.has("lasterror") ? now - 4 * min : null,
      pairingStale: false,
      awaitingPhoneAllow: false,
      messagesError: params.has("nudge") ? "the iPhone refused message access; turn on Show Notifications for this PC" : null,
      contactsError: params.has("nudge") ? "the iPhone refused contact access" : null,
      contactsShared: false,
      textsPairing: setup ? "missing" : params.has("textsbroken") ? "broken" : "ok",
      textsDevice: setup ? null : "Dave's iPhone",
      liveTexts: params.has("livetexts") ? "active" : "off",
    };

const nowPlaying: NowPlaying = noPhone || forgotten
  ? { player: null, state: "unknown", rate: null, elapsed: null, elapsedAt: null, volume: null, title: null, artist: null, album: null, duration: null, repeat: null, available: [] }
  : {
      player: "Spotify",
      state: "playing",
      rate: 1,
      elapsed: 74,
      elapsedAt: now,
      volume: 0.6,
      title: "Teardrop",
      artist: "Massive Attack",
      album: "Mezzanine",
      duration: 330,
      repeat: "off",
      available: [
        "play",
        "pause",
        "togglePlayPause",
        "nextTrack",
        "previousTrack",
        "volumeUp",
        "volumeDown",
        ...(noRepeat ? [] : (["advanceRepeatMode"] as const)),
      ],
    };

// A connected keyboard listed first: the Connect panel must still offer only iPhones. In ?setup the
// iPhone arrives nameless (shown as "iPhone" right away, named in place a moment later), alongside an
// old bond for another phone that belongs under "Paired before".
const discovered: DiscoveredDevice[] = setup
  ? [
      { id: "k", name: "Keychron K3", transport: "le", paired: true, connected: true, canPair: false, kind: "accessory" },
      { id: "a", name: "", transport: "classic", paired: false, connected: false, canPair: true, kind: "phone" },
      { id: "old", name: "DTD iPhone Max 15 Pro", transport: "classic", paired: true, connected: false, canPair: false, kind: "phone" },
    ]
  : leftover
  ? // A fresh install with both bonds of a previous iPhone still paired in Windows (same name).
    [
      { id: "k", name: "Keychron K3", transport: "le", paired: true, connected: true, canPair: false, kind: "accessory" },
      { id: "le", name: "Dave's iPhone", transport: "le", paired: true, connected: false, canPair: false, kind: "phone" },
      { id: "classic", name: "Dave's iPhone", transport: "classic", paired: true, connected: false, canPair: false, kind: "phone" },
    ]
  : latephone
  ? // The iPhone isn't discoverable yet; it appears after a rescan (see rescan_discovery below).
    [{ id: "k", name: "Keychron K3", transport: "le", paired: true, connected: true, canPair: false, kind: "accessory" }]
  : [
      { id: "k", name: "Keychron K3", transport: "le", paired: true, connected: true, canPair: false, kind: "accessory" },
      { id: "a", name: "Dave's iPhone", transport: "le", paired: false, connected: true, canPair: true, kind: "phone" },
      { id: "b", name: "WH-1000XM5", transport: "classic", paired: true, connected: false, canPair: false, kind: "accessory" },
      { id: "c", name: "LE-Bose Flex", transport: "le", paired: false, connected: false, canPair: true, kind: "accessory" },
    ];
// The same discovery list once the iPhone's name has filled in (emitted a couple of seconds later).
const namedLater: DiscoveredDevice[] | null = setup
  ? discovered.map((d) => (d.id === "a" ? { ...d, name: "Dave's iPhone" } : d))
  : null;

// seenSince 0: everything still on the phone counts as new, so badges show in the preview.
// Message access (MAP): Tay's texts, including ones read in the open chat that
// never became notifications, plus a reply sent from tug.
const TAY = "+13025550142";
const contacts: Contact[] = noPhone
  ? []
  : [
      { address: TAY, name: "Tay" },
      { address: "+12145550199", name: "Daviel" },
      { address: "+19725550111", name: "Dave Smith" },
    ];
let nextMsg = 1;
const sms = (direction: "in" | "out", body: string, agoMin: number, address = TAY, contactName: string | null = "Tay"): SmsMessage => ({
  id: nextMsg++,
  source: "iphone-map",
  direction,
  address,
  contactName,
  body,
  sentAt: null,
  receivedAt: now - agoMin * min,
  status: direction === "in" ? "received" : "accepted",
});
const messages: SmsMessage[] = noPhone
  ? []
  : [
      sms("in", "are you coming tonight?", 40),
      sms("out", "yeah! leaving soon", 38),
      sms("in", "did you see the photos I sent?", 4),
      sms("in", "omw, 10 mins 🚗", 1),
      // The spammer's earlier text, read over MAP: same conversation as their notification.
      sms("in", "Final notice: your car warranty is about to expire. Call now.", 60 * 3, "+15550132244", null),
      // A code that arrived as a text with no notification (the iPhone showed no banner): it shows
      // in the Feed with Copy code, even though the short code is an unknown sender.
      sms("in", "480579 is your Amazon OTP. Do not share it with anyone.", 2, "98626", null),
    ];

// Recents (PBAP call history): phone-local times, newest first, as the iPhone sends them.
const localIso = (agoMin: number) => {
  const d = new Date(now - agoMin * min);
  return new Date(d.getTime() - d.getTimezoneOffset() * min).toISOString().slice(0, 19);
};
const calls: CallRecord[] = noPhone
  ? []
  : [
      { direction: "missed", name: "Mum", number: "+19725550123", at: localIso(3) },
      { direction: "outgoing", name: "Tay", number: TAY, at: localIso(52) },
      { direction: "incoming", name: null, number: "+12145550199", at: localIso(130) },
      { direction: "missed", name: null, number: null, at: localIso(60 * 20) },
      { direction: "incoming", name: "Jane Doe", number: "+14695550188", at: localIso(60 * 26) },
      { direction: "outgoing", name: null, number: "+18005550100", at: localIso(60 * 50) },
    ];

const settings: Record<string, string> = { advertise: "true", "ui.toasts": "true", "ui.seenSince": "0" };
let autostart = false;

// Spotify connector: connected by default so its UI can be reviewed in the browser; ?spotifyoff
// starts it disconnected (to see Settings › Connectors and the empty states).
const spotifyState: SpotifyStatus = {
  connected: !noPhone && !params.has("spotifyoff"),
  account: !noPhone && !params.has("spotifyoff") ? "Dave James" : null,
};
const artSvg =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="6" fill="#1db954"/><circle cx="24" cy="44" r="7" fill="#0b2e18"/><rect x="29" y="18" width="6" height="26" fill="#0b2e18"/><path d="M35 18 L52 14 V22 L35 26 Z" fill="#0b2e18"/></svg>';
const spotifyPlayer: SpotifyPlayer = {
  isPlaying: true,
  shuffle: false,
  repeat: "off",
  saved: false,
  albumArt: `data:image/svg+xml;base64,${btoa(artSvg)}`,
  trackUri: "spotify:track:mock123",
  deviceName: "Dave's iPhone",
};
const img = (seed: string) => `https://i.scdn.co/mock/${encodeURIComponent(seed)}`;
const spotifyPlaylists: SpotifyPlaylist[] = [
  { uri: "spotify:playlist:1", id: "1", name: "Deep Focus", owner: "Spotify", trackCount: 120, imageUrl: img("Deep Focus"), owned: false },
  { uri: "spotify:playlist:2", id: "2", name: "Morning Run", owner: "Dave", trackCount: 42, imageUrl: img("Morning Run"), owned: true },
  { uri: "spotify:playlist:3", id: "3", name: "Discover Weekly", owner: "Spotify", trackCount: 30, imageUrl: img("Discover Weekly"), owned: false },
  { uri: "spotify:playlist:4", id: "4", name: "Coding Flow", owner: "Dave", trackCount: 88, imageUrl: img("Coding Flow"), owned: true },
  { uri: "spotify:playlist:5", id: "5", name: "Rainy Day Jazz", owner: "Dave", trackCount: 61, imageUrl: img("Rainy Day Jazz"), owned: true },
];

// A small catalogue so every panel tab/view renders in the browser. Searching filters it by name.
const mockArtist = (id: string, name: string): SpotifyArtist => ({ uri: `spotify:artist:${id}`, id, name, imageUrl: img(`artist ${name}`) });
const mockAlbum = (id: string, name: string, artists: string, year: string, total: number): SpotifyAlbum => ({
  uri: `spotify:album:${id}`,
  id,
  name,
  artists,
  imageUrl: img(`album ${name}`),
  totalTracks: total,
  year,
});
const mockTrack = (id: string, name: string, artists: string, album: string, ms: number, albumId = id, artistId = id): SpotifyTrack => ({
  uri: `spotify:track:${id}`,
  name,
  artists,
  artistUri: `spotify:artist:${artistId}`,
  album,
  albumUri: `spotify:album:${albumId}`,
  imageUrl: img(`album ${album}`),
  durationMs: ms,
});

const catalogueTracks: SpotifyTrack[] = [
  mockTrack("t1", "Teardrop", "Massive Attack", "Mezzanine", 330000, "mz", "ma"),
  mockTrack("t2", "Angel", "Massive Attack", "Mezzanine", 379000, "mz", "ma"),
  mockTrack("t3", "Midnight City", "M83", "Hurry Up, We're Dreaming", 244000, "m83", "m83a"),
  mockTrack("t4", "Nightcall", "Kavinsky", "OutRun", 258000, "or", "kav"),
  mockTrack("t5", "Redbone", "Childish Gambino", "Awaken, My Love!", 327000, "aml", "cg"),
  mockTrack("t6", "Flume", "Bon Iver", "For Emma, Forever Ago", 199000, "fe", "bi"),
  mockTrack("t7", "Runaway", "Kanye West", "My Beautiful Dark Twisted Fantasy", 548000, "mbdtf", "kw"),
  mockTrack("t8", "Teardrops", "Bring Me The Horizon", "Post Human", 210000, "ph", "bmth"),
];
const catalogueAlbums: SpotifyAlbum[] = [
  mockAlbum("mz", "Mezzanine", "Massive Attack", "1998", 11),
  mockAlbum("m83", "Hurry Up, We're Dreaming", "M83", "2011", 22),
  mockAlbum("aml", "Awaken, My Love!", "Childish Gambino", "2016", 11),
];
const catalogueArtists: SpotifyArtist[] = [
  mockArtist("ma", "Massive Attack"),
  mockArtist("m83a", "M83"),
  mockArtist("cg", "Childish Gambino"),
];
const spotifyDevices: SpotifyDevice[] = [
  { id: "phone", name: "Dave's iPhone", kind: "Smartphone", isActive: true },
  { id: "pc", name: "Spotify on this PC", kind: "Computer", isActive: false },
  { id: "spk", name: "Kitchen speaker", kind: "Speaker", isActive: false },
];

// ?setup: a scripted first run, so onboarding can be walked end to end in a browser.
// Pair → PIN → connected → the iPhone's three switches come on one by one → a first
// notification arrives.
let finishPairing: ((ok: boolean) => void) | null = null;
function simulateConnect(id: string) {
  const name = (namedLater ?? discovered).find((d) => d.id === id)?.name || "Dave's iPhone";
  const send = () => void emit("device-status", { ...status, services: { ...status.services } });
  status.device = { id, name };
  status.connection = "connecting";
  // On a fresh bond iOS holds the ANCS subscribe open until "Allow" is tapped: the UI should
  // prompt to look at the phone during this window.
  status.awaitingPhoneAllow = true;
  send();
  // The real order: connected, Share System Notifications switched on, then the Texts step's
  // Windows pairing (the phone asks for its switch), Show Notifications, Sync Contacts.
  setTimeout(() => {
    status.connection = "connected";
    status.awaitingPhoneAllow = false;
    status.battery = 76;
    status.services = { ...status.services, media: true, battery: true };
    send();
  }, 900);
  setTimeout(() => {
    status.services = { ...status.services, notifications: true };
    send();
  }, 2400);
  setTimeout(() => {
    status.messagesError = "the iPhone refused message access; turn on Show Notifications for this PC";
    status.textsPairing = "ok";
    status.textsDevice = "Dave's iPhone";
    send();
  }, 8000);
  setTimeout(() => {
    status.messagesError = null;
    status.services = { ...status.services, messages: true };
    send();
  }, 10500);
  setTimeout(() => {
    contacts.push({ address: TAY, name: "Tay" }, { address: "+12145550199", name: "Daviel" });
    void emit("contacts", [...contacts]);
  }, 12000);
  setTimeout(() => {
    const first = n("com.apple.MobileSMS", "Messages", "Tay", "hey! is this thing on? 👋", 0);
    first.id = 500;
    first.receivedAt = Date.now();
    history.unshift(first);
    void emit("notification", first);
  }, 16000);
}

mockIPC(
  (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "get_status":
        return status;
      case "get_now_playing":
        return nowPlaying;
      case "list_notifications": {
        const before = (a.beforeId as number | null) ?? Infinity;
        return history.filter((x) => x.id < before).slice(0, a.limit as number);
      }
      case "search_notifications": {
        const q = String(a.query).toLowerCase();
        return history.filter((x) => [x.title, x.message, x.appName ?? ""].some((s) => s.toLowerCase().includes(q)));
      }
      case "search_all": {
        const q = String(a.query).toLowerCase().trim();
        const words = q.split(/\s+/).filter(Boolean);
        const hit = (t: string) => words.every((w) => t.toLowerCase().split(/\W+/).some((x) => x.startsWith(w)));
        return {
          people: contacts.filter((c) => c.name.toLowerCase().includes(q)),
          messages: messages.filter((m) => hit(m.body)).reverse(),
          notifications: history.filter((x) => hit(`${x.title} ${x.message}`)),
        };
      }
      case "list_messages":
        return messages;
      case "get_calls":
        return calls;
      // Experimental hands-free dialing: works here, unless ?nodial shows the refusal.
      case "dial":
        return new Promise((resolve, reject) =>
          setTimeout(
            () =>
              params.has("nodial")
                ? reject("Windows won't share the iPhone's hands-free link with tug (Windows or Phone Link is probably using it)")
                : resolve(null),
            900,
          ),
        );
      case "get_contacts":
        return contacts;
      case "send_message": {
        const to = String(a.address ?? TAY);
        const m: SmsMessage = { ...sms("out", String(a.text), 0, to, contacts.find((c) => c.address === to)?.name ?? null), status: "pending" };
        messages.push(m);
        setTimeout(() => void emit("message", m), 0);
        setTimeout(() => void emit("message", { ...m, status: "accepted" }), 700);
        return m;
      }
      // A stand-in report so Settings › Copy diagnostics works in the browser. The real one is
      // built and redacted in Rust (src-tauri/src/diagnostics.rs).
      case "copy_diagnostics": {
        const report = [
          "tug diagnostics",
          "===============",
          "",
          "app version:     0.5.7 (dev mock)",
          "windows version: Microsoft Windows [Version 10.0.26200.0000]",
          `bluetooth:       ${status.radio === "on" ? "radio on" : "radio off"}, peripheral role supported`,
          "",
          "device status",
          "-------------",
          JSON.stringify({ ...status, device: status.device, textsDevice: status.textsDevice }, null, 2),
          "",
          "settings",
          "--------",
          ...Object.entries(settings).map(([k, v]) => `${k}: ${v}`),
          "",
          "recent log (2 lines)",
          "----------",
          "2026-10-05T10:11:12 [INFO] connected to [number]",
          "2026-10-05T10:11:13 [INFO] message from [number] to [email] saved",
        ].join("\n");
        void navigator.clipboard?.writeText(report).catch(() => undefined);
        return report;
      }
      case "open_logs_folder":
        return null;
      case "get_settings":
        return settings;
      case "set_setting":
        settings[a.key as string] = a.value as string;
        return null;
      case "get_autostart":
        return autostart;
      case "set_autostart":
        autostart = a.enabled as boolean;
        return null;
      case "app_icon": {
        // Stand-in icons (the real ones come from the App Store): a coloured tile per app.
        const id = String(a.appId ?? "");
        const hue = [...id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
        const letter = (id.split(".").pop() ?? "?")[0].toUpperCase();
        const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="hsl(${hue} 65% 52%)"/><text x="32" y="43" font-family="Segoe UI" font-size="30" font-weight="700" fill="white" text-anchor="middle">${letter}</text></svg>`;
        return `data:image/svg+xml;base64,${btoa(svg)}`;
      }
      case "media_command":
        if (a.command === "advanceRepeatMode" && !repeatIgnored) {
          nowPlaying.repeat = nowPlaying.repeat === "off" ? "all" : nowPlaying.repeat === "all" ? "one" : "off";
          void emit("now-playing", { ...nowPlaying });
        }
        if (a.command === "previousTrack") {
          Object.assign(nowPlaying, { elapsed: 0, elapsedAt: Date.now() });
          void emit("now-playing", { ...nowPlaying });
        }
        // One AMS VolumeUp/VolumeDown is one phone step; iOS reports volume as a 0–1 fraction,
        // ~16 steps. Clamp at the ends so press-and-hold stops there, as it would on hardware.
        if (a.command === "volumeUp" || a.command === "volumeDown") {
          const step = (a.command === "volumeUp" ? 1 : -1) / 16;
          nowPlaying.volume = Math.min(1, Math.max(0, (nowPlaying.volume ?? 0.5) + step));
          void emit("now-playing", { ...nowPlaying });
        }
        return null;
      case "open_url":
        // No browser launch in dev; just show what the real backend would open.
        console.log("[devMock] open_url", a.url);
        return null;
      case "app_website":
        return "https://www.example.com/";
      case "contact_photo": {
        // Stand-in contact photos so the avatar path is visible without a phone: a couple of mock
        // people have one, the rest fall back to initials (as iOS contacts without a photo would).
        const key = String(a.key ?? "");
        const withPhoto = new Set([TAY, "Tay", "+19725550111", "Dave Smith", "+19725550123", "Mum"]);
        if (!withPhoto.has(key)) return null;
        const hue = [...key].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
        const svg =
          `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">` +
          `<rect width="64" height="64" fill="hsl(${hue} 55% 60%)"/>` +
          `<circle cx="32" cy="25" r="12" fill="hsl(${hue} 45% 88%)"/>` +
          `<path d="M12 60c0-13 9-20 20-20s20 7 20 20z" fill="hsl(${hue} 45% 88%)"/></svg>`;
        return `data:image/svg+xml;base64,${btoa(svg)}`;
      }
      case "place_lookup":
        return JSON.stringify({ city: "Dallas", principalSubdivision: "Texas", countryCode: "US" });
      case "locate":
        // A real fix takes a few seconds; long enough to see tug's "finding you" lines.
        return new Promise((resolve) => setTimeout(() => resolve({ latitude: 32.78, longitude: -96.8 }), 4500));
      case "start_discovery":
        setTimeout(() => void emit("discovered-devices", [...discovered]), 400);
        // The iPhone's name arrives a little after it's first seen; the row updates in place.
        if (namedLater) setTimeout(() => void emit("discovered-devices", namedLater), 2600);
        return null;
      case "stop_discovery":
        return null;
      case "rescan_discovery":
        // ?latephone: the iPhone becomes discoverable only on a later inquiry; surface it now.
        if (latephone && !discovered.some((d) => d.kind === "phone")) {
          discovered.push({ id: "late", name: "Dave's iPhone", transport: "classic", paired: false, connected: false, canPair: true, kind: "phone" });
        }
        setTimeout(() => void emit("discovered-devices", [...discovered]), 200);
        return null;
      case "remove_pairing": {
        // Unpair both bonds (same name), then the phone reappears unpaired, ready to pair fresh.
        const name = discovered.find((d) => d.id === a.id)?.name;
        for (let i = discovered.length - 1; i >= 0; i--) {
          if (discovered[i].kind === "phone" && discovered[i].paired && discovered[i].name === name) discovered.splice(i, 1);
        }
        void emit("discovered-devices", [...discovered]);
        setTimeout(() => {
          discovered.push({ id: "fresh", name: name ?? "Dave's iPhone", transport: "classic", paired: false, connected: false, canPair: true, kind: "phone" });
          void emit("discovered-devices", [...discovered]);
        }, 1200);
        return null;
      }
      case "forget_device":
        // Start over / Remove: clear the device and drop back to the find step.
        status.device = null;
        status.connection = "noDevice";
        status.pairingStale = false;
        status.textsPairing = "missing";
        status.textsDevice = null;
        void emit("device-status", { ...status, services: { ...status.services } });
        return null;
      case "pair_device":
        // Like Windows: the PIN shows on both screens; the call returns once it's answered.
        setTimeout(() => void emit("pairing-request", { deviceName: "Dave's iPhone", pin: "482 913", confirmOnPhone: false }), 300);
        return new Promise<null>((resolve, reject) => {
          finishPairing = (ok) => (ok ? resolve(null) : reject("Pairing was cancelled"));
        }).then(() => {
          simulateConnect(String(a.id));
          return null;
        });
      case "confirm_pairing":
        setTimeout(() => void emit("pairing-request-closed", null), 0);
        finishPairing?.(Boolean(a.accept));
        finishPairing = null;
        return null;
      case "use_device":
        simulateConnect(String(a.id));
        return null;
      case "perform_action": {
        // Like the iPhone: a negative action (Clear/Decline) removes the notification, and so
        // does answering a call (it stops ringing).
        const target = history.find((x) => x.id === a.id);
        if (!a.positive || target?.category === "incomingCall") {
          if (target) target.removedAt = Date.now();
          setTimeout(() => void emit("notification-removed", a.id), 150);
        }
        return null;
      }
      // Windows pop-ups don't exist in a browser; log what tug would have shown.
      case "show_toast":
        console.info("[devMock] pop-up", a.spec);
        return null;
      // --- Spotify connector ---
      // Return fresh copies (the real Rust commands serialize new objects each call), so Vue
      // refs see an identity change and re-render. Returning the shared object wouldn't.
      case "spotify_status":
        return { ...spotifyState };
      case "spotify_connect":
        return new Promise((resolve) =>
          setTimeout(() => {
            spotifyState.connected = true;
            spotifyState.account = "Dave James";
            resolve({ ...spotifyState });
          }, 900),
        );
      case "spotify_disconnect":
        spotifyState.connected = false;
        spotifyState.account = null;
        return { ...spotifyState };
      case "spotify_playlists":
        return spotifyState.connected ? spotifyPlaylists.map((p) => ({ ...p })) : [];
      case "spotify_cover": {
        // Stand-in cover art (the real covers come from Spotify's CDN): a coloured tile per URL.
        const url = String(a.url ?? "");
        const hue = [...url].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
        const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="hsl(${hue} 55% 45%)"/><circle cx="32" cy="32" r="10" fill="hsl(${hue} 60% 80%)"/></svg>`;
        return `data:image/svg+xml;base64,${btoa(svg)}`;
      }
      case "spotify_play_context":
        console.log("[devMock] spotify play context", a.uri, "on", a.deviceId ?? "iPhone");
        return null;
      case "spotify_play_track":
        console.log("[devMock] spotify play track", a.uri, "in", a.contextUri, "on", a.deviceId ?? "iPhone");
        return null;
      case "spotify_player":
        return spotifyState.connected ? { ...spotifyPlayer } : null;
      case "spotify_set_repeat":
        spotifyPlayer.repeat = a.mode as RepeatMode;
        return null;
      case "spotify_set_shuffle":
        spotifyPlayer.shuffle = Boolean(a.on);
        return null;
      case "spotify_set_saved":
        spotifyPlayer.saved = Boolean(a.saved);
        return null;
      case "spotify_search": {
        const q = String(a.query ?? "").toLowerCase().trim();
        const kinds = (a.kinds as string[]) ?? [];
        const offset = Number(a.offset ?? 0);
        const want = (k: string) => kinds.length === 0 || kinds.includes(k);
        const hitT = (t: SpotifyTrack) => `${t.name} ${t.artists}`.toLowerCase().includes(q);
        const hitA = (al: SpotifyAlbum) => `${al.name} ${al.artists}`.toLowerCase().includes(q);
        const hitAr = (ar: SpotifyArtist) => ar.name.toLowerCase().includes(q);
        const hitP = (p: SpotifyPlaylist) => p.name.toLowerCase().includes(q);
        // Pad the track list so "Show more" can be exercised past the first page of 10.
        const allTracks = q ? [...catalogueTracks.filter(hitT), ...catalogueTracks.map((t, i) => ({ ...t, uri: `${t.uri}:${i}:${q}` }))] : [];
        const page = <T>(xs: T[]) => xs.slice(offset, offset + 10);
        const tracks = want("track") ? page(allTracks) : [];
        const albums = want("album") ? page(q ? catalogueAlbums.filter(hitA) : []) : [];
        const artists = want("artist") ? page(q ? catalogueArtists.filter(hitAr) : []) : [];
        const playlists = want("playlist") ? page(q ? spotifyPlaylists.filter(hitP) : []) : [];
        return {
          tracks,
          albums,
          artists,
          playlists,
          more: {
            tracks: want("track") && offset + 10 < allTracks.length,
            albums: false,
            artists: false,
            playlists: false,
          },
        };
      }
      case "spotify_queue":
        return { currentlyPlaying: catalogueTracks[0], queue: catalogueTracks.slice(1, 6) };
      case "spotify_add_to_queue":
        console.log("[devMock] spotify queue", a.uri);
        return null;
      case "spotify_recently_played":
        return catalogueTracks.slice(2, 8);
      case "spotify_top_tracks":
        return catalogueTracks.slice(0, 6);
      case "spotify_top_artists":
        return catalogueArtists;
      case "spotify_devices":
        return spotifyDevices;
      case "spotify_transfer":
        console.log("[devMock] spotify transfer to", a.deviceId);
        return null;
      case "spotify_seek":
        nowPlaying.elapsed = Math.round(Number(a.positionMs ?? 0) / 1000);
        nowPlaying.elapsedAt = Date.now();
        void emit("now-playing", { ...nowPlaying });
        return null;
      case "spotify_album": {
        const album = catalogueAlbums.find((al) => al.id === a.id) ?? mockAlbum(String(a.id), "Album", "Various", "2020", 3);
        return { album, tracks: catalogueTracks.slice(0, 4).map((t) => ({ ...t, albumUri: album.uri, album: album.name })) };
      }
      case "spotify_artist": {
        const artist = catalogueArtists.find((ar) => ar.id === a.id) ?? mockArtist(String(a.id), "Artist");
        return { artist, albums: catalogueAlbums };
      }
      case "spotify_playlist_items": {
        const p = spotifyPlaylists.find((x) => x.id === a.id);
        return p?.owned ? catalogueTracks.slice(0, 6) : [];
      }
      case "spotify_add_to_playlist":
        console.log("[devMock] spotify add to playlist", a.playlistId, a.trackUri);
        return null;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

// ?setup&btoff: Bluetooth starts off and comes on after a few seconds (setup's first check).
if (setup && params.has("btoff")) {
  status.radio = "off";
  status.advertising = "off";
  setTimeout(() => {
    status.radio = "on";
    status.advertising = "on";
    void emit("device-status", { ...status });
  }, 20000);
}

if (params.has("pairing")) {
  setTimeout(() => void emit("pairing-request", { deviceName: "Dave's iPhone", pin: "482 913", confirmOnPhone: false }), 600);
}

// ?call: the phone rings. Answer or Decline takes it down (perform_action above); left alone,
// it rings out after 30 s and comes back as a missed call, the way iOS reports it over ANCS.
if (params.has("call")) {
  const ring = n("com.apple.mobilephone", "Phone", "Jane Doe", "Incoming Call", 0, {
    category: "incomingCall",
    subtitle: "mobile",
    flags: flags({ important: true, positiveAction: true }),
    positiveLabel: "Answer",
    negativeLabel: "Decline",
  });
  ring.id = 600;
  setTimeout(() => {
    ring.receivedAt = Date.now();
    history.unshift(ring);
    void emit("notification", ring);
  }, 1500);
  setTimeout(() => {
    if (ring.removedAt != null) return;
    ring.removedAt = Date.now();
    void emit("notification-removed", ring.id);
    const missed = n("com.apple.mobilephone", "Phone", "Jane Doe", "Missed Call", 0, { category: "missedCall", flags: flags({ positiveAction: true }), positiveLabel: "Dial" });
    missed.id = 601;
    missed.receivedAt = Date.now();
    history.unshift(missed);
    void emit("notification", missed);
  }, 31500);
}
