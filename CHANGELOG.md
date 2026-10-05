# Changelog

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
