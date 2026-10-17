import { describe, expect, it } from "vitest";
import { ANSWER_ARM_MS, AnswerGate } from "./callKeys";

const T0 = 1_000_000;

describe("AnswerGate (keys on the incoming-call card)", () => {
  it("never answers on Space, on the card or on either call button, however long it waits", () => {
    const gate = new AnswerGate(T0);
    const later = T0 + 60_000;
    for (const target of ["card", "answer", "decline"] as const) {
      expect(gate.key({ key: " ", type: "keydown" }, target, later)).toBe("block");
      // Buttons fire Space on keyup, so the keyup is swallowed too.
      expect(gate.key({ key: " ", type: "keyup" }, target, later)).toBe("block");
    }
  });

  it("blocks an Enter typed straight after the call appears (you were mid-sentence)", () => {
    const gate = new AnswerGate(T0);
    expect(gate.key({ key: "Enter" }, "card", T0 + 200)).toBe("block");
    expect(gate.key({ key: "Enter" }, "answer", T0 + 900)).toBe("block");
  });

  it("answers on Enter once the card has been up for the arm time with no keystrokes", () => {
    const gate = new AnswerGate(T0);
    expect(gate.armed(T0 + ANSWER_ARM_MS - 1)).toBe(false);
    expect(gate.key({ key: "Enter" }, "card", T0 + ANSWER_ARM_MS)).toBe("answer");
  });

  it("restarts the wait on every keystroke, so typing through the ring never answers", () => {
    const gate = new AnswerGate(T0);
    let t = T0;
    // Typing "hello world" then Enter, one key every 300 ms, for several seconds.
    for (const key of [..."hello world and more words"]) {
      t += 300;
      gate.key({ key, type: "keydown" }, "card", t);
    }
    t += 300;
    expect(t - T0).toBeGreaterThan(ANSWER_ARM_MS);
    expect(gate.key({ key: "Enter" }, "card", t)).toBe("block");
    // The early Enter itself restarts the wait too.
    expect(gate.key({ key: "Enter" }, "card", t + ANSWER_ARM_MS - 1)).toBe("block");
    expect(gate.key({ key: "Enter" }, "card", t + 2 * ANSWER_ARM_MS)).toBe("answer");
  });

  it("an early Space also restarts the wait", () => {
    const gate = new AnswerGate(T0);
    gate.key({ key: " ", type: "keydown" }, "card", T0 + 1400);
    expect(gate.key({ key: "Enter" }, "card", T0 + 1600)).toBe("block");
  });

  it("ignores a held (auto-repeating) Enter and Enter while composing text", () => {
    const gate = new AnswerGate(T0);
    expect(gate.key({ key: "Enter", repeat: true }, "card", T0 + 5000)).toBe("block");
    const gate2 = new AnswerGate(T0);
    expect(gate2.key({ key: "Enter", isComposing: true }, "card", T0 + 5000)).toBe("block");
  });

  it("an armed Enter on a button you tabbed to presses that button (deliberate)", () => {
    const gate = new AnswerGate(T0);
    expect(gate.key({ key: "Enter" }, "decline", T0 + 3000)).toBe("pass");
  });

  it("a Tab restarts the wait, so Tab then Enter mid-typing can't press a button", () => {
    const gate = new AnswerGate(T0);
    gate.key({ key: "Tab" }, "card", T0 + 2000);
    expect(gate.key({ key: "Enter" }, "decline", T0 + 2100)).toBe("block");
  });

  it("leaves Escape and Tab to the focus trap, and keys on other controls alone", () => {
    const gate = new AnswerGate(T0);
    expect(gate.key({ key: "Escape" }, "card", T0 + 10)).toBe("ignore");
    expect(gate.key({ key: "Tab" }, "card", T0 + 10)).toBe("ignore");
    expect(gate.key({ key: " " }, "other", T0 + 10)).toBe("ignore");
    expect(gate.key({ key: "Enter" }, "other", T0 + 9000)).toBe("ignore");
  });

  it("a lone modifier doesn't count as typing", () => {
    const gate = new AnswerGate(T0);
    gate.key({ key: "Shift" }, "card", T0 + 1400);
    expect(gate.key({ key: "Enter" }, "card", T0 + 1600)).toBe("answer");
  });
});
