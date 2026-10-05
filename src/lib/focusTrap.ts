// Keep keyboard focus inside a modal dialog: Tab/Shift+Tab cycle within it,
// focus returns to where it was when the dialog closes, optional Escape handler.

import { onMounted, onUnmounted, type Ref } from "vue";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function useFocusTrap(root: Ref<HTMLElement | null>, onEscape?: () => void) {
  let previous: HTMLElement | null = null;

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape" && onEscape) {
      e.preventDefault();
      onEscape();
      return;
    }
    const el = root.value;
    if (e.key !== "Tab" || !el) return;
    const items = Array.from(el.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((x) => x.offsetParent !== null);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    const inside = el.contains(document.activeElement);
    if (!inside || (e.shiftKey && document.activeElement === first)) {
      e.preventDefault();
      (e.shiftKey ? last : first).focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  onMounted(() => {
    previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.addEventListener("keydown", onKey, true);
  });
  onUnmounted(() => {
    document.removeEventListener("keydown", onKey, true);
    previous?.focus();
  });
}
