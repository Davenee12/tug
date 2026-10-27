<script setup lang="ts">
// A small button with a menu of times (lib/reminders `choices`): Remind me on a row, Send later in
// the composer. Keyboard friendly (arrows move, Escape closes and returns focus), closes on a press
// outside, and stops every press so it never also acts on the row around it.
import { nextTick, onUnmounted, ref } from "vue";
import { choices, type Choice } from "../lib/reminders";

defineProps<{
  /** The menu's small heading ("Remind me", "Send later"). */
  heading: string;
  /** The button's tooltip. */
  title: string;
  disabled?: boolean;
  buttonClass?: string;
  /** Open above the button (the composer sits at the bottom of the window). */
  up?: boolean;
}>();
const emit = defineEmits<{ pick: [at: number, label: string] }>();

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const options = ref<Choice[]>([]);

function items(): HTMLElement[] {
  return [...(menu.value?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
}

async function toggle() {
  if (open.value) return close();
  options.value = choices(new Date());
  open.value = true;
  document.addEventListener("pointerdown", onOutside, true);
  await nextTick();
  items()[0]?.focus();
}

function close() {
  open.value = false;
  document.removeEventListener("pointerdown", onOutside, true);
}

function onOutside(e: PointerEvent) {
  if (root.value && !root.value.contains(e.target as Node)) close();
}

function onMenuKey(e: KeyboardEvent) {
  const list = items();
  const i = list.indexOf(document.activeElement as HTMLElement);
  if (e.key === "Escape") {
    e.preventDefault();
    close();
    root.value?.querySelector<HTMLElement>("[data-time-menu-toggle]")?.focus();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    list[(i + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length]?.focus();
  } else if (e.key === "Tab") {
    close();
  }
}

function pick(o: Choice) {
  close();
  emit("pick", o.at, o.label);
}

defineExpose({ close });
onUnmounted(() => document.removeEventListener("pointerdown", onOutside, true));
</script>

<template>
  <span ref="root" class="relative shrink-0" @click.stop @keydown.enter.stop @keydown.space.stop>
    <button
      type="button"
      data-time-menu-toggle
      aria-haspopup="menu"
      :aria-expanded="open"
      :title="title"
      :aria-label="title"
      :disabled="disabled"
      :class="buttonClass"
      @click="toggle"
    >
      <slot name="button" />
    </button>
    <div
      v-if="open"
      ref="menu"
      role="menu"
      :aria-label="heading"
      class="absolute right-0 z-30 w-60 rounded-xl border border-hairline bg-canvas p-1 shadow-lg"
      :class="up ? 'bottom-full mb-1' : 'top-full mt-1'"
      @keydown="onMenuKey"
    >
      <p class="px-3 pt-1.5 pb-1 text-[11px] font-semibold tracking-wide text-muted-soft uppercase">{{ heading }}</p>
      <button
        v-for="o in options"
        :key="o.label"
        type="button"
        role="menuitem"
        class="block w-full rounded-lg px-3 py-1.5 text-left text-[13px] text-ink hover:bg-surface-soft focus:bg-surface-soft focus:outline-none"
        @click="pick(o)"
      >
        {{ o.label }}
      </button>
      <slot name="extra" :close="close" />
    </div>
  </span>
</template>
