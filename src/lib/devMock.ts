// Dev-only stand-in for the Rust backend, so the UI can be built and reviewed in
// a plain browser (`npm run dev`) without Bluetooth or an iPhone. Loaded from
// main.ts only when Tauri isn't present; never included in `tauri build`.
//
//   http://localhost:1420/            connected iPhone with sample history
//   http://localhost:1420/?setup      first run, nothing paired (scripted: pair, PIN, sharing, first notification)
//   http://localhost:1420/?setup&btoff   …starting with Bluetooth off
//   http://localhost:1420/?pairing    PIN confirmation dialog open
//   http://localhost:1420/?call       a call rings 1.5 s after load (rings out after 30 s, as a missed call)
//   http://localhost:1420/?nodial     Settings › iPhone › Calls check fails, like a blocked hands-free link
//   http://localhost:1420/?norepeat   player doesn't list AdvanceRepeatMode: no loop button
//   http://localhost:1420/?repeatignored   player lists it but ignores it: the "didn't change" toast

import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { CallRecord, Contact, DeviceStatus, DiscoveredDevice, NowPlaying, PhoneNotification, SmsMessage } from "../types/protocol";

const params = new URLSearchParams(location.search);
const setup = params.has("setup");
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

const history: PhoneNotification[] = setup
  ? []
  : [
      n("com.apple.mobilephone", "Phone", "Mum", "Missed Call", 3, { category: "missedCall", flags: flags({ positiveAction: true }), positiveLabel: "Dial" }),
      n("com.apple.MobileSMS", "Messages", "Zoe", "omw, 10 mins 🚗", 1),
      n("com.apple.MobileSMS", "Messages", "Zoe", "did you see the photos I sent?", 4),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Are we still meeting at 5? I can grab a table if you're running late.", 2),
      n("com.google.Gmail", "Gmail", "Google", "G-591204 is your Google verification code.", 6, { category: "email" }),
      n("net.whatsapp.WhatsApp", "WhatsApp", "Sam Okafor", "Sent you the slides, have a look before the call", 9),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Also bring the charger 🙏", 14),
      n("com.apple.mobilecal", "Calendar", "Design review", "In 15 minutes · Room 4", 18, { category: "schedule" }),
      n("com.hammerandchisel.discord", "Discord", "#release", "ci passed on main, tagging v0.4 now", 41),
      n("com.apple.mobilemail", "Mail", "Netlify", "Deploy succeeded for example-site", 66, { category: "email", removedAt: now - 30 * min, live: false }),
      n("net.whatsapp.WhatsApp", "WhatsApp", "Sam Okafor", "Running 5 late", 180, { live: false }),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Booked for Thursday", 60 * 26, { live: false }),
      n("com.apple.MobileSMS", "Messages", "Zoe", "lol yes that's exactly what I meant", 60 * 25, { live: false, removedAt: now - 60 * 24 * min }),
      n("com.apple.Health", null, "Stand", "Time to stand! Stand and move for a minute.", 60 * 27, { category: "healthAndFitness", live: false }),
      n("com.apple.MobileSMS", "Messages", "Bank", "Your code is 482913. Don't share it with anyone.", 60 * 50, { live: false }),
      // Unknown senders (iOS titles them with the number): they land under Messages › Unknown
      // senders, without a badge. The short code's text carries a code, so it would still pop up.
      n("com.apple.MobileSMS", "Messages", "‪+1 (555) 013-2244‬", "Congrats! You've been selected for a $500 gift card. Reply YES to claim before midnight.", 7),
      n("com.apple.MobileSMS", "Messages", "72975", "Your Acme verification code is 731904. It expires in 10 minutes.", 12),
    ];

// Mirrors the backend's reconnect sweep: anything no longer on the phone is cleared.
for (const x of history) if (!x.live && x.removedAt == null) x.removedAt = x.receivedAt + min;

const status: DeviceStatus = setup
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
      messagesError: null,
      contactsError: null,
      textsPairing: setup ? "missing" : params.has("textsbroken") ? "broken" : "ok",
      textsDevice: setup ? null : "Jordan's iPhone",
    }
  : {
      radio: "on",
      peripheralSupported: true,
      advertising: "on",
      device: { id: "mock", name: "Jordan's iPhone" },
      connection: "connected",
      battery: 76,
      services: { notifications: true, media: true, battery: true, messages: true },
      lastError: params.has("lasterror") ? "Couldn't advertise to the iPhone: the radio is busy" : null,
      lastErrorAt: params.has("lasterror") ? now - 4 * min : null,
      pairingStale: false,
      messagesError: null,
      contactsError: null,
      textsPairing: setup ? "missing" : params.has("textsbroken") ? "broken" : "ok",
      textsDevice: setup ? null : "Jordan's iPhone",
    };

const nowPlaying: NowPlaying = setup
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

// A connected keyboard listed first: setup must still offer only the iPhone.
const discovered: DiscoveredDevice[] = [
  { id: "k", name: "Keychron K3", transport: "le", paired: true, connected: true, canPair: false, kind: "accessory" },
  { id: "a", name: "Jordan's iPhone", transport: "le", paired: false, connected: true, canPair: true, kind: "phone" },
  { id: "b", name: "WH-1000XM5", transport: "classic", paired: true, connected: false, canPair: false, kind: "accessory" },
  { id: "c", name: "LE-Bose Flex", transport: "le", paired: false, connected: false, canPair: true, kind: "accessory" },
];

// seenSince 0: everything still on the phone counts as new, so badges show in the preview.
// Message access (MAP): Zoe's texts, including ones read in the open chat that
// never became notifications, plus a reply sent from tug.
const ZOE = "+13025550142";
const contacts: Contact[] = setup
  ? []
  : [
      { address: ZOE, name: "Zoe" },
      { address: "+12145550199", name: "Priya" },
      { address: "+19725550111", name: "Dave Smith" },
    ];
let nextMsg = 1;
const sms = (direction: "in" | "out", body: string, agoMin: number, address = ZOE, contactName: string | null = "Zoe"): SmsMessage => ({
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
const messages: SmsMessage[] = setup
  ? []
  : [
      sms("in", "are you coming tonight?", 40),
      sms("out", "yeah! leaving soon", 38),
      sms("in", "did you see the photos I sent?", 4),
      sms("in", "omw, 10 mins 🚗", 1),
      // The spammer's earlier text, read over MAP: same conversation as their notification.
      sms("in", "Final notice: your car warranty is about to expire. Call now.", 60 * 3, "+15550132244", null),
    ];

// Recents (PBAP call history): phone-local times, newest first, as the iPhone sends them.
const localIso = (agoMin: number) => {
  const d = new Date(now - agoMin * min);
  return new Date(d.getTime() - d.getTimezoneOffset() * min).toISOString().slice(0, 19);
};
const calls: CallRecord[] = setup
  ? []
  : [
      { direction: "missed", name: "Mum", number: "+19725550123", at: localIso(3) },
      { direction: "outgoing", name: "Zoe", number: ZOE, at: localIso(52) },
      { direction: "incoming", name: null, number: "+12145550199", at: localIso(130) },
      { direction: "missed", name: null, number: null, at: localIso(60 * 20) },
      { direction: "incoming", name: "Jane Doe", number: "+14695550188", at: localIso(60 * 26) },
      { direction: "outgoing", name: null, number: "+18005550100", at: localIso(60 * 50) },
    ];

const settings: Record<string, string> = { advertise: "true", "ui.toasts": "true", "ui.seenSince": "0" };
let autostart = false;

// ?setup: a scripted first run, so onboarding can be walked end to end in a browser.
// Pair → PIN → connected → the iPhone's three switches come on one by one → a first
// notification arrives.
let finishPairing: ((ok: boolean) => void) | null = null;
function simulateConnect(id: string) {
  const name = discovered.find((d) => d.id === id)?.name ?? "iPhone";
  const send = () => void emit("device-status", { ...status, services: { ...status.services } });
  status.device = { id, name };
  status.connection = "connecting";
  send();
  // The real order: connected, Share System Notifications switched on, then the Texts step's
  // Windows pairing (the phone asks for its switch), Show Notifications, Sync Contacts.
  setTimeout(() => {
    status.connection = "connected";
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
    status.textsDevice = "Jordan's iPhone";
    send();
  }, 8000);
  setTimeout(() => {
    status.messagesError = null;
    status.services = { ...status.services, messages: true };
    send();
  }, 10500);
  setTimeout(() => {
    contacts.push({ address: ZOE, name: "Zoe" }, { address: "+12145550199", name: "Priya" });
    void emit("contacts", [...contacts]);
  }, 12000);
  setTimeout(() => {
    const first = n("com.apple.MobileSMS", "Messages", "Zoe", "hey! is this thing on? 👋", 0);
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
        const to = String(a.address ?? ZOE);
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
      case "place_lookup":
        return JSON.stringify({ city: "Dallas", principalSubdivision: "Texas", countryCode: "US" });
      case "locate":
        return { latitude: 32.78, longitude: -96.8 };
      case "start_discovery":
        setTimeout(() => void emit("discovered-devices", discovered), 400);
        return null;
      case "pair_device":
        // Like Windows: the PIN shows on both screens; the call returns once it's answered.
        setTimeout(() => void emit("pairing-request", { deviceName: "Jordan's iPhone", pin: "482 913" }), 300);
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
  setTimeout(() => void emit("pairing-request", { deviceName: "Jordan's iPhone", pin: "482 913" }), 600);
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
