import { describe, expect, it } from "vitest";
import { singleFlight } from "./singleFlight";

describe("singleFlight (Answer, Decline, ✕ Clear)", () => {
  it("a double press sends one action", async () => {
    const sent: boolean[] = [];
    let finish!: () => void;
    const answer = singleFlight((positive: boolean) => {
      sent.push(positive);
      return new Promise<void>((r) => (finish = r));
    });
    const first = answer.run(true);
    expect(answer.busy.value).toBe(true);
    await answer.run(true);
    await answer.run(false); // Decline while Answer is out: also ignored
    expect(sent).toEqual([true]);
    finish();
    await first;
    expect(answer.busy.value).toBe(false);
  });

  it("can be pressed again once the first press was answered, even if it failed", async () => {
    let calls = 0;
    const clear = singleFlight(async () => {
      calls++;
      throw new Error("gone");
    });
    await expect(clear.run()).rejects.toThrow("gone");
    expect(clear.busy.value).toBe(false);
    await expect(clear.run()).rejects.toThrow("gone");
    expect(calls).toBe(2);
  });
});
