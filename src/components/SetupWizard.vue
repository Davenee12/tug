<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { ArrowLeft, Bluetooth, Check, LoaderCircle, MapPin, Monitor, Radio, Smartphone } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useWeatherStore } from "../stores/weather";
import { useFocusTrap } from "../lib/focusTrap";
import PhoneSwitches from "./PhoneSwitches.vue";
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

// ---- Connect: the phone shows up in discovery once LightBlue connects to this PC ----
// Only connected devices that could be a phone: a keyboard or headphones also show up
// connected, and must never be offered as the iPhone. Likely phones first.
const KIND_RANK: Record<DiscoveredDevice["kind"], number> = { phone: 0, unknown: 1, accessory: 2 };
const candidates = computed(() =>
  tug.discovered
    .filter((d) => d.connected && d.transport === "le" && d.kind !== "accessory")
    .sort((a, b) => KIND_RANK[a.kind] - KIND_RANK[b.kind])
    .slice(0, 3),
);
/** One device that says it's a phone: offer it plainly. Otherwise ask which one. */
const sure = computed(() => candidates.value.length === 1 && candidates.value[0].kind === "phone");
const others = computed(() => tug.discovered.filter((d) => !candidates.value.includes(d) && d.transport === "le").slice(0, 3));
const pairing = ref(false);
const choosingId = ref<string | null>(null);
const connectError = ref<string | null>(null);
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
  // The result decides, not the PIN prompt closing: Windows closes it a moment before the
  // phone is actually chosen, so "closed, no device yet" is the normal path to success.
  const ok = d.paired ? await tug.useDevice(d.id) : await tug.pair(d.id);
  pairing.value = false;
  if (!ok && !s.value.device) {
    connectError.value = "Pairing didn't finish. Try again: tap this PC in LightBlue, then choose your iPhone here.";
    go("connect");
  }
}
// The phone forgot this PC (e.g. Forget This Device): drop Windows' half of the old pairing
// and pair from scratch, instead of retrying a bond the phone will never accept.
const repairing = ref(false);
async function pairAgain() {
  repairing.value = true;
  await tug.forget();
  repairing.value = false;
  go("connect");
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

// ---- Sharing: the notifications switch, mirrored live ----
const sharing = computed(() => [
  { label: "Share System Notifications", why: "Your notifications on this PC", on: s.value.services.notifications, required: true },
]);
const sharingDone = computed(() => sharing.value.every((x) => x.on));
const connecting = computed(() => s.value.connection !== "connected");

// ---- Texts: a second, Classic pairing made from Windows, then two more switches ----
// It comes after the notifications pairing on purpose: pairing for notifications when the
// phone was already paired for texts broke the texts pairing in testing; this order kept both.
const texts = computed(() => [
  { label: "Show Notifications", why: "Read and reply to texts", on: s.value.services.messages && !s.value.messagesError, required: false },
  { label: "Sync Contacts", why: "Names instead of numbers", on: tug.contacts.length > 0 && !s.value.contactsError, required: false },
]);
const textsDone = computed(() => texts.value.every((x) => x.on));
/** The phone answers for texts (connected, or asking for its switch): the Classic pairing works. */
const textsPaired = computed(() => s.value.services.messages || s.value.textsPairing === "ok");

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

onMounted(() => void nextTick(() => root.value?.querySelector<HTMLElement>("[data-autofocus]")?.focus()));
onUnmounted(() => {
  tug.setupSharingShown = false;
  window.clearTimeout(btTimer);
  window.clearTimeout(shareTimer);
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
            About 3 minutes. Over Bluetooth, with nothing to install on your phone but a free helper app, once. Your notifications and texts stay on this PC.
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
          <h1 class="headline text-[36px] leading-tight">Connect from your iPhone</h1>
          <p class="mt-2 text-[15px] text-muted">
            iPhones only connect to accessories from the phone's side, so this one time you'll use a free app called
            <strong class="font-medium text-body-strong">LightBlue</strong>.
          </p>
          <div class="mt-8 flex gap-8">
            <ol class="flex flex-1 flex-col gap-4 text-[14px] text-body">
              <li class="flex gap-3">
                <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">1</span>
                <span>Install <strong class="font-medium text-ink">LightBlue</strong> from the App Store and open it near this PC.</span>
              </li>
              <li class="flex gap-3">
                <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">2</span>
                <span>Tap the device called <strong class="font-medium text-ink">Unnamed</strong> with the strongest signal (the number closest to zero).</span>
              </li>
              <li class="flex gap-3">
                <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">3</span>
                <span>Your iPhone appears here. Choose it to pair.</span>
              </li>
            </ol>
            <!-- What LightBlue looks like, so you know what to tap -->
            <div class="w-[190px] shrink-0 rounded-[30px] border-[6px] border-ink bg-canvas p-3 shadow-sm" aria-hidden="true">
              <p class="text-center text-[10px] font-semibold text-ink">LightBlue</p>
              <ul class="mt-2 flex flex-col gap-1.5 text-[11px]">
                <li class="flex items-center justify-between rounded-lg bg-accent-amber/25 px-2 py-1.5 ring-2 ring-accent-amber">
                  <span class="font-medium text-ink">Unnamed</span><span class="font-mono text-muted">−45</span>
                </li>
                <li class="flex items-center justify-between px-2 py-1.5"><span class="text-body">LE-Bose Flex</span><span class="font-mono text-muted-soft">−71</span></li>
                <li class="flex items-center justify-between px-2 py-1.5"><span class="text-body">Unnamed</span><span class="font-mono text-muted-soft">−88</span></li>
              </ul>
            </div>
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
                  <p class="text-[12px] font-medium text-accent-teal">Connected now</p>
                </div>
                <button
                  :class="i === 0 ? 'btn-primary' : 'btn-secondary'"
                  :disabled="pairing"
                  :data-autofocus="i === 0 ? '' : undefined"
                  @click="choose(d)"
                >
                  <LoaderCircle v-if="pairing && choosingId === d.id" :size="14" class="animate-spin" /> This is my iPhone
                </button>
              </div>
            </div>
            <p v-else class="flex items-center gap-3 text-[14px] text-muted">
              <LoaderCircle :size="16" class="animate-spin" /> Waiting for your iPhone to connect…
            </p>
            <p v-if="connectError" class="mt-3 text-[13px] text-error">{{ connectError }}</p>
          </div>
          <details class="mt-4 text-[13px] text-muted">
            <summary class="cursor-pointer select-none">Can't tell which one is this PC?</summary>
            <p class="mt-2">
              Switch <em>Visible to iPhone</em> off for a moment
              <button class="ml-1 underline" @click="tug.setAdvertising(!tug.advertiseEnabled)">
                ({{ tug.advertiseEnabled ? "turn off" : "turn back on" }})
              </button>: the entry that disappears in LightBlue is this PC. Turn it back on and tap it.
              <template v-if="others.length"> Other devices nearby: {{ others.map((d) => d.name).join(", ") }}.</template>
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
          <div v-if="s.pairingStale" class="mt-6 flex items-center gap-3 rounded-xl bg-surface-card px-4 py-3">
            <p class="min-w-0 flex-1 text-[13px] text-body">Your iPhone has forgotten this PC, so it can't connect. Pair again: it takes a few seconds.</p>
            <button class="btn-primary btn-sm" :disabled="repairing" @click="pairAgain">
              <LoaderCircle v-if="repairing" :size="13" class="animate-spin" /> Pair again
            </button>
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
          <template v-if="!textsPaired">
            <p class="mt-2 text-[15px] text-muted">
              Reading and replying to texts uses a second Bluetooth connection, made from this PC. Optional, about a minute.
            </p>
            <p v-if="s.textsPairing === 'broken'" class="mt-4 rounded-xl bg-surface-card px-4 py-3 text-[13px] text-body">
              Windows has an older texts pairing with {{ s.textsDevice ?? "your iPhone" }} that's stopped working. Remove it in Bluetooth settings
              first, then add your iPhone again.
            </p>
            <ol class="mt-8 flex flex-col gap-4 text-[14px] text-body">
              <li class="flex gap-3">
                <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-card text-[12px] font-medium text-ink">1</span>
                <span>On your iPhone, open <strong class="font-medium text-body-strong">Settings › Bluetooth</strong> and leave it on screen.</span>
              </li>
              <li class="flex gap-3">
                <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-card text-[12px] font-medium text-ink">2</span>
                <span>
                  On this PC, open Bluetooth settings, choose <strong class="font-medium text-body-strong">Add device › Bluetooth</strong>, pick your iPhone,
                  and confirm the code on both screens.
                  <button class="btn-secondary btn-sm mt-2 flex" data-autofocus @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
                </span>
              </li>
            </ol>
            <p class="mt-8 flex items-center gap-2 text-[13px] text-muted">
              <LoaderCircle :size="14" class="animate-spin" /> Waiting for your iPhone to answer for texts…
            </p>
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
