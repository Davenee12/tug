<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { ChevronRight, ExternalLink, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { notificationTime, relativeTime, type FeedEntry } from "../lib/format";
import { findCode } from "../lib/codes";
import { GMAIL_APP_ID, webLinkFor } from "../lib/weblinks";
import AppAvatar from "./AppAvatar.vue";
import CodeChip from "./CodeChip.vue";
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
const clearable = computed(() => items.value.some((n) => n.live && n.flags.negativeAction));
// A verification code in the newest notification: copy it right from the row.
const code = computed(() => findCode(latest.value.message || latest.value.subtitle));
// A web page to open for this app, but only for app stacks — conversation threads (people) never
// get an "Open" button. For a stack the latest notification stands in (same app for all items).
const webLink = computed(() => (props.entry.kind === "stack" ? webLinkFor(latest.value) : null));
// Clicking a single Gmail notification's body opens it in the browser instead of expanding — the
// owner's "click the notification to open" for the one case where it doesn't fight the stack UI.
const bodyOpens = computed(
  () => props.entry.kind === "stack" && items.value.length === 1 && latest.value.appId === GMAIL_APP_ID && !!webLink.value,
);

const rowEl = ref<HTMLElement | null>(null);
const flashed = ref(false);

// Arriving from search: expand the stack holding the notification, scroll to it, highlight.
watch(
  () => tug.focusItem,
  async (item) => {
    if (!item?.startsWith("n")) return;
    const id = Number(item.slice(1));
    if (!items.value.some((n) => n.id === id)) return;
    if (props.entry.kind === "stack") expanded.value = true;
    tug.focusItem = null;
    await nextTick();
    rowEl.value?.scrollIntoView({ block: "center" });
    flashed.value = true;
    window.setTimeout(() => (flashed.value = false), 1800);
  },
  { immediate: true },
);

function open() {
  if (props.entry.kind === "thread") {
    tug.openThread(props.entry.key);
    return;
  }
  if (bodyOpens.value && webLink.value) {
    tug.openUrl(webLink.value.url);
    return;
  }
  expanded.value = !expanded.value;
  if (expanded.value) tug.markSeen(props.entry.key);
}
</script>

<template>
  <div
    ref="rowEl"
    :class="[
      'rounded-xl transition-colors',
      expanded ? 'bg-surface-soft' : '',
      flashed ? 'ring-2 ring-accent-amber ring-offset-2 ring-offset-canvas' : '',
    ]"
  >
    <div
      role="button"
      tabindex="0"
      class="flex w-full cursor-default items-center gap-3 rounded-xl px-3 py-2.5 text-left active:bg-surface-soft"
      @click="open"
      @keydown.enter.self="open"
    >
      <AppAvatar :app-id="appId" :label="entry.kind === 'thread' ? title : appLabel" :person="entry.kind === 'thread'" size="sm" />
      <span class="min-w-0 flex-1">
        <span class="flex items-baseline gap-2">
          <span :class="['truncate text-[14px] text-ink', fresh ? 'font-semibold' : 'font-medium']">{{ title }}</span>
          <span v-if="entry.kind === 'thread'" class="shrink-0 text-[12px] text-muted-soft">{{ appLabel }}</span>
          <span v-else-if="items.length > 1" class="shrink-0 text-[12px] text-muted-soft">{{ items.length }}</span>
          <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when }}</span>
        </span>
        <span :class="['mt-0.5 block truncate text-[13px]', fresh ? 'text-body-strong' : 'text-muted']">{{ preview }}</span>
      </span>
      <CodeChip v-if="code" :code="code.code" :from="[latest]" />
      <span
        v-if="fresh"
        class="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-ink px-1.5 text-[11px] font-semibold text-on-dark"
        :aria-label="`${fresh} new`"
      >
        {{ fresh }}
      </span>
      <span
        v-if="webLink"
        role="button"
        tabindex="0"
        class="shrink-0 rounded-md p-1 text-muted-soft active:bg-surface-card"
        :title="`Open in ${webLink.label} on the web`"
        @click.stop="tug.openUrl(webLink.url)"
        @keydown.enter.stop="tug.openUrl(webLink.url)"
      >
        <ExternalLink :size="14" />
      </span>
      <span
        v-if="clearable && !ringing"
        role="button"
        tabindex="0"
        class="shrink-0 rounded-md p-1 text-muted-soft active:bg-surface-card"
        :title="`Clear ${entry.kind === 'thread' ? title : appLabel} on your iPhone`"
        @click.stop="tug.clearItems(items)"
        @keydown.enter.stop="tug.clearItems(items)"
      >
        <X :size="14" />
      </span>
      <ChevronRight
        v-if="!bodyOpens"
        :size="15"
        :class="['shrink-0 text-muted-soft transition-transform', entry.kind === 'stack' && expanded ? 'rotate-90' : '']"
      />
    </div>

    <div v-if="ringing && !expanded" class="flex gap-2 px-3 pb-3 pl-14">
      <button v-if="latest.flags.positiveAction" class="btn-primary btn-sm" @click="tug.respondToCall(latest, true)">
        {{ latest.positiveLabel || "Answer" }}
      </button>
      <button v-if="latest.flags.negativeAction" class="btn-secondary btn-sm" @click="tug.respondToCall(latest, false)">
        {{ latest.negativeLabel || "Decline" }}
      </button>
    </div>

    <div v-if="expanded && entry.kind === 'stack'" class="divide-y divide-hairline-soft pb-1 pl-11">
      <NotificationItem v-for="n in entry.stack.items" :key="n.id" :n="n" compact />
    </div>
  </div>
</template>
