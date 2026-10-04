<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Info } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { clockTime, dayLabel, groupThreads, notificationTime, relativeTime } from "../lib/format";
import AppAvatar from "./AppAvatar.vue";

const tug = useTugStore();
const threads = computed(() => groupThreads(tug.visible));
const selectedKey = ref<string | null>(null);
const selected = computed(() => threads.value.find((t) => t.key === selectedKey.value) ?? threads.value[0] ?? null);

watch(threads, (list) => {
  if (selectedKey.value && !list.some((t) => t.key === selectedKey.value)) selectedKey.value = null;
});

function showDay(i: number): boolean {
  const items = selected.value?.items ?? [];
  if (i === 0) return true;
  return dayLabel(notificationTime(items[i])) !== dayLabel(notificationTime(items[i - 1]));
}
</script>

<template>
  <div v-if="threads.length === 0" class="flex flex-1 items-center justify-center px-8">
    <div class="max-w-sm text-center">
      <p class="headline text-[28px]">No conversations yet</p>
      <p class="mt-2 text-[14px] text-muted">
        Messages, WhatsApp, Signal, Telegram and other chat notifications collect here as threads, newest first.
      </p>
    </div>
  </div>

  <div v-else class="flex min-h-0 flex-1">
    <nav class="w-72 shrink-0 overflow-y-auto border-r border-hairline px-3 py-2">
      <button
        v-for="t in threads"
        :key="t.key"
        :class="['flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left', selected?.key === t.key ? 'bg-surface-card' : 'active:bg-surface-soft']"
        @click="selectedKey = t.key"
      >
        <AppAvatar :app-id="t.appId" :label="t.appLabel" size="sm" />
        <span class="min-w-0 flex-1">
          <span class="flex items-baseline gap-2">
            <span class="truncate text-[14px] font-medium text-ink">{{ t.contact }}</span>
            <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">
              {{ relativeTime(notificationTime(t.latest)) }}
            </span>
          </span>
          <span class="block truncate text-[13px] text-muted">{{ t.latest.message || t.latest.subtitle }}</span>
        </span>
      </button>
    </nav>

    <section v-if="selected" class="flex min-w-0 flex-1 flex-col">
      <header class="border-b border-hairline px-8 py-4">
        <p class="headline text-[26px] leading-tight">{{ selected.contact }}</p>
        <p class="text-[13px] text-muted">{{ selected.appLabel }} · {{ selected.items.length }} received</p>
      </header>
      <div class="flex-1 overflow-y-auto px-8 py-6">
        <template v-for="(m, i) in selected.items" :key="m.id">
          <div v-if="showDay(i)" class="caption-upper my-4 text-center text-muted-soft">
            {{ dayLabel(notificationTime(m)) }}
          </div>
          <div class="mb-2 flex max-w-[75%] flex-col items-start">
            <div class="selectable rounded-xl rounded-bl-sm bg-surface-card px-4 py-2.5 text-[14px] whitespace-pre-line text-ink">
              <span v-if="m.subtitle" class="mb-0.5 block text-[12px] font-medium text-muted">{{ m.subtitle }}</span>
              {{ m.message || "(no preview)" }}
            </div>
            <span class="mt-1 ml-1 font-mono text-[11px] text-muted-soft">{{ clockTime(notificationTime(m)) }}</span>
          </div>
        </template>
      </div>
      <footer class="flex items-center gap-2 border-t border-hairline px-8 py-3 text-[13px] text-muted">
        <Info :size="14" class="shrink-0" />
        Incoming only for now. Replying needs Bluetooth MAP, which tug doesn't support yet, so reply on your phone.
      </footer>
    </section>
  </div>
</template>
