# tug: project notes for Claude

Windows desktop app (Tauri 2, Rust, Vue 3 + TS + Pinia + Tailwind v4) mirroring iPhone notifications
and media controls over Bluetooth LE (ANCS/AMS). See README.md for architecture.

## Checks
- `npm run check` must pass before any push (vue-tsc, clippy `-D warnings`, cargo test).
- Bluetooth code can't be exercised without hardware. Keep protocol logic in the pure modules
  (`ancs.rs`, `ams.rs`, `store.rs`) and unit-test it there; keep `ble/` as thin I/O.
- Never claim Bluetooth behaviour was verified on a phone unless Dave reports it.

## Conventions
- The app name is **tug**, lowercase, everywhere user-facing.
- Rust serde types in `state.rs`/`store.rs`/`ams.rs` are mirrored in `src/types/protocol.ts`.
  Change both together.
- Design: warm cream canvas, coral (`primary`) only for primary actions, serif (`headline`) for
  display text, dark surfaces for device chrome. Tokens live in `src/style.css` `@theme`. Never
  inline hex in components. Custom classes must be `@utility` (Tailwind v4), not `@layer components`.
- UI work without a phone: `npm run dev` in a browser loads `src/lib/devMock.ts` (dev-only).
- Line endings are LF (`.gitattributes`).
