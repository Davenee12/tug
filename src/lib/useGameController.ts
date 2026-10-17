// The phone controller for one of tug's games (Tugboat Run, Harbor Stack), shared by both: opening
// and closing Tugboat's controller channel for that game, which phones are connected, the `game-pad`
// events, and what each phone is told back (paused, hits for a buzz, its player). The game keeps its
// own rules for what an input does; this keeps the plumbing identical for every game.
//
// Lifetime: listens only while the game's component is mounted, and closing the game closes the
// channel (an open still on its way is closed when it lands; see `controllerChannel`).

import { onMounted, onUnmounted, reactive, ref, watch } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTugboatStore } from "../stores/tugboat";
import { api, on } from "./ipc";
import { controllerChannel } from "./tugboat";
import type { GameId, GamePad } from "../types/protocol";

export type Seat = 1 | 2;

/** What a phone shows: paused or not, its hits so far (it buzzes on a new one), its player. */
export interface SeatFeedback {
  paused: boolean;
  hits: number;
  player: 1 | 2 | null;
}

export function useGameController(opts: {
  game: GameId;
  /** One phone, or two for a two-player game. */
  players: () => 1 | 2;
  /** A phone's input (or its going away), after `padConnected` is up to date. */
  onPad: (e: GamePad) => void;
  /** What the phone in `seat` should show now, or null to say nothing (seat 2 in a solo game). */
  feedback: (seat: Seat) => SeatFeedback | null;
}) {
  const tugboat = useTugboatStore();
  /** The controller channel was opened (closed again when the game closes). */
  const controllerOn = ref(false);
  const controllerError = ref<string | null>(null);
  /** Each phone (seat 1, seat 2) is sending inputs right now. */
  const padConnected = reactive([false, false]);
  let disposed = false;
  let unlisten: UnlistenFn | null = null;

  function handle(e: GamePad) {
    padConnected[e.seat === 2 ? 1 : 0] = e.connected;
    opts.onPad(e);
  }
  const gone = (seat: Seat): GamePad => ({ seat, connected: false, steer: 0, boost: false, drop: false });

  /** Bumped by each open and off, so only the latest one reports a failure. */
  let request = 0;
  /** Closing the game (or switching the controller off) while an open is on its way closes the
   * channel once it's open; otherwise an open is kept, whichever open it was: the backend has one
   * channel, so an older open landing last must not close the one a newer open made. */
  const channel = controllerChannel(
    { open: () => api.gamePadOpen(opts.players(), opts.game), close: () => api.gamePadClose() },
    () => ({ on: controllerOn.value, closed: disposed }),
  );

  /** Open (or re-size) the channel: one phone, or two. */
  async function open() {
    controllerError.value = null;
    const mine = ++request;
    try {
      const kept = await channel.open();
      if (kept) {
        lastFeedback[0] = lastFeedback[1] = "";
        sendFeedback();
      }
    } catch (e) {
      if (disposed || mine !== request) return;
      controllerOn.value = false;
      controllerError.value = typeof e === "string" ? e : "Couldn't start Tugboat.";
    }
  }

  /** "Use your phone as a controller". */
  async function use() {
    if (controllerOn.value) return;
    controllerOn.value = true;
    await open();
  }

  /** "Stop using the phone". */
  async function off() {
    request++;
    controllerOn.value = false;
    handle(gone(1));
    handle(gone(2));
    await api.gamePadClose().catch(() => undefined);
  }

  /** Tugboat turned off (its panel closed, tug hidden, or 10 minutes unused): so did the controller. */
  watch(
    () => tugboat.status.phase,
    (p) => {
      if (p === "off" && controllerOn.value) {
        controllerOn.value = false;
        handle(gone(1));
        handle(gone(2));
      }
    },
  );

  const lastFeedback = ["", ""];
  /** Tell each phone what it shows, when that changed. */
  function sendFeedback() {
    if (!controllerOn.value) return;
    for (const seat of [1, 2] as Seat[]) {
      const f = opts.feedback(seat);
      if (!f) continue;
      const key = `${f.paused}:${f.hits}:${f.player}`;
      if (key === lastFeedback[seat - 1]) continue;
      lastFeedback[seat - 1] = key;
      void api.gamePadFeedback(seat, f.paused, f.hits, f.player).catch(() => undefined);
    }
  }

  onMounted(() => {
    void on("game-pad", handle).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
  });
  onUnmounted(() => {
    disposed = true;
    unlisten?.();
    if (controllerOn.value) void api.gamePadClose().catch(() => undefined);
  });

  return { controllerOn, controllerError, padConnected, open, use, off, sendFeedback };
}
