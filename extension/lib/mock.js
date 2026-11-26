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
};

export function installMock(name) {
  const s = STATES[name] ?? STATES.connected;
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  globalThis.__tugMock = async (call) => {
    await wait(name === "loading" ? 1e9 : 60);
    if (s.all) throw s.all;
    switch (call.tool) {
      case "status":
        return { version: "0.5.24", permissions, phone: s.phone ?? phone };
      case "get_latest_code": {
        if (s.code instanceof Error) throw s.code;
        return { ...(s.code ?? code), copied: call.copy };
      }
      case "search_messages":
        // tug answers with the hits themselves, newest first.
        return hits.filter((h) => (h.text + h.from).toLowerCase().includes(call.query.toLowerCase()));
      case "media_control":
        return { ok: true };
      default:
        throw new TugError("not_allowed", "Not in the mock.");
    }
  };
  return name;
}
