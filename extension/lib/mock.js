// Made-up answers for previewing the popup without tug: open popup.html?mock=<state> (in the
// extension, or served by any static server for screenshots). Used by nothing else.

import { TugError } from "./tug.js";

const phone = {
  connected: true,
  connection: "connected",
  model: "iPhone 15 Pro",
  battery_percent: 64,
  notifications: true,
  texts: true,
  now_playing: { title: "Harbour Lights", artist: "The Night Ferries", album: "Low Tide", state: "playing", app: "Spotify" },
};
const permissions = {
  codes: true, search: true, dev_notifications: true, tugboat_files: true,
  phone_status: true, media: true, send_text: true, tugboat_send: true,
};
const code = { code: "482913", from: "Chase", app: "Messages", received_at: "", age_seconds: 120, copied: false };
const minsAgo = (m) => new Date(Date.now() - m * 60_000).toISOString();
const hits = [
  { kind: "text", from: "Maya Chen", app: "Messages", time: minsAgo(12), text: "Dinner at 7 still good? I booked the place by the water." },
  { kind: "text", from: "You", app: "Messages", time: minsAgo(10), text: "Perfect, see you at dinner!" },
  { kind: "notification", from: "OpenTable", app: "OpenTable", time: minsAgo(26 * 60), text: "Your dinner reservation for 2 is confirmed." },
  { kind: "text", from: "Sam Ortiz", app: "Messages", time: minsAgo(4 * 24 * 60), text: "Dinner Sunday at mom's, bring the pie" },
];

const contacts = [
  { name: "Maya Chen", recent: true },
  { name: "Sam Ortiz", recent: true },
  { name: "Mom", recent: true },
  ...["Alex Kim", "Ana Lima", "Arlo Diaz", "Ben Carter", "Chloe Martin", "Dad", "Dev Patel", "Elena Rossi",
    "Grace Wong", "Hannah Lee", "Isaac Brown", "Jordan Smith", "Leo Garcia", "Nora Quinn", "Priya Shah",
    "Sam Lee", "Theo Walsh", "Zoe Park"].map((name) => ({ name, recent: false })),
];

const STATES = {
  connected: {},
  copied: {},
  search: {},
  old: { code: { ...code, age_seconds: 7 * 60 } },
  nocode: { code: new TugError("not_found", "No verification code in the last 10 minutes.") },
  paused: { phone: { ...phone, now_playing: { ...phone.now_playing, state: "paused" } } },
  connecting: {
    phone: { ...phone, connected: false, connection: "connecting", battery_percent: null, now_playing: null },
    code: new TugError("not_found", "No verification code in the last 10 minutes."),
  },
  disconnected: {
    phone: { ...phone, connected: false, connection: "disconnected", battery_percent: null, now_playing: null },
    code: new TugError("not_found", "No verification code in the last 10 minutes."),
  },
  codesoff: { code: new TugError("tool_off", "Verification codes are switched off in tug › Settings › Developer tools.") },
  notrunning: { all: new TugError("not_running", "tug isn't running. Open tug and try again.") },
  off: { all: new TugError("off", "Let AI tools use tug is off.") },
  notinstalled: { all: new TugError("not_installed", "Specified native messaging host not found.") },
  wrongid: { all: new TugError("wrong_extension", "Access to the specified native messaging host is forbidden.") },
  compose: {},
  textsent: {},
  textwaiting: {},
  copyfail: { copyFails: true },
  textsnow: { sendsNow: true },
  textsoff: { permissions: { ...permissions, send_text: false } },
};

/** The last text as the service worker would have kept it, for the states that show one. */
export function mockLastText(name) {
  const at = Date.now() - 30_000;
  if (name === "textsent") return { state: "done", to: "Maya Chen", at, result: { outcome: "sent", to: "Maya Chen", detail: "" } };
  if (name === "textwaiting") return { state: "waiting", to: "Maya Chen", at };
  return null;
}

export function installMock(name) {
  const s = STATES[name] ?? STATES.connected;
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  globalThis.__tugMock = async (call) => {
    await wait(name === "loading" ? 1e9 : 60);
    if (s.all) throw s.all;
    switch (call.tool) {
      case "status":
        return {
          version: "0.5.24",
          permissions: s.permissions ?? permissions,
          phone: s.phone ?? phone,
          ...(s.sendsNow ? { sends_without_asking: true } : {}),
        };
      case "get_latest_code": {
        if (s.code instanceof Error) throw s.code;
        return { ...(s.code ?? code), copied: call.copy && !s.copyFails };
      }
      case "search_messages":
        // tug answers with the hits themselves, newest first.
        return hits.filter((h) => (h.text + h.from).toLowerCase().includes(call.query.toLowerCase()));
      case "media_control":
        return { ok: true };
      case "list_contacts":
        if (s.permissions?.send_text === false) throw new TugError("tool_off", "Send texts is switched off in tug.");
        return contacts;
      case "send_text": {
        // "nobody" and "sam" show tug's two answers about who it's to; anything else sends
        // once "you" click Send on tug's card.
        const to = String(call.to).trim();
        if (to.toLowerCase() === "nobody") throw new TugError("not_found", `No contact named "${to}".`);
        if (to.toLowerCase() === "sam") {
          throw new TugError("invalid", 'More than one contact matches "Sam": Sam Ortiz, Sam Lee. Say which one.');
        }
        await wait(1500);
        return { outcome: "sent", to, detail: null };
      }
      default:
        throw new TugError("not_allowed", "Not in the mock.");
    }
  };
  return name;
}
