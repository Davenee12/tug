<script setup lang="ts">
// Tugboat Run: a small harbor game over the whole window, from the sidebar's Play button or
// Ctrl+K "game". Three ways to play: today's course (the daily challenge, the same course for
// everyone today, shaped by the player's real weather), Free play (a fresh course every time), and
// Two players (a race in two lanes on one course). Keyboard, mouse, or phones as controllers
// (through Tugboat).
//
// Idle by design: requestAnimationFrame runs only while a run is being played (open, on screen,
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
  Flame,
  Gamepad2,
  LifeBuoy,
  LoaderCircle,
  Pause,
  Play,
  RotateCcw,
  Smartphone,
  Sun,
  Trophy,
  Users,
  Waves,
  Wind,
  X,
} from "lucide-vue-next";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useGameStore } from "../stores/game";
import { useTugStore } from "../stores/tug";
import { useTugboatStore } from "../stores/tugboat";
import { useDevToolsStore } from "../stores/devtools";
import { useWeatherStore } from "../stores/weather";
import { useFocusTrap } from "../lib/focusTrap";
import { api, on } from "../lib/ipc";
import { capitalised } from "../lib/tugboat";
import { CALM, FIELD_W, LIVES, advance, laneLayouts, layout, mergeInput, newRun, score, toFieldX, type Layout, type Run } from "../lib/tugboatRun";
import { CALM_SEAS, conditionsFrom, conditionsLine, currentStreak, dailySeed, handlingFor, localDate, sailedToday, type Conditions } from "../lib/tugboatDaily";
import { droppedPlayer, playerFor, raceKey, seatFor, winner, type Player, type Seat } from "../lib/tugboatRace";
import type { GamePad } from "../types/protocol";
import { draw, type Palette, type Sky } from "./tugboatRunDraw";

type Phase = "ready" | "playing" | "paused" | "over";
type Mode = "daily" | "free" | "duo";

const game = useGameStore();
const tug = useTugStore();
const tugboat = useTugboatStore();
const devtools = useDevToolsStore();
const weather = useWeatherStore();

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
  hopReady: boolean;
  over: boolean;
}
const freshHud = (): LaneHud => ({ score: 0, lives: LIVES, hopReady: true, over: false });
const hud = reactive<{ lanes: LaneHud[] }>({ lanes: [freshHud()] });

const seed = () => Math.floor(Math.random() * 2 ** 31);
// The runs are plain objects mutated every frame (one, or two in a race); only the HUD is reactive.
let runs: Run[] = [newRun(seed())];
let overAt = 0;
let shakes = [0, 0];

// --- Today: the daily challenge's date and water ---
const today = ref(localDate(new Date()));
const dayConditions = ref<Conditions>(CALM_SEAS);
/** The day and weather a daily run started with (kept for the whole run, even past midnight). */
let runDate = today.value;
let runConditions: Conditions = CALM_SEAS;
const weatherOn = computed(() => weather.place !== null && weather.place !== "off");

/** Read the clock and the weather again: on opening, before a run, back at the menu, on focus. */
function refreshDay() {
  today.value = localDate(new Date());
  dayConditions.value = weatherOn.value ? conditionsFrom(weather.forecast, Date.now()) : CALM_SEAS;
}
watch(() => [weather.forecast, weather.place], refreshDay);

const conditionsText = computed(() => conditionsLine(dayConditions.value, weather.unit));
const conditionsIcon = computed(() => {
  const c = dayConditions.value;
  if (!c.live) return Waves;
  if (c.snow) return CloudSnow;
  if (c.rain > 0) return CloudRain;
  if (c.night) return CloudMoon;
  if (Math.abs(handlingFor(c).drift) >= 2) return Wind;
  return Sun;
});
const streak = computed(() => currentStreak(game.daily, today.value));
const doneToday = computed(() => sailedToday(game.daily, today.value));
const todaysBest = computed(() => game.daily.bests[phase.value === "ready" ? today.value : runDate] ?? 0);
const dayLabel = computed(() => {
  const [y, m, d] = today.value.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
});

// --- Controls ---
const keys = { left: false, right: false, boost: false };
/** Each player's own keys in a race (Player 1 the arrows, Player 2 A, D and W). */
const raceKeys = [
  { left: false, right: false, hop: false },
  { left: false, right: false, hop: false },
];
let pointerX: number | null = null;
let pointerBoost = false;
let pointerDown: { at: number; x: number; y: number } | null = null;
/** Each phone (seat 1, seat 2), as its last `game-pad` event said (read by the loop; not reactive). */
const pads = [
  { connected: false, steer: 0, boost: false },
  { connected: false, steer: 0, boost: false },
];
/** A phone's slider moved more recently than its player's keys or the mouse: the boat follows it. */
const padLeads = [false, false];
/** A Boost press from a phone since the last frame (its release may arrive before the frame). */
const padPress = [false, false];
const padBoostWas = [false, false];
const padConnected = reactive([false, false]);

// --- A race ---
/** The first phone plays Player 2 (so the keyboard can be Player 1). */
const swapped = ref(false);
/** Each player's phone has steered in this race (only those can "disconnect"). */
const phoneDrove: [boolean, boolean] = [false, false];
/** Each player who left the race (their phone dropped and the other carried on solo). */
const left = reactive([false, false]);
/** A player whose phone dropped mid-race: the race waits for them. */
const dropped = ref<Player | null>(null);
const raceResult = ref<{ scores: [number, number]; winner: Player | null } | null>(null);

// --- Phone controller (through Tugboat) ---
/** "Use your phone as a controller" is showing. */
const sheet = ref(false);
/** The controller channel was opened (closed again when the game closes). */
const controllerOn = ref(false);
const controllerError = ref<string | null>(null);
const ts = computed(() => tugboat.status);
const phoneName = computed(() => ts.value.phone ?? "phone");
const players = computed<1 | 2>(() => (duo.value ? 2 : 1));

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
  refreshDay();
  if (mode.value === "daily") {
    runDate = today.value;
    runConditions = dayConditions.value;
    runs = [newRun(dailySeed(runDate), handlingFor(runConditions))];
  } else if (mode.value === "duo") {
    // One seed for both lanes: the same course, side by side.
    const s = seed();
    runs = [newRun(s), newRun(s)];
  } else {
    runs = [newRun(seed())];
  }
  hud.lanes = runs.map(freshHud);
  newBest.value = false;
  shakes = [0, 0];
  left[0] = left[1] = false;
  dropped.value = null;
  raceResult.value = null;
  for (const p of [1, 2] as Player[]) phoneDrove[p - 1] = padConnected[seatFor(p, swapped.value) - 1];
  syncHud();
  phase.value = "playing";
}

/** A fresh, still course to show behind the menu (in the mode's look: today's weather, two lanes). */
function preview() {
  runConditions = mode.value === "daily" ? dayConditions.value : CALM_SEAS;
  const s = mode.value === "daily" ? dailySeed(today.value) : 1;
  runs = duo.value ? [newRun(s), newRun(s)] : [newRun(s, mode.value === "daily" ? handlingFor(runConditions) : CALM)];
  hud.lanes = runs.map(freshHud);
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

/** Space / Boost: start, resume or play again (not the instant a run ends, so a held key doesn't). */
function primary() {
  if (phase.value === "ready") start();
  else if (phase.value === "paused") resume();
  else if (phase.value === "over" && performance.now() - overAt > 700) start();
}

function chooseMode(m: Mode) {
  if (phase.value === "playing" || phase.value === "paused") return;
  mode.value = m;
}

/** Back to the menu from the end of a run. */
function menu() {
  phase.value = "ready";
}

/** The dropped player's phone isn't coming back: their boat leaves the race, the other carries on. */
function continueSolo() {
  const p = dropped.value;
  if (p === null) return;
  left[p - 1] = true;
  runs[p - 1].over = true;
  dropped.value = null;
  syncHud();
  resume();
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
  if (duo.value) {
    for (const p of [1, 2] as Player[]) {
      const run = runs[p - 1];
      run.events = [];
      if (run.over) continue;
      const seat = seatFor(p, swapped.value);
      const k = raceKeys[p - 1];
      const input = mergeInput(
        { keyLeft: k.left, keyRight: k.right, keyBoost: k.hop, pointerX: null, pointerBoost: padPress[seat - 1], pad: pads[seat - 1], padLeads: padLeads[seat - 1] },
        run.x,
      );
      advance(run, input, dt);
    }
  } else {
    const run = runs[0];
    run.events = [];
    // A tap (mouse or phone) that came and went since the last frame still counts as a press.
    const tapped = pointerBoost || padPress[0];
    advance(run, mergeInput({ keyLeft: keys.left, keyRight: keys.right, keyBoost: keys.boost, pointerX, pointerBoost: tapped, pad: pads[0], padLeads: padLeads[0] }, run.x), dt);
  }
  pointerBoost = false;
  padPress[0] = padPress[1] = false;
  let hit = false;
  runs.forEach((run, i) => {
    shakes[i] = Math.max(0, shakes[i] - dt);
    if (run.events.includes("hit")) {
      if (!reducedMotion.value) shakes[i] = 0.25;
      hit = true;
    }
  });
  if (hit) sendFeedback();
  render();
  syncHud();
  if (runs.every((r) => r.over)) {
    finishRun();
    return;
  }
  if (phase.value === "playing") raf = requestAnimationFrame(frame);
}

function finishRun() {
  overAt = performance.now();
  if (mode.value === "duo") {
    const scores: [number, number] = [score(runs[0]), score(runs[1])];
    raceResult.value = { scores, winner: winner(scores) };
    newBest.value = game.finishDuo(scores);
  } else if (mode.value === "daily") {
    newBest.value = game.finishDay(runDate, score(runs[0]));
  } else {
    newBest.value = game.finish(score(runs[0]));
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
  runs.forEach((run, i) => {
    const h = hud.lanes[i];
    if (!h) return;
    const s = score(run);
    if (h.score !== s) h.score = s;
    if (h.lives !== run.lives) h.lives = run.lives;
    const ready = run.hop === 0 && run.cooldown === 0;
    if (h.hopReady !== ready) h.hopReady = ready;
    if (h.over !== run.over) h.over = run.over;
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
  if (controllerOn.value) void openController();
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
/** Each lane's place on screen (CSS pixels), for the race's name tags and "sunk" cards. */
const laneBoxes = ref<Array<{ left: number; width: number }>>([]);
let day: Palette | null = null;
let night: Palette | null = null;
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
  const h = runs[0]?.handling ?? CALM;
  const sky: Sky | null = c.live ? { rain: c.rain, snow: c.snow, night: c.night, drift: h.drift } : null;
  const base = c.live && c.night ? night : day;
  draw(ctx, {
    ...size,
    reducedMotion: reducedMotion.value,
    sky,
    lanes: runs.map((run, i) => ({
      run,
      layout: fits[i] ?? fits[0],
      palette: base,
      hull: i === 1 ? base.hull2 : base.hull,
      shake: shakes[i] ?? 0,
      lighthouse: i === 1 ? "right" : "left",
    })),
  });
}

/** tug's colours, read from theme classes on hidden swatches (no colour is written in code). */
function readPalette(set: "day" | "night"): Palette | null {
  const el = swatches.value;
  if (!el) return null;
  const pick = (name: string) => {
    const s = el.querySelector<HTMLElement>(`[data-swatch="${set}:${name}"]`) ?? el.querySelector<HTMLElement>(`[data-swatch="day:${name}"]`);
    return s ? getComputedStyle(s).backgroundColor : "";
  };
  const names = [
    "waterBase",
    "water",
    "waterDeep",
    "quay",
    "quayEdge",
    "hull",
    "hull2",
    "deck",
    "cabin",
    "ink",
    "rope",
    "coin",
    "log",
    "logEnd",
    "boat",
    "boatCabin",
    "foam",
    "spray",
    "flake",
    "tower",
    "lamp",
    "beam",
    "glow",
  ] as const;
  return Object.fromEntries(names.map((n) => [n, pick(n)])) as unknown as Palette;
}

// --- Input handlers ---
const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && t.closest("input, textarea, [contenteditable]") !== null;

function onKey(e: KeyboardEvent, down: boolean) {
  if (e.ctrlKey || e.metaKey || e.altKey || blocked.value || isTyping(e.target)) return;
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  const race = duo.value ? raceKey(k) : null;
  if (race) {
    const keysOf = raceKeys[race.player - 1];
    keysOf[race.action] = down;
    // That player's keys take the helm back from their phone's slider until it moves again.
    if (down && race.action !== "hop") padLeads[seatFor(race.player, swapped.value) - 1] = false;
    if (down && !e.repeat && race.action === "hop" && phase.value !== "playing") primary();
  } else if (!duo.value && (k === "ArrowLeft" || k === "a" || k === "ArrowRight" || k === "d")) {
    if (k === "ArrowLeft" || k === "a") keys.left = down;
    else keys.right = down;
    // The keyboard takes the helm back from the phone's slider until the slider moves again.
    if (down) padLeads[0] = false;
  } else if (k === " " || (!duo.value && (k === "ArrowUp" || k === "w"))) {
    // Space hops in a solo run; in a race each player has their own hop key, so it only starts.
    if (!duo.value) keys.boost = down;
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
  if (e.button !== 0 || phase.value !== "playing" || duo.value) return;
  try {
    canvas.value?.setPointerCapture(e.pointerId);
  } catch {
    /* the pointer already went */
  }
  pointerX = toFieldX(e.offsetX, fits[0]);
  padLeads[0] = false;
  pointerDown = { at: performance.now(), x: e.clientX, y: e.clientY };
}
function onPointerMove(e: PointerEvent) {
  if (pointerX !== null) pointerX = toFieldX(e.offsetX, fits[0]);
}
function onPointerUp(e: PointerEvent) {
  // A quick tap (not a drag) hops.
  if (pointerDown && performance.now() - pointerDown.at < 220 && Math.hypot(e.clientX - pointerDown.x, e.clientY - pointerDown.y) < 8) {
    pointerBoost = true;
  }
  pointerX = null;
  pointerDown = null;
}

function releaseKeys() {
  keys.left = keys.right = keys.boost = false;
  for (const k of raceKeys) k.left = k.right = k.hop = false;
  pointerX = null;
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
  // The slider moved: the boat follows it from the next frame.
  if (e.connected && pad.connected && e.steer !== pad.steer) padLeads[i] = true;
  if (!e.connected) padLeads[i] = false;
  pad.connected = e.connected;
  pad.steer = e.steer;
  pad.boost = e.boost;
  padConnected[i] = e.connected;
  const inRace = duo.value && (phase.value === "playing" || phase.value === "paused");
  const player = duo.value ? playerFor(seat, swapped.value) : 1;
  if (e.connected && inRace) {
    phoneDrove[player - 1] = true;
    // The phone that dropped is back: the race can go on.
    if (dropped.value === player) dropped.value = null;
  }
  if (!e.connected && wasConnected && duo.value) {
    const p = droppedPlayer(seat, {
      playing: inRace,
      swapped: swapped.value,
      out: [runs[0]?.over ?? true, runs[1]?.over ?? true],
      phoneDrove,
    });
    if (p !== null) {
      raceKeys[p - 1].left = raceKeys[p - 1].right = raceKeys[p - 1].hop = false;
      dropped.value = p;
      pause();
    }
  }
  const fresh = e.connected && e.boost && !padBoostWas[i];
  if (fresh && phase.value === "playing") padPress[i] = true;
  // From a phone alone: Boost starts, resumes or plays again (on a fresh press).
  if (fresh && phase.value !== "playing" && !blocked.value) {
    sheet.value = false;
    primary();
  }
  padBoostWas[i] = e.connected && e.boost;
}

/** Open (or re-size) the controller channel: one phone, or two in a race. */
async function openController() {
  controllerError.value = null;
  try {
    await api.gamePadOpen(players.value);
    lastFeedback[0] = lastFeedback[1] = "";
    sendFeedback();
  } catch (e) {
    controllerOn.value = false;
    controllerError.value = typeof e === "string" ? e : "Couldn't start Tugboat.";
  }
}

async function useController() {
  pause();
  sheet.value = true;
  if (controllerOn.value) return;
  controllerOn.value = true;
  await openController();
}

async function controllerOff() {
  controllerOn.value = false;
  sheet.value = false;
  onPad({ seat: 1, connected: false, steer: 0, boost: false });
  onPad({ seat: 2, connected: false, steer: 0, boost: false });
  await api.gamePadClose().catch(() => undefined);
}

/** Tugboat turned off (its panel closed, tug hidden, or 10 minutes unused): so did the controller. */
watch(
  () => ts.value.phase,
  (p) => {
    if (p === "off" && controllerOn.value) {
      controllerOn.value = false;
      onPad({ seat: 1, connected: false, steer: 0, boost: false });
      onPad({ seat: 2, connected: false, steer: 0, boost: false });
    }
  },
);

const lastFeedback = ["", ""];
/** What each phone shows: paused, its boat's hits (for a buzz), and in a race its player. */
function sendFeedback() {
  if (!controllerOn.value) return;
  const paused = phase.value !== "playing";
  for (const seat of [1, 2] as Seat[]) {
    if (seat === 2 && !duo.value) continue;
    const player = duo.value ? playerFor(seat, swapped.value) : null;
    const run = runs[player === null ? 0 : player - 1];
    const hits = run?.hits ?? 0;
    const key = `${paused}:${hits}:${player}`;
    if (key === lastFeedback[seat - 1]) continue;
    lastFeedback[seat - 1] = key;
    void api.gamePadFeedback(seat, paused, hits, player).catch(() => undefined);
  }
}

/** Whose phone is which, for a race: "Player 1" is coral, "Player 2" teal. */
const seatOf = (p: Player) => seatFor(p, swapped.value);
const phoneFor = (p: Player) => padConnected[seatOf(p) - 1];
const KEYS_LINE: Record<Player, string> = { 1: "← → steer, ↑ hops", 2: "A and D steer, W hops" };

const controllerLine = computed(() => {
  if (controllerError.value) return controllerError.value;
  if (duo.value) {
    const n = (padConnected[0] ? 1 : 0) + (padConnected[1] ? 1 : 0);
    if (n === 2) return "Both phones are connected. Each one shows its player's colour.";
    if (n === 1) return "One phone connected. Scan the same code with a second phone for the other player.";
  } else if (padConnected[0]) return `Your ${phoneName.value} is the controller. Steer with the pad, Boost to hop.`;
  const p = ts.value.phase;
  if (p === "off") return "Starting Tugboat…";
  if (p === "noNetwork") return "Connect this PC to your home or office Wi-Fi to use your phone.";
  if (duo.value) return "Both players scan this code with their phone's camera.";
  if (p === "connected" && ts.value.phoneActive) return `Your ${phoneName.value} is on Tugboat. The controller opens on it in a moment.`;
  if (p === "connected") return `Open Tugboat on your ${phoneName.value} again: scan the code if the page is closed.`;
  return "Scan with your phone's camera. Tugboat opens on it as a controller.";
});
const anyPhone = computed(() => padConnected[0] || padConnected[1]);
const showCode = computed(() => ts.value.qr !== null && (duo.value ? !(padConnected[0] && padConnected[1]) : !padConnected[0]));

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
const onMotion = (e: MediaQueryListEvent) => {
  reducedMotion.value = e.matches;
  if (!raf) render();
};

onMounted(() => {
  ctx = canvas.value?.getContext("2d") ?? null;
  day = readPalette("day");
  night = readPalette("night");
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion.value = motionQuery.matches;
  motionQuery.addEventListener("change", onMotion);
  // The daily challenge reads the weather card's forecast (refreshed if it's gone stale; nothing
  // is fetched when weather is off or not set up).
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
  window.removeEventListener("focus", onFocus);
  document.removeEventListener("visibilitychange", onVisibility);
  unlistenPad?.();
  if (controllerOn.value) void api.gamePadClose().catch(() => undefined);
});

const MODES: Array<{ id: Mode; label: string; icon: typeof Play }> = [
  { id: "daily", label: "Daily", icon: CalendarDays },
  { id: "free", label: "Free play", icon: Play },
  { id: "duo", label: "Two players", icon: Users },
];
const PLAYER_NAME: Record<Player, string> = { 1: "Player 1", 2: "Player 2" };
const leader = computed(() => raceResult.value?.winner ?? null);
</script>

<template>
  <div ref="root" role="dialog" aria-modal="true" aria-labelledby="run-title" class="fixed inset-0 z-50 flex flex-col bg-surface-dark text-on-dark">
    <header class="flex items-center gap-3 border-b border-surface-dark-elevated px-5 py-3">
      <Gamepad2 :size="20" class="shrink-0 text-primary" />
      <h2 id="run-title" class="font-display text-[24px] leading-none text-on-dark" style="letter-spacing: -0.02em">Tugboat Run</h2>

      <div v-if="!duo" class="ml-4 flex items-center gap-4 font-mono text-[14px]" aria-live="off">
        <span><span class="text-on-dark-soft">Score</span> {{ hud.lanes[0]?.score ?? 0 }}</span>
        <span v-if="mode === 'daily'"><span class="text-on-dark-soft">Today's best</span> {{ Math.max(todaysBest, phase === "ready" ? 0 : (hud.lanes[0]?.score ?? 0)) }}</span>
        <span v-else><span class="text-on-dark-soft">Best</span> {{ Math.max(game.best, hud.lanes[0]?.score ?? 0) }}</span>
        <span class="flex items-center gap-1" :aria-label="`${hud.lanes[0]?.lives ?? LIVES} of ${LIVES} lives`" role="img">
          <LifeBuoy v-for="i in LIVES" :key="i" :size="15" :class="i <= (hud.lanes[0]?.lives ?? LIVES) ? 'text-primary' : 'text-on-dark-soft/30'" />
        </span>
        <span :class="['rounded-full px-2 py-0.5 text-[12px]', hud.lanes[0]?.hopReady ? 'bg-surface-dark-elevated text-on-dark' : 'text-on-dark-soft/50']">Hop</span>
      </div>
      <div v-else class="ml-4 font-mono text-[14px]" aria-live="off">
        <span class="text-on-dark-soft">Best race score</span> {{ Math.max(game.duoBest, ...(phase === "ready" ? [0] : hud.lanes.map((l) => l.score))) }}
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
          :aria-label="duo ? 'Tugboat Run race, two lanes' : 'Tugboat Run playing field'"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointercancel="onPointerUp"
        />

        <!-- A race: each lane's player, score and lives on top of it, and a card once it's out. -->
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
                <LifeBuoy v-for="n in LIVES" :key="n" :size="12" :class="n <= (hud.lanes[i]?.lives ?? 0) ? (i === 0 ? 'text-primary' : 'text-accent-teal') : 'text-on-dark-soft/30'" />
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
              <p class="font-display text-[22px] leading-tight">{{ left[i] ? "Left the race" : "Sunk!" }}</p>
              <p class="font-mono text-[18px]">{{ hud.lanes[i]?.score ?? 0 }}</p>
              <p class="text-[12px] text-on-dark-soft">Waiting for {{ PLAYER_NAME[(2 - i) as Player] }}</p>
            </div>
          </div>
        </template>

        <!-- Ready, paused, over: one card over the still field. -->
        <div v-if="phase !== 'playing'" class="absolute inset-0 flex items-center justify-center overflow-y-auto p-6">
          <div class="w-full max-w-[420px] rounded-2xl bg-surface-dark/90 px-7 py-6 text-center shadow-xl ring-1 ring-surface-dark-elevated">
            <template v-if="phase === 'ready'">
              <p class="font-display text-[30px] leading-tight">Ready to sail?</p>
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
                <p class="mt-4 text-[13px] text-on-dark-soft">Today's course · {{ dayLabel }}</p>
                <p class="mt-1 text-[13px] text-on-dark-soft/80">The same course for everyone today.</p>
                <p class="mt-3 flex items-center justify-center gap-2 rounded-xl bg-surface-dark-elevated px-3 py-2 text-[14px] text-on-dark" role="status">
                  <component :is="conditionsIcon" :size="16" class="shrink-0 text-accent-teal" />
                  {{ conditionsText }}
                </p>
                <p v-if="weather.ready && weather.place === null" class="mt-1.5 text-[12px] text-on-dark-soft/80">Set up the weather card and today's course sails in your real weather.</p>
                <div class="mt-3 flex items-center justify-center gap-4 text-[13px]">
                  <span class="flex items-center gap-1.5" :class="streak > 0 ? 'text-accent-amber' : 'text-on-dark-soft'">
                    <Flame :size="15" />
                    {{ streak === 0 ? "No streak yet" : streak === 1 ? "1 day streak" : `${streak} day streak` }}
                  </span>
                  <span class="text-on-dark-soft">{{ doneToday ? `Today's best: ${todaysBest}` : "Not sailed yet today" }}</span>
                </div>
                <button class="btn-primary mt-5" @click="start">
                  <Play :size="16" />
                  {{ doneToday ? "Sail today's course again" : "Start today's run" }}
                </button>
              </template>

              <template v-else-if="mode === 'free'">
                <p class="mt-4 text-[14px] text-on-dark-soft">
                  Steer with ← → or A and D. Space hops over buoys and logs, but not other boats. Coins are points; a life ring brings back
                  a life.
                </p>
                <p class="mt-1.5 text-[13px] text-on-dark-soft/80">Or drag with the mouse to steer, and click to hop. A new course every time.</p>
                <button class="btn-primary mt-5" @click="start">
                  <Play :size="16" />
                  Start
                </button>
              </template>

              <template v-else>
                <p class="mt-4 text-[14px] text-on-dark-soft">Race side by side on the same course. The higher score wins.</p>
                <ul class="mt-3 space-y-2 text-left">
                  <li v-for="p in [1, 2] as Player[]" :key="p" class="flex items-center gap-3 rounded-xl bg-surface-dark-elevated px-3 py-2">
                    <span :class="['size-3 shrink-0 rounded-full', p === 1 ? 'bg-primary' : 'bg-accent-teal']" />
                    <div class="min-w-0 flex-1">
                      <p class="text-[14px] text-on-dark">{{ PLAYER_NAME[p] }}</p>
                      <p class="text-[12px] text-on-dark-soft">
                        {{ phoneFor(p) ? "Phone connected · or " : "" }}{{ KEYS_LINE[p] }}
                      </p>
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
                  Start race
                </button>
              </template>
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
                  <span v-if="left[p - 1]" class="text-[12px] text-on-dark-soft/80">left the race</span>
                  <span class="ml-auto font-mono text-[20px]" :class="leader === p ? 'text-on-dark' : 'text-on-dark-soft'">{{ raceResult.scores[p - 1] }}</span>
                </li>
              </ul>
              <p class="mt-3 text-[13px] text-on-dark-soft">{{ newBest ? "A new best race score on this PC!" : `Best race score on this PC: ${game.duoBest}` }}</p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="start">
                  <RotateCcw :size="16" />
                  Rematch
                </button>
                <button class="btn-on-dark" @click="menu">Menu</button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
              <p class="mt-3 text-[12px] text-on-dark-soft/80">Enter or Space for a rematch.</p>
            </template>

            <template v-else>
              <p class="font-display text-[30px] leading-tight">{{ newBest ? (mode === "daily" ? "Today's best!" : "New best!") : "Run over" }}</p>
              <p class="mt-2 font-mono text-[28px] text-on-dark">{{ hud.lanes[0]?.score ?? 0 }}</p>
              <template v-if="mode === 'daily'">
                <p class="text-[13px] text-on-dark-soft">Today's best: {{ todaysBest }}</p>
                <p class="mt-2 flex items-center justify-center gap-1.5 text-[14px] text-accent-amber">
                  <Flame :size="15" />
                  {{ streak === 1 ? "1 day streak" : `${streak} day streak` }}{{ streak > 1 ? " — see you tomorrow!" : " — come back tomorrow to keep it going" }}
                </p>
              </template>
              <p v-else class="text-[13px] text-on-dark-soft">{{ newBest ? "Your best score on this PC." : `Best on this PC: ${game.best}` }}</p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="start">
                  <RotateCcw :size="16" />
                  Play again
                </button>
                <button class="btn-on-dark" @click="menu">Menu</button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
              <p class="mt-3 text-[12px] text-on-dark-soft/80">Enter or Space plays again.</p>
            </template>
          </div>
        </div>
      </div>

      <!-- Phones as controllers: Tugboat's code and where things stand. -->
      <aside
        v-if="sheet"
        class="flex w-[320px] shrink-0 flex-col overflow-y-auto border-l border-surface-dark-elevated bg-surface-dark-soft px-6 py-5"
        aria-labelledby="run-controller-title"
      >
        <div class="flex items-center gap-2">
          <h3 id="run-controller-title" class="font-display text-[22px] leading-tight">{{ duo ? "Phone controllers" : "Phone controller" }}</h3>
          <button class="ml-auto rounded-md p-1.5 text-on-dark-soft active:bg-surface-dark-elevated" aria-label="Hide phone controller" @click="sheet = false">
            <X :size="16" />
          </button>
        </div>
        <p class="mt-3 flex items-start gap-2 text-[14px] text-on-dark" role="status">
          <LoaderCircle v-if="controllerOn && ts.phase === 'off' && !controllerError" :size="16" class="mt-0.5 shrink-0 animate-spin text-on-dark-soft" />
          <span v-else :class="['mt-[7px] size-2 shrink-0 rounded-full', anyPhone ? 'bg-accent-teal' : 'bg-on-dark-soft/40']" />
          {{ controllerLine }}
        </p>

        <div v-if="showCode && ts.qr" class="mx-auto mt-5 rounded-xl bg-canvas p-3">
          <svg :viewBox="`-2 -2 ${ts.qr.size + 4} ${ts.qr.size + 4}`" class="block size-[200px]" shape-rendering="crispEdges" role="img" aria-label="QR code to use your phone as a controller">
            <path :d="ts.qr.path" class="fill-ink" />
          </svg>
        </div>
        <p v-if="showCode && ts.qr" class="mt-3 text-center text-[12px] text-on-dark-soft">
          This is Tugboat's code: the same one for photos and files.{{ duo ? " Two phones can join a race; a third is turned away." : "" }}
        </p>
        <div v-else-if="!duo && padConnected[0]" class="mt-5 flex flex-col items-center gap-2 py-4 text-center">
          <span class="flex size-14 items-center justify-center rounded-2xl bg-surface-dark-elevated">
            <Smartphone :size="26" class="text-accent-teal" />
          </span>
          <p class="font-display text-[20px]">{{ capitalised(phoneName) }} connected</p>
        </div>

        <ul v-if="duo" class="mt-5 space-y-2">
          <li v-for="p in [1, 2] as Player[]" :key="p" class="flex items-center gap-3 rounded-xl bg-surface-dark-elevated px-3 py-2">
            <span :class="['size-3 shrink-0 rounded-full', p === 1 ? 'bg-primary' : 'bg-accent-teal']" />
            <span class="flex-1 text-[14px]">{{ PLAYER_NAME[p] }}</span>
            <span class="text-[12px]" :class="phoneFor(p) ? 'text-accent-teal' : 'text-on-dark-soft'">{{ phoneFor(p) ? "Phone connected" : "Keyboard" }}</span>
          </li>
        </ul>
        <button v-if="duo && controllerOn && (phase === 'ready' || phase === 'over')" class="btn-on-dark btn-sm mt-2 self-center" @click="swapped = !swapped">
          <ArrowLeftRight :size="14" />
          Swap phones
        </button>

        <div class="mt-auto space-y-2 pt-5">
          <button v-if="phase === 'paused' && dropped === null" class="btn-primary w-full" :disabled="blocked" @click="resume">
            <Play :size="16" />
            Resume
          </button>
          <button v-if="controllerOn" class="btn-on-dark btn-sm w-full" @click="controllerOff">{{ duo ? "Stop using phones" : "Stop using the phone" }}</button>
          <p class="text-center text-[12px] text-on-dark-soft/80">Encrypted, over your Wi-Fi. The keyboard always works too.</p>
        </div>
      </aside>
    </div>

    <!-- tug's colours for the canvas, read from these (theme tokens only, no colour in code).
         Night swatches replace the day ones they name; the rest are shared. -->
    <div ref="swatches" class="hidden" aria-hidden="true">
      <span data-swatch="day:waterBase" class="bg-canvas" />
      <span data-swatch="day:water" class="bg-accent-teal/20" />
      <span data-swatch="day:waterDeep" class="bg-accent-teal/60" />
      <span data-swatch="day:quay" class="bg-surface-cream-strong" />
      <span data-swatch="day:quayEdge" class="bg-muted-soft" />
      <span data-swatch="day:hull" class="bg-primary" />
      <span data-swatch="day:hull2" class="bg-accent-teal" />
      <span data-swatch="day:deck" class="bg-canvas" />
      <span data-swatch="day:cabin" class="bg-surface-dark" />
      <span data-swatch="day:ink" class="bg-ink" />
      <span data-swatch="day:rope" class="bg-body" />
      <span data-swatch="day:coin" class="bg-accent-amber" />
      <span data-swatch="day:log" class="bg-muted" />
      <span data-swatch="day:logEnd" class="bg-muted-soft" />
      <span data-swatch="day:boat" class="bg-surface-dark-elevated" />
      <span data-swatch="day:boatCabin" class="bg-on-dark-soft" />
      <span data-swatch="day:foam" class="bg-canvas" />
      <span data-swatch="day:spray" class="bg-surface-dark/35" />
      <span data-swatch="day:flake" class="bg-canvas" />
      <span data-swatch="day:tower" class="bg-canvas" />
      <span data-swatch="day:lamp" class="bg-accent-amber" />
      <span data-swatch="day:beam" class="bg-accent-amber/20" />
      <span data-swatch="day:glow" class="bg-accent-amber/25" />
      <span data-swatch="night:waterBase" class="bg-surface-dark" />
      <span data-swatch="night:water" class="bg-accent-teal/15" />
      <span data-swatch="night:waterDeep" class="bg-accent-teal/40" />
      <span data-swatch="night:quay" class="bg-surface-dark-soft" />
      <span data-swatch="night:quayEdge" class="bg-surface-dark-elevated" />
      <span data-swatch="night:ink" class="bg-on-dark-soft/60" />
      <span data-swatch="night:rope" class="bg-on-dark-soft" />
      <span data-swatch="night:log" class="bg-muted-soft" />
      <span data-swatch="night:logEnd" class="bg-muted" />
      <span data-swatch="night:boat" class="bg-on-dark-soft" />
      <span data-swatch="night:boatCabin" class="bg-surface-dark-elevated" />
      <span data-swatch="night:foam" class="bg-on-dark/60" />
      <span data-swatch="night:spray" class="bg-on-dark/45" />
      <span data-swatch="night:flake" class="bg-on-dark/85" />
    </div>
  </div>
</template>
