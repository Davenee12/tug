import { describe, expect, it } from "vitest";
import { askerName, countdown, lastUsedLabel, PERMISSION_HELP, secondsLeft, setupSnippets } from "./devtools";

const CLI = "C:\\Users\\Pat\\AppData\\Local\\tug\\bin\\tug.exe";

describe("setupSnippets", () => {
  const byId = Object.fromEntries(setupSnippets(CLI).map((s) => [s.id, s]));

  it("gives Claude Code a one-line command with the path quoted", () => {
    expect(byId.claude.code).toBe(`claude mcp add --scope user tug -- "${CLI}" mcp`);
  });

  it("writes JSON that parses back to the same path", () => {
    const cursor = JSON.parse(byId.cursor.code);
    expect(cursor.mcpServers.tug).toEqual({ command: CLI, args: ["mcp"] });
    const vscode = JSON.parse(byId.vscode.code);
    expect(vscode.servers.tug).toEqual({ type: "stdio", command: CLI, args: ["mcp"] });
  });

  it("uses a TOML literal string for Codex so backslashes survive", () => {
    expect(byId.codex.code).toContain(`command = '${CLI}'`);
    expect(byId.codex.code).toContain('args = ["mcp"]');
  });
});

describe("time helpers", () => {
  const now = 1_791_365_400_000;
  it("labels last use", () => {
    expect(lastUsedLabel(now - 10_000, now)).toBe("just now");
    expect(lastUsedLabel(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(lastUsedLabel(now - 3 * 3_600_000, now)).toBe("3 h ago");
    expect(lastUsedLabel(now - 30 * 3_600_000, now)).toBe("yesterday");
    expect(lastUsedLabel(now + 5000, now)).toBe("just now");
  });

  it("counts down the confirmation card", () => {
    expect(secondsLeft(now + 120_000, now)).toBe(120);
    expect(secondsLeft(now + 1, now)).toBe(1);
    expect(secondsLeft(now - 5, now)).toBe(0);
    expect(countdown(65)).toBe("1:05");
    expect(countdown(0)).toBe("0:00");
  });
});

describe("copy", () => {
  it("explains every switch in a sentence", () => {
    for (const text of Object.values(PERMISSION_HELP)) expect(text).toMatch(/^[A-Z].*\.$/);
  });
  it("names an unnamed asker", () => {
    expect(askerName("  ")).toBe("An AI tool");
    expect(askerName("claude-code")).toBe("claude-code");
  });
});
