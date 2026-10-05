# tug

Your iPhone on your Windows PC: notifications, texts (read and reply), contacts and media
controls, over Bluetooth. No app on the phone, no Mac, no cloud.

tug is a Windows desktop app (Tauri 2 + Rust + Vue 3) that pairs with an iPhone the way a
smartwatch or car kit does, using the Bluetooth services Apple publishes for accessories.
**Current release: v0.5.2** — see [CHANGELOG.md](CHANGELOG.md).

| Feature | How | Status |
|---|---|---|
| Live notifications from every app, with actions (answer/decline/clear) | ANCS (Bluetooth LE) | ✅ Verified on iPhone 15 Pro Max |
| Compact feed: one row per person/app, cleared items disappear | Local | ✅ |
| Searchable history that outlives the phone's lock screen | SQLite + FTS5 | ✅ |
| Read texts, including ones read in the open chat | MAP (Classic Bluetooth, OBEX) | ✅ Verified |
| Reply and start new texts from the PC | MAP PushMessage | ✅ Verified (SMS/iMessage chosen by iOS) |
| Contact names, new message by name | PBAP | Built; needs *Sync Contacts* on the phone |
| Now playing + play/pause/skip/volume | AMS (Bluetooth LE) | ✅ Verified |
| Phone battery level | Battery Service | ✅ Verified (level only; no charging flag) |
| Windows toasts, per-app mute, do not disturb, Ctrl +/−/0 zoom | Local | ✅ |
| Incoming-call card (answer/decline, Enter/Esc) | ANCS | Built; not yet verified on hardware |
| Recent calls | PBAP call history | Built; needs *Sync Contacts*; not yet verified on hardware |
| Call from tug (audio stays on the phone) | HFP, experimental | Behind a check in Settings › iPhone; unverified |
| Group texts, photos, texts sent from the phone itself | Not exposed by iOS over Bluetooth | Out of scope |

## Requirements

- Windows 10 2004+ or Windows 11.
- A Bluetooth adapter that supports the **peripheral role** (most Bluetooth 5 adapters do).
  The Connection panel says so if yours doesn't.
- An iPhone. Tested target: iPhone 15 Pro Max.

## First-time pairing

iOS only lets an accessory connect when the phone starts the connection, so the first pairing goes
through a free BLE app. After that, iOS reconnects to the PC by itself.

1. Start tug. Leave **Visible to iPhone** on (left rail).
2. On the iPhone, install **LightBlue** (free) and open it next to the PC. Windows doesn't include a
   name in its Bluetooth LE advert, so the PC most likely appears as **Unnamed** (sometimes as the PC name),
   usually with the strongest signal.
   To confirm, switch *Visible to iPhone* off in tug and watch which entry disappears; switch it back on
   and tap that entry. (Its advert lists the tug service `6E4C3A10-9B2F-4D7A-8C1E-5A0F2B7D9C01`.)
3. Accept the pairing request on the iPhone and turn on **Share System Notifications** if iOS asks.
4. In tug's Connection panel the iPhone appears at the top as **Connected now**. Click **Pair**,
   check the code matches the one on the phone, and confirm on both.

A phone already paired through Windows Settings › Bluetooth (e.g. via Phone Link) also shows in the
list as *Paired for calls & audio*; **Use** pairs that same phone's Bluetooth LE side for you.

**iPhone switches** (Settings › Bluetooth › ⓘ next to this PC): *Share System Notifications* for the
feed, *Show Notifications* for reading and sending texts, *Sync Contacts* for names and new messages.
The first time tug asks for messages or contacts, iOS refuses once; that's what makes the switch appear.

**Message text missing?** iOS only shares what the lock screen would show. Set
Settings › Notifications › Show Previews to *Always*.

## Develop

```bash
npm install
npm run tauri dev     # the real app, with Bluetooth
npm run dev           # UI only in a browser, with sample data (src/lib/devMock.ts)
npm run check         # vue-tsc + clippy -D warnings + cargo test
npx tauri build --no-bundle   # src-tauri/target/release/tug.exe
npx tauri build               # installer: src-tauri/target/release/bundle/nsis/tug_0.1.0_x64-setup.exe
```

Browser preview URLs: `/` (connected, sample history), `/?setup` (first run), `/?pairing` (PIN dialog),
`/?call` (a call rings), `/?nodial` (the calling check fails).

Logs: `%LOCALAPPDATA%\dev.davejames.tug\logs`. History DB: `%APPDATA%\dev.davejames.tug\tug.db`.

## How it works

```
src-tauri/src/
  ancs.rs        ANCS wire protocol: parse events, build Control Point requests, reassemble Data Source fragments
  ams.rs         AMS wire protocol: register for player/track updates, parse them, remote commands
  media_keys.rs  Windows media keys and flyout (SMTC) show and control the iPhone's music
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

1. **Calls:** the incoming-call card and recent calls are built (not yet verified on hardware).
   Dialing over hands-free (HFP) is experimental: it only works if Windows doesn't already hold the
   iPhone's hands-free link, which `cargo run --example hfp_probe` checks.
2. **Connectors:** messages are stored with a `source`; next sources are Android, then apps like
   Slack, Teams, WhatsApp and Wispr Flow feeding the same inbox.
3. **PC media:** show and control what's playing on the PC (e.g. YouTube in a browser) next to the phone.
4. **Exact charging state:** iOS exposes none over BLE; needs a companion app or a USB trust pairing.
5. ANCS service-solicitation advert so the PC appears in iOS Settings without LightBlue.

## Icon

`src-tauri/icons/source.svg` is the master. Its rope geometry comes from `scripts/rope_geometry.py`.
Regenerate the PNG/ICO set with `npx tauri icon src-tauri/icons/source.svg -o src-tauri/icons`.
