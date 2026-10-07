// Settings › Developer tools: copy for each switch, the copy-paste setup for each AI tool, and
// small time helpers. Pure, so it's all unit-tested.

import type { DevToolsPermissionKey } from "../types/protocol";

/** What each switch lets AI tools (and the `tug` command) do, in plain words. */
export const PERMISSION_HELP: Record<DevToolsPermissionKey, string> = {
  codes: "Read the newest verification code (from the last 10 minutes).",
  search: "Search your texts and notifications.",
  dev_notifications: "Read recent notifications from GitHub, Slack, Linear, Jira, Sentry, PagerDuty, Vercel and Netlify.",
  tugboat_files: "See files you sent from your phone with Tugboat, and look at small images.",
  phone_status: "See whether your phone is connected, its battery, and what's playing.",
  media: "Play, pause and skip music on your phone.",
  send_text: "Ask to text someone. You always see the message first, and it's sent only if you click Send.",
  tugboat_send: "Send files from this PC to your phone with Tugboat (tug boat).",
};

export interface SetupSnippet {
  id: "claude" | "codex" | "cursor" | "vscode";
  name: string;
  /** Where it goes, in a few words. */
  where: string;
  code: string;
}

/** JSON string contents: backslashes and quotes escaped. */
const jsonPath = (p: string) => JSON.stringify(p);

/**
 * A TOML string for a path: a literal string ('…') keeps Windows backslashes as they are, but
 * can't hold a `'` (C:\Users\O'Brien), so then a basic string with JSON-style escapes, which
 * TOML reads the same way.
 */
export function tomlString(p: string): string {
  return p.includes("'") || /[\u0000-\u001f]/.test(p) ? JSON.stringify(p) : `'${p}'`;
}

/** Copy-paste setup for each AI tool, pointing at this PC's tug command. */
export function setupSnippets(cliPath: string): SetupSnippet[] {
  return [
    {
      id: "claude",
      name: "Claude Code",
      where: "Run in a terminal",
      code: `claude mcp add --scope user tug -- "${cliPath}" mcp`,
    },
    {
      id: "codex",
      name: "Codex",
      where: "Add to ~/.codex/config.toml",
      code: `[mcp_servers.tug]\ncommand = ${tomlString(cliPath)}\nargs = ["mcp"]`,
    },
    {
      id: "cursor",
      name: "Cursor",
      where: "Add to ~/.cursor/mcp.json",
      code: `{\n  "mcpServers": {\n    "tug": { "command": ${jsonPath(cliPath)}, "args": ["mcp"] }\n  }\n}`,
    },
    {
      id: "vscode",
      name: "VS Code",
      where: "Add to mcp.json (MCP: Open User Configuration)",
      code: `{\n  "servers": {\n    "tug": { "type": "stdio", "command": ${jsonPath(cliPath)}, "args": ["mcp"] }\n  }\n}`,
    },
  ];
}

/** "just now", "5 min ago", "3 h ago", "yesterday", or the date. */
export function lastUsedLabel(at: number, now: number): string {
  const s = Math.max(0, (now - at) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 2 * 86_400) return "yesterday";
  return new Date(at).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Whole seconds left on the confirmation card (never negative). */
export function secondsLeft(expiresAt: number, now: number): number {
  return Math.max(0, Math.ceil((expiresAt - now) / 1000));
}

/** "1:05" for the card's countdown. */
export function countdown(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** How long Send stays disabled after the card appears, so a click or key already on its way can't send. */
export const SEND_ARM_MS = 1500;

/** Send is clickable only once the card has been up a moment and tug's window has focus. */
export function sendArmed(shownAt: number, now: number, windowFocused: boolean): boolean {
  return windowFocused && now - shownAt >= SEND_ARM_MS;
}

/** A tool name fit for the card's sentence ("Claude Code", already tamed by tug), "" → "An AI tool". */
export function askerName(tool: string): string {
  const t = tool.trim();
  return t.length ? t : "An AI tool";
}
