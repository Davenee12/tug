import { describe, expect, it } from "vitest";
import { clockTime } from "./format";
import type { Quarter } from "./weather";
import { quarterAfter } from "./weatherHarbor";
import { rainAlert } from "./rainAlert";

const NOW = "2026-10-05T17:20";
const clock = (h: number, m: number) => clockTime(new Date(2026, 9, 5, h, m));
/** Quarters from the current one (17:15) on, one mm value each. */
const q = (mm: number[]): Quarter[] => mm.map((precip, i) => ({ time: quarterAfter(NOW, i * 15), precip }));
const base = { nowLocal: NOW, what: "Rain" as const, place: "Portland, Oregon", battery: 80, lastSpell: null };

describe("rain soon", () => {
  it("says so shortly before rain starts", () => {
    // Dry now (17:15), wet from 17:30: ten minutes away.
    const a = rainAlert({ ...base, quarters: q([0, 0.4, 0.6, 0, 0, 0, 0, 0]) });
    expect(a).toEqual({
      title: "Rain soon in Portland",
      body: `Rain from about ${clock(17, 30)}, for about 30 minutes.`,
      spell: "2026-10-05T17:30",
    });
  });

  it("waits until it's within twenty minutes", () => {
    // Wet from 18:00: forty minutes away.
    expect(rainAlert({ ...base, quarters: q([0, 0, 0, 0.5, 0.5, 0, 0, 0]) })).toBeNull();
  });

  it("says nothing when it's already raining or the next two hours are dry", () => {
    expect(rainAlert({ ...base, quarters: q([0.5, 0.5, 0, 0, 0, 0, 0, 0]) })).toBeNull();
    expect(rainAlert({ ...base, quarters: q([0, 0, 0, 0, 0, 0, 0, 0]) })).toBeNull();
    expect(rainAlert({ ...base, quarters: undefined })).toBeNull();
  });

  it("tells about a spell once, and not again for rain soon after it", () => {
    const quarters = q([0, 0.4, 0.6, 0, 0, 0, 0, 0]);
    expect(rainAlert({ ...base, quarters, lastSpell: "2026-10-05T17:30" })).toBeNull();
    // An earlier spell an hour ago: this is the same weather coming back.
    expect(rainAlert({ ...base, quarters, lastSpell: "2026-10-05T16:30" })).toBeNull();
    // Yesterday's: a new spell.
    expect(rainAlert({ ...base, quarters, lastSpell: "2026-10-04T17:30" })?.spell).toBe("2026-10-05T17:30");
  });

  it("adds the phone's battery when it's low, and says snow for snow", () => {
    const a = rainAlert({ ...base, what: "Snow", battery: 18, quarters: q([0, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4]) });
    expect(a?.title).toBe("Snow soon in Portland");
    expect(a?.body).toBe(`Snow from about ${clock(17, 30)}. Your phone's at 18%: charge it before you head out.`);
  });
});
