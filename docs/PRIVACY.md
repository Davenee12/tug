# tug privacy policy

*Last updated: 2026-10-17 (album art)*

**In short: tug has no servers and no accounts, and it collects nothing.** It talks to your iPhone
over Bluetooth and keeps what it receives on your PC. Nothing is sent to the people who make tug.
The few things that go online are listed below, and each one only happens for a feature you use.

## What tug doesn't do

- No tug servers, no sign-up, no account.
- No analytics, telemetry, crash reporting, advertising or tracking.
- No update checks to anyone but GitHub, where tug's releases live (see below).
- Nothing you receive (notifications, texts, contacts, calls, files) is sent anywhere by tug.

## What stays on your PC

tug stores what it receives from your iPhone so you can see your history and search it. It's kept
in your Windows user profile, readable by your Windows account:

| What | Where |
|---|---|
| Notifications, texts, contacts, recent calls and settings (a SQLite database, `tug.db`) | `%APPDATA%\dev.davejames.tug` |
| Contact photos, app icons, album art and Spotify album art (caches) | `%APPDATA%\dev.davejames.tug` |
| Logs (rotated, at most about 10 MB), the window's web storage (for example the weather forecast cache), the developer tools' access token, and half-received Tugboat files | `%LOCALAPPDATA%\dev.davejames.tug` |
| Files you receive with Tugboat | Your **Pictures\Tugboat** folder |
| Texts brought in from an iPhone backup (if you use **Bring in your full history**) | In `tug.db`, with the rest of your texts |
| Photos and videos brought in from an iPhone backup, and their small previews | `%LOCALAPPDATA%\dev.davejames.tug\attachments` |
| While a backup is being read: its decrypted message database and each photo on its way in | `%LOCALAPPDATA%\dev.davejames.tug\backup-tmp`, deleted as soon as it's done (and at the next start if tug was closed mid-way) |
| Your Spotify sign-in (if you connect Spotify) | Windows Credential Manager (`tug/spotify-refresh-token`) |

tug's logs record connection events and errors, never message text or contact names (album art
lookups are logged as found, no match or failed, with how long they took: never the artist or
album). **Copy
diagnostics** (Settings › iPhone or About) copies recent logs to your clipboard only when you press
it, with phone numbers, email addresses, names, Bluetooth addresses and your phone's name removed;
it goes nowhere unless you paste it somewhere yourself.

## What goes online, and only when you use it

| Feature | Who it talks to | What it sends | When |
|---|---|---|---|
| **Weather** | Open-Meteo (`api.open-meteo.com`, `geocoding-api.open-meteo.com`) | Your chosen place's coordinates, rounded to 2 decimals (about 1 km), or the city name you type to search | Only after you set up the weather card; then every 15 minutes and when you come back to tug, until you hide it |
| **"Use my location"** for weather | Windows location services, then BigDataCloud (`api.bigdatacloud.net`) | Rounded coordinates, to name the place | Only when you press Use my location |
| **App icons** | Apple's public App Store lookup (`itunes.apple.com`) and Apple's image servers | The app's identifier (for example `com.example.app`), once per app; nothing about you or your notifications | While **App icons** is on (Settings › Data & privacy; on by default) |
| **Album art** | Apple's public iTunes Search API (`itunes.apple.com`) and Apple's image servers | The artist and album name of the song playing on your iPhone, and nothing else: no account, no song title, nothing about you. Only for albums tug has no cover for yet; an album Apple doesn't know is asked again after a day at most | While **Show album art** is on (Settings › General; on by default). When Spotify is connected and playing, its own cover is used and nothing is looked up |
| **Spotify** | Spotify (`accounts.spotify.com`, `api.spotify.com`, Spotify's image servers) | Your Spotify sign-in, and the requests you make (search, play, queue, like) | Only after you connect Spotify in Settings › Connectors |
| **Open on the web** | The website, in your browser | Whatever your browser sends | Only when you click it |
| **Automatic updates** | GitHub (`github.com` and GitHub's download servers, `objects.githubusercontent.com`) | An ordinary web request for tug's `latest.json`, then the installer if there's a new version; nothing about you, your phone or your data | In release builds set up for updates: shortly after tug starts and then every 6 hours, while **Download updates automatically** is on (Settings › About; on by default); otherwise only when you press **Check for updates**. Builds without an update key never check |
| **Get it** (HEIF Image Extensions or HEVC Video Extensions, when iPhone photos can't be shown) | The Microsoft Store page, in your browser | Whatever your browser sends | Only when you click it |

**Spotify** asks for permission to: read your private and collaborative playlists; read and
control playback; read what's currently playing; and read and change your Liked Songs. The sign-in
returns to tug on your own PC (`127.0.0.1`), and Spotify's policies apply to what Spotify receives.
Disconnect any time in Settings › Connectors, which removes the stored sign-in.

**Automatic updates** only ever contact GitHub. Like any web request, GitHub sees your IP address
and that a copy of tug asked for its latest version; GitHub's privacy statement applies to that.
tug sends no identifier, no version history and nothing from your phone. A downloaded update is
checked against tug's signing key before it can be installed, and tug only restarts to install it
when you click **Update** (or quit from the tray).

## Features that stay local

- **Tugboat** (phone ↔ PC transfers) runs a small server on your local network only while the
  Tugboat panel is open. Everything is encrypted with a key from the QR code, only the phone that
  scanned it can join, and nothing goes over the internet. It stops when you close it or after 10
  idle minutes. tug never changes your Windows Firewall. Your clipboard is read only when you press
  **Send clipboard to iPhone**, held in memory (never saved) for at most 2 minutes until your phone
  has it, and never written to tug's logs. Text you paste on the phone is kept out of Windows'
  clipboard history and cloud clipboard sync.
- **Bring in your full history** (Settings › Messages) only runs when you start it. tug reads a
  backup of your iPhone that the Apple Devices app or iTunes already made **on this PC**, copies
  your texts into its database and your photos and videos into its own folder (never your
  Pictures), and goes nowhere online to do it. For an encrypted backup you type its password;
  tug uses it once, in memory, to unlock the backup, and never saves it, logs it or sends it
  anywhere. The decrypted message database exists only in tug's private temporary folder while
  it's read, and is deleted straight after (or at tug's next start if it was closed mid-way). tug's
  logs record how many texts and photos came in and how long it took, never a word of a message,
  a name, a number, a file name or the password. Once brought in, texts and photos from an
  encrypted backup are kept like the rest of tug's history (see the table above), no longer behind
  the backup's password. They're part of your history in tug like any text, so if you let AI tools
  search your texts (Developer tools), that search includes them. **Remove imported history** in
  the same place deletes everything it brought in.
- **Voice typing** is **off by default**. When you turn it on and hold its shortcut, tug listens to
  your microphone only while the keys are down, and Windows' own speech recognizer turns it into
  text **on this PC**: the recording is never sent anywhere, never saved to disk, and dropped as
  soon as the words are typed. tug's logs note how long you spoke and how many characters were
  typed, never what you said. It downloads nothing.
- **Developer tools** are **off by default**. When you turn them on, AI tools and the `tug` command
  on your PC can connect through a local channel (a Windows named pipe) that only your Windows
  account can open; there's no network port. AI tools you connect can read only what you allow
  with the switches in Settings › Developer tools (verification codes, music control and sending
  texts start off), and a text is only ever sent after you click **Send** in tug. What an AI tool
  does with what it reads is up to that tool and its provider's privacy policy.
- **Hide previews while sharing or presenting** (Settings › Notifications, on by default) looks at
  this PC only: whether Windows reports a full-screen app or presentation mode, whether the display
  is duplicated, and whether a meeting app's or browser's sharing bar is open. To spot those bars,
  tug reads the title of every open browser window (Chrome, Edge, Brave, Vivaldi, Opera) and Teams
  window, which can include the page you're on; it compares it on the spot and never keeps or logs
  a window title, and its log names only which kind of signal it saw. When hiding starts, tug takes
  back its text and notification pop-ups still on screen or in Action Center (they stay in tug's
  Feed); its own pop-ups, like low battery, stay.

## Deleting your data

- **Inside tug:** Settings › Messages › **Remove imported history** deletes every text, photo and
  video tug brought in from an iPhone backup (texts it got over Bluetooth stay). Your backup itself
  is never changed. Settings › Data & privacy › **Clear history** deletes tug's saved notifications,
  and the imported history too.
  Deleting a conversation hides it in tug. **Start over** (Settings › iPhone) unpairs the phone and
  clears saved recent calls. Disconnecting Spotify removes its sign-in.
- **Everything:** uninstall tug (Windows Settings › Apps › Installed apps › tug › Uninstall; tick
  the uninstaller's option to delete the app's data if it offers one), then delete any of these that
  are left:
  - `%APPDATA%\dev.davejames.tug`
  - `%LOCALAPPDATA%\dev.davejames.tug` (this includes photos brought in from an iPhone backup)
  - `Pictures\Tugboat` (if you want to remove received files)
  - In Windows Credential Manager › Windows Credentials, the `tug/spotify-refresh-token` entry
- To remove the PC from your iPhone, open iPhone Settings › Bluetooth, tap ⓘ next to this PC, and
  choose Forget This Device.

## Your iPhone and other services

tug only receives what iOS shares with Bluetooth accessories and what you allow with the iPhone's
switches (Share System Notifications, Show Notifications, Sync Contacts). You can turn any of them
off on the iPhone at any time. Apple, Microsoft, Spotify, Open-Meteo, BigDataCloud and GitHub have
their own privacy policies for what they receive.

## Changes and contact

Changes to this policy are made in this file, and its history is public in the repository. Questions
or concerns: open an issue on GitHub, or for anything sensitive, use the repository's **Security**
tab (**Report a vulnerability**) to reach the maintainer privately.
