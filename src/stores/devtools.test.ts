import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { DevToolsConfirm, DevToolsStatus } from "../types/protocol";

const handlers = new Map<string, (payload: unknown) => void>();
const held: Record<string, (s: DevToolsStatus) => void> = {};
const hold = (name: string) => () => new Promise<DevToolsStatus>((resolve) => (held[name] = resolve));

vi.mock("../lib/ipc", () => ({
  api: {
    devtoolsStatus: vi.fn(hold("devtoolsStatus")),
    devtoolsSetEnabled: vi.fn(hold("devtoolsSetEnabled")),
  },
  on: vi.fn(async (event: string, fn: (payload: unknown) => void) => {
    handlers.set(event, fn);
    return () => handlers.delete(event);
  }),
  errorMessage: (e: unknown) => String(e),
}));

const { useDevToolsStore } = await import("./devtools");

const base: DevToolsStatus = {
  rev: 1,
  enabled: false,
  permissions: [],
  textsWithoutAsking: false,
  clients: [],
  cliPath: null,
  cliDir: null,
  onPath: false,
  bridgeRunning: true,
  pending: null,
};
const card: DevToolsConfirm = {
  id: 1,
  kind: "text",
  tool: "Example tool",
  toName: "Sam",
  toAddress: "+15550100",
  message: "hi",
  expiresAt: 0,
};
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("devtools store ordering", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    handlers.clear();
  });

  it("the startup read doesn't overwrite an event that overtook it", async () => {
    const dt = useDevToolsStore();
    await dt.init();
    await flush(); // the read is in flight
    handlers.get("devtools-status")!({ ...base, rev: 5, enabled: true });
    held.devtoolsStatus({ ...base, rev: 4, enabled: false, pending: card });
    await flush();
    expect(dt.status).toMatchObject({ rev: 5, enabled: true });
    // The older copy's card doesn't come back either.
    expect(dt.confirm).toBeNull();
  });

  it("a switch's answer composed before a newer event is ignored", async () => {
    const dt = useDevToolsStore();
    await dt.init();
    held.devtoolsStatus({ ...base, rev: 1 });
    await flush();
    const done = dt.setEnabled(true);
    handlers.get("devtools-status")!({ ...base, rev: 3, enabled: true, clients: [{ name: "x", kind: "mcp", lastUsed: 1 }] });
    held.devtoolsSetEnabled({ ...base, rev: 2, enabled: true });
    expect(await done).toBe(true);
    expect(dt.status?.rev).toBe(3);
    expect(dt.status?.clients).toHaveLength(1);
  });

  it("a newer answer is taken", async () => {
    const dt = useDevToolsStore();
    await dt.init();
    held.devtoolsStatus({ ...base, rev: 1 });
    await flush();
    const done = dt.setEnabled(true);
    held.devtoolsSetEnabled({ ...base, rev: 2, enabled: true });
    await done;
    expect(dt.status?.enabled).toBe(true);
  });
});
