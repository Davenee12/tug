# Changelog

## v0.5.11 — 2026-10-07

Tugboat: send photos, files and text between your phone and PC over Wi-Fi, with no app on the
phone. Plus your exact iPhone in the sidebar and a round of fixes. Merged as #83–#90. Tugboat was tried both ways on a real iPhone over home Wi-Fi.

### New
- **Tugboat** (button under your phone in the sidebar, or Ctrl+K "tugboat"): scan the QR code with
  your phone's camera and a tug page opens in Safari or Chrome. Send photos, videos, files and
  pasted text to the PC (files land in Pictures\Tugboat, text on the clipboard), and drag files onto
  tug to send them to the phone. Works on your Wi-Fi only, every transfer is encrypted, and it turns
  off when you close it or after 10 idle minutes. Received files carry Windows' "downloaded"
  mark, so a disguised program triggers the usual warning before it runs.
- **Your exact iPhone** in the sidebar and Settings › iPhone: tug reads the model over Bluetooth
  and shows its name ("iPhone 15 Pro Max") with tug's own drawing of that design.

### Fixed
- **One conversation per person:** WhatsApp and Snapchat sometimes hide an invisible mark in the
  sender's name, which split one person into two conversations (one without their photo).
- **Spotify back/forward 15 s** jump to the exact time instead of restarting the song.
- **Weather updates on its own** every 15 minutes, when you come back to tug, and "now" follows
  the clock between updates.
- **A stalled Bluetooth adapter** no longer looks like the PC waking from sleep: tug pauses,
  reconnects cleanly, says "Reconnecting…", and keeps the iPhone able to find the PC.

### Known issues
- A reply typed in the Windows pop-up is sent, but doesn't appear in the conversation until a later
  release.

### Developer
- `tugboat/` (axum on the LAN IPv4 only, HKDF + XChaCha20-Poly1305 per chunk, MAC-authenticated
  requests, first phone binds the session, resumable uploads, own hyper accept loop with header
  timeout and a 32-connection cap), phone page in `src/tugboat-page/` (bundled @noble/ciphers);
  independent security review: no blocks, all should-fixes applied.
- Bluetooth inventory (`bt_inventory/`): one privacy-safe `bt-inventory:` log line per app run
  and 15 minutes later; findings feed v0.5.13.
- Wake detection compares sleep-inclusive and awake-only clocks on its own thread; GATT
  operations run on bounded helper threads; wedge detector relinks a stalled adapter.
- Personal data removed from fixtures, docs and comments; release builds use
  `npm run build:release`, which strips local paths from the exe.

## v0.5.10 — 2026-10-06

Bug fixing only (every fifth release is a stabilization release). Merged as #77, #79–#81, after an
independent code review whose five blocks were all fixed before merge. Built and checked; hands-on
testing on the iPhone follows.

### Fixed
- **No more all-night reconnect loops.** When the iPhone is locked after a restart, tug shows
  "Unlock your iPhone" and waits longer between tries (up to 2 minutes) instead of retrying every
  30 seconds all night; texts back off the same way (up to 5 minutes). It still reconnects at once
  when Bluetooth comes on, the PC wakes, or you open the iPhone screens.
- **tug reconnects when the PC wakes from sleep**, instead of waiting on an old timer.
- **A split-second Bluetooth blip no longer loses the notifications already on your phone.** A
  real disconnect is still handled as one.
- **Spotify is read once per song** instead of every 30 seconds, so Spotify's rate limit stops
  biting. tug waits a moment after the song changes and checks it's the right song before showing
  its cover and Like, so Like can never save the previous song.
- **Clearing a notification the phone already dropped** no longer shows an "ATT error 0xA2"
  message; it's simply done.
- **The sidebar no longer cuts off** Now Playing's bottom controls on 1366×768 laptops; on shorter
  windows Like/Dislike sit by the title and ±15 s by the times, so every control fits.
- **The Calls list stays put** when a new call arrives.
- **A verification code can't pop up twice** when its text and notification arrive together.
- **Contact names on texts stay right** after contacts change (a deleted contact's name goes,
  names the phone sends with texts stay).
- Removing a notification no longer stalls the queue of notification details behind it.
- After Forget, Settings no longer says the iPhone is locked.

### Developer
- Image caches are capped (Spotify art 100 MB, app icons 50 MB; contact photos are never trimmed),
  trimmed at startup and at most once a minute after writes (`cache_trim.rs`).
- Crash diagnostics: frontend errors and WebView2 process failures are logged (redacted,
  rate-limited) to tug.log (`frontend_log.rs`, `webview_watch.rs`, `errorReport.ts`).
- Link events carry their own timestamp, so blip vs real outage is judged on when it happened, not
  when the actor got to it. Status gains `awaitingUnlock`; Spotify player snapshot gains `trackName`.
- Docs: `docs/COMPANION-PLAN.md` (companion apps, and the settled tug Drop design for v0.5.11).

### Verify on hardware
- Overnight with the phone locked: a handful of retries, "Unlock your iPhone" shown, quick
  reconnect after unlocking.
- Sleep and wake the PC: tug reconnects within seconds.
- Spotify: skip songs; cover and Like follow the right song.

## v0.5.9 — 2026-10-05

Easier connecting, a full Spotify panel, quiet hours, and contact photos. Merged as #63–#75.
Tried on a real iPhone where noted; Spotify search was built and reviewed but Spotify's rate limit
blocked a live test tonight.

### New
- **One-screen connect** replaces the setup wizard: tug opens on its main window with a
  "Connect your iPhone" panel that finds your phone, pairs with the code shown in tug, asks you to
  tap Allow, and waits until all three iPhone switches are on (or "Skip for now") before turning
  into your Feed. The same panel lives in Settings › iPhone. *(Proven in several clean setups.)*
- **Spotify panel:** search and play any song (it continues through its album), add to queue,
  save to Liked Songs, add to your playlists, album and artist pages, Recently played, Your top,
  Up next, drag-to-seek on Now Playing, and **Play on** to choose your iPhone or another Spotify
  device. Ctrl+K: "play <song or artist>", "queue <song>", "spotify".
- **Quiet hours, app mute and VIPs** in Settings › Notifications: hold pop-ups on a schedule, mute
  pop-ups per app, and let chosen people always get through. Calls still ring unless you turn on
  Mute calls.
- **Contact photos** from your iPhone in Messages, Calls, the Feed and Ctrl+K. *(Tried on a real
  iPhone.)*
- **More music controls:** skip back/forward 15 seconds, and Like/Dislike in Apple Music, shown only
  when the player supports them.
- **What's new** card once after each update, and in Settings › About.

### Fixed
- **Connecting:** the panel keeps scanning so a phone made findable later still shows; waits up to
  15 s for the notifications side after pairing; spots a leftover Windows pairing and offers Use or
  Remove; switches go green within about 10 s of being turned on.
- **Texts are never held up by contacts:** contacts re-sync in about a second and photos load
  separately in the background (they had blocked texts for a minute).
- **Low-battery alert** now actually pops up (it used a pop-up path that didn't show).
- **Spotify** stops asking while Spotify says to slow down, asks far less often, and explains a
  wait in plain words.
- One row per iPhone in setup, and the Sync Contacts check reflects what the phone shares now.
- The sidebar no longer scrolls; Settings sections fit better.

### Known limits
- iOS reports every message as a text over Bluetooth, so tug can't tell iMessage from SMS
  (bubbles stay neutral).
- Spotify extras need Premium and, while tug's Spotify app is in Development Mode, a listener
  added in its Spotify dashboard.

## v0.5.8 — 2026-10-05

Smoother setup, texts that arrive instantly, and Spotify. Merged as #48–#61; tried on a real
iPhone where noted.

### New
- **Setup finds your iPhone and pairs it from tug.** Open Settings › Bluetooth on the iPhone and
  tug lists it by name; click Pair, the code shows in tug, and that one pairing brings
  notifications, music, battery and texts. No LightBlue needed. Tapping the PC on the iPhone (or
  Windows' "Add a device" pop-up) still works: tug says Windows will show the code, then offers
  your iPhone. *(Both paths proven on a real iPhone, including two setups from a clean install.)*
- **Setup tells you what to do:** "Look at your iPhone and tap Allow" while the phone waits on
  you, a checklist of the iPhone's three switches naming the one that's off, a confirmed
  **Start over** for old or duplicate pairings, and your phone's real name instead of "iPhone".
- **Texts arrive the moment your phone gets them** (live texts), with **Sending… → Sent** on
  texts you send. If the phone drops the link, tug reconnects by itself; checking every few
  seconds stays as a backstop. *(Connected and held on a real iPhone.)*
- **Codes that arrive as texts show in the Feed** with a **Copy code** button, even when the
  iPhone showed no banner, and pop up in Windows.
- **Settings › Connectors**, starting with **Spotify**: click Connect and sign in. Then your
  playlists (with covers) play on your iPhone, repeat and shuffle work, Like the current song,
  album art on Now Playing, and Ctrl+K "play <playlist>". If Spotify is closed on the iPhone, tug
  asks you to open it and starts the playlist as soon as it's open. *(Tried on a real account.)*

### Fixed
- The Feed keeps your place when notifications come and go; Messages opens on the newest text
  and doesn't pull you down while you read back.
- A new pairing no longer drops texts and notifications every few seconds while the iPhone waits
  for you to tap Allow.
- tug doesn't touch a phone until you've picked it in setup.
- Setup shows your iPhone once (it could appear twice, and one of the two couldn't pair).
- The setup checklist's Sync Contacts reflects what the phone is sharing right now, not contacts
  tug saved earlier.
- **Calls are never ended or declined by tug tidying up.** Dismissing a call notification (the ✕,
  or opening that person's conversation) could hang up a call in progress, such as a WhatsApp
  call right after answering it. *(Added to v0.5.8 on 2026-10-05; reinstall 0.5.8 from the
  release page if you downloaded it earlier.)*

### Known limits
- Spotify extras need Spotify Premium; while tug's Spotify app is in Spotify's Development Mode,
  only listeners added in its Spotify dashboard (up to 5) can connect.
- iOS only shares the 10 newest incoming texts with a new install, and no sent texts.

## v0.5.7 — 2026-10-05

tug feels built into Windows, plus a full stabilization pass. Media keys verified on a real iPhone;
the rest is built and checked, with hands-on testing continuing.

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

Calls, real app icons and a steadier connection. Tested on a real iPhone.

### New
- **Incoming calls ring as a card over tug:** the caller's name large, rings rippling out, how long
  it's been ringing. **Answer** or **Decline** (Enter answers after a beat; Esc only hides the card,
  the phone keeps ringing). It goes as soon as the call does; the Windows pop-up still rings from the
  tray.
- **Calls tab:** your iPhone's recent calls (incoming, outgoing, missed), named from your contacts,
  kept current while it's open. Needs **Sync Contacts**.
- **Call back from tug:** any missed call still on your phone can be called back, from the Calls
  tab, the Feed ("Call back") or Ctrl+K (`call zoe`). Calling anyone else comes in v0.5.7.
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
- **Ctrl+K actions.** Type `text zoe running late`, `play`, `next`, `settings` and more. When a name
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
- **"Use my location"** names the place ("Portland, Oregon") instead of "Your location".
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
- A contact name saved from an inline reply no longer comes out garbled ("Zoer message").
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
- **Inline replies stay in the sender's conversation:** iOS titles them "zoe replied to you", which
  used to open a second conversation with the same message in both.
- **Messaging can't freeze or crash** on a stuck connection or a corrupt Bluetooth packet.
- **Now Playing progress no longer jumps back** when you change the volume.
- Fewer lost notification details around reconnects.

### Developer
- Frontend tests (Vitest) added to `npm run check`; 62 Rust tests.

## v0.5.1 — 2026-10-05

### Fixed
- One conversation per person: iOS sometimes pads a contact's name in notifications ("marco "),
  which split a person into two conversations. Names are now matched ignoring extra spaces and case.
- Avatars for names with emoji ("zoe 💜") no longer show a broken character.
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
