# tug

Your iPhone's notifications and media controls on your Windows PC, over Bluetooth LE.
No app on the phone, no Mac, no cloud.

tug is a Windows desktop app (Tauri 2 + Rust + Vue 3) that pairs with an iPhone the way a
smartwatch or car kit does. It uses the Bluetooth services Apple publishes for accessories:

| Feature | How | Status |
|---|---|---|
| Live notifications from every app | Apple Notification Center Service (ANCS) | Built |
| Notification actions (answer/decline, clear) | ANCS Control Point | Built |
| Searchable history that outlives the phone's lock screen | Local SQLite + FTS5 | Built |
| Message threads (incoming, grouped by sender) | ANCS notifications from chat apps | Built |
| Now playing + play/pause/skip/volume | Apple Media Service (AMS) | Built |
| Phone battery level | Bluetooth Battery Service | Built (if iOS exposes it) |
| Windows toasts, per-app mute, do not disturb | Tauri notification plugin | Built |
| Replying to texts | Bluetooth MAP (Classic, OBEX) | Not yet |
| Contacts, calls | PBAP, HFP | Not yet |
| Full iMessage history, groups, attachments | Needs an always-on Mac relay | Out of scope |
| Clipboard, files | Needs an iOS companion app | Out of scope |

## Requirements

- Windows 10 2004+ or Windows 11.
- A Bluetooth adapter that supports the **peripheral role** (most Bluetooth 5 adapters do).
  The Connection panel says so if yours doesn't.
- An iPhone. Tested target: iPhone 15 Pro Max.

## First-time pairing

iOS only lets an accessory connect when the phone starts the connection, so the first pairing goes
through a free BLE app. After that, iOS reconnects to the PC by itself.

1. Start tug. Leave **Visible to iPhone** on (left rail).
2. On the iPhone, install **LightBlue** (free), open it, and tap the entry for this PC. It advertises
   the tug service `6E4C3A10-9B2F-4D7A-8C1E-5A0F2B7D9C01`.
3. Accept the pairing request on the iPhone and turn on **Share System Notifications** if iOS asks.
4. In tug's Connection panel the iPhone appears at the top as **Connected now**. Click **Pair**,
   check the code matches the one on the phone, and confirm on both.

A phone already paired through Windows Settings › Bluetooth also shows in the list; **Use** tries its
Bluetooth LE side.

**Message text missing?** iOS only shares what the lock screen would show. Set
Settings › Notifications › Show Previews to *Always*.

## Develop

```bash
npm install
npm run tauri dev     # the real app, with Bluetooth
npm run dev           # UI only in a browser, with sample data (src/lib/devMock.ts)
npm run check         # vue-tsc + clippy -D warnings + cargo test
npx tauri build --no-bundle   # src-tauri/target/release/tug.exe
```

Browser preview URLs: `/` (connected, sample history), `/?setup` (first run), `/?pairing` (PIN dialog).

Logs: `%LOCALAPPDATA%\dev.davejames.tug\logs`. History DB: `%APPDATA%\dev.davejames.tug\tug.db`.

## How it works

```
src-tauri/src/
  ancs.rs        ANCS wire protocol: parse events, build Control Point requests, reassemble Data Source fragments
  ams.rs         AMS wire protocol: register for player/track updates, parse them, remote commands
  store.rs       SQLite history (FTS5 search), app names, settings
  state.rs       Status/now-playing shared with the UI; event names
  commands.rs    Tauri commands called from the UI
  ble/actor.rs   The Bluetooth actor: advertising, discovery, pairing, GATT session, reconnects
  ble/winrt.rs   Helpers over Windows.Devices.Bluetooth (GATT read/write/subscribe)
src/
  stores/tug.ts  Pinia store: all UI state, backend events, Windows toasts
  components/    DeviceRail, FeedPanel, MessageThreads, ConnectionPanel, PairingDialog, …
  types/protocol.ts  Mirrors the Rust serde types; keep the two in sync
```

**Connection model.** The PC advertises a connectable GATT service and the iPhone connects to it,
so the PC is the GAP *peripheral*. Over that same link the PC is the GATT *client* of the iPhone's
ANCS, AMS and Battery services. `GattSession.MaintainConnection` asks Windows to re-establish the link
whenever the phone comes back in range. All Bluetooth work runs in one actor on its own thread,
because WinRT async types aren't `Send`.

**ANCS details that matter.** Subscribe to Data Source *before* Notification Source. Send one Control
Point request at a time and reassemble its fragmented response. Notification UIDs are only valid within
one connection, so each subscription gets a session id. When iOS replays the notifications still on the
lock screen (flagged pre-existing) on reconnect, they're matched to existing history rows instead of
duplicated.

## Roadmap

1. Hardware validation on the iPhone 15 Pro Max (pairing flow, reconnect, battery service).
2. ANCS service-solicitation advert so the PC appears in iOS Settings › Bluetooth without LightBlue
   (needs testing: Windows may reject the advert type).
3. Bluetooth MAP client (SDP + RFCOMM + OBEX) for 1:1 replies and recent message bodies.
4. PBAP contacts; HFP calls.

## Icon

`src-tauri/icons/source.svg` is the master. Its rope geometry comes from `scripts/rope_geometry.py`.
Regenerate the PNG/ICO set with `npx tauri icon src-tauri/icons/source.svg -o src-tauri/icons`.
