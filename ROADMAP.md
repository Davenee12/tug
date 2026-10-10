# tug roadmap

Trustworthy first, features second. What's shipped is in [CHANGELOG.md](CHANGELOG.md); what tug
can do today is in [docs/FEATURES.md](docs/FEATURES.md). Plans here can change, and nothing below
is a promise until it ships.

## Release cadence
Every fifth version is **bug fixing only**: **v0.5.10, v0.5.15, v0.5.20, …** A stabilization pass
(hardware logs, independent review, known issues) with no new features. Features go in the
versions between.

## Shipped

- ~~**v0.5.2–v0.5.5 — stabilization sprints**~~: data-integrity bugs and the messaging crash path,
  universal search, migrations, phone sync, CI on every PR, a second bug hunt. See
  [docs/STABILIZATION.md](docs/STABILIZATION.md).
- ~~**v0.5.7 — tug feels built into Windows**~~: actionable pop-ups, media keys and the Windows
  media panel, Open on the web, connection health and Copy diagnostics, start with Windows, low
  battery alert.
- ~~**v0.5.8 — stability, setup, live texts, Spotify**~~: tug pairs the iPhone itself, texts
  arrive instantly, the Spotify connector.
- ~~**v0.5.9 — one-screen connect, Spotify panel, quiet hours, contact photos**~~.
- ~~**v0.5.10 — bug fixing only**~~: reconnect backoff and "Unlock your iPhone", reconnect on PC
  wake, link-blip handling, Spotify read once per song.
- ~~**v0.5.11 — Tugboat**~~ (2026-10-07): photos, files and text between phone and PC over home
  Wi-Fi with no app on the phone, plus your exact iPhone model in the sidebar.
- ~~**v0.5.12 — tug for developers + reliability**~~ (2026-10-07): a local MCP server and the
  `tug` command (codes, search, dev notifications, Tugboat files, texts confirmed on a card), plus
  a steadier connection and texts and notifications that don't go missing. See
  [docs/DEVELOPERS.md](docs/DEVELOPERS.md).

## Near term — open-source readiness
- **Code signing** for the installer and exes, via a free open-source route (SignPath Foundation)
  or Azure Trusted Signing, so SmartScreen and Smart App Control stop blocking installs.
- **winget and Microsoft Store listings.**
- **Spotify for everyone:** bring-your-own Spotify Client ID, or Spotify's Extended Quota for tug's
  app (today the connector is an invite-only beta: Development Mode allows 5 accounts).
- **Localized UI** (English only today).
- **Keep tug's data in local (non-roaming) AppData** instead of the roaming profile (owner decision
  pending; needs a safe move of existing data).

## v0.5.13 — use more of Bluetooth (planned)
1. **Bluetooth inventory probe** (shipped in v0.5.11). A first run on a recent iPhone found:
   Current Time Service with time zone and DST (travel-aware time is possible); AMS volume,
   playback rate, duration and repeat (queue index/count/shuffle need subscribing first); battery
   level only, no charging state; a favourites phonebook (PBAP `fav`) plus call history, no speed
   dial; Windows lists the iPhone for AudioPlaybackConnection (PC-as-speaker is feasible).
2. **Small wins, if the inventory confirms them:** ANCS categories (a coming-up card from Schedule
   alerts, a voicemail card, Important pinned, the phone's own action labels); deeper AMS (a real
   phone volume bar, "Song 4 of 12", playback speed, Apple Music shuffle/repeat state);
   travel-aware time from the Current Time Service; connection quality from Windows 11 link info;
   richer contact cards and birthdays if PBAP sends the fields.
3. **Spike:** "Play iPhone audio on this PC" via `AudioPlaybackConnection` (A2DP sink).
- **iOS limits:** signal bars/carrier need HFP (held by Windows; parked with calling); no charging
  state, AirPods/Watch battery or Focus over Bluetooth; tug won't parse Apple Continuity adverts.

## v0.5.14 — time savers (planned)
Small things that each save a pickup of the phone.
1. **Automatic codes:** a new verification code is copied instantly, a type-it hotkey types it
   into the focused field, and a browser extension can fill it in.
2. **One-click actions in texts:** links, addresses, dates and tracking numbers in a text become
   buttons.
3. **Pause PC audio when the phone rings**, and resume after the call.
4. **Quick-text hotkey:** a global shortcut that opens a small composer from anywhere.
5. **Send to my phone:** text, links or files from the PC to the phone in one step.
6. **Calm mode + daily summary:** hold non-urgent pop-ups and get one summary instead.
7. **Walk-away lock:** lock the PC when the phone leaves Bluetooth range (opt-in, alongside
   Windows' own Dynamic Lock).
8. **Pickups-saved counter:** a gentle count of how many times tug saved you reaching for the phone.

## v0.5.15 — bug fixing only (candidates)
- **Graceful shutdown on exit**, so a restart doesn't leave the phone's music controls unavailable
  for about 25 seconds while iOS times out the old link.
- **Window size and position memory** across launches (pending the owner's decision on the
  behaviour).
- **Show the MCP client's name** on the confirmation card ("Claude Code wants to text…") instead
  of "An AI tool" when the client doesn't send a name tug can show.
- **Group-text marking** in Messages, once a check on a real phone shows how iOS titles group texts
  over ANCS and MAP.
- Done in v0.5.12, still to verify on a real phone: faster recovery after a Bluetooth blip; a quick
  reply sorting after the text it answers even when the phone's clock is ahead; the developer tools
  end to end (a real code via `tug code --copy`, the send-text card with texts connected,
  `media_control` on a playing phone, Add tug to PATH, the installer placing `bin\tug.exe`, AI
  tools connecting for real).

## v0.5.16 — voice to code (planned headline)
Hold-to-talk dictation that types into the focused app (VS Code, terminal, an AI tool's prompt),
with a code-aware mode (camelCase, symbols, file names). Fully local open-source speech
recognition (for example whisper.cpp), no account.

## Later — candidates
- **Everyday texting:** Send later (sends while the phone is connected, or on reconnect with a
  clear "Sending late" note), quick replies in tug and the pop-up, "Remind me" on a text or
  notification.
- **Now Playing colours from album art.**
- **Connectors:** Google Calendar (meeting reminders with Join) and Slack (decide its job first).
- **Join a FaceTime link** from a text in the browser. Starting a FaceTime call isn't possible.
- **Clear "can't connect" help** when a PC's Bluetooth can't work with an iPhone.
- **Welcome back** digest; **Calls in Ctrl+K search**.
- **Spotify for everyone** needs Spotify's Extended Quota for tug's Spotify app.
- **Developer track:** Tugboat → agent/editor handoff; coding focus mode (hold pop-ups while an
  editor or terminal is in front, summary after); dev notification actions (Open PR / Open in VS
  Code). Pushing agent alerts *to* the iPhone isn't possible over Bluetooth without an app (ANCS is
  phone → PC only).
- **Tugboat v2:** a zip of several files, HEIC → JPEG, Save to Photos (needs HTTPS for the share
  sheet), mDNS.
- **PC media** next to the phone's (Windows media sessions, e.g. YouTube in a browser).
- **Full history import** from a local iPhone backup (sent messages, photos/attachments), since iOS
  doesn't expose those over Bluetooth.
- Ideas are curated in [docs/PRODUCT.md](docs/PRODUCT.md) and move here once approved.

## Parked — companion apps and Android (see [docs/COMPANION-PLAN.md](docs/COMPANION-PLAN.md))
tug stays zero-setup on the phone by default. A tug app on the phone is the step that unlocks
clipboard sync, faster files and, on Android, notifications from every app and full message
history. Order when it resumes: the tug protocol in Rust → an Android app (the biggest unlock) → an
iPhone app (needs a paid Apple developer account) → optional remote access.
- **Android without an app:** pairing an Android phone and using tug's existing texts (MAP) and
  contacts/calls (PBAP) code hasn't been tried yet; makers differ in what they allow.
  Notifications, media and battery aren't available over Bluetooth on Android without an app.
  Tugboat already works from Android browsers.

## Parked — calling anyone
Calling back a missed call works (iOS's "Dial" over ANCS) and stays. Dialing anyone needs Windows'
calling API (`PhoneLineTransportDevice` + `PhoneLine`, what Phone Link uses), which requires package
identity (a sparse MSIX package next to the NSIS install) and a restricted capability. A spike got
package identity and access allowed, but registering as the phone line's app didn't stick ("another
app owns the line"). Next step: re-run the probe after a PC restart and investigate line ownership.
Shipping it also needs paid code signing.

## Waiting on code signing
- **Auto-updates** from GitHub Releases, once releases are signed (see Near term). Signing alone
  doesn't instantly clear SmartScreen; reputation still builds over time.
- **Branded installer** (NSIS welcome/finish pages).

## Sustaining tug (monetization plan, owner-approved direction 2026-10-07)
tug's core stays free and MIT-licensed, and private: no ads, no selling data.
1. **Now:** GitHub Sponsors + a "Support tug" link (Settings › About, README). Funds code signing first.
2. **When signed and stable:** a paid Microsoft Store listing (one-time, signed, auto-updates); the GitHub build stays free.
3. **Later, "tug Plus" subscription** for things that cost money to run or save real time: an end-to-end
   encrypted relay to reach the phone away from home, companion-app features, browser-extension code
   autofill and calm mode, developer extras, encrypted backup/sync. Free users keep everything that
   works today. Paid parts live in services or separately licensed add-ons.

## Future — macOS
Tauri, the UI, the store and the pure protocol modules (`ancs.rs`, `ams.rs`, `map/` parsers) are
portable; the Bluetooth I/O (`ble/`, `map/session.rs`) and a few Windows bits (tray overlay,
clipboard, location, Settings links) are WinRT and would be rewritten on CoreBluetooth/IOBluetooth.
- **Spike first:** can a macOS app subscribe to an iPhone's ANCS/AMS and open MAP/PBAP at all, or
  does macOS reserve them for its own Continuity features? Go/no-go before any port.
- Until then: keep protocol logic out of `ble/` so the port stays a transport swap.
