import { describe, expect, it } from "vitest";
import { clearHistory, type ClearHistoryUi } from "./clearHistory";

function fakeUi() {
  const said: Array<[string, string]> = [];
  let emptied = 0;
  const ui: ClearHistoryUi = {
    emptyLists: () => void emptied++,
    say: (kind, text) => void said.push([kind, text]),
  };
  return { ui, said, emptied: () => emptied };
}

describe("clearHistory", () => {
  it("empties the lists and says so once the backend has cleared it", async () => {
    const f = fakeUi();
    expect(await clearHistory(async () => undefined, f.ui, String)).toBe(true);
    expect(f.emptied()).toBe(1);
    expect(f.said).toEqual([["info", "History cleared"]]);
  });

  it("a failure keeps everything shown and never claims it cleared", async () => {
    const f = fakeUi();
    const failing = () => Promise.reject(new Error("database is locked"));
    expect(await clearHistory(failing, f.ui, (e) => (e as Error).message)).toBe(false);
    expect(f.emptied()).toBe(0);
    expect(f.said).toEqual([["error", "Couldn't clear history: database is locked"]]);
  });
});
