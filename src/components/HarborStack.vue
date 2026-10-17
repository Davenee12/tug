<script setup lang="ts">
// Harbor Stack: a stacking game over the whole window, from the games picker (the sidebar's Play
// button, Ctrl+K "game", "stack" or "harbor stack", or the taskbar's Play a game). A crane swings a
// container over a boat; drop it on the stack, and whatever overhangs is sliced off. Three ways to
// play: today's stack (the same for everyone today, in the player's real weather), Free play, and a
// two-player Stack-off (side by side on the same seed; the taller stack wins). Keyboard, mouse, or
// phones as controllers (through Tugboat), like Tugboat Run, whose plumbing it shares.
//
// Idle by design: requestAnimationFrame runs only while a game is being played (open, on screen,
// not paused) and draws at most once per display frame. Paused, ready or over, the canvas is drawn
// once and left alone. Losing focus, hiding tug, or a call or confirmation card coming up pauses.
// Closing removes every listener, observer and frame request, and closes the phones' controller.
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  ArrowLeftRight,
  CalendarDays,
  CloudMoon,
  CloudRain,
  CloudSnow,
  Container,
  Flame,
  Package,
  Pause,
  Play,
  RotateCcw,
  Smartphone,
  Sparkles,
  Sun,
  Trophy,
  Users,
  Waves,
  Wind,
  X,
} from "lucide-vue-next";
import { useGameStore } from "../stores/game";
import { useTugStore } from "../stores/tug";
import { useDevToolsStore } from "../stores/devtools";
import { useWeatherStore } from "../stores/weather";
import { useFocusTrap } from "../lib/focusTrap";
import { useGameController } from "../lib/useGameController";
import { FIELD_W, laneLayouts, layout, toFieldX, type Layout } from "../lib/tugboatRun";
import { CALM_SEAS, conditionsFrom, conditionsLine, currentStreak, dailySeed, handlingFor, localDate, sailedToday, type Conditions } from "../lib/tugboatDaily";
import { droppedPlayer, playerFor, seatFor, winner, type Player, type Seat } from "../lib/tugboatRace";
import {
  CALM_STACK,
  LIVES,
  advance,
  milestoneLine,
  newStack,
  nudgeFrom,
  settled,
  stackOffKey,
  stackOutlook,
  stackScore,
  stackWeather,
  type Stack,
} from "../lib/harborStack";
import type { GamePad } from "../types/protocol";
import { drawStack, type StackPalette } from "./harborStackDraw";
import type { Sky } from "./tugboatRunDraw";
import { readSwatches } from "./gamePalette";
import PhoneControllerSheet from "./PhoneControllerSheet.vue";

type Phase = "ready" | "playing" | "paused" | "over";
type Mode = "daily" | "free" | "duo";

const game = useGameStore();
const tug = useTugStore();
const devtools = useDevToolsStore();
const weather = useWeatherStore();
const scores = computed(() => game.scores["harbor-stack"]);

const root = ref<HTMLElement | null>(null);
const stage = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const swatches = ref<HTMLElement | null>(null);

const phase = ref<Phase>("ready");
const mode = ref<Mode>("daily");
const duo = computed(() => mode.value === "duo");
const newBest = ref(false);
interface LaneHud {
  score: number;
  lives: number;
  combo: number;
  over: boolean;
}
const freshHud = (): LaneHud => ({ score: 0, lives: LIVES, combo: 0, over: false });
const hud = reactive<{ lanes: LaneHud[] }>({ lanes: [freshHud()] });
/** A word over the stack after a landing ("Perfect!", a milestone), for a moment. */
const callout = ref<{ text: string; lane: number; big: boolean; key: number } | null>(null);
let calloutTimer: number | undefined;
let calloutKey = 0;

const seed = () => Math.floor(Math.random() * 2 ** 31);
// The stacks are plain objects mutated every frame (one, or two in a stack-off); only the HUD is reactive.
let stacks: Stack[] = [newStack(seed())];
let overAt = 0;
let shakes = [0, 0];
/** Where each crane's trolley stopped (it stays where it let go). */
const trolleys = [FIELD_W / 2, FIELD_W / 2];

// --- Today: the daily stack's date and weather (Tugboat Run's daily rules) ---
const today = ref(localDate(new Date()));
const dayConditions = ref<Conditions>(CALM_SEAS);
let runDate = today.value;
let runConditions: Conditions = CALM_SEAS;
const weatherOn = computed(() => weather.place !== null && weather.place !== "off");

function refreshDay() {
  today.value = localDate(new Date());
  dayConditions.value = weatherOn.value ? conditionsFrom(weather.forecast, Date.now()) : CALM_SEAS;
}
watch(() => [weather.forecast, weather.place], refreshDay);

const conditionsText = computed(() => conditionsLine(dayConditions.value, weather.unit, stackOutlook));
const conditionsIcon = computed(() => {
  const c = dayConditions.value;
  if (!c.live) return Waves;
  if (c.snow) return CloudSnow;
  if (c.rain > 0) return CloudRain;
  if (c.night) return CloudMoon;
  if (Math.abs(handlingFor(c).drift) >= 2) return Wind;
  return Sun;
});
const streak = computed(() => currentStreak(scores.value.daily, today.value));
const doneToday = computed(() => sailedToday(scores.value.daily, today.value));
const todaysBest = computed(() => scores.value.daily.bests[phase.value === "ready" ? today.value : runDate] ?? 0);
const dayLabel = computed(() => {
  const [y, m, d] = today.value.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
});

// --- Controls ---
const keys = { left: false, right: false };
/** A drop pressed since the last frame, per lane (keys, a click, a phone). */
const dropPress = [false, false];
/** Each phone (seat 1, seat 2), as its last `game-pad` event said (read by the loop; not reactive). */
const pads = [
  { connected: false, steer: 0, drop: false },
  { connected: false, steer: 0, drop: false },
];
const padDropWas = [false, false];

// --- A stack-off ---
const swapped = ref(false);
const phoneDrove: [boolean, boolean] = [false, false];
const left = reactive([false, false]);
const dropped = ref<Player | null>(null);
const raceResult = ref<{ scores: [number, number]; winner: Player | null } | null>(null);

// --- Phone controller (through Tugboat, shared with Tugboat Run) ---
const sheet = ref(false);
const players = computed<1 | 2>(() => (duo.value ? 2 : 1));
const controller = useGameController({
  game: "harbor-stack",
  players: () => players.value,
  onPad,
  // What each phone shows: paused, its misses (a buzz), and in a stack-off its player.
  feedback: (seat) => {
    if (seat === 2 && !duo.value) return null;
    const player = duo.value ? playerFor(seat, swapped.value) : null;
    const s = stacks[player === null ? 0 : player - 1];
    return { paused: phase.value !== "playing", hits: s?.misses ?? 0, player };
  },
});
const { controllerOn, controllerError, padConnected } = controller;

const blocked = computed(() => tug.ringing !== null || devtools.confirm !== null || tug.pairingRequest !== null);

useFocusTrap(root, onEscape);

function onEscape() {
  if (sheet.value) sheet.value = false;
  else if (phase.value === "playing") pause();
  else close();
}

// --- Phases ---
function start() {
  refreshDay();
  if (mode.value === "daily") {
    runDate = today.value;
    runConditions = dayConditions.value;
    stacks = [newStack(dailySeed(runDate, "harbor-stack"), stackWeather(handlingFor(runConditions)))];
  } else if (mode.value === "duo") {
    // One seed for both: the same containers and the same swell, side by side.
    const s = seed();
    runConditions = CALM_SEAS;
    stacks = [newStack(s), newStack(s)];
  } else {
    runConditions = CALM_SEAS;
    stacks = [newStack(seed())];
  }
  hud.lanes = stacks.map(freshHud);
  newBest.value = false;
  shakes = [0, 0];
  trolleys[0] = trolleys[1] = FIELD_W / 2;
  dropPress[0] = dropPress[1] = false;
  left[0] = left[1] = false;
  dropped.value = null;
  raceResult.value = null;
  callout.value = null;
  for (const p of [1, 2] as Player[]) phoneDrove[p - 1] = padConnected[seatFor(p, swapped.value) - 1];
  syncHud();
  phase.value = "playing";
}

/** A still scene behind the menu, in the mode's look (today's weather, two stacks). */
function preview() {
  runConditions = mode.value === "daily" ? dayConditions.value : CALM_SEAS;
  const s = mode.value === "daily" ? dailySeed(today.value, "harbor-stack") : 1;
  const w = mode.value === "daily" ? stackWeather(handlingFor(runConditions)) : CALM_STACK;
  stacks = duo.value ? [newStack(s), newStack(s)] : [newStack(s, w)];
  hud.lanes = stacks.map(freshHud);
  layoutLanes();
  render();
}

function pause() {
  if (phase.value === "playing") phase.value = "paused";
}

function resume() {
  if (phase.value === "paused" && !blocked.value && dropped.value === null) {
    sheet.value = false;
    phase.value = "playing";
  }
}

function togglePause() {
  if (phase.value === "playing") pause();
  else if (phase.value === "paused") resume();
}

/** Space / Drop when not playing: start, resume or play again (not the instant a game ends). */
function primary() {
  if (phase.value === "ready") start();
  else if (phase.value === "paused") resume();
  else if (phase.value === "over" && performance.now() - overAt > 700) start();
}

function chooseMode(m: Mode) {
  if (phase.value === "playing" || phase.value === "paused") return;
  mode.value = m;
}

function menu() {
  phase.value = "ready";
}

function continueSolo() {
  const p = dropped.value;
  if (p === null) return;
  left[p - 1] = true;
  stacks[p - 1].over = true;
  dropped.value = null;
  syncHud();
  resume();
}

function close() {
  game.close();
}

/** Back to the games picker. */
function allGames() {
  void game.show("picker");
}

/** A drop for `lane` (0 or 1), from any control. */
function pressDrop(lane: number) {
  if (phase.value !== "playing") return;
  if (stacks[lane] && !stacks[lane].over) dropPress[lane] = true;
}

// --- The loop ---
let raf = 0;
let lastTs = 0;

function frame(now: number) {
  raf = 0;
  const dt = lastTs ? (now - lastTs) / 1000 : 1 / 60;
  lastTs = now;
  stacks.forEach((s, i) => {
    s.events = [];
    const seat = duo.value ? seatFor((i + 1) as Player, swapped.value) : 1;
    const pad = pads[seat - 1];
    const nudge = nudgeFrom({ keyLeft: !duo.value && keys.left, keyRight: !duo.value && keys.right, pad });
    advance(s, { drop: dropPress[i], nudge }, dt);
    dropPress[i] = false;
    if (s.crane === null && s.falling) trolleys[i] = s.falling.x;
  });
  let miss = false;
  stacks.forEach((s, i) => {
    shakes[i] = Math.max(0, shakes[i] - dt);
    if (s.events.includes("miss")) {
      if (!reducedMotion.value) shakes[i] = 0.3;
      miss = true;
    }
    if (s.events.includes("milestone") && s.cheer) say(milestoneLine(s.cheer.score) ?? `${s.cheer.score}!`, i, true);
    else if (s.events.includes("grow")) say("Perfect! Wider again", i, false);
    else if (s.events.includes("perfect")) say(s.combo > 1 ? `Perfect ×${s.combo}` : "Perfect!", i, false);
    else if (s.events.includes("miss") && !s.over) say(s.lives === 1 ? "Splash! Last life" : "Splash!", i, false);
  });
  if (miss) sendFeedback();
  render();
  syncHud();
  if (stacks.every((s) => settled(s))) {
    finishGame();
    return;
  }
  if (phase.value === "playing") raf = requestAnimationFrame(frame);
}

function say(text: string, lane: number, big: boolean) {
  callout.value = { text, lane, big, key: ++calloutKey };
  window.clearTimeout(calloutTimer);
  calloutTimer = window.setTimeout(() => (callout.value = null), big ? 2400 : 900);
}

function finishGame() {
  overAt = performance.now();
  if (mode.value === "duo") {
    const pair: [number, number] = [stackScore(stacks[0]), stackScore(stacks[1])];
    raceResult.value = { scores: pair, winner: winner(pair) };
    newBest.value = game.finishDuo(pair, "harbor-stack");
  } else if (mode.value === "daily") {
    newBest.value = game.finishDay(runDate, stackScore(stacks[0]), "harbor-stack");
  } else {
    newBest.value = game.finish(stackScore(stacks[0]), "harbor-stack");
  }
  phase.value = "over";
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
  stacks.forEach((s, i) => {
    const h = hud.lanes[i];
    if (!h) return;
    const sc = stackScore(s);
    if (h.score !== sc) h.score = sc;
    if (h.lives !== s.lives) h.lives = s.lives;
    if (h.combo !== s.combo) h.combo = s.combo;
    if (h.over !== s.over) h.over = s.over;
  });
}

watch(phase, (p) => {
  if (p === "playing") startLoop();
  else {
    stopLoop();
    if (p === "ready") {
      refreshDay();
      preview();
    } else render();
  }
  sendFeedback();
});

watch(mode, () => {
  preview();
  if (controllerOn.value) void controller.open();
});

watch(dayConditions, () => {
  if (phase.value === "ready") preview();
});

watch(blocked, (b) => {
  if (b) pause();
});

watch(swapped, () => sendFeedback());

// --- Drawing ---
let ctx: CanvasRenderingContext2D | null = null;
let size = { width: 1, height: 1, dpr: 1 };
let fits: Layout[] = [layout(1, 1)];
const laneBoxes = ref<Array<{ left: number; width: number }>>([]);
let day: StackPalette | null = null;
let night: StackPalette | null = null;
const reducedMotion = ref(false);

function layoutLanes() {
  fits = duo.value ? laneLayouts(size.width, size.height) : [layout(size.width, size.height)];
  laneBoxes.value = fits.map((l) => ({ left: l.left, width: FIELD_W * l.scale }));
}

function resize() {
  const el = stage.value;
  const c = canvas.value;
  if (!el || !c) return;
  const r = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  size = { width: Math.max(1, r.width), height: Math.max(1, r.height), dpr };
  c.width = Math.max(1, Math.round(size.width * dpr));
  c.height = Math.max(1, Math.round(size.height * dpr));
  layoutLanes();
}

function render() {
  if (!ctx || !day || !night) return;
  const c = runConditions;
  const h = handlingFor(c);
  const sky: Sky | null = c.live ? { rain: c.rain, snow: c.snow, night: c.night, drift: h.drift } : null;
  const base = c.live && c.night ? night : day;
  drawStack(ctx, {
    ...size,
    reducedMotion: reducedMotion.value,
    sky,
    lanes: stacks.map((stack, i) => ({
      stack,
      layout: fits[i] ?? fits[0],
      palette: base,
      hull: i === 1 ? base.teal : base.coral,
      shake: shakes[i] ?? 0,
      trolleyX: trolleys[i] ?? FIELD_W / 2,
    })),
  });
}

const PALETTE_NAMES = [
  "sky",
  "skyline",
  "waterBase",
  "water",
  "waterDeep",
  "foam",
  "deck",
  "cabin",
  "ink",
  "rope",
  "steel",
  "steelDark",
  "coral",
  "teal",
  "amber",
  "slate",
  "green",
  "gold",
  "spray",
  "flake",
  "tower",
  "lamp",
  "beam",
  "star",
] as const;

// --- Input handlers ---
const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && t.closest("input, textarea, [contenteditable]") !== null;

function onKey(e: KeyboardEvent, down: boolean) {
  if (e.ctrlKey || e.metaKey || e.altKey || blocked.value || isTyping(e.target)) return;
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  // Enter on a focused button is the button's.
  if (k === "Enter" && e.target instanceof HTMLButtonElement) return;
  const off = duo.value ? stackOffKey(k) : null;
  if (off !== null) {
    if (down && !e.repeat) {
      if (phase.value === "playing") pressDrop(off - 1);
      else if (k === "Enter" && phase.value === "over") primary();
    }
  } else if (k === " " || (!duo.value && (k === "Enter" || k === "ArrowDown" || k === "s"))) {
    if (down && !e.repeat) {
      if (phase.value === "playing") {
        if (!duo.value) pressDrop(0);
      } else primary();
    }
  } else if (!duo.value && (k === "ArrowLeft" || k === "a" || k === "ArrowRight" || k === "d")) {
    if (k === "ArrowLeft" || k === "a") keys.left = down;
    else keys.right = down;
  } else if (k === "p") {
    if (down && !e.repeat) togglePause();
  } else if (k === "r" && phase.value === "over") {
    if (down && !e.repeat) primary();
  } else {
    return;
  }
  // These keys belong to the game, not to a focused button or the page.
  e.preventDefault();
}
const onKeyDown = (e: KeyboardEvent) => onKey(e, true);
const onKeyUp = (e: KeyboardEvent) => onKey(e, false);

/** A click drops: on the press, so it feels as quick as a key. In a stack-off, on that stack. */
function onPointerDown(e: PointerEvent) {
  if (e.button !== 0 || phase.value !== "playing") return;
  if (!duo.value) {
    pressDrop(0);
    return;
  }
  const x = toFieldX(e.offsetX, fits[1] ?? fits[0]);
  pressDrop(x >= -7 ? 1 : 0);
}

function releaseKeys() {
  keys.left = keys.right = false;
}
function onBlur() {
  pause();
  releaseKeys();
}
function onFocus() {
  if (phase.value === "ready" || phase.value === "over") refreshDay();
}
function onVisibility() {
  if (document.visibilityState !== "visible") onBlur();
}

// --- Phones ---
function onPad(e: GamePad) {
  const seat: Seat = e.seat === 2 ? 2 : 1;
  const i = seat - 1;
  const pad = pads[i];
  const wasConnected = pad.connected;
  pad.connected = e.connected;
  pad.steer = e.connected ? e.steer : 0;
  pad.drop = e.drop;
  const inGame = duo.value && (phase.value === "playing" || phase.value === "paused");
  const player = duo.value ? playerFor(seat, swapped.value) : 1;
  if (e.connected && inGame) {
    phoneDrove[player - 1] = true;
    if (dropped.value === player) dropped.value = null;
  }
  if (!e.connected && wasConnected && duo.value) {
    const p = droppedPlayer(seat, {
      playing: inGame,
      swapped: swapped.value,
      out: [stacks[0]?.over ?? true, stacks[1]?.over ?? true],
      phoneDrove,
    });
    if (p !== null) {
      dropped.value = p;
      pause();
    }
  }
  const fresh = e.connected && e.drop && !padDropWas[i];
  if (fresh && phase.value === "playing") pressDrop(player - 1);
  // From a phone alone: Drop starts, resumes or plays again (on a fresh press).
  if (fresh && phase.value !== "playing" && !blocked.value) {
    sheet.value = false;
    primary();
  }
  padDropWas[i] = e.connected && e.drop;
}

const sendFeedback = () => controller.sendFeedback();

async function useController() {
  pause();
  sheet.value = true;
  await controller.use();
}

async function controllerOff() {
  sheet.value = false;
  await controller.off();
}

const seatOf = (p: Player) => seatFor(p, swapped.value);
const phoneFor = (p: Player) => padConnected[seatOf(p) - 1];
const anyPhone = computed(() => padConnected[0] || padConnected[1]);
const KEYS_LINE: Record<Player, string> = { 1: "↓ or Enter drops", 2: "S drops" };

// --- Lifetime ---
let ro: ResizeObserver | null = null;
let dprQuery: MediaQueryList | null = null;
let motionQuery: MediaQueryList | null = null;

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
const onMotion = (e: MediaQueryListEvent) => {
  reducedMotion.value = e.matches;
  if (!raf) render();
};

onMounted(() => {
  ctx = canvas.value?.getContext("2d") ?? null;
  day = readSwatches(swatches.value, "day", PALETTE_NAMES);
  night = readSwatches(swatches.value, "night", PALETTE_NAMES);
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion.value = motionQuery.matches;
  motionQuery.addEventListener("change", onMotion);
  void weather.init();
  refreshDay();
  resize();
  preview();
  ro = new ResizeObserver(() => {
    resize();
    if (!raf) render();
  });
  if (stage.value) ro.observe(stage.value);
  watchDpr();
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("keyup", onKeyUp);
  window.addEventListener("blur", onBlur);
  window.addEventListener("focus", onFocus);
  document.addEventListener("visibilitychange", onVisibility);
  stage.value?.focus();
});

onUnmounted(() => {
  stopLoop();
  window.clearTimeout(calloutTimer);
  ro?.disconnect();
  dprQuery?.removeEventListener("change", onDprChange);
  motionQuery?.removeEventListener("change", onMotion);
  window.removeEventListener("keydown", onKeyDown);
  window.removeEventListener("keyup", onKeyUp);
  window.removeEventListener("blur", onBlur);
  window.removeEventListener("focus", onFocus);
  document.removeEventListener("visibilitychange", onVisibility);
});

const MODES: Array<{ id: Mode; label: string; icon: typeof Play }> = [
  { id: "daily", label: "Daily", icon: CalendarDays },
  { id: "free", label: "Free play", icon: Play },
  { id: "duo", label: "Stack-off", icon: Users },
];
const PLAYER_NAME: Record<Player, string> = { 1: "Player 1", 2: "Player 2" };
const leader = computed(() => raceResult.value?.winner ?? null);
const best = computed(() => scores.value.best);
const duoBest = computed(() => scores.value.duoBest);
</script>

<template>
  <div ref="root" role="dialog" aria-modal="true" aria-labelledby="stack-title" class="fixed inset-0 z-50 flex flex-col bg-surface-dark text-on-dark">
    <header class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-surface-dark-elevated px-5 py-3">
      <Container :size="20" class="shrink-0 text-primary" />
      <h2 id="stack-title" class="font-display text-[24px] leading-none text-on-dark" style="letter-spacing: -0.02em">Harbor Stack</h2>

      <div v-if="!duo" class="ml-2 flex items-center gap-4 font-mono text-[14px]" aria-live="off">
        <span><span class="text-on-dark-soft">Stacked</span> {{ hud.lanes[0]?.score ?? 0 }}</span>
        <span v-if="mode === 'daily'"><span class="text-on-dark-soft">Today's best</span> {{ Math.max(todaysBest, phase === "ready" ? 0 : (hud.lanes[0]?.score ?? 0)) }}</span>
        <span v-else><span class="text-on-dark-soft">Best</span> {{ Math.max(best, hud.lanes[0]?.score ?? 0) }}</span>
        <span class="flex items-center gap-1" :aria-label="`${hud.lanes[0]?.lives ?? LIVES} of ${LIVES} lives`" role="img">
          <Package v-for="i in LIVES" :key="i" :size="15" :class="i <= (hud.lanes[0]?.lives ?? LIVES) ? 'text-primary' : 'text-on-dark-soft/30'" />
        </span>
        <span
          :class="[
            'flex items-center gap-1 rounded-full px-2 py-0.5 text-[12px]',
            (hud.lanes[0]?.combo ?? 0) > 1 ? 'bg-accent-amber/20 text-accent-amber' : 'text-on-dark-soft/50',
          ]"
        >
          <Sparkles :size="12" />
          ×{{ hud.lanes[0]?.combo ?? 0 }}
        </span>
      </div>
      <div v-else class="ml-2 font-mono text-[14px]" aria-live="off">
        <span class="text-on-dark-soft">Best stack-off</span> {{ Math.max(duoBest, ...(phase === "ready" ? [0] : hud.lanes.map((l) => l.score))) }}
      </div>

      <div class="ml-auto flex items-center gap-2">
        <span v-if="controllerOn" class="pill bg-surface-dark-elevated px-2.5 py-0.5 text-[12px] text-on-dark" role="status">
          <span :class="['size-1.5 rounded-full', anyPhone ? 'bg-accent-teal' : 'bg-on-dark-soft/40']" />
          <template v-if="duo">{{ (padConnected[0] ? 1 : 0) + (padConnected[1] ? 1 : 0) }} of 2 phones</template>
          <template v-else>{{ padConnected[0] ? "Phone connected" : "Phone not connected" }}</template>
        </span>
        <button class="btn-on-dark btn-sm" @click="useController">
          <Smartphone :size="14" />
          {{ duo ? "Use phones as controllers" : "Use your phone as a controller" }}
        </button>
        <button
          v-if="phase === 'playing' || phase === 'paused'"
          class="btn-on-dark btn-sm w-8 px-0"
          :disabled="phase === 'paused' && (dropped !== null || blocked)"
          :aria-label="phase === 'playing' ? 'Pause' : 'Resume'"
          :title="phase === 'playing' ? 'Pause (P)' : 'Resume (P)'"
          @click="togglePause"
        >
          <Pause v-if="phase === 'playing'" :size="14" />
          <Play v-else :size="14" />
        </button>
        <button class="btn-on-dark btn-sm w-8 px-0" aria-label="Close Harbor Stack" @click="close">
          <X :size="15" />
        </button>
      </div>
    </header>

    <div class="relative flex min-h-0 flex-1">
      <div ref="stage" tabindex="-1" class="relative min-w-0 flex-1 outline-none">
        <canvas
          ref="canvas"
          class="absolute inset-0 block size-full touch-none"
          :aria-label="duo ? 'Harbor Stack stack-off, two stacks' : 'Harbor Stack: the crane, the boat and the stack'"
          @pointerdown="onPointerDown"
        />

        <!-- A word after a landing: "Perfect!", or a milestone's cheer. -->
        <div
          v-if="callout && phase === 'playing'"
          :key="callout.key"
          class="pointer-events-none absolute top-[18%] flex justify-center"
          :style="laneBoxes[callout.lane] ? { left: `${laneBoxes[callout.lane].left}px`, width: `${laneBoxes[callout.lane].width}px` } : { left: '0', right: '0' }"
        >
          <p
            :class="[
              'animate-step-in rounded-full bg-surface-dark/85 px-4 py-1.5 text-center ring-1 ring-surface-dark-elevated',
              callout.big ? 'font-display text-[22px] text-accent-amber' : 'text-[15px] font-medium text-on-dark',
            ]"
            :role="callout.big ? 'status' : undefined"
          >
            {{ callout.text }}
          </p>
        </div>

        <!-- A stack-off: each stack's player, score and lives on top of it, and a card once it's out. -->
        <template v-if="duo && phase !== 'ready'">
          <div
            v-for="(box, i) in laneBoxes"
            :key="i"
            class="pointer-events-none absolute top-3 flex justify-center"
            :style="{ left: `${box.left}px`, width: `${box.width}px` }"
          >
            <div class="flex items-center gap-2.5 rounded-full bg-surface-dark/85 px-3 py-1 font-mono text-[13px] ring-1 ring-surface-dark-elevated">
              <span :class="['size-2.5 rounded-full', i === 0 ? 'bg-primary' : 'bg-accent-teal']" />
              <span class="font-sans text-on-dark-soft">{{ PLAYER_NAME[(i + 1) as Player] }}</span>
              <span>{{ hud.lanes[i]?.score ?? 0 }}</span>
              <span class="flex items-center gap-0.5" :aria-label="`${hud.lanes[i]?.lives ?? 0} of ${LIVES} lives`" role="img">
                <Package v-for="n in LIVES" :key="n" :size="12" :class="n <= (hud.lanes[i]?.lives ?? 0) ? (i === 0 ? 'text-primary' : 'text-accent-teal') : 'text-on-dark-soft/30'" />
              </span>
            </div>
          </div>
          <div
            v-for="(box, i) in laneBoxes"
            v-show="hud.lanes[i]?.over && phase === 'playing'"
            :key="`out${i}`"
            class="pointer-events-none absolute top-1/3 flex justify-center"
            :style="{ left: `${box.left}px`, width: `${box.width}px` }"
          >
            <div class="rounded-2xl bg-surface-dark/90 px-5 py-3 text-center ring-1 ring-surface-dark-elevated">
              <p class="font-display text-[22px] leading-tight">{{ left[i] ? "Left the stack-off" : "Out of containers!" }}</p>
              <p class="font-mono text-[18px]">{{ hud.lanes[i]?.score ?? 0 }}</p>
              <p class="text-[12px] text-on-dark-soft">Waiting for {{ PLAYER_NAME[(2 - i) as Player] }}</p>
            </div>
          </div>
        </template>

        <!-- Ready, paused, over: one card over the still scene. -->
        <div v-if="phase !== 'playing'" class="absolute inset-0 flex overflow-y-auto p-6">
          <div class="m-auto w-full max-w-[420px] rounded-2xl bg-surface-dark/90 px-7 py-6 text-center shadow-xl ring-1 ring-surface-dark-elevated">
            <template v-if="phase === 'ready'">
              <p class="font-display text-[30px] leading-tight">Ready to stack?</p>
              <div class="mt-4 grid grid-cols-3 gap-1 rounded-xl bg-surface-dark-elevated p-1" role="tablist" aria-label="How to play">
                <button
                  v-for="m in MODES"
                  :key="m.id"
                  role="tab"
                  :aria-selected="mode === m.id"
                  :class="[
                    'flex items-center justify-center gap-1.5 rounded-lg px-2 py-1.5 text-[13px]',
                    mode === m.id ? 'bg-surface-dark text-on-dark shadow-sm' : 'text-on-dark-soft',
                  ]"
                  @click="chooseMode(m.id)"
                >
                  <component :is="m.icon" :size="14" />
                  {{ m.label }}
                </button>
              </div>

              <template v-if="mode === 'daily'">
                <p class="mt-4 text-[13px] text-on-dark-soft">Today's stack · {{ dayLabel }}</p>
                <p class="mt-1 text-[13px] text-on-dark-soft/80">The same containers and swell for everyone today.</p>
                <p class="mt-3 flex items-center justify-center gap-2 rounded-xl bg-surface-dark-elevated px-3 py-2 text-[14px] text-on-dark" role="status">
                  <component :is="conditionsIcon" :size="16" class="shrink-0 text-accent-teal" />
                  {{ conditionsText }}
                </p>
                <p v-if="weather.ready && weather.place === null" class="mt-1.5 text-[12px] text-on-dark-soft/80">Set up the weather card and today's stack sways in your real weather.</p>
                <div class="mt-3 flex items-center justify-center gap-4 text-[13px]">
                  <span class="flex items-center gap-1.5" :class="streak > 0 ? 'text-accent-amber' : 'text-on-dark-soft'">
                    <Flame :size="15" />
                    {{ streak === 0 ? "No streak yet" : streak === 1 ? "1 day streak" : `${streak} day streak` }}
                  </span>
                  <span class="text-on-dark-soft">{{ doneToday ? `Today's best: ${todaysBest}` : "Not stacked yet today" }}</span>
                </div>
                <button class="btn-primary mt-5" @click="start">
                  <Play :size="16" />
                  {{ doneToday ? "Stack today's again" : "Start today's stack" }}
                </button>
              </template>

              <template v-else-if="mode === 'free'">
                <p class="mt-4 text-[14px] text-on-dark-soft">
                  Space, Enter or a click drops the container. Whatever hangs over the edge is sliced off, so line it up. A perfect drop keeps
                  its width, and four in a row win some back.
                </p>
                <p class="mt-1.5 text-[13px] text-on-dark-soft/80">← → slow the crane down or speed it up. A miss costs one of three containers.</p>
                <button class="btn-primary mt-5" @click="start">
                  <Play :size="16" />
                  Start
                </button>
              </template>

              <template v-else>
                <p class="mt-4 text-[14px] text-on-dark-soft">Stack side by side with the same containers. The taller stack wins.</p>
                <ul class="mt-3 space-y-2 text-left">
                  <li v-for="p in [1, 2] as Player[]" :key="p" class="flex items-center gap-3 rounded-xl bg-surface-dark-elevated px-3 py-2">
                    <span :class="['size-3 shrink-0 rounded-full', p === 1 ? 'bg-primary' : 'bg-accent-teal']" />
                    <div class="min-w-0 flex-1">
                      <p class="text-[14px] text-on-dark">{{ PLAYER_NAME[p] }}</p>
                      <p class="text-[12px] text-on-dark-soft">{{ phoneFor(p) ? "Phone connected · or " : "" }}{{ KEYS_LINE[p] }}</p>
                    </div>
                    <Smartphone v-if="phoneFor(p)" :size="16" class="shrink-0 text-accent-teal" aria-label="Phone connected" />
                  </li>
                </ul>
                <div class="mt-3 flex flex-wrap items-center justify-center gap-2">
                  <button v-if="controllerOn" class="btn-on-dark btn-sm" @click="swapped = !swapped">
                    <ArrowLeftRight :size="14" />
                    Swap phones
                  </button>
                  <button v-else class="btn-on-dark btn-sm" @click="useController">
                    <Smartphone :size="14" />
                    Add phones
                  </button>
                </div>
                <button class="btn-primary mt-5" @click="start">
                  <Play :size="16" />
                  Start stack-off
                </button>
              </template>
              <button class="mt-3 block w-full text-[12px] text-on-dark-soft underline-offset-2 hover:underline" @click="allGames">All games</button>
            </template>

            <template v-else-if="phase === 'paused' && dropped !== null">
              <p class="font-display text-[28px] leading-tight">{{ PLAYER_NAME[dropped] }} disconnected — waiting…</p>
              <p class="mt-2 text-[14px] text-on-dark-soft">Open the Tugboat page on their phone again, or carry on alone.</p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="continueSolo">Continue solo</button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
            </template>

            <template v-else-if="phase === 'paused'">
              <p class="font-display text-[30px] leading-tight">Paused</p>
              <p class="mt-2 text-[14px] text-on-dark-soft">Space or P to carry on. Esc again to close.</p>
              <button class="btn-primary mt-5" :disabled="blocked" @click="resume">
                <Play :size="16" />
                Resume
              </button>
            </template>

            <template v-else-if="duo && raceResult">
              <p class="flex items-center justify-center gap-2 font-display text-[30px] leading-tight">
                <Trophy v-if="leader" :size="24" :class="leader === 1 ? 'text-primary' : 'text-accent-teal'" />
                {{ leader ? `${PLAYER_NAME[leader]} wins!` : "It's a tie!" }}
              </p>
              <ul class="mx-auto mt-4 max-w-[260px] space-y-1.5">
                <li v-for="p in [1, 2] as Player[]" :key="p" class="flex items-center gap-2.5 text-[15px]">
                  <span :class="['size-2.5 rounded-full', p === 1 ? 'bg-primary' : 'bg-accent-teal']" />
                  <span class="text-on-dark-soft">{{ PLAYER_NAME[p] }}</span>
                  <span v-if="left[p - 1]" class="text-[12px] text-on-dark-soft/80">left</span>
                  <span class="ml-auto font-mono text-[20px]" :class="leader === p ? 'text-on-dark' : 'text-on-dark-soft'">{{ raceResult.scores[p - 1] }}</span>
                </li>
              </ul>
              <p class="mt-3 text-[13px] text-on-dark-soft">{{ newBest ? "The tallest stack-off stack on this PC!" : `Best stack-off on this PC: ${duoBest}` }}</p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="start">
                  <RotateCcw :size="16" />
                  Rematch
                </button>
                <button class="btn-on-dark" @click="menu">Menu</button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
              <p class="mt-3 text-[12px] text-on-dark-soft/80">Space for a rematch.</p>
            </template>

            <template v-else>
              <p class="font-display text-[30px] leading-tight">{{ newBest ? (mode === "daily" ? "Today's best!" : "New best!") : "Stack complete" }}</p>
              <p class="mt-2 font-mono text-[28px] text-on-dark">{{ hud.lanes[0]?.score ?? 0 }}</p>
              <p class="text-[13px] text-on-dark-soft">{{ (hud.lanes[0]?.score ?? 0) === 1 ? "container stacked" : "containers stacked" }}</p>
              <template v-if="mode === 'daily'">
                <p class="mt-1 text-[13px] text-on-dark-soft">Today's best: {{ todaysBest }}</p>
                <p class="mt-2 flex items-center justify-center gap-1.5 text-[14px] text-accent-amber">
                  <Flame :size="15" />
                  {{ streak === 1 ? "1 day streak" : `${streak} day streak` }}{{ streak > 1 ? " — see you tomorrow!" : " — come back tomorrow to keep it going" }}
                </p>
              </template>
              <p v-else class="mt-1 text-[13px] text-on-dark-soft">{{ newBest ? "Your tallest stack on this PC." : `Best on this PC: ${best}` }}</p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="start">
                  <RotateCcw :size="16" />
                  Play again
                </button>
                <button class="btn-on-dark" @click="menu">Menu</button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
              <p class="mt-3 text-[12px] text-on-dark-soft/80">Space plays again.</p>
            </template>
          </div>
        </div>
      </div>

      <PhoneControllerSheet
        v-if="sheet"
        :duo="duo"
        :controller-on="controllerOn"
        :controller-error="controllerError"
        :pad-connected="padConnected"
        :swapped="swapped"
        solo-hint="Drop with the big button; the slider nudges the crane."
        :can-swap="duo && controllerOn && (phase === 'ready' || phase === 'over')"
        :can-resume="phase === 'paused' && dropped === null"
        :resume-disabled="blocked"
        @close="sheet = false"
        @swap="swapped = !swapped"
        @resume="resume"
        @off="controllerOff"
      />
    </div>

    <!-- tug's colours for the canvas (theme tokens only, no colour in code). Night swatches
         replace the day ones they name; the rest are shared. -->
    <div ref="swatches" class="hidden" aria-hidden="true">
      <span data-swatch="day:sky" class="bg-surface-soft" />
      <span data-swatch="day:skyline" class="bg-surface-cream-strong" />
      <span data-swatch="day:waterBase" class="bg-canvas" />
      <span data-swatch="day:water" class="bg-accent-teal/30" />
      <span data-swatch="day:waterDeep" class="bg-accent-teal/70" />
      <span data-swatch="day:foam" class="bg-canvas" />
      <span data-swatch="day:deck" class="bg-canvas" />
      <span data-swatch="day:cabin" class="bg-surface-dark" />
      <span data-swatch="day:ink" class="bg-ink" />
      <span data-swatch="day:rope" class="bg-body" />
      <span data-swatch="day:steel" class="bg-muted" />
      <span data-swatch="day:steelDark" class="bg-surface-dark-elevated" />
      <span data-swatch="day:coral" class="bg-primary" />
      <span data-swatch="day:teal" class="bg-accent-teal" />
      <span data-swatch="day:amber" class="bg-accent-amber" />
      <span data-swatch="day:slate" class="bg-surface-dark-elevated" />
      <span data-swatch="day:green" class="bg-success" />
      <span data-swatch="day:gold" class="bg-warning" />
      <span data-swatch="day:spray" class="bg-surface-dark/35" />
      <span data-swatch="day:flake" class="bg-canvas" />
      <span data-swatch="day:tower" class="bg-canvas" />
      <span data-swatch="day:lamp" class="bg-accent-amber" />
      <span data-swatch="day:beam" class="bg-accent-amber/15" />
      <span data-swatch="day:star" class="bg-on-dark/70" />
      <span data-swatch="night:sky" class="bg-surface-dark" />
      <span data-swatch="night:skyline" class="bg-surface-dark-elevated" />
      <span data-swatch="night:waterBase" class="bg-surface-dark" />
      <span data-swatch="night:water" class="bg-accent-teal/15" />
      <span data-swatch="night:waterDeep" class="bg-accent-teal/40" />
      <span data-swatch="night:foam" class="bg-on-dark/60" />
      <span data-swatch="night:steel" class="bg-on-dark-soft" />
      <span data-swatch="night:steelDark" class="bg-muted" />
      <span data-swatch="night:slate" class="bg-muted" />
      <span data-swatch="night:ink" class="bg-surface-dark" />
      <span data-swatch="night:rope" class="bg-on-dark-soft" />
      <span data-swatch="night:spray" class="bg-on-dark/45" />
      <span data-swatch="night:flake" class="bg-on-dark/85" />
    </div>
  </div>
</template>
