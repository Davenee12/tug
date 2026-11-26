// The extension's one way to tug: a native message to bin\tug.exe, which makes the call over
// tug's local bridge (see src-tauri/crates/tug-bridge/src/native.rs for the format and the calls
// it lets through). Each call starts the host afresh, so a Revoke access in tug takes effect at
// once and nothing is kept between calls.

export const HOST = "dev.davejames.tug.browser";

export class TugError extends Error {
  constructor(code, message) {
    super(message || code);
    this.code = code;
  }
}

let nextId = 1;

/** Make one call; resolves with tug's answer or throws a TugError. */
export async function callTug(call) {
  if (globalThis.__tugMock) return globalThis.__tugMock(call);
  const id = nextId++;
  let reply;
  try {
    reply = await chrome.runtime.sendNativeMessage(HOST, { id, call });
  } catch (e) {
    // The browser couldn't start the host: tug isn't installed (or installed without it).
    const text = String(e?.message || e);
    if (/not found|not installed|forbidden/i.test(text)) throw new TugError("not_installed", text);
    throw new TugError("failed", text);
  }
  if (!reply || typeof reply !== "object") throw new TugError("failed", "tug sent an empty answer.");
  if (reply.error) throw new TugError(reply.error.code, reply.error.message);
  return reply.ok;
}

export const tug = {
  status: () => callTug({ tool: "status" }),
  latestCode: (copy = false) => callTug({ tool: "get_latest_code", copy }),
  search: (query, limit = 8) => callTug({ tool: "search_messages", query, limit }),
  media: (action) => callTug({ tool: "media_control", action }),
};
