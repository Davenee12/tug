<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Search, Settings2, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { dayLabel, entryLatest, groupFeed, groupThreads, notificationTime, type FeedEntry } from "../lib/format";
import FeedEntryRow from "./FeedEntryRow.vue";
import MessageThreads from "./MessageThreads.vue";
import NotificationItem from "./NotificationItem.vue";

defineProps<{ panelInline: boolean }>();
const emit = defineEmits<{ openPanel: [] }>();

const tug = useTugStore();
const query = ref(tug.searchQuery);

let debounce: number | undefined;
watch(query, (q) => {
  window.clearTimeout(debounce);
  debounce = window.setTimeout(() => void tug.search(q), 180);
});

// Searching shows every matching notification; browsing shows the compact grouped feed.
const entryGroups = computed(() => {
  const out: { label: string; entries: FeedEntry[] }[] = [];
  for (const e of groupFeed(tug.notifications)) {
    const label = dayLabel(notificationTime(entryLatest(e)));
    const last = out.at(-1);
    if (last?.label === label) last.entries.push(e);
    else out.push({ label, entries: [e] });
  }
  return out;
});

const unreadMessages = computed(() =>
  groupThreads(tug.notifications).reduce((sum, t) => sum + tug.newCount(t.key, t.items), 0),
);

const groups = computed(() => {
  const out: { label: string; items: typeof tug.visible }[] = [];
  for (const n of tug.visible) {
    const label = dayLabel(notificationTime(n));
    const last = out.at(-1);
    if (last?.label === label) last.items.push(n);
    else out.push({ label, items: [n] });
  }
  return out;
});

// Infinite scroll for the unfiltered feed.
const sentinel = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | undefined;
onMounted(() => {
  observer = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting)) void tug.loadMore();
  });
  watch(sentinel, (el, old) => {
    if (old) observer?.unobserve(old);
    if (el) observer?.observe(el);
  }, { immediate: true });
});
onUnmounted(() => observer?.disconnect());

const setUp = computed(() => tug.status.device != null);
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <header class="flex items-center gap-4 border-b border-hairline px-8 pt-6 pb-4">
      <h1 class="headline text-[36px] leading-none">{{ tug.view === "feed" ? "Notifications" : "Messages" }}</h1>
      <nav class="ml-2 flex gap-1">
        <button :class="['tab', tug.view === 'feed' && 'tab-active']" @click="tug.view = 'feed'">Feed</button>
        <button :class="['tab flex items-center gap-1.5', tug.view === 'messages' && 'tab-active']" @click="tug.view = 'messages'">
          Messages
          <span
            v-if="unreadMessages"
            class="flex h-[18px] min-w-[18px] items-center justify-center rounded-full bg-primary px-1 text-[11px] font-semibold text-on-primary"
          >
            {{ unreadMessages }}
          </span>
        </button>
      </nav>
      <div class="relative ml-auto w-full max-w-72">
        <Search :size="15" class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted-soft" />
        <input v-model="query" class="input pl-9" placeholder="Search history" spellcheck="false" />
        <button
          v-if="query"
          class="absolute top-1/2 right-2 -translate-y-1/2 rounded p-1 text-muted-soft active:bg-surface-card"
          aria-label="Clear search"
          @click="query = ''"
        >
          <X :size="14" />
        </button>
      </div>
      <button v-if="!panelInline" class="btn-secondary w-10 px-0" aria-label="Connection and settings" @click="emit('openPanel')">
        <Settings2 :size="16" />
      </button>
    </header>

    <!-- First run: nothing paired yet. -->
    <div v-if="!setUp && tug.notifications.length === 0" class="flex flex-1 items-center justify-center px-10">
      <div class="max-w-lg">
        <p class="caption-upper text-muted">Bluetooth LE · no app on your phone</p>
        <p class="headline mt-3 text-[48px] leading-[1.08]">Your iPhone's notifications, on your desk.</p>
        <p class="mt-4 text-[16px] text-body">
          tug pairs with your iPhone over Bluetooth and mirrors every notification here, keeps a searchable history
          after the phone has cleared it, and gives you play, pause and skip for whatever's playing.
        </p>
        <button class="btn-primary mt-8" @click="emit('openPanel')">Set up your iPhone</button>
      </div>
    </div>

    <MessageThreads v-else-if="tug.view === 'messages'" />

    <div v-else class="min-h-0 flex-1 overflow-y-auto px-5 pb-10">
      <div v-if="tug.visible.length === 0" class="flex h-full items-center justify-center">
        <div class="max-w-sm text-center">
          <p class="headline text-[28px]">{{ tug.searchResults ? "Nothing matches" : "Quiet for now" }}</p>
          <p class="mt-2 text-[14px] text-muted">
            {{
              tug.searchResults
                ? "Try fewer words. Search matches the start of words in titles, messages and app names."
                : tug.connected
                  ? "New notifications from your iPhone will land here."
                  : "Notifications appear here once your iPhone connects."
            }}
          </p>
        </div>
      </div>

      <template v-if="tug.searchResults">
        <section v-for="g in groups" :key="g.label">
          <h2 class="caption-upper sticky top-0 z-10 bg-canvas/95 px-4 pt-5 pb-2 text-muted backdrop-blur-sm">{{ g.label }}</h2>
          <div class="divide-y divide-hairline-soft">
            <NotificationItem v-for="n in g.items" :key="n.id" :n="n" />
          </div>
        </section>
      </template>
      <div v-else class="mx-auto max-w-3xl">
        <section v-for="g in entryGroups" :key="g.label">
          <h2 class="caption-upper sticky top-0 z-10 bg-canvas/95 px-3 pt-5 pb-2 text-muted backdrop-blur-sm">{{ g.label }}</h2>
          <div class="flex flex-col gap-0.5">
            <FeedEntryRow v-for="e in g.entries" :key="e.key" :entry="e" />
          </div>
        </section>
      </div>
      <div v-if="!tug.searchResults && tug.hasMore && tug.visible.length > 0" ref="sentinel" class="h-10" />
    </div>
  </div>
</template>
