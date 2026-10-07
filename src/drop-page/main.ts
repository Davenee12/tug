import { createApp } from "vue";
import "./page.css";
import DropPage from "./DropPage.vue";
import { DropClient, adoptClientId, clientId, type DropApi } from "./client";
import { deriveKeys, randomId, unb64 } from "./crypto";

/**
 * The session secret rides in the link's `#` part, which the browser never sends over the
 * network. Keep it for this tab (so a reload still works) and take it out of the address bar.
 * After a network change the PC adds this phone's client id (`#<secret>.<client>`), so the phone
 * picks up where it was even though the new address is a new origin with empty storage.
 */
function takeSecret(): Uint8Array | null {
  const [secretPart, client] = location.hash.slice(1).split(".");
  const fromHash = unb64(secretPart ?? "");
  if (fromHash && fromHash.length === 16) {
    try {
      sessionStorage.setItem("tugdrop.secret", secretPart);
    } catch {
      /* private mode: this load only */
    }
    if (client) adoptClientId(client);
    history.replaceState(null, "", location.pathname);
    return fromHash;
  }
  try {
    const stored = unb64(sessionStorage.getItem("tugdrop.secret") ?? "");
    return stored && stored.length === 16 ? stored : null;
  } catch {
    return null;
  }
}

async function boot() {
  let api: DropApi | null = null;
  // `npm run dev:drop` → /?mock previews the page without a PC (never in the built page).
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
    api = (await import("./devMock")).mockApi();
  } else {
    const secret = takeSecret();
    if (secret) api = new DropClient(deriveKeys(secret), clientId(randomId));
  }
  createApp(DropPage, { api }).mount("#app");
}

void boot();
