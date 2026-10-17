<script setup lang="ts">
// The phone as a game's controller, while one of tug's games on the PC asks for one. Held sideways
// like a gamepad: the left thumb has the slider, the right thumb the big button.
//   Tugboat Run: the slider steers (the boat goes where the knob is, and the knob stays where you
//   let go) and Boost hops.
//   Harbor Stack: the slider nudges the crane slower or faster (it springs back to the middle when
//   let go) and Drop lets the container go.
// Held upright, the page asks to be turned (Safari has no orientation lock); "Play in portrait
// anyway" keeps the upright layout. The PC says which game it is, at the start and in every reply,
// so the buttons follow the game.
//
// Inputs go out the moment something changes: a touch, a slide, the button pressed or let go.
// Changes are coalesced to at most one every 16 ms (~60 a second) and up to two requests are in
// flight, so a slow reply never holds up the next input; the PC keeps only the newest. While
// nothing changes, a heartbeat every 200 ms tells the PC the phone is still here. Hidden, the page
// stops sending and the PC hands the game back to the keyboard. Each control follows its own
// finger (pointer events, captured), so the slider and the button can be held at once. Haptics
// where the browser has them (iPhone's Safari doesn't), and a ring on every press either way.
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { ChevronLeft, ChevronRight, Smartphone, X } from "lucide-vue-next";
import TugMark from "../components/TugMark.vue";
import { TugboatError, type TugboatApi } from "./client";
import {
  BUSY_BACKOFF_MS,
  MAX_IN_FLIGHT,
  chooseLayout,
  controlsFor,
  PressLatch,
  encodeFor,
  gameFrom,
  nextSendDelay,
  padErrorAction,
  positionFromTouch,
  probe,
  samePad,
  tiltSupport,
  tiltToSteer,
  type PadGame,
  type PadInput,
} from "./pad";

const props = defineProps<{ api: TugboatApi; game?: PadGame }>();
const emit = defineEmits<{
  /** Back to the Tugboat page: by choice (`null`), or because the game or session ended (its code). */
  leave: [code: string | null];
}>();

/** The game on the PC: what the page was told, then what each reply says. */
const game = ref<PadGame>(props.game ?? "tugboat-run");
watch(
  () => props.game,
  (g) => {
    if (g) switchGame(g);
  },
);
const controls = computed(() => controlsFor(game.value));

/** Where the slider sits, -1 (full left) to 1 (full right). */
const steer = ref(0);
/** The big button (Boost or Drop) is held. */
const held = ref(false);
const status = ref<"connecting" | "ok" | "network">("connecting");
const paused = ref(false);
/** In a two-player game, which player this phone is (the PC says, in each reply). */
const player = ref<1 | 2 | null>(null);

const buzz = (ms: number) => {
  try {
    navigator.vibrate?.(ms);
  } catch {
    /* not supported */
  }
};

/** Keep a finger's events on its control even if it slides off (best effort: never lose the input). */
function capture(el: Element | null, id: number) {
  try {
    el?.setPointerCapture(id);
  } catch {
    /* the pointer already went */
  }
}

// --- Holding the phone: sideways like a gamepad, or upright if the player chose to ---
const landscapeQuery = window.matchMedia("(orientation: landscape)");
const landscape = ref(landscapeQuery.matches);
/** "Play in portrait anyway" (for this visit; nothing is stored). */
const portraitAnyway = ref(false);
const layout = computed(() => chooseLayout({ landscape: landscape.value, portraitAnyway: portraitAnyway.value }));
function readOrientation() {
  landscape.value = landscapeQuery.matches;
}

// --- The slider: steering, or the crane's nudge ---
const pad = ref<HTMLElement | null>(null);
let steerPointer: number | null = null;
const dragging = ref(false);
function steerAt(e: PointerEvent) {
  const el = pad.value;
  if (!el) return;
  const r = el.getBoundingClientRect();
  setSteer(positionFromTouch(e.clientX - r.left, r.width));
}
function onPadDown(e: PointerEvent) {
  if (steerPointer !== null) return;
  steerPointer = e.pointerId;
  dragging.value = true;
  capture(pad.value, e.pointerId);
  steerAt(e);
}
function onPadMove(e: PointerEvent) {
  if (e.pointerId === steerPointer) steerAt(e);
}
function onPadUp(e: PointerEvent) {
  if (e.pointerId !== steerPointer) return;
  steerPointer = null;
  dragging.value = false;
  // Tugboat Run: the knob stays where it was let go, and so does the boat. Harbor Stack: a nudge
  // lasts as long as the thumb is on it.
  if (controls.value.springBack) setSteer(0);
}
function setSteer(v: number) {
  if (v === steer.value) return;
  steer.value = v;
  kick();
}

// --- The big button: on the press itself, not the release ---
let actionPointer: number | null = null;
/** Bumped on each press, to start the ring again. */
const taps = ref(0);
function onActionDown(e: PointerEvent) {
  actionPointer = e.pointerId;
  capture(e.currentTarget as Element, e.pointerId);
  held.value = true;
  presses.press(performance.now());
  taps.value++;
  buzz(12);
  kick();
}
function onActionUp(e: PointerEvent) {
  if (e.pointerId !== actionPointer) return;
  actionPointer = null;
  held.value = false;
  presses.release(performance.now());
  kick();
}

/** The PC is playing another game now: its buttons, from a fresh start. */
function switchGame(g: PadGame) {
  if (g === game.value) return;
  game.value = g;
  steer.value = 0;
  held.value = false;
  presses.clear();
  actionPointer = steerPointer = null;
  dragging.value = false;
  if (tiltOn.value) tiltOff();
  kick();
}

// --- Tilt: only where the browser really gives the page the phone's orientation ---
// On Tugboat's plain-HTTP page it never does (Safari needs a secure connection and a permission
// prompt), so there's no switch there at all. Only Tugboat Run steers.
const tilt = tiltSupport({ secure: window.isSecureContext, hasOrientation: "DeviceOrientationEvent" in window });
const touchPhone = window.matchMedia("(pointer: coarse)").matches;
const canTilt = computed(() => tilt === "available" && touchPhone && game.value === "tugboat-run");
const tiltOn = ref(false);
function onOrientation(e: DeviceOrientationEvent) {
  if (steerPointer !== null) return; // a thumb on the slider wins
  const angle = screen.orientation?.angle ?? 0;
  setSteer(Math.round(tiltToSteer(e.beta, e.gamma, angle) * 100) / 100);
}
async function toggleTilt() {
  if (tiltOn.value) {
    tiltOff();
    return;
  }
  const asker = (window.DeviceOrientationEvent as unknown as { requestPermission?: () => Promise<string> }).requestPermission;
  try {
    // iOS asks the person first (inside this tap); elsewhere it's simply there.
    if (asker && (await asker()) !== "granted") return;
  } catch {
    return;
  }
  window.addEventListener("deviceorientation", onOrientation);
  tiltOn.value = true;
}
function tiltOff() {
  window.removeEventListener("deviceorientation", onOrientation);
  tiltOn.value = false;
}

// --- Sending ---
let timer: number | undefined;
let inFlight = 0;
let lastSentAt = Number.NEGATIVE_INFINITY;
/** The last input sent (or being sent): what "changed" compares against. */
let lastQueued: PadInput | null = null;
/** When the oldest change not yet sent happened, for the latency probe. */
let dirtySince: number | null = null;
/** The last round trip, for the latency probe. */
let lastRtt = 0;
let lastHits: number | null = null;
/** Replies can come back out of order with two in flight: only the newest one's news counts. */
let sentCount = 0;
let appliedReply = 0;
let stopped = false;
let backoffUntil = 0;
/** The button's presses and releases the PC hasn't confirmed. Only the newest state goes out and two
 * can be in flight, so a quick tap (down and up between two sends, or a release overtaking its
 * press) would otherwise never reach the PC, and two quick taps could arrive as one: each change
 * stays in every input until a reply to one carrying it comes back, and only then does the next go
 * (`PressLatch`), so two taps are two presses. One the PC hasn't heard in 500 ms is dropped. */
const presses = new PressLatch();

const current = () => encodeFor(game.value, steer.value, presses.down(performance.now()));

/** Something changed: send it now if the pacing allows, or at the next 16 ms mark. */
function kick() {
  dirtySince ??= performance.now();
  pump();
}

function pump() {
  window.clearTimeout(timer);
  timer = undefined;
  // A request finishing calls pump again; so does the page coming back on screen.
  if (stopped || inFlight >= MAX_IN_FLIGHT || document.visibilityState !== "visible") return;
  const now = performance.now();
  const changed = !samePad(lastQueued, current());
  const wait = Math.max(backoffUntil - now, nextSendDelay(now - lastSentAt, changed));
  if (wait <= 0) void send();
  else timer = window.setTimeout(pump, wait);
}

async function send() {
  inFlight++;
  const id = ++sentCount;
  const input = current();
  const started = performance.now();
  // Only an input carrying a new touch says how long that touch waited (heartbeats don't).
  const p = dirtySince === null ? undefined : probe(started - dirtySince, lastRtt);
  dirtySince = null;
  lastSentAt = started;
  lastQueued = input;
  const mark = presses.mark();
  // Ready for the next change straight away (up to two in flight).
  pump();
  try {
    const reply = await props.api.pad(input, p);
    lastRtt = performance.now() - started;
    status.value = "ok";
    // The PC has this change: the next one can follow (a change, so the next pump sends it at once).
    if (mark > 0) presses.heardUpTo(mark, performance.now());
    if (id > appliedReply) {
      appliedReply = id;
      paused.value = reply.paused;
      player.value = reply.player === 1 || reply.player === 2 ? reply.player : null;
      if (lastHits !== null && reply.hits > lastHits) buzz(120);
      lastHits = reply.hits;
      if (reply.game !== undefined) switchGame(gameFrom(reply.game));
    }
  } catch (e) {
    const code = e instanceof TugboatError ? e.code : "error";
    const action = padErrorAction(code);
    if (action === "leave") {
      stopped = true;
      emit("leave", code);
      return;
    }
    // Not delivered: send the state again (an unconfirmed press is still waiting in the latch).
    lastQueued = null;
    if (action === "slow") backoffUntil = performance.now() + BUSY_BACKOFF_MS;
    else {
      if (code === "network") {
        status.value = "network";
        // A press saved during a Wi-Fi blip mustn't fire when it comes back.
        presses.clear();
      }
      backoffUntil = performance.now() + 500;
    }
  } finally {
    inFlight--;
  }
  pump();
}

function onVisibility() {
  if (document.visibilityState === "visible") {
    pump();
    return;
  }
  // Hidden (locked, another app): let go of the button and stop. The PC notices the silence and
  // hands the game back to the keyboard. Tugboat Run's slider keeps its place for when you come
  // back; Harbor Stack's nudge lets go.
  window.clearTimeout(timer);
  steerPointer = actionPointer = null;
  dragging.value = false;
  held.value = false;
  presses.clear();
  if (controls.value.springBack) steer.value = 0;
}

/** Pinching would zoom the page out from under the thumbs (Safari ignores the viewport's no-zoom). */
const noGesture = (e: Event) => e.preventDefault();

function done() {
  stopped = true;
  window.clearTimeout(timer);
  emit("leave", null);
}

onMounted(() => {
  document.addEventListener("visibilitychange", onVisibility);
  document.addEventListener("gesturestart", noGesture);
  landscapeQuery.addEventListener("change", readOrientation);
  window.addEventListener("resize", readOrientation);
  pump();
});
onUnmounted(() => {
  stopped = true;
  window.clearTimeout(timer);
  document.removeEventListener("visibilitychange", onVisibility);
  document.removeEventListener("gesturestart", noGesture);
  landscapeQuery.removeEventListener("change", readOrientation);
  window.removeEventListener("resize", readOrientation);
  tiltOff();
});

const statusText = computed(() =>
  status.value === "network" ? "Can't reach PC" : status.value === "connecting" ? "Connecting…" : paused.value ? "Paused on your PC" : "Connected",
);
/** Player 1 is coral, Player 2 teal, like their boats (and stacks) on the PC. */
const teal = computed(() => player.value === 2);
/** Where the knob sits on the pad, matching `positionFromTouch` (6% margin each side). */
const knob = computed(() => `${6 + (steer.value + 1) * 44}%`);
const sideways = computed(() => layout.value === "landscape");
</script>

<template>
  <div
    class="fixed inset-0 flex touch-none flex-col overscroll-none bg-canvas pt-[max(0.5rem,env(safe-area-inset-top))] pr-[max(1rem,env(safe-area-inset-right))] pb-[max(0.75rem,env(safe-area-inset-bottom))] pl-[max(1rem,env(safe-area-inset-left))] select-none"
    style="-webkit-touch-callout: none; -webkit-user-select: none"
  >
    <header class="flex shrink-0 items-center gap-2.5">
      <TugMark :size="sideways ? 22 : 26" class="shrink-0 text-ink" />
      <h1 :class="['headline truncate leading-none', sideways ? 'text-[20px]' : 'text-[24px]']">{{ controls.title }}</h1>
      <!-- In a two-player game: which player this phone is, in its colour. -->
      <span
        v-if="player"
        :class="['shrink-0 rounded-full px-2.5 py-0.5 text-[13px] font-medium text-on-primary', teal ? 'bg-accent-teal' : 'bg-primary']"
      >
        Player {{ player }}
      </span>
      <span class="ml-auto inline-flex shrink-0 items-center gap-1.5 rounded-full bg-card px-3 py-1 text-[13px] font-medium text-body" role="status">
        <span
          :class="[
            'size-2 rounded-full',
            status === 'ok' && !paused ? 'bg-accent-teal' : status === 'network' ? 'bg-error' : 'bg-muted',
          ]"
        />
        {{ statusText }}
      </span>
      <button class="-mr-1 shrink-0 rounded-lg p-2 text-muted active:bg-card-strong" aria-label="Stop using as a controller" @click="done">
        <X :size="20" />
      </button>
    </header>

    <!-- Upright: ask for sideways (Safari can't lock it), with a way to play upright anyway. -->
    <div v-if="layout === 'rotate'" class="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 px-4 text-center" role="status">
      <span class="flex size-24 items-center justify-center rounded-3xl bg-card">
        <Smartphone :size="44" class="animate-turn text-ink" aria-hidden="true" />
      </span>
      <p class="headline text-[26px] leading-tight">Turn your phone sideways to play</p>
      <p class="max-w-[300px] text-[15px] text-muted">{{ controls.thumbs }}</p>
      <button class="mt-1 rounded-lg px-3 py-2 text-[15px] font-medium text-body underline underline-offset-4 active:bg-card" @click="portraitAnyway = true">
        Play in portrait anyway
      </button>
    </div>

    <div v-else :class="['mt-2 flex min-h-0 flex-1 gap-3', sideways ? 'flex-row' : 'flex-col']">
      <!-- Left thumb: the slider. -->
      <div
        ref="pad"
        class="relative flex min-h-0 flex-1 items-center overflow-hidden rounded-3xl bg-card"
        role="slider"
        :aria-label="controls.steerLabel"
        aria-valuemin="-100"
        aria-valuemax="100"
        :aria-valuenow="Math.round(steer * 100)"
        @pointerdown="onPadDown"
        @pointermove="onPadMove"
        @pointerup="onPadUp"
        @pointercancel="onPadUp"
      >
        <div class="absolute inset-y-6 left-1/2 w-px bg-hairline" />
        <span class="absolute left-3 flex flex-col items-center">
          <ChevronLeft :size="44" :class="steer < 0 ? (teal ? 'text-accent-teal' : 'text-primary') : 'text-muted'" />
          <span v-if="controls.ends" class="text-[13px] font-medium text-muted">{{ controls.ends[0] }}</span>
        </span>
        <span class="absolute right-3 flex flex-col items-center">
          <ChevronRight :size="44" :class="steer > 0 ? (teal ? 'text-accent-teal' : 'text-primary') : 'text-muted'" />
          <span v-if="controls.ends" class="text-[13px] font-medium text-muted">{{ controls.ends[1] }}</span>
        </span>
        <span
          :class="[
            'absolute top-1/2 size-16 -translate-x-1/2 -translate-y-1/2 rounded-full border-4 border-canvas bg-ink/80 shadow-md',
            controls.springBack && !dragging ? 'transition-[left] duration-150 motion-reduce:transition-none' : '',
          ]"
          :style="{ left: knob }"
          aria-hidden="true"
        />
        <p class="pointer-events-none absolute inset-x-0 bottom-3 text-center text-[13px] text-muted">{{ tiltOn ? "Tilt to steer, or slide" : controls.steerHint }}</p>
        <button
          v-if="canTilt"
          class="absolute top-2 right-2 rounded-full bg-canvas px-3 py-1 text-[13px] font-medium text-body"
          :aria-pressed="tiltOn"
          @pointerdown.stop
          @click="toggleTilt"
        >
          {{ tiltOn ? "Tilt on" : "Tilt to steer" }}
        </button>
      </div>

      <!-- Right thumb: Boost or Drop. -->
      <button
        :class="[
          'relative flex shrink-0 flex-col items-center justify-center overflow-hidden rounded-3xl text-on-primary transition-[background-color,transform] duration-75 motion-reduce:transition-none',
          sideways ? 'w-[40%]' : 'h-36',
          held ? 'scale-[0.97]' : '',
          teal ? (held ? 'bg-accent-teal brightness-90' : 'bg-accent-teal') : held ? 'bg-primary-active' : 'bg-primary',
        ]"
        :aria-label="controls.action"
        @pointerdown="onActionDown"
        @pointerup="onActionUp"
        @pointercancel="onActionUp"
        @contextmenu.prevent
      >
        <span v-if="taps > 0" :key="taps" class="animate-tap pointer-events-none absolute size-48 rounded-full bg-canvas/40" aria-hidden="true" />
        <span class="relative text-[30px] font-medium">{{ controls.action }}</span>
        <span class="relative text-[14px] opacity-90">{{ controls.actionHint }}</span>
      </button>
    </div>
  </div>
</template>
