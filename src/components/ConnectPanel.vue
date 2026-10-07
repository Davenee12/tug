<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Check, LoaderCircle, RefreshCw, Settings, Smartphone } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { bondHint, leftoverPhone, pairingProblem, setupDeviceLists, startedOutsideTug } from "../lib/pairings";
import { canSkipSwitches, connectStep, rescanDue } from "../lib/connectFlow";
import { connectionBusy, connectionSentence } from "../lib/connectionStatus";
import { api } from "../lib/ipc";
import { phoneModel } from "../lib/phoneModel";
import PhoneArt from "./PhoneArt.vue";
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
const step = computed(() => connectStep(s.value, tug.pairingRequest, tug.connectSkipped));
const canSkip = computed(() => tug.showConnect && canSkipSwitches(s.value));
// The same switches every other surface reads (store), so this card can't disagree with health.
const switches = computed(() => tug.switches);

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
// A fresh install can still have an iPhone paired from a previous install: offered prominently as
// `leftover` (Use / Remove) rather than only in the quiet "Paired before" list.
const leftover = computed(() => (s.value.device ? null : leftoverPhone(tug.discovered, null)));
const pairedBefore = computed(() =>
  candidates.value.filter((d) => d.paired && d.id !== s.value.device?.id && d.id !== leftover.value?.id),
);

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
  // A pairing that didn't take (or a bond the phone dropped mid-pairing) leaves no device: stay on
  // the find step and point back at confirming the code on the iPhone.
  if (!ok && !s.value.device) {
    connectError.value = "Pairing didn't finish. On your iPhone keep Settings › Bluetooth open, tap Pair again, and confirm the code on both screens.";
  }
}

// The phone looks to have forgotten this PC while Windows still holds the bond (stale-bond errors,
// or a bond the phone never connects to): decided purely in lib/pairings.
const bond = computed(() => bondHint(s.value));
// More than one iPhone paired, or a different one than tug remembers: one clean Start over.
const problem = computed(() => pairingProblem(tug.discovered, s.value.device?.id ?? null));

// A fresh install can still have an iPhone paired in Windows from a previous install (both bonds):
// `leftover` (defined above) offers it to adopt (Use) or clear (Remove), rather than leaving it
// buried under "Paired before" while the panel just searches.
const confirmRemove = ref(false);
let removeTimer: number | undefined;
const removing = ref(false);
async function removeLeftover(id: string) {
  // Unpairing is irreversible, so confirm first (same two-tap pattern as Start over).
  if (!confirmRemove.value) {
    confirmRemove.value = true;
    window.clearTimeout(removeTimer);
    removeTimer = window.setTimeout(() => (confirmRemove.value = false), 4000);
    return;
  }
  confirmRemove.value = false;
  removing.value = true;
  connectError.value = null;
  await tug.removePairing(id);
  removing.value = false;
}

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

// Fast switch checks ("watching") now key off the panel's visibility in the store, so this panel no
// longer has to flag itself — see stores/tug connectPanelVisible.

// Re-inquire for the iPhone every ~15 s while the find step is up: an unpaired-Classic AEP watcher
// inquires once, so a phone made discoverable after scanning began needs a fresh inquiry to appear.
let lastRescanAt = Date.now();
onMounted(() => {
  graceTimer = window.setInterval(() => {
    const t = Date.now();
    graceNow.value = t;
    if (rescanDue(step.value === "find" && scanning.value, t - lastRescanAt)) {
      lastRescanAt = t;
      tug.rescan();
    }
  }, 1000);
});
onUnmounted(() => {
  window.clearInterval(graceTimer);
  window.clearTimeout(resetTimer);
  window.clearTimeout(removeTimer);
  if (scanning.value) void tug.stopDiscovery();
});

// The link in the sidebar's words: a spinner only while tug is actually connecting.
const busy = computed(() => connectionBusy(s.value));
const sentence = computed(() => connectionSentence(s.value));
// Settings › iPhone pictures the exact phone (the Feed's first-run panel stays as it was).
const settingsArt = computed(() => props.context === "settings");
const model = computed(() => phoneModel(s.value.device?.model));
</script>

<template>
  <div :class="context === 'feed' ? 'flex h-full min-h-0 flex-col overflow-y-auto' : ''">
    <div :class="['flex flex-col gap-6', context === 'feed' ? 'mx-auto w-full max-w-[560px] px-8 py-10' : '']">
      <!-- Heading (only when this panel stands in for the Feed; Settings already titles the page). -->
      <div v-if="context === 'feed'">
        <div class="flex items-center justify-between gap-3">
          <p class="caption-upper text-muted">Welcome to tug</p>
          <button class="btn-secondary btn-sm" @click="tug.openSettings()"><Settings :size="13" /> Settings</button>
        </div>
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
          {{ confirmReset ? "Click again to start over" : "Start over" }}
        </button>
      </div>

      <!-- The phone looks to have forgotten this PC while Windows still holds the bond: remove it
           here (unpairs both bonds and scans fresh) rather than leaving them unable to reconnect. -->
      <div v-if="bond" class="flex flex-col gap-2 rounded-xl border border-error/30 bg-canvas px-4 py-3">
        <p class="text-[13px] text-body-strong">
          Your iPhone has forgotten this PC — remove it and pair again.
        </p>
        <div class="flex gap-2">
          <button class="btn-primary btn-sm" :disabled="resetting" @click="startOver">
            <LoaderCircle v-if="resetting" :size="13" class="animate-spin" />
            {{ confirmReset ? "Click again to remove" : "Remove" }}
          </button>
          <button class="btn-secondary btn-sm" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
        </div>
      </div>

      <!-- Paired: the phone, the switches ticking green, and a way to start over. -->
      <template v-if="s.device">
        <section class="rounded-xl border border-hairline bg-canvas p-5">
          <!-- In Settings, the phone pictured beside its name (model read over Bluetooth). -->
          <div class="flex items-center gap-5">
            <PhoneArt v-if="settingsArt" :face="model.face" :size="model.size" :height="112" :lit="s.connection === 'connected'" />
            <div class="min-w-0 flex-1">
              <p class="caption-upper text-muted">Your iPhone</p>
              <p class="headline mt-1 text-[24px]">{{ s.device.name }}</p>
              <p v-if="settingsArt && model.known" class="text-[13px] text-muted">{{ model.name }}</p>

              <!-- Fresh bond: iOS holds the connection open until "Allow" is tapped on the phone. -->
              <div v-if="step === 'allow'" class="mt-3 flex items-center gap-3 rounded-xl bg-surface-card px-4 py-3">
                <Smartphone :size="18" class="shrink-0 text-ink" />
                <p class="min-w-0 flex-1 text-[13px] text-body">
                  Look at your iPhone and tap <strong class="font-medium text-body-strong">Allow</strong> to let this PC see your notifications.
                </p>
              </div>
              <p v-else-if="busy" class="mt-2 flex items-center gap-2 text-[13px] text-muted">
                <LoaderCircle :size="14" class="animate-spin" /> {{ sentence }}
              </p>
              <p v-else class="mt-1 text-[13px] text-muted">{{ sentence }}</p>
            </div>
          </div>
        </section>

        <!-- The three switches on the iPhone, each lighting up green as it comes on. -->
        <div>
          <p class="text-center text-[14px] text-muted">
            On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong>, tap
            <strong class="font-medium text-body-strong">ⓘ</strong> next to this PC, and turn these on. They light up here as you do.
          </p>
          <PhoneSwitches :switches="switches" @recheck="tug.checkSwitches()" />
          <button
            v-if="canSkip"
            class="mx-auto mt-2 block text-[13px] text-muted underline-offset-2 hover:underline"
            @click="tug.connectSkipped = true"
          >
            Skip for now
          </button>
        </div>

        <button class="btn-secondary btn-sm self-start" :disabled="resetting" @click="startOver">
          <LoaderCircle v-if="resetting" :size="13" class="animate-spin" />
          {{ confirmReset ? "Click again to start over" : "Start over" }}
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

        <!-- An iPhone still paired in Windows from a previous install (the phone may have forgotten
             this PC): adopt it, or remove both bonds and pair fresh. Shown up front, not buried. -->
        <div v-if="leftover" class="flex flex-col gap-2 rounded-xl border border-hairline bg-surface-card px-4 py-3">
          <div class="flex items-center gap-3">
            <span class="flex size-9 shrink-0 items-center justify-center rounded-full bg-canvas"><Smartphone :size="17" class="text-ink" /></span>
            <div class="min-w-0 flex-1">
              <p class="truncate text-[14px] font-medium text-ink">{{ phoneLabel(leftover) }}</p>
              <p class="text-[12px] text-muted">Paired before</p>
            </div>
          </div>
          <p class="text-[13px] text-body">
            This PC is still paired with it from before. Use it if your iPhone still trusts this PC, or remove it and pair again.
          </p>
          <div class="flex gap-2">
            <button class="btn-primary btn-sm" :disabled="busyId !== null || removing" @click="choose(leftover)">
              <LoaderCircle v-if="busyId === leftover.id" :size="13" class="animate-spin" />
              Use
            </button>
            <button class="btn-secondary btn-sm" :disabled="busyId !== null || removing" @click="removeLeftover(leftover.id)">
              <LoaderCircle v-if="removing" :size="13" class="animate-spin" />
              {{ confirmRemove ? "Click again to remove" : "Remove" }}
            </button>
          </div>
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
                <p v-else-if="isNameless(d)" class="text-[12px] text-muted-soft">Finding name…</p>
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
            <li v-if="primaryPhones.length === 0 && scanning" class="flex items-center gap-3 rounded-xl border border-dashed border-hairline px-4 py-6 text-[13px] text-muted-soft">
              <LoaderCircle :size="15" class="animate-spin" /> Looking for your iPhone… keep Settings › Bluetooth open on it.
            </li>
            <li v-else-if="primaryPhones.length === 0" class="rounded-xl border border-dashed border-hairline px-4 py-6 text-[13px] text-muted-soft">
              Not looking right now. Press Look again with Settings › Bluetooth open on your iPhone.
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
