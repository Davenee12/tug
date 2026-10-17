// Tugboat Run for two: the pure parts of a race. Two lanes side by side on the same course (one
// seed), each player with their own boat, lives and score; the race is over when both boats are.
//
// Controls: each player has keys of their own (Player 1 the arrows, Player 2 A, D and W), and a
// phone each. Seat 1 is the phone that opened Tugboat on this PC, seat 2 the second phone to scan
// the same code. "Swap" puts the first phone on Player 2, so the keyboard can be Player 1.

export type Player = 1 | 2;
export type Seat = 1 | 2;

/** Which phone steers `player`. */
export function seatFor(player: Player, swapped: boolean): Seat {
  return (swapped ? 3 - player : player) as Seat;
}

/** Which player the phone in `seat` steers. */
export function playerFor(seat: Seat, swapped: boolean): Player {
  return (swapped ? 3 - seat : seat) as Player;
}

export type RaceAction = "left" | "right" | "hop";

/** A key in a race: whose it is and what it does (null: not a race key). */
export function raceKey(key: string): { player: Player; action: RaceAction } | null {
  switch (key.length === 1 ? key.toLowerCase() : key) {
    case "ArrowLeft":
      return { player: 1, action: "left" };
    case "ArrowRight":
      return { player: 1, action: "right" };
    case "ArrowUp":
      return { player: 1, action: "hop" };
    case "a":
      return { player: 2, action: "left" };
    case "d":
      return { player: 2, action: "right" };
    case "w":
      return { player: 2, action: "hop" };
    default:
      return null;
  }
}

/** The race's result: the higher score wins; level scores are a tie (null). */
export function winner(scores: readonly [number, number]): Player | null {
  if (scores[0] === scores[1]) return null;
  return scores[0] > scores[1] ? 1 : 2;
}

export interface DropState {
  /** The race is being played (not ready, paused or over). */
  playing: boolean;
  swapped: boolean;
  /** Each player's boat is already out (sunk, or left the race). */
  out: readonly [boolean, boolean];
  /** Each player's phone has steered during this race. */
  phoneDrove: readonly [boolean, boolean];
}

/**
 * A phone just went quiet: the player whose race should pause for it, or null. Only a phone that
 * was steering a boat still in the race pauses it; a phone that never joined in, or whose boat has
 * sunk, doesn't (and the keyboard player is never "disconnected").
 */
export function droppedPlayer(seat: Seat, s: DropState): Player | null {
  if (!s.playing) return null;
  const p = playerFor(seat, s.swapped);
  return s.phoneDrove[p - 1] && !s.out[p - 1] ? p : null;
}
