import { describe, expect, it } from "vitest";
import { quickActionRunner, runQuickAction, type QuickActionHandlers } from "./quickActions";

function recorder() {
  const ran: string[] = [];
  const handlers: QuickActionHandlers = {
    newMessage: () => ran.push("newMessage"),
    tugboat: () => ran.push("tugboat"),
    copyCode: () => ran.push("copyCode"),
    game: () => ran.push("game"),
    settings: () => ran.push("settings"),
  };
  return { ran, handlers };
}

/** A take() the test answers by hand, like a backend call still in flight. */
function manualTake() {
  const calls: Array<{ resolve: (v: string[]) => void; reject: (e: unknown) => void }> = [];
  return {
    take: () => new Promise<string[]>((resolve, reject) => calls.push({ resolve, reject })),
    calls,
  };
}

describe("runQuickAction", () => {
  it("sends each task to its own handler", () => {
    const { ran, handlers } = recorder();
    for (const a of ["new-message", "tugboat", "copy-code", "game", "settings"]) {
      expect(runQuickAction(a, handlers)).toBe(true);
    }
    expect(ran).toEqual(["newMessage", "tugboat", "copyCode", "game", "settings"]);
  });

  it("does nothing for an action it doesn't know", () => {
    const { ran, handlers } = recorder();
    expect(runQuickAction("format-c", handlers)).toBe(false);
    expect(runQuickAction("toString", handlers)).toBe(false);
    expect(runQuickAction("newMessage", handlers)).toBe(false);
    expect(ran).toEqual([]);
  });
});

describe("quickActionRunner", () => {
  it("runs what was waiting, in order, once", async () => {
    const ran: string[] = [];
    let queue = ["copy-code", "settings"];
    const runner = quickActionRunner(
      async () => {
        const out = queue;
        queue = [];
        return out;
      },
      (a) => ran.push(a),
    );
    await runner.drain();
    // A second nudge with nothing new runs nothing again.
    await runner.drain();
    expect(ran).toEqual(["copy-code", "settings"]);
  });

  it("takes again after a nudge that came mid-take, without overlapping takes", async () => {
    const ran: string[] = [];
    const t = manualTake();
    const runner = quickActionRunner(t.take, (a) => ran.push(a));
    const first = runner.drain();
    // Two more nudges while the first take is in flight: one more take afterwards, not two at once.
    void runner.drain();
    void runner.drain();
    expect(t.calls).toHaveLength(1);
    t.calls[0].resolve(["new-message"]);
    await Promise.resolve();
    await Promise.resolve();
    expect(t.calls).toHaveLength(2);
    t.calls[1].resolve(["game"]);
    await first;
    expect(ran).toEqual(["new-message", "game"]);
    expect(t.calls).toHaveLength(2);
  });

  it("a failed take runs nothing and the next nudge tries again", async () => {
    const ran: string[] = [];
    let fail = true;
    const runner = quickActionRunner(
      async () => {
        if (fail) throw new Error("not ready");
        return ["tugboat"];
      },
      (a) => ran.push(a),
    );
    await runner.drain();
    expect(ran).toEqual([]);
    fail = false;
    await runner.drain();
    expect(ran).toEqual(["tugboat"]);
  });
});
