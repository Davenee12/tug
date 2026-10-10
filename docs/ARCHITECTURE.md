# tug architecture

A guide for contributors: how tug is put together, where data flows, where the testable logic
lives, and how to change things safely without a phone on your desk. For what tug does, see
[FEATURES.md](FEATURES.md); for the developer bridge in depth, see [DEVELOPERS.md](DEVELOPERS.md).

## The big picture

tug is a **Tauri 2** app: a Rust backend (`src-tauri/`) and a Vue 3 + TypeScript + Pinia +
Tailwind v4 UI (`src/`) in a WebView2 window. It talks to the iPhone the way a smartwatch or car kit
does, over the Bluetooth services Apple publishes for accessories. Nothing runs on the phone and
there is no tug server.

| Phone service | Transport | What tug gets | Code |
|---|---|---|---|
| ANCS (Apple Notification Center Service) | Bluetooth LE GATT | Notifications, their actions (answer/decline/clear/dial) | `ancs.rs`, `ancs_queue.rs`, `ble/actor/notifications.rs` |
| AMS (Apple Media Service) | Bluetooth LE GATT | Now Playing and remote commands | `ams.rs`, `ble/actor/media.rs` |
| Battery Service, Device Information | Bluetooth LE GATT | Battery level, model | `ble/actor/link.rs`, `device_info.rs` |
| MAP + MNS (Message Access / Notification) | Classic Bluetooth, OBEX over RFCOMM | Texts in, texts out, live text events | `map/` |
| PBAP (Phonebook Access) | Classic Bluetooth, OBEX | Contacts, photos, recent calls | `map/vcard.rs`, `map/calls.rs`, `map/service.rs` |
| HFP (hands-free, experimental) | Classic Bluetooth | Calls check / dial | `hfp/` |

**Connection model.** The PC advertises a connectable GATT service and the iPhone connects to it,
so the PC is the GAP *peripheral*. Over that same link the PC is the GATT *client* of the iPhone's
ANCS, AMS and Battery services. One pairing from the iPhone's Bluetooth settings (or tug's Pair)
creates the Classic bond, and Windows derives the LE bond from it (cross-transport key
derivation), so notifications, music, battery and texts all follow from one pairing.

## Processes and threads

Two programs ship: **`tug.exe`** (the app) and **`bin\tug.exe`** (the `tug` command and MCP server,
built from `src-tauri/crates/tug-cli`; AI tools start it as `tug mcp`).

Inside `tug.exe`:

```
                    ┌────────────────────────── WebView2 (Vue UI, src/) ──────────────────────────┐
                    │  stores/tug.ts · stores/devtools.ts · stores/tugboat.ts · stores/weather.ts  │
                    │  (weather: Open-Meteo fetched from the webview every 15 min)                 │
                    └───────────▲ Tauri events (device-status, notification, message, …)  │ invoke ─┘
                                │                                                         ▼
  ┌──────────────┐  Shared (Arc: status, now playing, calls, store, MapHandle) ┌──────────────────────┐
  │ tug-ble      │◄────────────── commands.rs (92 Tauri commands) ────────────►│ tug-map              │
  │ BLE actor    │  BleHandle (mpsc + oneshot replies)    MapHandle (mpsc)     │ MAP worker           │
  │ 1 s tick     │                                                             │ MAP/MNS/PBAP/HFP     │
  └──┬───────┬───┘                                                             └──┬───────────────────┘
     │       │ tug-gatt-op helpers (one per GATT call, ≤16, time-limited)        │ MNS server (WinRT
     │       └ tug-wake-watch (sleep/resume heartbeat)                           │ listener → worker)
     │                                                                           └ tug-photos (daily)
  tug-media-keys (Windows media flyout, SMTC) ──► BleHandle
  devtools bridge (named-pipe accept loop, async task) ──► DevTools ──► MapHandle / BleHandle / store
  Tugboat (axum on the LAN address, only while the panel is open)
```

| Thread / task | Where | What it does |
|---|---|---|
| **`tug-ble`**, the Bluetooth actor | `ble/mod.rs`, `ble/actor/mod.rs` | Owns *all* Bluetooth LE state on its own OS thread (a current-thread runtime and a `LocalSet`, because WinRT types aren't `Send`). One `select!` over UI commands, WinRT events and a 1 s tick (retries, subscription checks, ANCS timeouts, sweeps). Nothing about the link is shared or locked. |
| `tug-gatt-op` helpers | `ble/winrt.rs` | Windows can block the call that *starts* a GATT operation for seconds when the adapter stalls, so each GATT operation runs on a short-lived helper thread that the actor awaits with a time limit (10 s, 30 s for discovery, 90 s for subscribing; at most 16 at once). |
| `tug-wake-watch` | `ble/actor/heartbeat.rs` (logic in `wake.rs`) | Compares the sleep-inclusive and awake-only clocks plus Windows' resume notification, so a real sleep is told apart from a stalled thread; sends `Woke` to the actor. |
| **`tug-map`**, the MAP worker | `map/service.rs` | Own thread and runtime. Texts in and out, live text events, contacts and calls (PBAP), HFP. Polls every 2–30 s depending on state; live events from the MNS server arrive on the same loop. |
| MNS server | `map/mns.rs` | The phone connects to tug's RFCOMM server to report new texts and send results; connections are served on the MAP worker's runtime. |
| `tug-photos` | `map/service.rs` | Contact photos, at most once a day, serialized with other PBAP users. |
| `tug-media-keys` | `media_keys.rs` | Keeps the Windows media flyout in step with Now Playing; flyout buttons and media keys send commands to the actor. |
| Developer bridge | `devtools/mod.rs`, `crates/tug-bridge/src/server.rs` | A named-pipe accept loop for the app's lifetime (at most 8 connections); answers "off" while switched off. |
| Tugboat | `tugboat/` | An HTTP server, a 2 s supervisor (idle stop after 10 minutes, network changes) and per-connection tasks; exists only while the panel is open (or Tugboat Run asked for the phone as a controller). While the game's controller channel is open, a 150 ms watch notices a phone that went quiet. |
| Toasts, tray | `toast/`, `tray.rs` | Toast activations arrive on a pool thread and are handed to the async runtime; the tray runs on the main thread. |
| Spotify, location | `commands.rs`, `spotify/`, `location.rs` | No background task: each command runs on `spawn_blocking`. The UI reads Spotify's player when the song changes. |

**Talking between them.** `BleHandle` and `MapHandle` are unbounded mpsc senders of commands, with
oneshot replies. `Shared` (`state.rs`) is the one `Arc` everything holds; `update_status` and
`update_now_playing` emit to the UI only when something changed.

**Events to the UI** (Rust → Vue): `device-status`, `now-playing`, `notification`,
`notification-removed`, `app-name`, `discovered-devices`, `pairing-request`,
`pairing-request-closed`, `message`, `contacts`, `calls`, `open-latest-conversation`,
`open-settings`, `toast-pressed` (`state.rs`); `tugboat-status`, `tugboat-text`, `tugboat-dropped`,
`game-pad` (`tugboat/mod.rs`); `devtools-status`, `devtools-confirm` (`devtools/mod.rs`). The typed listener
map is `EventPayloads` in `src/lib/ipc.ts`.

## Startup

`src-tauri/src/lib.rs`: plugins (single-instance first, so a second launch just shows the window;
autostart with `--minimized`; log with rotation; notification; dialog) → create the data folder →
open the SQLite store and migrate → `Shared` → start the BLE actor → start the MAP worker → media
keys → Spotify → Tugboat service (cleans partial uploads) → developer bridge → register state → a
one-off cache trim → tray → WebView2 crash watch → show the window unless started minimized.
Closing the window hides it to the tray (unless the user turned that off); on exit tug withdraws its
toasts and stops Tugboat.

## Data flows

**A notification arrives (ANCS)**

```
iPhone ─ANCS Notification Source─► WinRT handler ─Event─► BLE actor
  ancs::parse_notification_source ─► ancs_queue (one Control Point request at a time)
  ─► Data Source chunks ─► ancs::Reassembler ─► store.upsert_notification (fresh?)
  ─► emit "notification" ─► stores/tug.ts ─► lib/popup + lib/senders + quiet hours/VIPs
  ─► lib/toastSpec ─► invoke show_toast ─► toast/xml.rs ─► Windows toast
  (Messages app? also map.refresh() so the text itself is fetched)
```

Subscribe to Data Source *before* Notification Source. ANCS UIDs are only valid within one
connection, so each subscription gets a session id. After a reconnect iOS replays what's on the lock
screen flagged pre-existing; tug matches those to existing rows instead of duplicating, and pops up
only replays that are new to tug and arrived during the gap (`lib/reconnectPopups.ts`).

**A text arrives (MAP/MNS)**

```
iPhone ─MNS event report─► MNS server ─► MAP worker: mns::event_action ─► sync()
  update inbox ─► list (map/listing.rs) ─► get each new message (map/bmessage.rs)
  ─► store.insert_incoming (messages.rs) ─► emit "message" ─► learn names ─► emit "contacts"
```

The ordinary text pop-up comes from the Messages *notification* (ANCS); the MAP copy fills the
conversation. iOS lists only about 10 inbox texts, so after a long gap tug flags the oldest new text
(`gap_before`) and the thread says earlier texts may only be on the phone.

**Sending a text**

```
composer ─► invoke send_message ─► map::service::send_text
  store.insert_outgoing (pending) + emit "message"      ← shows at once, even if the worker is busy
  ─► MapCommand::Deliver(row id) ─► worker pushes the bMessage over OBEX
  ─► accepted / unconfirmed ("May have sent") / failed (Retry) ─► emit "message"
  ─► later MNS SendingSuccess/Failure ─► mns::choose_outgoing settles it
```

**A media command**

```
Now Playing / Ctrl+K / Windows media keys / MCP media_control
  ─► Command::Media ─► BLE actor ─► ams::CommandGate (drop a duplicate queued behind a stall)
  ─► AMS Remote Command write
Spotify-specific (seek, like, shuffle, repeat, search, Play on) ─► Spotify Web API (spotify/)
```

**An MCP tool call**

```
AI tool ─stdio─► bin\tug.exe mcp (rmcp) ─named pipe─► tug-bridge server
  hello → "off"? → challenge/proof (HMAC over nonces) → call
  ─► DevTools::handle: switch on? rate limit ok? ─► devtools/tools.rs ─► store / Shared / MapHandle
send_text: recipient::resolve ─► confirm.rs opens a card ─► emit "devtools-confirm"
  ─► ToolConfirmCard.vue (Send armed after 1.5 s, window focused) ─► devtools_confirm
  ─► Approved? map::service::send_text (same path as above) ─► result back over the pipe
```

**A Tugboat upload**

```
QR: http://<LAN IPv4>:<port>/#<secret>   (the secret after # is never sent)
phone page ─POST /api/up─► PUT chunks (XChaCha20-Poly1305, keys from HKDF of the secret)
  every request MAC'd (first phone binds, replays refused) ─► tugboat/session.rs
  ─► finish: sanitized name, " (2)" on collisions ─► Pictures\Tugboat + Mark-of-the-Web
  ─► emit "tugboat-status"
```

**A game controller input (Tugboat Run)**

```
game: "Use your phone as a controller" ─► invoke game_pad_open ─► Tugboat starts if off; Session::pad_open
phone page polls GET /api/state ─► { game: true } ─► Controller.vue (pad + Boost)
  POST /api/pad, ~30/s while the input changes, a heartbeat every 200 ms, one in flight, newest wins
  signed like every request (same secret, MAC, replay window, first phone binds); body sealed to it
  ─► authorize ─► channel open? (else 409 no-game) ─► rate limit (else 429 busy) ─► ≤ 104-byte body
  ─► pad::decode: exactly {"steer": -100..=100, "boost": bool} ─► older sequence numbers ignored
  ─► Pad state changed? emit "game-pad" ─► TugboatRun.vue (the loop reads the latest state)
  reply (sealed): { paused, hits } from game_pad_feedback, so the phone shows Paused and buzzes
150 ms watch: no input for 700 ms ─► emit "game-pad" { connected: false } (the keyboard has it)
game closes ─► game_pad_close ─► inputs get 409 ─► the page goes back to Tugboat
```

The input channel is a small POST endpoint rather than a WebSocket: it reuses Tugboat's
authentication, sealing, connection limits and timeouts unchanged, needs no new dependency or
connection upgrade, and on a home network one request per input (kept alive) is well inside the
100 ms target. An input can only change the one `PadInput` the game reads.

## Where the testable logic lives

Bluetooth can't run in CI, so decisions live in **pure modules** that take plain values and return
plain values; `ble/`, `map/session.rs`, `toast/native.rs` and the other WinRT code stay thin I/O
that calls them. Every module below has a `#[cfg(test)]` suite.

| Module | What it decides |
|---|---|
| `ancs.rs`, `ancs_queue.rs` | ANCS wire format and reassembly; one-request-at-a-time queue with retries and timeouts |
| `ams.rs` | AMS wire format, `NowPlaying`, `CommandGate` (one press, one command) |
| `store.rs`, `messages.rs` | SQLite + FTS5 history, settings, migrations; conversations, contacts, name learning, send states |
| `link_policy.rs` | What a failed connect means, the sticky "away" state, waiting for link-up, keep-or-relink after wake, when to ask for a re-pair |
| `wedge.rs` | An adapter that's connected but not answering: relink with backoff and a cap |
| `wake.rs` | A real sleep vs a stalled thread, from two clocks and the resume flag |
| `map/contacts_watch.rs` | When to pull the phonebook; when an empty phonebook means Sync Contacts is off |
| `map/obex.rs`, `bmessage.rs`, `listing.rs`, `vcard.rs`, `mns_event.rs`, `mns.rs`, `pick.rs` | OBEX framing, MAP/PBAP parsers, MNS event handling, which paired device is the iPhone |
| `devtools/confirm.rs`, `rate_limit.rs`, `recipient.rs`, `settings.rs` | The send-confirmation state machine, per-tool limits, name → contact, switch defaults |
| `crates/tug-bridge` (`auth`, `protocol`, `framing`, `paths`, `time`) | Bridge protocol and handshake; the whole exchange is also tested over a real pipe |
| `crates/tug-cli` (`args`, `format`, `mcp`) | Command parsing, terminal output, MCP tool mapping |
| `tugboat/crypto.rs`, `names.rs`, `auth.rs`, `upload.rs` | Tugboat keys and sealing (a test vector shared with the phone page), safe file names, request MACs, chunk bookkeeping |
| `tugboat/pad.rs` | The game controller channel: what an input may say, the rate limit, newest-input ordering, when a quiet phone counts as gone |
| `codes.rs` | One-time codes; a port of `src/lib/codes.ts`, both tested against `src/lib/codes.cases.json` |
| `spotify/model.rs`, `toast/xml.rs`, `cache_trim.rs`, `diagnostics.rs` | Spotify JSON and device choice; toast XML and actions; cache caps; diagnostics redaction |

Frontend logic lives in **`src/lib/*.ts`**, each with a Vitest `*.test.ts` beside it: `attention`
(is the window really visible), `battery`, `coalesce` (one render per frame for bursts of
updates), `codeFeed`, `codes`, `commands` (Ctrl+K verbs),
`connectFlow`, `connectionPanel`, `connectionStatus` (one wording for the link everywhere),
`devtools`, `errorReport`, `escape`, `format` (conversation grouping and display), `health`,
`locating`, `media` (which controls show, skip routing), `messageSync`, `messageType`, `pairings`,
`permission`, `phoneModel`, `phoneSwitches` (the three iPhone switches), `playback` (Spotify Connect
titles and devices), `popup` (pop-up policy), `reconnectPopups`, `scroll`, `senders`
(unknown-sender filter), `spotify`, `stableList` (regrouped lists keep unchanged rows, so only
they re-render), `threadWindow` (long conversations render their newest texts first),
`toastLimiter`, `toastSpec`, `tugboat`, `tugboatRun` (the game's rules: seeded spawning, collisions,
scoring, the speed curve, inertia, merging the controls), `vips`, `weather`, `weblinks`, `whatsNew`.
The phone page has its own: `tugboat-page/chunks`, `crypto` and `pad` (the controller's wire format,
pacing and errors). Components and
stores should call these rather than grow logic of their own.

Integration tests: `tugboat/tests.rs` plays the phone over a real socket; `tug-bridge` tests run the
handshake over a real named pipe (including a low-integrity impostor). Hardware probes live in
`src-tauri/examples/` (`map_probe`, `hfp_probe`, `toast_probe`; `cargo run --example …`).

## Rust ↔ TypeScript: `src/types/protocol.ts`

Everything the backend sends to the UI or takes from it is a serde type in Rust **mirrored by hand**
in `src/types/protocol.ts`: `state.rs`, `store.rs`, `ams.rs`, `ancs.rs`, `messages.rs`,
`map/calls.rs`, `map/health.rs`, `toast/`, `spotify/`, `tugboat/`, `bt_inventory/` and `devtools/`.

- App-facing types use `#[serde(rename_all = "camelCase")]` (fields and enum variants), so
  `contacts_off` in Rust is `contactsOff` in TypeScript.
- **Change both sides in the same commit.** No test compares them automatically; a few Rust tests
  pin JSON shapes (`toast/mod.rs`), and the PR checklist asks.
- The developer bridge's wire protocol (`crates/tug-bridge/src/protocol.rs`) is separate: it uses
  `snake_case`, is versioned, and isn't mirrored in TypeScript.

## Storage

| What | Where |
|---|---|
| History and settings (SQLite, FTS5) | `%APPDATA%\<app id>\tug.db` |
| App icons, Spotify art, contact photos | `%APPDATA%\<app id>\` (`icons/` 50 MB cap, `spotify_art/` 100 MB cap, `contacts/`) |
| Logs (2 MB × 5, rotated) | `%LOCALAPPDATA%\<app id>\logs` |
| Bridge token, half-received Tugboat files | `%LOCALAPPDATA%\<app id>\` |
| Files received with Tugboat | `Pictures\Tugboat` |
| Spotify refresh token | Windows Credential Manager |

`<app id>` is the `identifier` in `src-tauri/tauri.conf.json`. The schema is a baseline plus an
append-only `MIGRATIONS` list in `store.rs`, tracked with `PRAGMA user_version`; each migration runs
in its own transaction, and a database newer than the build is left alone.

## Working without a phone

`npm run dev` opens the UI in a browser at `http://localhost:1420` with `src/lib/devMock.ts`
standing in for the backend (dev builds only, never bundled). Add a scenario to the URL:

| URL | Shows |
|---|---|
| `/` | Connected iPhone with sample history |
| `/?setup` | First run: the Connect panel, scripted end to end (add `&btoff` to start with Bluetooth off) |
| `/?leftover` | An old Windows pairing offered with Use / Remove |
| `/?latephone` | The iPhone appears after a rescan |
| `/?forgotten` | "Your iPhone has forgotten this PC" |
| `/?justpaired`, `/?contactsoff` (`&flip`), `/?textsoff`, `/?textsmissing`, `/?radiooff` | Setup and switch states |
| `/?nudge` | Texts and contacts off: the Feed's "Get more from tug" nudge |
| `/?lasterror`, `/?textsbroken`, `/?livetexts` | Connection health states |
| `/?away` | The phone out of range, last model kept |
| `/?model=iPhone12,8`, `/?nomodel` | A specific phone model / none reported |
| `/?pairing` | The PIN confirmation dialog |
| `/?call` | A call rings after 1.5 s (and becomes a missed call) |
| `/?popupreply` (`&missed`) | A text answered from the Windows pop-up (with its events dropped) |
| `/?nodial` | Settings › iPhone › Calls check fails |
| `/?applemusic`, `/?norepeat`, `/?repeatignored` | Apple Music controls and repeat edge cases |
| `/?speaker`, `/?spotifyoff` | Spotify on a Connect speaker / Spotify not connected |
| `/?tugboat` (`&android`), `/?tugboatwait`, `/?tugboatnonet` | Tugboat transferring / waiting for a phone / no usable network |
| `/?game` | Tugboat Run, ready to start |
| `/?gamephone` | Tugboat Run with the phone controller: Tugboat's code, a phone scanning it, then the phone starting the run and steering |
| `/?devtools`, `/?devconfirm` | Settings › Developer tools / the send-text confirmation card |
| `/?whatsnew` | The What's new card for the newest release |
| `/?heavy` | A long history for performance work: ~2,000 texts in ~60 conversations, ~500 notifications, 120 calls, Spotify playing, weather on |

The Tugboat phone page has its own mock: `npm run dev:tugboat`, then
`http://localhost:1430/?mock` (`&busy`, `&offline`, `&closed`, `&game` for the controller). To drive the
real page against a real server (real signing and sealing), see `manual_page` in `tugboat/tests.rs`
(`TUGBOAT_GAME=1` opens the controller channel and prints each input).

## Adding a feature safely

1. **Decide in a pure module, do I/O at the edge.** Put the rule (when to retry, what to show,
   how to parse) in a function with plain inputs, test it, and call it from the actor, worker,
   command or component.
2. **Keep the actors single-owner.** Bluetooth LE state belongs to the BLE actor; MAP/PBAP state to
   the MAP worker. Other code sends them a command; don't add locks around link state.
3. **Time-limit every Windows call that can hang** (see `ble/winrt.rs`), and never block the actor
   or the async runtime: use helper threads or `spawn_blocking` for slow work.
4. **Mirror types** in `protocol.ts`, add a migration for schema changes, and emit an event only
   when something changed.
5. **Teach the dev mock** the new command or event so the UI can be built and reviewed without a
   phone, and add a URL scenario if it's a new state.
6. **Tell the truth in the UI.** If tug can't know something, it says so; no states that can't end.
7. **Keep the window idle.** tug sits open all day, so anything that ticks lives in the small
   component that shows it (`lib/useNow`, which also stops while tug is hidden), never in a store
   that big lists read. No transitions on width or height and no endless animations outside short,
   bounded states; long lists render a page at a time (`lib/useRevealMore`). Check with `/?heavy`.
8. **Say what you verified.** `npm run check` passing is not "works on an iPhone".

## Testing without hardware

- `npm run check`: Tugboat page build, `vue-tsc`, Vitest (`src/`), `cargo fmt --check`, clippy
  `-D warnings`, `cargo test --workspace`. CI (`.github/workflows/check.yml`) runs the same on
  `windows-latest` for every pull request.
- Rust tests cover the pure modules above (several hundred tests); `perf.rs` has manual timing
  tests run with `--ignored`.
- The browser preview covers UI states through the scenarios above.
- A test build with its own identifier (see DEVELOPERS.md) gets its own data, pipe and token, so it
  never touches your installed tug.

## Hardware facts worth knowing

- iOS shares only its ~10 newest incoming texts over MAP, never texts sent from the phone, and no
  attachments.
- After the iPhone restarts, nothing connects until it's unlocked once.
- The iPhone only shows Show Notifications / Sync Contacts for the PC after tug has asked for them.
  An empty phonebook (rather than a refusal) usually means Sync Contacts is off.
- A PC whose iPhone forgot it keeps stale bonds that make the phone connect and drop; remove both
  Windows entries before re-pairing.
- After registering for notifications, an open MAP session's inbox listing can go stale if the MNS
  link dies; reopening the session fixes it.
