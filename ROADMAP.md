# tug roadmap

Order follows the stabilization plan: trustworthy first, features second.
See docs/STABILIZATION.md for the current backlog.

## Now — stabilization sprints
1. ~~Sprint 2: high-severity data-integrity bugs and the messaging crash path.~~ (v0.5.2)
2. ~~Sprint 3: universal search, migrations, frontend fixes.~~ (v0.5.3) Accuracy audit (times, time
   zones, counts, badges) carries into Sprint 4.
3. Sprint 4: phone sync (mark texts read on the phone when opened in tug, clear their
   notifications), accuracy audit fixes, performance, maintainability, CI.

## Next — onboarding & distribution (product readiness)
- **Settings page** — a full page (not the side panel) with a left nav, like Wispr Flow's but tug's
  own. Built first in this track because the wizard, updates and connectors all need a home:
  - *General:* start with Windows, notifications/toasts, sounds, zoom, theme.
  - *iPhone:* paired device, connection status, the three iPhone switches with live ✓, re-pair, forget.
  - *Connectors:* see below (the section ships empty-but-honest until the first connector lands).
  - *Data & privacy:* history retention, clear history, export, where data lives (local only).
  - *About:* version, check for updates, changelog, logs folder.
- **First-run setup wizard** in the app: Bluetooth check, guided pairing with live status, the three
  iPhone switches (Share System Notifications, Show Notifications, Sync Contacts) with screenshots and
  live ✓ detection, and a send-yourself-a-test step.
- **Pairing without LightBlue** — spike first:
  1. Make the PC appear in iOS Settings › Bluetooth (ANCS service-solicitation advert, or Classic
     pairing from the phone with cross-transport keys). No app needed if it works.
  2. Fallback: QR code → Apple App Clip that pairs in one tap (needs Apple Developer account + review).
  3. Long term: companion iOS app (also unlocks exact charging state, clipboard).
- **Branded installer** (NSIS welcome/finish pages, Start-menu shortcut).
- **Code signing** (avoid SmartScreen "unknown publisher"), **auto-updates** from GitHub Releases,
  opt-in diagnostics.

## Later — product ideas
The small, curated idea backlog (one-time codes, live texts, Ctrl+K actions, tray, reply from the
pop-up, the glance strip with weather, welcome back...) and what we decided *not* to build live in
[docs/PRODUCT.md](docs/PRODUCT.md). Ideas move onto this roadmap only once approved.

## Later — features
- **Delete conversations:** an ✕ on each conversation to remove it from tug (local, with undo; nothing
  on the phone is touched). Stretch: notice deletions made on the phone — only partly possible, since
  message access shows a ~10-message window; a message vanishing from *inside* the window means deleted.
- **Calls:** incoming-call card and recent calls built (`feat/calls`, awaiting a hardware check).
  Dialing over HFP is experimental and gated by a check; if Windows holds the hands-free link, the
  fallback is packaging tug (MSIX/sparse package) for `PhoneLine.Dial`. Audio stays on the phone.
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
