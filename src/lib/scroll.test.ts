import { describe, expect, it } from "vitest";
import { EDGE_SLACK_PX, isAtTop, preservedScrollTop, shouldStickToBottom, TOP_SLACK_PX } from "./scroll";

describe("shouldStickToBottom", () => {
  // A 600px viewport over 2000px of content: the bottom is at scrollTop 1400.
  it("sticks when the reader is sitting at the bottom", () => {
    expect(shouldStickToBottom(1400, 2000, 600)).toBe(true);
  });

  it("sticks within the slack just above the bottom", () => {
    expect(shouldStickToBottom(1400 - EDGE_SLACK_PX, 2000, 600)).toBe(true);
    expect(shouldStickToBottom(1400 - EDGE_SLACK_PX + 1, 2000, 600)).toBe(true);
  });

  it("does not stick once the reader has scrolled up into history", () => {
    expect(shouldStickToBottom(1400 - EDGE_SLACK_PX - 1, 2000, 600)).toBe(false);
    expect(shouldStickToBottom(0, 2000, 600)).toBe(false);
  });

  it("treats a thread shorter than its viewport as at the bottom", () => {
    // Opening a brand-new conversation (one line, nothing to scroll) should still pin to bottom.
    expect(shouldStickToBottom(0, 200, 600)).toBe(true);
  });

  it("honours a custom slack", () => {
    expect(shouldStickToBottom(1000, 2000, 600, 10)).toBe(false);
    expect(shouldStickToBottom(1395, 2000, 600, 10)).toBe(true);
  });
});

describe("isAtTop", () => {
  it("is true at the very top and within the slack", () => {
    expect(isAtTop(0)).toBe(true);
    expect(isAtTop(TOP_SLACK_PX)).toBe(true);
  });
  it("is false once scrolled past the slack", () => {
    expect(isAtTop(TOP_SLACK_PX + 1)).toBe(false);
    expect(isAtTop(500)).toBe(false);
  });
});

describe("preservedScrollTop", () => {
  it("stays at the top so a new notification is seen", () => {
    expect(preservedScrollTop({ prevTop: 0, prevHeight: 2000, newHeight: 2080, prependedAtTop: true })).toBe(0);
  });

  it("compensates for a notification added above the viewport", () => {
    // Scrolled to 900; an 80px row lands on top; the reader's rows should not move.
    expect(preservedScrollTop({ prevTop: 900, prevHeight: 2000, newHeight: 2080, prependedAtTop: true })).toBe(980);
  });

  it("keeps the position when a row above the viewport is cleared", () => {
    // Content shrank at the top by 80px: shift up by the same so the view holds.
    expect(preservedScrollTop({ prevTop: 900, prevHeight: 2000, newHeight: 1920, prependedAtTop: true })).toBe(820);
  });

  it("leaves the view alone (null) when older history pages in below", () => {
    // loadMore appends at the bottom: not a top prepend, so the browser's own anchoring holds it.
    expect(preservedScrollTop({ prevTop: 900, prevHeight: 2000, newHeight: 3000, prependedAtTop: false })).toBeNull();
  });

  it("never returns a negative offset", () => {
    expect(preservedScrollTop({ prevTop: 20, prevHeight: 2000, newHeight: 1000, prependedAtTop: true })).toBe(0);
  });
});
