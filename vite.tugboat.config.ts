import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

// The Tugboat phone page: a separate small bundle that tug serves to the phone over the LAN.
// `npm run build` builds it into src-tauri/tugboat-page/dist, which build.rs embeds in the binary.
// `npm run dev:tugboat` previews it at http://localhost:1430/?mock without a PC.
export default defineConfig({
  root: fileURLToPath(new URL("./src/tugboat-page", import.meta.url)),
  base: "/",
  plugins: [vue(), tailwindcss()],
  // No public folder: everything the page needs is bundled.
  publicDir: false,
  server: { port: 1430, strictPort: true },
  build: {
    outDir: fileURLToPath(new URL("./src-tauri/tugboat-page/dist", import.meta.url)),
    emptyOutDir: true,
    // iOS 15+ Safari; keeps the bundle modern and small.
    target: "safari15",
    assetsInlineLimit: 0,
    reportCompressedSize: false,
  },
});
