# Changelog

## v0.5.4 — 2026-10-05

Phone sync and weather (Sprint 4).

### New
- **Weather on the Feed.** A quiet card with the temperature (°F/°C switch beside it), today's high
  and low, your place and its local time, and the one thing worth knowing ("Rain likely around
  4 PM"). Drag it down for the next 24 hours and the week, drag it back up to close. Off until you
  choose **Use my location** or type a city; hide or change it any time (also in settings).

### Fixed
- **Opening a conversation clears it on the phone.** Notifications for the conversation you're
  reading leave your lock screen and the Feed, and its texts are marked read over Bluetooth (on
  iPhone, the Messages app's own unread dot still needs the phone; Android clears fully).
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
