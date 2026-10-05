<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { useTugStore } from "./stores/tug";
import ConnectionPanel from "./components/ConnectionPanel.vue";
import DeviceRail from "./components/DeviceRail.vue";
import FeedPanel from "./components/FeedPanel.vue";
import IncomingCall from "./components/IncomingCall.vue";
import NewConversation from "./components/NewConversation.vue";
import SearchPalette from "./components/SearchPalette.vue";
import SettingsPage from "./components/SettingsPage.vue";
import SetupWizard from "./components/SetupWizard.vue";
import PairingDialog from "./components/PairingDialog.vue";

const tug = useTugStore();

// While there's no connected iPhone, wide windows show the Connection panel beside the
// feed; everything else (and narrow windows) uses Settings › iPhone.
const WIDE = "(min-width: 1240px)";
const wide = ref(window.matchMedia(WIDE).matches);
const panelInline = computed(() => wide.value && tug.status.connection !== "connected" && tug.view !== "settings");
const mq = window.matchMedia(WIDE);
const onMq = (e: MediaQueryListEvent) => (wide.value = e.matches);
// The toast keeps showing (and acting on) its last message while it fades out, so a
// click on Undo during the fade still works instead of hitting an emptied message.
const shown = ref(tug.flash);
watch(
  () => tug.flash,
  (f) => {
    if (f) shown.value = f;
  },
);

// Ctrl+K: search from anywhere. Ctrl+N: new message (not while typing in a field).
// Ctrl+Shift+C: copy the latest one-time code. Ctrl+,: settings.
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
  // A ringing call has the keyboard (Enter answers, Esc declines) until it's dealt with.
  if (tug.ringing) return;
  const typing = e.target instanceof HTMLElement && e.target.closest("input, textarea, [contenteditable]") !== null;
  if (key === "k") {
    e.preventDefault();
    tug.pickerOpen = false;
    tug.searchOpen = true;
  } else if (key === ",") {
    e.preventDefault();
    tug.searchOpen = false;
    tug.pickerOpen = false;
    tug.openSettings();
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
      <SettingsPage v-if="tug.view === 'settings'" />
      <FeedPanel v-else />
    </main>

    <ConnectionPanel v-if="panelInline" class="w-[360px] shrink-0 border-l border-hairline" />

    <!-- First run: setup covers everything (and shows the PIN itself). -->
    <SetupWizard v-if="tug.showSetup" />
    <PairingDialog v-if="tug.pairingRequest && !tug.showSetup" />
    <NewConversation v-if="tug.pickerOpen" />
    <SearchPalette v-if="tug.searchOpen" />
    <!-- Last, so a ringing call sits over any other dialog. -->
    <IncomingCall v-if="tug.ringing && !tug.showSetup" :key="tug.ringing.id" :call="tug.ringing" />

    <Transition enter-from-class="opacity-0 translate-y-2" leave-to-class="opacity-0 translate-y-2" enter-active-class="transition" leave-active-class="transition">
      <div
        v-if="tug.flash"
        role="status"
        class="fixed bottom-6 left-1/2 z-50 flex max-w-[560px] -translate-x-1/2 items-center gap-2.5 rounded-xl bg-surface-dark px-5 py-3 text-[14px] text-on-dark"
      >
        <CircleAlert v-if="shown?.kind === 'error'" :size="16" class="shrink-0 text-error" />
        <span class="selectable">{{ shown?.text }}</span>
        <button
          v-if="shown?.action"
          class="-my-1 ml-2 rounded-md px-2 py-1 font-medium text-accent-amber active:bg-surface-dark-elevated"
          @click="shown.action.run()"
        >
          {{ shown.action.label }}
        </button>
      </div>
    </Transition>
  </div>
</template>
