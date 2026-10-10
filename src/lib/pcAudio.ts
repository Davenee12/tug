// "Play iPhone audio on this PC": what the Now Playing button and Settings › iPhone say for each
// state. The decisions (when it's on, which device is the iPhone, turning on by itself) are in Rust
// (src-tauri/src/pc_audio); this only turns its status into words and the one thing a click does.

import type { PcAudioProblem, PcAudioStatus } from "../types/protocol";

/** One calm sentence per problem: what happened, and what to do. */
export function pcAudioProblemText(problem: PcAudioProblem): string {
  switch (problem) {
    case "notFound":
      return "This PC can't find your iPhone for audio. Make sure your iPhone is connected in Bluetooth settings, then try again.";
    case "denied":
      return "Windows didn't let your iPhone play on this PC. Try again in a moment.";
    case "timedOut":
      return "Your iPhone didn't connect audio. Make sure it's unlocked and try again.";
    case "failed":
      return "Your iPhone's audio couldn't connect. Try again in a moment.";
    case "dropped":
      return "Your iPhone stopped playing on this PC.";
  }
}

export interface PcAudioView {
  /** Whether to offer it at all (Windows supports it and an iPhone is connected, or it's on). */
  show: boolean;
  /** "start" turns it on (also Try again / Reconnect); "stop" turns it off or cancels connecting. */
  action: "start" | "stop" | null;
  /** The button's words. */
  label: string;
  /** Spoken name and tooltip for the button. */
  title: string;
  /** Playing (or about to play) through this PC: styled so it can't be missed. */
  active: boolean;
  busy: boolean;
  /** One line for Settings: where things stand. */
  line: string;
  /** The current problem as a sentence, if any. */
  problem: string | null;
}

/** What to show, from the status (null until it's loaded) and whether the iPhone is connected. */
export function pcAudioView(s: PcAudioStatus | null, connected: boolean): PcAudioView {
  const none: PcAudioView = {
    show: false,
    action: null,
    label: "Play on this PC",
    title: "Play on this PC",
    active: false,
    busy: false,
    line: "Play your iPhone's sound through this PC's speakers.",
    problem: null,
  };
  if (!s) return none;
  if (!s.supported) return { ...none, line: "Needs Windows 10 version 2004 or later." };
  if (s.state === "on") {
    return {
      show: true,
      action: "stop",
      label: "Stop",
      title: "Stop playing on this PC",
      active: true,
      busy: false,
      line: "Your iPhone's sound is playing through this PC's speakers.",
      problem: null,
    };
  }
  if (s.state === "connecting") {
    return {
      show: true,
      action: "stop",
      label: "Cancel",
      title: "Stop connecting",
      active: true,
      busy: true,
      line: "Connecting to your iPhone's audio…",
      problem: null,
    };
  }
  const problem = s.problem ? pcAudioProblemText(s.problem) : null;
  if (!connected) return { ...none, line: problem ?? "Connect your iPhone first.", problem };
  const retry = s.problem === "dropped" ? "Reconnect" : s.problem ? "Try again" : "Play on this PC";
  return {
    show: true,
    action: "start",
    label: retry,
    title: s.problem ? retry : "Play on this PC: hear your iPhone through this PC's speakers",
    active: false,
    busy: false,
    line: problem ?? none.line,
    problem,
  };
}

/**
 * A problem worth a message at the bottom of the window: one that just appeared (not one already
 * showing when the status first loads, and not the same one again).
 */
export function newPcAudioProblem(prev: PcAudioStatus | null, next: PcAudioStatus): PcAudioProblem | null {
  if (!prev || !next.problem) return null;
  if (prev.problem === next.problem && prev.state === next.state) return null;
  return next.problem;
}
