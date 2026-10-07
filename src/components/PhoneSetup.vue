<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { CircleAlert, LoaderCircle, RefreshCw, Smartphone } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { bondHint, pairingProblem, setupDeviceLists } from "../lib/pairings";
import { api } from "../lib/ipc";
import { connectionSentence } from "../lib/connectionStatus";
import type { DiscoveredDevice } from "../types/protocol";

// The paired iPhone, or the steps to pair one. Shared by first-run setup (the inline
// Connection panel) and Settings › iPhone.
const tug = useTugStore();
// Offer iPhones up front; anything tug can't confirm as a phone (a nameless just-connected
// iPhone, but also an Echo Dot) goes under "Other devices" with no primary action. Keyboards,
// mice and headphones are never the phone, so they're hidden entirely. A discoverable but unpaired
// Classic iPhone (the freshly-forgotten case) is offered too — the split lives in lib/pairings.
const lists = computed(() => setupDeviceLists(tug.discovered));
const devices = computed(() => lists.value.phones);
const otherDevices = computed(() => lists.value.others);
const hidden = computed(() => lists.value.hiddenAccessories);
const s = computed(() => tug.status);
const scanning = ref(false);
const busyId = ref<string | null>(null);

async function startScan() {
  scanning.value = true;
  await tug.startDiscovery();
}
async function stopScan() {
  scanning.value = false;
  await tug.stopDiscovery();
}

// Scan while there's no iPhone chosen; stop as soon as one is.
watch(
  () => s.value.device,
  (device) => (device ? void stopScan() : void startScan()),
  { immediate: true },
);
onUnmounted(() => {
  if (scanning.value) void tug.stopDiscovery();
  window.clearTimeout(resetTimer);
  window.clearTimeout(startOverTimer);
});

async function choose(d: DiscoveredDevice) {
  busyId.value = d.id;
  if (d.paired) await tug.useDevice(d.id);
  else await tug.pair(d.id);
  busyId.value = null;
}

const confirmForget = ref(false);
let resetTimer: number | undefined;
async function forget() {
  if (!confirmForget.value) {
    confirmForget.value = true;
    window.clearTimeout(resetTimer);
    resetTimer = window.setTimeout(() => (confirmForget.value = false), 4000);
    return;
  }
  confirmForget.value = false;
  await tug.forget();
}

// More than one iPhone paired, or a different one than tug remembers: one clean Start over.
const problem = computed(() => pairingProblem(tug.discovered, s.value.device?.id ?? null));
const confirmStartOver = ref(false);
let startOverTimer: number | undefined;
async function startOver() {
  if (!confirmStartOver.value) {
    confirmStartOver.value = true;
    window.clearTimeout(startOverTimer);
    startOverTimer = window.setTimeout(() => (confirmStartOver.value = false), 4000);
    return;
  }
  confirmStartOver.value = false;
  await tug.forget();
}

// The phone looks to have forgotten this PC while Windows still holds the bond (stale-bond
// errors, or a bond the phone never connects to): decided purely in lib/pairings.
const bond = computed(() => bondHint(s.value));

const advertisingLabel = computed(
  () => ({ off: "Off", starting: "Starting…", on: "On", error: "Failed" })[s.value.advertising],
);
</script>

<template>
  <div class="flex flex-col gap-6">
    <!-- The phone looks to have forgotten this PC while Windows still holds the bond. -->
    <div v-if="bond" class="flex flex-col gap-2 rounded-xl border border-error/30 bg-canvas px-4 py-3">
      <p class="text-[13px] text-body-strong">
        <template v-if="bond === 'forgotten'">Your PC still remembers a pairing your iPhone forgot, so they can't reconnect.</template>
        <template v-else>Your iPhone keeps refusing this PC's pairing — it may have been forgotten on the phone.</template>
        Remove <strong class="font-medium">{{ s.textsDevice ?? s.device?.name ?? "your iPhone" }}</strong> in Windows Bluetooth settings, or Start over
        here, then pair again.
      </p>
      <div class="flex gap-2">
        <button class="btn-primary btn-sm" @click="startOver">{{ confirmStartOver ? "Click again to start over" : "Start over" }}</button>
        <button class="btn-secondary btn-sm" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
      </div>
    </div>

    <div
      v-else-if="s.lastError && (s.connection !== 'connected' || !s.services.notifications)"
      class="flex gap-2.5 rounded-xl border border-error/30 bg-canvas px-4 py-3 text-[13px] text-body-strong"
    >
      <CircleAlert :size="16" class="mt-0.5 shrink-0 text-error" />
      <span class="selectable min-w-0 flex-1">{{ s.lastError }}</span>
    </div>

    <!-- More than one iPhone paired, or a different one than tug remembers: one clean Start over. -->
    <div v-if="problem" class="flex flex-col gap-2 rounded-xl border border-hairline bg-canvas px-4 py-3">
      <p class="text-[13px] text-body">
        <template v-if="problem === 'duplicates'">More than one iPhone is paired with this PC, so tug can't tell which to use.</template>
        <template v-else>The iPhone paired now isn't the one tug remembers.</template>
        Start over clears the pairings tug made and begins again. Also tap <em>Forget This Device</em> for this PC under Settings › Bluetooth on the iPhone.
      </p>
      <button class="btn-secondary btn-sm self-start" @click="startOver">
        {{ confirmStartOver ? "Click again to start over" : "Start over" }}
      </button>
    </div>

    <!-- Fresh bond: iOS holds the subscribe open until "Allow" is tapped on the phone. -->
    <div v-if="s.awaitingPhoneAllow" class="flex items-center gap-3 rounded-xl border border-hairline bg-canvas px-4 py-3">
      <Smartphone :size="18" class="shrink-0 text-ink" />
      <p class="min-w-0 flex-1 text-[13px] text-body">
        Look at your iPhone and tap <strong class="font-medium text-body-strong">Allow</strong> to let this PC see your notifications.
      </p>
    </div>

    <!-- Paired: device card -->
    <section v-if="s.device" class="rounded-xl border border-hairline bg-canvas p-5">
      <p class="caption-upper text-muted">Paired iPhone</p>
      <p class="headline mt-1 text-[24px]">{{ s.device.name }}</p>
      <p class="mt-1 text-[13px] text-muted">
        {{ connectionSentence(s) }}
      </p>
      <button class="btn-secondary btn-sm mt-4" @click="forget">
        {{ confirmForget ? "Click again to forget" : "Forget this iPhone" }}
      </button>
      <p class="mt-2 text-[12px] text-muted-soft">
        Also tap <em>Forget This Device</em> under Settings › Bluetooth on the iPhone before pairing again.
      </p>
    </section>

    <!-- Not paired: setup steps -->
    <section v-else>
      <ol class="flex flex-col gap-5">
        <li class="flex gap-3">
          <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">1</span>
          <div class="min-w-0">
            <p class="text-[15px] font-medium text-ink">Make this PC visible</p>
            <p class="text-[13px] text-muted">
              Advertising: <span class="font-medium text-body-strong">{{ advertisingLabel }}</span>
              <template v-if="!tug.advertiseEnabled"> · turn on <em>Visible to iPhone</em></template>
            </p>
            <p v-if="s.peripheralSupported === false" class="mt-1 text-[13px] text-error">
              This PC's Bluetooth adapter can't act as a peripheral, so the iPhone can't connect to it. A Bluetooth 5 USB adapter fixes this.
            </p>
          </div>
        </li>
        <li class="flex gap-3">
          <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">2</span>
          <div class="min-w-0 text-[13px] text-muted">
            <p class="text-[15px] font-medium text-ink">Open Bluetooth on your iPhone</p>
            <p>
              On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> and keep that screen open. Don't tap this PC
              in the list — just leave it showing. Your iPhone appears below in a moment.
            </p>
          </div>
        </li>
        <li class="flex gap-3">
          <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">3</span>
          <div class="min-w-0 flex-1 text-[13px] text-muted">
            <p class="text-[15px] font-medium text-ink">Pair it here</p>
            <p>
              Click <strong class="font-medium text-body-strong">Pair</strong> next to your iPhone below, confirm the code on both screens, then tap
              <strong class="font-medium text-body-strong">Allow</strong> on the iPhone. You only do this once — afterwards iOS reconnects on its own.
            </p>
          </div>
        </li>
      </ol>

      <div class="mt-4 flex items-center justify-between">
        <span class="flex items-center gap-2 text-[13px] text-muted">
          <LoaderCircle v-if="scanning" :size="14" class="animate-spin" />
          {{ scanning ? "Scanning nearby devices" : "Scan stopped" }}
        </span>
        <button class="btn-secondary btn-sm" @click="scanning ? stopScan() : startScan()">
          <RefreshCw v-if="!scanning" :size="13" />
          {{ scanning ? "Stop" : "Scan" }}
        </button>
      </div>

      <ul class="mt-3 flex flex-col gap-2">
        <li
          v-for="d in devices"
          :key="d.id"
          :class="['flex items-center gap-3 rounded-xl border bg-canvas px-4 py-3', d.connected ? 'border-accent-teal/60' : 'border-hairline']"
        >
          <div class="min-w-0 flex-1">
            <p class="truncate text-[14px] font-medium text-ink">{{ d.name }}</p>
            <p class="flex gap-2 text-[12px] text-muted">
              <span v-if="d.connected" class="font-medium text-ink">Connected now</span>
              <span v-if="d.paired && d.transport === 'classic'">Paired for calls &amp; audio</span>
              <span v-else-if="d.paired">Paired</span>
            </p>
          </div>
          <button
            :class="[d.connected ? 'btn-primary' : 'btn-secondary', 'btn-sm']"
            :disabled="busyId !== null || (!d.paired && !d.canPair)"
            @click="choose(d)"
          >
            <LoaderCircle v-if="busyId === d.id" :size="13" class="animate-spin" />
            {{ d.paired ? "Use" : "Pair" }}
          </button>
        </li>
        <li
          v-if="scanning && devices.length === 0"
          class="rounded-xl border border-dashed border-hairline px-4 py-6 text-center text-[13px] text-muted-soft"
        >
          Looking for devices…
        </li>
      </ul>
      <p v-if="hidden" class="mt-2 text-[12px] text-muted-soft">
        Not showing {{ hidden }} {{ hidden === 1 ? "accessory" : "accessories" }} (keyboards, headphones and the like).
      </p>

      <!-- Devices tug couldn't confirm as a phone (possibly a nameless just-connected iPhone). -->
      <details v-if="otherDevices.length" class="mt-3 text-[13px] text-muted">
        <summary class="cursor-pointer select-none">Don't see your iPhone? Other nearby devices</summary>
        <ul class="mt-2 flex flex-col gap-2">
          <li
            v-for="d in otherDevices"
            :key="d.id"
            class="flex items-center gap-3 rounded-xl border border-hairline bg-canvas px-4 py-2.5"
          >
            <p class="min-w-0 flex-1 truncate text-[13px] text-ink">{{ d.name }}</p>
            <button class="btn-secondary btn-sm" :disabled="busyId !== null || (!d.paired && !d.canPair)" @click="choose(d)">
              <LoaderCircle v-if="busyId === d.id" :size="13" class="animate-spin" />
              {{ d.paired ? "Use" : "Pair" }}
            </button>
          </li>
        </ul>
      </details>

      <!-- Fallback for older iOS or when the phone never appears: the LightBlue route. -->
      <details class="mt-3 text-[13px] text-muted">
        <summary class="cursor-pointer select-none">Can't see your iPhone?</summary>
        <p class="mt-2">
          Keep <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> open on your iPhone — it only advertises while that screen is
          showing. If it still doesn't appear, install the free <strong class="font-medium text-body-strong">LightBlue</strong> app, open it next to this
          PC, and tap the <strong class="font-medium text-body-strong">Unnamed</strong> entry with the strongest signal (closest to 0, e.g. −45); your
          iPhone then shows up here. To be sure which entry is this PC, switch <em>Visible to iPhone</em> off for a moment: the one that disappears is
          this PC.
        </p>
      </details>
    </section>
  </div>
</template>
