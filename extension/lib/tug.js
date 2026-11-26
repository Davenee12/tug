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

/**
 * Why the browser couldn't start the host, from its own error text. "Specified native messaging
 * host not found" means no tug, or a tug from before the installer registered the host (0.5.24 and
 * earlier), which the browser can't tell apart. "Access to the specified native messaging host is
 * forbidden" means tug is there but this copy of the extension has another ID (README › Develop).
 */
export function hostError(text) {
  if (/forbidden/i.test(text)) return new TugError("wrong_extension", text);
  if (/not found|not installed/i.test(text)) return new TugError("not_installed", text);
  return new TugError("failed", text);
}

/** Make one call; resolves with tug's answer or throws a TugError. */
export async function callTug(call) {
  if (globalThis.__tugMock) return globalThis.__tugMock(call);
  const id = nextId++;
  let reply;
  try {
    reply = await chrome.runtime.sendNativeMessage(HOST, { id, call });
  } catch (e) {
    throw hostError(String(e?.message || e));
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
