import { describe, expect, it } from "vitest";
import {
  canRestart,
  createHoldRepeater,
  holdDelay,
  HOLD_MIN_REPEAT_MS,
  HOLD_REPEAT_MS,
  repeatIgnoredMessage,
  repeatLabel,
  SKIP_SECONDS,
  supportsDislike,
  supportsLike,
  supportsRepeat,
  supportsSkipBack,
  supportsSkipForward,
  skipMode,
  skipTargetMs,
  type HoldTimers,
} from "./media";

describe("supportsRepeat", () => {
  it("needs AdvanceRepeatMode in the player's command list", () => {
    expect(supportsRepeat({ available: ["play", "pause", "advanceRepeatMode"] })).toBe(true);
    expect(supportsRepeat({ available: ["play", "pause", "nextTrack"] })).toBe(false);
  });

  it("is off before the list arrives, unlike the other buttons", () => {
    expect(supportsRepeat({ available: [] })).toBe(false);
  });
});

describe("repeatLabel", () => {
  it("names the phone-reported mode", () => {
    expect(repeatLabel("off")).toBe("Repeat is off");
    expect(repeatLabel("all")).toBe("Repeating all");
    expect(repeatLabel("one")).toBe("Repeating this song");
    expect(repeatLabel(null)).toBe("Repeat");
  });
});

describe("repeatIgnoredMessage", () => {
  it("names the player", () => {
    expect(repeatIgnoredMessage("Spotify")).toBe("Spotify didn't change repeat from your PC");
    expect(repeatIgnoredMessage(null)).toBe("The player didn't change repeat from your PC");
  });
});

describe("extra AMS controls appear only when the player lists them", () => {
  it("skip ±15 s follows skipBackward / skipForward", () => {
    const apple = { available: ["play", "pause", "skipForward", "skipBackward", "likeTrack", "dislikeTrack"] };
    expect(supportsSkipBack(apple)).toBe(true);
    expect(supportsSkipForward(apple)).toBe(true);
    const spotify = { available: ["play", "pause", "skipForward", "skipBackward"] };
    expect(supportsSkipForward(spotify)).toBe(true);
    expect(supportsLike(spotify)).toBe(false);
  });

  it("skip ±15 s seeks through the Spotify connection for Spotify, never its track-skipping command", () => {
    const spotify = { player: "Spotify", available: ["play", "pause", "skipForward", "skipBackward"] };
    expect(skipMode(spotify, "back", true)).toBe("seek");
    expect(skipMode(spotify, "forward", true)).toBe("seek");
    expect(skipMode(spotify, "back", false)).toBe("none");
    const apple = { player: "Music", available: ["skipBackward"] };
    expect(skipMode(apple, "back", false)).toBe("ams");
    expect(skipMode(apple, "forward", true)).toBe("none");
  });

  it("a skip lands inside the song", () => {
    expect(skipTargetMs(73, -15, 200)).toBe(58_000);
    expect(skipTargetMs(10, -15, 200)).toBe(0);
    expect(skipTargetMs(195, 15, 200)).toBe(199_000);
    expect(skipTargetMs(30, 15, null)).toBe(45_000);
  });

  it("like / dislike follow likeTrack / dislikeTrack", () => {
    const apple = { available: ["likeTrack", "dislikeTrack"] };
    expect(supportsLike(apple)).toBe(true);
    expect(supportsDislike(apple)).toBe(true);
    expect(supportsLike({ available: ["play"] })).toBe(false);
  });

  it("nothing shows before the command list arrives", () => {
    const none = { available: [] };
    expect(supportsSkipBack(none)).toBe(false);
    expect(supportsSkipForward(none)).toBe(false);
    expect(supportsLike(none)).toBe(false);
    expect(supportsDislike(none)).toBe(false);
    expect(SKIP_SECONDS).toBe(15);
  });
});

describe("canRestart", () => {
  it("only sends Back well past the start", () => {
    expect(canRestart(42)).toBe(true);
    expect(canRestart(5.5)).toBe(true);
    expect(canRestart(4)).toBe(false);
    expect(canRestart(0)).toBe(false);
    expect(canRestart(null)).toBe(true);
  });
});

describe("holdDelay", () => {
  const steady = { repeatMs: 100, minRepeatMs: 100, accel: 1 };

  it("is steady with no acceleration", () => {
    expect(holdDelay(0, steady)).toBe(100);
    expect(holdDelay(5, steady)).toBe(100);
  });

  it("accelerates geometrically but never below the floor", () => {
    const cfg = { repeatMs: 200, minRepeatMs: 90, accel: 0.5 };
    expect(holdDelay(0, cfg)).toBe(200);
    expect(holdDelay(1, cfg)).toBe(100);
    expect(holdDelay(2, cfg)).toBe(90); // 50 clamped up to the floor
    expect(holdDelay(9, cfg)).toBe(90);
  });

  it("defaults sit between the floor and the base interval", () => {
    expect(holdDelay(0)).toBe(HOLD_REPEAT_MS);
    expect(holdDelay(50)).toBe(HOLD_MIN_REPEAT_MS);
  });
});

// A controllable clock + timer pair, and a step whose promise the test resolves by hand, so
// both the timing and the "one write in flight" rule can be checked deterministically.
function harness() {
  let seq = 1;
  let clock = 0;
  const pending = new Map<number, { fn: () => void; due: number }>();
  const timers: HoldTimers = {
    set: (fn, ms) => {
      const id = seq++;
      pending.set(id, { fn, due: clock + ms });
      return id;
    },
    clear: (id) => void pending.delete(id),
  };
  function advance(ms: number) {
    clock += ms;
    for (;;) {
      const next = [...pending.entries()].filter(([, t]) => t.due <= clock).sort((a, b) => a[1].due - b[1].due)[0];
      if (!next) break;
      pending.delete(next[0]);
      next[1].fn();
    }
  }
  return { timers, advance, get scheduled() {
    return pending.size;
  } };
}

// Resolve microtasks so the step's `.finally` (which clears the busy flag) runs.
const flush = () => Promise.resolve().then(() => Promise.resolve());

describe("createHoldRepeater", () => {
  const cfg = { initialMs: 400, repeatMs: 180, minRepeatMs: 180, accel: 1 };

  it("steps once immediately on press and repeats after the initial delay", async () => {
    const h = harness();
    let calls = 0;
    const r = createHoldRepeater(h.timers, cfg);
    r.start(() => {
      calls++;
      return Promise.resolve();
    }, () => false);

    expect(calls).toBe(1); // immediate
    expect(r.active).toBe(true);

    h.advance(399);
    await flush();
    expect(calls).toBe(1); // not yet

    h.advance(1);
    await flush();
    expect(calls).toBe(2); // first auto-repeat at 400ms

    h.advance(180);
    await flush();
    expect(calls).toBe(3); // steady repeat

    r.stop();
    expect(r.active).toBe(false);
    h.advance(1000);
    await flush();
    expect(calls).toBe(3); // nothing after stop
    expect(h.scheduled).toBe(0); // timer cleaned up
  });

  it("stops repeating once the limit is reached, and never after stop()", async () => {
    const h = harness();
    let calls = 0;
    let atLimit = false;
    const r = createHoldRepeater(h.timers, cfg);
    r.start(() => {
      calls++;
      return Promise.resolve();
    }, () => atLimit);

    expect(calls).toBe(1);
    await flush(); // let the immediate step settle so the next tick isn't skipped as busy
    h.advance(400);
    await flush();
    expect(calls).toBe(2);

    atLimit = true; // phone reports volume hit the end
    h.advance(180);
    await flush();
    expect(calls).toBe(2); // the limit tick sends nothing
    expect(r.active).toBe(false);
    expect(h.scheduled).toBe(0);
  });

  it("never has more than one write in flight: a busy tick is skipped", async () => {
    const h = harness();
    let calls = 0;
    let inFlight = 0;
    let maxInFlight = 0;
    let resolve: () => void = () => {};
    const r = createHoldRepeater(h.timers, cfg);
    const step = () => {
      calls++;
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      return new Promise<void>((res) => {
        resolve = () => {
          inFlight--;
          res();
        };
      });
    };
    r.start(step, () => false);
    expect(calls).toBe(1); // immediate, still in flight (unresolved)

    // The initial write hasn't resolved: the 400ms tick must skip rather than start a second.
    h.advance(400);
    await flush();
    expect(calls).toBe(1);
    expect(maxInFlight).toBe(1);

    // Finish the first write; the next retry tick is then free to send.
    resolve();
    await flush();
    h.advance(180);
    await flush();
    expect(calls).toBe(2);
    expect(maxInFlight).toBe(1);

    r.stop();
  });

  it("ignores a second start() while already holding", () => {
    const h = harness();
    let calls = 0;
    const r = createHoldRepeater(h.timers, cfg);
    const step = () => {
      calls++;
      return Promise.resolve();
    };
    r.start(step, () => false);
    r.start(step, () => false); // key auto-repeat / double pointer: ignored
    expect(calls).toBe(1);
    r.stop();
  });
});
