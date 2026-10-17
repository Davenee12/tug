// The Answer / Decline buttons a ringing call gets outside the call card (Feed rows, notification
// items). They're ordinary buttons, reachable with Tab, so the call card's rule applies to them
// too (lib/callKeys): Space never presses them, and Enter only once the call has been ringing for
// a moment with no keystrokes anywhere in the window. A click always works.

import { onUnmounted, watch, type Ref } from "vue";
import { AnswerGate, blocksCallButton, type CallKey } from "./callKeys";

/** Marks a call button, so the window-wide key watch leaves its keys to the button's own check. */
export const CALL_BUTTON_ATTR = "data-call-button";

function callKey(e: KeyboardEvent): CallKey {
  return { key: e.key, repeat: e.repeat, isComposing: e.isComposing, type: e.type === "keyup" ? "keyup" : "keydown" };
}

export function useCallButtonGate(ringing: Ref<boolean>) {
  let gate = new AnswerGate(Date.now());
  // Typing anywhere while it rings restarts the wait, as on the call card.
  const onWindowKey = (e: KeyboardEvent) => {
    const target = e.target instanceof Element ? e.target : null;
    if (target?.closest(`[${CALL_BUTTON_ATTR}]`)) return;
    gate.key(callKey(e), "other", Date.now());
  };
  const stopWatching = () => window.removeEventListener("keydown", onWindowKey, true);
  watch(
    ringing,
    (now) => {
      stopWatching();
      if (!now) return;
      gate = new AnswerGate(Date.now());
      window.addEventListener("keydown", onWindowKey, true);
    },
    { immediate: true },
  );
  onUnmounted(stopWatching);

  /** Keydown and keyup on Answer / Decline: swallow what mustn't press them. */
  function onCallKey(e: KeyboardEvent, target: "answer" | "decline") {
    if (blocksCallButton(gate, callKey(e), target, Date.now())) {
      e.preventDefault();
      e.stopPropagation();
    }
  }
  return { onCallKey };
}
