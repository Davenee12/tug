import { onScopeDispose, ref, watch, type Ref } from "vue";

/**
 * `Date.now()`, refreshed every `periodMs`, but only while `active()` holds and the window is on
 * screen. Hidden in the tray or minimised, or with nothing on screen that moves, no timer runs at
 * all; it catches up the moment it's needed again. Use it in the small component that shows the
 * time, never in a store: a tick then re-renders just that component, not everything that reads
 * the store.
 */
export function useNow(active: () => boolean, periodMs: number): Readonly<Ref<number>> {
  const now = ref(Date.now());
  let timer: number | undefined;
  const sync = () => {
    const on = active() && document.visibilityState === "visible";
    if (on && timer === undefined) {
      now.value = Date.now();
      timer = window.setInterval(() => (now.value = Date.now()), periodMs);
    } else if (!on && timer !== undefined) {
      window.clearInterval(timer);
      timer = undefined;
    }
  };
  watch(active, sync, { immediate: true });
  document.addEventListener("visibilitychange", sync);
  onScopeDispose(() => {
    window.clearInterval(timer);
    timer = undefined;
    document.removeEventListener("visibilitychange", sync);
  });
  return now;
}
