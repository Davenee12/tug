# What tug can do

Everything tug does today (v0.5.12), one line each. tug works with **no app on the phone**: it
uses what iOS shares with any Bluetooth accessory, like a smartwatch or a car kit. The limits that
come from that are at the end, in [What iPhone doesn't allow (yet)](#what-iphone-doesnt-allow-yet).

## Connection & pairing

- On first run, a **Connect your iPhone** panel takes the Feed's place; the same panel lives in
  Settings › iPhone.
- Open Settings › Bluetooth on the iPhone and tug lists it by name; click **Pair** and check the
  code matches. One pairing brings notifications, music, battery and texts.
- Pairing from the iPhone's side or from Windows' "Add a device" also works; tug says when Windows
  will show the code.
- Keyboards, headphones and other accessories are left out of the list, so your iPhone is easy to
  spot.
- tug asks you to **tap Allow** on the iPhone when iOS is waiting for you.
- A checklist of the iPhone's three switches (Share System Notifications, Show Notifications, Sync
  Contacts) turns each one green as it comes on, with **Check again** and **Skip for now**.
- Once notifications work, a dismissible "Get more from tug" banner on the Feed points out any
  optional switch that's still off.
- tug explains pairing problems in plain words: more than one iPhone paired, the iPhone has
  forgotten this PC, or a leftover pairing from before (with Use or Remove).
- **Start over** (Forget this iPhone) removes both of Windows' pairings with the phone and clears
  saved recent calls; your history stays.
- It tells you when Bluetooth is off (with a button to turn it on), when the PC has no Bluetooth,
  and when the adapter can't host a connection.
- One clear connection status everywhere: Connected, Connecting…, Reconnecting…, Unlock your
  iPhone, iPhone away, Waiting for iPhone or Bluetooth is off.
- tug reconnects by itself when the phone comes back in range, after a Bluetooth blip, and when the
  PC wakes from sleep.
- On wide windows, a Connection panel opens beside the Feed when the phone has been away for a
  while.
- Settings › iPhone shows connection health (Bluetooth, visible to iPhone, notifications, music,
  battery, texts, contacts, recent calls), each with a plain fix, and the last error.
- **Visible to iPhone** (on by default, also in the sidebar) lets you stop tug advertising to the
  phone.
- **Copy diagnostics** and **Open logs folder** help with support (Settings › iPhone and About).
- The sidebar shows your exact iPhone model with tug's own drawing of it (home button, notch or
  Dynamic Island).
- Opening tug a second time brings the running window forward.

## Notifications & pop-ups

- The **Feed** shows live notifications from every app on your iPhone; chat apps are grouped one
  row per person, other apps stack.
- The Feed shows what's still waiting on your phone; cleared notifications leave the Feed but stay
  in search, and older history loads as you scroll.
- Act on a notification with the phone's own buttons (answer, decline and so on), or **Call back**
  a missed call.
- The ✕ on a row clears it on your iPhone too.
- **Open on the web** opens a notification's web page (for example your inbox for Gmail), or the
  app's website.
- Real app icons, looked up once per app from Apple's App Store and kept on your PC (you can turn
  this off).
- **Windows pop-ups** for new notifications, with a reply box for texts, Mark read, Copy code, Call
  back and Clear where they fit.
- Pop-ups disappear from Windows when the phone clears the notification or tug quits.
- At most 3 pop-ups every 10 seconds; the rest become one "N more notifications" pop-up.
- Notifications that arrive while the iPhone reconnects still pop up (more than three become one
  summary).
- **Pop-up sound** can be turned off if the iPhone's own chime is enough.
- tug warns you when **Windows is blocking its pop-ups**, with a button to the right Windows
  setting.
- **Do not disturb** keeps collecting notifications but stops pop-ups (Settings and the sidebar).
- **Quiet hours** hold pop-ups on a schedule you choose, including overnight.
- **Mute pop-ups** from any app, from its bell icon or in Settings › Notifications.
- **Always let through:** chosen people pop up even during quiet hours and Do not disturb.
- Calls ring through quiet hours and Do not disturb; **Mute calls** silences call pop-ups (people
  you always let through still ring).
- **Filter unknown senders** (on by default): texts from numbers that aren't in your contacts and
  you've never texted go to an Unknown senders list with no badge or pop-up; codes still pop up.
- A **tray icon** opens tug (and your newest unread conversation), has Settings… and Quit, and
  shows a dot when you have unread texts.
- Closing the window keeps tug running in the tray (you can change this), with a one-time hint.
- A **low phone battery** pop-up at 20% and again at 10%.

## Messages (texts)

- Texts arrive the moment your iPhone gets them, and you can **reply** from tug (Enter sends,
  Shift+Enter adds a line).
- Start a **new conversation** with Ctrl+N or +: recent people, contacts, or a typed number.
- Drafts are kept per conversation while tug is open, so text typed to one person can't go to
  another.
- When a contact has several numbers, **To:** lets you choose which one.
- Honest send status: Sending…, Sent, Not sent (with **Retry**, which resends the same message to
  the same number), or "May have sent — check your iPhone" when a second try could text them twice.
- **Delete a conversation** from tug (your iPhone keeps it), with Undo; a new text from them brings
  it back.
- Opening a conversation marks its texts read on your iPhone and clears their notifications, even
  if the phone was out of reach at the time.
- Unknown senders sit in their own section, with **Move to conversations**.
- A photo with no caption reads "Photo or attachment — open it on your iPhone".
- When the iPhone may not have shared every text from while tug was away, the conversation says so.
- Messages from other chat apps (WhatsApp and so on) show as read-only conversations: reply on your
  phone.
- **Verification codes** in texts and notifications get a **Copy code** button, a code text with no
  notification gets its own Feed row, and **Ctrl+Shift+C** copies the latest code.
- Copying a code clears its notification on the phone.

## Contacts

- With **Sync Contacts** on, names replace numbers, and people who text from an email address get
  their name too.
- Contact photos come from your iPhone and are kept on your PC; tug never fetches them online.
- A number in a different format (with or without the country code) still matches the contact.
- Notifications under a contact's old name stay in the right conversation.
- tug learns a name for an unsaved number from its notification, but only when there's no doubt
  (never from a group text).
- Sync Contacts shows **Off** when the iPhone isn't sharing contacts, and tug notices it being
  turned on or off within seconds while the switches are on screen.
- Names saved earlier keep showing if Sync Contacts is turned off later, and tug says why.

## Calls

- An **incoming-call card** shows who's calling, with Answer and Decline (on the iPhone). Enter
  answers; Esc hides the card while the phone keeps ringing.
- Answer or decline from the Feed row too.
- **Recent calls** (Calls tab) shows the iPhone's call history with Message and Call back buttons,
  and is kept across restarts (needs Sync Contacts).
- **Call back** a missed call from the Feed, a pop-up, the Calls tab or Ctrl+K; the iPhone places
  the call.
- **Call from tug** (experimental, off by default) can dial anyone over the hands-free link, once
  Settings › iPhone › Calls passes its check; on many PCs Windows holds that link, so it may not
  work.

## Music & Spotify

- **Now Playing** shows the song, artist, album and progress from your iPhone, with Previous,
  Play/Pause, Next and Restart song.
- **Volume** buttons repeat when you press and hold.
- **Skip back / forward** when the player supports it; the buttons only say 15 seconds when the
  jump really is 15 seconds.
- **Like / Dislike** in Apple Music, when the player offers it.
- Your keyboard's **media keys** and the **Windows media flyout** control the iPhone's music while
  it's playing.
- One press is one command, even when the phone is slow to answer.
- **Spotify connector** (Settings › Connectors) adds album art, Like, shuffle, repeat and
  drag-to-seek while Spotify is playing on your iPhone.
- The **Spotify panel**: search songs, artists, albums and playlists (each song once), your
  playlists, recently played, your top songs and artists, and Up next.
- Album, artist and playlist pages with Play; a song's menu has Add to queue, Save to Liked Songs,
  Add to playlist, Go to album and Go to artist.
- **Play on** chooses where Spotify plays (your iPhone or another Spotify device) and marks the one
  that's actually playing.
- On a Spotify speaker, Now Playing shows the real song, artist and the device it's playing on.
- Spotify playback controls need **Spotify Premium**.
- **The Spotify connector is an invite-only beta.** tug's Spotify app is in Spotify's Development
  Mode, which allows only 5 accounts that the owner adds by hand; other accounts can't connect yet.

## Tugboat (phone ↔ PC over Wi-Fi)

- Scan a QR code with your phone's camera and a small tug page opens in Safari or Chrome; nothing
  to install, iPhone or Android.
- **Phone → PC:** photos, videos and files (up to 8 GB each) land in **Pictures › Tugboat**, with
  Show and Open folder.
- **PC → phone:** drag files onto tug's window, or Choose files (up to 1 GB each); the phone taps
  Get to save them.
- Text pasted on the phone lands on the PC's clipboard, and text from the PC shows on the phone
  page with a Copy button.
- Everything is encrypted with a key from the QR code, and only the phone that scanned it can join.
- Files from the phone carry Windows' "downloaded from the internet" mark, so a disguised program
  gets the usual warning.
- Tugboat turns itself off when you close it, after 10 idle minutes, when tug hides to the tray, or
  when tug quits; closing mid-transfer asks first.
- "Can't connect?" help appears after 30 seconds (same Wi-Fi, Private network, guest Wi-Fi or VPN).
- Open it from the **Tugboat** button under your phone, Ctrl+K "tugboat", or `tug boat <file>`.
- Tugboat needs an IPv4 home or office network set to **Private** in Windows; tug never changes
  your firewall.

## Weather, battery & the phone card

- An optional **weather card** on the Feed: now, feels like, high and low, the next 24 hours, 7
  days, wind, humidity and a rain or snow outlook.
- Choose a place with **Use my location** (Windows location) or by typing a city.
- Forecasts from Open-Meteo, refreshed every 15 minutes and when you come back to tug.
- °F or °C, on the card or in Settings › Weather.
- The phone card shows its name, model, connection, **battery level** (red when low) and what's
  working (notifications, music, battery).

## Search (Ctrl+K)

- **Ctrl+K** searches people, texts and notifications, including ones you've cleared.
- It also takes actions. Type:
  - `play`, `pause`, `next`, `previous`, `louder`, `quieter` for music;
  - `text sam on my way` (or `message`, `tell`, `send`) to send a text, or `text sam` to open the
    conversation;
  - `call sam` to call back a missed call (or call, if Call from tug is on);
  - `play <song or playlist>` and `queue <song>` with Spotify connected;
  - `copy code`, `clear all`, `mark all read`, `dnd on` / `dnd off`;
  - an app's name to show its notifications;
  - `new message`, `settings`, `spotify`, `tugboat` to open them.
- If a name matches several people, tug lists them and waits for you to pick.

**Other shortcuts:** Ctrl+N new message · Ctrl+Shift+C copy the latest code · Ctrl+, Settings ·
Ctrl+plus / Ctrl+minus / Ctrl+0 zoom · Esc closes dialogs and Settings.

## Developer tools (MCP server and the `tug` command)

Details and setup: [DEVELOPERS.md](DEVELOPERS.md).

- **Let AI tools use tug** (Settings › Developer tools) is **off until you turn it on**; only
  programs on this PC, running as you, can connect.
- AI tools such as **Claude Code, Codex, Cursor and VS Code** connect through tug's local MCP
  server, with copy-paste setup for each.
- Once on, AI tools can: get the newest verification code, search your texts and notifications,
  read developer notifications (GitHub, Slack, Linear, Jira, Sentry, PagerDuty, Vercel, Netlify),
  list and view files sent with Tugboat, see phone status, control music, and ask to send a text.
- Each has its own switch; **verification codes, music controls and sending texts start off**.
- A text from an AI tool is sent only after you click **Send** on tug's card within 2 minutes.
- The **`tug` command**: `tug code [--copy]`, `tug boat <file>`, `tug text <name> "<message>"`,
  `tug status`, `tug mcp`.
- **Add tug to PATH** puts the command in new terminals for your account only; **Remove from PATH**
  undoes it.
- **Connected tools** lists who has used tug and when; **Revoke access** disconnects them all.
- Rate limits on every tool, and tug's log records which tool was used, never what it read.
- ChatGPT's desktop app can't use tug yet (it can't start local tools).

## Settings

- **General:** Windows pop-ups, Pop-up sound, Do not disturb, Low phone battery, Keep running when
  closed, Start with Windows (starts in the tray), Zoom, and the keyboard shortcuts.
- **iPhone:** connection health, Copy diagnostics, Open logs folder, help for the texts pairing,
  Visible to iPhone, Call from tug, and your iPhone's Connect panel.
- **Notifications:** Filter unknown senders, Quiet hours, Mute calls, Always let through, and
  muted apps.
- **Connectors:** connect or disconnect Spotify.
- **Weather:** show, set up, change place or hide; °F / °C.
- **Developer tools:** see above.
- **Data & privacy:** what stays on your PC and what goes online, the App icons switch, and Clear
  history.
- **About:** version, What's new, credits, Copy diagnostics.
- A **What's new** card appears once after each update (and any time from About).

## Privacy & data

- **No tug servers, no account, no analytics, no telemetry, no update checker.**
- Your history (notifications, texts, contacts, settings) lives in a database on your PC:
  `%APPDATA%\dev.davejames.tug\tug.db`. Cached app icons, Spotify art and contact photos sit in
  the same folder; logs are in `%LOCALAPPDATA%\dev.davejames.tug\logs`.
- The Spotify sign-in is kept in Windows Credential Manager, not in a file.
- **What goes online, and only then:**
  - Weather, if you set it up: Open-Meteo, with your place rounded to about 1 km (or the city you
    type); "Use my location" names the place through BigDataCloud.
  - App icons, if App icons is on: Apple's App Store lookup, sending only the app's identifier.
  - Spotify, only if you connect it.
  - Opening a notification's web page opens your browser.
- Tugboat stays on your local network; the developer bridge is local to your Windows account.
- **Copy diagnostics** removes phone numbers, email addresses, names, Bluetooth addresses and your
  phone's name before anything reaches the clipboard; tug's logs never contain message text.
- **Clear history** deletes tug's saved notifications. Deleting a conversation hides it in tug and
  can be undone.

## What iPhone doesn't allow (yet)

These come from what iOS shares with Bluetooth accessories. A tug app on the phone could lift some
of them one day (see [COMPANION-PLAN.md](COMPANION-PLAN.md)).

- **Notification actions** are only the ones iOS offers to accessories (usually answer/decline or a
  positive and negative action); you can't, for example, like a post or archive an email from tug.
- **Texts:** iOS shares only about the **10 newest incoming texts** when tug catches up (on a fresh
  install or after time away); everything after that is kept in tug's history.
- **Texts you send from the iPhone itself** never reach tug, and photos and attachments aren't
  shared (tug says "Photo or attachment").
- **iMessage vs SMS:** iOS usually reports every message as a text, so tug can't tell them apart.
- **Group texts:** a group's messages arrive per sender, so a reply from tug goes to that one
  person, and group pop-ups have no reply box.
- **Message text** only comes through if the lock screen shows previews (Settings › Notifications ›
  Show Previews › Always).
- **Battery:** the level only; iOS doesn't share whether the phone is charging.
- **Clipboard sync** isn't possible without an app on the phone; Tugboat's paste box is the
  one-tap alternative.
- **After the iPhone restarts**, nothing connects until it's unlocked once.
- **Calling anyone** from the PC needs Windows' calling connection, which Windows usually keeps for
  itself (Call from tug is experimental).
- **Android:** tug can't pair an Android phone yet. Tugboat works from Android browsers today.
