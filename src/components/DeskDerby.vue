<script setup lang="ts">
// Desk Derby: a top-down RC car race over the whole window, from the games picker (the sidebar's
// Play button, Ctrl+K "game", "desk derby", "race" or "car game", or the taskbar's Play a game).
// Three laps round a track laid out on a cluttered desk; the car drives itself, you steer, and
// holding Drift slides it round corners and charges a boost for when you let go. Three ways to
// play: today's race (the same track for everyone today, against a ghost of your best run today),
// Free play on a track of your choice (against your best on it), and a two-player Head to head on
// one desk. Keyboard or phones as controllers (through Tugboat), like the other two games, whose
// plumbing it shares.
//
// Idle by design: requestAnimationFrame runs only while a race is being played (open, on screen,
// not paused) and draws at most once per display frame. Paused, ready or over, the canvas is drawn
// once and left alone. Losing focus, hiding tug, or a call or confirmation card coming up pauses.
// Closing removes every listener, observer and frame request, and closes the phones' controller.
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { ArrowLeftRight, CalendarDays, Car, Flag, Flame, Ghost as GhostIcon, Pause, Play, RotateCcw, Shuffle, Smartphone, Timer, Trophy, Users, X } from "lucide-vue-next";
import { useGameStore } from "../stores/game";
import { useTugStore } from "../stores/tug";
import { useDevToolsStore } from "../stores/devtools";
import { useFocusTrap } from "../lib/focusTrap";
import { useGameController } from "../lib/useGameController";
import { currentStreak, dailySeed, localDate, sailedToday } from "../lib/tugboatDaily";
import { droppedPlayer, playerFor, seatFor, type Player, type Seat } from "../lib/tugboatRace";
import {
  LAPS,
  PENALTY_S,
  TRACKS,
  advance,
  autopilot,
  betterGhost,
  buildTrack,
  clockOf,
  countdownLabel,
  dailyVariant,
  derbyWinner,
  duoKey,
  formatSplit,
  formatTime,
  freeVariant,
  ghostKey,
  ghostOf,
  ghostPose,
  inputFrom,
  newRace,
  parseGhost,
  places,
  raceMs,
  trackDef,
  type Ghost,
  type Race,
  type TrackId,
  type Variant,
} from "../lib/deskDerby";
import type { GamePad } from "../types/protocol";
import { deskLayout, drawDerby, newFx, updateCamera, updateFx, type DerbyPalette, type Fx } from "./deskDerbyDraw";
import { readSwatches } from "./gamePalette";
import PhoneControllerSheet from "./PhoneControllerSheet.vue";

type Phase = "ready" | "playing" | "paused" | "over";
type Mode = "daily" | "free" | "duo";

const game = useGameStore();
const tug = useTugStore();
const devtools = useDevToolsStore();
const scores = computed(() => game.scores["desk-derby"]);

const root = ref<HTMLElement | null>(null);
const stage = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const swatches = ref<HTMLElement | null>(null);

const phase = ref<Phase>("ready");
const mode = ref<Mode>("daily");
const duo = computed(() => mode.value === "duo");
/** Free play and two players: which track ("random" picks one at the start). */
const choice = ref<TrackId | "random">("inbox");
const newBest = ref(false);

interface CarHud {
  lap: number;
  /** The race clock, hundredths (so the HUD only changes when the digits do). */
  cs: number;
  place: number;
  level: number;
  finished: boolean;
  dnf: boolean;
}
const freshHud = (): CarHud => ({ lap: 1, cs: 0, place: 1, level: 0, finished: false, dnf: false });
const hud = reactive<{ cars: CarHud[]; count: string | null }>({ cars: [freshHud()], count: null });
/** A word over the desk for a moment: a lap time and its split, a fall, the final lap, a new best. */
const callout = ref<{ text: string; split: string | null; ahead: boolean | null; big: boolean; key: number } | null>(null);
let calloutTimer: number | undefined;
let calloutKey = 0;

// The race is a plain object mutated every frame; only the HUD is reactive.
let variant: Variant = freeVariant("inbox");
let race: Race = newRace(buildTrack(variant));
let fx: Fx = newFx();
const carry = { left: 0 };
let overAt = 0;
let endedAt = 0;
let shake = 0;
/** The ghost being raced (today's best, or the best on this track), and the run's own best laps. */
let ghost: Ghost | null = null;
const hasGhost = ref(false);
const result = ref<{ ms: number; laps: number[]; clips: number; falls: number; dnf: boolean } | null>(null);
const raceResult = ref<{ times: [number, number]; winner: Player | null } | null>(null);

// --- Today ---
const today = ref(localDate(new Date()));
let runDate = today.value;
const streak = computed(() => currentStreak(scores.value.daily, today.value));
const doneToday = computed(() => sailedToday(scores.value.daily, today.value));
const todaysBest = computed(() => scores.value.daily.bests[phase.value === "ready" ? today.value : runDate] ?? 0);
const todaysTrack = computed(() => {
  const v = dailyVariant(dailySeed(today.value, "desk-derby"));
  return { name: trackDef(v.track).name, mirror: v.mirror };
});
const dayLabel = computed(() => {
  const [y, m, d] = today.value.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
});
const trackBest = computed(() => (choice.value === "random" ? 0 : (game.trackBests[choice.value] ?? 0)));
const runTrackName = ref(trackDef("inbox").name);
const runTrackBest = ref(0);
/** The track being raced (the header's best follows the store, so a new best shows at once). */
const runTrack = ref<TrackId>("inbox");

// --- Ghosts: kept in this browser's storage (a convenience, like the weather cache), not settings ---
const GHOST_STORE = "tug.deskDerby.ghosts.v1";
interface GhostBook {
  daily: { date: string; ghost: Ghost } | null;
  free: Record<string, Ghost>;
}
function readGhosts(): GhostBook {
  try {
    const j = JSON.parse(localStorage.getItem(GHOST_STORE) ?? "null") as Partial<GhostBook> | null;
    const daily = j?.daily && typeof j.daily.date === "string" ? parseGhost(j.daily.ghost) : null;
    const free: Record<string, Ghost> = {};
    for (const [k, g] of Object.entries(j?.free ?? {})) {
      const v = parseGhost(g);
      if (v && TRACKS.some((t) => t.id === k)) free[k] = v;
    }
    return { daily: daily && j?.daily ? { date: j.daily.date, ghost: daily } : null, free };
  } catch {
    return { daily: null, free: {} };
  }
}
function saveGhost(run: Ghost) {
  const book = readGhosts();
  if (mode.value === "daily") {
    const old = book.daily?.date === runDate ? book.daily.ghost : null;
    const keep = betterGhost(old, run);
    if (keep) book.daily = { date: runDate, ghost: keep };
  } else {
    const keep = betterGhost(book.free[variant.track] ?? null, run);
    if (keep) book.free[variant.track] = keep;
  }
  try {
    localStorage.setItem(GHOST_STORE, JSON.stringify(book));
  } catch {
    /* storage full or off: no ghost next time */
  }
}
function ghostFor(v: Variant): Ghost | null {
  const book = readGhosts();
  const g = mode.value === "daily" ? (book.daily?.date === runDate ? book.daily.ghost : null) : (book.free[v.track] ?? null);
  return g && g.key === ghostKey(v) ? g : null;
}

// --- Controls ---
/** Held keys: Player 1 (or solo) and Player 2. */
const keys = [
  { left: false, right: false, drift: false },
  { left: false, right: false, drift: false },
];
/** Each phone (seat 1, seat 2), as its last `game-pad` event said (read by the loop; not reactive). */
const pads = [
  { connected: false, steer: 0, drift: false },
  { connected: false, steer: 0, drift: false },
];
const padWas = [false, false];

// --- Head to head ---
const swapped = ref(false);
const phoneDrove: [boolean, boolean] = [false, false];
const left = reactive([false, false]);
const dropped = ref<Player | null>(null);

// --- Phone controller (through Tugboat, shared with the other games) ---
const sheet = ref(false);
const players = computed<1 | 2>(() => (duo.value ? 2 : 1));
const controller = useGameController({
  game: "desk-derby",
  players: () => players.value,
  onPad,
  // What each phone shows: paused, its knocks (a buzz on each fall or bump), and its player.
  feedback: (seat) => {
    if (seat === 2 && !duo.value) return null;
    const player = duo.value ? playerFor(seat, swapped.value) : null;
    const c = race.cars[player === null ? 0 : player - 1];
    return { paused: phase.value !== "playing", hits: c ? c.falls + c.bumps : 0, player };
  },
});
const { controllerOn, controllerError, padConnected } = controller;

const blocked = computed(
  () => tug.ringing !== null || devtools.confirm !== null || tug.pairingRequest !== null || tug.quickTextOpen,
);

useFocusTrap(root, onEscape);

function onEscape() {
  if (sheet.value) sheet.value = false;
  else if (phase.value === "playing") pause();
  else close();
}

// --- Phases ---
function pickVariant(): Variant {
  if (mode.value === "daily") return dailyVariant(dailySeed(runDate, "desk-derby"));
  const id = choice.value === "random" ? TRACKS[Math.floor(Math.random() * TRACKS.length)].id : choice.value;
  return freeVariant(id);
}

function start() {
  today.value = localDate(new Date());
  runDate = today.value;
  variant = pickVariant();
  race = newRace(buildTrack(variant), duo.value ? 2 : 1);
  fx = newFx();
  carry.left = 0;
  ghost = duo.value ? null : ghostFor(variant);
  hasGhost.value = ghost !== null;
  runTrackName.value = race.track.name;
  runTrackBest.value = game.trackBests[variant.track] ?? 0;
  runTrack.value = variant.track;
  hud.cars = race.cars.map(freshHud);
  hud.count = countdownLabel(race.t);
  newBest.value = false;
  result.value = null;
  raceResult.value = null;
  callout.value = null;
  endedAt = 0;
  shake = 0;
  left[0] = left[1] = false;
  dropped.value = null;
  for (const p of [1, 2] as Player[]) phoneDrove[p - 1] = padConnected[seatFor(p, swapped.value) - 1];
  phase.value = "playing";
}

/** One car racing, with motion allowed: the camera follows it, zoomed in. Otherwise the whole desk. */
function following(): boolean {
  return !duo.value && !reducedMotion.value && !race.over;
}

/** A still scene behind the menu: the track the mode would race, the cars on the grid. */
function preview() {
  runDate = today.value;
  variant = mode.value === "daily" ? pickVariant() : freeVariant(choice.value === "random" ? "inbox" : choice.value);
  race = newRace(buildTrack(variant), duo.value ? 2 : 1);
  fx = newFx();
  ghost = null;
  hud.cars = race.cars.map(freshHud);
  hud.count = null;
  updateCamera(fx, race, 0, false);
  render();
}

function pause() {
  if (phase.value === "playing") phase.value = "paused";
  // Keys let go while paused (under a call card, say) would otherwise stay held on resume.
  releaseKeys();
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

/** Space / Drift when not racing: start, resume or race again (not the instant a race ends). */
function primary() {
  if (phase.value === "ready") start();
  else if (phase.value === "paused") resume();
  else if (phase.value === "over" && performance.now() - overAt > 700) start();
}

function chooseMode(m: Mode) {
  if (phase.value === "playing" || phase.value === "paused") return;
  mode.value = m;
}

function chooseTrack(t: TrackId | "random") {
  choice.value = t;
}

function menu() {
  phase.value = "ready";
}

function continueSolo() {
  const p = dropped.value;
  if (p === null) return;
  left[p - 1] = true;
  race.cars[p - 1].dnf = true;
  dropped.value = null;
  syncHud();
  resume();
}

function close() {
  game.close();
}

function allGames() {
  void game.show("picker");
}

// --- The loop ---
let raf = 0;
let lastTs = 0;

/** The preview's &bot: the test driver takes the wheel (every car), for checking tracks by eye. */
const devBot = import.meta.env.DEV && new URLSearchParams(location.search).has("bot");
// The preview exposes the race so a browser script can wait for a moment worth a screenshot.
if (import.meta.env.DEV) (window as unknown as { deskDerby?: () => Race }).deskDerby = () => race;

function inputs() {
  return race.cars.map((c, i) => {
    if (devBot) return autopilot(race, c, { line: i === 0 ? 0 : 3 });
    const seat = duo.value ? seatFor((i + 1) as Player, swapped.value) : 1;
    return inputFrom({ ...keys[i], pad: pads[seat - 1] });
  });
}

function frame(now: number) {
  raf = 0;
  const dt = lastTs ? Math.min(0.1, (now - lastTs) / 1000) : 1 / 60;
  lastTs = now;
  race.events = [];
  for (const c of race.cars) c.events = [];
  advance(race, inputs(), dt, carry);
  updateFx(fx, race, dt, reducedMotion.value);
  updateCamera(fx, race, dt, following());
  shake = Math.max(0, shake - dt);
  let knocked = false;
  race.cars.forEach((c, i) => {
    for (const e of c.events) {
      if (e === "bump" || e === "fall") knocked = true;
      if ((e === "bump" || e === "land") && !reducedMotion.value) shake = Math.max(shake, e === "bump" ? 0.18 : 0.12);
    }
    const ev = new Set(c.events);
    const who = duo.value ? `${PLAYER_NAME[(i + 1) as Player]}: ` : "";
    if (ev.has("finish")) onFinish(i);
    else if (ev.has("lap") || ev.has("final-lap")) lapCallout(i, ev.has("final-lap"));
    else if (ev.has("fall")) say(`${who}Off the desk! +${PENALTY_S.toFixed(0)} s`, null, false, false);
  });
  if (knocked) sendFeedback();
  render();
  syncHud();
  if (race.over) {
    // Kept the moment it ends (the best in the header turns at once, and Close during the pause
    // before the results can't lose the time); the results follow a moment later.
    if (endedAt === 0) {
      endedAt = now;
      recordResult();
    }
    // A moment to see the finish before the results.
    if (now - endedAt > 1100) {
      finishGame();
      return;
    }
  }
  if (phase.value === "playing") raf = requestAnimationFrame(frame);
}

/** A lap done: its time, and how it stands against the ghost's run (or the best lap so far). */
function lapCallout(i: number, final: boolean) {
  const c = race.cars[i];
  const lapMs = Math.round(c.laps[c.laps.length - 1] * 1000);
  const n = c.laps.length;
  let split: number | null = null;
  if (!duo.value && ghost && ghost.laps.length >= n) {
    const mine = Math.round(c.laps.reduce((a, b) => a + b, 0) * 1000);
    split = mine - ghost.laps.slice(0, n).reduce((a, b) => a + b, 0);
  } else if (n > 1) {
    split = lapMs - Math.round(Math.min(...c.laps.slice(0, -1)) * 1000);
  }
  const who = duo.value ? `${PLAYER_NAME[(i + 1) as Player]} · ` : "";
  say(`${who}Lap ${n} ${formatTime(lapMs)}${final ? " · Final lap!" : ""}`, split === null ? null : formatSplit(split), split === null || split === 0 ? null : split < 0, false);
}

function onFinish(i: number) {
  const c = race.cars[i];
  const ms = raceMs(c);
  if (duo.value) {
    const first = race.cars.filter((o) => o.finished !== null).length === 1;
    say(first ? `${PLAYER_NAME[(i + 1) as Player]} wins! ${formatTime(ms)}` : `${PLAYER_NAME[(i + 1) as Player]} ${formatTime(ms)}`, null, false, true);
    return;
  }
  const best = mode.value === "daily" ? todaysBest.value : runTrackBest.value;
  const isBest = best <= 0 || ms < best;
  const diff = ghost ? ms - ghost.ms : best > 0 ? ms - best : null;
  say(isBest ? `NEW BEST ${formatTime(ms)}` : `Finished ${formatTime(ms)}`, diff === null ? null : formatSplit(diff), diff === null || diff === 0 ? null : diff < 0, true);
}

function say(text: string, split: string | null, ahead: boolean | null, big: boolean) {
  callout.value = { text, split, ahead, big, key: ++calloutKey };
  window.clearTimeout(calloutTimer);
  calloutTimer = window.setTimeout(() => (callout.value = null), big ? 2600 : 1800);
}

/** The finished race's times into the scores (and its ghost), once, as soon as it ends. */
function recordResult() {
  if (duo.value) {
    const times: [number, number] = [raceMs(race.cars[0]), raceMs(race.cars[1])];
    raceResult.value = { times, winner: derbyWinner(times) };
    // A race one player left was one car driving alone: not a head to head to keep.
    newBest.value = left[0] || left[1] ? false : game.finishDuo(times, "desk-derby", variant.track);
  } else {
    const c = race.cars[0];
    const ms = raceMs(c);
    result.value = { ms, laps: c.laps.map((l) => Math.round(l * 1000)), clips: c.clips, falls: c.falls, dnf: ms === 0 };
    if (ms > 0) {
      newBest.value = mode.value === "daily" ? game.finishDay(runDate, ms, "desk-derby") : game.finish(ms, "desk-derby", variant.track);
      const run = ghostOf(race);
      if (run) saveGhost(run);
    }
  }
}

function finishGame() {
  overAt = performance.now();
  // The whole desk behind the results.
  updateCamera(fx, race, 0, false);
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
  const order = places(race);
  race.cars.forEach((c, i) => {
    const h = hud.cars[i];
    if (!h) return;
    const lap = Math.min(LAPS, c.lap + 1);
    // A finished car shows its recorded time (rounded, as the results show it), not the running
    // clock cut down, so the header and the results never differ by a hundredth.
    const cs = c.finished !== null && !c.dnf ? Math.round(raceMs(c) / 10) : Math.floor(clockOf(race, c) * 100);
    if (h.lap !== lap) h.lap = lap;
    if (h.cs !== cs) h.cs = cs;
    if (h.place !== order[i]) h.place = order[i];
    if (h.level !== c.level) h.level = c.level;
    if (h.finished !== (c.finished !== null)) h.finished = c.finished !== null;
    if (h.dnf !== c.dnf) h.dnf = c.dnf;
  });
  const count = countdownLabel(race.t);
  if (hud.count !== count) hud.count = count;
}

watch(phase, (p) => {
  if (p === "playing") startLoop();
  else {
    stopLoop();
    if (p === "ready") preview();
    else render();
  }
  sendFeedback();
});

watch([mode, choice], () => {
  if (phase.value === "ready") preview();
});

watch(mode, () => {
  if (controllerOn.value) void controller.open();
});

watch(blocked, (b) => {
  if (b) pause();
});

watch(swapped, () => sendFeedback());

// --- Drawing ---
let ctx: CanvasRenderingContext2D | null = null;
let size = { width: 1, height: 1, dpr: 1 };
let day: DerbyPalette | null = null;
let night: DerbyPalette | null = null;
const reducedMotion = ref(false);
/** The desk lamp is on in the evening (or for the preview's &night / &day). */
const lampOn = ref(false);
function refreshLamp() {
  const h = new Date().getHours();
  let on = h >= 20 || h < 6;
  if (import.meta.env.DEV) {
    const q = new URLSearchParams(location.search);
    if (q.has("night")) on = true;
    if (q.has("day")) on = false;
  }
  lampOn.value = on;
}
/** Where the cars' labels sit over the canvas in a two-player race (CSS pixels). */
const deskBox = ref({ left: 0, top: 0, width: 0, height: 0 });

function resize() {
  const el = stage.value;
  const c = canvas.value;
  if (!el || !c) return;
  const r = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  size = { width: Math.max(1, r.width), height: Math.max(1, r.height), dpr };
  c.width = Math.max(1, Math.round(size.width * dpr));
  c.height = Math.max(1, Math.round(size.height * dpr));
  const l = deskLayout(size.width, size.height);
  deskBox.value = { left: l.left, top: l.top, width: l.scale * 360, height: l.scale * 225 };
}

function render() {
  if (!ctx || !day || !night) return;
  drawDerby(ctx, {
    ...size,
    reducedMotion: reducedMotion.value,
    night: lampOn.value,
    palette: lampOn.value ? night : day,
    race,
    fx,
    ghost: ghost && phase.value !== "ready" ? ghostPose(ghost, race.t) : null,
    shake: reducedMotion.value ? 0 : shake,
    showNext: !duo.value && phase.value !== "ready",
  });
}

const PALETTE_NAMES = [
  "floor",
  "desk",
  "grain",
  "deskEdge",
  "shadow",
  "lane",
  "laneLine",
  "tape",
  "curb",
  "paper",
  "paperLine",
  "keyboard",
  "keycap",
  "keyTop",
  "mug",
  "mugRim",
  "coffee",
  "coffeeTint",
  "pencil",
  "wood",
  "lead",
  "eraser",
  "ferrule",
  "pen",
  "penCap",
  "mouse",
  "mouseLine",
  "cable",
  "stapler",
  "staplerTop",
  "noteA",
  "noteB",
  "noteC",
  "noteD",
  "clip",
  "clipShine",
  "car1",
  "car2",
  "carDark",
  "outline",
  "glass",
  "stripe",
  "wheel",
  "skid",
  "scuff",
  "spark1",
  "spark2",
  "spark3",
  "flame",
  "flameCore",
  "dust",
  "lamp",
  "checkA",
  "checkB",
  "headlight",
] as const;

// --- Input handlers ---
const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && t.closest("input, textarea, [contenteditable]") !== null;

function onKey(e: KeyboardEvent, down: boolean) {
  if (e.ctrlKey || e.metaKey || e.altKey || blocked.value || isTyping(e.target)) {
    // A key let go while keys don't reach the game (Ctrl held, a card up, typing) is still let go.
    if (!down) releaseKeys();
    return;
  }
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  // Enter or Space on a focused button is the button's.
  if ((k === "Enter" || k === " ") && e.target instanceof HTMLButtonElement && phase.value !== "playing") return;
  const racing = phase.value === "playing";
  const pair = duo.value ? duoKey(e.key, e.code) : null;
  if (pair) {
    const held = keys[pair.player - 1];
    if (pair.action === "left") held.left = down;
    else if (pair.action === "right") held.right = down;
    else held.drift = down;
  } else if (k === " " || k === "Enter") {
    if (down && !e.repeat && !racing) primary();
    else if (!duo.value && k === " ") keys[0].drift = down && racing;
  } else if (!duo.value && (k === "ArrowLeft" || k === "a")) {
    keys[0].left = down;
  } else if (!duo.value && (k === "ArrowRight" || k === "d")) {
    keys[0].right = down;
  } else if (!duo.value && (k === "Shift" || k === "ArrowUp" || k === "w")) {
    keys[0].drift = down;
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

function releaseKeys() {
  for (const k of keys) k.left = k.right = k.drift = false;
}
function onBlur() {
  pause();
  releaseKeys();
}
function onFocus() {
  if (phase.value === "ready" || phase.value === "over") today.value = localDate(new Date());
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
  // Desk Derby's Drift travels as the phone's held button (`boost` on the wire).
  pad.drift = e.connected && e.boost;
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
      out: [race.cars[0]?.finished !== null || (race.cars[0]?.dnf ?? true), race.cars[1]?.finished !== null || (race.cars[1]?.dnf ?? true)],
      phoneDrove,
    });
    if (p !== null) {
      dropped.value = p;
      pause();
    }
  }
  // From a phone alone: a fresh press of Drift starts, resumes or races again.
  const fresh = pad.drift && !padWas[i];
  padWas[i] = pad.drift;
  if (fresh && phase.value !== "playing" && !blocked.value) {
    sheet.value = false;
    primary();
  }
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
const KEYS_LINE: Record<Player, string> = { 1: "← → steer, ↑ or right Shift drifts", 2: "A D steer, W or left Shift drifts" };

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
  if (e.matches) updateCamera(fx, race, 0, false);
  if (!raf) render();
};

onMounted(() => {
  ctx = canvas.value?.getContext("2d") ?? null;
  day = readSwatches(swatches.value, "day", PALETTE_NAMES);
  night = readSwatches(swatches.value, "night", PALETTE_NAMES);
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion.value = motionQuery.matches;
  motionQuery.addEventListener("change", onMotion);
  refreshLamp();
  if (import.meta.env.DEV) {
    // The preview's &track=coffee: Free play on that track.
    const t = new URLSearchParams(location.search).get("track");
    if (t && TRACKS.some((d) => d.id === t)) {
      mode.value = "free";
      choice.value = t as TrackId;
    }
  }
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
  { id: "duo", label: "Head to head", icon: Users },
];
const PLAYER_NAME: Record<Player, string> = { 1: "Player 1", 2: "Player 2" };
const PLACE = ["", "1st", "2nd"];
const leader = computed(() => raceResult.value?.winner ?? null);
/** The fastest head to head on this race's track (tracks differ in length). */
const duoBest = computed(() => game.duoTrackBests[runTrack.value] ?? 0);
const clock = (cs: number) => (cs > 0 ? formatTime(cs * 10) : "0:00.00");
const lapText = (h: CarHud | undefined) => (h ? (h.finished ? "Finished" : h.dnf ? "Out" : `Lap ${h.lap}/${LAPS}`) : "");
</script>

<template>
  <div ref="root" role="dialog" aria-modal="true" aria-labelledby="derby-title" class="fixed inset-0 z-50 flex flex-col bg-surface-dark text-on-dark">
    <header class="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-surface-dark-elevated px-5 py-3">
      <Car :size="20" class="shrink-0 text-primary" />
      <h2 id="derby-title" class="font-display text-[24px] leading-none text-on-dark" style="letter-spacing: -0.02em">Desk Derby</h2>

      <div v-if="!duo && phase !== 'ready'" class="ml-2 flex items-center gap-4 font-mono text-[14px]" aria-live="off">
        <span class="flex items-center gap-1.5"><Flag :size="14" class="text-on-dark-soft" /> {{ lapText(hud.cars[0]) }}</span>
        <span class="flex items-center gap-1.5 tabular-nums"><Timer :size="14" class="text-on-dark-soft" /> {{ clock(hud.cars[0]?.cs ?? 0) }}</span>
        <span v-if="mode === 'daily'"><span class="text-on-dark-soft">Today's best</span> {{ formatTime(todaysBest) }}</span>
        <span v-else><span class="text-on-dark-soft">Best</span> {{ formatTime(game.trackBests[runTrack] ?? 0) }}</span>
        <!-- The drift's boost charge: three pips in the sparks' colours. -->
        <span class="flex items-center gap-1" role="img" :aria-label="`Boost charge ${hud.cars[0]?.level ?? 0} of 3`">
          <span v-for="n in 3" :key="n" :class="['size-2 rounded-full', (hud.cars[0]?.level ?? 0) >= n ? (n === 1 ? 'bg-accent-teal' : n === 2 ? 'bg-accent-amber' : 'bg-primary') : 'bg-on-dark-soft/25']" />
        </span>
        <span v-if="hasGhost" class="flex items-center gap-1 text-[12px] text-on-dark-soft"><GhostIcon :size="13" /> Ghost</span>
      </div>
      <div v-else-if="duo" class="ml-2 font-mono text-[14px]" aria-live="off">
        <span class="text-on-dark-soft">Best head to head</span> {{ formatTime(duoBest) }}
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
        <button class="btn-on-dark btn-sm w-8 px-0" aria-label="Close Desk Derby" @click="close">
          <X :size="15" />
        </button>
      </div>
    </header>

    <div class="relative flex min-h-0 flex-1">
      <div ref="stage" tabindex="-1" class="relative min-w-0 flex-1 outline-none">
        <canvas ref="canvas" class="absolute inset-0 block size-full touch-none" :aria-label="duo ? 'Desk Derby head to head, two cars on one desk' : 'Desk Derby: the desk, the track and your car'" />

        <!-- Head to head: each car's lap, place and clock in the desk's top corners. -->
        <template v-if="duo && phase !== 'ready'">
          <div
            v-for="(h, i) in hud.cars"
            :key="i"
            class="pointer-events-none absolute flex items-center gap-2.5 rounded-full bg-surface-dark/85 px-3 py-1 font-mono text-[13px] ring-1 ring-surface-dark-elevated"
            :style="i === 0 ? { left: `${deskBox.left + 10}px`, top: `${deskBox.top + 10}px` } : { right: `${deskBox.left + 10}px`, top: `${deskBox.top + 10}px` }"
          >
            <span :class="['size-2.5 rounded-full', i === 0 ? 'bg-primary' : 'bg-accent-teal']" />
            <span class="font-sans text-on-dark-soft">{{ PLAYER_NAME[(i + 1) as Player] }}</span>
            <span>{{ lapText(h) }}</span>
            <span class="tabular-nums">{{ h.dnf ? "–" : clock(h.cs) }}</span>
            <span v-if="!h.dnf" :class="h.place === 1 ? 'text-accent-amber' : 'text-on-dark-soft'">{{ PLACE[h.place] }}</span>
          </div>
        </template>

        <!-- 3, 2, 1, GO. -->
        <div v-if="hud.count && phase === 'playing'" class="pointer-events-none absolute inset-0 flex items-center justify-center">
          <p
            :key="hud.count"
            :class="[
              'flex min-w-[148px] items-center justify-center rounded-[28px] px-9 py-4 font-sans leading-none font-bold tabular-nums shadow-2xl ring-4',
              hud.count === 'GO' ? 'bg-primary text-[96px] text-on-primary ring-on-primary/80' : 'bg-surface-dark text-[96px] text-on-dark ring-primary',
            ]"
            style="letter-spacing: -0.03em"
            role="status"
          >
            {{ hud.count === "GO" ? "GO!" : hud.count }}
          </p>
        </div>

        <!-- A word for a moment: a lap time and its split, a fall, the finish. -->
        <div v-if="callout && phase === 'playing'" :key="callout.key" class="pointer-events-none absolute inset-x-0 top-[14%] flex justify-center">
          <p
            :class="[
              'animate-step-in flex items-center gap-3 rounded-full bg-surface-dark/85 px-4 py-1.5 text-center ring-1 ring-surface-dark-elevated',
              callout.big ? 'font-display text-[24px] text-accent-amber' : 'text-[15px] font-medium text-on-dark',
            ]"
            :role="callout.big ? 'status' : undefined"
          >
            {{ callout.text }}
            <span v-if="callout.split" :class="['font-mono text-[14px]', callout.ahead === null ? 'text-on-dark-soft' : callout.ahead ? 'text-success' : 'text-error']">{{ callout.split }}</span>
          </p>
        </div>

        <!-- Ready, paused, over: one card over the still desk. -->
        <div v-if="phase !== 'playing'" class="absolute inset-0 flex overflow-y-auto p-6">
          <div class="m-auto w-full max-w-[440px] rounded-2xl bg-surface-dark/90 px-7 py-6 text-center shadow-xl ring-1 ring-surface-dark-elevated">
            <template v-if="phase === 'ready'">
              <p class="font-display text-[30px] leading-tight">Ready to race?</p>
              <div class="mt-4 grid grid-cols-3 gap-1 rounded-xl bg-surface-dark-elevated p-1" role="tablist" aria-label="How to play">
                <button
                  v-for="m in MODES"
                  :key="m.id"
                  role="tab"
                  :aria-selected="mode === m.id"
                  :class="['flex items-center justify-center gap-1.5 rounded-lg px-2 py-1.5 text-[13px]', mode === m.id ? 'bg-surface-dark text-on-dark shadow-sm' : 'text-on-dark-soft']"
                  @click="chooseMode(m.id)"
                >
                  <component :is="m.icon" :size="14" />
                  {{ m.label }}
                </button>
              </div>

              <template v-if="mode === 'daily'">
                <p class="mt-4 text-[13px] text-on-dark-soft">Today's race · {{ dayLabel }}</p>
                <p class="mt-1 text-[15px] text-on-dark">{{ todaysTrack.name }}{{ todaysTrack.mirror ? " (mirrored)" : "" }}</p>
                <p class="mt-1 text-[13px] text-on-dark-soft/80">The same desk for everyone today. Race your ghost: your best run today.</p>
                <div class="mt-3 flex items-center justify-center gap-4 text-[13px]">
                  <span class="flex items-center gap-1.5" :class="streak > 0 ? 'text-accent-amber' : 'text-on-dark-soft'">
                    <Flame :size="15" />
                    {{ streak === 0 ? "No streak yet" : streak === 1 ? "1 day streak" : `${streak} day streak` }}
                  </span>
                  <span class="text-on-dark-soft">{{ doneToday ? `Today's best: ${formatTime(todaysBest)}` : "Not raced yet today" }}</span>
                </div>
                <button class="btn-primary mt-5" @click="start">
                  <Play :size="16" />
                  {{ doneToday ? "Race today's again" : "Start today's race" }}
                </button>
              </template>

              <template v-else>
                <div class="mt-4 grid grid-cols-2 gap-1.5" role="radiogroup" aria-label="Track">
                  <button
                    v-for="t in TRACKS"
                    :key="t.id"
                    role="radio"
                    :aria-checked="choice === t.id"
                    :class="[
                      'rounded-xl px-3 py-2 text-left ring-1 transition-colors',
                      choice === t.id ? 'bg-surface-dark-elevated ring-primary' : 'bg-surface-dark-elevated/50 ring-transparent hover:ring-on-dark-soft/30',
                    ]"
                    @click="chooseTrack(t.id)"
                  >
                    <span class="block text-[14px] text-on-dark">{{ t.name }}</span>
                    <span class="block font-mono text-[12px] text-on-dark-soft">{{ game.trackBests[t.id] ? `Best ${formatTime(game.trackBests[t.id])}` : "No time yet" }}</span>
                  </button>
                </div>
                <button
                  :class="['mt-1.5 flex w-full items-center justify-center gap-1.5 rounded-xl px-3 py-1.5 text-[13px] ring-1', choice === 'random' ? 'bg-surface-dark-elevated text-on-dark ring-primary' : 'text-on-dark-soft ring-transparent hover:ring-on-dark-soft/30']"
                  role="radio"
                  :aria-checked="choice === 'random'"
                  @click="chooseTrack('random')"
                >
                  <Shuffle :size="13" />
                  Surprise me
                </button>
                <p v-if="choice !== 'random'" class="mt-3 text-[13px] text-on-dark-soft">{{ trackDef(choice).blurb }}</p>

                <template v-if="mode === 'free'">
                  <p class="mt-3 text-[14px] text-on-dark-soft">
                    ← → steer. Hold Space or Shift to drift round a corner, and let go for a boost: the longer the drift, the bigger the boost.
                  </p>
                  <p class="mt-1.5 text-[13px] text-on-dark-soft/80">Three laps. Off the edge of the desk costs {{ PENALTY_S }} seconds. {{ trackBest ? "Your best run rides along as a ghost." : "" }}</p>
                  <button class="btn-primary mt-5" @click="start">
                    <Play :size="16" />
                    Start
                  </button>
                </template>

                <template v-else>
                  <p class="mt-3 text-[14px] text-on-dark-soft">Two cars, one desk, three laps. First across the line wins.</p>
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
                    Start the race
                  </button>
                </template>
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
                {{ leader ? `${PLAYER_NAME[leader]} wins!` : "No winner this time" }}
              </p>
              <ul class="mx-auto mt-4 max-w-[280px] space-y-1.5">
                <li v-for="p in [1, 2] as Player[]" :key="p" class="flex items-center gap-2.5 text-[15px]">
                  <span :class="['size-2.5 rounded-full', p === 1 ? 'bg-primary' : 'bg-accent-teal']" />
                  <span class="text-on-dark-soft">{{ PLAYER_NAME[p] }}</span>
                  <span v-if="left[p - 1]" class="text-[12px] text-on-dark-soft/80">left</span>
                  <span class="ml-auto font-mono text-[20px]" :class="leader === p ? 'text-on-dark' : 'text-on-dark-soft'">{{ raceResult.times[p - 1] ? formatTime(raceResult.times[p - 1]) : "Didn't finish" }}</span>
                </li>
              </ul>
              <p class="mt-3 text-[13px] text-on-dark-soft">{{ newBest ? `The fastest head to head on ${runTrackName} on this PC!` : `Fastest head to head on ${runTrackName} on this PC: ${formatTime(duoBest)}` }}</p>
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

            <template v-else-if="result">
              <p class="font-display text-[30px] leading-tight" :class="newBest ? 'text-accent-amber' : ''">
                {{ result.dnf ? "Out of time" : newBest ? (mode === "daily" ? "Today's best!" : "New best!") : "Finished" }}
              </p>
              <p class="mt-1 text-[13px] text-on-dark-soft">{{ runTrackName }}</p>
              <p class="mt-2 font-mono text-[34px] text-on-dark tabular-nums">{{ formatTime(result.ms) }}</p>
              <ul v-if="result.laps.length" class="mx-auto mt-2 flex max-w-[320px] justify-center gap-4 font-mono text-[13px] text-on-dark-soft">
                <li v-for="(l, i) in result.laps" :key="i">
                  <span class="text-on-dark-soft/70">L{{ i + 1 }}</span> {{ formatTime(l) }}
                </li>
              </ul>
              <p class="mt-2 text-[13px] text-on-dark-soft">
                {{ result.clips }} paper {{ result.clips === 1 ? "clip" : "clips" }} · {{ result.falls === 0 ? "stayed on the desk" : `${result.falls} ${result.falls === 1 ? "fall" : "falls"} (+${result.falls * PENALTY_S} s)` }}
              </p>
              <template v-if="mode === 'daily'">
                <p class="mt-1 text-[13px] text-on-dark-soft">Today's best: {{ formatTime(todaysBest) }}</p>
                <p v-if="!result.dnf" class="mt-2 flex items-center justify-center gap-1.5 text-[14px] text-accent-amber">
                  <Flame :size="15" />
                  {{ streak === 1 ? "1 day streak" : `${streak} day streak` }}{{ streak > 1 ? " — see you tomorrow!" : " — come back tomorrow to keep it going" }}
                </p>
              </template>
              <p v-else class="mt-1 text-[13px] text-on-dark-soft">
                {{ newBest ? "Your fastest on this track. Your ghost will race you next time." : `Best on this track: ${formatTime(game.trackBests[variant.track] ?? 0)}` }}
              </p>
              <div class="mt-5 flex justify-center gap-2">
                <button class="btn-primary" @click="start">
                  <RotateCcw :size="16" />
                  Race again
                </button>
                <button class="btn-on-dark" @click="menu">Menu</button>
                <button class="btn-on-dark" @click="close">Close</button>
              </div>
              <p class="mt-3 text-[12px] text-on-dark-soft/80">Space races again.</p>
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
        solo-hint="Steer with the slider; hold Drift round corners and let go for a boost."
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
         (the desk lamp on) replace the day ones they name; the rest are shared. -->
    <div ref="swatches" class="hidden" aria-hidden="true">
      <span data-swatch="day:floor" class="bg-surface-dark" />
      <span data-swatch="day:desk" class="bg-surface-cream-strong" />
      <span data-swatch="day:grain" class="bg-accent-amber/30" />
      <span data-swatch="day:deskEdge" class="bg-accent-amber/50" />
      <span data-swatch="day:shadow" class="bg-surface-dark/25" />
      <span data-swatch="day:lane" class="bg-surface-dark-elevated" />
      <span data-swatch="day:laneLine" class="bg-on-dark/30" />
      <span data-swatch="day:tape" class="bg-canvas" />
      <span data-swatch="day:curb" class="bg-primary" />
      <span data-swatch="day:paper" class="bg-canvas" />
      <span data-swatch="day:paperLine" class="bg-accent-teal/30" />
      <span data-swatch="day:keyboard" class="bg-body" />
      <span data-swatch="day:keycap" class="bg-ink" />
      <span data-swatch="day:keyTop" class="bg-surface-dark-elevated" />
      <span data-swatch="day:mug" class="bg-canvas" />
      <span data-swatch="day:mugRim" class="bg-muted-soft" />
      <span data-swatch="day:coffee" class="bg-ink" />
      <span data-swatch="day:coffeeTint" class="bg-accent-amber/35" />
      <span data-swatch="day:pencil" class="bg-warning" />
      <span data-swatch="day:wood" class="bg-surface-cream-strong" />
      <span data-swatch="day:lead" class="bg-ink" />
      <span data-swatch="day:eraser" class="bg-primary/80" />
      <span data-swatch="day:ferrule" class="bg-muted-soft" />
      <span data-swatch="day:pen" class="bg-accent-teal" />
      <span data-swatch="day:penCap" class="bg-surface-dark-elevated" />
      <span data-swatch="day:mouse" class="bg-canvas" />
      <span data-swatch="day:mouseLine" class="bg-muted-soft" />
      <span data-swatch="day:cable" class="bg-muted" />
      <span data-swatch="day:stapler" class="bg-surface-dark-elevated" />
      <span data-swatch="day:staplerTop" class="bg-muted" />
      <span data-swatch="day:noteA" class="bg-accent-amber" />
      <span data-swatch="day:noteB" class="bg-accent-teal" />
      <span data-swatch="day:noteC" class="bg-success" />
      <span data-swatch="day:noteD" class="bg-warning" />
      <span data-swatch="day:clip" class="bg-muted" />
      <span data-swatch="day:clipShine" class="bg-canvas" />
      <span data-swatch="day:car1" class="bg-primary" />
      <span data-swatch="day:car2" class="bg-accent-teal" />
      <span data-swatch="day:carDark" class="bg-ink" />
      <span data-swatch="day:outline" class="bg-canvas" />
      <span data-swatch="day:glass" class="bg-surface-dark/80" />
      <span data-swatch="day:stripe" class="bg-canvas/80" />
      <span data-swatch="day:wheel" class="bg-ink" />
      <span data-swatch="day:skid" class="bg-ink/35" />
      <span data-swatch="day:scuff" class="bg-on-dark/30" />
      <span data-swatch="day:spark1" class="bg-accent-teal" />
      <span data-swatch="day:spark2" class="bg-accent-amber" />
      <span data-swatch="day:spark3" class="bg-primary" />
      <span data-swatch="day:flame" class="bg-accent-amber" />
      <span data-swatch="day:flameCore" class="bg-canvas" />
      <span data-swatch="day:dust" class="bg-muted/50" />
      <span data-swatch="day:lamp" class="bg-accent-amber/0" />
      <span data-swatch="day:checkA" class="bg-ink" />
      <span data-swatch="day:checkB" class="bg-canvas" />
      <span data-swatch="day:headlight" class="bg-accent-amber/35" />
      <span data-swatch="night:floor" class="bg-ink" />
      <span data-swatch="night:desk" class="bg-surface-dark-soft" />
      <span data-swatch="night:grain" class="bg-accent-amber/15" />
      <span data-swatch="night:deskEdge" class="bg-accent-amber/25" />
      <span data-swatch="night:shadow" class="bg-ink/60" />
      <span data-swatch="night:lane" class="bg-ink" />
      <span data-swatch="night:laneLine" class="bg-on-dark/20" />
      <span data-swatch="night:tape" class="bg-on-dark/75" />
      <span data-swatch="night:paper" class="bg-surface-cream-strong/80" />
      <span data-swatch="night:keyboard" class="bg-surface-dark-elevated" />
      <span data-swatch="night:keycap" class="bg-ink" />
      <span data-swatch="night:keyTop" class="bg-body" />
      <span data-swatch="night:mug" class="bg-surface-cream-strong" />
      <span data-swatch="night:mouse" class="bg-surface-cream-strong" />
      <span data-swatch="night:skid" class="bg-on-dark/20" />
      <span data-swatch="night:scuff" class="bg-on-dark/25" />
      <span data-swatch="night:dust" class="bg-on-dark-soft/40" />
      <span data-swatch="night:lamp" class="bg-accent-amber/20" />
      <span data-swatch="night:stapler" class="bg-body" />
      <span data-swatch="night:staplerTop" class="bg-on-dark-soft" />
      <span data-swatch="night:checkB" class="bg-on-dark/85" />
    </div>
  </div>
</template>
