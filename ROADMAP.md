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

## v0.5.7 — tug feels built into Windows (split agreed with Dave, 2026-10-05)
Self-contained features that don't wait on hardware spikes or a purchase; built in parallel.
- **Actionable pop-ups**: reply to a text from the Windows notification, Copy code, Call back on a
  missed call, Clear; body click still opens tug.
- **Media keys + Windows media panel** control the iPhone's music (SystemMediaTransportControls).
- **Open in browser**: an "Open" button on notifications from apps with a website; Gmail opens a
  search for that email.
- **Filter unknown senders**: unsaved numbers / spam go to a collapsed "Unknown senders" list, out of
  conversations, the unread badge and pop-ups (codes still pop up); "Move to conversations" or reply.
- **Press-and-hold volume.**
- **Connection health + "Copy diagnostics"** (redacted logs for support).
- **Start with Windows, minimized to the tray** (off by default) and **remember the phone by id**.
- **Low phone battery alert** at 20% and 10%.
- Live texts' pure groundwork (event parser, OBEX server framing, registration call) lands inert.

## v0.5.8 — tug becomes your phone (calling anyone, live texts, auto-updates)
The big bets: they need Dave's hardware for spikes and, for calling and auto-updates, signing.
Dave is in the US, so Azure Artifact Signing (~$9.99/mo) is available to him. Spike 0 passed
2026-10-05 (Handsfree Telephony back on: one PhoneLineTransportDevice for the iPhone).

### Needs Dave before coding can finish
- **Hardware (spikes).** Calling and live texts can't be proven without Dave's PC + iPhone; the
  pure modules land regardless, but the WinRT wiring is spike-gated.
- **Re-tick Windows "Handsfree Telephony" service.** Dave unticked it today, so
  `PhoneLineTransportDevice` count is 0; it must be back on before Spike 0, and Phone Link's own
  calling must be off (one app owns a phone line at a time).
- **Signing decision + country.** Calling's restricted capability and the installer need a signed
  build to ship (sideloading the spike doesn't). Options: **Azure Artifact (Trusted) Signing**
  ~$9.99/mo but **individuals only in US/Canada** — so we need Dave's country — or an **OV
  certificate** $150–300/yr. Either shows Dave's name as publisher; neither instantly clears
  SmartScreen — reputation still builds over time (correcting the old "signing removes the warning"
  wording below). Auto-updates and the calling ship both wait on this.

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

### Live texts (MAP notifications / MNS) — after the pure modules, hardware to wire
Texts land the instant the phone gets them instead of within ~8 s of polling; sends show "Sent".
iOS supports `SetNotificationRegistration`. tug hosts an MNS server (`RfcommServiceProvider` 0x1133 +
`StreamSocketListener` + SDP: name, MAP profile v1.1), keeping the 8 s poll as a backstop.
- PRs: event parser (pure) → OBEX server framing (pure) → `SetNotificationRegistration` client call
  → WinRT MNS listener → wire into the worker with poll fallback → "Sent" status UI.
- Risks: an RFCOMM **server** from an unpackaged app is unproven; the `PushMessage` handle may not
  equal the `SendingSuccess` handle (so "Sent" matching may be approximate).

### tug pairs for texts itself — after the pure pick logic, hardware to wire
The setup's Texts step pairs the phone's Classic side from inside tug (code shown in tug) instead of
sending people to Windows › Add device. Discover the unpaired Classic iPhone via
`BluetoothDevice.GetDeviceSelectorFromPairingState(false)` (inquiry; iPhone must have Settings ›
Bluetooth open), pair with the existing `DeviceInformationCustomPairing` code. Order matters because
of CTKD — **LE (notifications) first, then Classic (texts)**; persist `TEXTS_DEVICE_ID`.
- PRs: pure pick logic → texts discovery mode (only during the step) → pair-for-texts command →
  remember texts device by id → wizard button → ordering guard on re-pair.

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

### Auto-updates
From GitHub Releases, **after** releases are signed (same signing work as calling).

## v0.5.9 — candidates
- **Welcome back**: after 30+ min away, who texted and called.
- **Calls in Ctrl+K search**: a person's recent calls in their search result.

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
