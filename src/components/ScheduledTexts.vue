<script setup lang="ts">
// Send later (stores/scheduledTexts): a conversation's texts waiting to go, above its composer, each
// with Send now and Cancel. One that couldn't go in time (held) says when it was due and offers
// Send now or Discard; tug never sends a late text by itself.
import { computed } from "vue";
import { Clock, TriangleAlert } from "lucide-vue-next";
import { whenLabel } from "../lib/reminders";
import { useScheduledTextsStore } from "../stores/scheduledTexts";
import { useTugStore } from "../stores/tug";

const props = defineProps<{ threadKey: string }>();
const scheduled = useScheduledTextsStore();
const tug = useTugStore();
const items = computed(() => scheduled.inThread(props.threadKey));
const canSend = computed(() => tug.status.services.messages);

/** "Sends at 6:00 PM", "Sends tomorrow 9:00 AM", "Sends Thu 9:00 AM". */
function sends(at: number): string {
  const when = whenLabel(at, Date.now());
  if (/^\d/.test(when)) return `Sends at ${when}`;
  return `Sends ${when.startsWith("Tomorrow") ? `tomorrow${when.slice(8)}` : when}`;
}
</script>

<template>
  <ul v-if="items.length" class="mb-2 space-y-1.5" aria-label="Scheduled texts">
    <li
      v-for="t in items"
      :key="t.id"
      class="flex items-center gap-2.5 rounded-lg border border-hairline-soft bg-surface-soft px-3 py-2 text-[13px]"
    >
      <TriangleAlert v-if="t.state === 'held'" :size="14" class="shrink-0 text-warning" />
      <Clock v-else :size="14" class="shrink-0 text-muted" />
      <span class="min-w-0 flex-1">
        <span class="block text-[11px] text-muted">
          {{ t.state === "held" ? `Not sent: it was due at ${whenLabel(t.at, t.at)}` : sends(t.at) }}
        </span>
        <span class="preview-private block truncate text-ink">{{ t.text }}</span>
      </span>
      <button class="btn-secondary btn-sm shrink-0" :disabled="!canSend" :title="canSend ? 'Send it now' : 'Your iPhone isn\'t connected for texts'" @click="scheduled.sendNow(t.id)">
        Send now
      </button>
      <button class="shrink-0 rounded-md px-2 py-1 text-[12px] text-muted hover:bg-surface-card hover:text-ink" @click="scheduled.discard(t.id)">
        {{ t.state === "held" ? "Discard" : "Cancel" }}
      </button>
    </li>
  </ul>
</template>
