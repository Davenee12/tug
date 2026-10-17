// The PC's clipboard on the phone page ("Send clipboard to iPhone" on the PC): when one is new,
// when to tell the PC it arrived, and how to copy it on a page that isn't a secure context.
//
// Tugboat's page is plain HTTP on the home Wi-Fi, so browsers don't give it `navigator.clipboard`
// (it exists only on HTTPS or localhost pages). The older execCommand("copy") still works inside a
// tap on iOS Safari and Chrome, so that's tried first; if the browser refuses that too, the page
// selects the text and says how to copy it by hand. Pictures can't be put on the clipboard from a
// page like this at all, so the page shows the picture and says to press and hold it.

import { isFatal } from "./chunks";
import type { PageClip, PageOffer } from "./client";

/** A clipboard from the PC as the page shows it. The page keeps it after the PC lets go. */
export interface ShownClip {
  id: number;
  text: string | null;
  image: PageOffer | null;
  /** Text is ready at once; a picture once it's fetched. */
  phase: "loading" | "ready" | "failed";
  /** The PC has been told this page has it. */
  acked: boolean;
  /** A fetched picture's object URL. */
  url?: string;
  got: number;
  error?: string;
}

/** A clipboard the page hasn't shown yet (the PC sends the same one until the page says it has it). */
export function isNewClip(shown: Pick<ShownClip, "id"> | null, incoming: PageClip | null): incoming is PageClip {
  return incoming !== null && incoming.id !== shown?.id;
}

/** Start showing a clipboard from the PC: text is ready now, a picture still has to be fetched. */
export function showClip(incoming: PageClip): ShownClip {
  return {
    id: incoming.id,
    text: incoming.text,
    image: incoming.text === null ? incoming.image : null,
    phase: incoming.text !== null ? "ready" : incoming.image ? "loading" : "failed",
    acked: false,
    got: 0,
  };
}

/** Tell the PC only once the clipboard is really on screen here, and only until it has heard. */
export function needsAck(c: ShownClip | null): boolean {
  return c !== null && c.phase === "ready" && !c.acked;
}

/**
 * What the PC's answer to "I have it" means. "done": it heard (or has moved on to a newer clipboard,
 * or dropped this one): stop telling it. "retry": try again at the next check-in. "fatal": the
 * session is over, as for any other request.
 */
export function ackOutcome(code: string | null): "done" | "retry" | "fatal" {
  if (code === null || code === "not-found") return "done";
  if (["closed", "unauthorized", "in-use"].includes(code)) return "fatal";
  if (isFatal(code)) return "done";
  return "retry";
}

/** What the browser offers for copying. Built from `window` in copy.ts; plain values in tests. */
export interface CopyEnv {
  /** `window.isSecureContext`: false on Tugboat's plain-HTTP page. */
  secure: boolean;
  /** `navigator.clipboard.writeText`, which browsers only provide on secure pages. */
  writeText?: (text: string) => Promise<void>;
  /** execCommand("copy") on a selected copy of the text; true if the browser did it. */
  execCopy: (text: string) => boolean;
}

/**
 * Copy `text` from inside a tap. "copied", or "select" when the browser won't let the page copy
 * (the page then selects the text for a press-and-hold Copy). On a page that isn't secure nothing
 * is awaited before execCommand, so it still counts as part of the tap.
 */
export async function copyWith(text: string, env: CopyEnv): Promise<"copied" | "select"> {
  if (env.secure && env.writeText) {
    try {
      await env.writeText(text);
      return "copied";
    } catch {
      /* refused: try the older way */
    }
  }
  return env.execCopy(text) ? "copied" : "select";
}
