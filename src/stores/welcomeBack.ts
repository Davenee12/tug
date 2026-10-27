// "Welcome back" (lib/welcomeBack): watches for the person coming back to tug's window after a
// while and, when enough is waiting, holds the absence the Feed's card sums up. The card itself
// works out what to show (it ages codes on its own clock); this only keeps which absence, and
// whether the card has been seen.

import { defineStore } from "pinia";
import { ref } from "vue";
import { AwayClock, welcomeBack, worthACard, type Absence, type WelcomeBack } from "../lib/welcomeBack";
import { useTugStore } from "./tug";

/** Input that says someone is at the window. Pointer moves too: coming back, the mouse moves first. */
const PRESENCE_EVENTS = ["pointerdown", "pointermove", "keydown", "wheel"] as const;

export const useWelcomeBackStore = defineStore("welcomeBack", () => {
  const tug = useTugStore();
  /** The absence the card is about, or null for no card. */
  const card = ref<(Absence & { seen: boolean }) | null>(null);

  /** What's waiting from that absence, as of `now`, from what tug holds right now. */
  function summary(a: Absence, now: number): WelcomeBack {
    return welcomeBack({
      ...a,
      now,
      notifications: tug.notifications,
      messages: tug.messages,
      contacts: tug.contacts,
      seenAt: (key) => tug.seen[key] ?? tug.seenSince,
      isKnown: tug.isKnown,
      clearedCodes: tug.clearedCodes,
    });
  }

  /**
   * They're back after `a`: a card if it's worth one. Either way it replaces the card from an
   * earlier absence, which by now is old news. True when a card is showing.
   */
  function cameBack(a: Absence): boolean {
    card.value = worthACard(summary(a, a.until)) ? { ...a, seen: false } : null;
    return card.value !== null;
  }

  /** The card has been on screen long enough to have been read. */
  function markSeen() {
    if (card.value) card.value = { ...card.value, seen: true };
  }

  /** "Got it", or done with it: scrolled on, or acted elsewhere in the Feed. */
  function dismiss() {
    card.value = null;
  }

  let clock: AwayClock | null = null;
  let teardown: Array<() => void> = [];

  // Window events only (lib/welcomeBack's AwayClock says why that's the signal): passive and
  // captured, so nothing in the page can hide them, and cheap enough for every pointer move.
  const here = () => {
    if (document.hidden || !clock) return;
    const away = clock.here(Date.now());
    if (away) cameBack(away);
  };
  const left = () => clock?.left(Date.now());
  const onVisibility = () => (document.hidden ? left() : document.hasFocus() && here());

  function init() {
    if (clock) return;
    clock = new AwayClock(Date.now(), !document.hidden && document.hasFocus());
    const opts = { capture: true, passive: true };
    for (const type of PRESENCE_EVENTS) window.addEventListener(type, here, opts);
    window.addEventListener("focus", here);
    window.addEventListener("blur", left);
    document.addEventListener("visibilitychange", onVisibility);
    teardown = [
      () => {
        for (const type of PRESENCE_EVENTS) window.removeEventListener(type, here, opts);
        window.removeEventListener("focus", here);
        window.removeEventListener("blur", left);
        document.removeEventListener("visibilitychange", onVisibility);
      },
    ];
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    clock = null;
  }

  return { card, summary, cameBack, markSeen, dismiss, init, dispose };
});
