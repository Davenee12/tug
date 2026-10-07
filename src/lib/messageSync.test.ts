import { describe, expect, it } from "vitest";
import { byArrival, EVENT_GRACE_MS, missingMessages, RESYNC_AFTER_MS, settledMessages, shouldResync } from "./messageSync";
import type { SmsMessage } from "../types/protocol";

const T0 = Date.parse("2026-10-05T21:49:00");

function sms(id: number, direction: "in" | "out", receivedAt: number, extra: Partial<SmsMessage> = {}): SmsMessage {
  return {
    id,
    source: "iphone-map",
    direction,
    address: "+13025550142",
    contactName: "Zoe",
    body: direction === "in" ? "omw " : "see you soon",
    sentAt: null,
    receivedAt,
    status: direction === "in" ? "received" : "accepted",
    msgType: direction === "in" ? "SMS_GSM" : null,
    ...extra,
  };
}

describe("missingMessages", () => {
  it("returns only the texts the window doesn't have", () => {
    const have = [sms(1, "in", T0), sms(2, "out", T0 + 5_000)];
    const reply = sms(3, "out", T0 + 9_000);
    expect(missingMessages(have, [have[0], reply, have[1]])).toEqual([reply]);
  });

  it("never overwrites a text the window has, even with a different copy", () => {
    // A live event already moved the reply on to "sent"; the press carried the "accepted" copy.
    const have = [sms(3, "out", T0, { status: "sent" })];
    expect(missingMessages(have, [sms(3, "out", T0, { status: "accepted" })])).toEqual([]);
  });

  it("adds each missing text once", () => {
    const reply = sms(3, "out", T0);
    expect(missingMessages([], [reply, { ...reply }])).toEqual([reply]);
  });

  it("finds nothing missing in an empty re-read", () => {
    expect(missingMessages([sms(1, "in", T0)], [])).toEqual([]);
  });
});

describe("settledMessages", () => {
  it("leaves a text stored moments ago to its own event", () => {
    const now = T0 + 60_000;
    const old = sms(1, "in", now - EVENT_GRACE_MS);
    const fresh = sms(2, "out", now - EVENT_GRACE_MS + 1);
    expect(settledMessages([old, fresh], now)).toEqual([old]);
  });
});

describe("byArrival", () => {
  it("orders oldest first, ties by id, like list_messages", () => {
    const list = [sms(5, "out", T0 + 2), sms(4, "in", T0 + 2), sms(9, "in", T0)];
    expect(list.sort(byArrival).map((m) => m.id)).toEqual([9, 4, 5]);
  });
});

describe("shouldResync", () => {
  it("re-reads only after being away a while", () => {
    expect(shouldResync(null, T0)).toBe(false);
    expect(shouldResync(T0, T0 + RESYNC_AFTER_MS - 1)).toBe(false);
    expect(shouldResync(T0, T0 + RESYNC_AFTER_MS)).toBe(true);
    expect(shouldResync(T0, T0 + 10 * RESYNC_AFTER_MS)).toBe(true);
  });

  it("takes a custom threshold", () => {
    expect(shouldResync(T0, T0 + 500, 1_000)).toBe(false);
    expect(shouldResync(T0, T0 + 1_000, 1_000)).toBe(true);
  });
});
