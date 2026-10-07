// Tugboat on the desktop side: the small decisions the Tugboat panel makes, kept pure for tests.

import type { TugboatSkipped, TugboatStatus } from "../types/protocol";

export { formatSize } from "../tugboat-page/chunks";

/** How long the QR code shows with no phone before the panel offers connection help. */
export const HELP_AFTER_MS = 30_000;

/** Tugboat as it is before it's opened (and after it closes). */
export const TUGBOAT_OFF: TugboatStatus = {
  phase: "off",
  url: null,
  address: null,
  qr: null,
  phone: null,
  phoneActive: false,
  folder: null,
  incoming: [],
  outgoing: [],
  texts: [],
  sentText: null,
  sending: false,
  receiving: false,
  ended: null,
};

/** No phone has turned up ~30 s after the code appeared: time for "Can't connect?" help. */
export function showConnectHelp(s: Pick<TugboatStatus, "phase">, shownAt: number | null, now: number): boolean {
  return s.phase === "waiting" && shownAt !== null && now - shownAt >= HELP_AFTER_MS;
}

/**
 * Files are moving either way right now (closing would cut them off). From the backend's live
 * flags, not "an upload is unfinished": one the phone abandoned stays unfinished forever.
 */
export function transferring(s: Pick<TugboatStatus, "sending" | "receiving">): boolean {
  return s.sending || s.receiving;
}

/** Why closing Tugboat would lose something, or null when it can just close: files still moving
 * either way, or files offered to the phone that it hasn't saved yet (they're gone once it closes). */
export type CloseWarning = "moving" | "unsaved";
export function closeWarning(s: Pick<TugboatStatus, "incoming" | "sending" | "outgoing">): CloseWarning | null {
  if (transferring(s)) return "moving";
  if (s.outgoing.some((o) => o.downloads === 0)) return "unsaved";
  return null;
}

/** "phone" → "Phone": a phone name at the start of a sentence. Leaves names like "iPhone" alone. */
export function capitalised(name: string): string {
  if (/^[a-z][A-Z]/.test(name)) return name;
  return name.charAt(0).toUpperCase() + name.slice(1);
}

/** One plain sentence for files that couldn't be offered to the phone, or null if all were. */
export function skippedMessage(skipped: TugboatSkipped[]): string | null {
  if (!skipped.length) return null;
  if (skipped.length === 1) {
    const { name, reason } = skipped[0];
    switch (reason) {
      case "tooBig":
        return `“${name}” is over 1 GB, too big to send to a phone's browser.`;
      case "folder":
        return `“${name}” is a folder. Send the files inside it instead.`;
      case "tooMany":
        return `“${name}” wasn't added: that's as many files as Tugboat can offer at once.`;
      default:
        return `Couldn't read “${name}”.`;
    }
  }
  const reasons = new Set(skipped.map((s) => s.reason));
  const why =
    reasons.size === 1 && reasons.has("tooBig")
      ? "they're over 1 GB"
      : reasons.size === 1 && reasons.has("folder")
        ? "folders can't be sent"
        : "folders, files over 1 GB, or too many at once";
  return `${skipped.length} items weren't added: ${why}.`;
}

/** Percent of a file received, for its progress bar. */
export function percent(received: number, size: number): number {
  return size > 0 ? Math.min(100, Math.round((received / size) * 100)) : 100;
}
