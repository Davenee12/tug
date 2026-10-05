<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Check, LoaderCircle, RefreshCw, Smartphone } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { bondHint, pairingProblem, setupDeviceLists, startedOutsideTug } from "../lib/pairings";
import { connectStep } from "../lib/connectFlow";
import { phoneSwitches } from "../lib/phoneSwitches";
import { api } from "../lib/ipc";
import PhoneSwitches from "./PhoneSwitches.vue";
import type { DiscoveredDevice } from "../types/protocol";

// One "Connect your iPhone" panel, live from top to bottom, shown in two places: where the Feed
// goes on first run (and after Start over), and in Settings › iPhone for re-pairing. It reuses the
// proven pieces — the discovery split (lib/pairings), the three-switch checklist (lib/phoneSwitches
// + PhoneSwitches.vue), and the global pairing dialog for the code — and a thin state machine
// (lib/connectFlow) decides which section shows. The code and "Tap Pair on your iPhone" prompt come
// from the shared PairingDialog overlay, so this panel never renders a PIN itself.
const props = withDefaults(defineProps<{ context?: "feed" | "settings" }>(), { context: "feed" });

const tug = useTugStore();
const s = computed(() => tug.status);
const step = computed(() => connectStep(s.value, tug.pairingRequest));
const switches = computed(() => phoneSwitches(s.value));

// Discovery runs only while this panel is on screen (started below, stopped on unmount), so tug
// never scans in the background. One row per iPhone comes straight from setupDeviceLists.
const lists = computed(() => setupDeviceLists(tug.discovered));
// Windows can drop an unpaired link nobody holds within seconds, which made a just-appeared iPhone
// vanish before it could be clicked. Hold a phone we've seen for a short grace so a transient blip
// doesn't pull the row out from under the cursor.
const CANDIDATE_GRACE_MS = 10_000;
const heldPhones = new Map<string, { device: DiscoveredDevice; at: number }>();
const graceNow = ref(Date.now());
let graceTimer: number | undefined;
const candidates = computed(() => {
  const t = graceNow.value;
  const live = lists.value.phones;
  for (const d of live) heldPhones.set(d.id, { device: d, at: Date.now() });
  const liveIds = new Set(live.map((d) => d.id));
  const stale = [...heldPhones.values()].filter((e) => !liveIds.has(e.device.id) && t - e.at < CANDIDATE_GRACE_MS).map((e) => e.device);
  return [...live, ...stale].slice(0, 4);
});
const otherDevices = computed(() => lists.value.others.slice(0, 5));
const hidden = computed(() => lists.value.hiddenAccessories);

// A just-discovered iPhone often arrives nameless (the name follows a moment later), so show it
// straight away as a pairable row labelled "iPhone" and let the real name fill in when it lands.
const NAMELESS = new Set(["", "unnamed device"]);
const isNameless = (d: DiscoveredDevice) => NAMELESS.has(d.name.trim().toLowerCase());
const phoneLabel = (d: DiscoveredDevice) => (isNameless(d) ? "iPhone" : d.name);

// Unpaired iPhones (and the remembered one) are the primary choices; a phone paired to this PC
// before that isn't the one we remember — an old bond for another phone — goes in a quiet "Paired
// before" section with Use, so it never crowds the main list.
const primaryPhones = computed(() => candidates.value.filter((d) => !d.paired || d.id === s.value.device?.id));
const pairedBefore = computed(() => candidates.value.filter((d) => d.paired && d.id !== s.value.device?.id));

const busyId = ref<string | null>(null);
const connectError = ref<string | null>(null);
const scanning = ref(false);

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

// Windows' own pairing dialog (phone-initiated: tapping this PC in the iPhone's Bluetooth list) can
// create the bond before tug does, and then the code shows in Windows, not tug. Detect a device
// flipping unpaired→paired that tug didn't start, so we can point at Windows' prompt.
const windowsPairing = ref(false);
const initiatedPair = new Set<string>();
const priorPaired = new Map<string, boolean>();
watch(
  () => tug.discovered.map((d) => [d.id, d.paired] as const),
  (now) => {
    for (const [id, paired] of now) {
      if (startedOutsideTug(priorPaired.get(id), paired, initiatedPair.has(id))) windowsPairing.value = true;
      priorPaired.set(id, paired);
    }
  },
);

async function choose(d: DiscoveredDevice) {
  connectError.value = null;
  busyId.value = d.id;
  // Prefer tug's own pairing for an unpaired phone, so the code shows in tug's prompt, not Windows'.
  // Record that we started it, so the Windows-pairing hint doesn't fire for our own flip.
  if (!d.paired) initiatedPair.add(d.id);
  const ok = d.paired ? await tug.useDevice(d.id) : await tug.pair(d.id);
  busyId.value = null;
  if (!ok && !s.value.device) {
    connectError.value = "Pairing didn't finish. Keep Settings › Bluetooth open on your iPhone and try Pair again.";
  }
}

// The phone looks to have forgotten this PC while Windows still holds the bond (stale-bond errors,
// or a bond the phone never connects to): decided purely in lib/pairings.
const bond = computed(() => bondHint(s.value));
// More than one iPhone paired, or a different one than tug remembers: one clean Start over.
const problem = computed(() => pairingProblem(tug.discovered, s.value.device?.id ?? null));

// Start over / Forget unpairs the bonds tug made; confirmed first, because unpairing is irreversible.
const confirmReset = ref(false);
let resetTimer: number | undefined;
const resetting = ref(false);
async function startOver() {
  if (!confirmReset.value) {
    confirmReset.value = true;
    window.clearTimeout(resetTimer);
    resetTimer = window.setTimeout(() => (confirmReset.value = false), 4000);
    return;
  }
  confirmReset.value = false;
  resetting.value = true;
  await tug.forget();
  resetting.value = false;
}

// While this panel is up and a phone is chosen, the switches are what's left to turn on, so ask the
// backend to check them promptly (it watches while this is true; cleared on unmount).
watch(
  () => !!s.value.device,
  (hasDevice) => (tug.setupSharingShown = hasDevice),
  { immediate: true },
);

onMounted(() => {
  graceTimer = window.setInterval(() => (graceNow.value = Date.now()), 1000);
});
onUnmounted(() => {
  tug.setupSharingShown = false;
  window.clearInterval(graceTimer);
  window.clearTimeout(resetTimer);
  if (scanning.value) void tug.stopDiscovery();
});

const connecting = computed(() => s.value.connection !== "connected");
</script>

<template>
  <div :class="context === 'feed' ? 'flex h-full min-h-0 flex-col overflow-y-auto' : ''">
    <div :class="['flex flex-col gap-6', context === 'feed' ? 'mx-auto w-full max-w-[560px] px-8 py-10' : '']">
      <!-- Heading (only when this panel stands in for the Feed; Settings already titles the page). -->
      <div v-if="context === 'feed'">
        <p class="caption-upper text-muted">Welcome to tug</p>
        <h1 class="headline mt-2 text-[40px] leading-[1.1]">Connect your iPhone</h1>
      </div>

      <!-- More than one iPhone paired, or the wrong one: offer one clean Start over. -->
      <div v-if="problem" class="flex flex-col gap-2 rounded-xl border border-hairline bg-surface-card px-4 py-3">
        <p class="text-[13px] text-body">
          <template v-if="problem === 'duplicates'">More than one iPhone is paired with this PC, so tug can't tell which to use.</template>
          <template v-else>The iPhone paired now isn't the one tug remembers.</template>
          Start over clears the pairings tug made and begins again. Also tap <em>Forget This Device</em> for this PC under Settings › Bluetooth on
          the iPhone.
        </p>
        <button class="btn-secondary btn-sm self-start" :disabled="resetting" @click="startOver">
          <LoaderCircle v-if="resetting" :size="13" class="animate-spin" />
          {{ confirmReset ? "Tap again to start over" : "Start over" }}
        </button>
      </div>

      <!-- The phone looks to have forgotten this PC while Windows still holds the bond. -->
      <div v-if="bond" class="flex flex-col gap-2 rounded-xl border border-error/30 bg-canvas px-4 py-3">
        <p class="text-[13px] text-body-strong">
          <template v-if="bond === 'forgotten'">Your PC still remembers a pairing your iPhone forgot, so they can't reconnect.</template>
          <template v-else>Your iPhone keeps refusing this PC's pairing — it may have been forgotten on the phone.</template>
          Remove <strong class="font-medium">{{ s.textsDevice ?? s.device?.name ?? "your iPhone" }}</strong> in Windows Bluetooth settings, or Start
          over here, then pair again.
        </p>
        <div class="flex gap-2">
          <button class="btn-primary btn-sm" :disabled="resetting" @click="startOver">
            <LoaderCircle v-if="resetting" :size="13" class="animate-spin" />
            {{ confirmReset ? "Tap again to start over" : "Start over" }}
          </button>
          <button class="btn-secondary btn-sm" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
        </div>
      </div>

      <!-- Paired: the phone, the switches ticking green, and a way to start over. -->
      <template v-if="s.device">
        <section class="rounded-xl border border-hairline bg-canvas p-5">
          <p class="caption-upper text-muted">Your iPhone</p>
          <p class="headline mt-1 text-[24px]">{{ s.device.name }}</p>

          <!-- Fresh bond: iOS holds the connection open until "Allow" is tapped on the phone. -->
          <div v-if="step === 'allow'" class="mt-3 flex items-center gap-3 rounded-xl bg-surface-card px-4 py-3">
            <Smartphone :size="18" class="shrink-0 text-ink" />
            <p class="min-w-0 flex-1 text-[13px] text-body">
              Look at your iPhone and tap <strong class="font-medium text-body-strong">Allow</strong> to let this PC see your notifications.
            </p>
          </div>
          <p v-else-if="connecting" class="mt-2 flex items-center gap-2 text-[13px] text-muted">
            <LoaderCircle :size="14" class="animate-spin" /> Connecting to your iPhone…
          </p>
          <p v-else class="mt-1 text-[13px] text-muted">
            Connected. tug reconnects by itself when you come back in range.
          </p>
        </section>

        <!-- The three switches on the iPhone, each lighting up green as it comes on. -->
        <div>
          <p class="text-center text-[14px] text-muted">
            On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong>, tap
            <strong class="font-medium text-body-strong">ⓘ</strong> next to this PC, and turn these on. They light up here as you do.
          </p>
          <PhoneSwitches :switches="switches" />
        </div>

        <button class="btn-secondary btn-sm self-start" :disabled="resetting" @click="startOver">
          <LoaderCircle v-if="resetting" :size="13" class="animate-spin" />
          {{ confirmReset ? "Tap again to start over" : "Start over" }}
        </button>
        <p class="-mt-4 text-[12px] text-muted-soft">
          Also tap <em>Forget This Device</em> under Settings › Bluetooth on the iPhone before pairing again.
        </p>
      </template>

      <!-- Nothing paired yet: find the iPhone and pair it. -->
      <template v-else>
        <p class="text-[15px] text-body">
          On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> and keep it open. Your iPhone shows up
          below — click <strong class="font-medium text-body-strong">Pair</strong> next to it, confirm the code on both screens, then tap
          <strong class="font-medium text-body-strong">Allow</strong> on the iPhone.
        </p>

        <!-- Bluetooth has to be ready for any of this: say plainly when it isn't. -->
        <div v-if="s.radio === 'off'" class="flex items-center gap-3 rounded-xl border border-hairline bg-surface-card px-4 py-3 text-[13px] text-body">
          <span class="flex-1">Bluetooth is off on this PC.</span>
          <button class="btn-secondary btn-sm shrink-0" @click="api.openWindowsSettings('bluetooth')">Turn on Bluetooth</button>
        </div>
        <p v-else-if="s.radio === 'unavailable'" class="rounded-xl border border-hairline bg-surface-card px-4 py-3 text-[13px] text-body">
          This PC has no Bluetooth. A small Bluetooth 5 USB adapter fixes that.
        </p>
        <p v-else-if="s.peripheralSupported === false" class="rounded-xl border border-hairline bg-surface-card px-4 py-3 text-[13px] text-body">
          This PC's Bluetooth can't host a connection, so your iPhone can't reach it. A Bluetooth 5 USB adapter fixes that.
        </p>
        <div v-else-if="!tug.advertiseEnabled" class="flex items-center gap-3 rounded-xl border border-hairline bg-surface-card px-4 py-3 text-[13px] text-body">
          <span class="flex-1">tug isn't visible to your iPhone yet.</span>
          <button class="btn-secondary btn-sm shrink-0" @click="tug.setAdvertising(true)">Make it visible</button>
        </div>

        <!-- Windows' own pairing dialog is handling it (the phone started the pairing). -->
        <div v-if="windowsPairing" class="flex items-start gap-3 rounded-xl border border-hairline bg-surface-card px-4 py-3">
          <Smartphone :size="18" class="mt-0.5 shrink-0 text-ink" />
          <p class="min-w-0 flex-1 text-[13px] text-body">
            Windows is pairing your iPhone and will show a code — check it matches your iPhone and confirm on both.
          </p>
        </div>

        <div>
          <ul class="flex flex-col gap-2">
            <li
              v-for="(d, i) in primaryPhones"
              :key="d.id"
              :class="['flex items-center gap-3 rounded-xl border bg-canvas px-4 py-3', d.connected ? 'border-accent-teal/60' : 'border-hairline']"
            >
              <span class="flex size-9 shrink-0 items-center justify-center rounded-full bg-surface-card"><Smartphone :size="17" class="text-ink" /></span>
              <div class="min-w-0 flex-1">
                <p class="truncate text-[14px] font-medium text-ink">{{ phoneLabel(d) }}</p>
                <p v-if="d.connected" class="text-[12px] font-medium text-accent-teal">Connected now</p>
                <p v-else-if="isNameless(d)" class="text-[12px] text-muted-soft">finding name…</p>
                <p v-else-if="d.paired" class="text-[12px] text-muted">Paired</p>
              </div>
              <button
                :class="[i === 0 ? 'btn-primary' : 'btn-secondary', 'btn-sm']"
                :disabled="busyId !== null || (!d.paired && !d.canPair)"
                @click="choose(d)"
              >
                <LoaderCircle v-if="busyId === d.id" :size="13" class="animate-spin" />
                {{ d.paired ? "Use" : "Pair" }}
              </button>
            </li>
            <li v-if="primaryPhones.length === 0" class="flex items-center gap-3 rounded-xl border border-dashed border-hairline px-4 py-6 text-[13px] text-muted-soft">
              <LoaderCircle :size="15" class="animate-spin" /> Looking for your iPhone… keep Settings › Bluetooth open on it.
            </li>
          </ul>
          <p v-if="connectError" class="mt-3 text-[13px] text-error">{{ connectError }}</p>
          <p v-if="hidden" class="mt-2 text-[12px] text-muted-soft">
            Not showing {{ hidden }} {{ hidden === 1 ? "accessory" : "accessories" }} (keyboards, headphones and the like).
          </p>
        </div>

        <!-- An iPhone paired to this PC before, that isn't the one we remember (e.g. an old bond for
             another phone): offered to reuse, but tucked away so it doesn't crowd the main choice. -->
        <details v-if="pairedBefore.length" class="text-[13px] text-muted">
          <summary class="cursor-pointer select-none">Paired before ({{ pairedBefore.length }})</summary>
          <ul class="mt-2 flex flex-col gap-2">
            <li v-for="d in pairedBefore" :key="d.id" class="flex items-center gap-3 rounded-xl border border-hairline bg-canvas px-4 py-2.5">
              <p class="min-w-0 flex-1 truncate text-[13px] text-ink">{{ phoneLabel(d) }}</p>
              <button class="btn-secondary btn-sm" :disabled="busyId !== null" @click="choose(d)">
                <LoaderCircle v-if="busyId === d.id" :size="13" class="animate-spin" />
                Use
              </button>
            </li>
          </ul>
        </details>

        <!-- Don't see it: other nearby devices, then the manual Windows / LightBlue routes. -->
        <details v-if="otherDevices.length" class="text-[13px] text-muted">
          <summary class="cursor-pointer select-none">Don't see your iPhone? Other nearby devices</summary>
          <ul class="mt-2 flex flex-col gap-2">
            <li v-for="d in otherDevices" :key="d.id" class="flex items-center gap-3 rounded-xl border border-hairline bg-canvas px-4 py-2.5">
              <p class="min-w-0 flex-1 truncate text-[13px] text-ink">{{ d.name }}</p>
              <button class="btn-secondary btn-sm" :disabled="busyId !== null || (!d.paired && !d.canPair)" @click="choose(d)">
                <LoaderCircle v-if="busyId === d.id" :size="13" class="animate-spin" />
                {{ d.paired ? "Use" : "Pair" }}
              </button>
            </li>
          </ul>
        </details>

        <details class="text-[13px] text-muted">
          <summary class="cursor-pointer select-none">Still don't see your iPhone?</summary>
          <p class="mt-2">
            Keep <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> open on your iPhone — it only appears while that screen is
            showing. You can also add it from Windows:
            <button class="btn-secondary btn-sm mt-2 flex" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
          </p>
          <details class="mt-3">
            <summary class="cursor-pointer select-none">Last resort</summary>
            <p class="mt-2">
              Install the free <strong class="font-medium text-body-strong">LightBlue</strong> app on your iPhone, open it near this PC, and tap the
              <strong class="font-medium text-body-strong">Unnamed</strong> entry with the strongest signal (closest to 0, e.g. −45) to wake the link;
              your iPhone then shows up here. To be sure which entry is this PC, switch <em>Visible to iPhone</em> off for a moment
              <button class="underline" @click="tug.setAdvertising(!tug.advertiseEnabled)">({{ tug.advertiseEnabled ? "turn off" : "turn back on" }})</button>:
              the one that disappears is this PC.
            </p>
          </details>
        </details>

        <div class="flex items-center gap-2 text-[12px] text-muted-soft">
          <LoaderCircle v-if="scanning" :size="13" class="animate-spin" />
          <span>{{ scanning ? "Looking for your iPhone" : "Not looking" }}</span>
          <button class="btn-secondary btn-sm ml-auto" @click="scanning ? stopScan() : startScan()">
            <RefreshCw v-if="!scanning" :size="12" />
            {{ scanning ? "Stop" : "Look again" }}
          </button>
        </div>
      </template>

      <!-- When this panel is the Feed stand-in, a quiet way past it for someone not ready yet. -->
      <p v-if="context === 'feed' && !s.device" class="text-[12px] text-muted-soft">
        <Check :size="12" class="mb-0.5 mr-1 inline text-accent-teal" />
        You only do this once — afterwards your iPhone reconnects on its own.
      </p>
    </div>
  </div>
</template>
