// Keep keyboard focus inside a modal dialog: Tab/Shift+Tab cycle within it,
// focus returns to where it was when the dialog closes, optional Escape handler.
// Dialogs can stack (a pairing request over search); only the top one acts.

import { onMounted, onUnmounted, type Ref } from "vue";

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Open traps, oldest first; the last one owns the keyboard. */
const stack: symbol[] = [];

export function useFocusTrap(root: Ref<HTMLElement | null>, onEscape?: () => void) {
  const id = Symbol("focus-trap");
  let previous: HTMLElement | null = null;

  const onKey = (e: KeyboardEvent) => {
    if (stack[stack.length - 1] !== id) return;
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
    stack.push(id);
    document.addEventListener("keydown", onKey, true);
  });
  onUnmounted(() => {
    const i = stack.indexOf(id);
    if (i >= 0) stack.splice(i, 1);
    document.removeEventListener("keydown", onKey, true);
    previous?.focus();
  });
}
