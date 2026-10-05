//! **Experimental.** Placing a call on the iPhone from tug over the Hands-Free Profile (HFP),
//! not verified on hardware. tug acts as a hands-free unit just long enough to dial: open
//! RFCOMM to the phone's Audio Gateway (service class 0x111F), set up the Service Level
//! Connection (AT+BRSF, AT+CIND=?, AT+CIND?, AT+CMER), send `ATD<number>;`, then let go.
//! The call stays on the phone, and so does its audio: tug never takes the SCO audio link.
//!
//! Why this route (researched 2026-10-05, see the PR for the full notes):
//! - The supported Windows API, `Windows.ApplicationModel.Calls` (`PhoneLineTransportDevice`,
//!   `PhoneLine.Dial`), is what Phone Link and MyPhone use, but it needs package identity and
//!   the restricted `phoneLineTransportManagement` capability. tug ships unpackaged (NSIS),
//!   so that's out until tug is packaged (MSIX, or a sparse package with a signing cert).
//! - Raw RFCOMM to the Audio Gateway is plain Bluetooth, which tug already does for MAP and
//!   PBAP. The open question only hardware can answer: whether Windows (its hands-free
//!   profile, or Phone Link) already holds the phone's hands-free link. A phone takes one
//!   hands-free connection per device, so then tug's connect is refused (`Blocked`).
//!
//! So nothing here is on by default: Settings › iPhone › Calls checks the link first, and
//! only a successful check turns Call buttons on. `examples/hfp_probe.rs` reports each step.
//!
//! Layering: `at` (commands, replies, the setup sequence) is pure and unit-tested;
//! `session` is the WinRT RFCOMM I/O.

pub mod at;
#[cfg(windows)]
pub mod session;
