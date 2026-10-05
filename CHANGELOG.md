# Changelog

## Unreleased

Calls. Not yet verified on hardware.

### New
- **Incoming calls ring as a card over tug:** the caller's name large, rings rippling out, how long
  it's been ringing. **Enter** answers, **Esc** declines, ✕ hides it while the phone keeps ringing.
  It goes as soon as the call does. The Windows pop-up still rings through from the tray.
- **Calls tab:** your iPhone's last 50 calls (incoming, outgoing, missed), named from your contacts,
  with a Message button on each. Needs **Sync Contacts**; refreshes after each call.
- **Call from tug (experimental):** Settings › iPhone › Calls checks whether this PC can reach the
  iPhone's hands-free link; only then do Call buttons appear (recent calls, conversations, the
  new-message picker). You talk on the phone.

### Developer
- `map/calls.rs` parses PBAP call history; `hfp/` builds and parses HFP AT commands (both pure,
  tested). `cargo run --example hfp_probe` checks the hands-free link, dials, or pulls recent calls.
- New commands `get_calls`, `refresh_calls`, `dial`; event `calls`; setting `ui.dialing`.

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
