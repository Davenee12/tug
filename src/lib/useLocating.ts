import { onUnmounted, ref } from "vue";
import { FOUND, FOUND_MS, LINE_MS, locatingLines } from "./locating";

/**
 * The words on screen while "Use my location" works: a line straight away, the next every
 * LINE_MS, and "There you are" for a beat once it's found. Only the presentation: the search
 * itself is the caller's.
 */
export function useLocating() {
  const line = ref<string | null>(null);
  const found = ref(false);
  let timer: number | undefined;

  function start() {
    const lines = locatingLines();
    let i = 0;
    found.value = false;
    line.value = lines[0];
    window.clearInterval(timer);
    timer = window.setInterval(() => {
      i = Math.min(i + 1, lines.length - 1);
      line.value = lines[i];
    }, LINE_MS);
  }

  /** Found: say so, briefly, however far the lines got. */
  async function succeed() {
    window.clearInterval(timer);
    found.value = true;
    line.value = FOUND;
    await new Promise((resolve) => window.setTimeout(resolve, FOUND_MS));
  }

  function stop() {
    window.clearInterval(timer);
    line.value = null;
    found.value = false;
  }

  onUnmounted(stop);
  return { line, found, start, succeed, stop };
}
