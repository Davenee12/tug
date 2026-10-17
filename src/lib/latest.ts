// Newest request wins. When the same view can ask for something again before the last answer
// came back (open album A then album B, switch a range twice, press Back), an older answer that
// lands late must not replace the newer one, clear its spinner, or reopen a view that was left.

import { ref, shallowRef, type Ref, type ShallowRef } from "vue";

export interface Latest {
  /** Start a request; the returned check stays true until a newer request starts or `cancel()`. */
  begin: () => () => boolean;
  /** Whatever is in flight no longer counts (the view it was for was left). */
  cancel: () => void;
}

export function latestOnly(): Latest {
  let current = 0;
  return {
    begin() {
      const mine = ++current;
      return () => mine === current;
    },
    cancel() {
      current++;
    },
  };
}

export interface PendingKey {
  /** A request for this key is out. */
  has: (key: string) => boolean;
  /** A request for `key` starts; call what's returned when it ends. */
  begin: (key: string) => () => void;
}

/**
 * Which key a request is out for, so the same thing isn't asked for twice at once. Each request
 * owns its mark: one that ends late clears only its own, never a newer request's for the same key
 * (a lookup for album A, then B, then A again: the first A ending must not clear the second's).
 */
export function pendingKey(): PendingKey {
  let current: { key: string } | null = null;
  return {
    has: (key) => current?.key === key,
    begin(key) {
      const mine = { key };
      current = mine;
      return () => {
        if (current === mine) current = null;
      };
    },
  };
}

export interface Loader<T> {
  /** What the newest load brought (null while loading, after `clear()`, or after a failure). */
  value: ShallowRef<T | null>;
  /** The newest load is still out. */
  loading: Ref<boolean>;
  /** Load anew; an older load still out stops counting. */
  load: (fetch: () => Promise<T>) => Promise<void>;
  /** Leave the view: nothing in flight lands, and the spinner goes. */
  clear: () => void;
}

/** One view's data with the newest-wins rule built in (the Spotify panel's album/artist/playlist). */
export function createLoader<T>(onError: (e: unknown) => void): Loader<T> {
  const value = shallowRef<T | null>(null);
  const loading = ref(false);
  const runs = latestOnly();
  async function load(fetch: () => Promise<T>) {
    const live = runs.begin();
    loading.value = true;
    value.value = null;
    try {
      const v = await fetch();
      if (live()) value.value = v;
    } catch (e) {
      if (live()) onError(e);
    } finally {
      if (live()) loading.value = false;
    }
  }
  function clear() {
    runs.cancel();
    value.value = null;
    loading.value = false;
  }
  return { value, loading, load, clear };
}
