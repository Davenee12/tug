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
 * What a failed call means for the person, from the native host's error code (see
 * src-tauri/crates/tug-bridge/src/native.rs) or the browser's own failure to start the host.
 * `screen` is a whole-popup state; otherwise the message shows inline where the call was made.
 */
export function explainError(code, message) {
  switch (code) {
    case "not_installed":
      return {
        screen: "install",
        title: "tug isn't on this PC yet",
        body: "This extension works with tug, the free Windows app that brings your iPhone to your PC. Install it, then come back.",
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
