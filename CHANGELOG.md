# Changelog

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
