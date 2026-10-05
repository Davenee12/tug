<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { useTugStore } from "./stores/tug";
import ConnectionPanel from "./components/ConnectionPanel.vue";
import DeviceRail from "./components/DeviceRail.vue";
import FeedPanel from "./components/FeedPanel.vue";
import NewConversation from "./components/NewConversation.vue";
import SearchPalette from "./components/SearchPalette.vue";
import PairingDialog from "./components/PairingDialog.vue";

const tug = useTugStore();

// The connection panel sits inline on wide windows and slides over on narrow ones.
const WIDE = "(min-width: 1240px)";
const wide = ref(window.matchMedia(WIDE).matches);
// Inline while setting up or disconnected; once connected it tucks behind the gear
// so the feed and conversations get the room.
const panelInline = computed(() => wide.value && tug.status.connection !== "connected");
const mq = window.matchMedia(WIDE);
const onMq = (e: MediaQueryListEvent) => (wide.value = e.matches);
// Once the panel sits inline it no longer covers anything.
watch(panelInline, (inline) => {
  if (inline) tug.panelOpen = false;
});

// Ctrl+K: search from anywhere. Ctrl+N: new message (not while typing in a field).
// Ctrl+Shift+C: copy the latest one-time code.
function onShortcut(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
  const key = e.key.toLowerCase();
  if (e.shiftKey) {
    if (key !== "c") return;
    e.preventDefault();
    const latest = tug.latestCode();
    if (latest) void tug.copyCode(latest.code, latest.from);
    else tug.notify("info", "No code in the last 10 minutes");
    return;
  }
  const typing = e.target instanceof HTMLElement && e.target.closest("input, textarea, [contenteditable]") !== null;
  if (key === "k") {
    e.preventDefault();
    tug.pickerOpen = false;
    tug.searchOpen = true;
  } else if (key === "n" && !typing) {
    e.preventDefault();
    tug.searchOpen = false;
    tug.view = "messages";
    tug.pickerOpen = true;
  }
}

onMounted(async () => {
  mq.addEventListener("change", onMq);
  window.addEventListener("keydown", onShortcut);
  try {
    await tug.init();
  } catch (e) {
    tug.notify("error", `Couldn't start tug: ${String(e)}`);
  }
});
onUnmounted(() => {
  mq.removeEventListener("change", onMq);
  window.removeEventListener("keydown", onShortcut);
  tug.dispose();
});
</script>

<template>
  <div class="flex h-full">
    <DeviceRail class="w-[288px] shrink-0" />

    <main class="min-w-0 flex-1">
      <FeedPanel :panel-inline="panelInline" @open-panel="tug.panelOpen = true" />
    </main>

    <ConnectionPanel v-if="panelInline" class="w-[360px] shrink-0 border-l border-hairline" :closable="false" />
    <template v-else>
      <Transition enter-from-class="opacity-0" leave-to-class="opacity-0" enter-active-class="transition-opacity" leave-active-class="transition-opacity">
        <div v-if="tug.panelOpen" class="fixed inset-0 z-30 bg-ink/20" @click="tug.panelOpen = false" />
      </Transition>
      <Transition enter-from-class="translate-x-full" leave-to-class="translate-x-full" enter-active-class="transition-transform" leave-active-class="transition-transform">
        <ConnectionPanel v-if="tug.panelOpen" class="fixed inset-y-0 right-0 z-40 w-[380px] border-l border-hairline" closable @close="tug.panelOpen = false" />
      </Transition>
    </template>

    <PairingDialog v-if="tug.pairingRequest" />
    <NewConversation v-if="tug.pickerOpen" />
    <SearchPalette v-if="tug.searchOpen" />

    <Transition enter-from-class="opacity-0 translate-y-2" leave-to-class="opacity-0 translate-y-2" enter-active-class="transition" leave-active-class="transition">
      <div
        v-if="tug.flash"
        role="status"
        class="fixed bottom-6 left-1/2 z-50 flex max-w-[560px] -translate-x-1/2 items-center gap-2.5 rounded-xl bg-surface-dark px-5 py-3 text-[14px] text-on-dark"
      >
        <CircleAlert v-if="tug.flash.kind === 'error'" :size="16" class="shrink-0 text-error" />
        <span class="selectable">{{ tug.flash.text }}</span>
      </div>
    </Transition>
  </div>
</template>
