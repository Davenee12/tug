// A long conversation renders only its newest items; older ones appear as you scroll up to them.
// Opening someone with hundreds of texts then costs the same as opening a short conversation, and
// typing or a new text doesn't re-render history nobody is looking at. Pure, so it's tested.

/** How many of a conversation's newest items are on screen when it opens. */
export const THREAD_WINDOW = 60;
/** How many older items each reveal adds, once you scroll near the top of what's shown. */
export const THREAD_STEP = 60;
/** Within this many pixels of the top, the next older items are revealed. */
export const REVEAL_PX = 400;
/** Items kept above a search result that's revealed, so it doesn't sit at the very top. */
const CONTEXT = 10;

/**
 * Where the window starts in `items` (oldest first). It's pinned to an item (`anchor`, the oldest
 * one on screen) rather than a count from the end, so a new text arriving at the bottom never
 * pushes one off the top while you read. No anchor, or it's gone: the newest `size` items.
 */
export function windowStart(items: readonly { id: string }[], anchor: string | null, size = THREAD_WINDOW): number {
  if (anchor != null) {
    const at = items.findIndex((i) => i.id === anchor);
    if (at >= 0) return at;
  }
  return Math.max(0, items.length - size);
}

/** The start after revealing the next older step. */
export function olderWindowStart(start: number, step = THREAD_STEP): number {
  return Math.max(0, start - step);
}

/** A start that includes the item at `target` (opened from search), with a little above it. */
export function startIncluding(start: number, target: number): number {
  return target >= 0 && target < start ? Math.max(0, target - CONTEXT) : start;
}
