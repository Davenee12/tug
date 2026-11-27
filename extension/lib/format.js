// Pure helpers for what the popup and the in-page toast show. No chrome.* here, so they're
// tested with node --test (extension/test).

/** A code split the way people read it aloud: "482 913", "1234 5678". */
export function groupCode(code) {
  const c = String(code ?? "").trim();
  if (c.length === 6) return `${c.slice(0, 3)} ${c.slice(3)}`;
  if (c.length === 8) return `${c.slice(0, 4)} ${c.slice(4)}`;
  return c;
}

/** "just now", "2 min ago", "1 hr ago". */
export function ago(seconds) {
  const s = Math.max(0, Math.floor(Number(seconds) || 0));
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return `${h} hr ago`;
}

/** A search hit's time (ISO, UTC) the way the Feed shows it: "6:42 PM", "Yesterday", "Mon", "Mar 3". */
export function shortTime(iso, now = new Date(), locale = undefined) {
  const t = new Date(iso);
  if (Number.isNaN(t.getTime())) return "";
  const day = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((day(now) - day(t)) / 86_400_000);
  if (days <= 0) return t.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
  if (days === 1) return "Yesterday";
  if (days < 7) return t.toLocaleDateString(locale, { weekday: "short" });
  return t.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

/** tug hands out codes from the last 10 minutes; past 5 a site may already have expired it. */
export const CODE_OLD_SECONDS = 5 * 60;
export function isOldCode(seconds) {
  return Number(seconds) >= CODE_OLD_SECONDS;
}

/** The code card's age: past 5 minutes, a site may already have expired the code. */
export function codeAge(seconds) {
  return isOldCode(seconds) ? `${ago(seconds)} · may have expired` : ago(seconds);
}

/** The letter in a sender's avatar: the first letter or digit, uppercased ("#" for none). */
export function initial(name) {
  const m = String(name ?? "").match(/[\p{L}\p{N}]/u);
  return m ? m[0].toLocaleUpperCase() : "#";
}

/** Where the popup's code came from: "Chase · Messages", or just the app when they're the same. */
export function codeSource(from, app) {
  const f = String(from ?? "").trim();
  const a = String(app ?? "").trim();
  if (!f) return a;
  if (!a || a.toLowerCase() === f.toLowerCase()) return f;
  return `${f} · ${a}`;
}

/** The phone line in the popup's header. */
export function phoneLine(phone) {
  if (!phone) return { label: "iPhone", state: "unknown", detail: "" };
  const label = phone.model || "iPhone";
  const state = phone.connection === "no_phone" ? "none" : phone.connection;
  const detail =
    state === "connected"
      ? phone.battery_percent != null
        ? `${phone.battery_percent}%`
        : ""
      : state === "connecting"
        ? "Connecting…"
        : state === "none"
          ? "No iPhone set up"
          : "Not connected";
  return { label, state, detail };
}

/** A notice above the code when the phone isn't connected, or null when all is well. */
export function phoneNotice(phone) {
  if (!phone) return null;
  const name = phone.model || "Your iPhone";
  switch (phone.connection) {
    case "connected":
      return null;
    case "connecting":
      return {
        tone: "neutral",
        icon: "loader-circle",
        title: `Connecting to ${phone.model || "your iPhone"}…`,
        body: "Codes and texts arrive once it's connected.",
      };
    case "no_phone":
      return { tone: "neutral", icon: "smartphone", title: "No iPhone set up yet", body: "Connect your iPhone in tug first." };
    default:
      return {
        tone: "warning",
        icon: "bluetooth-off",
        title: `${name} isn't connected`,
        body: "Keep it nearby with Bluetooth on. tug reconnects by itself.",
      };
  }
}

/** Now playing, or null when there's nothing to show. */
export function nowPlaying(phone) {
  const np = phone?.now_playing;
  if (!np || np.state === "stopped" || (!np.title && !np.artist)) return null;
  return {
    title: np.title || "Unknown song",
    subtitle: [np.artist, np.app].filter(Boolean).join(" · "),
    playing: np.state === "playing",
  };
}

/**
 * Now playing as the popup shows it right after a music button: the phone takes a moment to report
 * the change, so a pressed play/pause shows at once and a skip dims the track until a new title
 * arrives, for up to `hold.until`. `hold` is { kind: "toggle", playing } or { kind: "skip", title }.
 * While a skip is held and the phone briefly reports nothing playing, the last shown track stays.
 * Returns what to show, whether it's still pending, and the hold to keep (null once settled).
 */
export function heldNowPlaying(np, hold, now = Date.now(), shown = null) {
  if (!hold || now > hold.until) return { np, pending: false, hold: null };
  if (hold.kind === "toggle") {
    if (!np || np.playing === hold.playing) return { np, pending: false, hold: null };
    return { np: { ...np, playing: hold.playing }, pending: false, hold };
  }
  if (!np) return { np: shown, pending: Boolean(shown), hold };
  if (np.title !== hold.title) return { np, pending: false, hold: null };
  return { np, pending: true, hold };
}

/** tug's limits for a text, in characters (tug_bridge::protocol MAX_NAME_CHARS, MAX_TEXT_CHARS). */
export const MAX_TO_CHARS = 100;
export const MAX_TEXT_CHARS = 1000;

/** Characters as tug counts them (Unicode scalar values, as Rust's `chars()`), not UTF-16 units. */
export const charCount = (s) => [...String(s ?? "")].length;

/** What stops a text going to tug as written, or null when nothing does. */
export function composeProblem(to, message) {
  if (!String(to ?? "").trim()) return "Who's it to?";
  if (!String(message ?? "").trim()) return "Write a message first.";
  if (charCount(String(to).trim()) > MAX_TO_CHARS) return `A name can be ${MAX_TO_CHARS} characters at most.`;
  if (charCount(message) > MAX_TEXT_CHARS) return `A text can be ${MAX_TEXT_CHARS} characters at most.`;
  return null;
}

/**
 * The contacts the To field offers for what's typed so far (`list` as tug's `list_contacts`
 * gives it: people texted lately first, then A to Z). Nothing typed: everyone, in that order.
 * Otherwise names where every typed word starts a word of the name come first ("sam r" →
 * "Sam Rivera"), then names that merely contain it, each keeping tug's order.
 */
export function matchContacts(list, query) {
  const fold = (s) => String(s ?? "").toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const q = fold(query);
  if (!Array.isArray(list)) return [];
  if (!q.length) return list;
  const starts = [];
  const contains = [];
  const joined = q.join(" ");
  for (const c of list) {
    const words = fold(c.name);
    let i = 0;
    const prefix = q.every((part) => {
      const at = words.slice(i).findIndex((w) => w.startsWith(part));
      if (at === -1) return false;
      i += at + 1;
      return true;
    });
    if (prefix) starts.push(c);
    else if (words.join(" ").includes(joined)) contains.push(c);
  }
  return [...starts, ...contains];
}

/** A search result that can be answered: a text someone sent (not a notification, not your own). */
export function canReply(hit) {
  return hit?.kind === "text" && typeof hit.from === "string" && hit.from.trim() !== "" && hit.from !== "You";
}

/**
 * What tug's answer to a text means for the person (tug_bridge::protocol SendOutcome, worded as
 * `tug text` words it). `queued` reads as a warning, never a failure: it may still send, so it
 * must not invite sending it again.
 */
export function sendOutcome(r) {
  const to = r?.to || "them";
  switch (r?.outcome) {
    case "sent":
      return { tone: "success", title: `Sent to ${to}`, body: r.detail || "" };
    case "queued":
      return {
        tone: "warning",
        title: `Queued for ${to}`,
        body: "tug is still waiting for your iPhone, so it may still send. Check the conversation in tug, and don't send it again.",
      };
    case "declined":
      return { tone: "info", title: "Not sent", body: `You chose Don't send for ${to}.` };
    case "timed_out":
      return { tone: "info", title: "Not sent", body: "Nobody clicked Send in tug within 2 minutes." };
    case "cancelled":
      return { tone: "info", title: "Not sent", body: "It was cancelled in tug." };
    case "failed":
      return {
        tone: "error",
        title: "Not sent",
        body: `Your iPhone didn't take it${r.detail ? ` (${r.detail.replace(/\.$/, "")})` : ""}. You can retry it from the conversation in tug.`,
      };
    default:
      return { tone: "warning", title: "Not sure it was sent", body: "Check the conversation in tug before sending it again." };
  }
}

/** A failed `send_text`: who it's to went wrong, or the usual reasons a call fails. */
export function explainSendError(code, message) {
  switch (code) {
    case "invalid":
      return { title: "Check who it's to", body: message || "tug couldn't tell who to send it to." };
    case "not_found":
      return { title: "No contact by that name", body: message || "Use their name as it's saved on your iPhone, or a phone number." };
    case "unavailable":
      return { title: "Can't send right now", body: message || "Texts aren't connected in tug right now." };
    case "failed":
      // The host may have gone after the card was answered: never invite a second send.
      return { title: "Couldn't reach tug", body: "If you clicked Send in tug, check the conversation there before sending it again." };
    default:
      return explainError(code, message);
  }
}

/**
 * What a failed call means for the person, from the native host's error code (see
 * src-tauri/crates/tug-bridge/src/native.rs) or the browser's own failure to start the host.
 * `screen` is a whole-popup state; otherwise the message shows inline where the call was made.
 */
export function explainError(code, message) {
  switch (code) {
    case "not_installed":
      // Also what a tug from before the extension looks like, so it can't say tug isn't here.
      return {
        screen: "install",
        title: "This needs the latest tug",
        body: "The extension works with tug, the free Windows app that brings your iPhone to your PC. Install tug, or update it if you already have it, then come back.",
      };
    case "wrong_extension":
      return {
        screen: "wrong_extension",
        title: "tug doesn't recognise this copy of the extension",
        body: "tug answers only the copies of the extension it trusts. Update tug, or use the tug extension from the Chrome Web Store, then try again.",
      };
    case "not_running":
      return {
        screen: "closed",
        title: "tug isn't running",
        body: "Open tug from the Start menu or the system tray, then try again.",
      };
    case "not_set_up":
    case "off":
      return {
        screen: "off",
        title: "Let tug answer the extension",
        body: "In tug, go to Settings › Developer tools and turn on “Let AI tools use tug”. The extension uses the same switches.",
      };
    case "unauthorized":
      return { title: "Access was reset in tug", body: "Try again." };
    case "tool_off":
      return { title: "Switched off in tug", body: message || "Turn it on in tug › Settings › Developer tools." };
    case "unavailable":
      return { title: "iPhone not connected", body: message || "tug will reconnect when your iPhone is nearby." };
    case "not_found":
      return { title: "No recent code", body: message || "No verification code in the last 10 minutes." };
    case "rate_limited":
      return { title: "Slow down a little", body: message || "Too many requests. Try again in a minute." };
    case "busy":
      return { title: "tug is already asking", body: message || "Answer the card in tug first." };
    case "timeout":
    case "send_timeout":
      return { title: "tug didn't answer in time", body: message || "Try again." };
    case "impostor":
      return { title: "Something other than tug answered", body: "Quit and reopen tug, then try again." };
    default:
      return { title: "Couldn't reach tug", body: message || "Try again." };
  }
}
