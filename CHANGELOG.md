# Changelog

## Unreleased

### New
- **Spotify connector (opt-in):** connect your own free Spotify app in Settings › Spotify to get
  what the iPhone's media link can't do for Spotify — working **repeat and shuffle**, a **Like**
  button, **album art** on Now Playing, and your **playlists** to start on your iPhone. Open the
  Playlists panel from the Now Playing card, or press Ctrl+K and type "play &lt;playlist&gt;".
  Sign-in uses OAuth with PKCE through your browser (no client secret); the refresh token is kept in
  Windows Credential Manager, never in plaintext, and the account name and tokens never appear in
  diagnostics. Development-mode apps need the owner to have Spotify Premium and allow up to 5 users.

## v0.5.7 — 2026-10-05

tug feels built into Windows, plus a full stabilization pass. Media keys verified on Dave's iPhone;
the rest is built and checked, with Dave's hands-on testing continuing.

### New
- **Act from the Windows pop-up:** reply to a text right in the notification, Mark read, Copy code,
  Call back a missed call, or Clear — without opening tug.
- **Media keys and the Windows media panel** control your iPhone's music: play/pause, next and
  previous, with the song shown in Windows' media controls.
- **Open notifications on the web:** an Open button on any app's notification goes to that app's
  best page, or its own website (Gmail opens your inbox; a Google alert searches for what it's about).
- **Filter unknown senders:** texts from unsaved numbers, short codes and spam go to a collapsed
  "Unknown senders" list, out of your conversations, the unread badge and pop-ups (verification
  codes still pop up). Move someone to your conversations, or reply, and they're known.
- **Hold to change volume** on the Now Playing card.
- **Connection health** in Settings › iPhone, and **Copy diagnostics** for support (phone numbers,
  emails, your phone's name and Bluetooth addresses are hidden).
- **Start with Windows**, minimized to the tray (off by default).
- **Low phone battery alert** at 20% and 10%.
- **Finding your location** speaks in tug's voice ("Giving the map a little tug…").

### Fixed
- **Settings no longer seems to open and close by itself:** the Connection panel only appears if
  your phone has been away for 30 seconds, and Esc closing search no longer closes Settings too.
- **A draft can't be sent to the wrong person:** drafts belong to their own conversation.
- **Links open in your browser** (they could open File Explorer).
- **Recent calls clear** when you clear your phone's call history.
- **tug can't freeze** if the phone drops mid-connection: every Bluetooth step has a time limit.
- Lighter on the phone: contacts are re-checked at most every 10 s while the switches screen is
  open, and fast checks switch themselves off.
- Unknown numbers stay out of the new-message picker's Recent list.
- tug remembers your phone by its id, so renaming it can't confuse texts.

### Developer
- Live texts groundwork (MNS event-report parser, OBEX server framing, registration request),
  not yet switched on (v0.5.8).
- Calling anyone is parked (v0.5.13+); see ROADMAP "Parked" for the spike findings.

## v0.5.6 — 2026-10-05

Calls, real app icons and a steadier connection. Tested on Dave's iPhone.

### New
- **Incoming calls ring as a card over tug:** the caller's name large, rings rippling out, how long
  it's been ringing. **Answer** or **Decline** (Enter answers after a beat; Esc only hides the card,
  the phone keeps ringing). It goes as soon as the call does; the Windows pop-up still rings from the
  tray.
- **Calls tab:** your iPhone's recent calls (incoming, outgoing, missed), named from your contacts,
  kept current while it's open. Needs **Sync Contacts**.
- **Call back from tug:** any missed call still on your phone can be called back, from the Calls
  tab, the Feed ("Call back") or Ctrl+K (`call tay`). Calling anyone else comes in v0.5.7.
- **Real app icons** in the Feed (Gmail, Instagram, LinkedIn…), fetched once per app from Apple's App
  Store; people keep their initials. Switch off in Data & privacy.
- **More Ctrl+K actions:** `copy code`, `clear all` (never a ringing call), `dnd`, `mark all read`,
  and typing an app's name jumps to its notifications.
- **Tray:** clicking the icon with unread texts opens the newest unread conversation.
- **Now Playing:** ↺ restarts the song.

### Fixed
- **tug reconnects by itself** when Windows closes its Bluetooth connection objects (e.g. after
  changing the iPhone's services in Windows), instead of silently missing notifications and calls.
- **"Texts stopped connecting"** shows in Settings when the texts pairing breaks, with how to fix it.
- **Renaming your iPhone** shows up in tug without re-pairing; contact changes arrive within 15 minutes.
- Logs keep five 2 MB files, so the minutes before a problem are still there.

### Developer
- `map/calls.rs` (PBAP call history), `map/health.rs` (texts pairing health), `app_icons.rs`
  (App Store lookup + cache), `hfp/` (experimental hands-free dialing; Windows' own hands-free driver
  blocks it on most PCs, see ROADMAP v0.5.7), all pure-tested; `cargo run --example hfp_probe`.
- New commands `get_calls`, `refresh_calls`, `dial`, `app_icon`, `place_lookup`, `set_watching`;
  status `textsPairing`/`textsDevice`/`pairingStale`; settings `ui.dialing`, `ui.appIcons`.

## v0.5.5 — 2026-10-05

Setup, settings and everyday shortcuts (Sprints 5–7), plus fixes from a full fresh-install test.

### New
- **First-run setup.** A step-by-step setup that explains before it asks, skips what's already fine
  and lights up as you go: Bluetooth checks, connecting your iPhone, **Share System Notifications**,
  then **Bring your texts over** (pairing for texts from Windows, with Show Notifications and Sync
  Contacts lighting up live), a first notification, and a few personal touches.
- **Settings page** (gear or **Ctrl+,**): General, iPhone, Notifications, Weather, Data & privacy and
  About, all in one place.
- **One-time codes.** A **Copy** chip on texts and notifications with a verification code; one click
  copies it and clears the notification on the phone. **Ctrl+Shift+C** copies the newest code.
- **Delete a conversation** from tug, with **Undo**. Nothing changes on the phone.
- **Ctrl+K actions.** Type `text tay running late`, `play`, `next`, `settings` and more. When a name
  fits several people, tug asks instead of guessing.
- **Tray icon and taskbar dot.** tug keeps running in the tray when you close it (switchable), shows
  unread texts on its taskbar button, and Quit lives in the tray menu.

### Fixed
- **Setup never offers a keyboard, mouse or headphones as your iPhone.** Devices are told apart by
  what they report about themselves, then by name; anything unclear is asked about, not picked.
- **"Pair again"** appears when your iPhone has forgotten this PC, instead of tug retrying forever.
- **The phone's switches register in seconds.** While a switch is still off (first minutes after
  starting or pairing, or with setup or Settings open), tug checks every 2 seconds.
- **Sync Contacts switched on a little late** no longer means hours without names.
- **"Use my location"** names the place ("Orlando, Florida") instead of "Your location".
- Clearing a stack of notifications no longer stops at the first one the phone has already dropped.
- tug looks further back through your inbox on first sync, as far as the phone allows.
- Reconnects back off at the edge of Bluetooth range instead of retrying every few seconds.
- **Opening the new-message picker no longer clears another conversation on the phone.** The
  phone only clears for a conversation you clicked into, while nothing (search, the picker,
  settings) covers it.
- A notification dismissed on the phone while tug was fetching its details no longer comes back.
- A contact can't pick up an unsaved sender's name from one shared short text ("ok").
- One text the phone won't send over no longer stops newer texts from syncing.
- A text you sent can't get stuck on "Sending…" when the phone reuses a message id.
- A `>` in a text no longer hides the rest of its details (including whether it's read).
- Apps whose name lookup failed get asked again instead of showing a raw app id.
- Pop-up notifications are rate-limited: a burst becomes "N more notifications" (calls always ring).
- Esc closes search and the new-message picker from anywhere inside them.
- Weather: the "now" dot stays on its bar, and a stalled request can't leave the card loading.
- Removed the out-of-date "What tug can and can't do" box from settings.

### Developer
- GitHub Actions runs the full check suite on every PR and on main (Windows).
- `ble/actor.rs` split into six focused modules (a pure move, verified function by function).
- GATT notification handlers unregister themselves; GATT reads/writes time out after 10 s.
- Schema v5 (`hidden_at`) for deleted conversations. Discovered devices carry a `kind`; status carries
  `pairingStale`. Reverse geocoding runs natively (`place_lookup`); `set_watching` drives fast checks.

## v0.5.4 — 2026-10-05

Phone sync and weather (Sprint 4).

### New
- **Weather on the Feed.** A quiet card with the temperature (°F/°C switch beside it), today's high
  and low, your place and its local time, and the one thing worth knowing ("Rain likely around
  4 PM"). Drag it down for the next 24 hours and the week, drag it back up to close. Off until you
  choose **Use my location** or type a city; hide or change it any time (also in settings).

### Fixed
- **Opening a conversation clears it on the phone.** Notifications for the conversation you're
  reading leave your lock screen and the Feed, and its texts are marked read over Bluetooth, so the
  unread dot in the phone's Messages app clears too.
- **Renaming a contact on your phone no longer splits their conversation.** tug remembers earlier
  names, including renames made before this version, and keeps everything under the current one.
- A contact name saved from an inline reply no longer comes out garbled ("Tayr message").
- Message times sent with a time zone are kept instead of falling back to the sync time.
- A name the phone sends with a text shows until the number is a known contact.

### Developer
- Schema v3 (`unread_on_phone`, open-notifications index) and v4 (`contact_aliases`).
- Large-history timing test (50k notifications, 10k texts): every query the UI runs stays under
  20 ms. `cargo audit`: no vulnerabilities. 75 Rust tests, 25 frontend tests.

## v0.5.3 — 2026-10-05

Search release (Sprint 3).

### New
- **Search everything with Ctrl+K.** One search box finds **people** (by name or part of their
  number), **texts** and **notifications**, including ones you've already cleared. Pick a result to
  jump straight to it: texts open the conversation and highlight the message.

### Fixed
- Starting a conversation with **+** no longer leaves an empty duplicate once their first text
  arrives, and follows the text if it lands under their contact name.
- Notifications or texts that arrive while tug is starting up are no longer lost from the screen.
- Pop-ups (pairing, new message, search) keep keyboard focus inside them and close with Esc; the
  pairing pop-up starts on Cancel so a stray Enter can't approve it. Ctrl+N no longer fires while
  typing.
- The all-caught-up hint stays on one line.

### Developer
- Versioned database migrations (`PRAGMA user_version`); v2 adds full-text search over texts and
  indexes existing history on first launch. 67 Rust tests, 15 frontend tests.

## v0.5.2 — 2026-10-05

Stability release (Sprint 2): fixes found by a full review of the app, each with a regression test.

### Fixed
- **Replies can no longer go to the wrong person.** When one name has several numbers (two contacts
  with the same name, or one person with two phones), the reply box shows a **To:** picker with the
  number clearly chosen, instead of silently using whoever texted last.
- **No more wrong contact names:** tug only learns a name from a matching text when the match is
  unambiguous.
- **Notifications no longer vanish after a reconnect** when fetching their details fails; tug retries,
  and won't mark anything "cleared" unless it's sure.
- **Two identical notifications or texts** (e.g. two "ok"s) are both kept instead of merged.
- **Inline replies stay in the sender's conversation:** iOS titles them "tay replied to you", which
  used to open a second conversation with the same message in both.
- **Messaging can't freeze or crash** on a stuck connection or a corrupt Bluetooth packet.
- **Now Playing progress no longer jumps back** when you change the volume.
- Fewer lost notification details around reconnects.

### Developer
- Frontend tests (Vitest) added to `npm run check`; 62 Rust tests.

## v0.5.1 — 2026-10-05

### Fixed
- One conversation per person: iOS sometimes pads a contact's name in notifications ("damian "),
  which split a person into two conversations. Names are now matched ignoring extra spaces and case.
- Avatars for names with emoji ("tay 🤎") no longer show a broken character.
- Contacts: when Sync Contacts is off, tug now shows the switch to turn on (iOS refuses with an
  unusual code that was previously reported as a generic error).

### Improved
- New message is a pop-up (Ctrl+N or +): people you've recently texted appear first without typing,
  then all contacts A–Z; search ignores case and accents and covers your message history.
- Replying from tug clears that person's notifications on your iPhone.

## v0.5.0 — 2026-10-04

First release. Verified end to end on an iPhone 15 Pro Max with Windows 11.

### Notifications
- Every iPhone notification mirrored over Bluetooth LE (Apple Notification Center Service), with
  Answer / Decline / Clear acting on the phone.
- Compact feed: one row per conversation and per app, new-message badges, ✕ to clear a whole row.
  Anything cleared in tug, on the phone or on the watch leaves the feed — including items cleared
  while tug was closed.
- Searchable history kept on the PC; Windows toasts with per-app mute and do-not-disturb.

### Messages
- Reads your recent texts over Bluetooth MAP — including ones that never became notifications
  because you read them in the open conversation — and keeps them so history grows on the PC.
- Reply from the PC, and start a new conversation with **+** (contact name or number). iOS chooses
  SMS or iMessage. "Sent via iPhone" means the phone accepted it, not that it was delivered.
- One conversation per person: notifications and messages merged and de-duplicated; names from
  your contacts (PBAP) or learned automatically.

### Phone
- Now playing with play/pause/skip/volume, including what's already playing when tug starts.
- Battery level.
- Pairs with an iPhone already paired for calls/audio (e.g. through Phone Link); reconnects on its own.

### App
- Ctrl +/−/0 and Ctrl+wheel zoom, remembered. Single instance. Black rope app icon.

### Known limits
- No group texts, photos, or messages sent from the phone itself (iOS doesn't share them).
- No charging indicator yet (iOS exposes no charging flag over Bluetooth LE).
- First pairing uses the free LightBlue app on the iPhone.
