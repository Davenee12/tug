<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { ChevronRight, ExternalLink, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { canClear, MESSAGES_APP, notificationTime, relativeTime, type FeedEntry } from "../lib/format";
import { findCode } from "../lib/codes";
import { smartChips } from "../lib/smartActions";
import { previewOf, textContent } from "../lib/messageType";
import { GMAIL_APP_ID, mayHaveWebsite, webLinkFor, type WebLink } from "../lib/weblinks";
import AppAvatar from "./AppAvatar.vue";
import CodeChip from "./CodeChip.vue";
import MediaChip from "./MediaChip.vue";
import NotificationItem from "./NotificationItem.vue";
import RemindButton from "./RemindButton.vue";
import type { RemindTarget } from "../stores/reminders";
import SmartChip from "./SmartChip.vue";
import { singleFlight } from "../lib/singleFlight";
import { useCallButtonGate } from "../lib/useCallButtonGate";

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
// A text's photo ("Attachment: 1 Photo") reads as a chip, not the raw line (lib/attachments).
const content = computed(() => {
  const n = latest.value;
  return props.entry.kind === "thread" && n.appId === MESSAGES_APP && n.message ? textContent(n.message) : null;
});
const chipOnly = computed(() => !!content.value && !content.value.text && (!!content.value.media || content.value.unreadable));
const fresh = computed(() => tug.newCount(props.entry.key, items.value));
const when = computed(() => relativeTime(notificationTime(latest.value)));
// A ringing call needs its buttons without expanding anything.
const ringing = computed(() => latest.value.category === "incomingCall" && latest.value.live);
// Space never answers or declines here either; Enter only once it has rung a moment untouched.
const { onCallKey } = useCallButtonGate(ringing);
const clearable = computed(() => items.value.some(canClear));
// A verification code in the newest notification: copy it right from the row.
const code = computed(() => findCode(latest.value.message || latest.value.subtitle));
// A web page to open for this app, but only for app stacks — conversation threads (people) never
// get an "Open" button. For a stack the latest notification stands in (same app for all items).
// Every app can be opened: its best page when tug knows one, else the app's own website.
const webLink = computed<WebLink | null>(() => {
  if (!(props.entry.kind === "stack")) return null;
  const known = webLinkFor(latest.value);
  if (known) return known;
  const site = mayHaveWebsite(latest.value.appId) ? tug.websiteFor(latest.value.appId) : null;
  return site ? { url: site, label: appLabel.value } : null;
});
// A link, number, address or parcel number in the newest notification (lib/smartActions): up to
// two chips (one beside a code chip, so the row keeps room for its words), never the page the row's
// Open button already goes to.
const smart = computed(() => {
  const n = latest.value;
  return smartChips([n.subtitle, n.message].filter(Boolean).join("\n"), {
    limit: code.value ? 1 : 2,
    skipUrls: [webLink.value?.url],
    // No web link chip on a stranger's text: one-click links are how scam texts work.
    links: tug.isKnownSender({ appId: n.appId, contact: title.value, addresses: [] }),
  });
});
// Clicking a single Gmail notification's body opens it in the browser instead of expanding — the
// owner's "click the notification to open" for the one case where it doesn't fight the stack UI.
const bodyOpens = computed(
  () => props.entry.kind === "stack" && items.value.length === 1 && latest.value.appId === GMAIL_APP_ID && !!webLink.value,
);

// Remind me: a person's conversation as a whole, or the app's newest notification.
const remindTarget = computed<RemindTarget>(() => ({
  title: title.value,
  body: preview.value,
  notificationIds: items.value.map((n) => n.id),
  threadKey: props.entry.kind === "thread" ? props.entry.key : null,
}));

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

// One press, one action: Answer/Decline and ✕ wait for the phone before they can be pressed again.
const { run: respond, busy: answering } = singleFlight((answer: boolean) => tug.respondToCall(latest.value, answer));
const { run: clearAll, busy: clearing } = singleFlight(() => tug.clearItems(items.value));

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
      @keydown.space.self.prevent="open"
    >
      <AppAvatar :app-id="appId" :label="entry.kind === 'thread' ? title : appLabel" :person="entry.kind === 'thread'" size="sm" />
      <span class="min-w-0 flex-1">
        <span class="flex items-baseline gap-2">
          <span :class="['truncate text-[14px] text-ink', fresh ? 'font-semibold' : 'font-medium', entry.kind === 'thread' && 'preview-private']">{{ title }}</span>
          <span v-if="entry.kind === 'thread'" class="shrink-0 text-[12px] text-muted-soft">{{ appLabel }}</span>
          <span v-else-if="items.length > 1" class="shrink-0 text-[12px] text-muted-soft">{{ items.length }}</span>
          <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when }}</span>
        </span>
        <span :class="['preview-private mt-0.5 block truncate text-[13px]', fresh ? 'text-body-strong' : 'text-muted']">
          <MediaChip v-if="chipOnly && content" inline :media="content.media" :unreadable="content.unreadable" />
          <template v-else-if="content?.media">{{ previewOf(content) }}</template>
          <template v-else>{{ preview }}</template>
        </span>
      </span>
      <!-- An open stack shows each notification's code on its own line instead. -->
      <CodeChip v-if="code && !expanded" :code="code.code" :from="[latest]" />
      <template v-if="!expanded">
        <SmartChip v-for="a in smart" :key="`${a.kind}:${a.start}`" :action="a" />
      </template>
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
        @keydown.space.stop.prevent="tug.openUrl(webLink.url)"
      >
        <ExternalLink :size="14" />
      </span>
      <RemindButton v-if="!ringing" :target="remindTarget" />
      <span
        v-if="clearable && !ringing"
        role="button"
        :tabindex="clearing ? -1 : 0"
        :aria-disabled="clearing ? 'true' : undefined"
        :class="['shrink-0 rounded-md p-1 text-muted-soft active:bg-surface-card', clearing ? 'opacity-50' : '']"
        :title="`Clear ${entry.kind === 'thread' ? title : appLabel} on your iPhone`"
        @click.stop="clearAll"
        @keydown.enter.stop="clearAll"
        @keydown.space.stop.prevent="clearAll"
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
      <button
        v-if="latest.flags.positiveAction"
        class="btn-primary btn-sm"
        data-call-button
        :disabled="answering"
        @keydown="onCallKey($event, 'answer')"
        @keyup="onCallKey($event, 'answer')"
        @click="respond(true)"
      >
        {{ latest.positiveLabel || "Answer" }}
      </button>
      <button
        v-if="latest.flags.negativeAction"
        class="btn-secondary btn-sm"
        data-call-button
        :disabled="answering"
        @keydown="onCallKey($event, 'decline')"
        @keyup="onCallKey($event, 'decline')"
        @click="respond(false)"
      >
        {{ latest.negativeLabel || "Decline" }}
      </button>
    </div>

    <div v-if="expanded && entry.kind === 'stack'" class="divide-y divide-hairline-soft pb-1 pl-11">
      <NotificationItem v-for="n in entry.stack.items" :key="n.id" :n="n" compact />
    </div>
  </div>
</template>
