<script setup lang="ts">
import { computed, ref } from "vue";
import { ChevronRight } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { notificationTime, relativeTime, type FeedEntry } from "../lib/format";
import AppAvatar from "./AppAvatar.vue";
import NotificationItem from "./NotificationItem.vue";

const props = defineProps<{ entry: FeedEntry }>();
const tug = useTugStore();
const expanded = ref(false);

const items = computed(() => (props.entry.kind === "thread" ? props.entry.thread.items : props.entry.stack.items));
const latest = computed(() => (props.entry.kind === "thread" ? props.entry.thread.latest : props.entry.stack.latest));
const appId = computed(() => latest.value.appId);
const title = computed(() => (props.entry.kind === "thread" ? props.entry.thread.contact : props.entry.stack.appLabel));
const appLabel = computed(() => (props.entry.kind === "thread" ? props.entry.thread.appLabel : props.entry.stack.appLabel));
const preview = computed(() => {
  const n = latest.value;
  if (props.entry.kind === "thread") return n.message || n.subtitle || "(no preview)";
  return [n.title, n.message || n.subtitle].filter(Boolean).join(" · ");
});
const fresh = computed(() => tug.newCount(props.entry.key, items.value));
const when = computed(() => relativeTime(notificationTime(latest.value)));
// A ringing call needs its buttons without expanding anything.
const ringing = computed(() => latest.value.category === "incomingCall" && latest.value.live);

function open() {
  if (props.entry.kind === "thread") {
    tug.openThread(props.entry.key);
    return;
  }
  expanded.value = !expanded.value;
  if (expanded.value) tug.markSeen(props.entry.key);
}
</script>

<template>
  <div :class="['rounded-xl transition-colors', expanded ? 'bg-surface-soft' : '']">
    <button class="flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left active:bg-surface-soft" @click="open">
      <AppAvatar :app-id="appId" :label="entry.kind === 'thread' ? title : appLabel" size="sm" />
      <span class="min-w-0 flex-1">
        <span class="flex items-baseline gap-2">
          <span :class="['truncate text-[14px] text-ink', fresh ? 'font-semibold' : 'font-medium']">{{ title }}</span>
          <span v-if="entry.kind === 'thread'" class="shrink-0 text-[12px] text-muted-soft">{{ appLabel }}</span>
          <span v-else-if="items.length > 1" class="shrink-0 text-[12px] text-muted-soft">{{ items.length }}</span>
          <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when }}</span>
        </span>
        <span :class="['mt-0.5 block truncate text-[13px]', fresh ? 'text-body-strong' : 'text-muted']">{{ preview }}</span>
      </span>
      <span
        v-if="fresh"
        class="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-ink px-1.5 text-[11px] font-semibold text-on-dark"
        :aria-label="`${fresh} new`"
      >
        {{ fresh }}
      </span>
      <ChevronRight
        :size="15"
        :class="['shrink-0 text-muted-soft transition-transform', entry.kind === 'stack' && expanded ? 'rotate-90' : '']"
      />
    </button>

    <div v-if="ringing && !expanded" class="flex gap-2 px-3 pb-3 pl-14">
      <button v-if="latest.flags.positiveAction" class="btn-primary btn-sm" @click="tug.performAction(latest.id, true)">
        {{ latest.positiveLabel || "Answer" }}
      </button>
      <button v-if="latest.flags.negativeAction" class="btn-secondary btn-sm" @click="tug.performAction(latest.id, false)">
        {{ latest.negativeLabel || "Decline" }}
      </button>
    </div>

    <div v-if="expanded && entry.kind === 'stack'" class="divide-y divide-hairline-soft pb-1 pl-11">
      <NotificationItem v-for="n in entry.stack.items" :key="n.id" :n="n" compact />
    </div>
  </div>
</template>
