# tug roadmap

Order follows the stabilization plan: trustworthy first, features second.
See docs/STABILIZATION.md for the current backlog.

## Now — stabilization sprints
1. Sprint 2: high-severity data-integrity bugs and the messaging crash path.
2. Sprint 3: frontend tests + accuracy audit.
3. Sprint 4: performance and maintainability.

## Next — onboarding & distribution (product readiness)
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

## Later — features
- **Calls:** recent calls (PBAP call history), dial from the PC (HFP; Windows holds the hands-free link,
  so start with a feasibility spike); audio on the phone first.
- **Connectors:** messages already carry a `source`. Android, then Slack, Teams, WhatsApp, Wispr Flow
  into one inbox.
- **PC media** next to the phone's (Windows media sessions, e.g. YouTube in a browser).
- **Full history import** from a local iPhone backup (sent messages, photos/attachments), since iOS
  doesn't expose those over Bluetooth.
- Exact charging indicator (needs companion app or USB trust pairing).
