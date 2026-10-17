import { describe, expect, it } from "vitest";
import { createFeedPager } from "./feedPaging";

/**
 * A Feed whose pages come in one by one: `visibleAfter` is how many pages it takes before something
 * still waiting pushes the end of the list off screen (pages before that were all cleared).
 */
function feed({ pages, visibleAfter, failAt }: { pages: number; visibleAfter: number; failAt?: number }) {
  let loaded = 0;
  const pager = createFeedPager({
    load: async () => {
      if (loaded === failAt) return false;
      loaded++;
      return true;
    },
    canLoad: () => loaded < pages && loaded < visibleAfter,
    settle: async () => {},
  });
  return { pager, loaded: () => loaded };
}

describe("Feed paging", () => {
  it("keeps going past pages that were all cleared, while the end is still on screen", async () => {
    const f = feed({ pages: 10, visibleAfter: 4 });
    await f.pager.kick();
    expect(f.loaded()).toBe(4);
    expect(f.pager.busy.value).toBe(false);
  });

  it("stops when there's no more history", async () => {
    const f = feed({ pages: 2, visibleAfter: 99 });
    await f.pager.kick();
    expect(f.loaded()).toBe(2);
  });

  it("stops at a page that failed (the next time the end comes into view tries again)", async () => {
    const f = feed({ pages: 10, visibleAfter: 99, failAt: 1 });
    await f.pager.kick();
    expect(f.loaded()).toBe(1);
  });

  it("one run at a time: a second kick while paging doesn't start another", async () => {
    let calls = 0;
    let release!: () => void;
    const pager = createFeedPager({
      load: () => new Promise<boolean>((r) => ((calls++, (release = () => r(true))))),
      canLoad: () => calls < 1,
      settle: async () => {},
    });
    const run = pager.kick();
    expect(pager.busy.value).toBe(true);
    await pager.kick();
    expect(calls).toBe(1);
    release();
    await run;
    expect(pager.busy.value).toBe(false);
  });
});
