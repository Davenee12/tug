# tug

Your iPhone on your Windows PC: notifications, texts (read and reply), contacts and media
controls, over Bluetooth. No app on the phone, no Mac, no cloud.

tug is a Windows desktop app (Tauri 2 + Rust + Vue 3) that pairs with an iPhone the way a
smartwatch or car kit does, using the Bluetooth services Apple publishes for accessories.
**Current release: v0.5.11** — see [CHANGELOG.md](CHANGELOG.md).

| Feature | How | Status |
|---|---|---|
| Live notifications from every app, with actions (answer/decline/clear) | ANCS (Bluetooth LE) | ✅ Verified on iPhone 15 Pro Max |
| Compact feed, real app icons, open a notification's web page | Local + App Store lookup | ✅ |
| Search everything with Ctrl+K: people, texts, notifications, plus actions (`text zoe …`, `play …`) | SQLite + FTS5 | ✅ |
| Read texts the moment they arrive, reply and start new ones | MAP + MNS (Classic Bluetooth, OBEX) | ✅ Verified |
| Verification codes from texts and notifications, with Copy code | Local | ✅ Verified |
| Contact names and photos | PBAP | ✅ Verified (needs *Sync Contacts*) |
| Incoming-call card, recent calls, call back a missed call | ANCS + PBAP | ✅ Verified |
| Now playing, play/pause/skip/±15 s/volume, Windows media keys | AMS (Bluetooth LE) + SMTC | ✅ Verified |
| Spotify: playlists, search, queue, Like, albums, artists, Play on | Spotify Web API (Settings › Connectors) | ✅ Built; needs Spotify Premium |
| Phone battery level and low-battery alert | Battery Service | ✅ Verified (level only; no charging flag) |
| Windows pop-ups you can reply from, quiet hours, app mute, VIPs | Local | ✅ |
| Weather on the Feed | Open-Meteo | ✅ |
| Group texts, photos/attachments, texts sent from the phone itself, iMessage vs SMS | Not exposed by iOS over Bluetooth | Out of scope |

## Requirements

- Windows 10 2004+ or Windows 11.
- A Bluetooth adapter that supports the **peripheral role** (most Bluetooth 5 adapters do).
  tug says so if yours doesn't.
- An iPhone. Tested on an iPhone 15 Pro Max and an iPhone X.

## Connecting your iPhone

tug opens with a **Connect your iPhone** panel (also in Settings › iPhone):

1. On the iPhone, open **Settings › Bluetooth** and keep it open. tug lists your phone by name.
2. Click **Pair**, check the code in tug matches the phone, and confirm. That one pairing brings
   notifications, music, battery and texts. No extra apps needed.
3. Tap **Allow** on the iPhone when it asks, then turn on the three switches tug shows
   (Settings › Bluetooth › ⓘ next to this PC): *Share System Notifications* for the Feed,
   *Show Notifications* for texts, *Sync Contacts* for names, photos and calls. tug turns each one
   green as it comes on.

Tapping the PC in the iPhone's Bluetooth list, or Windows' "Add a device", also works.

**What iOS allows over Bluetooth:**
- Only the 10 newest incoming texts come over on a fresh install, and texts sent from the phone
  are never shared. Everything after that is kept in tug's history.
- After the iPhone restarts, nothing connects until it's unlocked once. tug shows
  "Unlock your iPhone" and reconnects by itself.
- Message text missing? iOS only shares what the lock screen shows. Set
  Settings › Notifications › Show Previews to *Always*.

## Develop

```bash
npm install
npm run tauri dev     # the real app, with Bluetooth
npm run dev           # UI only in a browser, with sample data (src/lib/devMock.ts)
npm run check         # vue-tsc + vitest + cargo fmt + clippy -D warnings + cargo test
npm run build:release -- --no-bundle   # src-tauri/target/release/tug.exe
npm run build:release                  # installer: src-tauri/target/release/bundle/nsis/tug_<version>_x64-setup.exe
```

Build releases with `npm run build:release` rather than a bare `npx tauri build`: it wraps the build
in `scripts/remap-paths.mjs`, which remaps local paths (your user folder, Cargo's registry, the repo)
so the exe doesn't carry your Windows user name.

Browser preview URLs: `/` (connected, sample history), `/?setup` (first run), `/?pairing` (PIN dialog),
`/?call` (a call rings), `/?whatsnew` (the What's new card), `/?applemusic`, `/?spotifyoff`,
`/?btoff`. See `src/lib/devMock.ts` for the rest.

Logs: `%LOCALAPPDATA%\dev.davejames.tug\logs`. History DB: `%APPDATA%\dev.davejames.tug\tug.db`.

## How it works

```
src-tauri/src/
  ancs.rs, ancs_queue.rs  ANCS wire protocol and the one-request-at-a-time detail queue
  ams.rs          AMS wire protocol: player/track updates and remote commands
  ble/actor/      The Bluetooth actor: advertising, discovery, pairing, GATT session, reconnects
                  (link.rs: connect/backoff/blip handling; notifications.rs; media.rs; pairing.rs)
  ble/winrt.rs    Helpers over Windows.Devices.Bluetooth; every await is time-limited
  map/            Texts, contacts and calls over Classic Bluetooth (OBEX, MAP, MNS, PBAP)
  spotify/        Spotify connector (sign-in, Web API, rate-limit backoff)
  media_keys.rs   Windows media keys and flyout (SMTC) for the iPhone's music
  toast/, tray.rs Windows pop-ups with actions; tray icon
  store.rs        SQLite history (FTS5 search), settings; versioned migrations
  state.rs        Status/now-playing shared with the UI; event names
  commands.rs     Tauri commands called from the UI
  cache_trim.rs, diagnostics.rs, frontend_log.rs, webview_watch.rs  upkeep and crash logging
src/
  stores/tug.ts   Pinia store: all UI state, backend events, Windows toasts
  components/     DeviceRail, FeedPanel, MessageThreads, CallsPanel, NowPlayingCard, Settings…
  lib/whatsNew.ts End-user release notes shown once after each update
  types/protocol.ts  Mirrors the Rust serde types; keep the two in sync
```

**Connection model.** The PC advertises a connectable GATT service and the iPhone connects to it, so
the PC is the GAP *peripheral*. Over that same link the PC is the GATT *client* of the iPhone's
ANCS, AMS and Battery services. `GattSession.MaintainConnection` asks Windows to re-establish the
link whenever the phone comes back in range. All Bluetooth state lives in one actor on its own
thread, so nothing about the link is shared or locked. Each GATT operation (discovery, reads,
writes, subscribing) and opening the device run on a short-lived helper thread that the actor awaits
with a time limit, because Windows can block the call that *starts* an operation for seconds when
the adapter stalls (a few quick calls, such as closing the device, still run on the actor). Retries
back off progressively (to 2 minutes while the phone is locked after a restart), a sub-second link
blip keeps the session, an adapter that stops answering is given a few seconds and then reconnected,
and waking the PC from sleep reconnects at once (a heartbeat on its own thread goes by Windows'
sleep-aware clocks and its resume notification, so a stalled thread can't pass for a wake).

**ANCS details that matter.** Subscribe to Data Source *before* Notification Source. Send one Control
Point request at a time and reassemble its fragmented response. Notification UIDs are only valid within
one connection, so each subscription gets a session id. When iOS replays the notifications still on the
lock screen (flagged pre-existing) on reconnect, they're matched to existing history rows instead of
duplicated.

## Roadmap

See [ROADMAP.md](ROADMAP.md). Every fifth version (v0.5.10, v0.5.15, …) is bug fixing only.
Next up:
1. **v0.5.11 — tug Drop:** send photos, files and text between the iPhone and the PC over Wi-Fi,
   with no app on the phone: scan a QR code in tug and a tug page opens in Safari.
2. **Companion apps** (planned): a tug protocol in Rust, then Android and iPhone apps for clipboard,
   files and full message history. See [docs/COMPANION-PLAN.md](docs/COMPANION-PLAN.md).

## Icon

`src-tauri/icons/source.svg` is the master. Its rope geometry comes from `scripts/rope_geometry.py`.
Regenerate the PNG/ICO set with `npx tauri icon src-tauri/icons/source.svg -o src-tauri/icons`.
