<script setup lang="ts">
import { computed, ref } from "vue";
import { Check, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";
import TugMark from "./TugMark.vue";

const tug = useTugStore();
const root = ref<HTMLElement | null>(null);

useFocusTrap(root, () => close());

// Newest first (the store hands them over in that order). The first is shown in full; any earlier
// updates the user also skipped sit behind a toggle so the card stays compact.
const entries = computed(() => tug.whatsNewEntries);
const latest = computed(() => entries.value[0] ?? null);
const earlier = computed(() => entries.value.slice(1));
const showEarlier = ref(false);

function close() {
  tug.whatsNewOpen = false;
}
</script>

<template>
  <div
    v-if="latest"
    class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[12vh] backdrop-blur-[2px]"
    @click.self="close"
  >
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-labelledby="whats-new-title"
      class="flex max-h-[72vh] w-full max-w-[460px] flex-col overflow-hidden rounded-2xl border border-hairline bg-canvas shadow-xl"
    >
      <header class="flex items-center gap-3 px-6 pt-6 pb-4">
        <span class="flex size-10 shrink-0 items-center justify-center rounded-xl bg-surface-card text-primary">
          <TugMark :size="22" />
        </span>
        <div class="min-w-0 flex-1">
          <h2 id="whats-new-title" class="headline text-[24px] leading-tight text-ink">What's new</h2>
          <p class="text-[13px] text-muted-soft">tug {{ latest.version }}</p>
        </div>
        <button class="rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close" @click="close">
          <X :size="16" />
        </button>
      </header>

      <div class="min-h-0 flex-1 overflow-y-auto px-6 pb-2">
        <p class="text-[15px] font-medium text-ink">{{ latest.title }}</p>
        <ul class="mt-3 space-y-2.5">
          <li v-for="(point, i) in latest.highlights" :key="i" class="flex items-start gap-2.5 text-[14px] leading-snug text-body">
            <Check :size="16" class="mt-0.5 shrink-0 text-primary" />
            <span>{{ point }}</span>
          </li>
        </ul>

        <template v-if="earlier.length">
          <button
            class="mt-5 text-[13px] font-medium text-muted active:text-ink"
            :aria-expanded="showEarlier"
            @click="showEarlier = !showEarlier"
          >
            {{ showEarlier ? "Hide earlier updates" : `Also new in earlier updates (${earlier.length})` }}
          </button>
          <div v-if="showEarlier" class="mt-3 space-y-5 border-t border-hairline-soft pt-4">
            <section v-for="note in earlier" :key="note.version">
              <p class="text-[14px] font-medium text-ink">{{ note.title }}</p>
              <p class="text-[12px] text-muted-soft">tug {{ note.version }}</p>
              <ul class="mt-2 space-y-2">
                <li v-for="(point, i) in note.highlights" :key="i" class="flex items-start gap-2.5 text-[13px] leading-snug text-muted">
                  <Check :size="14" class="mt-0.5 shrink-0 text-muted-soft" />
                  <span>{{ point }}</span>
                </li>
              </ul>
            </section>
          </div>
        </template>
      </div>

      <footer class="flex justify-end px-6 pt-3 pb-5">
        <button class="btn-primary btn-sm" @click="close">Got it</button>
      </footer>
    </div>
  </div>
</template>
