import { describe, expect, it } from "vitest";
import { EMOTICONS, SHORTCODES, afterTyping, emojiFor, finishForSend, undoOnBackspace } from "./emoji";

/** Type `text` one character at a time, applying each change, as the box does. */
function type(text: string): string {
  let box = "";
  for (const ch of text) {
    box += ch;
    const change = afterTyping(box, box.length);
    if (change) box = change.text;
  }
  return box;
}

describe("emoticons", () => {
  it("turn into emoji once finished by a space, a new line or punctuation", () => {
    expect(type("hi :) ")).toBe("hi 🙂 ");
    expect(type("love you <3!")).toBe("love you ❤️!");
    expect(type(":D\n")).toBe("😄\n");
    expect(type("ok ;).")).toBe("ok 😉.");
    expect(type("nooo :'( ")).toBe("nooo 😢 ");
  });

  it("wait until they're finished", () => {
    expect(type("hi :)")).toBe("hi :)");
    // ":D" isn't caught as ":" and "</3" isn't caught at "<3".
    expect(type("sad </3 ")).toBe("sad 💔 ");
    expect(type(":-D ")).toBe("😄 ");
  });

  it("only stand on their own", () => {
    expect(type("see http://x.com ")).toBe("see http://x.com ");
    expect(type("ratio 4:3 ")).toBe("ratio 4:3 ");
    expect(type("a:) ")).toBe("a:) ");
    expect(type("at 10:30 ")).toBe("at 10:30 ");
    expect(type("(option B) ")).toBe("(option B) ");
  });
});

describe("named emoji", () => {
  it("turn as soon as the closing colon is typed", () => {
    expect(type("nice :fire:")).toBe("nice 🔥");
    expect(type(":thumbsup: thanks")).toBe("👍 thanks");
    expect(type(":+1:")).toBe("👍");
    expect(type(":100:")).toBe("💯");
  });

  it("leave unknown names and names stuck to a word alone", () => {
    expect(type("hi :nope: ")).toBe("hi :nope: ");
    expect(type("note:fire: ")).toBe("note:fire: ");
    expect(type("time: 5:fire:")).toBe("time: 5:fire:");
  });
});

describe("in the middle of the text", () => {
  it("changes only what's before the caret and moves the caret with it", () => {
    // Typed a space after ":)" in "a :)| b" (caret after the new space).
    const text = "a :)  b";
    const change = afterTyping(text, 5);
    expect(change?.text).toBe("a 🙂  b");
    expect(change?.caret).toBe(2 + "🙂".length + 1);
  });
});

describe("Send", () => {
  it("finishes an emoticon at the very end", () => {
    expect(finishForSend("see you :)")).toBe("see you 🙂");
    expect(finishForSend("<3")).toBe("❤️");
    expect(finishForSend("half:)")).toBe("half:)");
    expect(finishForSend("no emoticon")).toBe("no emoticon");
  });
});

describe("Backspace straight after", () => {
  it("puts back exactly what was typed", () => {
    const change = afterTyping("hi :P ", 6)!;
    expect(change.text).toBe("hi 😛 ");
    expect(undoOnBackspace(change.undo, change.text, change.caret)).toEqual({ text: "hi :P ", caret: 6 });
  });

  it("does nothing once the text has moved on", () => {
    const change = afterTyping("hi :P ", 6)!;
    expect(undoOnBackspace(change.undo, change.text + "x", change.caret + 1)).toBeNull();
    expect(undoOnBackspace(change.undo, change.text, 0)).toBeNull();
    expect(undoOnBackspace(null, "x", 1)).toBeNull();
  });
});

describe("the tables", () => {
  it("map every entry to an emoji, and know only their own keys", () => {
    for (const [k, v] of Object.entries(EMOTICONS)) expect(emojiFor(k), k).toBe(v);
    for (const [k, v] of Object.entries(SHORTCODES)) expect(emojiFor(`:${k}:`), k).toBe(v);
    expect(emojiFor("toString")).toBeNull();
    expect(emojiFor(":constructor:")).toBeNull();
  });
});
