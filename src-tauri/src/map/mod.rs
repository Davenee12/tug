//! Bluetooth MAP (Message Access Profile) client: read the iPhone's recent
//! messages and send replies through it, over the Classic Bluetooth pairing.
//!
//! Layering: `obex` (framing) → `session` (RFCOMM transport + MAP operations),
//! with `bmessage` and `listing` as the two MAP object formats. Everything but
//! `session` is pure and unit-tested.

pub mod bmessage;
pub mod listing;
pub mod obex;
#[cfg(windows)]
pub mod session;
