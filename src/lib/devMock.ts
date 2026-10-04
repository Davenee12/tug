// Dev-only stand-in for the Rust backend, so the UI can be built and reviewed in
// a plain browser (`npm run dev`) without Bluetooth or an iPhone. Loaded from
// main.ts only when Tauri isn't present; never included in `tauri build`.
//
//   http://localhost:1420/            connected iPhone with sample history
//   http://localhost:1420/?setup      first run, nothing paired
//   http://localhost:1420/?pairing    PIN confirmation dialog open

import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Contact, DeviceStatus, DiscoveredDevice, NowPlaying, PhoneNotification, SmsMessage } from "../types/protocol";

const params = new URLSearchParams(location.search);
const setup = params.has("setup");
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
      n("com.apple.mobilephone", "Phone", "Mum", "Incoming call", 0, {
        category: "incomingCall",
        flags: flags({ important: true, positiveAction: true }),
        positiveLabel: "Answer",
        negativeLabel: "Decline",
      }),
      n("com.apple.MobileSMS", "Messages", "Zoe", "omw, 10 mins 🚗", 1),
      n("com.apple.MobileSMS", "Messages", "Zoe", "did you see the photos I sent?", 4),
      n("com.apple.MobileSMS", "Messages", "Jane Doe", "Are we still meeting at 5? I can grab a table if you're running late.", 2),
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
      charging: null,
      services: { notifications: false, media: false, battery: false, messages: false },
      lastError: null,
      messagesError: null,
      contactsError: null,
    }
  : {
      radio: "on",
      peripheralSupported: true,
      advertising: "on",
      device: { id: "mock", name: "Jordan's iPhone" },
      connection: "connected",
      battery: 76,
      charging: true,
      services: { notifications: true, media: true, battery: true, messages: true },
      lastError: null,
      messagesError: null,
      contactsError: null,
    };

const nowPlaying: NowPlaying = setup
  ? { player: null, state: "unknown", rate: null, elapsed: null, volume: null, title: null, artist: null, album: null, duration: null, available: [] }
  : {
      player: "Spotify",
      state: "playing",
      rate: 1,
      elapsed: 74,
      volume: 0.6,
      title: "Teardrop",
      artist: "Massive Attack",
      album: "Mezzanine",
      duration: 330,
      available: ["play", "pause", "togglePlayPause", "nextTrack", "previousTrack", "volumeUp", "volumeDown"],
    };

const discovered: DiscoveredDevice[] = [
  { id: "a", name: "Jordan's iPhone", transport: "le", paired: false, connected: true, canPair: true },
  { id: "b", name: "WH-1000XM5", transport: "classic", paired: true, connected: false, canPair: false },
  { id: "c", name: "LE-Bose Flex", transport: "le", paired: false, connected: false, canPair: true },
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
const sms = (direction: "in" | "out", body: string, agoMin: number): SmsMessage => ({
  id: nextMsg++,
  source: "iphone-map",
  direction,
  address: ZOE,
  contactName: "Zoe",
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
    ];

const settings: Record<string, string> = { advertise: "true", "ui.toasts": "true", "ui.seenSince": "0" };

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
      case "list_messages":
        return messages;
      case "get_contacts":
        return contacts;
      case "send_message": {
        const m: SmsMessage = { ...sms("out", String(a.text), 0), status: "pending" };
        messages.push(m);
        setTimeout(() => void emit("message", m), 0);
        setTimeout(() => void emit("message", { ...m, status: "accepted" }), 700);
        return m;
      }
      case "get_settings":
        return settings;
      case "set_setting":
        settings[a.key as string] = a.value as string;
        return null;
      case "start_discovery":
        setTimeout(() => void emit("discovered-devices", discovered), 400);
        return null;
      case "perform_action":
        // Like the iPhone: a negative action (Clear/Decline) removes the notification.
        if (!a.positive) {
          const target = history.find((x) => x.id === a.id);
          if (target) target.removedAt = Date.now();
          setTimeout(() => void emit("notification-removed", a.id), 150);
        }
        return null;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

if (params.has("pairing")) {
  setTimeout(() => void emit("pairing-request", { deviceName: "Jordan's iPhone", pin: "482 913" }), 600);
}
