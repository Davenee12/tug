<script setup lang="ts">
// Remind me (stores/reminders): a row's small alarm-clock button and its menu of times. With a
// reminder set it shows when ("3:30 PM"), and the menu can change or cancel it.
import { computed } from "vue";
import { AlarmClock } from "lucide-vue-next";
import { whenLabel } from "../lib/reminders";
import { useRemindersStore, type RemindTarget } from "../stores/reminders";
import { useTugStore } from "../stores/tug";
import TimeMenu from "./TimeMenu.vue";

const props = defineProps<{ target: RemindTarget }>();
const reminders = useRemindersStore();
const tug = useTugStore();

const pending = computed(() => reminders.forTarget(props.target));
const pendingLabel = computed(() => (pending.value ? whenLabel(pending.value.at, Date.now()) : null));

async function pick(at: number, label: string) {
  await reminders.remind(props.target, at);
  tug.notify("info", `tug will remind you ${label.charAt(0).toLowerCase()}${label.slice(1)}.`);
}

function cancel(close: () => void) {
  close();
  if (!pending.value) return;
  reminders.cancel(pending.value.id);
  tug.notify("info", "Reminder cancelled.");
}
</script>

<template>
  <TimeMenu
    heading="Remind me"
    :title="pendingLabel ? `Reminder at ${pendingLabel}. Change or cancel it` : 'Remind me later'"
    :button-class="[
      'flex items-center gap-1 rounded-md p-1 text-[11px] active:bg-surface-card',
      pendingLabel ? 'bg-surface-soft px-1.5 font-medium text-body-strong' : 'text-muted-soft',
    ].join(' ')"
    @pick="pick"
  >
    <template #button>
      <AlarmClock :size="14" />
      <span v-if="pendingLabel">{{ pendingLabel }}</span>
    </template>
    <template #extra="{ close }">
      <button
        v-if="pending"
        type="button"
        role="menuitem"
        class="mt-1 block w-full rounded-lg border-t border-hairline-soft px-3 py-1.5 text-left text-[13px] text-error hover:bg-surface-soft focus:bg-surface-soft focus:outline-none"
        @click="cancel(close)"
      >
        Cancel reminder
      </button>
    </template>
  </TimeMenu>
</template>
