// The weather card's "or type a city" box: searches as you type (after a short pause), shows only
// the newest search's results, and says in plain words when the search can't be reached. Once a
// search works again, that message goes away rather than sit under good results.

import { getCurrentScope, onScopeDispose, ref, watch } from "vue";
import { CITY_SEARCH_ERROR, searchPlaces, type Place } from "./weather";

export function useCitySearch(search: (query: string) => Promise<Place[]> = searchPlaces, delayMs = 250) {
  const query = ref("");
  const results = ref<Place[]>([]);
  /** Plain words for a search that couldn't be reached (never the browser's "Failed to fetch"). */
  const error = ref<string | null>(null);
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  watch(query, (q) => {
    clearTimeout(timer);
    if (q.trim().length < 2) {
      seq++; // a search still out for the longer text mustn't land now
      results.value = [];
      error.value = null;
      return;
    }
    timer = setTimeout(async () => {
      const mine = ++seq;
      try {
        const r = await search(q);
        if (mine !== seq) return;
        results.value = r;
        error.value = null;
      } catch {
        if (mine === seq) error.value = CITY_SEARCH_ERROR;
      }
    }, delayMs);
  });

  /** A city was picked (or the box is done with): empty it and forget any search still out. */
  function reset() {
    clearTimeout(timer);
    seq++;
    query.value = "";
    results.value = [];
    error.value = null;
  }

  if (getCurrentScope()) onScopeDispose(() => clearTimeout(timer));
  return { query, results, error, reset };
}
