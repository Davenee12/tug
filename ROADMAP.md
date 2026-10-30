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
- ~~**v0.5.13 — faster and steadier**~~ (2026-10-09), ~~**v0.5.14 — a little more life**~~
  (2026-10-10), ~~**v0.5.15–v0.5.16 — security scanning; play, listen, sign in faster**~~
  (2026-10-10): the phone volume bar, "Play iPhone audio on this PC", and dependency and code
  scanning on every PR.
- ~~**v0.5.17–v0.5.18 — texts that send right away, a shared clipboard, smart replies, your whole
  history, album art, automatic updates**~~ (2026-10-17).
- ~~**v0.5.19–v0.5.21 — voice to code**~~ (2026-10-18 to 2026-10-27): voice typing on your PC
  (Accurate, with whisper.cpp), 12× faster, and `tug text` without the card when you choose.
- ~~**v0.5.22 — a live weather harbor**~~ (2026-10-27): the harbor and rain to the quarter-hour,
  Send later, Remind me, one-click links and addresses in texts, Welcome back, calls in Ctrl+K,
  music from the terminal, Pickups saved, Report a problem and staged rollouts.

## v0.5.23 — in progress
On main or in review: voice typing's Most accurate engine with your own words and app-aware hints,
faster texts reconnect, the website and Support tug; and in this release's PR, a quick-text
hotkey, Focus mode (pop-ups held while you're in a meeting, a full-screen app or your editor, one
summary after), music and videos on the PC paused while the iPhone rings, Rain soon pop-ups, and
AI tools named on the confirmation card.

## Road to 1.0

1.0 is a promise, not a feature list: tug is safe to install, stable on PCs and iPhones other than
the maintainer's, and its developer surface won't change under people's scripts. It ships when all
of these are true:

- [ ] **People want it:** the demand goals in [docs/RELEASING.md](docs/RELEASING.md#do-people-want-tug)
      met within about a month of announcing tug.
- [ ] **Code-signed installer**, so SmartScreen and Smart App Control don't stand in the way.
- [ ] **A beta group** of 10–20 people on different PCs, Bluetooth adapters and iPhones, for a few
      weeks, with no serious problems left open.
- [ ] **A quiet fix-only release:** one bug-fixing-only version whose review finds nothing serious.
- [ ] **Decisions that get harder later, made:** where tug keeps its data (roaming or local
      AppData), the shape of the `tug` command and MCP tools (after 1.0 they only change with a
      new major version), and Spotify out of beta or clearly labelled.
- [x] **Safe releases:** staged rollouts, Report a problem, and signed automatic updates.

## Near term — open-source readiness
- **Code signing** for the installer and exes, via a free open-source route (SignPath Foundation)
  or Azure Trusted Signing, so SmartScreen and Smart App Control stop blocking installs.
- **winget and Microsoft Store listings.**
- **Spotify for everyone:** bring-your-own Spotify Client ID, or Spotify's Extended Quota for tug's
  app (today the connector is an invite-only beta: Development Mode allows 5 accounts).
- **Localized UI** (English only today).
- **Keep tug's data in local (non-roaming) AppData** instead of the roaming profile (owner decision
  pending; needs a safe move of existing data).

## Still open from earlier plans
- **Walk-away lock:** lock the PC when the phone leaves Bluetooth range (opt-in, alongside Windows'
  own Dynamic Lock), without locking on a brief Bluetooth drop.
- **Window size and position memory** across launches.
- **Graceful shutdown on exit**, so a restart doesn't leave the phone's music controls unavailable
  for about 25 seconds while iOS times out the old link.
- **Group-text marking** in Messages, once a check on a real phone shows how iOS titles group texts
  over ANCS and MAP.
- **More of Bluetooth**, from the v0.5.11 inventory: a coming-up card from Schedule alerts, a
  voicemail card, Important pinned, "Song 4 of 12" and shuffle/repeat state, travel-aware time from
  the Current Time Service, richer contact cards and birthdays if PBAP sends the fields.
- **iOS limits:** signal bars/carrier need HFP (held by Windows; parked with calling); no charging
  state, AirPods/Watch battery or Focus over Bluetooth; tug won't parse Apple Continuity adverts.
- **To verify on a real phone:** the developer tools end to end (a real code via `tug code --copy`,
  the send-text card, `media_control`, Add tug to PATH, AI tools connecting for real), and voice
  typing's Most accurate engine with a real voice.

## Later — candidates
- **Now Playing colours from album art.**
- **Connectors:** Google Calendar (meeting reminders with Join) and Slack (decide its job first).
- **Join a FaceTime link** from a text in the browser. Starting a FaceTime call isn't possible.
- **Clear "can't connect" help** when a PC's Bluetooth can't work with an iPhone.
- **Developer track:** Tugboat → agent/editor handoff; dev notification actions (Open PR / Open in
  VS Code). Pushing agent alerts *to* the iPhone isn't possible over Bluetooth without an app (ANCS is
  phone → PC only).
- **Tugboat v2:** a zip of several files, HEIC → JPEG, Save to Photos (needs HTTPS for the share
  sheet), mDNS.
- **PC media** next to the phone's (Windows media sessions, e.g. YouTube in a browser).
- ~~**Full history import**~~ from a local iPhone backup (sent messages, photos/attachments), since
  iOS doesn't expose those over Bluetooth: built as Settings › Messages › Bring in your full history
  (see [docs/FEATURES.md](docs/FEATURES.md)). Still open: keeping it in step with newer backups
  automatically.
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
- **Branded installer** (NSIS welcome/finish pages). Signing alone doesn't instantly clear
  SmartScreen; reputation still builds over time.

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
