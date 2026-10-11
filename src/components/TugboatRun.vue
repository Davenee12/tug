<script setup lang="ts">
// Tugboat Run: a small endless runner over the whole window, from the sidebar's Play button or
// Ctrl+K "game". Keyboard, mouse, or the phone as a controller (through Tugboat).
//
// Idle by design: requestAnimationFrame runs only while a run is being played (open, on screen,
// not paused) and draws at most once per display frame. Paused, ready or over, the canvas is drawn
// once and left alone. Losing focus, hiding tug, or a call or confirmation card coming up pauses.
// Closing removes every listener, observer and frame request, and closes the phone's controller.
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { Gamepad2, LifeBuoy, LoaderCircle, Pause, Play, RotateCcw, Smartphone, X } from "lucide-vue-next";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useGameStore } from "../stores/game";
import { useTugStore } from "../stores/tug";
import { useTugboatStore } from "../stores/tugboat";
import { useDevToolsStore } from "../stores/devtools";
import { useFocusTrap } from "../lib/focusTrap";
import { api, on } from "../lib/ipc";
import { capitalised } from "../lib/tugboat";
import { LIVES, advance, layout, mergeInput, newRun, score, toFieldX, type Layout, type Run } from "../lib/tugboatRun";
import { draw, type Palette } from "./tugboatRunDraw";

type Phase = "ready" | "playing" | "paused" | "over";

const game = useGameStore();
const tug = useTugStore();
const tugboat = useTugboatStore();
const devtools = useDevToolsStore();

const root = ref<HTMLElement | null>(null);
const stage = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const swatches = ref<HTMLElement | null>(null);

const phase = ref<Phase>("ready");
const newBest = ref(false);
const hud = reactive({ score: 0, lives: LIVES, hopReady: true });

const seed = () => Math.floor(Math.random() * 2 ** 31);
// The run is a plain object mutated every frame; only the HUD numbers above are reactive.
let run: Run = newRun(seed());
let overAt = 0;
let shake = 0;

// --- Controls ---
const keys = { left: false, right: false, boost: false };
let pointerX: number | null = null;
let pointerBoost = false;
let pointerDown: { at: number; x: number; y: number } | null = null;
/** The phone, as the last `game-pad` event said (read by the loop; not reactive). */
const pad = { connected: false, steer: 0, boost: false };
/** The phone's slider moved more recently than the keys or mouse were used: the boat follows it. */
let padLeads = false;
const padConnected = ref(false);

// --- Phone controller (through Tugboat) ---
/** "Use your phone as a controller" is showing. */
const sheet = ref(false);
/** The controller channel was opened (closed again when the game closes). */
const controllerOn = ref(false);
const controllerError = ref<string | null>(null);
const ts = computed(() => tugboat.status);
const phoneName = computed(() => ts.value.phone ?? "phone");

/** Another dialog (a call, a text an AI tool wants to send, a pairing PIN) is over the game. */
const blocked = computed(() => tug.ringing !== null || devtools.confirm !== null || tug.pairingRequest !== null);

useFocusTrap(root, onEscape);

function onEscape() {
  if (sheet.value) sheet.value = false;
  else if (phase.value === "playing") pause();
  else close();
}

// --- Phases ---
function start() {
  run = newRun(seed());
  newBest.value = false;
  shake = 0;
  syncHud();
  phase.value = "playing";
}

function pause() {
  if (phase.value === "playing") phase.value = "paused";
}

function resume() {
  if (phase.value === "paused" && !blocked.value) {
    sheet.value = false;
    phase.value = "playing";
  }
}

function togglePause() {
  if (phase.value === "playing") pause();
  else if (phase.value === "paused") resume();
}

/** Space / Boost: start, resume or play again (not the instant a run ends, so a held key doesn't). */
function primary() {
  if (phase.value === "ready") start();
  else if (phase.value === "paused") resume();
  else if (phase.value === "over" && performance.now() - overAt > 700) start();
}

function close() {
  game.close();
}

// --- The loop ---
let raf = 0;
let lastTs = 0;

function frame(now: number) {
  raf = 0;
  const dt = lastTs ? (now - lastTs) / 1000 : 1 / 60;
  lastTs = now;
  run.events = [];
  // A tap (mouse or phone) that came and went since the last frame still counts as a press.
  const tapped = pointerBoost || padPress;
  advance(run, mergeInput({ keyLeft: keys.left, keyRight: keys.right, keyBoost: keys.boost, pointerX, pointerBoost: tapped, pad, padLeads }, run.x), dt);
  pointerBoost = false;
  padPress = false;
  shake = Math.max(0, shake - dt);
  if (run.events.includes("hit")) {
    if (!reducedMotion.value) shake = 0.25;
    sendFeedback();
  }
  render();
  syncHud();
  if (run.over) {
    overAt = performance.now();
    newBest.value = game.finish(score(run));
    phase.value = "over";
    return;
  }
  if (phase.value === "playing") raf = requestAnimationFrame(frame);
}

function startLoop() {
  if (raf) return;
  lastTs = 0;
  raf = requestAnimationFrame(frame);
}

function stopLoop() {
  if (raf) cancelAnimationFrame(raf);
  raf = 0;
}

function syncHud() {
  const s = score(run);
  if (hud.score !== s) hud.score = s;
  if (hud.lives !== run.lives) hud.lives = run.lives;
  const ready = run.hop === 0 && run.cooldown === 0;
  if (hud.hopReady !== ready) hud.hopReady = ready;
}

watch(phase, (p) => {
  if (p === "playing") startLoop();
  else {
    stopLoop();
    render();
  }
  sendFeedback();
});

watch(blocked, (b) => {
  if (b) pause();
});

// --- Drawing ---
let ctx: CanvasRenderingContext2D | null = null;
let size = { width: 1, height: 1, dpr: 1 };
let fit: Layout = layout(1, 1);
let palette: Palette | null = null;
const reducedMotion = ref(false);

function resize() {
  const el = stage.value;
  const c = canvas.value;
  if (!el || !c) return;
  const r = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  size = { width: Math.max(1, r.width), height: Math.max(1, r.height), dpr };
  c.width = Math.max(1, Math.round(size.width * dpr));
  c.height = Math.max(1, Math.round(size.height * dpr));
  fit = layout(size.width, size.height);
}

function render() {
  if (!ctx || !palette) return;
  draw(ctx, run, { ...size, layout: fit, reducedMotion: reducedMotion.value, shake }, palette);
}

/** tug's colours, read from theme classes on hidden swatches (no colour is written in code). */
function readPalette(): Palette | null {
  const el = swatches.value;
  if (!el) return null;
  const pick = (name: string) => {
    const s = el.querySelector<HTMLElement>(`[data-swatch="${name}"]`);
    return s ? getComputedStyle(s).backgroundColor : "";
  };
  return {
    waterBase: pick("waterBase"),
    water: pick("water"),
    waterDeep: pick("waterDeep"),
    quay: pick("quay"),
    quayEdge: pick("quayEdge"),
    hull: pick("hull"),
    deck: pick("deck"),
    cabin: pick("cabin"),
    ink: pick("ink"),
    rope: pick("rope"),
    coin: pick("coin"),
    log: pick("log"),
    logEnd: pick("logEnd"),
    boat: pick("boat"),
    boatCabin: pick("boatCabin"),
    foam: pick("foam"),
  };
}

// --- Input handlers ---
const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && t.closest("input, textarea, [contenteditable]") !== null;

function onKey(e: KeyboardEvent, down: boolean) {
  if (e.ctrlKey || e.metaKey || e.altKey || blocked.value || isTyping(e.target)) return;
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (k === "ArrowLeft" || k === "a" || k === "ArrowRight" || k === "d") {
    if (k === "ArrowLeft" || k === "a") keys.left = down;
    else keys.right = down;
    // The keyboard takes the helm back from the phone's slider until the slider moves again.
    if (down) padLeads = false;
  } else if (k === " " || k === "ArrowUp" || k === "w") {
    keys.boost = down;
    if (down && !e.repeat && phase.value !== "playing") primary();
  } else if (k === "p") {
    if (down && !e.repeat) togglePause();
  } else if ((k === "Enter" || k === "r") && phase.value === "over") {
    // Enter on a focused button is the button's.
    if (e.target instanceof HTMLButtonElement) return;
    if (down && !e.repeat) primary();
  } else {
    return;
  }
  // Space and arrows belong to the game, not to a focused button or the page.
  e.preventDefault();
}
const onKeyDown = (e: KeyboardEvent) => onKey(e, true);
const onKeyUp = (e: KeyboardEvent) => onKey(e, false);

function onPointerDown(e: PointerEvent) {
  if (e.button !== 0 || phase.value !== "playing") return;
  try {
    canvas.value?.setPointerCapture(e.pointerId);
  } catch {
    /* the pointer already went */
  }
  pointerX = toFieldX(e.offsetX, fit);
  padLeads = false;
  pointerDown = { at: performance.now(), x: e.clientX, y: e.clientY };
}
function onPointerMove(e: PointerEvent) {
  if (pointerX !== null) pointerX = toFieldX(e.offsetX, fit);
}
function onPointerUp(e: PointerEvent) {
  // A quick tap (not a drag) hops.
  if (pointerDown && performance.now() - pointerDown.at < 220 && Math.hypot(e.clientX - pointerDown.x, e.clientY - pointerDown.y) < 8) {
    pointerBoost = true;
  }
  pointerX = null;
  pointerDown = null;
}

function onBlur() {
  pause();
  keys.left = keys.right = keys.boost = false;
  pointerX = null;
}
function onVisibility() {
  if (document.visibilityState !== "visible") onBlur();
}

// --- Phone controller ---
let padBoostWas = false;
/** A Boost press from the phone since the last frame (its release may arrive before the frame). */
let padPress = false;
function onPad(e: { connected: boolean; steer: number; boost: boolean }) {
  // The slider moved: the boat follows it from the next frame.
  if (e.connected && pad.connected && e.steer !== pad.steer) padLeads = true;
  if (!e.connected) padLeads = false;
  Object.assign(pad, e);
  padConnected.value = e.connected;
  if (e.connected && e.boost && !padBoostWas && phase.value === "playing") padPress = true;
  // From the phone alone: Boost starts, resumes or plays again (on a fresh press).
  if (e.connected && e.boost && !padBoostWas && phase.value !== "playing" && !blocked.value) {
    sheet.value = false;
    primary();
  }
  padBoostWas = e.connected && e.boost;
}

async function useController() {
  pause();
  sheet.value = true;
  if (controllerOn.value) return;
  controllerOn.value = true;
  controllerError.value = null;
  try {
    await api.gamePadOpen();
    sendFeedback();
  } catch (e) {
    controllerOn.value = false;
    controllerError.value = typeof e === "string" ? e : "Couldn't start Tugboat.";
  }
}

async function controllerOff() {
  controllerOn.value = false;
  sheet.value = false;
  onPad({ connected: false, steer: 0, boost: false });
  await api.gamePadClose().catch(() => undefined);
}

/** Tugboat turned off (its panel closed, tug hidden, or 10 minutes unused): so did the controller. */
watch(
  () => ts.value.phase,
  (p) => {
    if (p === "off" && controllerOn.value) {
      controllerOn.value = false;
      onPad({ connected: false, steer: 0, boost: false });
    }
  },
);

let lastFeedback = "";
function sendFeedback() {
  if (!controllerOn.value) return;
  const paused = phase.value !== "playing";
  const key = `${paused}:${run.hits}`;
  if (key === lastFeedback) return;
  lastFeedback = key;
  void api.gamePadFeedback(paused, run.hits).catch(() => undefined);
}

const controllerLine = computed(() => {
  if (controllerError.value) return controllerError.value;
  if (padConnected.value) return `Your ${phoneName.value} is the controller. Steer with the pad, Boost to hop.`;
  const p = ts.value.phase;
  if (p === "off") return "Starting Tugboat…";
  if (p === "noNetwork") return "Connect this PC to your home or office Wi-Fi to use your phone.";
  if (p === "connected" && ts.value.phoneActive) return `Your ${phoneName.value} is on Tugboat. The controller opens on it in a moment.`;
  if (p === "connected") return `Open Tugboat on your ${phoneName.value} again: scan the code if the page is closed.`;
  return "Scan with your phone's camera. Tugboat opens on it as a controller.";
});

// --- Lifetime ---
let ro: ResizeObserver | null = null;
let dprQuery: MediaQueryList | null = null;
let motionQuery: MediaQueryList | null = null;
let unlistenPad: UnlistenFn | null = null;
let disposed = false;

function onDprChange() {
  watchDpr();
  resize();
  render();
}
function watchDpr() {
  dprQuery?.removeEventListener("change", onDprChange);
  dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  dprQuery.addEventListener("change", onDprChange);
}
const onMotion = (e: MediaQueryListEvent) => (reducedMotion.value = e.matches);

onMounted(() => {
  ctx = canvas.value?.getContext("2d") ?? null;
  palette = readPalette();
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion.value = motionQuery.matches;
  motionQuery.addEventListener("change", onMotion);
  resize();
  render();
  ro = new ResizeObserver(() => {
    resize();
    if (!raf) render();
  });
  if (stage.value) ro.observe(stage.value);
  watchDpr();
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  window.addEventListener("blur", onBlur);
  document.addEventListener("visibilitychange", onVisibility);
  void on("game-pad", onPad).then((off) => {
    if (disposed) off();
    else unlistenPad = off;
  });
  stage.value?.focus();
});

onUnmounted(() => {
  disposed = true;
  stopLoop();
  ro?.disconnect();
  dprQuery?.removeEventListener("change", onDprChange);
  motionQuery?.removeEventListener("change", onMotion);
  window.removeEventListener("keydown", onKeyDown);
  window.removeEventListener("keyup", onKeyUp);
  window.removeEventListener("blur", onBlur);
  document.removeEventListener("visibilitychange", onVisibility);
  unlistenPad?.();
  if (controllerOn.value) void api.gamePadClose().catch(() => undefined);
});
</script>

<template>
  <div ref="root" role="dialog" aria-modal="true" aria-labelledby="run-title" class="fixed inset-0 z-50 flex flex-col bg-surface-dark text-on-dark">
    <header class="flex items-center gap-3 border-b border-surface-dark-elevated px-5 py-3">
      <Gamepad2 :size="20" class="shrink-0 text-primary" />
      <h2 id="run-title" class="font-display text-[24px] leading-none text-on-dark" style="letter-spacing: -0.02em">Tugboat Run</h2>

      <div class="ml-4 flex items-center gap-4 font-mono text-[14px]" aria-live="off">
        <span><span class="text-on-dark-soft">Score</span> {{ hud.score }}</span>
        <span><span class="text-on-dark-soft">Best</span> {{ Math.max(game.best, hud.score) }}</span>
        <span class="flex items-center gap-1" :aria-label="`${hud.lives} of ${LIVES} lives`" role="img">
          <LifeBuoy v-for="i in LIVES" :key="i" :size="15" :class="i <= hud.lives ? 'text-primary' : 'text-on-dark-soft/30'" />
        </span>
        <span :class="['rounded-full px-2 py-0.5 text-[12px]', hud.hopReady ? 'bg-surface-dark-elevated text-on-dark' : 'text-on-dark-soft/50']">Hop</span>
      </div>

      <div class="ml-auto flex items-center gap-2">
        <span v-if="controllerOn" class="pill bg-surface-dark-elevated px-2.5 py-0.5 text-[12px] text-on-dark" role="status">
          <span :class="['size-1.5 rounded-full', padConnected ? 'bg-accent-teal' : 'bg-on-dark-soft/40']" />
          {{ padConnected ? "Phone connected" : "Phone not connected" }}
        </span>
        <button class="btn-on-dark btn-sm" @click="useController">
          <Smartphone :size="14" />
          Use your phone as a controller
        </button>
        <button
          v-if="phase === 'playing' || phase === 'paused'"
          class="btn-on-dark btn-sm w-8 px-0"
          :aria-label="phase === 'playing' ? 'Pause' : 'Resume'"
          :title="phase === 'playing' ? 'Pause (P)' : 'Resume (P)'"
          @click="togglePause"
        >
          <Pause v-if="phase === 'playing'" :size="14" />
          <Play v-else :size="14" />
        </button>
        <button class="btn-on-dark btn-sm w-8 px-0" aria-label="Close Tugboat Run" @click="close">
          <X :size="15" />
        </button>
      </div>
    </header>

    <div class="relative flex min-h-0 flex-1">
      <div ref="stage" tabindex="-1" class="relative min-w-0 flex-1 outline-none">
        <canvas
          ref="canvas"
          class="absolute inset-0 block size-full touch-none"
          aria-label="Tugboat Run playing field"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointercancel="onPointerUp"
        />

        <!-- Ready, paused, over: one card over the still field. -->
        <div v-if="phase !== 'playing'" class="absolute inset-0 flex items-center justify-center p-6">
          <div class="w-full max-w-[380px] rounded-2xl bg-surface-dark/90 px-7 py-6 text-center shadow-xl ring-1 ring-surface-dark-elevated">
            <template v-if="phase === 'ready'">
              <p class="font-display text-[30px] leading-tight">Ready to sail?</p>
              <p class="mt-2 text-[14px] text-on-dark-soft">
                Steer with ← → or A and D. Space hops over buoys and logs, but not other boats. Coins are points; a life ring brings back
                a life.
              </p>
              <p class="mt-1.5 text-[13px] text-on-dark-soft/80">Or drag with the mouse to steer, and click to hop.</p>
              <button class="btn-primary mt-5" @click="start">
                <Play :size="16" />
                Start
              </button>
            </template>
            <template v-else-if="phase === 'paused'">
              <p class="font-display text-[30px] leading-tight">Paused</p>
              <p class="mt-2 text-[14px] text-on-dark-soft">Space or P to carry on. Esc again to close.</p>
              <button class="btn-primary mt-5" :disabled="blocked" @click="resume">
                <Play :size="16" />
                Resume
              </button>
            </template>
            <template v-else>
              <p class="font-display text-[30px] leading-tight">{{ newBest ? "New best!" : "Run over" }}</p>
              <p class="mt-2 font-mono text-[28px] text-on-dark">{{ hud.score }}</p>
              <p class="text-[13px] text-on-dark-soft">{{ newBest ? "Your best score on this PC." : `Best on this PC: ${game.best}` }}</p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="start">
                  <RotateCcw :size="16" />
                  Play again
                </button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
              <p class="mt-3 text-[12px] text-on-dark-soft/80">Enter or Space plays again.</p>
            </template>
          </div>
        </div>
      </div>

      <!-- The phone as a controller: Tugboat's code and where things stand. -->
      <aside
        v-if="sheet"
        class="flex w-[320px] shrink-0 flex-col border-l border-surface-dark-elevated bg-surface-dark-soft px-6 py-5"
        aria-labelledby="run-controller-title"
      >
        <div class="flex items-center gap-2">
          <h3 id="run-controller-title" class="font-display text-[22px] leading-tight">Phone controller</h3>
          <button class="ml-auto rounded-md p-1.5 text-on-dark-soft active:bg-surface-dark-elevated" aria-label="Hide phone controller" @click="sheet = false">
            <X :size="16" />
          </button>
        </div>
        <p class="mt-3 flex items-start gap-2 text-[14px] text-on-dark" role="status">
          <LoaderCircle v-if="controllerOn && ts.phase === 'off' && !controllerError" :size="16" class="mt-0.5 shrink-0 animate-spin text-on-dark-soft" />
          <span v-else :class="['mt-[7px] size-2 shrink-0 rounded-full', padConnected ? 'bg-accent-teal' : 'bg-on-dark-soft/40']" />
          {{ controllerLine }}
        </p>

        <div v-if="ts.qr && !padConnected" class="mx-auto mt-5 rounded-xl bg-canvas p-3">
          <svg :viewBox="`-2 -2 ${ts.qr.size + 4} ${ts.qr.size + 4}`" class="block size-[200px]" shape-rendering="crispEdges" role="img" aria-label="QR code to use your phone as a controller">
            <path :d="ts.qr.path" class="fill-ink" />
          </svg>
        </div>
        <p v-if="ts.qr && !padConnected" class="mt-3 text-center text-[12px] text-on-dark-soft">
          This is Tugboat's code: the same one for photos and files.
        </p>
        <div v-else-if="padConnected" class="mt-5 flex flex-col items-center gap-2 py-4 text-center">
          <span class="flex size-14 items-center justify-center rounded-2xl bg-surface-dark-elevated">
            <Smartphone :size="26" class="text-accent-teal" />
          </span>
          <p class="font-display text-[20px]">{{ capitalised(phoneName) }} connected</p>
        </div>

        <div class="mt-auto space-y-2 pt-5">
          <button v-if="phase === 'paused'" class="btn-primary w-full" :disabled="blocked" @click="resume">
            <Play :size="16" />
            Resume
          </button>
          <button v-if="controllerOn" class="btn-on-dark btn-sm w-full" @click="controllerOff">Stop using the phone</button>
          <p class="text-center text-[12px] text-on-dark-soft/80">Encrypted, over your Wi-Fi. The keyboard always works too.</p>
        </div>
      </aside>
    </div>

    <!-- tug's colours for the canvas, read from these (theme tokens only, no colour in code). -->
    <div ref="swatches" class="hidden" aria-hidden="true">
      <span data-swatch="waterBase" class="bg-canvas" />
      <span data-swatch="water" class="bg-accent-teal/20" />
      <span data-swatch="waterDeep" class="bg-accent-teal/60" />
      <span data-swatch="quay" class="bg-surface-cream-strong" />
      <span data-swatch="quayEdge" class="bg-muted-soft" />
      <span data-swatch="hull" class="bg-primary" />
      <span data-swatch="deck" class="bg-canvas" />
      <span data-swatch="cabin" class="bg-surface-dark" />
      <span data-swatch="ink" class="bg-ink" />
      <span data-swatch="rope" class="bg-body" />
      <span data-swatch="coin" class="bg-accent-amber" />
      <span data-swatch="log" class="bg-muted" />
      <span data-swatch="logEnd" class="bg-muted-soft" />
      <span data-swatch="boat" class="bg-surface-dark-elevated" />
      <span data-swatch="boatCabin" class="bg-on-dark-soft" />
      <span data-swatch="foam" class="bg-canvas" />
    </div>
  </div>
</template>
