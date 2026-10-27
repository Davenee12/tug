<script setup lang="ts">
// Remind me (stores/reminders): a row's small alarm-clock button and its menu of times. With a
// reminder set it shows when ("3:30 PM"), and the menu can change or cancel it. Every press stops
// here, so it never also opens or expands the row it sits in.
import { computed, nextTick, onUnmounted, ref } from "vue";
import { AlarmClock } from "lucide-vue-next";
import { choices, whenLabel } from "../lib/reminders";
import { useRemindersStore, type RemindTarget } from "../stores/reminders";
import { useTugStore } from "../stores/tug";

const props = defineProps<{ target: RemindTarget }>();
const reminders = useRemindersStore();
const tug = useTugStore();

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const options = ref<{ label: string; at: number }[]>([]);
const pending = computed(() => reminders.forTarget(props.target));
const pendingLabel = computed(() => (pending.value ? whenLabel(pending.value.at, Date.now()) : null));

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
    root.value?.querySelector<HTMLElement>("[data-remind-toggle]")?.focus();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const next = (i + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length;
    list[next]?.focus();
  } else if (e.key === "Tab") {
    close();
  }
}

async function pick(at: number, label: string) {
  close();
  await reminders.remind(props.target, at);
  tug.notify("info", `tug will remind you ${label.charAt(0).toLowerCase()}${label.slice(1)}.`);
}

function cancel() {
  if (!pending.value) return;
  reminders.cancel(pending.value.id);
  close();
  tug.notify("info", "Reminder cancelled.");
}

onUnmounted(() => document.removeEventListener("pointerdown", onOutside, true));
</script>

<template>
  <span ref="root" class="relative shrink-0" @click.stop @keydown.enter.stop @keydown.space.stop>
    <button
      type="button"
      data-remind-toggle
      aria-haspopup="menu"
      :aria-expanded="open"
      :title="pendingLabel ? `Reminder at ${pendingLabel}. Change or cancel it` : 'Remind me later'"
      :class="[
        'flex items-center gap-1 rounded-md p-1 text-[11px] active:bg-surface-card',
        pendingLabel ? 'bg-surface-soft px-1.5 font-medium text-body-strong' : 'text-muted-soft',
      ]"
      @click="toggle"
    >
      <AlarmClock :size="14" />
      <span v-if="pendingLabel">{{ pendingLabel }}</span>
    </button>
    <div
      v-if="open"
      ref="menu"
      role="menu"
      aria-label="Remind me"
      class="absolute top-full right-0 z-30 mt-1 w-60 rounded-xl border border-hairline bg-canvas p-1 shadow-lg"
      @keydown="onMenuKey"
    >
      <p class="px-3 pt-1.5 pb-1 text-[11px] font-semibold tracking-wide text-muted-soft uppercase">Remind me</p>
      <button
        v-for="o in options"
        :key="o.label"
        type="button"
        role="menuitem"
        class="block w-full rounded-lg px-3 py-1.5 text-left text-[13px] text-ink hover:bg-surface-soft focus:bg-surface-soft focus:outline-none"
        @click="pick(o.at, o.label)"
      >
        {{ o.label }}
      </button>
      <button
        v-if="pending"
        type="button"
        role="menuitem"
        class="mt-1 block w-full rounded-lg border-t border-hairline-soft px-3 py-1.5 text-left text-[13px] text-error hover:bg-surface-soft focus:bg-surface-soft focus:outline-none"
        @click="cancel"
      >
        Cancel reminder
      </button>
    </div>
  </span>
</template>
