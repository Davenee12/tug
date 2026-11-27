// Texts sent from the popup: what the popup hands the service worker, and how it reads back what
// happened. The service worker keeps who the last text went to and the outcome, never the
// message, in storage.session (memory only, gone when the browser closes).

import { explainSendError, sendOutcome } from "./format.js";

export const LAST_TEXT = "lastText";
export const MESSAGE_SEND_TEXT = "tug:send-text";

/** A finished text stays on the popup this long unless dismissed. */
export const SHOW_RESULT_MS = 10 * 60_000;
/** Longer than any send waits (the host's limit is 3½ minutes): a wait older than this was cut off. */
export const WAITING_MAX_MS = 4 * 60_000;

const ICON = { success: "check", warning: "clock", info: "message-square", error: "circle-alert" };

/**
 * The popup's notice for the last text, or null for none. `waiting` while tug's card is up;
 * `dismissable` once nothing more will happen.
 */
export function lastTextNotice(last, now = Date.now()) {
  if (!last || typeof last !== "object" || typeof last.at !== "number") return null;
  const age = now - last.at;
  const to = last.to || "them";
  if (last.state === "waiting") {
    if (age <= WAITING_MAX_MS) {
      return {
        tone: "info",
        icon: "loader-circle",
        title: "Waiting for you in tug",
        body: `Click Send on tug's card to text ${to}.`,
        waiting: true,
        dismissable: false,
      };
    }
    // The answer never came back (the browser stopped the extension mid-wait): don't guess.
    if (age > WAITING_MAX_MS + SHOW_RESULT_MS) return null;
    return { ...sendOutcome(null), icon: ICON.warning, waiting: false, dismissable: true };
  }
  if (age > SHOW_RESULT_MS) return null;
  if (last.state === "done") {
    const o = sendOutcome(last.result);
    return { ...o, icon: ICON[o.tone], waiting: false, dismissable: true };
  }
  if (last.state === "error") {
    const e = explainSendError(last.error?.code, last.error?.message);
    return { tone: "error", icon: ICON.error, title: e.title, body: e.body, waiting: false, dismissable: true };
  }
  return null;
}
