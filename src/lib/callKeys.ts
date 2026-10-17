// Keys on the incoming-call card. A call can ring while you're typing, so a keystroke meant for
// something else must never answer or decline it: tug must never act on a call without you.
//
// - Focus stays on the card itself, never on Answer: a focused button fires on Space or Enter.
// - Space never answers or declines, wherever focus is.
// - Enter answers (or presses the focused Decline / Answer button) only once the card has been up
//   for ANSWER_ARM_MS with no keystrokes at all. Every keystroke, an early Enter included, starts
//   that wait again, so someone typing through the ring can't pick up by accident.
// - Escape (hide) and Tab (move between buttons) are handled by the focus trap as usual.

export const ANSWER_ARM_MS = 1500;

/** Where the key landed: the card itself, one of the two call buttons, or anything else. */
export type CallKeyTarget = "card" | "answer" | "decline" | "other";

/**
 * What to do with a key:
 * - "answer": answer the call (Enter on the card, armed);
 * - "pass": let the browser handle it (an armed Enter on a focused button presses that button);
 * - "block": swallow it so the focused button doesn't fire;
 * - "ignore": not ours (Escape, Tab, keys on other controls), leave it alone.
 */
export type CallKeyAction = "answer" | "pass" | "block" | "ignore";

export interface CallKey {
  key: string;
  repeat?: boolean;
  isComposing?: boolean;
  /** keydown or keyup; buttons fire Space on keyup, so both are checked. */
  type?: "keydown" | "keyup";
}

export class AnswerGate {
  private quietSince: number;

  constructor(shownAt: number) {
    this.quietSince = shownAt;
  }

  /** True once the card has been up, with no keystrokes, for ANSWER_ARM_MS. */
  armed(now: number): boolean {
    return now - this.quietSince >= ANSWER_ARM_MS;
  }

  /** Decide one key event. Keydowns (other than modifiers alone) restart the quiet wait. */
  key(e: CallKey, target: CallKeyTarget, now: number): CallKeyAction {
    const type = e.type ?? "keydown";
    const onCallButton = target === "answer" || target === "decline";
    if (e.key === " " || e.key === "Spacebar") {
      if (type === "keydown") this.quietSince = now;
      return target === "card" || onCallButton ? "block" : "ignore";
    }
    if (type === "keyup") return "ignore";
    if (e.key === "Enter") {
      // Decide before restarting the wait: this Enter is itself a keystroke.
      const ready = this.armed(now) && !e.repeat && !e.isComposing;
      this.quietSince = now;
      if (target === "other") return "ignore";
      if (!ready) return "block";
      return target === "card" ? "answer" : "pass";
    }
    if (!MODIFIERS.has(e.key)) this.quietSince = now;
    return "ignore";
  }
}

// A lone modifier isn't typing (Ctrl+Tab, Alt to look at the menu); the key that follows is.
const MODIFIERS = new Set(["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock"]);

/**
 * The Answer / Decline buttons a ringing call also gets in the Feed. They're ordinary buttons
 * there, reachable with Tab, so the same gate applies: true when a key must be swallowed (Space
 * always; Enter until the call has been on screen a moment with no keystrokes), so a key pressed
 * while typing never picks up or hangs up. A click still works.
 */
export function blocksCallButton(gate: AnswerGate, e: CallKey, target: "answer" | "decline", now: number): boolean {
  return gate.key(e, target, now) === "block";
}
