import { describe, expect, it } from "vitest";
import { replyChoices, replyNumber } from "./replyNumber";

const MOBILE = "+13025550100";
const WORK = "+13025550111";

describe("replyNumber", () => {
  it("defaults to the conversation's own number (whoever texted last)", () => {
    expect(replyNumber({ address: MOBILE, addresses: [MOBILE, WORK] }, undefined)).toBe(MOBILE);
    expect(replyNumber({ address: null, addresses: [MOBILE, WORK] }, undefined)).toBeNull();
  });

  it("goes to the number picked on purpose, not the default", () => {
    // The bug: New message › "John · work" opened John's conversation and sent to his mobile.
    expect(replyNumber({ address: MOBILE, addresses: [MOBILE, WORK] }, WORK)).toBe(WORK);
  });

  it("goes to a number the conversation hasn't seen yet", () => {
    // The bug: Text on a call from another number saved under the same name went to the old one,
    // and with only one number in the conversation there was no To: box to show it.
    expect(replyNumber({ address: MOBILE, addresses: [MOBILE] }, WORK)).toBe(WORK);
  });
});

describe("replyChoices", () => {
  it("offers the conversation's numbers", () => {
    expect(replyChoices([MOBILE, WORK], undefined)).toEqual([MOBILE, WORK]);
    expect(replyChoices([MOBILE, WORK], WORK)).toEqual([MOBILE, WORK]);
  });

  it("adds a number asked for that isn't among them, so the To: box shows where the text goes", () => {
    expect(replyChoices([MOBILE], WORK)).toEqual([WORK, MOBILE]);
    expect(replyChoices([], WORK)).toEqual([WORK]);
  });
});
