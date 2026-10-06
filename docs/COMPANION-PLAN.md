# tug companion apps — plan

Status: **planned, not started** (Dave, 2026-10-06). tug today needs nothing on the phone: it talks to
the iPhone over Bluetooth using what iOS shares with any accessory (notifications, music, battery,
texts received, contacts, calls). That stays the default. This plan covers what a **tug app on the
phone** would add, in what order, and what each step needs from Dave.

## Why a phone app at all

Without an app, iOS decides what tug can see. With an app, tug can also:

| Capability | iPhone app (Swift) | Android app (Kotlin) |
|---|---|---|
| Send files PC ↔ phone (fast, over Wi-Fi) | Yes, from the share sheet / while open | Yes, any time |
| Clipboard sync | Only while the app is open (iOS shows a paste banner) | Yes, in the background |
| All your texts, incl. ones you sent and full history | No (Messages is closed to apps) | Yes (SMS/RCS via the system) |
| Notifications from every app | Already via Bluetooth (ANCS) | Yes (notification listener) |
| Calls through the PC | Limited (Windows owns the line) | Better (app can bridge audio) |
| Runs in the background | Only briefly (iOS limits) | Yes (foreground service) |
| Distribution | App Store, **$99/year** Apple Developer account + review | Play Store **$25 once**, or a direct APK |

**Takeaway:** Android unlocks the most of the "my devices are one workspace" vision. iPhone gains files
and clipboard-when-open, but Apple still blocks screen mirroring, remote control, reading other apps'
data, and full message history — for every app, Phone Link included.

## Phases

### Phase 0 — now: make what exists rock solid
v0.5.x, keeping the cadence (every 5th version bug-fix only). Reliability first: reconnects, no dropped
notifications, PC sleep/wake, polish of setup, texts, notifications and Spotify. Nothing in later
phases is worth building on an unreliable base.

### Phase 1 — the tug protocol (Rust, desktop side only)
The foundation every companion app talks to. Designed once, in Rust, before any phone code.
- **Device identity:** each device gets its own key pair (Ed25519), stored in Windows Credential
  Manager on the PC (like the Spotify token). Devices recognise each other by key, not by name.
- **Pairing:** reuse tug's existing Bluetooth pairing to exchange keys safely (a QR code as the
  fallback). One pairing, then trusted.
- **Local connection:** find each other on the same Wi-Fi (mDNS), then an encrypted, mutually
  authenticated connection (Noise protocol or TLS with the device keys). Bluetooth remains the
  always-on control link; Wi-Fi is used for speed when available, switching automatically.
- **Messages:** a small, versioned set of events ("clipboard changed", "file offer", "notification
  dismissed"…) — events, not polling. Serialization: Protobuf (works in Rust, Kotlin and Swift).
- **Catch-up:** each side keeps a short event log so a device that was away asks "what did I miss?"
  and replays it.
- Ships with nothing visible until a phone app exists; unit-tested in Rust.

### Phase 2 — Android companion (Kotlin) — the big unlock
- Pair with tug (Phase 1), then: two-way clipboard, drag-and-drop file transfer both ways, full
  SMS/RCS history and sending, notifications from every app with actions, battery and status.
- Distribution: Play Store ($25 once) or a signed APK from tug's releases.
- Needs an Android phone to test on.

### Phase 3 — iPhone companion (Swift)
- Share-sheet "Send to tug" (photos, files, links), files both ways while open, clipboard while open,
  a widget showing PC status.
- Needs the **$99/year Apple Developer account** and App Store review — Dave's call when the time comes.
- Background limits mean Bluetooth (no app) stays the always-on link for notifications and texts.

### Phase 4 — reaching your PC away from home (optional)
- **WebRTC** data channels for a direct, encrypted phone ↔ PC connection over the internet (needs a
  small signaling step to introduce the two devices).
- **Nostr** (optional): for finding your devices and signaling without running a tug server.
  Only if it gives a real advantage over a tiny tug relay; never the foundation.

## What stays the same
- Desktop stack: **Rust + Tauri + Vue + SQLite.** No React rewrite, no C#/.NET, no Python — Rust
  already reaches every Windows API tug needs.
- New languages only arrive with the phone apps: Kotlin (Phase 2), Swift (Phase 3).
- Local-first: devices talk directly; cloud only for Phase 4, and only if needed.

## Decisions for Dave (when we get there)
1. Android first (recommended) or iPhone first?
2. Pay Apple's $99/year (iPhone app) / Google's $25 once (Play Store), or sideload Android only?
3. Is reaching the PC away from home (Phase 4) wanted at all?
