import { describe, expect, it } from "vitest";
import { afterTyping, undoOnBackspace } from "./emoji";
import { differ } from "./emojiTyping";

/** Apply a `differ` result to `before`. */
const apply = (before: string, d: ReturnType<typeof differ>) => before.slice(0, d.start) + d.insert + before.slice(d.end);

describe("differ", () => {
  it("finds just the stretch a conversion changed, so it can be made as one edit", () => {
    const change = afterTyping("hi :) there", 6)!;
    const d = differ("hi :) there", change.text);
    expect(d).toEqual({ start: 3, end: 5, insert: "🙂" });
    expect(apply("hi :) there", d)).toBe(change.text);
  });

  it("does the same for Backspace's undo", () => {
    const change = afterTyping("nice :fire:", 11)!;
    const back = undoOnBackspace(change.undo, change.text, change.caret)!;
    const d = differ(change.text, back.text);
    expect(d.insert).toBe(":fire:");
    expect(apply(change.text, d)).toBe("nice :fire:");
  });

  it("never splits an emoji in two", () => {
    for (const [a, b] of [
      ["😛x", "😜x"],
      ["a😛", "a😜"],
      ["🙂", "🙂🙂"],
    ]) {
      const d = differ(a, b);
      expect(apply(a, d)).toBe(b);
      expect(d.insert).toBe(d.insert.normalize());
      expect(/[\ud800-\udbff]$/.test(d.insert) || /^[\udc00-\udfff]/.test(d.insert)).toBe(false);
    }
  });
});
