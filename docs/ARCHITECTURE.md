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
| A2DP (the PC as the phone's speaker) | Classic Bluetooth, via Windows' AudioPlaybackConnection | The iPhone's audio through the PC's speakers, when asked for | `pc_audio/` |

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
  │ tug-ble      │◄────────────── commands.rs (106 Tauri commands) ───────────►│ tug-map              │
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
| `tug-hotkey` | `code_fill/win.rs` (logic in `code_fill/mod.rs`) | Owns the type-the-code shortcut (RegisterHotKey belongs to the registering thread); each press types the newest code with SendInput on a short-lived `tug-code-fill` thread. |
| `tug-share-watch` | `share_privacy/win.rs` (logic in `share_privacy/mod.rs`) | Hide previews while sharing or presenting: sleeps on a condition variable until tug's window is on screen or a pop-up finds the screen shared; then one look every 2 s (shell notification state, display topology, a pass over top-level window classes, reading the titles of browser and Teams windows only, compared on the spot and never kept or logged: about 0.3 ms) until the window hides and hiding clears. Each pop-up also takes a fresh look (on a short-lived `tug-share-look` helper, at most 500 ms) before it shows. It fails closed: no answer in time, or a failed Windows call with nothing found, hides the preview and blurs the window with a banner saying tug couldn't check; a look stuck for 2 s is replaced by a fresh one (at most three at once). When hiding starts, the phone's pop-ups and their follow-ups are taken back from the screen and Action Center (`toast::withdraw_while_hiding`); tug's own stay. |
| `tug-voice-key`, `tug-voice` | `voice/win/` (logic in `voice/mod.rs`, `voice/text.rs`) | Only once voice typing is switched on. `tug-voice-key` owns the hold-to-talk shortcut (RegisterHotKey) and the GDI listening indicator; `tug-voice` is a COM thread that runs each hold (microphone while the key is down, Windows' offline SAPI recognizer, the text rules, SendInput) and keeps the recognizer between holds. |
| Toasts, tray | `toast/`, `tray.rs` | Toast activations arrive on a pool thread and are handed to the async runtime; the tray runs on the main thread. |
| `tug-jump-list` | `quick_actions/win.rs` (logic in `quick_actions/mod.rs`) | A short-lived COM thread that hands the taskbar jump list's Tasks to Windows, at startup and when Show games changes; nothing waits for it. |
| Automatic updates | `updater/mod.rs` (logic in `updater/policy.rs`) | Only in release builds with a real public key in `tauri.conf.json`. An async task that looks at the clock every minute: the first check 90 s after startup, then every 6 hours (an hour after a failure), while Download updates automatically is on. `tauri-plugin-updater` fetches `latest.json` from GitHub Releases, downloads the NSIS installer and verifies its minisign signature; the verified bytes wait in memory until the person clicks Update (`update_apply`, refused during a ringing call, a Tugboat transfer or voice typing) or quits from the tray. Applying runs the exit tidy-up (`tidy_before_exit`), starts the installer in update mode and exits; the installer restarts tug, which reopens the saved screen. |
| Spotify, location | `commands.rs`, `spotify/`, `location.rs` | No background task: each command runs on `spawn_blocking`. The UI reads Spotify's player when the song changes. |
| PC audio | `pc_audio/` | No thread of its own: AudioPlaybackConnection is agile, so `PcAudio` keeps the open connection behind a mutex and each Windows call runs on a time-limited helper (`off_thread`). Follows the connection through a status hook on `Shared` (stop on a phone switch; the opt-in automatic switch). Closed on Stop, Forget and exit. |

**Talking between them.** `BleHandle` and `MapHandle` are unbounded mpsc senders of commands, with
oneshot replies. `Shared` (`state.rs`) is the one `Arc` everything holds; `update_status` and
`update_now_playing` emit to the UI only when something changed, under the lock, with a `rev` that
counts the changes (PC audio's status has one too; Tugboat's and Developer tools' statuses number
each copy as it's composed, `RevCounter`). Events and reads can still reach the window out
of order (a startup read, a command's answer), so the stores keep the highest `rev`
(`lib/ordering`), and a notification list read can't undo a removal that landed while it was in
flight.

**Events to the UI** (Rust → Vue): `device-status`, `now-playing`, `notification`,
`notification-removed`, `app-name`, `discovered-devices`, `pairing-request`,
`pairing-request-closed`, `message`, `contacts`, `calls`, `open-latest-conversation`,
`open-settings`, `toast-pressed`, `code-filled`, `quick-action` (`state.rs`); `tugboat-status`, `tugboat-text`, `tugboat-dropped`,
`tugboat-clip`, `tugboat-send-clipboard`, `game-pad` (`tugboat/mod.rs`); `devtools-status`, `devtools-confirm` (`devtools/mod.rs`); `pc-audio` (`pc_audio/mod.rs`); `screen-share` (`share_privacy/`); `voice-status`, `voice-typed` (`voice/mod.rs`); `update-status` (`updater/mod.rs`); `backup-import` (`backup/mod.rs`). The typed listener
map is `EventPayloads` in `src/lib/ipc.ts`.

## Startup

`src-tauri/src/lib.rs` (also, before the store opens: sweep any decrypted scratch a backup import
left in `backup-tmp`): claim tug's AppUserModelID (`quick_actions::init`, before any window) →
plugins (single-instance first, so a second launch just shows the window or carries out its
`--action`; log with rotation; notification; dialog) → withdraw the last run's pop-ups and, running
elevated, move an old all-users Start with Windows entry back to this user if tug lives in that
account's own Local AppData (both off the main thread; this run's own pop-ups wait for the
withdrawal, at most 3 s, so it can't take them back;
`autostart.rs` owns the HKCU Run entry, quoted, with `--minimized`) → create the data folder →
open the SQLite store and migrate → queue this launch's `--action`, if any, and build the taskbar
jump list (off-thread) → `Shared` → start the BLE actor → start the MAP worker → media
keys → Spotify → Tugboat service (cleans partial uploads) → developer bridge → register state →
automatic updates (`updater`: does nothing without a public key) → a one-off cache trim → tray → WebView2 watch (reloads the page if its process dies or hangs; rebuilds the
window if WebView2's browser process exits, except during a Windows shutdown) → show the window unless
started minimized.
Closing the window hides it to the tray (unless the user turned that off); on exit tug withdraws its
toasts, stops Tugboat and hands PC audio back to the phone. When Windows is signing out or shutting
down (`session_end.rs`: the main window is subclassed for WM_QUERYENDSESSION/WM_ENDSESSION, backed
by `SM_SHUTTINGDOWN`), exit runs inside WM_ENDSESSION and Windows waits for it, so it skips the
toast withdrawal and Tugboat's temp cleanup (both done at the next start) and doesn't wait for PC
audio to close; a log line times the exit either way. The uninstaller runs
`tug.exe --remove-from-path` before removing files, which takes tug's `bin` folder off the user's
PATH whatever its length (`run_uninstall_step` in `lib.rs`) and exits without starting the app.

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
  ─► MapCommand::Deliver(row id) ─► command_queue: ahead of every refresh, check and mark-read
  ─► worker pushes the bMessage over OBEX (on the open session: the PushMessage alone)
  ─► accepted / unconfirmed ("May have sent") / failed (Retry) ─► emit "message"
  ─► later MNS SendingSuccess/Failure ─► mns::choose_outgoing settles it
```

A send never waits for a whole refresh: the worker looks at its queue between a refresh's steps
(after the listing, before each download, before the contacts and calls pulls, between mark-reads),
and a contacts or calls pull in progress is dropped (its own PBAP connection) and retried later.
What it can't cut into is one OBEX exchange already on the wire, or a message-access connect in
progress. Typing a text opens message access if it isn't open (`warm_texts`). The log follows each
send by id only: `send N: queued`, `pushing after … ms in the queue`, `accepted it in … ms`, and the
phone's own report `… ms after accepting it`.

**Bringing in history from an iPhone backup** (Settings › Messages)

```
backup_list ─► discover.rs: %APPDATA%\Apple\MobileSync\Backup\*, %USERPROFILE%\Apple\MobileSync\Backup\*
  (Info/Manifest/Status plists: name, model, iOS, date, size, encrypted, too old/unfinished)
  the window names a backup by id only; tug keeps the folder (or Windows' folder picker, from Rust)
backup_open(id, password?) ─► tug-backup thread (BackupService in backup/mod.rs)
  encrypted? keybag.rs: PBKDF2-SHA256 (DPIC rounds, progress + Cancel) → PBKDF2-SHA1 → AES unwrap
             of the class keys (wrong password = failed integrity check) → ManifestKey
  Manifest.db decrypted into backup-tmp\<session> ─► manifest.rs: HomeDomain Library/SMS/sms.db
  (+ -wal), MediaDomain Library/SMS/Attachments/* ─► decrypt.rs (per-file key, AES-256-CBC, chunked)
  ─► smsdb.rs + typedstream.rs ─► SmsData in memory; the decrypted databases deleted at once
  ─► import::scan ─► "backup-import" { phase: ready, scan }
backup_import(texts, photos) ─► import.rs: plan (a group chat? left out; guid already imported?
  the same text from Bluetooth by direction + match_key + words + time? which address tug already
  uses?) ─► batches
  of 500 in transactions ─► photos: decrypt to scratch → SHA-256 → attachments\<hash>.<ext>
  ─► "backup-import" { phase: done, summary } ─► the session (keys, data, scratch folder) dropped
window: stores/backup.ts re-reads messages and conversation heads; MessageThreads loads a
  conversation's whole history when it opens (conversation_messages); photos via attachment_preview
  (wic.rs: JPEG preview, HEIC needs HEIF Image Extensions) and open_attachment (default app)
```

Imported rows are ordinary `messages` with source `iphone-backup` (keyed by the backup's message
guid, so a re-run or a cancelled run adds only what's new), plus `edited`/`unsent` and
`message_attachments`. Only one-to-one conversations come in: group chats (more than one other
person, or marked as a group by iOS) stay on the iPhone, because Messages in tug holds only
conversations it can reply to and Bluetooth can't send to a group. A Bluetooth text the import
matched is never changed: its photos are attachment rows pointing at it, undone by Remove imported
history.

**A media command**

```
Now Playing / Ctrl+K / Windows media keys / MCP media_control
  ─► Command::Media ─► BLE actor ─► ams::CommandGate (drop a duplicate queued behind a stall)
  ─► AMS Remote Command write
Spotify-specific (seek, like, shuffle, repeat, search, Play on) ─► Spotify Web API (spotify/)
```

**Album art on Now Playing**

```
song changes ─► stores/albumArt: lib/albumArt artChoice
  Spotify is the player? wait until it reports this song ─► its cover (no lookup)
  else, Show album art on, artist + album known, tug on screen ─► invoke album_art(artist, album)
    ─► album_art.rs: <data>/album_art/<sha256(artist, album)>.img? ─► data: URI (no network)
       a miss in the last day (.none)? ─► nothing
       Gate: wait 600 ms (a newer song gives up this one), ≤ 1 lookup / 2 s
       ─► iTunes Search (artist + album only) ─► pick: artist and album, normalised ─► 600 px cover
       ─► cache (50 MB, least recently used trimmed) ─► data: URI
  cover ─► lib/artCanvas: one 24 px canvas pass ─► extractPalette ─► cardTint (WCAG AA or none)
  ─► NowPlayingCard: --art-from/--art-to/--art-accent (eased, swapped with reduced motion)
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

**Files to the phone (`tug boat`, `send_file_to_phone`)**

```
tug boat report.pdf ─► tug-cli: resolve against its folder, outgoing::check (regular file, links
  followed) ─► Call::OfferFiles { paths, stdin?, wait_secs, progress: true }
  ─► DevTools::handle: "Send files to your iPhone" on? rate limit ─► devtools/outgoing.rs checks
     again (size, names::sanitize) ─► Progress::Confirming ─► confirm.rs card (Ask::Files)
  ─► Send? TugboatService::hold (starts Tugboat) ─► Session::hold: offers + held.rs Hold (window)
     no phone page open? the panel opens, "Scan to receive report.pdf on your iPhone"
  ─► every 500 ms Session::held: Progress::Waiting ─► phone fetches every chunk ─► Delivered
     window over (+ up to 60 s for a file still coming) ─► withdrawn, TimedOut
  ─► OfferResult ─► "Delivered: your iPhone downloaded report.pdf." (exit 0) / exit 1
```

Progress lines (`{"type":"progress",...}`) go only to a call that asked for them, after the
handshake. The supervisor's 2 s tick also settles holds (`held_tick`), so a command stopped with
Ctrl+C never leaves files on offer past their window.

**A Tugboat upload**

```
QR: http://<LAN IPv4>:<port>/#<secret>   (the secret after # is never sent)
phone page ─POST /api/up─► PUT chunks (XChaCha20-Poly1305, keys from HKDF of the secret)
  every request MAC'd (first phone binds, replays refused) ─► tugboat/session.rs
  first upload: can tug write to Pictures\Tugboat? Controlled folder access refusing it (access
  denied) ─► Downloads\Tugboat, and the panel says why; refused there too ─► "blocked" to the phone
  ─► finish: sanitized name, " (2)" on collisions ─► that folder + Mark-of-the-Web
  ─► emit "tugboat-status"
```

**The shared clipboard (Tugboat)**

```
PC → phone: tray "Send clipboard to iPhone" (emits tugboat-send-clipboard) / Ctrl+K / the panel
  ─► stores/tugboat.ts sendClipboard (one press, one send) ─► invoke tugboat_send_clipboard
  ─► clipboard::read_for_sharing (Win32, off the main thread) ─► clip::prepare
     (text ≤ 256 KB; a CF_DIB picture ─► dib.rs ─► PNG ≤ 64 MB / 24 MP, held in memory as a hidden offer)
  ─► Tugboat starts if it's off ─► Session::send_clip ─► clip::Outbox (waiting, 2-minute window)
  no phone on the page? the panel opens: the code says "Scan to receive your clipboard on your iPhone"
phone page polls GET /api/state ─► { clip: { id, text | image } } ─► big Copy card
  (a picture is fetched chunk by chunk like any offer) ─► POST /api/clip { id } (signed, sealed)
  ─► Outbox: delivered, contents let go ─► emit "tugboat-clip" ─► "Your clipboard is on your iPhone"
  2 minutes with no acknowledgement ─► dropped ─► "tugboat-clip" expired ─► "wasn't sent"
phone → PC: "Paste to PC" ─► POST /api/text ─► clipboard::set_text_private (no history, no cloud
  sync) ─► emit "tugboat-text" ─► "Copied from your iPhone"
```

The page is plain HTTP, so it isn't a secure context: browsers give it no `navigator.clipboard`.
Copy uses `execCommand("copy")` inside the tap (`tugboat-page/clip.ts` `copyWith`), and if the browser
refuses, the page selects the text and says to press and hold it. Pictures can't be put on the
clipboard from such a page, so the page shows the picture to press and hold.

**A taskbar quick action (jump list)**

```
right-click tug on the taskbar / in Start ─► Tasks: New message · Tugboat · Copy latest code ·
  Play a game (only while Show games is on; opens the games picker) · Settings
  each task is a shell link: tug.exe --action <id>   (the exe path made plain by shell_path: tauri's
  current_exe is a \\?\ verbatim path, which IShellLinkW::SetPath rejects; registered by quick_actions/win.rs under the
  AppUserModelID the process claims: the bundle identifier, which the installer stamps on the
  Start menu shortcut; a cargo build uses "<identifier>.dev" so it never repoints the installed list)
tug running?  the new tug.exe hands its args to the running one (single-instance) and exits
              ─► quick_actions::receive: queue it, tray::show, emit "quick-action" (no payload)
not running?  this launch is tug: setup queues parse_args(launch_args()) (args_os, read lossily: a non-Unicode argument can't abort tug)
UI (App.vue), only after tug.init() has loaded history and settings, and on every nudge:
  invoke take_quick_actions ─► the queue, drained (each click runs once; duplicates of a waiting
  action collapse) ─► lib/quickActions ─► the same paths as Ctrl+K / Ctrl+N / Ctrl+Shift+C / Ctrl+,
```

A nudge carries nothing, so one lost to a page that's still loading loses nothing: the action
waits in the queue until the next take.

**A game controller input (Tugboat Run, Harbor Stack)**

```
game: "Use your phone as a controller" ─► invoke game_pad_open(players, game) ─► Tugboat starts if
  off; Session::pad_open(players, GameId)    (lib/useGameController.ts, shared by both games)
phone page polls GET /api/state ─► { game: true, gameId } ─► Controller.vue with that game's buttons,
  held sideways: left thumb the slider (Tugboat Run steers; Harbor Stack nudges the crane and
  springs back), right thumb the button (Boost / Drop); upright, "Turn your phone sideways to play"
  POST /api/pad the moment the input changes (touch, slide, button down/up), coalesced to one per
  16 ms (~60/s), up to two in flight, newest wins; a heartbeat every 200 ms while nothing changes
  signed like every request (same secret, MAC, replay window, first phone binds); body sealed to it
  ─► authorize ─► ≤ 120-byte body ─► channel open? (else 409 no-game) ─► rate limit (else 429 busy)
  ─► pad::decode: exactly {"steer": -100..=100, "boost": bool} (+ optional "drop": bool, + optional
     "age"/"rtt" probe) ─► GameId::allow: Tugboat Run never sees a drop, Harbor Stack never a boost
  ─► older sequence numbers ignored
  ─► Pad state changed? emit "game-pad" ─► the open game applies it on the next frame. Tugboat Run:
     the slider position is the boat's target (critically damped follow, τ 50 ms); the keyboard
     keeps inertia. Harbor Stack: a fresh Drop drops, the slider speeds or slows the crane ±35%
  reply (sealed): { paused, hits, player?, game } (game_pad_feedback + the channel's GameId), so the
     phone shows Paused, buzzes, and switches buttons if another game took the channel over
150 ms watch: no input for 700 ms ─► emit "game-pad" { connected: false } (the keyboard has it)
game closes ─► game_pad_close ─► inputs get 409 ─► the page goes back to Tugboat
```

Two players: `game_pad_open(2)` opens a second seat, and `TugboatStatus` carries a second code
(`guestUrl`/`guestQr`) for as long as it's open, which TugboatRun.vue shows as the **Player 2
code** beside Tugboat's own (Player 1 scans Tugboat's usual code). The Player 2 code's fragment is
`#g.` and a guest secret, `HKDF-SHA256(session secret, salt "tugboat/1", info "tugboat guest v1")`,
16 bytes (`crypto::guest_secret`, `guestSecret` in the page's crypto.ts, pinned by
`guestVector.json`); it can't be turned back into the session secret. The page reads the `g.`
(`secret.ts`), derives keys from the guest secret the usual way and signs and seals with them, and
shows only the race. `auth.rs` checks a request against the session's keys first (the owner,
bound by the first such request as always) and then the guest's: a guest-key request may only be
`GET /api/state` (which tells it only whether there's a game to join, and which) or `POST /api/pad`, anything else is
403 `in-use`; it can never bind the session, even as the first request; and it joins as the guest
only while seat 2 is open (else 403 `no-race`), one at a time with its own replay window (a guest
quiet for 8 s gives way to the next, otherwise 403 `full`). A second phone with Tugboat's own code
during a race gets 403 `player-2-code`, never the guest seat. Its bodies and replies are sealed with
the guest keys. Seat 1 is the session's own phone, seat 2 the guest; each seat has its own
ordering, rate limit, silence watch, reply (`player` 1 or 2, so the phone shows its colour) and
latency summary, and `game-pad` carries the `seat`. `game_pad_open(1)` closes seat 2 again (the
game hears it's gone; the guest waits on its page for the next race) and the Player 2 code goes.

The input channel is a small POST endpoint rather than a WebSocket: it reuses Tugboat's
authentication, sealing, connection limits and timeouts unchanged, needs no new dependency or
connection upgrade, and on a home network one request per input (kept alive) is well inside the
100 ms target. An input can only change the one `PadInput` the game reads.

Latency: the body is read before any refusal (a reply that leaves a body unread would close the
connection), accepted sockets set `TCP_NODELAY`, and connections are kept alive. Hyper's header
timeout (10 s) doubles as the idle limit for a kept-alive connection; it restarts with each
request, so an active controller never meets it, and only connections left idle are closed. The
phone's probe (`age` = how long the newest touch waited before going out, `rtt` = its last round
trip) gives a touch-to-PC estimate of `age + rtt/2`, logged as one summary line at debug level
when the controller closes. While the channel is open, the 150 ms watch also logs, at most once a
second and only for a phone that sent anything, a debug line of counts (`controller 1: inputs 58,
late 0, presses 2 in 1.0 s`), and an info line when a phone starts sending (`game controller 1
sending`), so "the button does nothing" can be told apart from "nothing arrives".

A quick tap on the phone can't be lost between sends: the press stays in every input until a reply
to one that carried it comes back, and only then does the release go (`PressLatch` in
`tugboat-page/pad.ts`); the game latches a fresh press until its next frame. `padBody` builds the
sealed body, and `padVector.json` pins the page's bodies against the Rust decoder (vitest and
cargo test both read it).

**Voice typing (hold to talk)**

```
hold the shortcut ─WM_HOTKEY─► tug-voice-key: note the window in front ─► tug-voice
  privacy switch blocks the mic? ─► "Microphone blocked" (indicator + Settings)
  wave-in (16 kHz mono, memory only) while GetAsyncKeyState says the key is down, + 200 ms
  ─► too short / silent? stop ─► SAPI in-process recognizer, dictation, from a memory stream
  ─► voice::text::format (spoken symbols, casing, style from the program's .exe name)
  ─► keys let go, same window still in front? ─► SendInput (Unicode; new line = Shift+Enter in known prose and chat apps, a space elsewhere)
  ─► emit "voice-typed" (the outcome only) ─► indicator hides or says what went wrong
```

Recognition sits behind `voice::engine::Engine`. Windows' desktop SAPI recognizer was chosen
because it is offline, ships with Windows and needs nothing new to build. `Windows.Media.SpeechRecognition`
was ruled out: its free-dictation grammar is recognized by Microsoft's online service and it needs
package identity, which tug (an unpackaged installer) doesn't have. whisper.cpp (`whisper-rs`)
would be more accurate but needs CMake and libclang that contributors don't have, plus a model
download; it can be added later as another `Engine`.
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
| `forgotten.rs` | The iPhone forgot this PC while Windows kept the bond: a link that keeps coming up and dropping at once on a phone that has worked here before, Windows' stale-bond error twice, or refused connects while the link is also flapping. Refusals alone never count (a healthy phone refuses too, with Share System Notifications off or after Don't Allow), nor do blips while notifications are subscribed; never while it's locked, and not after a wake, radio change or adapter stall |
| `wake.rs` | A real sleep vs a stalled thread, from two clocks and the resume flag |
| `map/contacts_watch.rs` | When to check the phonebook (every 10 min, plus triggers; every 10 s while the switches are on screen), whether a check must pull the names (PBAP 1.2 version counters), when an empty phonebook means Sync Contacts is off |
| `pin_confirm.rs` | The PIN prompt waiting on the user, by id, so one pairing's leftovers can't close the next's |
| `map/command_queue.rs` | The message worker's priority order: sends before refreshes, mark-read last |
| `map/obex.rs`, `bmessage.rs`, `listing.rs`, `vcard.rs`, `mns_event.rs`, `mns.rs`, `pick.rs` | OBEX framing, MAP/PBAP parsers, MNS event handling, which paired device is the iPhone |
| `map/charset.rs`, `sms_pdu.rs` | A text's bytes to words without U+FFFD: UTF-8, then UTF-16, CESU-8, a cut-off last character, Windows-1252; native SMS PDUs |
| `devtools/confirm.rs`, `rate_limit.rs`, `recipient.rs`, `settings.rs` | The send-confirmation state machine (texts and files), per-tool limits, name → contact, switch defaults |
| `crates/tug-bridge` (`auth`, `protocol`, `framing`, `paths`, `time`) | Bridge protocol and handshake; the whole exchange is also tested over a real pipe |
| `crates/tug-cli` (`args`, `format`, `mcp`) | Command parsing, terminal output, MCP tool mapping |
| `tugboat/crypto.rs`, `names.rs`, `auth.rs`, `upload.rs` | Tugboat keys and sealing (a test vector shared with the phone page), safe file names, request MACs, chunk bookkeeping |
| `tugboat/pad.rs` | The game controller channel: which game has it (`GameId`) and the one action each game may see, what an input may say, the rate limit, newest-input ordering, when a quiet phone counts as gone, two seats for a two-player game |
| `tugboat/net.rs`, `tugboat/tally.rs`, `tugboat/session.rs` (`choose_folder`) | Which adapter the phone can reach (including a Hyper-V external switch); logging strangers' refused requests once a minute with a count; Pictures or the Downloads fallback |
| `autostart.rs`, `session_end.rs` | Start with Windows over a `Registry` trait (tested with a fake: HKCU only, moving an old HKLM entry only for a tug in the process user's own Local AppData); what exit skips when Windows is ending the session |
| `tugboat/held.rs`, `devtools/outgoing.rs`, `crates/tug-bridge` `outgoing` | Files sent to the phone with `tug boat`: when a hold is delivered, timed out (with grace for a file still coming) or withdrawn; which paths may be sent at all (regular files, links followed, no devices or pipes) |
| `tugboat/clip.rs`, `tugboat/dib.rs` | The shared clipboard: one waiting clipboard at a time, delivered only on the phone's word, dropped after 2 minutes, what can be sent; a clipboard bitmap to PNG |
| `quick_actions/mod.rs` | Taskbar quick actions: reading `--action` from a launch, the jump list's tasks (the game only with Show games on), the AppUserModelID for installed, portable and cargo builds, the take-once queue |
| `codes.rs` | One-time codes; a port of `src/lib/codes.ts`, both tested against `src/lib/codes.cases.json` |
| `code_fill/mod.rs` | Code fill: what may be copied or typed, the newest fresh code, the clipboard-clear rule, the shortcut list (shared with `src/lib/codeHotkeys.json`), keystrokes |
| `spotify/model.rs`, `toast/xml.rs`, `cache_trim.rs`, `diagnostics.rs` | Spotify JSON and device choice; toast XML and actions; cache caps; diagnostics redaction |
| `album_art.rs` | Album art from Apple's catalog: names normalised (editions like Remastered/Deluxe and featured artists set aside), the best match by artist and album, the cache key, the day-long miss memory, the debounce and rate limit (`Gate`); the cache and setting tested with a fake network |
| `share_privacy/mod.rs` | Hide previews while sharing or presenting: which signals mean someone may be watching (shell state confirmed by the foreground window covering its monitor, duplicate display, known share and slideshow windows by program + class), which one names the reason, the log line, what a hidden pop-up says, and that no answer means hidden (which looks to join, start or give up on), which pop-ups to take back when hiding starts |
| `voice/mod.rs`, `voice/text.rs` | Voice typing: the shortcut list (shared with `src/lib/voiceHotkeys.json`), which language, the microphone privacy switches, what a hold means (too short, silent, keys still held, window changed) and what the pill says; spoken symbols, casing commands, code vs prose style and where "new line" may break a line, by program; keystrokes. `voice/win/sapi.rs` has a by-hand end-to-end test that recognizes text-to-speech audio (`--ignored`) |
| `backup/discover.rs`, `keybag.rs`, `decrypt.rs`, `manifest.rs`, `smsdb.rs`, `typedstream.rs`, `import.rs`, `clock.rs`, `files.rs` | Bring in your full history: which backups exist and what they are; iOS backup encryption (keybag TLV, both PBKDF2 steps, RFC 3394 unwrap, AES-256-CBC streaming, against RFC vectors and a generated encrypted backup); Manifest.db and its NSKeyedArchiver file records; Messages' tables across iOS versions, Apple-epoch times, which chats are groups (left out), edits, unsends, tapbacks; the text in `attributedBody`; what's new, dedupe against Bluetooth texts (the phone's zoneless times read in UTC with an offset tug knows, never guessed; nothing after the backup's date), resumable batches, Remove imported; content-addressed photo files, key checks, the scratch folder and its startup sweep. `backup/mod.rs` runs a whole encrypted backup end to end in a test |
| `pc_audio/policy.rs` | PC audio on/off state machine (stale attempts can't turn it on), what Windows' open status means, which audio device is the iPhone (never a guess), when to turn on by itself |
| `updater/policy.rs` | Automatic updates: whether the build is set up (a real minisign public key, not a debug build), installed vs portable, newer-only version comparison (no downgrades), the check schedule, what a check leads to, what holds a restart (a ringing call, a Tugboat transfer, voice typing), applying on quit, the release-notes headline |

Frontend logic lives in **`src/lib/*.ts`**, each with a Vitest `*.test.ts` beside it: `albumArt`
(which art Now Playing shows, its palette, and a tint that keeps WCAG AA contrast), `backupImport`
(the full-history wizard's words and steps), `attachments`
("Attachment: 1 Photo" → a photo chip), `attention`
(is the window really visible), `battery`, `callKeys` (no keystroke answers or declines a call), `coalesce` (one render per frame for bursts of
updates), `codeFeed`, `codeFill` (auto-copy and the type-the-code shortcut), `codes`, `commands` (Ctrl+K verbs),
`connectFlow`, `connectionPanel`, `connectionStatus` (one wording for the link everywhere),
`devtools`, `errorReport`, `escape`, `feedPaging` (keep paging while the Feed's end is in view),
`format` (conversation grouping and display), `health`, `latest` (newest request wins: late
answers never land), `layout` (sidebar density and "wide", from sizes that follow the zoom),
`locating`, `media` (which controls show, skip routing, seek keys), `messageSync`, `messageType`,
`ordering` (newest state wins over late events and reads), `pairings`,
`pcAudio` (PC audio button and Settings words), `permission`, `phoneModel`, `phoneSwitches` (the three iPhone switches), `playback` (Spotify Connect
titles and devices), `popup` (pop-up policy), `reconnectPopups`, `scroll`, `senders`
(unknown-sender filter), `sharePrivacy` (hiding previews while sharing: the banner, Show anyway, a pop-up's kind), `singleFlight` (one press, one action), `smartReplies` (suggested replies: the rules, and when there are none), `spotify`, `stableList` (regrouped lists keep unchanged rows, so only
they re-render), `startup` (startup reads that fail are retried, never block), `threadWindow` (long conversations render their newest texts first),
`quickActions` (routing a taskbar task, one take at a time), `toastLimiter`, `toastSpec`, `tugboat`, `tugboatRun` (the game's rules: the seeded course, collisions,
scoring, the speed curve, inertia, the weather's handling, merging the controls), `tugboatDaily` (the
daily challenge: the date's seed, weather → water, the start screen's line, the streak), `tugboatRace`
(two players: who steers what, the result, when a dropped phone pauses the race; Harbor Stack's
stack-off uses it too), `harborStack` (Harbor Stack's rules: slicing the overhang, perfect drops, the
combo and width back, the crane's speed curve, the seeded sway and gusts, the daily's weather, the
controls), `useGameController` (the phone controller's channel, shared by both games), `update` (the Update
button, its Restart confirm and the About row; reopening the same screen after the restart), `useCitySearch` (the
weather card's city box), `vips`, `voice` (voice typing's Settings words and problems), `weather`, `weblinks`, `whatsNew`. Thin DOM composables such as
`useElementSize` (an element's size in zoomed CSS pixels) and `useCallButtonGate` (a ringing call's Feed buttons) feed these rules.
The phone page has its own: `tugboat-page/chunks`, `crypto`, `clip` (the PC's clipboard: when to
acknowledge it, copying without a secure context) and `pad` (the controller's wire format, each
game's buttons, pacing, errors, sideways or upright, and tilt only where it's really available).
Components and
stores should call these rather than grow logic of their own.

Integration tests: `tugboat/tests.rs` plays the phone over a real socket; `tug-bridge` tests run the
handshake over a real named pipe (including a low-integrity impostor). Hardware probes live in
`src-tauri/examples/` (`map_probe`, `hfp_probe`, `toast_probe`; `cargo run --example …`).

## Rust ↔ TypeScript: `src/types/protocol.ts`

Everything the backend sends to the UI or takes from it is a serde type in Rust **mirrored by hand**
in `src/types/protocol.ts`: `state.rs`, `store.rs`, `ams.rs`, `ancs.rs`, `messages.rs`,
`map/calls.rs`, `map/health.rs`, `toast/`, `code_fill/`, `spotify/`, `tugboat/`, `bt_inventory/`, `devtools/`, `voice/`,
`pc_audio/policy.rs`, `share_privacy/` and `backup/`.

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
| App icons, album art, Spotify art, contact photos | `%APPDATA%\<app id>\` (`icons/` 50 MB cap, `album_art/` 50 MB cap, `spotify_art/` 100 MB cap, `contacts/`) |
| Logs (2 MB × 5, rotated) | `%LOCALAPPDATA%\<app id>\logs` |
| Bridge token, half-received Tugboat files | `%LOCALAPPDATA%\<app id>\` |
| Photos and videos brought in from an iPhone backup (`<sha-256>.<ext>`) and their JPEG previews | `%LOCALAPPDATA%\<app id>\attachments\` (`thumbs\`) |
| Decrypted scratch while a backup is read (deleted after, swept at every start) | `%LOCALAPPDATA%\<app id>\backup-tmp\` |
| Files received with Tugboat | `Pictures\Tugboat` (`Downloads\Tugboat` while Controlled folder access refuses Pictures) |
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
| `/?forgotten` | "Your iPhone forgot this PC" with Pair again |
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
| `/?albumart` | Album art from the (mocked) catalog lookup: Next and Previous step through a light, a dark, a vivid and a grey cover, then an album with none |
| `/?speaker`, `/?spotifyoff` | Spotify on a Connect speaker / Spotify not connected |
| `/?novolume` | A player that doesn't report its volume: no volume bar |
| `/?tugboat` (`&android`, `&cfa`), `/?tugboatwait`, `/?tugboatnonet` | Tugboat transferring (Controlled folder access refusing Pictures) / waiting for a phone / no usable network |
| `/?tugboatclip` | Send clipboard to iPhone with no phone yet: the code with its clipboard hint, then a phone scanning it and taking the clipboard |
| `/?game` | Tugboat Run, ready to start |
| `/?gamephone` | Tugboat Run with the phone controller: Tugboat's code, a phone scanning it, then the phone starting the run and steering |
| `/?game2p` (`&p2drop`) | Tugboat Run for two players with two phones (Player 2's phone drops mid-race: the waiting card and Continue solo) |
| `/?game&weather=rain` | The daily challenge in made-up weather for the preview: `rain`, `storm`, `snow`, `night`, `windy` or `off` |
| `/?games` | The games picker (the Play button, Ctrl+K "game", the taskbar's Play a game) |
| `/?stack`, `/?stackphone`, `/?stack2p` (`&p2drop`) | Harbor Stack: ready to start / with the phone controller (it drops every 1.6 s) / a stack-off with two phones; `&weather=` works here too |
| `/?devtools`, `/?devconfirm`, `/?devconfirmfiles` | Settings › Developer tools / the send-text confirmation card / the send-files card (Send opens Tugboat with the files waiting) |
| `/?pcaudio`, `/?pcaudiofail`, `/?pcaudiodrop`, `/?pcaudioold` | iPhone audio playing on this PC / Play on this PC times out / it drops after 6 s (Reconnect) / Windows too old for it |
| `/?whatsnew` | The What's new card for the newest release |
| `/?quickaction=new-message` (`tugboat`, `copy-code`, `game`, `settings`; `&again`) | tug started from that taskbar task (and, with `&again`, the task clicked again 4 s later while running) |
| `/?codefill` (`&hotkeybusy`) | A code copied once from a notification and its text, then two presses of the type-the-code shortcut (with Ctrl+Shift+V taken by another app) |
| `/?sharing` (`&presenting`, `&stop`) | The screen is being shared: blurred texts and names under the `Previews hidden` banner with Show anyway (a slide show / sharing ends after 10 s); add `&call` for the call card |
| `/?voice` (`&micblocked`, `&nospeech`, `&voicebusy`) | Settings › General › Voice typing on, with the microphone blocked / no speech language / the shortcut taken |
| `/?backup` (`&none`, `&imported`, `&heif`) | Settings › Messages › Bring in your full history with made-up backups (the encrypted one's password is `test`) / no backups / history already brought in (the label, photos) / photos Windows can't show without HEIF Image Extensions |
| `/?heavy` | A long history for performance work: ~2,000 texts in ~60 conversations, ~500 notifications, 120 calls, Spotify playing, weather on |

The Tugboat phone page has its own mock: `npm run dev:tugboat`, then
`http://localhost:1430/?mock` (`&busy`, `&offline`, `&closed`, `&game` for the controller with
`&stack` for Harbor Stack's or `&switch` for a game change mid-way, `&clip` /
`&clipimage` for the PC's clipboard). To drive the real page against a real server (real signing and
sealing), see `manual_page` in `tugboat/tests.rs` (`TUGBOAT_GAME=1` opens the controller channel and
prints each input, `TUGBOAT_GAME=stack` for Harbor Stack; `TUGBOAT_CLIP=1` sends a made-up clipboard; `TUGBOAT_HOST=<LAN address>` serves
it where it isn't a secure context, as on a phone).

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
- A PC whose iPhone forgot it keeps stale bonds that make the phone connect and drop (about every
  2 s, with no stale-bond error); remove both Windows entries before re-pairing. tug spots this
  (`forgotten.rs`) and offers Pair again, which does exactly that. ATT refusals (0x05, 0x08, 0x0F)
  or access denied don't prove it: a healthy phone returns them with Share System Notifications off
  or after Don't Allow.
- After registering for notifications, an open MAP session's inbox listing can go stale if the MNS
  link dies; reopening the session fixes it.
