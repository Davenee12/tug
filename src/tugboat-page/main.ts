import { createApp } from "vue";
import "./page.css";
import TugboatPage from "./TugboatPage.vue";
import { TugboatClient, clientId, type TugboatApi } from "./client";
import { deriveKeys, randomId } from "./crypto";
import { keyFromHash, takeKey, type SecretEnv } from "./secret";

function browser(): SecretEnv {
  let storage: Storage | null = null;
  try {
    storage = window.sessionStorage;
  } catch {
    /* blocked */
  }
  return { location: window.location, history: window.history, sessionStorage: storage };
}

async function boot() {
  let api: TugboatApi | null = null;
  /** Opened from a race's Player 2 code: only ever a controller. */
  let guest = false;
  // `npm run dev:tugboat` → /?mock previews the page without a PC (never in the built page).
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
    api = (await import("./devMock")).mockApi();
    guest = new URLSearchParams(location.search).has("guest");
  } else {
    // The secret lives only in this closure (see secret.ts), never in storage or the URL. The page
    // signs and seals with whichever secret its link carried: the session's, or the Player 2 code's.
    const key = takeKey(browser());
    if (key) {
      api = new TugboatClient(deriveKeys(key.secret), clientId(randomId));
      guest = key.guest;
    }
  }
  createApp(TugboatPage, { api, guest }).mount("#app");
  // Scanning the code again in the same tab only changes the `#` (no reload). Reload with the
  // new link still in the address bar, so the fresh page takes the secret from there and
  // starts clean with it.
  window.addEventListener("hashchange", () => {
    if (keyFromHash(location.hash)) location.reload();
  });
}

void boot();
