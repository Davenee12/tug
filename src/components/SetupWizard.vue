<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { ArrowLeft, Bluetooth, Check, LoaderCircle, MapPin, Monitor, Radio, Smartphone } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useWeatherStore } from "../stores/weather";
import { useFocusTrap } from "../lib/focusTrap";
import PhoneSwitches from "./PhoneSwitches.vue";
import { phoneSwitches } from "../lib/phoneSwitches";
import { bondHint, pairingProblem, setupDeviceLists, startedOutsideTug } from "../lib/pairings";
import { api, errorMessage } from "../lib/ipc";
import { friendlyLocateError } from "../lib/locating";
import { useLocating } from "../lib/useLocating";
import SettingsSwitch from "./SettingsSwitch.vue";
import TugMark from "./TugMark.vue";
import type { DiscoveredDevice, PhoneNotification } from "../types/protocol";

// First-run setup, one task per screen. Wherever tug can see a step happen (Bluetooth,
// the phone connecting, the PIN, the iPhone's switches, the first notification), the
// screen reacts by itself instead of asking "done?".
const tug = useTugStore();
const weather = useWeatherStore();
const root = ref<HTMLElement | null>(null);
useFocusTrap(root);

type Step = "welcome" | "bluetooth" | "connect" | "pin" | "sharing" | "texts" | "try" | "personal" | "done";
const ORDER: Step[] = ["welcome", "bluetooth", "connect", "pin", "sharing", "texts", "try", "personal", "done"];
const PROGRESS: Array<[Step[], string]> = [
  [["bluetooth"], "Bluetooth"],
  [["connect", "pin"], "Connect"],
  [["sharing"], "Notifications"],
  [["texts"], "Texts"],
  [["try"], "Try it"],
  [["personal", "done"], "Finish"],
];
const step = ref<Step>("welcome");
const progressIndex = computed(() => PROGRESS.findIndex(([steps]) => steps.includes(step.value)));

function go(next: Step) {
  step.value = next;
  void nextTick(() => root.value?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
}
function back() {
  const i = ORDER.indexOf(step.value);
  // Never step back into the PIN screen; it only exists while Windows is asking.
  let prev = ORDER[Math.max(0, i - 1)];
  if (prev === "pin") prev = "connect";
  go(prev);
}

const s = computed(() => tug.status);

// ---- Bluetooth: three things this PC needs, skipped when they're already fine ----
const btChecks = computed(() => [
  { label: "Bluetooth is on", ok: s.value.radio === "on", wait: s.value.radio === "unknown" },
  {
    label: "This PC can host a connection",
    ok: s.value.peripheralSupported === true,
    wait: s.value.peripheralSupported === null,
  },
  { label: "Visible to iPhone", ok: s.value.advertising === "on", wait: s.value.advertising === "starting" },
]);
const btReady = computed(() => btChecks.value.every((c) => c.ok));
let btTimer: number | undefined;
watch(
  [step, btReady],
  ([st, ready]) => {
    window.clearTimeout(btTimer);
    if (st === "bluetooth" && ready) btTimer = window.setTimeout(() => step.value === "bluetooth" && go("connect"), 1200);
  },
  { immediate: true },
);

// ---- Connect: the iPhone appears in the scan whenever its Settings › Bluetooth screen is open ----
// It does NOT have to connect to the PC first (the old LightBlue assumption, which left this list
// empty on a clean pair). Phones Windows classifies as such are offered up front; the list/filter
// is pure and unit-tested in lib/pairings. Scanning runs continuously while this step is open.
const lists = computed(() => setupDeviceLists(tug.discovered));
// Windows can drop an unpaired LE link nobody holds within seconds, which made a just-appeared
// iPhone vanish before it could be clicked. Hold a phone we've seen in the list for a short grace
// so a transient discovery blip doesn't pull the row out from under the cursor.
const CANDIDATE_GRACE_MS = 10_000;
const heldPhones = new Map<string, { device: DiscoveredDevice; at: number }>();
const graceNow = ref(Date.now());
let graceTimer: number | undefined;
const candidates = computed(() => {
  const t = graceNow.value;
  const live = lists.value.phones;
  for (const d of live) heldPhones.set(d.id, { device: d, at: Date.now() });
  const liveIds = new Set(live.map((d) => d.id));
  const stale = [...heldPhones.values()]
    .filter((e) => !liveIds.has(e.device.id) && t - e.at < CANDIDATE_GRACE_MS)
    .map((e) => e.device);
  return [...live, ...stale].slice(0, 4);
});
/** One device that says it's a phone: offer it plainly. Otherwise ask which one. */
const sure = computed(() => candidates.value.length === 1);
/** Unknowns (which could be a nameless just-connected iPhone): a quiet fallback list. */
const otherDevices = computed(() => lists.value.others.slice(0, 5));
const pairing = ref(false);
const choosingId = ref<string | null>(null);
const connectError = ref<string | null>(null);

// Windows' own pairing dialog (phone-initiated: tapping this PC in the iPhone's Bluetooth list)
// can create the bond before tug does, and then the code shows in Windows, not tug. Detect a
// device flipping unpaired→paired that tug didn't start, so we can point at Windows' prompt.
const windowsPairing = ref(false);
const initiatedPair = new Set<string>();
const pairedBefore = new Map<string, boolean>();
watch(
  () => tug.discovered.map((d) => [d.id, d.paired] as const),
  (now) => {
    for (const [id, paired] of now) {
      if (startedOutsideTug(pairedBefore.get(id), paired, initiatedPair.has(id))) windowsPairing.value = true;
      pairedBefore.set(id, paired);
    }
  },
);
watch(
  step,
  (st, old) => {
    // Already paired (setup run again from Settings): nothing to connect, carry on.
    if (st === "connect" && s.value.device && old !== "sharing") return go("sharing");
    if (st === "connect" && !s.value.device) void tug.startDiscovery();
    if (old === "connect" && st !== "pin") void tug.stopDiscovery();
  },
  { immediate: true },
);
async function choose(d: DiscoveredDevice) {
  connectError.value = null;
  pairing.value = true;
  choosingId.value = d.id;
  // Prefer tug's own pairing for an unpaired phone, so the code shows in tug's prompt, not
  // Windows'. Record that we started it, so the Windows-pairing hint doesn't fire for our flip.
  if (!d.paired) initiatedPair.add(d.id);
  // The result decides, not the PIN prompt closing: Windows closes it a moment before the
  // phone is actually chosen, so "closed, no device yet" is the normal path to success.
  const ok = d.paired ? await tug.useDevice(d.id) : await tug.pair(d.id);
  pairing.value = false;
  if (!ok && !s.value.device) {
    connectError.value = "Pairing didn't finish. Keep Settings › Bluetooth open on your iPhone and try Pair again.";
    go("connect");
  }
}
// The phone looks to have forgotten this PC while Windows still holds the bond (stale-bond
// errors, or a bond the phone never connects to): decided purely in lib/pairings.
const bond = computed(() => bondHint(s.value));

// Windows ended up with more than one iPhone bond (an old one and a new one), or the phone that's
// paired now isn't the one tug remembers. One clear "Start over" unpairs both bonds tug knows
// about and returns to the first step. Confirmed first, because unpairing is irreversible.
const problem = computed(() => pairingProblem(tug.discovered, s.value.device?.id ?? null));
const confirmStartOver = ref(false);
let startOverTimer: number | undefined;
const startingOver = ref(false);
async function startOver() {
  if (!confirmStartOver.value) {
    confirmStartOver.value = true;
    window.clearTimeout(startOverTimer);
    startOverTimer = window.setTimeout(() => (confirmStartOver.value = false), 4000);
    return;
  }
  confirmStartOver.value = false;
  startingOver.value = true;
  await tug.forget();
  startingOver.value = false;
  go("welcome");
}
// Windows asks for the PIN → its own screen; once paired, the device watcher moves on.
watch(
  () => tug.pairingRequest,
  (req) => {
    if (req && (step.value === "connect" || step.value === "pin")) go("pin");
  },
);
watch(
  () => s.value.device,
  (device) => {
    if (device && (step.value === "connect" || step.value === "pin")) {
      void tug.stopDiscovery();
      go("sharing");
    }
    // "Pair again" can land on Connect before the forget shows up in the status: start
    // looking once it does, or the step waits forever.
    if (!device && step.value === "connect") void tug.startDiscovery();
  },
);

// ---- The iPhone's three switches, each with a live state from real signals (lib/phoneSwitches) ----
const allSwitches = computed(() => phoneSwitches(s.value));

// ---- Sharing: the notifications switch, mirrored live ----
const sharing = computed(() => allSwitches.value.filter((x) => x.key === "notifications"));
const sharingDone = computed(() => sharing.value.every((x) => x.state === "on"));
const connecting = computed(() => s.value.connection !== "connected");

// ---- Texts: a second, Classic pairing made from Windows, then two more switches ----
// It comes after the notifications pairing on purpose: pairing for notifications when the
// phone was already paired for texts broke the texts pairing in testing; this order kept both.
const texts = computed(() => allSwitches.value.filter((x) => x.key === "messages" || x.key === "contacts"));
const textsDone = computed(() => texts.value.every((x) => x.state === "on"));
/** The phone answers for texts (connected, or asking for its switch): the Classic pairing works. */
const textsPaired = computed(() => s.value.services.messages || s.value.textsPairing === "ok");

// With one pairing the iPhone usually creates the Classic (texts) bond too (cross-transport keys),
// so the texts step first waits a short while to see it connect, and only offers to pair for texts
// (or the Windows fallback) if no bond turns up. Pairing for texts from inside tug (lib/ipc).
const TEXTS_WAIT_MS = 15_000;
const textsWaitElapsed = ref(false);
let textsWaitTimer: number | undefined;
const pairingTexts = ref(false);
async function pairForTexts() {
  pairingTexts.value = true;
  await tug.pairTexts();
  pairingTexts.value = false;
}

let shareTimer: number | undefined;
watch(
  [step, sharingDone, textsDone],
  ([st, shared, textsOn]) => {
    window.clearTimeout(shareTimer);
    if (st === "sharing" && shared) shareTimer = window.setTimeout(() => step.value === "sharing" && go("texts"), 1500);
    if (st === "texts" && textsOn) shareTimer = window.setTimeout(() => step.value === "texts" && go("try"), 1500);
  },
  { immediate: true },
);

// ---- Try it: the first notification that arrives after this screen opens ----
let baseline = 0;
const first = ref<PhoneNotification | null>(null);
watch(step, (st) => {
  tug.setupSharingShown = st === "sharing" || st === "texts";
  // Give the Classic bond a moment to come up on its own before offering to pair for texts.
  window.clearTimeout(textsWaitTimer);
  if (st === "texts") {
    textsWaitElapsed.value = false;
    textsWaitTimer = window.setTimeout(() => (textsWaitElapsed.value = true), TEXTS_WAIT_MS);
  }
  if (st === "try") {
    baseline = Math.max(0, ...tug.notifications.map((n) => n.id));
    first.value = null;
  }
});
watch(
  () => tug.notifications[0],
  (n) => {
    if (step.value === "try" && n && n.id > baseline && !n.flags.preExisting) first.value = n;
  },
);
const firstApp = computed(() => (first.value ? tug.appNameFor(first.value.appId) : ""));

// ---- Make it yours ----
const toasts = computed({ get: () => tug.settings.toasts, set: (v) => void tug.setSetting("toasts", v) });
const closeToTray = computed({ get: () => tug.settings.closeToTray, set: (v) => void tug.setSetting("closeToTray", v) });
const locating = ref(false);
const weatherNote = ref<string | null>(null);
const finding = useLocating();
async function addWeather() {
  weatherNote.value = null;
  locating.value = true;
  finding.start();
  try {
    const p = await weather.findMyPlace();
    await finding.succeed();
    await weather.setPlace(p);
  } catch (e) {
    weatherNote.value = friendlyLocateError(errorMessage(e));
  } finally {
    finding.stop();
    locating.value = false;
  }
}
const weatherPlace = computed(() => (weather.place && weather.place !== "off" ? weather.place.name : null));

function finish() {
  tug.finishSetup();
  tug.view = "feed";
}

onMounted(() => {
  void nextTick(() => root.value?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
  // Tick so the candidate grace expires devices that haven't reappeared.
  graceTimer = window.setInterval(() => (graceNow.value = Date.now()), 1000);
});
onUnmounted(() => {
  tug.setupSharingShown = false;
  window.clearTimeout(btTimer);
  window.clearTimeout(shareTimer);
  window.clearTimeout(startOverTimer);
  window.clearTimeout(textsWaitTimer);
  window.clearInterval(graceTimer);
  if (step.value === "connect") void tug.stopDiscovery();
});

const SHORTCUTS: Array<[string, string]> = [
  ["Ctrl K", "Search everything, or type an action"],
  ["Ctrl N", "New message"],
  ["Ctrl Shift C", "Copy the latest one-time code"],
  ["Ctrl ,", "Settings"],
];
</script>

<template>
  <div class="fixed inset-0 z-40 flex flex-col overflow-y-auto bg-canvas" role="dialog" aria-modal="true" aria-label="Set up tug">
    <div ref="root" class="mx-auto flex w-full max-w-[600px] flex-1 flex-col px-6 pt-8 pb-10">
      <!-- Progress: hidden on the welcome screen -->
      <header class="flex h-10 items-center gap-4">
        <TugMark :size="24" class="shrink-0 text-ink" />
        <template v-if="step !== 'welcome'">
          <ol class="flex flex-1 gap-1.5" aria-label="Setup progress">
            <li
              v-for="([, label], i) in PROGRESS"
              :key="label"
              :class="['h-1 flex-1 rounded-full transition-colors duration-500', i <= progressIndex ? 'bg-ink' : 'bg-hairline']"
              :aria-current="i === progressIndex ? 'step' : undefined"
              :aria-label="label"
            />
          </ol>
          <span class="w-24 text-right text-[12px] text-muted">{{ PROGRESS[progressIndex]?.[1] }}</span>
        </template>
      </header>

        <!-- Welcome -->
        <section v-if="step === 'welcome'" key="welcome" class="animate-step-in flex flex-1 flex-col justify-center py-10">
          <p class="caption-upper text-muted">Welcome to tug</p>
          <h1 class="headline mt-3 text-[52px] leading-[1.05]" style="letter-spacing: -0.02em">Your iPhone,<br />on your desk.</h1>
          <ul class="mt-8 flex flex-col gap-3 text-[15px] text-body">
            <li class="flex gap-3"><Check :size="18" class="mt-0.5 shrink-0 text-accent-teal" /> Every notification, here, as it happens.</li>
            <li class="flex gap-3"><Check :size="18" class="mt-0.5 shrink-0 text-accent-teal" /> Read and reply to texts from your keyboard.</li>
            <li class="flex gap-3"><Check :size="18" class="mt-0.5 shrink-0 text-accent-teal" /> Your music, battery and codes, one glance away.</li>
          </ul>
          <p class="mt-8 text-[13px] text-muted">
            About 3 minutes, over Bluetooth, with nothing to install on your phone. Your notifications and texts stay on this PC.
          </p>
          <div class="mt-8 flex items-center gap-4">
            <button class="btn-primary" data-autofocus @click="go('bluetooth')">Get started</button>
            <button class="text-[13px] text-muted active:text-ink" @click="finish">Skip setup</button>
          </div>
        </section>

        <!-- Bluetooth -->
        <section v-else-if="step === 'bluetooth'" key="bluetooth" class="animate-step-in flex flex-1 flex-col py-10">
          <h1 class="headline text-[36px] leading-tight">First, this PC</h1>
          <p class="mt-2 text-[15px] text-muted">tug talks to your iPhone over Bluetooth. Checking that everything's ready…</p>
          <ul class="mt-8 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <li v-for="(c, i) in btChecks" :key="c.label" class="flex items-center gap-4 px-5 py-4">
              <component :is="[Bluetooth, Monitor, Radio][i]" :size="18" class="shrink-0 text-muted" />
              <span class="flex-1 text-[14px] text-ink">{{ c.label }}</span>
              <Check v-if="c.ok" :size="18" class="text-accent-teal" />
              <LoaderCircle v-else-if="c.wait" :size="16" class="animate-spin text-muted-soft" />
              <span v-else class="pill bg-accent-amber/20 text-[12px] text-ink">Needs attention</span>
            </li>
          </ul>
          <div v-if="s.radio === 'off'" class="mt-4 rounded-xl border border-hairline px-5 py-4 text-[14px] text-body">
            Bluetooth is off.
            <button class="btn-secondary btn-sm ml-2" data-autofocus @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
          </div>
          <p v-else-if="s.radio === 'unavailable'" class="mt-4 text-[14px] text-body">
            This PC has no Bluetooth. A small Bluetooth 5 USB adapter fixes that.
          </p>
          <p v-else-if="s.peripheralSupported === false" class="mt-4 text-[14px] text-body">
            This PC's Bluetooth can't host a connection, so your iPhone can't reach it. A Bluetooth 5 USB adapter fixes that.
          </p>
          <div v-else-if="!tug.advertiseEnabled" class="mt-4 rounded-xl border border-hairline px-5 py-4 text-[14px] text-body">
            tug isn't visible to your iPhone.
            <button class="btn-secondary btn-sm ml-2" data-autofocus @click="tug.setAdvertising(true)">Make it visible</button>
          </div>
          <p v-if="btReady" class="mt-6 flex items-center gap-2 text-[14px] font-medium text-ink">
            <Check :size="16" class="text-accent-teal" /> All set. On to your iPhone.
          </p>
        </section>

        <!-- Connect -->
        <section v-else-if="step === 'connect'" key="connect" class="animate-step-in flex flex-1 flex-col py-10">
          <h1 class="headline text-[36px] leading-tight">Pair your iPhone</h1>
          <p class="mt-2 text-[15px] text-muted">
            This takes a few seconds and sets up everything — notifications, texts and your music — in one go.
          </p>
          <ol class="mt-8 flex flex-col gap-4 text-[14px] text-body">
            <li class="flex gap-3">
              <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">1</span>
              <span>On your iPhone, open <strong class="font-medium text-ink">Settings › Bluetooth</strong> and keep that screen open. Don't tap this PC in the list — just leave it showing.</span>
            </li>
            <li class="flex gap-3">
              <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">2</span>
              <span>Your iPhone appears below. Click <strong class="font-medium text-ink">Pair</strong> next to it.</span>
            </li>
            <li class="flex gap-3">
              <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">3</span>
              <span>A code appears here and on your iPhone — check they match and confirm on both.</span>
            </li>
            <li class="flex gap-3">
              <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">4</span>
              <span>Tap <strong class="font-medium text-ink">Allow</strong> on your iPhone when it asks to share notifications.</span>
            </li>
          </ol>

          <!-- More than one iPhone paired, or the wrong one: offer one clean Start over. -->
          <div v-if="problem" class="mt-8 flex flex-col gap-2 rounded-xl border border-hairline bg-surface-card px-4 py-3">
            <p class="text-[13px] text-body">
              <template v-if="problem === 'duplicates'">More than one iPhone is paired with this PC, so tug can't tell which to use.</template>
              <template v-else>The iPhone paired now isn't the one tug remembers.</template>
              Start over clears the pairings tug made and begins again. Also tap <strong class="font-medium text-body-strong">Forget This Device</strong>
              for this PC under Settings › Bluetooth on the iPhone.
            </p>
            <button class="btn-secondary btn-sm self-start" :disabled="startingOver" @click="startOver">
              <LoaderCircle v-if="startingOver" :size="13" class="animate-spin" />
              {{ confirmStartOver ? "Tap again to start over" : "Start over" }}
            </button>
          </div>

          <!-- Windows' own pairing dialog is handling it (the phone started the pairing). -->
          <div v-if="windowsPairing && !s.device" class="mt-8 flex items-start gap-3 rounded-xl border border-hairline bg-surface-card px-4 py-3">
            <Smartphone :size="18" class="mt-0.5 shrink-0 text-ink" />
            <p class="min-w-0 flex-1 text-[13px] text-body">
              Windows is pairing your iPhone and will show a code — check it matches your iPhone and confirm on both.
            </p>
          </div>

          <div class="mt-8 rounded-xl bg-surface-card p-4">
            <div v-if="s.device" class="flex items-center gap-4">
              <span class="flex size-10 items-center justify-center rounded-full bg-canvas"><Check :size="18" class="text-accent-teal" /></span>
              <p class="min-w-0 flex-1 truncate text-[15px] font-medium text-ink">Paired with {{ s.device.name }}</p>
              <button class="btn-primary" data-autofocus @click="go('sharing')">Continue</button>
            </div>
            <div v-else-if="candidates.length" class="flex flex-col gap-3">
              <p v-if="!sure" class="text-[13px] text-muted">
                {{ candidates.length > 1 ? "Which one is your iPhone?" : "Is this your iPhone?" }}
              </p>
              <div v-for="(d, i) in candidates" :key="d.id" class="flex items-center gap-4">
                <span class="flex size-10 items-center justify-center rounded-full bg-canvas"><Smartphone :size="18" class="text-ink" /></span>
                <div class="min-w-0 flex-1">
                  <p class="truncate text-[15px] font-medium text-ink">{{ d.name }}</p>
                  <p v-if="d.connected" class="text-[12px] font-medium text-accent-teal">Connected now</p>
                  <p v-else-if="d.paired" class="text-[12px] text-muted">Paired</p>
                </div>
                <button
                  :class="i === 0 ? 'btn-primary' : 'btn-secondary'"
                  :disabled="pairing"
                  :data-autofocus="i === 0 ? '' : undefined"
                  @click="choose(d)"
                >
                  <LoaderCircle v-if="pairing && choosingId === d.id" :size="14" class="animate-spin" />
                  {{ d.paired ? "Use" : "Pair" }}
                </button>
              </div>
            </div>
            <p v-else class="flex items-center gap-3 text-[14px] text-muted">
              <LoaderCircle :size="16" class="animate-spin" /> Looking for your iPhone… keep Settings › Bluetooth open on it.
            </p>
            <p v-if="connectError" class="mt-3 text-[13px] text-error">{{ connectError }}</p>
          </div>

          <!-- Other nearby devices tug couldn't confirm as a phone (possibly a nameless iPhone). -->
          <details v-if="otherDevices.length" class="mt-4 text-[13px] text-muted">
            <summary class="cursor-pointer select-none">Don't see your iPhone? Other nearby devices</summary>
            <ul class="mt-3 flex flex-col gap-2">
              <li v-for="d in otherDevices" :key="d.id" class="flex items-center gap-3 rounded-xl border border-hairline bg-canvas px-4 py-2.5">
                <p class="min-w-0 flex-1 truncate text-[13px] text-ink">{{ d.name }}</p>
                <button class="btn-secondary btn-sm" :disabled="pairing" @click="choose(d)">
                  <LoaderCircle v-if="pairing && choosingId === d.id" :size="13" class="animate-spin" />
                  {{ d.paired ? "Use" : "Pair" }}
                </button>
              </li>
            </ul>
          </details>

          <!-- Fallback for older iOS or when the phone never appears: the LightBlue route. -->
          <details class="mt-4 text-[13px] text-muted">
            <summary class="cursor-pointer select-none">Can't see your iPhone?</summary>
            <p class="mt-2">
              Keep <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> open on your iPhone — it only advertises while that
              screen is showing. If it still doesn't appear, install the free <strong class="font-medium text-body-strong">LightBlue</strong> app, open
              it near this PC, and tap the <strong class="font-medium text-body-strong">Unnamed</strong> entry with the strongest signal (closest to 0,
              e.g. −45) to wake the link; your iPhone then shows up here. To be sure which entry is this PC, switch
              <em>Visible to iPhone</em> off for a moment
              <button class="underline" @click="tug.setAdvertising(!tug.advertiseEnabled)">({{ tug.advertiseEnabled ? "turn off" : "turn back on" }})</button>:
              the one that disappears is this PC.
            </p>
          </details>
        </section>

        <!-- PIN -->
        <section
          v-else-if="step === 'pin' && !tug.pairingRequest"
          key="pairing"
          class="animate-step-in flex flex-1 flex-col items-center justify-center gap-4 py-10 text-center"
        >
          <LoaderCircle :size="22" class="animate-spin text-muted" />
          <p class="text-[15px] text-muted">Finishing pairing…</p>
        </section>
        <!-- ConfirmOnly: Windows has accepted; the user just taps Pair on the iPhone. -->
        <section
          v-else-if="step === 'pin' && tug.pairingRequest?.confirmOnPhone"
          key="pin-phone"
          class="animate-step-in flex flex-1 flex-col items-center justify-center gap-4 py-10 text-center"
        >
          <Smartphone :size="26" class="text-ink" />
          <h1 class="headline text-[36px] leading-tight">Tap Pair on your iPhone</h1>
          <p class="text-[15px] text-muted">{{ tug.pairingRequest?.deviceName }}</p>
          <p class="text-[14px] text-muted">Confirm the pairing on your iPhone — it continues here on its own.</p>
          <LoaderCircle :size="20" class="animate-spin text-muted-soft" />
        </section>
        <section v-else-if="step === 'pin'" key="pin" class="animate-step-in flex flex-1 flex-col items-center justify-center py-10 text-center">
          <h1 class="headline text-[36px] leading-tight">{{ tug.pairingRequest?.pin ? "Do the codes match?" : "Pair with your iPhone?" }}</h1>
          <p class="mt-2 text-[15px] text-muted">{{ tug.pairingRequest?.deviceName }}</p>
          <p v-if="tug.pairingRequest?.pin" class="my-8 font-mono text-[56px] tracking-[0.2em] text-ink">{{ tug.pairingRequest.pin }}</p>
          <p class="text-[14px] text-muted">
            {{ tug.pairingRequest?.pin ? "Check it matches the code on your iPhone, then tap Pair here and on the phone." : "Tap Pair here, then confirm on your iPhone." }}
          </p>
          <div class="mt-8 flex gap-3">
            <button class="btn-secondary" @click="tug.confirmPairing(false)">Cancel</button>
            <button class="btn-primary" data-autofocus @click="tug.confirmPairing(true)">Pair</button>
          </div>
        </section>

        <!-- Sharing -->
        <section v-else-if="step === 'sharing'" key="sharing" class="animate-step-in flex flex-1 flex-col py-10">
          <h1 class="headline text-[36px] leading-tight">Turn on sharing</h1>
          <p class="mt-2 text-[15px] text-muted">
            On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong>, tap
            <strong class="font-medium text-body-strong">ⓘ</strong> next to this PC, and switch this on. It lights up here as you do.
          </p>
          <PhoneSwitches :switches="sharing" />
          <div v-if="bond" class="mt-6 flex flex-col gap-2 rounded-xl bg-surface-card px-4 py-3">
            <p class="text-[13px] text-body">
              <template v-if="bond === 'forgotten'">Your PC still remembers a pairing your iPhone forgot, so they can't reconnect.</template>
              <template v-else>Your iPhone keeps refusing this PC's pairing — it may have been forgotten on the phone.</template>
              Remove <strong class="font-medium text-body-strong">{{ s.textsDevice ?? s.device?.name ?? "your iPhone" }}</strong> in Windows Bluetooth
              settings, or Start over here, then pair again.
            </p>
            <div class="flex gap-2">
              <button class="btn-primary btn-sm" :disabled="startingOver" @click="startOver">
                <LoaderCircle v-if="startingOver" :size="13" class="animate-spin" />
                {{ confirmStartOver ? "Tap again to start over" : "Start over" }}
              </button>
              <button class="btn-secondary btn-sm" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
            </div>
          </div>
          <div v-else-if="s.awaitingPhoneAllow" class="mt-6 flex items-center gap-3 rounded-xl bg-surface-card px-4 py-3">
            <Smartphone :size="18" class="shrink-0 text-ink" />
            <p class="min-w-0 flex-1 text-[13px] text-body">Look at your iPhone and tap <strong class="font-medium text-body-strong">Allow</strong> to let this PC see your notifications.</p>
          </div>
          <p v-else-if="connecting" class="mt-6 flex items-center justify-center gap-2 text-[13px] text-muted">
            <LoaderCircle :size="14" class="animate-spin" /> Connecting to your iPhone…
          </p>
          <p v-else-if="sharingDone" class="mt-6 flex items-center justify-center gap-2 text-[14px] font-medium text-ink">
            <Check :size="16" class="text-accent-teal" /> Notifications are on.
          </p>
          <div class="mt-auto flex justify-end pt-8">
            <button class="btn-primary" :disabled="!sharingDone" data-autofocus @click="go('texts')">Continue</button>
          </div>
        </section>

        <!-- Texts: pair for messages from Windows, then two switches on the phone -->
        <section v-else-if="step === 'texts'" key="texts" class="animate-step-in flex flex-1 flex-col py-10">
          <h1 class="headline text-[36px] leading-tight">Bring your texts over</h1>
          <!-- The one pairing usually brings texts too; wait briefly before offering to pair. -->
          <template v-if="!textsPaired && !textsWaitElapsed">
            <p class="mt-2 text-[15px] text-muted">Setting up texts over the same pairing…</p>
            <div class="mt-10 flex flex-col items-center gap-4 rounded-xl border border-dashed border-hairline px-6 py-12 text-center">
              <LoaderCircle :size="20" class="animate-spin text-muted" />
              <p class="text-[14px] text-muted">Checking whether your iPhone shares texts over this pairing…</p>
            </div>
          </template>
          <template v-else-if="!textsPaired">
            <p class="mt-2 text-[15px] text-muted">
              Reading and replying to texts needs a second Bluetooth pairing. tug can make it for you — about a minute, and optional.
            </p>
            <p v-if="s.textsPairing === 'broken'" class="mt-4 rounded-xl bg-surface-card px-4 py-3 text-[13px] text-body">
              Windows has an older texts pairing with {{ s.textsDevice ?? "your iPhone" }} that's stopped working. Press Start over, or remove it in
              Bluetooth settings first, then pair again.
            </p>
            <!-- Pairing in progress: the code shows right here (ConfirmPinMatch) or on the phone. -->
            <div v-if="pairingTexts && tug.pairingRequest" class="mt-8 rounded-xl bg-surface-card px-5 py-5 text-center">
              <p class="text-[14px] font-medium text-ink">{{ tug.pairingRequest.confirmOnPhone ? "Tap Pair on your iPhone" : "Do the codes match?" }}</p>
              <p v-if="tug.pairingRequest.pin" class="my-4 font-mono text-[44px] tracking-[0.2em] text-ink">{{ tug.pairingRequest.pin }}</p>
              <p class="text-[13px] text-muted">
                {{ tug.pairingRequest.confirmOnPhone ? "Confirm on your iPhone — it continues here." : "Check it matches your iPhone, then tap Pair on both." }}
              </p>
              <div v-if="!tug.pairingRequest.confirmOnPhone" class="mt-4 flex justify-center gap-3">
                <button class="btn-secondary btn-sm" @click="tug.confirmPairing(false)">Cancel</button>
                <button class="btn-primary btn-sm" @click="tug.confirmPairing(true)">Pair</button>
              </div>
            </div>
            <template v-else>
              <ol class="mt-8 flex flex-col gap-4 text-[14px] text-body">
                <li class="flex gap-3">
                  <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-card text-[12px] font-medium text-ink">1</span>
                  <span>On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> and leave it on screen.</span>
                </li>
                <li class="flex gap-3">
                  <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-card text-[12px] font-medium text-ink">2</span>
                  <span>Click Pair for texts — a code shows here and on your iPhone; confirm on both.</span>
                </li>
              </ol>
              <button class="btn-primary mt-6 self-start" data-autofocus :disabled="pairingTexts" @click="pairForTexts">
                <LoaderCircle v-if="pairingTexts" :size="14" class="animate-spin" /> Pair for texts
              </button>
              <details class="mt-4 text-[13px] text-muted">
                <summary class="cursor-pointer select-none">Pair from Windows instead</summary>
                <p class="mt-2">
                  With <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> open on your iPhone, open Bluetooth settings on this PC,
                  choose <strong class="font-medium text-body-strong">Add device › Bluetooth</strong>, pick your iPhone and confirm the code on both.
                  <button class="btn-secondary btn-sm mt-2 flex" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
                </p>
              </details>
            </template>
          </template>
          <template v-else>
            <p class="mt-2 text-[15px] text-muted">
              Paired for texts. On your iPhone, tap <strong class="font-medium text-body-strong">ⓘ</strong> next to this PC again and switch these on. Sync
              Contacts appears a few seconds after the first.
            </p>
            <PhoneSwitches :switches="texts" />
            <p v-if="textsDone" class="mt-6 flex items-center justify-center gap-2 text-[14px] font-medium text-ink">
              <Check :size="16" class="text-accent-teal" /> Perfect. Texts and names are coming over.
            </p>
          </template>
          <div class="mt-auto flex items-center justify-end gap-4 pt-8">
            <button v-if="!textsDone" class="text-[13px] text-muted active:text-ink" @click="go('try')">Skip for now</button>
            <button v-else class="btn-primary" data-autofocus @click="go('try')">Continue</button>
          </div>
        </section>

        <!-- Try it -->
        <section v-else-if="step === 'try'" key="try" class="animate-step-in flex flex-1 flex-col py-10">
          <template v-if="!first">
            <h1 class="headline text-[36px] leading-tight">Let's see it work</h1>
            <p class="mt-2 text-[15px] text-muted">
              Get a notification on your iPhone. The easiest way: say to Siri,
              <strong class="font-medium text-body-strong">“Remind me in one minute to try tug.”</strong> Or ask a friend to text you.
            </p>
            <div class="mt-10 flex flex-col items-center gap-4 rounded-xl border border-dashed border-hairline px-6 py-12 text-center">
              <span class="relative flex size-3"><span class="absolute inline-flex size-full animate-ping rounded-full bg-accent-teal opacity-60" /><span class="relative inline-flex size-3 rounded-full bg-accent-teal" /></span>
              <p class="text-[14px] text-muted">Listening for your next notification…</p>
            </div>
            <div class="mt-auto flex justify-end pt-8">
              <button class="text-[13px] text-muted active:text-ink" @click="go('personal')">Skip for now</button>
            </div>
          </template>
          <template v-else>
            <h1 class="headline text-[36px] leading-tight">There it is.</h1>
            <p class="mt-2 text-[15px] text-muted">That just came from your iPhone. Every notification will land in tug like this.</p>
            <div class="mx-auto mt-10 w-full max-w-[420px] rounded-2xl bg-surface-card px-5 py-4 shadow-sm">
              <p class="flex items-baseline gap-2 text-[12px] text-muted"><span class="caption-upper">{{ firstApp }}</span><span class="ml-auto">now</span></p>
              <p class="mt-1 text-[15px] font-medium text-ink">{{ first.title }}</p>
              <p class="text-[14px] text-body">{{ first.message || first.subtitle }}</p>
            </div>
            <div class="mt-auto flex justify-end pt-8">
              <button class="btn-primary" data-autofocus @click="go('personal')">Continue</button>
            </div>
          </template>
        </section>

        <!-- Make it yours -->
        <section v-else-if="step === 'personal'" key="personal" class="animate-step-in flex flex-1 flex-col py-10">
          <h1 class="headline text-[36px] leading-tight">Make it yours</h1>
          <p class="mt-2 text-[15px] text-muted">All optional, and all in Settings later.</p>
          <div class="mt-8 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <div class="flex items-center gap-6 px-5 py-4">
              <div class="flex-1">
                <p class="text-[14px] font-medium text-ink">Windows alerts</p>
                <p class="text-[13px] text-muted">Pop up new notifications on this PC.</p>
              </div>
              <SettingsSwitch v-model="toasts" label="Windows alerts" />
            </div>
            <div class="flex items-center gap-6 px-5 py-4">
              <div class="flex-1">
                <p class="text-[14px] font-medium text-ink">Keep running when closed</p>
                <p class="text-[13px] text-muted">tug stays in the tray, still mirroring your iPhone.</p>
              </div>
              <SettingsSwitch v-model="closeToTray" label="Keep running when closed" />
            </div>
            <div class="flex items-center gap-6 px-5 py-4">
              <div class="flex-1">
                <p class="text-[14px] font-medium text-ink">Weather on the Feed</p>
                <p v-if="finding.line.value" class="text-[13px]" role="status" aria-live="polite">
                  <Transition
                    mode="out-in"
                    enter-active-class="transition duration-300 ease-out motion-reduce:transition-none"
                    enter-from-class="opacity-0 translate-y-1"
                    leave-active-class="transition duration-200 ease-in motion-reduce:transition-none"
                    leave-to-class="opacity-0 -translate-y-1"
                  >
                    <span :key="finding.line.value" :class="['inline-block', finding.found.value ? 'font-medium text-ink' : 'text-muted']">{{ finding.line.value }}</span>
                  </Transition>
                </p>
                <p v-else class="text-[13px] text-muted">
                  <template v-if="weatherPlace">Showing {{ weatherPlace }}.</template>
                  <template v-else>A forecast at the top of your notifications.</template>
                </p>
                <p v-if="weatherNote" class="mt-1 text-[12px] text-error">{{ weatherNote }}</p>
              </div>
              <button v-if="!weatherPlace" class="btn-secondary btn-sm" :disabled="locating" @click="addWeather">
                <MapPin :size="13" :class="locating ? 'animate-pulse motion-reduce:animate-none' : ''" /> Use my location
              </button>
              <Check v-else :size="18" class="text-accent-teal" />
            </div>
          </div>
          <div class="mt-auto flex justify-end pt-8">
            <button class="btn-primary" data-autofocus @click="go('done')">Continue</button>
          </div>
        </section>

        <!-- Done -->
        <section v-else key="done" class="animate-step-in flex flex-1 flex-col justify-center py-10">
          <h1 class="headline text-[44px] leading-tight">You're all set.</h1>
          <p class="mt-3 text-[15px] text-muted">
            tug lives in the tray by the clock and reconnects to your iPhone on its own. A few keys worth knowing:
          </p>
          <ul class="mt-8 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <li v-for="[keys, what] in SHORTCUTS" :key="keys" class="flex items-center gap-4 px-5 py-3 text-[14px] text-ink">
              <span class="flex gap-1">
                <kbd v-for="k in keys.split(' ')" :key="k" class="rounded border border-hairline bg-canvas px-1.5 font-mono text-[12px] text-body">{{ k }}</kbd>
              </span>
              <span class="text-body">{{ what }}</span>
            </li>
          </ul>
          <div class="mt-10">
            <button class="btn-primary" data-autofocus @click="finish">Open tug</button>
          </div>
        </section>

      <!-- Back: everywhere it makes sense (not mid-pairing, not at the ends) -->
      <footer v-if="!['welcome', 'pin', 'done'].includes(step)" class="pt-2">
        <button class="flex items-center gap-1.5 text-[13px] text-muted active:text-ink" @click="back">
          <ArrowLeft :size="14" /> Back
        </button>
      </footer>
    </div>
  </div>
</template>
