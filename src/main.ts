import { createApp } from "vue";
import { createPinia } from "pinia";
import "@fontsource/eb-garamond/400.css";
import "@fontsource/eb-garamond/500.css";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "./style.css";
import App from "./App.vue";
import { install as installErrorReporting } from "./lib/errorReport";
import { invoke } from "@tauri-apps/api/core";
import { setPcRegion } from "./lib/address";

async function boot() {
  // Plain-browser dev preview: stub the Rust backend (tree-shaken from builds).
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    await import("./lib/devMock");
  }
  // Read numbers typed without a country code the way Windows' Region setting says (as the Rust
  // side does), before anything normalises an address.
  await invoke<string | null>("pc_region")
    .then(setPcRegion)
    .catch(() => undefined);
  const app = createApp(App).use(createPinia());
  // Forward uncaught errors to the Rust log so a window crash is diagnosable (no-op in the browser).
  installErrorReporting(app);
  app.mount("#app");
}

void boot();
