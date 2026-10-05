// Now Playing rules that don't need the DOM, so they can be tested.
import type { NowPlaying, RepeatMode } from "../types/protocol";

/**
 * The loop button only appears when the player lists AdvanceRepeatMode. A repeat value on its
 * own isn't enough: iOS can report the queue's repeat mode for a player that won't take the
 * command, and a button that does nothing is worse than no button.
 */
export function supportsRepeat(np: Pick<NowPlaying, "available">): boolean {
  return np.available.includes("advanceRepeatMode");
}

export function repeatLabel(mode: RepeatMode | null): string {
  switch (mode) {
    case "all":
      return "Repeating all";
    case "one":
      return "Repeating this song";
    case "off":
      return "Repeat is off";
    default:
      return "Repeat";
  }
}

/** How long to wait for the phone to report a new repeat mode before saying it didn't change. */
export const REPEAT_CONFIRM_MS = 2000;

/** Shown when a repeat press went through but the phone never reported a new mode. */
export function repeatIgnoredMessage(player: string | null): string {
  return `${player ?? "The player"} didn't change repeat from your PC`;
}

/**
 * AMS has no seek, but Back restarts the song once it's a few seconds in (and goes to the previous
 * song near the start). tug's position is an estimate from the last report, so keep a margin above
 * the phone's ~3 s cut-off: a Back that lands just under it would skip to the previous song.
 */
export const RESTART_AFTER_S = 5;

/** Whether Back would restart the song rather than go to the previous one. Unknown position: try. */
export function canRestart(elapsed: number | null): boolean {
  return elapsed == null || elapsed > RESTART_AFTER_S;
}
