<script setup lang="ts">
// The phone as Tugboat Run's controller, while tug's game on the PC asks for one: a slider pad
// (the boat goes where the knob is, and the knob stays where you let go) and a Boost button.
//
// Inputs go out the moment something changes: a touch, a slide, Boost pressed or let go. Changes
// are coalesced to at most one every 16 ms (~60 a second) and up to two requests are in flight,
// so a slow reply never holds up the next input; the PC keeps only the newest. While nothing
// changes, a heartbeat every 200 ms tells the PC the phone is still here. Hidden, the page stops
// sending and the PC hands the boat back to the keyboard. Haptics where the browser has them
// (iPhone's Safari doesn't).
import { computed, onMounted, onUnmounted, ref } from "vue";
import { ChevronLeft, ChevronRight, X } from "lucide-vue-next";
import TugMark from "../components/TugMark.vue";
import { TugboatError, type TugboatApi } from "./client";
import {
  BUSY_BACKOFF_MS,
  MAX_IN_FLIGHT,
  encodePad,
  nextSendDelay,
  padErrorAction,
  positionFromTouch,
  probe,
  samePad,
  tiltSupport,
  type PadInput,
} from "./pad";

const props = defineProps<{ api: TugboatApi }>();
const emit = defineEmits<{
  /** Back to the Tugboat page: by choice (`null`), or because the game or session ended (its code). */
  leave: [code: string | null];
}>();

/** Where the slider sits, -1 (full left) to 1 (full right). */
const steer = ref(0);
const boost = ref(false);
const status = ref<"connecting" | "ok" | "network">("connecting");
const paused = ref(false);
const tilt = tiltSupport({ secure: window.isSecureContext, hasOrientation: "DeviceOrientationEvent" in window });

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

// --- Slider pad ---
const pad = ref<HTMLElement | null>(null);
let steerPointer: number | null = null;
function steerAt(e: PointerEvent) {
  const el = pad.value;
  if (!el) return;
  const r = el.getBoundingClientRect();
  setSteer(positionFromTouch(e.clientX - r.left, r.width));
}
function onPadDown(e: PointerEvent) {
  if (steerPointer !== null) return;
  steerPointer = e.pointerId;
  capture(pad.value, e.pointerId);
  steerAt(e);
}
function onPadMove(e: PointerEvent) {
  if (e.pointerId === steerPointer) steerAt(e);
}
function onPadUp(e: PointerEvent) {
  // The knob stays where it was let go, and so does the boat.
  if (e.pointerId === steerPointer) steerPointer = null;
}
function setSteer(v: number) {
  if (v === steer.value) return;
  steer.value = v;
  kick();
}

// --- Boost: on the press itself, not the release ---
let boostPointer: number | null = null;
function onBoostDown(e: PointerEvent) {
  boostPointer = e.pointerId;
  capture(e.currentTarget as Element, e.pointerId);
  boost.value = true;
  pressPending = true;
  buzz(12);
  kick();
}
function onBoostUp(e: PointerEvent) {
  if (e.pointerId !== boostPointer) return;
  boostPointer = null;
  boost.value = false;
  kick();
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
/** A Boost press not yet sent. Only the newest state goes out, so a quick tap (down and up between
 * two sends) would otherwise never reach the PC: the press is held until one send carries it, and
 * the release follows in the next. */
let pressPending = false;

const current = () => encodePad(steer.value, boost.value || pressPending);

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
  if (input.boost) pressPending = false;
  // Ready for the next change straight away (up to two in flight).
  pump();
  try {
    const reply = await props.api.pad(input, p);
    lastRtt = performance.now() - started;
    status.value = "ok";
    if (id > appliedReply) {
      appliedReply = id;
      paused.value = reply.paused;
      if (lastHits !== null && reply.hits > lastHits) buzz(120);
      lastHits = reply.hits;
    }
  } catch (e) {
    const code = e instanceof TugboatError ? e.code : "error";
    const action = padErrorAction(code);
    if (action === "leave") {
      stopped = true;
      emit("leave", code);
      return;
    }
    // Not delivered: send the state again, and a lost tap again.
    lastQueued = null;
    if (input.boost && !boost.value) pressPending = true;
    if (action === "slow") backoffUntil = performance.now() + BUSY_BACKOFF_MS;
    else {
      if (code === "network") status.value = "network";
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
  // Hidden (locked, another app): let go of Boost and stop. The PC notices the silence and hands
  // the boat back to the keyboard. The slider keeps its place for when you come back.
  window.clearTimeout(timer);
  steerPointer = boostPointer = null;
  boost.value = false;
  pressPending = false;
}

function done() {
  stopped = true;
  window.clearTimeout(timer);
  emit("leave", null);
}

onMounted(() => {
  document.addEventListener("visibilitychange", onVisibility);
  pump();
});
onUnmounted(() => {
  stopped = true;
  window.clearTimeout(timer);
  document.removeEventListener("visibilitychange", onVisibility);
});

const statusText = computed(() =>
  status.value === "network" ? "Can't reach PC" : status.value === "connecting" ? "Connecting…" : paused.value ? "Paused on your PC" : "Connected",
);
/** Where the knob sits on the pad, matching `positionFromTouch` (6% margin each side). */
const knob = computed(() => `${6 + (steer.value + 1) * 44}%`);</script>

<template>
  <div
    class="fixed inset-0 flex touch-none flex-col bg-canvas pt-[max(0.75rem,env(safe-area-inset-top))] pr-[max(1rem,env(safe-area-inset-right))] pb-[max(1rem,env(safe-area-inset-bottom))] pl-[max(1rem,env(safe-area-inset-left))] select-none"
  >
    <header class="flex items-center gap-2.5">
      <TugMark :size="26" class="text-ink" />
      <h1 class="headline text-[24px] leading-none">Tugboat Run</h1>
      <span class="ml-auto inline-flex items-center gap-1.5 rounded-full bg-card px-3 py-1 text-[13px] font-medium text-body" role="status">
        <span
          :class="[
            'size-2 rounded-full',
            status === 'ok' && !paused ? 'bg-accent-teal' : status === 'network' ? 'bg-error' : 'bg-muted',
          ]"
        />
        {{ statusText }}
      </span>
      <button class="-mr-1 rounded-lg p-2 text-muted active:bg-card-strong" aria-label="Stop using as a controller" @click="done">
        <X :size="20" />
      </button>
    </header>

    <div class="mt-3 flex min-h-0 flex-1 flex-col gap-3 landscape:flex-row">
      <!-- Steering: touch and slide; further from the middle turns harder. -->
      <div
        ref="pad"
        class="relative flex min-h-0 flex-1 items-center overflow-hidden rounded-3xl bg-card"
        role="slider"
        aria-label="Steer"
        aria-valuemin="-100"
        aria-valuemax="100"
        :aria-valuenow="Math.round(steer * 100)"
        @pointerdown="onPadDown"
        @pointermove="onPadMove"
        @pointerup="onPadUp"
        @pointercancel="onPadUp"
      >
        <div class="absolute inset-y-6 left-1/2 w-px bg-hairline" />
        <ChevronLeft :size="44" :class="['absolute left-4', steer < 0 ? 'text-primary' : 'text-muted']" />
        <ChevronRight :size="44" :class="['absolute right-4', steer > 0 ? 'text-primary' : 'text-muted']" />
        <span
          class="absolute top-1/2 size-16 -translate-x-1/2 -translate-y-1/2 rounded-full border-4 border-canvas bg-ink/80 shadow-md"
          :style="{ left: knob }"
          aria-hidden="true"
        />
        <p class="pointer-events-none absolute inset-x-0 bottom-3 text-center text-[13px] text-muted">Slide: the boat follows</p>
      </div>

      <button
        :class="[
          'flex shrink-0 flex-col items-center justify-center rounded-3xl text-on-primary transition-colors h-36 landscape:h-auto landscape:w-[38%]',
          boost ? 'bg-primary-active' : 'bg-primary',
        ]"
        aria-label="Boost"
        @pointerdown="onBoostDown"
        @pointerup="onBoostUp"
        @pointercancel="onBoostUp"
        @contextmenu.prevent
      >
        <span class="text-[28px] font-medium">Boost</span>
        <span class="text-[14px] opacity-90">Hop over buoys and logs</span>
      </button>
    </div>

    <p v-if="tilt === 'needs-secure'" class="mt-3 text-center text-[13px] text-muted">Tilt needs a secure connection; use touch.</p>
  </div>
</template>
