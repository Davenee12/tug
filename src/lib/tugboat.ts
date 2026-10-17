// Tugboat on the desktop side: the small decisions the Tugboat panel makes, kept pure for tests.

import type { TugboatClip, TugboatClipEvent, TugboatSkipped, TugboatStatus } from "../types/protocol";

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
  folderBlock: null,
  incoming: [],
  outgoing: [],
  texts: [],
  sentText: null,
  sending: false,
  receiving: false,
  ended: null,
  clip: null,
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
export function closeWarning(s: Pick<TugboatStatus, "sending" | "receiving" | "outgoing">): CloseWarning | null {
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

/** What the panel says when Windows Security (Controlled folder access) refused Pictures, or null. */
export function folderBlockMessage(s: Pick<TugboatStatus, "folderBlock">): string | null {
  switch (s.folderBlock) {
    case "fallback":
      return "Windows Security blocked tug from saving to Pictures, so files from your phone are going to Downloads\\Tugboat. To use Pictures, allow tug under Controlled folder access.";
    case "blocked":
      return "Windows Security blocked tug from saving to Pictures. Allow tug under Controlled folder access.";
    default:
      return null;
  }
}

/** Percent of a file received, for its progress bar. */
export function percent(received: number, size: number): number {
  return size > 0 ? Math.min(100, Math.round((received / size) * 100)) : 100;
}

/**
 * Open something that takes a while (the game's phone-controller channel), and if it's no longer
 * wanted by the time it opens (the game closed, or the controller was switched off meanwhile),
 * close it again rather than leave it open with nobody using it. A close sent while the open was
 * still on its way can be overtaken by it, so this one is sent after. True if it's open and wanted.
 * A failed open throws, as `open` does.
 */
export async function openUnlessAbandoned(
  open: () => Promise<unknown>,
  close: () => Promise<unknown>,
  stillWanted: () => boolean,
): Promise<boolean> {
  await open();
  if (stillWanted()) return true;
  await close().catch(() => undefined);
  return false;
}

/**
 * Tugboat Run's phone-controller channel. The backend has exactly one (a single slot that the
 * latest open or close sets), so an open that lands keeps it only if the controller is wanted
 * *now*: switched on, and the game not closed. Deciding by which open is the newest instead would
 * let an older open that lands last close the channel a newer one just opened (on, off, on again
 * with the first open slow), leaving the phones dead while the game shows the controller on.
 */
export function controllerChannel(
  io: { open: () => Promise<unknown>; close: () => Promise<unknown> },
  state: () => { on: boolean; closed: boolean },
) {
  const wanted = () => {
    const s = state();
    return s.on && !s.closed;
  };
  return {
    /** Open (or re-size) the channel. True if it's open and still wanted; throws if the open failed. */
    open: () => openUnlessAbandoned(io.open, io.close, wanted),
  };
}

// --- The shared clipboard ("Send clipboard to iPhone"; src-tauri/src/tugboat/clip.rs) ---

type Say = { kind: "info" | "error"; text: string };

/** The phone in clipboard wording: the one that connected, else "iPhone" (the phone tug is for). */
export function clipPhone(phone: string | null): string {
  return phone ?? "iPhone";
}

/**
 * After "Send clipboard": with Tugboat's page open on the phone it arrives in a moment, so a quiet
 * note is enough. Otherwise the panel opens, with the code to scan or a nudge to open the page.
 */
export function clipNeedsPanel(s: Pick<TugboatStatus, "phase" | "phoneActive">): boolean {
  return !(s.phase === "connected" && s.phoneActive);
}

/** "1:42": what's left of a waiting clipboard's 2 minutes. */
export function timeLeft(expiresAt: number, now: number): string {
  const secs = Math.max(0, Math.ceil((expiresAt - now) / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/** The note when the clipboard reached the phone or was dropped. Only ever after the phone said so. */
export function clipEventMessage(e: TugboatClipEvent, phone: string | null): Say | null {
  const p = clipPhone(phone);
  if (e.state === "delivered") {
    return { kind: "info", text: e.kind === "image" ? `The picture from your clipboard is on your ${p}` : `Your clipboard is on your ${p}` };
  }
  if (e.state === "expired") {
    return { kind: "error", text: `Your clipboard wasn't sent: Tugboat wasn't opened on your ${p} within 2 minutes.` };
  }
  return null;
}

/** The Tugboat panel's line about the last clipboard sent, or null if none was. */
export function clipLine(clip: TugboatClip | null, s: Pick<TugboatStatus, "phone" | "phoneActive">, now: number): string | null {
  if (!clip) return null;
  const p = clipPhone(s.phone);
  const what = clip.kind === "image" ? "The picture from your clipboard" : "Your clipboard";
  switch (clip.state) {
    case "waiting":
      return s.phoneActive
        ? `Sending your clipboard to your ${p}…`
        : `Your clipboard is waiting for Tugboat to open on your ${p} · ${timeLeft(clip.expiresAt, now)} left`;
    case "delivered":
      return `${what} is on your ${p}.`;
    case "expired":
      return `Not sent: Tugboat wasn't opened on your ${p} within 2 minutes.`;
  }
}

/** Under the QR code: with a clipboard waiting, scanning is how it gets to the phone. */
export function codeCaption(clip: TugboatClip | null): string {
  return clip?.state === "waiting"
    ? "Scan to receive your clipboard on your iPhone."
    : "Scan with your phone's camera to send photos, files and text.";
}

/** The quiet note once text pasted on the phone is on this PC's clipboard (or couldn't go there). */
export function phoneTextMessage(ok: boolean, phone: string | null): Say {
  const p = phone ?? "phone";
  return ok ? { kind: "info", text: `Copied from your ${p}` } : { kind: "error", text: `Text from your ${p} arrived, but couldn't go on the clipboard` };
}
