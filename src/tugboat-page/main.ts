import { createApp } from "vue";
import "./page.css";
import TugboatPage from "./TugboatPage.vue";
import { TugboatClient, clientId, type TugboatApi } from "./client";
import { deriveKeys, randomId } from "./crypto";
import { secretFromHash, takeSecret, type SecretEnv } from "./secret";

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
  // `npm run dev:tugboat` → /?mock previews the page without a PC (never in the built page).
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
    api = (await import("./devMock")).mockApi();
  } else {
    // The secret lives only in this closure (see secret.ts), never in storage or the URL.
    const secret = takeSecret(browser());
    if (secret) api = new TugboatClient(deriveKeys(secret), clientId(randomId));
  }
  createApp(TugboatPage, { api }).mount("#app");
  // Scanning the code again in the same tab only changes the `#` (no reload). Reload with the
  // new link still in the address bar, so the fresh page takes the secret from there and
  // starts clean with it.
  window.addEventListener("hashchange", () => {
    if (secretFromHash(location.hash)) location.reload();
  });
}

void boot();
