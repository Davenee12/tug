# tug

**Your iPhone on your Windows PC.** tug brings your iPhone's notifications, texts, calls and music
to your PC over Bluetooth, the way a smartwatch or a car kit does. There's no app to install on the
phone, no Mac, no account and no cloud: tug pairs with your iPhone directly and keeps everything on
your PC.

- **Notifications** from every app, with Windows pop-ups you can act on, quiet hours and VIPs.
- **Texts:** read them the moment they arrive and reply from your keyboard, with verification codes
  ready to copy.
- **Calls:** an incoming-call card, recent calls, and call back a missed call.
- **Music:** Now Playing with controls, Windows media keys, and an optional Spotify panel.
- **Tugboat:** photos, files and text between your phone and PC over Wi-Fi, no app needed.
- **Ctrl+K** search across people, texts and notifications, plus quick actions.
- **For developers:** your AI tools (Claude Code, Codex, Cursor, VS Code) and a `tug` command can
  use your phone, always with you in charge.

> Screenshots are coming. Until then, `npm run dev` shows the whole UI with sample data (see
> [Build from source](#build-from-source)).

**Current release: v0.5.14** · [What's changed](CHANGELOG.md) · [Everything tug can do](docs/FEATURES.md) · [Roadmap](ROADMAP.md)

## Download

Get the installer (`tug_<version>_x64-setup.exe`) from
**[Releases](https://github.com/Davenee12/tug/releases)**. It installs for your Windows account
only, with no admin rights needed.

**The installer isn't code-signed yet** (signing is planned; see the [roadmap](ROADMAP.md)), so
Windows will be cautious:

- **SmartScreen** shows "Windows protected your PC". Click **More info**, check the file is the one
  you downloaded from this repository's Releases page, then click **Run anyway**.
- **Smart App Control** (Windows 11, on by default on some fresh installs) may block unsigned apps
  outright, with no Run anyway. If it does, tug can't be installed on that PC until releases are
  signed. Turning Smart App Control off in Windows Security may not be reversible without
  resetting Windows, so think twice before you do.

To check your download wasn't changed, compare its SHA-256 checksum with the one listed in that
release's notes:

```powershell
Get-FileHash .\tug_0.5.12_x64-setup.exe -Algorithm SHA256
```

## Requirements

- **Windows 10 version 2004 or later, or Windows 11**, on an x64 PC. ARM64 PCs run it under
  emulation, untested.
- **A Bluetooth LE adapter that can act as a peripheral.** Most Bluetooth 5 adapters can; some
  Realtek, MediaTek and older adapters can't. tug tells you if yours can't (a Bluetooth 5 USB
  adapter fixes it).
- **An iPhone.** Android phones can't pair with tug yet (Tugboat works from Android browsers).
- **WebView2 Runtime**, already part of Windows 11 and up-to-date Windows 10.
- The app is in **English** only for now.
- Tugboat needs an **IPv4 home or office network** set to **Private** in Windows.

## Quick start

tug opens with a **Connect your iPhone** panel (also in Settings › iPhone):

1. On the iPhone, open **Settings › Bluetooth** and keep it open. tug lists your phone by name.
2. Click **Pair**, check the code in tug matches the phone, and confirm. That one pairing brings
   notifications, music, battery and texts.
3. Tap **Allow** on the iPhone when it asks, then turn on the switches tug shows (iPhone
   Settings › Bluetooth › ⓘ next to this PC): **Share System Notifications** for the Feed, **Show
   Notifications** for texts, and **Sync Contacts** for names, photos and calls. tug turns each one
   green as it comes on.

Good to know:
- Message text missing from notifications? iOS only shares what the lock screen shows: set
  iPhone Settings › Notifications › Show Previews to **Always**.
- After the iPhone restarts, nothing connects until it's unlocked once; tug says "Unlock your
  iPhone" and reconnects by itself.
- On a fresh install, iOS shares only the 10 newest incoming texts, and never texts sent from the
  phone itself. Everything after that is kept in tug's history.

## Features

The full list, area by area, with what iOS doesn't allow: **[docs/FEATURES.md](docs/FEATURES.md)**.

**Spotify:** the Spotify connector (Settings › Connectors) is an **invite-only beta**. tug's
Spotify app is in Spotify's Development Mode, which allows only 5 accounts added by hand, and
playback controls need Spotify Premium. The phone's own music controls work for everyone.

## Developer tools

Turn on **Settings › Developer tools › Let AI tools use tug** (off by default) and your AI tools can
use your phone through tug's local MCP server: the newest 2FA code, a search of your texts and
notifications, GitHub/CI/deploy notifications, Tugboat files (a phone screenshot straight into the
agent), what's playing, and texts that are only sent after you click **Send** in tug. Codes, music
and texts each start switched off. Setup is copy-paste from that page, for example:

```bash
claude mcp add --scope user tug -- "C:\Users\<you>\AppData\Local\tug\bin\tug.exe" mcp
```

In a terminal: `tug code --copy`, `tug boat screenshot.png`, `tug text Sam "On my way"`,
`tug status`. Setup for Codex, Cursor and VS Code, the safety model and troubleshooting:
**[docs/DEVELOPERS.md](docs/DEVELOPERS.md)**.

## Privacy

tug has **no servers, no account, no analytics and no telemetry**. It talks to your iPhone over
Bluetooth and keeps your history (notifications, texts, contacts, settings) in a database on your
PC, in `%APPDATA%\dev.davejames.tug`.

What goes online, and only when you use it:
- **Weather** (if you set it up): forecasts from Open-Meteo for your place rounded to about 1 km;
  "Use my location" names the place through BigDataCloud.
- **App icons** (on by default, switchable): Apple's App Store lookup, sending only the app's
  identifier.
- **Spotify**, only if you connect it.

Tugboat stays on your local network, and the developer bridge only accepts programs on your PC
running as you. Copy diagnostics removes numbers, names and message text before anything reaches
the clipboard. Full details, and how to delete everything: **[docs/PRIVACY.md](docs/PRIVACY.md)**.

## Build from source

**Prerequisites** (Windows 10/11):
- [Node.js](https://nodejs.org) 22 LTS
- [Rust](https://rustup.rs) stable, with `clippy` and `rustfmt`
- Visual Studio Build Tools with **Desktop development with C++** and a **Windows 10/11 SDK** (it
  provides `RC.EXE`, which the build needs)
- WebView2 Runtime (already on Windows 11)

```bash
npm install
npm run dev            # the UI in a browser with sample data, no phone needed (src/lib/devMock.ts)
npm run tauri dev      # the real app, with Bluetooth
npm run check          # Tugboat page build + vue-tsc + vitest + cargo fmt + clippy -D warnings + cargo test
npm run build:release  # installer: src-tauri/target/release/bundle/nsis/tug_<version>_x64-setup.exe
```

Use `npm run build:release` rather than a bare `npx tauri build`: it also builds the `tug` command
and strips local paths (your user folder) from the exes. Browser preview scenarios (`/?setup`,
`/?call`, `/?whatsnew`, …) and how tug fits together are in
**[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

**Spotify in your own build:** tug's Spotify Client ID is built in as `CLIENT_ID` in
`src-tauri/src/spotify/mod.rs`. A fork should create its own app in the
[Spotify developer dashboard](https://developer.spotify.com/dashboard), register the redirect URI
`http://127.0.0.1:8972/callback`, add its listeners while in Development Mode, and put its own
Client ID there.

## Support tug

tug is free and open source, and stays that way. If it saves you time, you can
[sponsor its development on GitHub](https://github.com/sponsors/Davenee12). Sponsorships go
first towards code signing, so Windows stops warning about the installer.

## Contributing

Bug reports, fixes and ideas are welcome. Read **[CONTRIBUTING.md](CONTRIBUTING.md)** for setup,
the branch and pull request flow, and the rules that keep tug honest (logic in pure modules with
tests, `npm run check` passing, no personal data in fixtures). Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Please report vulnerabilities privately through the repository's **Security** tab (**Report a
vulnerability**), not in public issues. Scope and what to expect: **[SECURITY.md](SECURITY.md)**.

## License

[MIT](LICENSE) © 2026 tug contributors.

**Trademark:** the MIT licence covers the code, not the name. The **tug** name and logo are the
owner's. If you publish a fork, please give it a different name and icon so people don't confuse it
with tug.

tug isn't affiliated with or endorsed by Apple, Microsoft or Spotify. iPhone is a trademark of
Apple Inc.
