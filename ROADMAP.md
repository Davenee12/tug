# tug roadmap

Order follows the stabilization plan: trustworthy first, features second.
See docs/STABILIZATION.md for the current backlog.

## Now — stabilization sprints
1. ~~Sprint 2: high-severity data-integrity bugs and the messaging crash path.~~ (v0.5.2)
2. ~~Sprint 3: universal search, migrations, frontend fixes.~~ (v0.5.3) Accuracy audit (times, time
   zones, counts, badges) carried into Sprint 4.
3. ~~Sprint 4: phone sync (mark texts read on the phone when opened in tug, clear their
   notifications), accuracy audit fixes, performance, maintainability.~~ (v0.5.4)
4. ~~Sprint 5: CI on every PR, second bug hunt (3 High / 3 Med / 8 Low fixed), `actor.rs` split
   into six modules.~~ (shipped alongside v0.5.5)

## ~~v0.5.7 — tug feels built into Windows~~ (released 2026-10-05)
Actionable pop-ups, media keys + Windows media panel, Open on the web, unknown-sender filtering,
press-and-hold volume, connection health + Copy diagnostics, start with Windows, low battery alert,
plus a stabilization pass (Settings flicker, per-conversation drafts, bounded Bluetooth calls,
calmer polling). See CHANGELOG.md.

## ~~v0.5.8 — stability, setup, live texts, Spotify~~ (released 2026-10-05)
#48–#61. See CHANGELOG v0.5.8.
- ~~Stabilization leftovers~~ (#50): Feed keeps its place, Messages sticks to newest, symmetric
  store teardown.
- ~~Setup, ironed out~~ (#49, #53, #59): human-sized Bluetooth time limits, tap-Allow hint, switch
  checklist, Start over, real phone name, MAP gated until a phone is chosen, wizard scans for an
  unpaired iPhone (Classic) and adopts the LE bond CTKD creates (proven in two clean setups);
  one row per iPhone, Classic preferred for an unpaired phone (#61).
- ~~Live texts~~ (#51, #54): MNS server, instant delivery, Sent status; idle link no longer timed
  out; a dropped link reopens message access (at most once a minute). Proven: iOS connects to an
  unpackaged app's RFCOMM server.
- ~~Spotify connector~~ (#52, #55, #57, #58): Settings › Connectors, built-in Client ID (one-click
  Connect), Feb 2026 API fixes (`items.total`, `/me/library?uris=`), playlist covers, waits for
  Spotify to open on the iPhone. Redirect `http://127.0.0.1:8972/callback` (the dashboard rejects
  port-less loopback URIs).
- ~~Codes from texts in the Feed~~ (#56).

### Hardware facts learned
- Working setup: iPhone on Settings › Bluetooth → Windows "Add a device" pop-up (or tug's Pair) →
  confirm code → one pairing gives Classic + LE (CTKD) → notifications, music, battery, texts.
- A PC whose iPhone forgot it keeps stale bonds that make the phone connect then drop (LightBlue
  too); remove both Windows entries before re-pairing.
- After registering for notifications, an open MAP session's inbox listing can go stale if the
  MNS link dies; reopening the session fixes it (#54).

## Release cadence (Dave, 2026-10-05)
Every fifth version is **bug fixing only**: **v0.5.10, v0.5.15, v0.5.20, …** — a stabilization
pass (hardware logs, independent review, known issues), no new features. Features go in the
versions between.

## ~~v0.5.9 — one-screen connect, Spotify panel, quiet hours, contact photos~~ (released 2026-10-05)
#63–#75. See CHANGELOG v0.5.9.

## v0.5.10 — bug fixing only (release PR open, 2026-10-06)
#77, #79–#81: reconnect backoff + "Unlock your iPhone", resume on PC wake, link-blip debounce,
MAP backoff, Spotify read once per song (verified track), sidebar fit at 1366×768, calls scroll,
image cache caps, crash diagnostics, code pop-up dedupe, contact names. See CHANGELOG v0.5.10.
- **Still to verify on hardware:** overnight locked-phone behaviour, sleep/wake reconnect, Spotify
  song changes, the What's new card on an update from 0.5.9; tune LINK_BLIP_GRACE (1.5 s) and
  RESUME_GAP (10 s) if the log shows false blips or false wakes.
- **Watch:** the WebView2 crash from 2026-10-06 00:14 UTC — now logged if it happens again.

## Later — candidates
- **Connectors:** Google Calendar (meeting reminders with Join; needs a Google Cloud OAuth
  client; Testing-mode tokens expire every 7 days) and Slack (decide its job first).
- **Join a FaceTime link** from a text in the browser. Starting a FaceTime call isn't possible.
- **Clear "can't connect" help** when a PC's Bluetooth can't work with an iPhone.
- **Welcome back** digest; **Calls in Ctrl+K search**; **Send later**; **Remind me to call back**.
- **Spotify for everyone** needs Spotify's Extended Quota (registered business, 250k users).

## v0.5.11 — Tugboat (formerly tug Drop) (next feature release, after v0.5.10)
Move photos, files and text between the phone and PC over home Wi-Fi with no app and no fees:
scan a QR in tug, a tug page opens in the phone's browser (iPhone or Android), send or receive. See docs/COMPANION-PLAN.md
"Phase 0.5" for the design and safety notes.

**Landed (PR `feat/tug-drop`, not yet released):**
- **Entry points:** a Tugboat button in the sidebar under the iPhone, `tugboat` (or `drop`, `send
  files`, `send to phone`) in Ctrl+K, and files dragged onto tug's window (opens Tugboat with them
  offered). Works with Bluetooth down. Future CLI verb: `tug boat <file>`.
- **Panel:** QR code + "Type the link instead"; "Can't connect?" help after 30 s (same Wi-Fi, allow
  tug on Private networks, guest Wi-Fi/VPN/Private Relay); then the connected phone, files and
  text in both directions with progress, Show in folder and Copy.
- **Phone page** (bundled into tug, no CDN): Photos & videos / Files buttons, chunked resumable
  uploads, a paste box that lands on the PC clipboard, Get → Save for PC files (1 GB cap), text to
  copy, light/dark.
- **Safety:** plain HTTP on the gateway adapter's address only, random port, new 128-bit secret
  per open (in the `#`, never sent); XChaCha20-Poly1305 per chunk with direction/file/chunk bound
  in; every request MAC'd, first phone binds, replays refused. Stops passive sniffing, not an
  active attacker on the first page load. Stops on close, quit, or 10 idle minutes; never touches
  the firewall.
- **Files:** saved to Pictures › Tugboat (Pictures known folder), sanitized names, " (2)" on
  collisions, 8 GB per file, free-space check; half-received files live in app data, never in
  (possibly OneDrive-synced) Pictures. HEIC kept as-is.
- **Verify on Dave's phones before release:** iPhone (Safari): Photos picker upload incl. Live
  Photos and a large video with the phone locking mid-way (resume), Save of a PC file (Files ›
  Downloads), Copy on the page, light/dark. Galaxy S20 (Chrome and Samsung Internet): the Photos &
  videos / Files pickers, a large video with the screen off mid-way, Save to Downloads, Copy,
  light/dark. Both: the Windows firewall prompt, real Wi-Fi speed.
- **Not in v1:** zip of several files, HEIC→JPEG, Save to Photos (needs the share sheet, which
  wants HTTPS), mDNS.

## Future — companion apps (planned, see docs/COMPANION-PLAN.md)
tug stays zero-setup on the phone by default. A tug phone app is the step that unlocks files,
clipboard and Wi-Fi speed. Order: **Phase 0** reliability and polish (now) → **Phase 1** the tug
protocol in Rust (device keys, pairing over Bluetooth, encrypted local Wi-Fi link, events with
catch-up) → **Phase 2** Android app (Kotlin, the biggest unlock) → **Phase 3** iPhone app (Swift;
needs the $99/year Apple account) → **Phase 4** optional remote access (WebRTC; Nostr only if it
earns its place). Desktop stack stays Rust + Tauri + Vue + SQLite.
- Brand: "within reach" announcement cards (tug loop + partner name; follow each brand's logo
  rules; only announce real capabilities) and an in-app "✦ New" tied to What's new.

## Waiting on code signing (Dave isn't buying it for now)
- **Auto-updates** from GitHub Releases, once releases are signed. Dave is in the US, so Azure
  Artifact Signing (~$9.99/mo) is available, or an OV certificate ($150–300/yr). Neither instantly
  clears SmartScreen; reputation still builds over time.

## Parked — calling anyone (v0.5.13 or later; Dave, 2026-10-05)
Calling back a missed call already works (iOS's "Dial" over ANCS) and stays. Dialing anyone is
parked: on 2026-10-05 Spike 0 passed and Spike 1 got package identity + `RequestAccessAsync` =
Allowed (the sparse package passes the restricted-capability check), but `RegisterApp` didn't
stick ("another app owns the line"), even with Windows' Mobile devices off. Next time: re-run
the probe (PR #33, `spikes/phoneline`) after a PC restart, then investigate line ownership.
Shipping it to others also needs paid code signing.

### Spikes first (Dave's hardware, gate the big items)
- **Spike 0 — phone line exists.** Re-tick Handsfree Telephony; confirm one `PhoneLineTransportDevice`
  appears for the iPhone (matched by Bluetooth address).
- **Spike 1 — sparse package passes the restricted-capability check.** Standalone probe crate, sparse
  MSIX identity, self-signed cert in `CurrentUser\TrustedPeople` (no admin). Drive the full flow:
  `RequestAccessAsync` → `RegisterApp` → `ConnectAsync` → `PhoneCallManager.RequestStoreAsync` →
  `PhoneLineWatcher` → `PhoneLine.DialWithResultAsync` (tug must be foreground) →
  `ChangeAudioDeviceAsync(RemoteDevice)` to keep audio on the phone. This is the biggest unknown:
  whether an identity-only sparse package beside an NSIS install satisfies `phoneLineTransportManagement`.
  **Go/no-go for calling rides on this.**


### Calling anyone — spike-gated, ships behind signing
Call anyone from tug (Calls tab, Ctrl+K "call zoe", conversations). Today only missed calls can be
called back (iOS's "Dial" over ANCS); dialing anyone needs Windows' calling API
(`PhoneLineTransportDevice` + `PhoneLine`, what Phone Link uses), which requires **package identity**:
a sparse package (identity-only MSIX manifest registered with `Add-AppxPackage -ExternalLocation` next
to the NSIS install), capabilities `phoneCall` (general) + `phoneLineTransportManagement` (restricted;
sideloading needs no Store approval) + `runFullTrust`, and the exe manifest getting an `<msix>` identity
element via `tauri_build` `WindowsAttributes::app_manifest` (keep Common-Controls v6). `windows` crate
feature `ApplicationModel_Calls`.
- PRs (after Spike 1 is a go): identity plumbing → calling module (uses the pure state machine) + UI
  → installer hooks (`Add-AppxPackage -ExternalLocation`) → production signing (Dave's chosen cert).


## Next — onboarding & distribution (product readiness)
- ~~**Settings page** — a full page (not the side panel) with a left nav: General, iPhone, Connectors
  (empty-but-honest), Data & privacy, About.~~ (v0.5.5 — shipped as General, iPhone, Notifications,
  Weather, Data & privacy, About; Connectors section not built yet, see Later → Connectors.)
- ~~**First-run setup wizard** in the app: Bluetooth check, guided pairing with live status, the three
  iPhone switches with live ✓ detection, and a send-yourself-a-test step.~~ (v0.5.5)
- **Pairing without LightBlue** — spike first. *Partly addressed:* v0.5.7 makes tug pair the Classic
  (texts) side itself; the LE (notifications) first-pair still needs LightBlue. Remaining options:
  1. Make the PC appear in iOS Settings › Bluetooth (ANCS service-solicitation advert, or Classic
     pairing from the phone with cross-transport keys). No app needed if it works.
  2. Fallback: QR code → Apple App Clip that pairs in one tap (needs Apple Developer account + review).
  3. Long term: companion iOS app (also unlocks exact charging state, clipboard).
- **Branded installer** (NSIS welcome/finish pages, Start-menu shortcut). *Partly done:* real app
  icons shipped (v0.5.6); welcome/finish pages still to do.
- **Code signing** and **auto-updates** from GitHub Releases, opt-in diagnostics — **pulled forward
  into v0.5.7** (calling needs the signed, identity-carrying build). Signing shows a named publisher
  but does not instantly clear SmartScreen; reputation builds over time.

## Later — product ideas
The small, curated idea backlog and what we decided *not* to build live in
[docs/PRODUCT.md](docs/PRODUCT.md). Shipped from it so far: one-time codes, Ctrl+K actions, tray, the
glance strip (weather) — all v0.5.5. Still open: live texts (now in v0.5.7), reply from the pop-up,
welcome back. Ideas move onto this roadmap only once approved.

## Later — features
- ~~**Delete conversations:** an ✕ on each conversation to remove it from tug (local, with undo).~~
  (v0.5.5) Stretch (notice deletions made on the phone via the ~10-message window) not built.
- **Calls:** *partly shipped* (v0.5.6) — incoming-call card, recent calls (PBAP), call back a missed
  call (ANCS "Dial"), experimental hands-free check. **Dialing anyone** is the v0.5.7 calling bet
  above (Windows calling API, package identity, signing). Audio stays on the phone first.
- **Connectors:** messages already carry a `source`. Android, then Slack, Teams, WhatsApp, Wispr Flow
  into one inbox. Each gets a card in Settings › Connectors (icon, one-line "what you get", Connect /
  Disconnect, status), built alongside the first real connector so the page is never a list of
  dead buttons.
- **PC media** next to the phone's (Windows media sessions, e.g. YouTube in a browser).
- **Full history import** from a local iPhone backup (sent messages, photos/attachments), since iOS
  doesn't expose those over Bluetooth.
- Exact charging indicator (needs companion app or USB trust pairing).

## Future — macOS
Tauri, the UI, the store and the pure protocol modules (`ancs.rs`, `ams.rs`, `map/` parsers) are
portable; the Bluetooth I/O (`ble/`, `map/session.rs`) and a few Windows bits (tray overlay,
clipboard, location, Settings links) are WinRT and would be rewritten on CoreBluetooth/IOBluetooth.
- **Spike first:** can a macOS app subscribe to an iPhone's ANCS/AMS and open MAP/PBAP at all, or
  does macOS reserve them for its own Continuity features? Go/no-go before any port.
- **Product angle:** Mac users already get iPhone texts (Messages), notifications (iPhone
  Mirroring) and code autofill from Apple, so tug on a Mac would lead with what Apple doesn't do:
  connectors in one inbox (WhatsApp, Slack, Teams), and Android phones on a Mac.
- Until then: keep protocol logic out of `ble/` so the port stays a transport swap.
