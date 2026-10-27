import { describe, expect, it } from "vitest";
import { clockTime } from "./format";
import type { Forecast, Quarter } from "./weather";
import {
  beaufort,
  beforeYouGo,
  compass,
  harborScene,
  localMinutes,
  moonName,
  moonPhase,
  nextRain,
  placeNow,
  quarterAfter,
  rainBars,
  seaLine,
  skyCaption,
  windOf,
} from "./weatherHarbor";

const at = (local: string) => Date.parse(`${local}:00Z`); // the place is in UTC below
const clock = (h: number, m: number) => clockTime(new Date(2026, 9, 5, h, m));

function forecast(over: Partial<Forecast["now"]> = {}, days: Partial<Forecast["days"][number]>[] = [{}]): Forecast {
  return {
    fetchedAt: 0,
    timezone: "UTC",
    now: { time: "2026-10-05T14:30", temp: 12, feels: 10, code: 0, day: true, wind: 10, windDir: 225, humidity: 70, ...over },
    hours: [],
    days: days.map((d, i) => ({
      date: `2026-10-0${5 + i}`,
      code: 0,
      high: 14,
      low: 8,
      rain: null,
      sunrise: `2026-10-0${5 + i}T07:00`,
      sunset: `2026-10-0${5 + i}T19:00`,
      ...d,
    })),
  };
}

describe("the harbor's wind", () => {
  it("is the Beaufort force a harbor would give", () => {
    expect([0, 1, 5.9, 6, 19, 20, 38, 39, 74, 75, 117, 118, 200].map(beaufort)).toEqual([0, 1, 1, 2, 3, 4, 5, 6, 8, 9, 11, 12, 12]);
  });

  it("blows the flag the way the real wind goes", () => {
    // From the west it pushes east; from the east, west; from the north, neither.
    expect(windOf(20, 270).push).toBe(1);
    expect(windOf(20, 90).push).toBe(-1);
    expect(windOf(20, 0).push).toBeCloseTo(0);
    expect(windOf(20, undefined)).toMatchObject({ push: 0, from: null });
  });

  it("says it in a harbor's words", () => {
    expect(compass(225)).toBe("southwest");
    expect(compass(-10)).toBe("north");
    expect(compass(359)).toBe("north");
    expect(seaLine(windOf(24, 225))).toBe("Force 4 · moderate breeze from the southwest");
    expect(seaLine(windOf(0.5, 225))).toBe("Force 0 · flat calm");
    expect(seaLine(windOf(70, null))).toBe("Force 8 · gale");
  });
});

describe("the harbor's moon", () => {
  it("follows the real lunar month", () => {
    // Eclipses pin it down: the lunar eclipse of 2022-11-08 (10:59 UTC) was at full moon, the
    // solar eclipse of 2024-04-08 (18:21 UTC) at new moon. The real moon wanders up to about half a
    // day either side of the average month.
    expect(Math.abs(moonPhase(Date.UTC(2022, 10, 8, 10, 59)) - 0.5)).toBeLessThan(0.03);
    const fresh = moonPhase(Date.UTC(2024, 3, 8, 18, 21));
    expect(Math.min(fresh, 1 - fresh)).toBeLessThan(0.03);
    expect(moonName(0.5)).toBe("Full moon");
    expect(moonName(0.26)).toBe("First quarter");
    expect(moonName(0.98)).toBe("New moon");
  });
});

describe("the harbor scene", () => {
  it("puts the sun where it really is between sunrise and sunset", () => {
    const noon = harborScene(forecast(), at("2026-10-05T13:00"));
    expect(noon.phase).toBe("day");
    expect(noon.sun?.at).toBeCloseTo(0.5);
    expect(noon.sun?.height).toBeCloseTo(1);
    expect(noon.moon).toBeNull();
    expect(harborScene(forecast(), at("2026-10-05T07:10")).phase).toBe("dawn");
    expect(harborScene(forecast(), at("2026-10-05T18:45")).phase).toBe("dusk");
  });

  it("brings the moon out at night, across the night to the next sunrise", () => {
    // 1 AM: 6 hours into the 12 from 7 PM to 7 AM, whether yesterday is still the first day (a
    // cached forecast) or today is (withCurrentHour dropped yesterday).
    const late = harborScene(forecast({ day: false }, [{}, {}]), at("2026-10-06T01:00"));
    expect(late.phase).toBe("night");
    expect(late.sun).toBeNull();
    expect(late.moon?.at).toBeCloseTo(0.5);
    const today = { ...forecast({ day: false }, [{}, {}]) };
    today.days = today.days.slice(1);
    expect(harborScene(today, at("2026-10-06T01:00")).moon?.at).toBeCloseTo(0.5);
    const evening = harborScene(forecast({ day: false }, [{}, {}]), at("2026-10-05T22:00"));
    expect(evening.moon?.at).toBeCloseTo(3 / 12);
    expect(harborScene(forecast({ day: false }), at("2026-10-05T19:20")).phase).toBe("dusk");
  });

  it("works from the place's own clock, not the PC's", () => {
    const f = { ...forecast(), timezone: "Asia/Tokyo" };
    // 04:00 UTC is 13:00 in Tokyo.
    expect(harborScene(f, Date.UTC(2026, 9, 5, 4, 0)).sun?.at).toBeCloseTo(0.5);
    expect(placeNow(Date.UTC(2026, 9, 5, 4, 0), "Asia/Tokyo")).toBe("2026-10-05T13:00");
    expect(placeNow(Date.UTC(2026, 9, 5, 4, 0), "Not/AZone")).toMatch(/^2026-10-0\dT\d\d:00$/);
  });

  it("copes with an older forecast that has no sunrise or sunset", () => {
    const old = forecast({ day: false }, [{ sunrise: undefined, sunset: undefined }]);
    const s = harborScene(old, at("2026-10-05T23:00"));
    expect(s.phase).toBe("night");
    expect(s.moon).not.toBeNull();
    expect(harborScene(forecast({}, [{ sunrise: undefined }]), at("2026-10-05T23:00")).sun).not.toBeNull();
  });

  it("rains, snows, fogs and storms as hard as the weather says", () => {
    const t = at("2026-10-05T13:00");
    expect(harborScene(forecast({ code: 0 }), t)).toMatchObject({ rain: 0, snow: 0, fog: false, storm: false });
    expect(harborScene(forecast({ code: 61 }), t).rain).toBeCloseTo(0.4);
    // Measured rain lifts light rain into a downpour, but never invents it.
    expect(harborScene(forecast({ code: 61, precip: 0.9 }), t).rain).toBeCloseTo(0.9);
    expect(harborScene(forecast({ code: 2, precip: 3 }), t).rain).toBe(0);
    expect(harborScene(forecast({ code: 75 }), t)).toMatchObject({ rain: 0, snow: 0.9 });
    expect(harborScene(forecast({ code: 45 }), t).fog).toBe(true);
    expect(harborScene(forecast({ code: 95 }), t).storm).toBe(true);
  });

  it("takes cloud cover as measured, or from the sky when it isn't", () => {
    const t = at("2026-10-05T13:00");
    expect(harborScene(forecast({ clouds: 35 }), t).clouds).toBeCloseTo(0.35);
    expect(harborScene(forecast({ code: 3 }), t).clouds).toBeCloseTo(0.9);
    expect(harborScene(forecast({ code: 0 }), t).clouds).toBeCloseTo(0.1);
  });

  it("feels the gusts, not only the average", () => {
    const t = at("2026-10-05T13:00");
    expect(harborScene(forecast({ wind: 10, gusts: 60 }), t).wind.force).toBe(beaufort(42));
    expect(harborScene(forecast({ wind: 30, gusts: 31 }), t).wind.force).toBe(beaufort(30));
  });
});

describe("the harbor's caption", () => {
  it("gives the sunset by day, and the moon and the next sunrise by night", () => {
    const f = forecast({}, [{}, {}]);
    expect(skyCaption(harborScene(f, at("2026-10-05T13:00")), f, "2026-10-05T13:00")).toBe(`Sunset ${clock(19, 0)}`);
    const evening = skyCaption(harborScene(f, at("2026-10-05T22:00")), f, "2026-10-05T22:00");
    expect(evening).toMatch(new RegExp(`^(New|Waxing|First|Full|Waning|Last).* · Sunrise ${clock(7, 0)}$`));
    expect(skyCaption(harborScene(f, at("2026-10-05T03:00")), f, "2026-10-05T03:00")).toMatch(/Sunrise/);
  });
});

describe("rain to the quarter-hour", () => {
  const now = "2026-10-05T14:37";
  const q = (mm: number[]): Quarter[] => mm.map((precip, i) => ({ time: quarterAfter(now, i * 15), precip }));

  it("says when it starts and how long it lasts", () => {
    const r = nextRain(q([0, 0, 0.3, 0.5, 0, 0, 0, 0]), now);
    expect(r.start).toBe(clock(15, 0));
    expect(r.line).toBe(`Rain from about ${clock(15, 0)}, for about 30 minutes`);
    expect(nextRain(q([0, 0, 0, 0, 0, 0, 0.2, 0.2]), now).line).toBe(`Rain from about ${clock(16, 0)}`);
    expect(nextRain(q([0, 0.2, 0.2, 0.2, 0.2, 0.2, 0, 0]), now, "Snow").line).toBe(`Snow from about ${clock(14, 45)}, for about an hour`);
  });

  it("says when rain that's falling eases off", () => {
    expect(nextRain(q([0.4, 0.4, 0, 0, 0, 0, 0, 0]), now)).toMatchObject({ start: "now", line: `Rain easing off around ${clock(15, 0)}` });
    expect(nextRain(q([0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4]), now).line).toBe("Rain for the next 2 hours or more");
    expect(nextRain(q([0.4, 0.4, 0.4]), now).line).toBe("Rain for the next 45 minutes or more");
  });

  it("stays quiet when it's dry or there's nothing to go on", () => {
    expect(nextRain(q([0, 0.05, 0, 0, 0, 0, 0, 0]), now)).toMatchObject({ line: null, start: null });
    expect(nextRain(undefined, now).line).toBeNull();
    expect(nextRain(q([0.5]), now).line).toBeNull();
  });

  it("drops the quarters that have passed since the forecast came", () => {
    const later = "2026-10-05T15:05";
    const r = nextRain(q([0.5, 0.5, 0, 0, 0, 0, 0, 0]), later);
    expect(r.quarters[0].time).toBe("2026-10-05T15:00");
    expect(r.line).toBeNull();
  });

  it("draws the strip from light to heavy", () => {
    const bars = rainBars(q([0, 0.05, 0.5, 5, 20]));
    [0, 0, 0.3 + 0.5 / 1.5, 1, 1].forEach((want, i) => expect(bars[i]).toBeCloseTo(want));
  });
});

describe("before you head out", () => {
  const base = { feelsC: 10, feelsLabel: "50°" };

  it("puts rain on the way and a low phone together", () => {
    expect(beforeYouGo({ ...base, battery: 18, start: "5:45 PM" })).toBe("Your phone's at 18%. Charge it before the rain at 5:45 PM.");
    expect(beforeYouGo({ ...base, battery: 30, start: "now", what: "Snow" })).toBe("Your phone's at 30%. Charge it before you head out in the snow.");
  });

  it("warns about the cold, which drains batteries", () => {
    expect(beforeYouGo({ battery: 25, start: null, feelsC: -4, feelsLabel: "25°" })).toBe(
      "It feels like 25° out and your phone's at 25%. Cold drains batteries faster, so charge it before you go.",
    );
  });

  it("says nothing when the phone is fine, unknown, or the weather is", () => {
    expect(beforeYouGo({ ...base, battery: 31, start: "now" })).toBeNull();
    expect(beforeYouGo({ ...base, battery: null, start: "now" })).toBeNull();
    expect(beforeYouGo({ ...base, battery: 10, start: null })).toBeNull();
  });
});

describe("place-local minutes", () => {
  it("does arithmetic across midnight", () => {
    expect(localMinutes("2026-10-06T00:10")! - localMinutes("2026-10-05T23:50")!).toBe(20);
    expect(localMinutes("nonsense")).toBeNull();
    expect(quarterAfter("2026-10-05T23:50", 15)).toBe("2026-10-06T00:00");
  });
});
