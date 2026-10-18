<script setup lang="ts">
// "Get voice typing ready now?": a small card offered once, on the first launch after an install or
// update when voice typing has never been set up (App.vue decides when; the backend remembers it
// was shown, so it never comes back by itself). Download fetches the picked engine's speech model
// in the background, with progress and Cancel here; Not now (or ×) just closes it. Nothing here
// turns voice typing on.
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Mic, X } from "lucide-vue-next";
import { useVoiceStore } from "../stores/voice";
import { useTugStore } from "../stores/tug";
import { engineModelId, readyCardState, readyPromptText } from "../lib/voice";

const voice = useVoiceStore();
const tug = useTugStore();
const started = ref(false);

const state = computed(() => (voice.status ? readyCardState(voice.status, started.value) : null));
const title = computed(() => (voice.status ? readyPromptText(voice.status).title : ""));
const model = computed(() => (voice.status ? (engineModelId(voice.status.engine) ?? "accurate") : "accurate"));

function close() {
  voice.readyCardOpen = false;
}
async function download() {
  started.value = true;
  await voice.downloadModel(model.value);
}
async function cancel() {
  await voice.cancelDownload(model.value);
  close();
}
function openSettings() {
  close();
  tug.openSettings("general");
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && state.value?.phase !== "downloading") close();
}
onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <section
    v-if="state"
    class="fixed right-6 bottom-6 z-40 w-[340px] rounded-xl border border-hairline bg-surface-card p-4 shadow-xl"
    role="dialog"
    aria-labelledby="voice-ready-title"
  >
    <div class="flex items-start gap-3">
      <Mic :size="18" class="mt-0.5 shrink-0 text-ink" />
      <div class="min-w-0 flex-1">
        <p id="voice-ready-title" class="text-[14px] font-medium text-ink">
          {{ state.phase === "ready" ? "Voice typing is ready" : title }}
        </p>
        <p class="mt-1 text-[13px] leading-snug" :class="state.phase === 'failed' ? 'text-error' : 'text-muted'" role="status" aria-live="polite">
          {{ state.text }}
        </p>
        <div
          v-if="state.phase === 'downloading'"
          class="mt-2 h-1.5 overflow-hidden rounded-full bg-surface-cream-strong"
          role="progressbar"
          aria-label="Speech model download"
          :aria-valuenow="state.percent"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <div class="h-full rounded-full bg-ink" :style="{ width: `${state.percent}%` }" />
        </div>
        <div class="mt-3 flex flex-wrap gap-2">
          <template v-if="state.phase === 'offer'">
            <button class="btn-primary btn-sm" @click="download">Download</button>
            <button class="btn-secondary btn-sm" @click="close">Not now</button>
          </template>
          <button v-else-if="state.phase === 'downloading'" class="btn-secondary btn-sm" @click="cancel">Cancel</button>
          <template v-else-if="state.phase === 'failed'">
            <button class="btn-secondary btn-sm" @click="download">Try again</button>
            <button class="btn-secondary btn-sm" @click="close">Not now</button>
          </template>
          <button v-else class="btn-secondary btn-sm" @click="openSettings">Open Settings</button>
        </div>
      </div>
      <button
        class="-mt-1 -mr-1 rounded-md p-1 text-muted hover:text-ink"
        :aria-label="state.phase === 'downloading' ? 'Hide (the download carries on)' : 'Close'"
        @click="close"
      >
        <X :size="16" />
      </button>
    </div>
  </section>
</template>
