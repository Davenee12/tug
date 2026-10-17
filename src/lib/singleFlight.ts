// One press, one action: a button that talks to the phone (Answer, Decline, ✕ Clear) ignores
// further presses until its first one has been answered, and `busy` lets it show that.

import { ref, type Ref } from "vue";

export interface SingleFlight<A extends unknown[]> {
  /** Run the action unless a run is already out; resolves when that run has finished. */
  run: (...args: A) => Promise<void>;
  busy: Ref<boolean>;
}

export function singleFlight<A extends unknown[]>(action: (...args: A) => Promise<unknown>): SingleFlight<A> {
  const busy = ref(false);
  async function run(...args: A) {
    if (busy.value) return;
    busy.value = true;
    try {
      await action(...args);
    } finally {
      busy.value = false;
    }
  }
  return { run, busy };
}
