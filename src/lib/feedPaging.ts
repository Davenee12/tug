// Infinite scroll for the Feed. The Feed shows only what's still waiting, but older history comes in
// a page at a time with cleared notifications mixed in, so a page can add nothing to the screen. An
// IntersectionObserver only speaks when the end of the list crosses into or out of view, so after a
// page like that the end is still in view, nothing fires, and the Feed sat on "You're all caught
// up" with more history behind it. This keeps paging while the end is in view and there's more.

import { ref, type Ref } from "vue";

export interface FeedPagerOptions {
  /** Load the next page; true if one came in (false: failed, or nothing left). */
  load: () => Promise<boolean>;
  /** There's more history and the end of the list is on screen, measured now. */
  canLoad: () => boolean;
  /** Wait for the page to be on screen (the DOM updated) before measuring again. */
  settle: () => Promise<void>;
}

export interface FeedPager {
  /** The end of the list came into view (or might have): page until it's out of view or done. */
  kick: () => Promise<void>;
  /** A run of pages is in progress. */
  busy: Ref<boolean>;
}

export function createFeedPager({ load, canLoad, settle }: FeedPagerOptions): FeedPager {
  const busy = ref(false);
  async function kick() {
    if (busy.value) return; // the run in progress measures again after each page
    busy.value = true;
    try {
      while (canLoad()) {
        // A failed page stops the run; the next time the end comes into view tries again.
        if (!(await load())) break;
        await settle();
      }
    } finally {
      busy.value = false;
    }
  }
  return { kick, busy };
}
