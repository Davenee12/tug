import { describe, expect, it } from "vitest";
import {
  HEARTBEAT_MS,
  MAX_IN_FLIGHT,
  MIN_GAP_MS,
  PRESS_EXPIRE_MS,
  PressLatch,
  chooseLayout,
  controlsFor,
  encodeFor,
  encodePad,
  encodeStackPad,
  gameFrom,
  nextSendDelay,
  padBody,
  padErrorAction,
  positionFromTouch,
  probe,
  samePad,
  tiltSupport,
  tiltToSteer,
} from "./pad";
import { advance, newStack, padDrop } from "../lib/harborStack";
import vector from "./padVector.json";

describe("encodePad", () => {
  it("sends whole-number steering from -100 to 100 and a real boolean", () => {
    expect(encodePad(0.5, true)).toEqual({ steer: 50, boost: true });
    expect(encodePad(-1, false)).toEqual({ steer: -100, boost: false });
    expect(encodePad(0.333, false)).toEqual({ steer: 33, boost: false });
  });

  it("never sends anything the PC would refuse", () => {
    expect(encodePad(7, false).steer).toBe(100);
    expect(encodePad(-7, false).steer).toBe(-100);
    expect(encodePad(Number.NaN, false).steer).toBe(0);
    expect(encodePad(Number.POSITIVE_INFINITY, false).steer).toBe(0);
    expect(Object.is(encodePad(-0.001, false).steer, 0)).toBe(true);
    // Only the two fields, so the JSON is exactly what the PC expects.
    expect(Object.keys(encodePad(0.2, true))).toEqual(["steer", "boost"]);
    expect(JSON.stringify(encodePad(-0.004, false))).toBe('{"steer":0,"boost":false}');
  });

  it("compares states", () => {
    expect(samePad(null, encodePad(0, false))).toBe(false);
    expect(samePad(encodePad(0.5, true), encodePad(0.5, true))).toBe(true);
    expect(samePad(encodePad(0.5, true), encodePad(0.5, false))).toBe(false);
  });
});

describe("positionFromTouch", () => {
  it("maps straight across the pad, with easy full left and full right", () => {
    expect(positionFromTouch(200, 400)).toBe(0);
    expect(positionFromTouch(300, 400)).toBeCloseTo(0.5 / 0.88);
    expect(positionFromTouch(100, 400)).toBeCloseTo(-0.5 / 0.88);
    // The thin margins at the edges are full lock.
    expect(positionFromTouch(395, 400)).toBe(1);
    expect(positionFromTouch(4, 400)).toBe(-1);
    // Off the pad (a finger that slid past the edge) stays at full lock.
    expect(positionFromTouch(-30, 400)).toBe(-1);
    expect(positionFromTouch(480, 400)).toBe(1);
  });

  it("moves in proportion to the finger: no dead zone", () => {
    const a = positionFromTouch(210, 400);
    const b = positionFromTouch(220, 400);
    expect(a).toBeGreaterThan(0);
    expect(b - a).toBeCloseTo(a - positionFromTouch(200, 400));
  });

  it("is safe with odd sizes", () => {
    expect(positionFromTouch(10, 0)).toBe(0);
    expect(positionFromTouch(Number.NaN, 400)).toBe(0);
  });
});

describe("probe", () => {
  it("sends whole milliseconds the PC accepts", () => {
    expect(probe(3.4, 12.6)).toEqual({ age: 3, rtt: 13 });
    expect(probe(-5, 99_999)).toEqual({ age: 0, rtt: 10_000 });
    expect(probe(Number.NaN, Number.POSITIVE_INFINITY)).toEqual({ age: 0, rtt: 0 });
  });
});
describe("pacing", () => {
  it("sends a change at once, at most ~60 times a second, and a heartbeat when nothing changes", () => {
    expect(MIN_GAP_MS).toBeLessThanOrEqual(17);
    expect(MAX_IN_FLIGHT).toBe(2);
    // The first change after a pause goes straight out.
    expect(nextSendDelay(Number.POSITIVE_INFINITY, true)).toBe(0);
    expect(nextSendDelay(0, true)).toBe(MIN_GAP_MS);
    expect(nextSendDelay(MIN_GAP_MS + 5, true)).toBe(0);
    expect(nextSendDelay(50, false)).toBe(HEARTBEAT_MS - 50);
    expect(nextSendDelay(HEARTBEAT_MS * 2, false)).toBe(0);
  });
});

describe("padErrorAction", () => {
  it("goes back to the Tugboat page when the game or session ended", () => {
    // "full": a two-player race already has its two phones.
    for (const code of ["no-game", "closed", "unauthorized", "in-use", "full", "no-race", "player-2-code"]) expect(padErrorAction(code)).toBe("leave");
    expect(padErrorAction("busy")).toBe("slow");
    for (const code of ["network", "stale", "bad-request", "error"]) expect(padErrorAction(code)).toBe("retry");
  });
});

describe("tiltSupport", () => {
  it("says tilt needs a secure connection on Tugboat's plain-HTTP page", () => {
    expect(tiltSupport({ secure: false, hasOrientation: true })).toBe("needs-secure");
    expect(tiltSupport({ secure: false, hasOrientation: false })).toBe("needs-secure");
    expect(tiltSupport({ secure: true, hasOrientation: true })).toBe("available");
    expect(tiltSupport({ secure: true, hasOrientation: false })).toBe("unsupported");
  });
});

describe("Harbor Stack's input", () => {
  it("carries Drop and the crane nudge, never a boost", () => {
    expect(encodeStackPad(-0.5, true)).toEqual({ steer: -50, boost: false, drop: true });
    expect(JSON.stringify(encodeStackPad(0, false))).toBe('{"steer":0,"boost":false,"drop":false}');
    expect(encodeStackPad(Number.NaN, true).steer).toBe(0);
  });

  it("leaves Tugboat Run's input exactly as it was", () => {
    expect(encodeFor("tugboat-run", 0.2, true)).toEqual({ steer: 20, boost: true });
    expect(Object.keys(encodeFor("tugboat-run", 0, false))).toEqual(["steer", "boost"]);
    expect(encodeFor("harbor-stack", 0.2, true)).toEqual({ steer: 20, boost: false, drop: true });
  });

  it("counts a change of Drop as a change", () => {
    expect(samePad(encodeStackPad(0, false), encodeStackPad(0, true))).toBe(false);
    expect(samePad(encodeStackPad(0, true), encodeStackPad(0, true))).toBe(true);
    // A Tugboat Run input without the field equals one that says "not held".
    expect(samePad({ steer: 0, boost: false }, { steer: 0, boost: false, drop: false })).toBe(true);
  });
});

describe("which game", () => {
  it("reads the PC's game id, and falls back to Tugboat Run", () => {
    expect(gameFrom("harbor-stack")).toBe("harbor-stack");
    expect(gameFrom("tugboat-run")).toBe("tugboat-run");
    expect(gameFrom(undefined)).toBe("tugboat-run");
    expect(gameFrom("snake")).toBe("tugboat-run");
  });

  it("gives each game its own buttons", () => {
    const run = controlsFor("tugboat-run");
    expect([run.title, run.action, run.springBack]).toEqual(["Tugboat Run", "Boost", false]);
    const stack = controlsFor("harbor-stack");
    expect([stack.title, stack.action, stack.springBack]).toEqual(["Harbor Stack", "Drop", true]);
    expect(stack.ends).toEqual(["Slower", "Faster"]);
  });
});

describe("chooseLayout", () => {
  it("is a gamepad held sideways", () => {
    expect(chooseLayout({ landscape: true, portraitAnyway: false })).toBe("landscape");
    expect(chooseLayout({ landscape: true, portraitAnyway: true })).toBe("landscape");
  });

  it("asks to turn the phone when it's upright, unless the player chose to play upright", () => {
    expect(chooseLayout({ landscape: false, portraitAnyway: false })).toBe("rotate");
    expect(chooseLayout({ landscape: false, portraitAnyway: true })).toBe("portrait");
  });
});

describe("tiltToSteer", () => {
  it("turns the phone like a steering wheel when it's held sideways", () => {
    expect(tiltToSteer(12.5, 3, 90)).toBe(0.5);
    expect(tiltToSteer(12.5, 3, -90)).toBe(-0.5);
    expect(tiltToSteer(12.5, 3, 270)).toBe(-0.5);
    expect(tiltToSteer(80, 0, 90)).toBe(1);
  });

  it("uses side-to-side tilt upright, and is safe without readings", () => {
    expect(tiltToSteer(40, -25, 0)).toBe(-1);
    expect(tiltToSteer(null, null, 90)).toBe(0);
    expect(tiltToSteer(Number.NaN, 0, 90)).toBe(0);
  });
});

describe("the body on the wire (padVector.json, shared with the Rust decoder)", () => {
  it("is exactly what the page's encoder makes, Drop included", () => {
    for (const c of vector.cases) {
      const input = encodeFor(gameFrom(c.game), c.steer, c.held || c.pending);
      const p = "probe" in c ? c.probe : undefined;
      expect(JSON.stringify(padBody(input, p)), c.name).toBe(c.body);
    }
  });

  it("never drops a field the input carries (Drop once went missing here)", () => {
    expect(padBody({ steer: 0, boost: false, drop: true })).toEqual({ steer: 0, boost: false, drop: true });
    expect(Object.keys(padBody({ steer: 1, boost: true }, { age: 1, rtt: 2 }))).toEqual(["steer", "boost", "age", "rtt"]);
  });
});

describe("PressLatch: a quick tap always reaches the PC", () => {
  /** What goes on the wire, input after input, as each reply comes straight back. */
  function wire(latch: PressLatch, now: number, sends: number): boolean[] {
    const out: boolean[] = [];
    for (let i = 0; i < sends; i++) {
      out.push(latch.down(now));
      latch.heardUpTo(latch.mark(), now);
    }
    return out;
  }

  it("keeps the press in every input until a reply to one carrying it comes back", () => {
    const latch = new PressLatch();
    expect(latch.pending).toBe(false);
    // Down and up before the next send: the finger is already off the button.
    latch.press(0);
    latch.release(5);
    expect(encodeFor("harbor-stack", 0, latch.down(10)).drop).toBe(true);
    const mark = latch.mark();
    // A second input goes out before the first reply (two in flight): still the press, so even if
    // the PC gets this one first and ignores the older, it sees the drop.
    expect(encodeFor("harbor-stack", 0, latch.down(12)).drop).toBe(true);
    // The reply comes back: now the release goes, and once that's heard, nothing waits.
    latch.heardUpTo(mark, 20);
    expect(encodeFor("harbor-stack", 0, latch.down(21)).drop).toBe(false);
    latch.heardUpTo(latch.mark(), 30);
    expect(latch.pending).toBe(false);
    expect(latch.down(31)).toBe(false);
  });

  it("two quick taps are two presses, with a release between them", () => {
    const latch = new PressLatch();
    // Tap, tap, before the first press was heard.
    latch.press(0);
    latch.release(30);
    latch.press(60);
    latch.release(90);
    expect(wire(latch, 100, 5)).toEqual([true, false, true, false, false]);
    // Held through a tap: a press, a release, a press, and it stays pressed.
    const held = new PressLatch();
    held.press(0);
    held.release(10);
    held.press(20);
    expect(wire(held, 30, 4)).toEqual([true, false, true, true]);
  });

  it("doesn't lose a tap to a stale reply or a failed send", () => {
    const latch = new PressLatch();
    latch.press(0);
    const one = latch.mark();
    latch.release(5);
    latch.press(10);
    // A reply to an input sent before anything was waiting confirms nothing.
    latch.heardUpTo(0, 12);
    expect(latch.down(12)).toBe(true);
    // The first press is heard; its release goes next.
    latch.heardUpTo(one, 15);
    expect(latch.down(16)).toBe(false);
    // That send failed: nothing confirmed, still waiting to release.
    latch.mark();
    expect(latch.down(20)).toBe(false);
    latch.heardUpTo(latch.mark(), 25);
    expect(latch.down(26)).toBe(true);
    latch.heardUpTo(latch.mark(), 30);
    expect(latch.pending).toBe(false);
    // Still held.
    expect(latch.down(31)).toBe(true);
  });

  it("a press the PC never heard expires, and a held finger doesn't fire it late", () => {
    const latch = new PressLatch();
    latch.press(0);
    expect(latch.down(PRESS_EXPIRE_MS)).toBe(true);
    // The Wi-Fi blipped: nothing came back in time.
    expect(latch.down(PRESS_EXPIRE_MS + 1)).toBe(false);
    expect(latch.pending).toBe(false);
    // The finger is still down; it only counts again once it lifts and presses.
    expect(latch.down(2000)).toBe(false);
    latch.release(2100);
    expect(latch.pending).toBe(false);
    latch.press(2200);
    expect(latch.down(2201)).toBe(true);
  });

  it("the connection dropping forgets waiting presses", () => {
    const latch = new PressLatch();
    latch.press(0);
    latch.release(5);
    latch.press(10);
    latch.clear();
    expect(latch.pending).toBe(false);
    expect(latch.down(20)).toBe(false);
  });

  it("drops a container end to end: tap → wire → PC events → the game's next frame", () => {
    // The phone: a tap whose release comes before the first send.
    const latch = new PressLatch();
    latch.press(0);
    latch.release(5);
    const press = padBody(encodeFor("harbor-stack", 0, latch.down(10)));
    latch.heardUpTo(latch.mark(), 20);
    const release = padBody(encodeFor("harbor-stack", 0, latch.down(21)));
    // The PC turns each into a `game-pad` event (the Rust side of this is in pad.rs, from the same
    // padVector.json bodies): both land between two frames.
    const events = [press, release].map((b) => ({ connected: true, drop: b.drop === true }));
    expect(events.map((e) => e.drop)).toEqual([true, false]);
    let held = false;
    let latched = false;
    for (const e of events) {
      const r = padDrop(held, e);
      if (r.fresh) latched = true;
      held = r.held;
    }
    // The next frame takes the latched press: exactly one drop.
    const s = newStack(1);
    advance(s, { drop: latched, nudge: 0 }, 1 / 60);
    expect(s.events.filter((e) => e === "drop")).toHaveLength(1);
    // Holding Drop doesn't drop again; only a new press does.
    expect(padDrop(true, { connected: true, drop: true }).fresh).toBe(false);
    expect(padDrop(false, { connected: false, drop: true }).fresh).toBe(false);
  });
});
