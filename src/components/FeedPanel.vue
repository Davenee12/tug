<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Search, Settings2 } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { dayLabel, entryLatest, groupFeed, groupThreads, notificationTime, type FeedEntry } from "../lib/format";
import FeedEntryRow from "./FeedEntryRow.vue";
import MessageThreads from "./MessageThreads.vue";
import WeatherCard from "./WeatherCard.vue";
import { useWeatherStore } from "../stores/weather";

defineProps<{ panelInline: boolean }>();
const emit = defineEmits<{ openPanel: [] }>();

const tug = useTugStore();
const weather = useWeatherStore();
onMounted(() => void weather.init());
onUnmounted(() => weather.dispose());

// The feed is what's still waiting: anything cleared (here, on the phone or the
// watch) leaves it. History stays in Messages and in search.
const entryGroups = computed(() => {
  const out: { label: string; entries: FeedEntry[] }[] = [];
  for (const e of groupFeed(tug.notifications.filter((n) => n.removedAt == null))) {
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

// Infinite scroll through older history.
const sentinel = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | undefined;
onMounted(() => {
  observer = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting)) void tug.loadMore();
  });
  watch(
    sentinel,
    (el, old) => {
      if (old) observer?.unobserve(old);
      if (el) observer?.observe(el);
    },
    { immediate: true },
  );
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
      <button
        class="ml-auto flex h-10 w-full max-w-72 items-center gap-2 rounded-lg border border-hairline bg-canvas px-3.5 text-left text-[14px] text-muted-soft active:bg-surface-soft"
        aria-label="Search people, texts and notifications"
        @click="tug.searchOpen = true"
      >
        <Search :size="15" class="shrink-0" />
        <span class="flex-1 truncate">Search everything</span>
        <kbd class="rounded border border-hairline px-1.5 font-mono text-[11px]">Ctrl K</kbd>
      </button>
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

    <div v-else class="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 pb-10">
      <div v-if="weather.place !== 'off'" class="mx-auto w-full max-w-3xl px-3 pt-5">
        <WeatherCard />
      </div>

      <div v-if="entryGroups.length === 0" class="flex flex-1 items-center justify-center py-10">
        <div class="max-w-lg text-center">
          <p class="headline text-[28px]">{{ tug.notifications.length ? "You're all caught up" : "Quiet for now" }}</p>
          <p class="mt-2 text-[14px] text-muted">
            {{
              tug.notifications.length
                ? "Cleared notifications live on in Messages and search (Ctrl K)."
                : tug.connected
                  ? "New notifications from your iPhone will land here."
                  : "Notifications appear here once your iPhone connects."
            }}
          </p>
        </div>
      </div>

      <div class="mx-auto w-full max-w-3xl">
        <section v-for="g in entryGroups" :key="g.label">
          <h2 class="caption-upper sticky top-0 z-10 bg-canvas/95 px-3 pt-5 pb-2 text-muted backdrop-blur-sm">{{ g.label }}</h2>
          <div class="flex flex-col gap-0.5">
            <FeedEntryRow v-for="e in g.entries" :key="e.key" :entry="e" />
          </div>
        </section>
      </div>
      <div v-if="tug.hasMore && tug.notifications.length > 0" ref="sentinel" class="h-10" />
    </div>
  </div>
</template>
