import { nextTick, onMounted, onUnmounted, ref, useTemplateRef, watch, type Ref } from "vue";

/**
 * A long list rendered a page at a time: `limit` starts at `page` and grows by a page whenever the
 * element with template ref `sentinelRef` (an empty element after the last row) comes within a
 * screen of the bottom of `root` (the list's scroller). Opening the list then builds one page, not
 * every row; scrolling, or tabbing down through the rows, brings the rest in before they're reached.
 */
export function useRevealMore(root: Ref<HTMLElement | null>, sentinelRef: string, page: number) {
  const limit = ref(page);
  const sentinel = useTemplateRef<HTMLElement>(sentinelRef);
  let observer: IntersectionObserver | undefined;

  onMounted(() => {
    observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        limit.value += page;
        // Still in view after the new rows (a tall window): look again once they're in.
        void nextTick(() => {
          const el = sentinel.value;
          if (el && observer) {
            observer.unobserve(el);
            observer.observe(el);
          }
        });
      },
      { root: root.value, rootMargin: "0px 0px 100% 0px" },
    );
    watch(
      sentinel,
      (el, old) => {
        if (old) observer?.unobserve(old);
        if (el) observer?.observe(el);
      },
      { immediate: true },
    );
  });
  onUnmounted(() => observer?.disconnect());

  return limit;
}
