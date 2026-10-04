<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { useTugStore } from "./stores/tug";
import ConnectionPanel from "./components/ConnectionPanel.vue";
import DeviceRail from "./components/DeviceRail.vue";
import FeedPanel from "./components/FeedPanel.vue";
import PairingDialog from "./components/PairingDialog.vue";

const tug = useTugStore();

// The connection panel sits inline on wide windows and slides over on narrow ones.
const WIDE = "(min-width: 1240px)";
const wide = ref(window.matchMedia(WIDE).matches);
const panelOpen = ref(false);
const mq = window.matchMedia(WIDE);
const onMq = (e: MediaQueryListEvent) => (wide.value = e.matches);

onMounted(async () => {
  mq.addEventListener("change", onMq);
  try {
    await tug.init();
  } catch (e) {
    tug.notify("error", `Couldn't start tug: ${String(e)}`);
  }
});
onUnmounted(() => mq.removeEventListener("change", onMq));
</script>

<template>
  <div class="flex h-full">
    <DeviceRail class="w-[288px] shrink-0" />

    <main class="min-w-0 flex-1">
      <FeedPanel :panel-inline="wide" @open-panel="panelOpen = true" />
    </main>

    <ConnectionPanel v-if="wide" class="w-[360px] shrink-0 border-l border-hairline" :closable="false" />
    <template v-else>
      <Transition enter-from-class="opacity-0" leave-to-class="opacity-0" enter-active-class="transition-opacity" leave-active-class="transition-opacity">
        <div v-if="panelOpen" class="fixed inset-0 z-30 bg-ink/20" @click="panelOpen = false" />
      </Transition>
      <Transition enter-from-class="translate-x-full" leave-to-class="translate-x-full" enter-active-class="transition-transform" leave-active-class="transition-transform">
        <ConnectionPanel v-if="panelOpen" class="fixed inset-y-0 right-0 z-40 w-[380px] border-l border-hairline" closable @close="panelOpen = false" />
      </Transition>
    </template>

    <PairingDialog />

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
