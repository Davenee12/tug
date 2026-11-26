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

/**
 * One call over a port held open until tug answers. For a text: it waits (up to 3½ minutes) for
 * the person to click Send on tug's card, and an open native port keeps the service worker alive
 * for that long, where a one-off message might not.
 */
export function callTugOverPort(call) {
  if (globalThis.__tugMock) return globalThis.__tugMock(call);
  return new Promise((resolve, reject) => {
    const id = nextId++;
    let port;
    try {
      port = chrome.runtime.connectNative(HOST);
    } catch (e) {
      reject(hostError(String(e?.message || e)));
      return;
    }
    let settled = false;
    const settle = (then) => {
      if (settled) return;
      settled = true;
      try {
        port.disconnect();
      } catch {
        // Already gone.
      }
      then();
    };
    port.onMessage.addListener((reply) => {
      if (!reply || typeof reply !== "object" || reply.id !== id) return;
      settle(() => (reply.error ? reject(new TugError(reply.error.code, reply.error.message)) : resolve(reply.ok)));
    });
    port.onDisconnect.addListener(() => {
      const text = chrome.runtime.lastError?.message || "tug closed before it answered.";
      settle(() => reject(hostError(text)));
    });
    port.postMessage({ id, call });
  });
}

export const tug = {
  status: () => callTug({ tool: "status" }),
  latestCode: (copy = false) => callTug({ tool: "get_latest_code", copy }),
  search: (query, limit = 8) => callTug({ tool: "search_messages", query, limit }),
  media: (action) => callTug({ tool: "media_control", action }),
  // Always through tug's card: the host takes out anything that could skip it.
  sendText: (to, message) => callTugOverPort({ tool: "send_text", to, message }),
};
