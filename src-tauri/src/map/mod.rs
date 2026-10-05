//! Bluetooth MAP (Message Access Profile) client: read the iPhone's recent
//! messages and send replies through it, over the Classic Bluetooth pairing.
//!
//! Layering: `obex` (framing) → `session` (RFCOMM transport + MAP operations),
//! with `bmessage` and `listing` as the two MAP object formats, `mns_event` for the
//! event reports the phone pushes once notifications are registered, and `mns` for the
//! notification server that receives them (its OBEX logic pure, its WinRT socket plumbing
//! thin). Everything but `session` and `mns`'s WinRT half is pure and unit-tested. PBAP
//! (contacts in `vcard`, call history in `calls`) rides the same Classic pairing and OBEX code.

pub mod address;
pub mod bmessage;
pub mod calls;
pub mod health;
pub mod listing;
pub mod mns;
pub mod mns_event;
pub mod obex;
pub mod pick;
pub mod service;
#[cfg(windows)]
pub mod session;
pub mod vcard;
