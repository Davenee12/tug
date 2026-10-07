import { createApp } from "vue";
import "./page.css";
import TugboatPage from "./TugboatPage.vue";
import { TugboatClient, clientId, type TugboatApi } from "./client";
import { deriveKeys, randomId, unb64 } from "./crypto";

/**
 * The session secret rides in the link's `#` part, which the browser never sends over the
 * network. Keep it for this tab (so a reload still works) and take it out of the address bar.
 * The link carries nothing else: after a network change the PC simply waits for a fresh scan.
 */
function takeSecret(): Uint8Array | null {
  const secretPart = location.hash.slice(1);
  const fromHash = unb64(secretPart);
  if (fromHash && fromHash.length === 16) {
    try {
      sessionStorage.setItem("tugboat.secret", secretPart);
    } catch {
      /* private mode: this load only */
    }
    history.replaceState(null, "", location.pathname);
    return fromHash;
  }
  try {
    const stored = unb64(sessionStorage.getItem("tugboat.secret") ?? "");
    return stored && stored.length === 16 ? stored : null;
  } catch {
    return null;
  }
}

async function boot() {
  let api: TugboatApi | null = null;
  // `npm run dev:tugboat` → /?mock previews the page without a PC (never in the built page).
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
    api = (await import("./devMock")).mockApi();
  } else {
    const secret = takeSecret();
    if (secret) api = new TugboatClient(deriveKeys(secret), clientId(randomId));
  }
  createApp(TugboatPage, { api }).mount("#app");
  // Scanning the code again in the same tab only changes the `#` (no reload): take the secret
  // out of the address bar again and start fresh with it.
  window.addEventListener("hashchange", () => {
    if (location.hash.length > 1 && takeSecret()) location.reload();
  });
}

void boot();
