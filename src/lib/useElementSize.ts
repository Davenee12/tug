// An element's own size in layout pixels. Unlike window.innerWidth/innerHeight and media queries,
// this follows tug's CSS zoom (lib/layout). A ResizeObserver catches the element resizing; it
// doesn't report a zoom change (Chromium hands it zoom-adjusted sizes), so a zoom change measures
// again too, as does a window resize.

import { onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { currentZoom } from "./zoom";

export function useElementSize(el: Ref<HTMLElement | null>, fallback: { width: number; height: number }) {
  const width = ref(fallback.width);
  const height = ref(fallback.height);
  let ro: ResizeObserver | undefined;
  const measure = () => {
    const e = el.value;
    if (!e) return;
    // clientWidth/Height are in the element's own (zoomed) CSS pixels.
    width.value = e.clientWidth;
    height.value = e.clientHeight;
  };
  watch(currentZoom, measure, { flush: "post" });
  onMounted(() => {
    measure();
    ro = new ResizeObserver(measure);
    if (el.value) ro.observe(el.value);
    window.addEventListener("resize", measure);
  });
  onUnmounted(() => {
    ro?.disconnect();
    window.removeEventListener("resize", measure);
  });
  return { width, height };
}
