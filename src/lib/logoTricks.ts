// The sidebar logo's little moments: a tug when the phone connects, a wiggle on hover and a
// full rope spin for anyone who types "tug" five times into search. All one-shot animations.

import type { ConnectionState } from "../types/protocol";

/**
 * Whether the logo should give its tug: the connection has just become "connected". `prev` is
 * null before tug knows the real status (launch), so a phone that was already connected when
 * the window loaded doesn't tug, while the first real connect after launch does. Status
 * refreshes that don't change the state never replay it.
 */
export function shouldTug(prev: ConnectionState | null, next: ConnectionState | null): boolean {
  return prev !== null && prev !== "connected" && next === "connected";
}

/** Hovering wiggles at most this often, so resting the pointer on the logo doesn't loop it. */
export const WIGGLE_GAP_MS = 2000;

/** Whether a hover now may wiggle, given when the last wiggle started (null: never). */
export function canWiggle(lastAt: number | null, nowMs: number): boolean {
  return lastAt === null || nowMs - lastAt >= WIGGLE_GAP_MS;
}

/** "tug" five times, any case, spaces optional ("tug tug tug tug tug", "TUGTUGTUGTUGTUG"). */
export function isTugChant(query: string): boolean {
  return /^(\s*tug){5}\s*$/i.test(query);
}
