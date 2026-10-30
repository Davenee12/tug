import { describe, expect, it } from "vitest";
import { askerName, countdown, fileFrom, filesQuestion, lastUsedLabel, PERMISSION_HELP, secondsLeft, sendArmed, setupSnippets, tomlString } from "./devtools";

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

  it("keeps a path with an apostrophe intact for Codex", () => {
    const odd = "C:\\Users\\O'Brien\\AppData\\Local\\tug\\bin\\tug.exe";
    expect(tomlString(odd)).toBe('"C:\\\\Users\\\\O\'Brien\\\\AppData\\\\Local\\\\tug\\\\bin\\\\tug.exe"');
    // TOML basic strings unescape like JSON for these characters.
    expect(JSON.parse(tomlString(odd))).toBe(odd);
  });
});

describe("the send button", () => {
  it("waits a moment and needs tug's window in front", () => {
    expect(sendArmed(0, 1000, true)).toBe(false);
    expect(sendArmed(0, 1500, true)).toBe(true);
    expect(sendArmed(0, 60_000, false)).toBe(false);
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
  });
  it("shows the name as tug made it, saying it's an AI tool", () => {
    expect(askerName("Claude Code (an AI tool)")).toBe("Claude Code (an AI tool)");
    expect(askerName(" Cursor (an AI tool) ")).toBe("Cursor (an AI tool)");
    expect(askerName("tug command")).toBe("tug command");
  });
});

describe("the file card", () => {
  const report = { name: "report.pdf", size: 2.1 * 1024 * 1024, folder: "C:\\Users\\Pat\\Documents" };
  it("asks about one file by name and size", () => {
    expect(filesQuestion([report])).toBe("Send report.pdf (2.1 MB) to your iPhone?");
  });
  it("counts several and adds up their sizes", () => {
    const shot = { name: "shot.png", size: 512 * 1024, folder: "C:\\Users\\Pat\\Pictures" };
    expect(filesQuestion([report, shot, shot])).toBe("Send 3 files (3.1 MB) to your iPhone?");
  });
  it("says where each file comes from", () => {
    expect(fileFrom(report)).toBe("C:\\Users\\Pat\\Documents");
    expect(fileFrom({ name: "notes.txt", size: 5, folder: null })).toBe("Piped into the tug command");
  });
});