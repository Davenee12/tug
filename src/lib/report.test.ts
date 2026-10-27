import { describe, expect, it, vi } from "vitest";

const calls: string[] = [];
let copyFails = false;
vi.mock("./ipc", () => ({
  api: {
    copyDiagnostics: vi.fn(async () => {
      calls.push("copy");
      if (copyFails) throw new Error("clipboard busy");
      return "report";
    }),
    reportProblem: vi.fn(async (unclean: boolean) => {
      calls.push(`open:${unclean}`);
    }),
  },
}));

const { reportProblem } = await import("./report");

describe("reportProblem", () => {
  it("copies the diagnostics before opening the form", async () => {
    calls.length = 0;
    await reportProblem(false);
    await reportProblem(true);
    expect(calls).toEqual(["copy", "open:false", "copy", "open:true"]);
  });

  it("doesn't open the form without the diagnostics to paste", async () => {
    calls.length = 0;
    copyFails = true;
    await expect(reportProblem(false)).rejects.toThrow("clipboard busy");
    expect(calls).toEqual(["copy"]);
    copyFails = false;
  });
});
