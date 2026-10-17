import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { useCitySearch } from "./useCitySearch";
import { CITY_SEARCH_ERROR, type Place } from "./weather";

const place = (name: string): Place => ({ name, latitude: 1, longitude: 2 });

/** Searches that resolve or fail when the test says, in the order they were asked for. */
function deferredSearch() {
  const calls: Array<{ q: string; resolve: (p: Place[]) => void; reject: (e: unknown) => void }> = [];
  const search = (q: string) => new Promise<Place[]>((resolve, reject) => calls.push({ q, resolve, reject }));
  return { calls, search };
}

async function type(box: ReturnType<typeof useCitySearch>, text: string) {
  box.query.value = text;
  await nextTick();
  vi.advanceTimersByTime(250);
}
/** Let the settled search's continuation run (microtasks; timers are faked). */
const settle = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

describe("useCitySearch", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] }));
  afterEach(() => vi.useRealTimers());

  it("says it in plain words when the search can't be reached, not the browser's error", async () => {
    const { calls, search } = deferredSearch();
    const box = useCitySearch(search);
    await type(box, "Portland");
    calls[0].reject(new TypeError("Failed to fetch"));
    await settle();
    expect(box.error.value).toBe(CITY_SEARCH_ERROR);
    expect(box.error.value).not.toMatch(/fetch/i);
  });

  it("drops the error once a search works again", async () => {
    const { calls, search } = deferredSearch();
    const box = useCitySearch(search);
    await type(box, "Portland");
    calls[0].reject(new TypeError("Failed to fetch"));
    await settle();
    await type(box, "Portland, Maine");
    calls[1].resolve([place("Portland, Maine")]);
    await settle();
    expect(box.error.value).toBeNull();
    expect(box.results.value.map((p) => p.name)).toEqual(["Portland, Maine"]);
  });

  it("shows only the newest search's results", async () => {
    const { calls, search } = deferredSearch();
    const box = useCitySearch(search);
    await type(box, "Par");
    await type(box, "Paris");
    calls[1].resolve([place("Paris")]);
    calls[0].resolve([place("Parma")]);
    await settle();
    expect(box.results.value.map((p) => p.name)).toEqual(["Paris"]);
  });

  it("a search still out when the box is emptied doesn't land", async () => {
    const { calls, search } = deferredSearch();
    const box = useCitySearch(search);
    await type(box, "Paris");
    box.reset();
    calls[0].resolve([place("Paris")]);
    await settle();
    expect(box.results.value).toEqual([]);
  });
});
