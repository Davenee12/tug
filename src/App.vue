<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert, Upload } from "lucide-vue-next";
import { useTugStore } from "./stores/tug";
import { useTugboatStore } from "./stores/tugboat";
import { useDevToolsStore } from "./stores/devtools";
import { usePcAudioStore } from "./stores/pcAudio";
import { useVoiceStore } from "./stores/voice";
import { useGameStore } from "./stores/game";
import { useUpdateStore } from "./stores/update";
import ConnectionPanel from "./components/ConnectionPanel.vue";
import ConnectPanel from "./components/ConnectPanel.vue";
import DeviceRail from "./components/DeviceRail.vue";
import TugboatPanel from "./components/TugboatPanel.vue";
import TugboatRun from "./components/TugboatRun.vue";
import HarborStack from "./components/HarborStack.vue";
import GamePicker from "./components/GamePicker.vue";
import FeedPanel from "./components/FeedPanel.vue";
import IncomingCall from "./components/IncomingCall.vue";
import NewConversation from "./components/NewConversation.vue";
import SearchPalette from "./components/SearchPalette.vue";
import SettingsPage from "./components/SettingsPage.vue";
import SpotifyPanel from "./components/SpotifyPanel.vue";
import WhatsNew from "./components/WhatsNew.vue";
import PairingDialog from "./components/PairingDialog.vue";
import ToolConfirmCard from "./components/ToolConfirmCard.vue";
import UpdateConfirm from "./components/UpdateConfirm.vue";
import { nextDownSince, RECONNECT_GRACE_MS, showConnectionPanel } from "./lib/connectionPanel";
import { showsReconnecting } from "./lib/connectionStatus";
import { phoneTextMessage, skippedMessage } from "./lib/tugboat";
import { START_FAILED } from "./lib/startup";
import { isWide } from "./lib/layout";
import { useElementSize } from "./lib/useElementSize";
import { api, on } from "./lib/ipc";
import { quickActionRunner, runQuickAction } from "./lib/quickActions";

const tug = useTugStore();
const tugboat = useTugboatStore();
const devtools = useDevToolsStore();
const pcAudio = usePcAudioStore();
const voice = useVoiceStore();
const game = useGameStore();
const update = useUpdateStore();

// While the iPhone needs the user (nothing paired, or it's been away a while), wide windows
// show the Connection panel beside the feed; everything else (and narrow windows) uses
// Settings › iPhone. A quick relink doesn't count: see lib/connectionPanel. "Wide" is measured from
// the window's layout, which follows tug's zoom; a media query wouldn't (lib/layout).
const shell = ref<HTMLElement | null>(null);
const { width: shellWidth } = useElementSize(shell, { width: window.innerWidth, height: window.innerHeight });
const wide = computed(() => isWide(shellWidth.value));
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
    void tug.copyLatestCode();
    return;
  }
  // A ringing call has the keyboard (Enter answers, Esc declines) until it's dealt with, and
  // a game (or the games picker) has it while it's open.
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

// Taskbar quick actions (the jump list's Tasks): the same paths as Ctrl+K and the shortcuts. Rust
// has already brought the window forward; anything covering the window that would hide what was
// asked for is closed first (a ringing call stays on top until it's answered).
function newMessage() {
  game.close();
  tug.searchOpen = false;
  tug.view = "messages";
  tug.pickerOpen = true;
}
function settings() {
  game.close();
  tug.searchOpen = false;
  tug.pickerOpen = false;
  tug.openSettings();
}
const quickActions = quickActionRunner(api.takeQuickActions, (action) => {
  runQuickAction(action, {
    newMessage,
    tugboat: () => {
      tug.searchOpen = false;
      void tugboat.show();
    },
    copyCode: () => void tug.copyLatestCode(),
    game: () => {
      tug.searchOpen = false;
      tug.pickerOpen = false;
      void game.show();
    },
    settings,
  });
});
let quickActionsOff: (() => void) | undefined;
let unmounted = false;

onMounted(async () => {
  window.addEventListener("keydown", onShortcut);
  // Tugboat: files dragged onto the window open Tugboat with them offered to the phone.
  void tugboat.init({
    onSkipped: (skipped) => {
      const msg = skippedMessage(skipped);
      if (msg) tug.notify("info", msg);
    },
    // Text from the phone replaces the clipboard without asking, so say so (only once it's true).
    onText: (ok) => {
      const m = phoneTextMessage(ok, tugboat.status.phone);
      tug.notify(m.kind, m.text);
    },
    // The shared clipboard's notes: sending, on the phone, not sent in time, or why it can't go.
    say: (kind, text) => tug.notify(kind, text),
  });
  // Developer tools: only listens for the confirmation card (a text an AI tool wants to send).
  void devtools.init();
  // Play iPhone audio on this PC: the Now Playing button and Settings › iPhone read this.
  void pcAudio.init();
  // Voice typing: Settings › General and Ctrl+K read this.
  void voice.init();
  try {
    await tug.init();
  } catch (e) {
    // init() recovers from failed reads by itself; this is only for something unexpected. The
    // details go to the console, the person gets words they can act on.
    console.error("tug init failed", e);
    tug.notify("error", START_FAILED);
  }
  // Automatic updates: the Update button and Settings › About; after an update restart, this also
  // reopens the screen the person left (so it runs once tug's own state has loaded).
  void update.init();
  // Only now, with history and settings loaded (so Copy latest code can find the code): a task
  // clicked before tug was ready has been waiting in the backend and runs here, once.
  try {
    const off = await on("quick-action", () => void quickActions.drain());
    if (unmounted) off();
    else quickActionsOff = off;
  } catch {
    /* not running in tug */
  }
  void quickActions.drain();
});
onUnmounted(() => {
  unmounted = true;
  quickActionsOff?.();
  quickActionsOff = undefined;
  window.removeEventListener("keydown", onShortcut);
  window.clearTimeout(graceTimer);
  tug.dispose();
  tugboat.dispose();
  devtools.dispose();
  pcAudio.dispose();
  voice.dispose();
  update.dispose();
});
</script>

<template>
  <div ref="shell" class="flex h-full">
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
    <!-- The games (and their picker) cover the window; a call, a confirmation card or a pairing PIN
         still come up over them (and pause a game). -->
    <GamePicker v-if="game.screen === 'picker'" />
    <TugboatRun v-else-if="game.screen === 'tugboat-run'" />
    <HarborStack v-else-if="game.screen === 'harbor-stack'" />
    <TugboatPanel v-if="tugboat.open" />
    <!-- A text an AI tool (or `tug text`) wants to send: nothing goes without Send here. -->
    <ToolConfirmCard v-if="devtools.confirm" :key="devtools.confirm.id" :request="devtools.confirm" />
    <!-- "Restart tug to update?" from the Update button: nothing restarts without Restart now. -->
    <UpdateConfirm v-if="update.confirm.open" />
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
