# tug privacy policy

*Last updated: 2026-10-07 (tug v0.5.12)*

**In short: tug has no servers and no accounts, and it collects nothing.** It talks to your iPhone
over Bluetooth and keeps what it receives on your PC. Nothing is sent to the people who make tug.
The few things that go online are listed below, and each one only happens for a feature you use.

## What tug doesn't do

- No tug servers, no sign-up, no account.
- No analytics, telemetry, crash reporting, advertising or tracking.
- No update checks: tug never contacts anyone to see whether there's a new version.
- Nothing you receive (notifications, texts, contacts, calls, files) is sent anywhere by tug.

## What stays on your PC

tug stores what it receives from your iPhone so you can see your history and search it. It's kept
in your Windows user profile, readable by your Windows account:

| What | Where |
|---|---|
| Notifications, texts, contacts, recent calls and settings (a SQLite database, `tug.db`) | `%APPDATA%\dev.davejames.tug` |
| Contact photos, app icons and Spotify album art (caches) | `%APPDATA%\dev.davejames.tug` |
| Logs (rotated, at most about 10 MB), the window's web storage (for example the weather forecast cache), the developer tools' access token, and half-received Tugboat files | `%LOCALAPPDATA%\dev.davejames.tug` |
| Files you receive with Tugboat | Your **Pictures\Tugboat** folder |
| Your Spotify sign-in (if you connect Spotify) | Windows Credential Manager (`tug/spotify-refresh-token`) |

tug's logs record connection events and errors, never message text or contact names. **Copy
diagnostics** (Settings › iPhone or About) copies recent logs to your clipboard only when you press
it, with phone numbers, email addresses, names, Bluetooth addresses and your phone's name removed;
it goes nowhere unless you paste it somewhere yourself.

## What goes online, and only when you use it

| Feature | Who it talks to | What it sends | When |
|---|---|---|---|
| **Weather** | Open-Meteo (`api.open-meteo.com`, `geocoding-api.open-meteo.com`) | Your chosen place's coordinates, rounded to 2 decimals (about 1 km), or the city name you type to search | Only after you set up the weather card; then every 15 minutes and when you come back to tug, until you hide it |
| **"Use my location"** for weather | Windows location services, then BigDataCloud (`api.bigdatacloud.net`) | Rounded coordinates, to name the place | Only when you press Use my location |
| **App icons** | Apple's public App Store lookup (`itunes.apple.com`) and Apple's image servers | The app's identifier (for example `com.example.app`), once per app; nothing about you or your notifications | While **App icons** is on (Settings › Data & privacy; on by default) |
| **Spotify** | Spotify (`accounts.spotify.com`, `api.spotify.com`, Spotify's image servers) | Your Spotify sign-in, and the requests you make (search, play, queue, like) | Only after you connect Spotify in Settings › Connectors |
| **Open on the web** | The website, in your browser | Whatever your browser sends | Only when you click it |

**Spotify** asks for permission to: read your private and collaborative playlists; read and
control playback; read what's currently playing; and read and change your Liked Songs. The sign-in
returns to tug on your own PC (`127.0.0.1`), and Spotify's policies apply to what Spotify receives.
Disconnect any time in Settings › Connectors, which removes the stored sign-in.

## Features that stay local

- **Tugboat** (phone ↔ PC transfers) runs a small server on your local network only while the
  Tugboat panel is open. Everything is encrypted with a key from the QR code, only the phone that
  scanned it can join, and nothing goes over the internet. It stops when you close it or after 10
  idle minutes. tug never changes your Windows Firewall.
- **Developer tools** are **off by default**. When you turn them on, AI tools and the `tug` command
  on your PC can connect through a local channel (a Windows named pipe) that only your Windows
  account can open; there's no network port. AI tools you connect can read only what you allow
  with the switches in Settings › Developer tools (verification codes, music control and sending
  texts start off), and a text is only ever sent after you click **Send** in tug. What an AI tool
  does with what it reads is up to that tool and its provider's privacy policy.

## Deleting your data

- **Inside tug:** Settings › Data & privacy › **Clear history** deletes tug's saved notifications.
  Deleting a conversation hides it in tug. **Start over** (Settings › iPhone) unpairs the phone and
  clears saved recent calls. Disconnecting Spotify removes its sign-in.
- **Everything:** uninstall tug (Windows Settings › Apps › Installed apps › tug › Uninstall; tick
  the uninstaller's option to delete the app's data if it offers one), then delete any of these that
  are left:
  - `%APPDATA%\dev.davejames.tug`
  - `%LOCALAPPDATA%\dev.davejames.tug`
  - `Pictures\Tugboat` (if you want to remove received files)
  - In Windows Credential Manager › Windows Credentials, the `tug/spotify-refresh-token` entry
- To remove the PC from your iPhone, open iPhone Settings › Bluetooth, tap ⓘ next to this PC, and
  choose Forget This Device.

## Your iPhone and other services

tug only receives what iOS shares with Bluetooth accessories and what you allow with the iPhone's
switches (Share System Notifications, Show Notifications, Sync Contacts). You can turn any of them
off on the iPhone at any time. Apple, Microsoft, Spotify, Open-Meteo and BigDataCloud have their own
privacy policies for what they receive.

## Changes and contact

Changes to this policy are made in this file, and its history is public in the repository. Questions
or concerns: open an issue on GitHub, or for anything sensitive, use the repository's **Security**
tab (**Report a vulnerability**) to reach the maintainer privately.
