<script setup lang="ts">
import { computed, onUnmounted, ref, useId, watch } from "vue";
import { PhoneMissed } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useWelcomeBackStore } from "../stores/welcomeBack";
import { rowMeta, SEEN_AFTER_MS, whenLabel, type WelcomeGroup } from "../lib/welcomeBack";
import { useNow } from "../lib/useNow";
import AppAvatar from "./AppAvatar.vue";
import CodeChip from "./CodeChip.vue";

// "Welcome back" at the top of the Feed (lib/welcomeBack): what arrived while the person was away,
// one row per person or app. Worked out again as things change, so a row that's opened or cleared
// drops off. It goes on Got it (or Esc), once it has been read and the person scrolls or acts in the
// Feed, or when a newer absence brings a new one (stores/welcomeBack).
const props = defineProps<{ scroller: HTMLElement | null }>();
const tug = useTugStore();
const wb = useWelcomeBackStore();
const headingId = useId();

// Codes age on this clock (fresh for 10 minutes); it only runs while the card is up and on screen.
const now = useNow(() => !!wb.card, 30_000);
const summary = computed(() => (wb.card ? wb.summary(wb.card, now.value) : null));
// Everything on it opened or cleared: nothing left to sum up.
watch(summary, (s) => s && s.groups.length === 0 && wb.dismiss(), { immediate: true });

// Read: on screen, with someone looking, for a moment.
const root = ref<HTMLElement | null>(null);
const inView = ref(false);
const io = new IntersectionObserver(([e]) => (inView.value = !!e && e.intersectionRatio >= 0.5), { threshold: [0, 0.5] });
watch(root, (el, old) => {
  if (old) io.unobserve(old);
  if (el) io.observe(el);
});
let seenTimer: number | undefined;
watch(
  () => inView.value && tug.canSee && !!wb.card && !wb.card.seen,
  (looking) => {
    window.clearTimeout(seenTimer);
    if (looking) seenTimer = window.setTimeout(() => wb.markSeen(), SEEN_AFTER_MS);
  },
  { immediate: true },
);

// Once read, scrolling the Feed or acting on anything else in it puts the card away. Wheel, press
// and key, not `scroll`: the Feed moves itself to hold the reader's place as rows arrive.
const ACTS = ["wheel", "pointerdown", "keydown"] as const;
function onAct(e: Event) {
  if (!wb.card?.seen) return;
  if (e.target instanceof Node && root.value?.contains(e.target)) return;
  wb.dismiss();
}
watch(
  () => props.scroller,
  (el, old) => {
    for (const type of ACTS) {
      old?.removeEventListener(type, onAct);
      el?.addEventListener(type, onAct, { passive: true });
    }
  },
  { immediate: true },
);
onUnmounted(() => {
  for (const type of ACTS) props.scroller?.removeEventListener(type, onAct);
  io.disconnect();
  window.clearTimeout(seenTimer);
});

function open(g: WelcomeGroup) {
  if (g.target.kind === "conversation") {
    // Seen there, so the row drops off; the rest of the card waits here in the Feed.
    tug.openThread(g.target.key);
    return;
  }
  // Its row in the Feed opens, scrolls into view and flashes (FeedEntryRow follows focusItem).
  wb.dismiss();
  tug.focusItem = g.target.item;
}
</script>

<template>
  <section
    v-if="summary && summary.groups.length"
    ref="root"
    class="rounded-xl bg-surface-card px-5 pt-4 pb-3"
    :aria-labelledby="headingId"
    @keydown.esc="wb.dismiss()"
  >
    <div class="flex items-start gap-3">
      <div class="min-w-0 flex-1">
        <p class="caption-upper text-muted">Welcome back</p>
        <h2 :id="headingId" class="mt-1 text-[14px] font-medium text-ink">{{ summary.headline }}</h2>
      </div>
      <button type="button" class="btn-secondary btn-sm shrink-0" @click="wb.dismiss()">Got it</button>
    </div>

    <ul class="-mx-2 mt-2 flex flex-col">
      <li v-for="g in summary.groups" :key="g.key" class="flex items-center gap-2">
        <button
          type="button"
          class="flex min-w-0 flex-1 items-center gap-3 rounded-lg px-2 py-1.5 text-left outline-none active:bg-surface-cream-strong focus-visible:ring-2 focus-visible:ring-ink/20"
          :title="g.target.kind === 'conversation' ? 'Open the conversation' : 'Show it in the Feed'"
          @click="open(g)"
        >
          <span v-if="g.kind === 'call'" class="flex size-8 shrink-0 items-center justify-center rounded-lg bg-error/10 text-error">
            <PhoneMissed :size="15" />
          </span>
          <AppAvatar v-else :app-id="g.appId" :label="g.name" :person="g.kind !== 'app'" size="sm" />
          <span class="min-w-0 flex-1">
            <span class="flex items-baseline gap-2">
              <span :class="['truncate text-[13px] font-medium text-ink', g.kind !== 'app' && 'preview-private']">{{ g.name }}</span>
              <span v-if="rowMeta(g)" class="shrink-0 text-[12px] text-muted-soft">{{ rowMeta(g) }}</span>
              <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ whenLabel(g.at, now) }}</span>
            </span>
            <span :class="['preview-private mt-0.5 block truncate text-[12px]', g.kind === 'call' ? 'text-error' : 'text-muted']">
              {{ g.line }}
            </span>
          </span>
        </button>
        <CodeChip v-if="g.code" :code="g.code" :from="g.codeFrom" />
      </li>
    </ul>
    <p v-if="summary.more" class="pt-1 text-[12px] text-muted">and {{ summary.more }} more below</p>
  </section>
</template>
