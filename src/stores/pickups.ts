// Pickups saved (lib/pickups): the count lives here, loaded from and saved to tug's settings on
// this PC. The main store calls `count` where something was done from the PC instead of the phone.

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { api } from "../lib/ipc";
import { EMPTY, parsePickups, pickupsLine, record, totalLine, type PickupKind, type Pickups } from "../lib/pickups";

export const PICKUPS_SETTING = "ui.pickups";

export const usePickupsStore = defineStore("pickups", () => {
  const state = ref<Pickups>(EMPTY);
  /** Bumped by `count` so the lines below follow the clock to a new week without a timer. */
  const now = ref(Date.now());
  let loaded: Promise<void> | null = null;
  /** Only once the saved count was read: a failed read must never overwrite it with a fresh one. */
  let canSave = false;

  function load(): Promise<void> {
    // Through a resolved promise, so even a missing settings API (tests, an older backend) only
    // means nothing is saved, never an error.
    loaded ??= Promise.resolve()
      .then(() => api.getSettings())
      .then((raw) => {
        state.value = parsePickups(raw[PICKUPS_SETTING]);
        canSave = true;
      })
      .catch(() => undefined);
    return loaded;
  }

  /** Something was just done from the PC instead of the phone (counted once per burst). */
  async function count(kind: PickupKind) {
    await load();
    now.value = Date.now();
    const before = state.value.total;
    state.value = record(state.value, kind, now.value);
    if (canSave && state.value.total !== before) {
      void api.setSetting(PICKUPS_SETTING, JSON.stringify(state.value)).catch(() => undefined);
    }
  }

  const greetingLine = computed(() => pickupsLine(state.value, Math.max(now.value, Date.now())));
  const aboutLine = computed(() => totalLine(state.value));

  return { state, load, count, greetingLine, aboutLine };
});
