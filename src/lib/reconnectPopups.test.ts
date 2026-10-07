import { describe, expect, it } from "vitest";
import type { PhoneNotification } from "../types/protocol";
import { arrivedDuringGap, GAP_POPUP_MAX, gapSummaryText, planGapPopups } from "./reconnectPopups";

const min = 60_000;
// A fixed local wall-clock moment; postedAt is iPhone-local time without a zone, like the backend's.
const NOW = new Date(2026, 9, 7, 14, 30, 0).getTime();
const local = (ms: number) => {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
};

function note(postedMsAgo: number, over: Partial<PhoneNotification> = {}, preExisting = true): PhoneNotification {
  return {
    id: 1,
    appId: "com.apple.MobileSMS",
    appName: "Messages",
    category: "social",
    title: "Jane Doe",
    subtitle: "",
    message: "here now",
    postedAt: local(NOW - postedMsAgo),
    receivedAt: NOW,
    flags: { silent: false, important: false, preExisting, positiveAction: false, negativeAction: false },
    positiveLabel: "",
    negativeLabel: "",
    removedAt: null,
    live: true,
    fresh: true,
    ...over,
  };
}

describe("arrivedDuringGap", () => {
  const lostAt = NOW - 4 * min;

  it("is a replayed notification new to tug, posted after the link was lost and recent", () => {
    expect(arrivedDuringGap(note(2 * min), lostAt, NOW)).toBe(true);
  });

  it("isn't one tug already had (replayed again after a reconnect)", () => {
    expect(arrivedDuringGap(note(2 * min, { fresh: false }), lostAt, NOW)).toBe(false);
  });

  it("isn't a live arrival (those pop up the normal way)", () => {
    expect(arrivedDuringGap(note(0, {}, false), lostAt, NOW)).toBe(false);
  });

  it("isn't backlog from before the outage, allowing a minute of clock difference", () => {
    expect(arrivedDuringGap(note(10 * min), lostAt, NOW)).toBe(false);
    expect(arrivedDuringGap(note(4 * min + 30_000), lostAt, NOW)).toBe(true);
  });

  it("isn't older than ten minutes, however long the outage", () => {
    expect(arrivedDuringGap(note(11 * min), NOW - 60 * min, NOW)).toBe(false);
    expect(arrivedDuringGap(note(9 * min), NOW - 60 * min, NOW)).toBe(true);
  });

  it("is nothing without an outage seen this launch (the startup replay stays quiet)", () => {
    expect(arrivedDuringGap(note(1 * min), null, NOW)).toBe(false);
  });

  it("falls back to when tug received it when the phone sent no time", () => {
    expect(arrivedDuringGap(note(0, { postedAt: null, receivedAt: NOW - min }), lostAt, NOW)).toBe(true);
  });
});

describe("planGapPopups", () => {
  it("pops up a few one by one", () => {
    const items = Array.from({ length: GAP_POPUP_MAX }, (_, i) => note(min, { id: i }));
    expect(planGapPopups(items)).toEqual({ kind: "each", items });
  });

  it("sums up more than three in one pop-up", () => {
    const items = Array.from({ length: GAP_POPUP_MAX + 1 }, (_, i) => note(min, { id: i }));
    expect(planGapPopups(items)).toEqual({ kind: "summary", count: 4 });
  });

  it("says nothing for none", () => {
    expect(planGapPopups([])).toEqual({ kind: "each", items: [] });
  });
});

describe("gapSummaryText", () => {
  it("reads as end-user copy", () => {
    expect(gapSummaryText(4)).toBe("4 notifications arrived while your iPhone was reconnecting");
    expect(gapSummaryText(1)).toBe("1 notification arrived while your iPhone was reconnecting");
  });
});
