// Scroll-position maths for the live lists, kept pure so they can be unit-tested without a DOM.
// Two shapes of list: the Messages thread grows at the *bottom* (a chat), the Feed grows at the
// *top* (newest first). Both must hold the user's place while entries arrive, clear and re-render.

/** Within this many pixels of an edge counts as "at" it — a comfortable finger's width. */
export const EDGE_SLACK_PX = 120;
/** The Feed treats a smaller slop as "at the top": there the user means "show me the newest". */
export const TOP_SLACK_PX = 8;

/**
 * A chat should snap to the newest line only when the reader is already there. Measured before
 * the new message lands: if the viewport's bottom is within `slack` of the content's bottom, the
 * reader is following along and we keep them pinned; if they've scrolled up into history, we don't
 * yank them down. A thread shorter than its viewport (nothing to scroll) always counts as "at the
 * bottom", so the first message of a conversation sticks.
 */
export function shouldStickToBottom(
  scrollTop: number,
  scrollHeight: number,
  clientHeight: number,
  slack: number = EDGE_SLACK_PX,
): boolean {
  return scrollHeight - clientHeight - scrollTop <= slack;
}

/** Already scrolled to (or near) the top of a newest-first list. */
export function isAtTop(scrollTop: number, slack: number = TOP_SLACK_PX): boolean {
  return scrollTop <= slack;
}

/**
 * Where to move a newest-first list (the Feed) after its contents change, so the user keeps their
 * place — or `null` to leave the scroll where it is. At the top we pin to the top, so a new
 * notification is seen. Otherwise, when something new arrived at the *top* of the list (a new row
 * or one bumped up above the viewport), push the scroll down by exactly how much taller the content
 * got, so the rows under the reader's eye don't move. For anything else (older history paged in at
 * the bottom, a row changing out of sight) we return `null` and let the browser's own scroll
 * anchoring hold the view — overriding it would be the thing that makes the list jump.
 */
export function preservedScrollTop(opts: {
  prevTop: number;
  prevHeight: number;
  newHeight: number;
  prependedAtTop: boolean;
  topSlack?: number;
}): number | null {
  const { prevTop, prevHeight, newHeight, prependedAtTop, topSlack = TOP_SLACK_PX } = opts;
  if (isAtTop(prevTop, topSlack)) return 0;
  if (!prependedAtTop) return null;
  return Math.max(0, prevTop + (newHeight - prevHeight));
}
