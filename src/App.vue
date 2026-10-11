<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert, Upload } from "lucide-vue-next";
import { useTugStore } from "./stores/tug";
import { useTugboatStore } from "./stores/tugboat";
import { useDevToolsStore } from "./stores/devtools";
import { usePcAudioStore } from "./stores/pcAudio";
import { useGameStore } from "./stores/game";
import ConnectionPanel from "./components/ConnectionPanel.vue";
import ConnectPanel from "./components/ConnectPanel.vue";
import DeviceRail from "./components/DeviceRail.vue";
import TugboatPanel from "./components/TugboatPanel.vue";
import TugboatRun from "./components/TugboatRun.vue";
import FeedPanel from "./components/FeedPanel.vue";
import IncomingCall from "./components/IncomingCall.vue";
import NewConversation from "./components/NewConversation.vue";
import SearchPalette from "./components/SearchPalette.vue";
import SettingsPage from "./components/SettingsPage.vue";
import SpotifyPanel from "./components/SpotifyPanel.vue";
import WhatsNew from "./components/WhatsNew.vue";
import PairingDialog from "./components/PairingDialog.vue";
import ToolConfirmCard from "./components/ToolConfirmCard.vue";
import { nextDownSince, RECONNECT_GRACE_MS, showConnectionPanel } from "./lib/connectionPanel";
import { showsReconnecting } from "./lib/connectionStatus";
import { skippedMessage } from "./lib/tugboat";

const tug = useTugStore();
const tugboat = useTugboatStore();
const devtools = useDevToolsStore();
const pcAudio = usePcAudioStore();
const game = useGameStore();

// While the iPhone needs the user (nothing paired, or it's been away a while), wide windows
// show the Connection panel beside the feed; everything else (and narrow windows) uses
// Settings › iPhone. A quick relink doesn't count: see lib/connectionPanel.
const WIDE = "(min-width: 1240px)";
const wide = ref(window.matchMedia(WIDE).matches);
const mq = window.matchMedia(WIDE);
const onMq = (e: MediaQueryListEvent) => (wide.value = e.matches);
const downSince = ref<number | null>(null);
const now = ref(Date.now());
let graceTimer: number | undefined;
watch(
  () => tug.status.connection === "connected",
  (up) => {
    downSince.value = nextDownSince(downSince.value, up, Date.now());
    now.value = Date.now();
    window.clearTimeout(graceTimer);
    // Look again once the grace period is over, in case the phone is still away.
    if (!up) graceTimer = window.setTimeout(() => (now.value = Date.now()), RECONNECT_GRACE_MS + 100);
  },
  { immediate: true },
);
const panelInline = computed(() =>
  showConnectionPanel({
    wide: wide.value,
    inSettings: tug.view === "settings",
    statusKnown: tug.statusKnown,
    connection: tug.status.connection,
    hasDevice: tug.status.device != null,
    pairingStale: tug.status.pairingStale,
    reconnecting: showsReconnecting(tug.status),
    awaitingUnlock: tug.status.awaitingUnlock,
    downSince: downSince.value,
    now: now.value,
  }),
);
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
  // A ringing call has the keyboard (Enter answers, Esc declines) until it's dealt with, and
  // Tugboat Run has it while it's open.
  if (tug.ringing || game.open) return;
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
  // Tugboat: files dragged onto the window open Tugboat with them offered to the phone.
  void tugboat.init({
    onSkipped: (skipped) => {
      const msg = skippedMessage(skipped);
      if (msg) tug.notify("info", msg);
    },
    // Text from the phone replaces the clipboard without asking, so say so (only once it's true).
    onText: (ok) =>
      ok
        ? tug.notify("info", "Text from your phone is on your clipboard")
        : tug.notify("error", "Text from your phone arrived, but couldn't go on the clipboard"),
  });
  // Developer tools: only listens for the confirmation card (a text an AI tool wants to send).
  void devtools.init();
  // Play iPhone audio on this PC: the Now Playing button and Settings › iPhone read this.
  void pcAudio.init();
  try {
    await tug.init();
  } catch (e) {
    tug.notify("error", `Couldn't start tug: ${String(e)}`);
  }
});
onUnmounted(() => {
  mq.removeEventListener("change", onMq);
  window.removeEventListener("keydown", onShortcut);
  window.clearTimeout(graceTimer);
  tug.dispose();
  tugboat.dispose();
  devtools.dispose();
  pcAudio.dispose();
});
</script>

<template>
  <div class="flex h-full">
    <DeviceRail class="w-[288px] shrink-0" />

    <main class="min-w-0 flex-1">
      <SettingsPage v-if="tug.view === 'settings'" />
      <!-- No iPhone set up (first run, or after Start over): the Connect panel is the Feed's stand-in. -->
      <ConnectPanel v-else-if="tug.showConnect" />
      <FeedPanel v-else />
    </main>

    <!-- Beside the feed on wide windows for an already-onboarded phone that dropped; the Connect
         panel owns the first-run/Start-over case, so they never show together. -->
    <ConnectionPanel v-if="panelInline && !tug.showConnect" class="w-[360px] shrink-0 border-l border-hairline" />

    <PairingDialog v-if="tug.pairingRequest" />
    <NewConversation v-if="tug.pickerOpen" />
    <SearchPalette v-if="tug.searchOpen" />
    <SpotifyPanel v-if="tug.spotifyPanelOpen" />
    <WhatsNew v-if="tug.whatsNewOpen" />
    <!-- The game covers the window; a call, a confirmation card or a pairing PIN still come up over it (and pause it). -->
    <TugboatRun v-if="game.open" />
    <TugboatPanel v-if="tugboat.open" />
    <!-- A text an AI tool (or `tug text`) wants to send: nothing goes without Send here. -->
    <ToolConfirmCard v-if="devtools.confirm" :key="devtools.confirm.id" :request="devtools.confirm" />
    <!-- Files dragged over tug: they'll go to the phone through Tugboat. The panel's own drop zone
         lights up instead while it's open. -->
    <div
      v-if="tugboat.dragging && !tugboat.open"
      class="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-ink/30 p-8 backdrop-blur-[2px]"
    >
      <div class="flex flex-col items-center gap-3 rounded-2xl border-2 border-dashed border-primary bg-canvas px-12 py-10 text-center shadow-xl">
        <Upload :size="28" class="text-primary" />
        <p class="headline text-[24px]">Drop to send to your phone</p>
        <p class="text-[13px] text-muted">They'll show up in Tugboat on the phone.</p>
      </div>
    </div>
    <!-- Last, so a ringing call sits over any other dialog. -->
    <IncomingCall v-if="tug.ringing" :key="tug.ringing.id" :call="tug.ringing" />

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
