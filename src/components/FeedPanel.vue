<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { Search, Settings2 } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { dayLabel, entryLatest, groupFeed, notificationTime, type FeedEntry } from "../lib/format";
import type { CodeEntry } from "../lib/codeFeed";
import { preservedScrollTop } from "../lib/scroll";
import CallsPanel from "./CallsPanel.vue";
import CodeFeedRow from "./CodeFeedRow.vue";
import FeedEntryRow from "./FeedEntryRow.vue";
import MessageThreads from "./MessageThreads.vue";
import WeatherCard from "./WeatherCard.vue";
import { useWeatherStore } from "../stores/weather";


const tug = useTugStore();
const weather = useWeatherStore();
onMounted(() => void weather.init());
onUnmounted(() => weather.dispose());

// The feed is what's still waiting: anything cleared (here, on the phone or the
// watch) leaves it. History stays in Messages and in search. Notification rows and code-text rows
// (codes that arrived as texts with no notification) are interleaved by time, newest first.
type Row =
  | { kind: "entry"; key: string; at: number; entry: FeedEntry }
  | { kind: "code"; key: string; at: number; code: CodeEntry };

const rows = computed<Row[]>(() => {
  const list: Row[] = groupFeed(tug.notifications.filter((n) => n.removedAt == null)).map((entry) => ({
    kind: "entry",
    key: entry.key,
    at: notificationTime(entryLatest(entry)).getTime(),
    entry,
  }));
  for (const code of tug.codeFeed) list.push({ kind: "code", key: code.key, at: code.at, code });
  return list.sort((a, b) => b.at - a.at);
});

const entryGroups = computed(() => {
  const out: { label: string; rows: Row[] }[] = [];
  for (const r of rows.value) {
    const label = dayLabel(new Date(r.at));
    const last = out.at(-1);
    if (last?.label === label) last.rows.push(r);
    else out.push({ label, rows: [r] });
  }
  return out;
});

// Hold the reader's place as the feed changes under them. The list is newest-first, so a new
// notification, a bump or a cleared row lands at or near the top: left alone the viewport would
// slide. Measure before the DOM updates (this watcher runs pre-flush), then restore afterwards —
// stay at the top if they were at the top (so the newest is seen), otherwise keep the rows under
// their eye still. Paging older history in at the bottom isn't a top change, so it's left be.
const feedScroller = ref<HTMLElement | null>(null);
const firstKey = (groups: { rows: Row[] }[]): string | undefined => groups[0]?.rows[0]?.key;
watch(entryGroups, async (next, prev) => {
  const el = feedScroller.value;
  if (!el) return;
  const prevTop = el.scrollTop;
  const prevHeight = el.scrollHeight;
  const prependedAtTop = firstKey(next) !== firstKey(prev ?? []);
  await nextTick();
  const after = feedScroller.value;
  if (!after) return;
  const top = preservedScrollTop({ prevTop, prevHeight, newHeight: after.scrollHeight, prependedAtTop });
  if (top !== null) after.scrollTop = top;
});

const unreadMessages = computed(() => tug.unreadTexts);
const TITLES: Record<string, string> = { feed: "Notifications", messages: "Messages", calls: "Calls" };

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
      <h1 class="headline text-[36px] leading-none">{{ TITLES[tug.view] ?? "Notifications" }}</h1>
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
        <button :class="['tab', tug.view === 'calls' && 'tab-active']" @click="tug.view = 'calls'">Calls</button>
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
      <button class="btn-secondary w-10 px-0" aria-label="Settings" title="Settings (Ctrl+,)" @click="tug.openSettings()">
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
        <button class="btn-primary mt-8" @click="tug.openSettings('iphone')">Set up your iPhone</button>
      </div>
    </div>

    <MessageThreads v-else-if="tug.view === 'messages'" />

    <CallsPanel v-else-if="tug.view === 'calls'" />

    <div v-else ref="feedScroller" class="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 pb-10">
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
            <template v-for="r in g.rows" :key="r.key">
              <CodeFeedRow v-if="r.kind === 'code'" :entry="r.code" />
              <FeedEntryRow v-else :entry="r.entry" />
            </template>
          </div>
        </section>
      </div>
      <div v-if="tug.hasMore && tug.notifications.length > 0" ref="sentinel" class="h-10" />
    </div>
  </div>
</template>
