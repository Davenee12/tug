// Keeping derived lists stable across recomputes, so the UI only re-renders what changed.
//
// Grouping functions (groupFeed, groupConversations) build fresh objects every time they run. Run
// on every new text, every cleared notification, every minute, that handed each row a "new" prop
// and re-rendered every row on screen for one change. These helpers hand back the previous object
// for anything whose contents are the same objects as before, and the previous list itself when
// nothing changed at all, so Vue skips the rows (and anything reading the list) entirely.

import type { Conversation, ConversationItem, FeedEntry } from "./format";

/** Same length and the same elements (by identity) in the same order. */
export function sameItems<T>(a: readonly T[], b: readonly T[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

/**
 * `next`, with each element swapped for the previous one under the same key when `same` says they
 * match. If every element matched and nothing moved, `prev` itself comes back, so a computed built
 * on this reports no change at all.
 */
export function reuseUnchanged<T extends { key: string }>(prev: readonly T[] | undefined, next: T[], same: (a: T, b: T) => boolean): T[] {
  if (!prev || prev.length === 0) return next;
  const before = new Map(prev.map((x) => [x.key, x]));
  let allReused = prev.length === next.length;
  const out = next.map((x, i) => {
    const old = before.get(x.key);
    if (old && same(old, x)) {
      if (prev[i] !== old) allReused = false;
      return old;
    }
    allReused = false;
    return x;
  });
  return allReused ? (prev as T[]) : out;
}

/**
 * Two Feed rows show the same thing: same kind, the same notifications (the objects themselves, so
 * an in-place change such as `live` is still seen by the row), the same newest one and the same
 * names (an app's display name can arrive later and change in place).
 */
export function sameFeedEntry(a: FeedEntry, b: FeedEntry): boolean {
  if (a.kind === "thread" && b.kind === "thread") {
    return (
      a.thread.latest === b.thread.latest &&
      a.thread.contact === b.thread.contact &&
      a.thread.appLabel === b.thread.appLabel &&
      sameItems(a.thread.items, b.thread.items)
    );
  }
  if (a.kind === "stack" && b.kind === "stack") {
    return a.stack.latest === b.stack.latest && a.stack.appLabel === b.stack.appLabel && sameItems(a.stack.items, b.stack.items);
  }
  return false;
}

/** The text or notification behind a conversation item (the object the store holds). */
const source = (i: ConversationItem) => (i.kind === "message" ? i.m : i.n);

/**
 * Two conversations show the same thing: same people and numbers, the same notifications, and the
 * same items in the same order, each backed by the same stored text or notification (a send that
 * moves from Sending to Sent replaces its message object, so it never counts as the same) at the
 * same time with the same words.
 */
export function sameConversation(a: Conversation, b: Conversation): boolean {
  if (
    a.contact !== b.contact ||
    a.appId !== b.appId ||
    a.appLabel !== b.appLabel ||
    a.address !== b.address ||
    a.items.length !== b.items.length ||
    !sameItems(a.addresses, b.addresses) ||
    !sameItems(a.notifications, b.notifications)
  ) {
    return false;
  }
  for (let i = 0; i < a.items.length; i++) {
    const x = a.items[i];
    const y = b.items[i];
    if (x.kind !== y.kind || x.id !== y.id || source(x) !== source(y) || x.body !== y.body || x.at.getTime() !== y.at.getTime()) return false;
  }
  return true;
}
