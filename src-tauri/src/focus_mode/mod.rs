//! Focus mode: while the person is focused, tug holds its Windows pop-ups (everything still
//! arrives in the window), and when the focus ends the window shows one summary pop-up of what
//! came meanwhile. The frontend owns the pop-up policy and the summary (`src/lib/popup.ts`,
//! `src/lib/focusMode.ts`); the backend only says when a focus starts and ends.
//!
//! This module is the pure part: the settings ([`FocusSettings`]), what one look found and which
//! reason wins ([`Look`], [`reason`]), whether an app counts as using the microphone or camera
//! ([`apps_in_use`]), and the debouncing that keeps a moment of full screen or a quick switch to
//! another window from starting or ending a focus ([`Debounce`]). `win.rs` is the thin I/O.
//!
//! **The signals**, each its own switch in Settings:
//! - A full-screen app, game or presentation: Windows' own answer (`SHQueryUserNotificationState`),
//!   confirmed the same way hiding previews confirms it (`share_privacy::quns_signal`).
//! - A meeting: another app is using the microphone or camera. Windows records each app's use in
//!   the registry (`CapabilityAccessManager\ConsentStore`); an app using it now has a start time
//!   and no stop time. tug's own entry (voice typing) never counts.
//! - Coding: a code editor or terminal is in front, by the list voice typing already keeps
//!   (`voice::vocab::focus_for_process`), so the two always agree.
//!
//! **Cost.** Nothing at all while Focus mode is off: the watcher sleeps until it's switched on.
//! While on, one look every 2 s, and a look only asks about the switches that are on.
//!
//! **Privacy.** Which app is in front, or which app uses the microphone, never leaves the look:
//! the event and the log carry the category only (and, for a meeting, how many apps).

// Off Windows only the tests use most of this.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod win;

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::voice::vocab::{focus_for_process, Focus};

/// The Settings switches (Settings › Notifications › Focus mode), one JSON value written by the
/// frontend's `setSetting`.
pub const SETTING: &str = "ui.focus";
/// The event the window hears when a focus starts, changes reason or ends (payload: [`FocusState`]).
pub const EVENT: &str = "focus-state";

/// How often to look while Focus mode is on.
pub const POLL: Duration = Duration::from_secs(2);
/// A focus must be seen this long before pop-ups are held: a moment of full screen (a video
/// opened by mistake, a game's loading screen, alt-tab into an editor and out) never starts one,
/// so nothing is held and no summary follows. Three looks at the 2 s poll.
pub const ENTER_AFTER: Duration = Duration::from_secs(5);
/// A focus ends only once nothing has said "focused" for this long: a quick look at another
/// window, or a meeting app closing and opening the microphone again, doesn't end it (and fire a
/// summary) in the middle.
pub const LEAVE_AFTER: Duration = Duration::from_secs(20);
/// An app whose microphone or camera use started longer ago than this, and never stopped, is an
/// entry an app left behind when it closed badly, not a meeting: it can't hold pop-ups forever.
pub const MAX_USE: Duration = Duration::from_secs(12 * 60 * 60);

/// Settings › Notifications › Focus mode. Mirrored in `src/types/protocol.ts`. Off until the
/// person turns it on; then full screen and meetings count, coding only if they choose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FocusSettings {
    pub enabled: bool,
    pub full_screen: bool,
    pub meetings: bool,
    pub coding: bool,
}

impl Default for FocusSettings {
    fn default() -> Self {
        FocusSettings {
            enabled: false,
            full_screen: true,
            meetings: true,
            coding: false,
        }
    }
}

impl FocusSettings {
    /// The stored value; missing or unreadable means the defaults (off).
    pub fn from_setting(value: Option<&str>) -> Self {
        value.and_then(|v| serde_json::from_str(v).ok()).unwrap_or_default()
    }

    /// Whether there is anything to look for: on, with at least one switch on.
    pub fn watching(&self) -> bool {
        self.enabled && (self.full_screen || self.meetings || self.coding)
    }

    /// Whether this reason's switch is on (with Focus mode itself).
    pub fn allows(&self, reason: Reason) -> bool {
        self.enabled
            && match reason {
                Reason::Meeting => self.meetings,
                Reason::FullScreen => self.full_screen,
                Reason::Coding => self.coding,
            }
    }
}

/// Why pop-ups are held. Mirrored in `src/types/protocol.ts`; also what the log says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// The microphone or camera is in use by another app.
    Meeting,
    /// A full-screen app, game or presentation is in front.
    FullScreen,
    /// A code editor or terminal is in front.
    Coding,
}

impl Reason {
    /// A short, fixed name for the log.
    pub fn name(self) -> &'static str {
        match self {
            Reason::Meeting => "meeting",
            Reason::FullScreen => "full-screen",
            Reason::Coding => "coding",
        }
    }
}

/// What the window reads: whether pop-ups are held right now, and why. Mirrored in
/// `src/types/protocol.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FocusState {
    pub active: bool,
    pub reason: Option<Reason>,
}

impl FocusState {
    fn from_reason(reason: Option<Reason>) -> Self {
        FocusState {
            active: reason.is_some(),
            reason,
        }
    }
}

/// One look. A signal whose switch is off is never looked for, so it reads false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    pub full_screen: bool,
    /// How many other apps are using the microphone or camera right now.
    pub meeting_apps: usize,
    pub coding: bool,
}

/// The reason this look says the person is focused, if any, by the switches that are on. A
/// meeting names it over full screen (a presentation in a call is the meeting), and both over
/// coding (a full-screen editor is full screen).
pub fn reason(settings: &FocusSettings, look: &Look) -> Option<Reason> {
    [
        (Reason::Meeting, look.meeting_apps > 0),
        (Reason::FullScreen, look.full_screen),
        (Reason::Coding, look.coding),
    ]
    .into_iter()
    .find(|&(r, found)| found && settings.allows(r))
    .map(|(r, _)| r)
}

/// Whether the program in front (its path or file name) is a code editor or terminal.
pub fn is_coding(exe: &str) -> bool {
    focus_for_process(exe) == Focus::Code
}

/// One app's entry under the ConsentStore's microphone or webcam key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityUse {
    /// The key's name: a Store app's package name, or a desktop program's path with `#` for `\`.
    /// Compared here, never logged.
    pub key: String,
    /// `LastUsedTimeStart` and `LastUsedTimeStop`, as Windows FILETIMEs (0 when absent).
    pub start: u64,
    pub stop: u64,
}

/// The ConsentStore key name Windows gives a desktop program at this path.
pub fn non_packaged_key(exe_path: &str) -> String {
    exe_path.replace('\\', "#")
}

/// FILETIME ticks (100 ns) in a second, and between 1601 (FILETIME's start) and 1970.
const TICKS_PER_SEC: u64 = 10_000_000;
const EPOCH_GAP_SECS: u64 = 11_644_473_600;

/// A unix time in milliseconds as a Windows FILETIME.
pub fn filetime_from_unix_ms(ms: u64) -> u64 {
    (ms / 1000 + EPOCH_GAP_SECS) * TICKS_PER_SEC + (ms % 1000) * (TICKS_PER_SEC / 1000)
}

/// How many apps are using the microphone or camera now (`now` as a FILETIME): started, not
/// stopped, not too long ago ([`MAX_USE`]), and not tug itself (`own`, its key name). An app
/// using both counts once.
pub fn apps_in_use(entries: &[CapabilityUse], own: &str, now: u64) -> usize {
    let oldest = now.saturating_sub(MAX_USE.as_secs() * TICKS_PER_SEC);
    let own = own.to_lowercase();
    let mut seen: Vec<String> = Vec::new();
    for e in entries {
        let key = e.key.to_lowercase();
        if e.start == 0 || e.stop != 0 || e.start < oldest || key == own || seen.contains(&key) {
            continue;
        }
        seen.push(key);
    }
    seen.len()
}

/// Turns looks into focus spells: a focus starts once it has been seen for [`ENTER_AFTER`] and
/// ends once nothing has said "focused" for [`LEAVE_AFTER`]. The reason follows the latest look
/// while it lasts.
#[derive(Debug, Clone, Default)]
pub struct Debounce {
    /// What the window was last told.
    state: FocusState,
    /// Not focused yet: since when every look has said focused.
    seen_since: Option<Instant>,
    /// Focused: since when no look has said so.
    gone_since: Option<Instant>,
}

impl Debounce {
    pub fn state(&self) -> FocusState {
        self.state
    }

    /// A look at `now` found `found`. Returns the new state when the window should hear of it.
    pub fn update(&mut self, now: Instant, found: Option<Reason>) -> Option<FocusState> {
        let next = match (self.state.reason, found) {
            (None, None) => {
                self.seen_since = None;
                return None;
            }
            (None, Some(r)) => {
                let since = *self.seen_since.get_or_insert(now);
                if now.duration_since(since) < ENTER_AFTER {
                    return None;
                }
                Some(r)
            }
            (Some(_), Some(r)) => {
                self.gone_since = None;
                Some(r)
            }
            (Some(_), None) => {
                let since = *self.gone_since.get_or_insert(now);
                if now.duration_since(since) < LEAVE_AFTER {
                    return None;
                }
                None
            }
        };
        self.set(next)
    }

    /// Focus mode went off, or stopped looking for anything: any focus ends now.
    pub fn reset(&mut self) -> Option<FocusState> {
        self.set(None)
    }

    fn set(&mut self, reason: Option<Reason>) -> Option<FocusState> {
        if reason.is_none() {
            self.seen_since = None;
            self.gone_since = None;
        }
        let next = FocusState::from_reason(reason);
        if next == self.state {
            return None;
        }
        self.state = next;
        Some(next)
    }
}

/// One INFO line for the log when the state changes: the category only, and for a meeting how
/// many apps (never which).
pub fn transition(before: FocusState, after: FocusState, meeting_apps: usize) -> Option<String> {
    let what = |r: Reason| match r {
        Reason::Meeting => format!("meeting, {meeting_apps} app(s) using the microphone or camera"),
        _ => r.name().to_string(),
    };
    match (before.reason, after.reason) {
        (None, Some(r)) => Some(format!("holding pop-ups ({})", what(r))),
        (Some(b), None) => Some(format!("showing pop-ups again ({} ended)", b.name())),
        (Some(b), Some(a)) if a != b => Some(format!("still holding pop-ups ({}, was {})", what(a), b.name())),
        _ => None,
    }
}

#[cfg(windows)]
pub use win::{set_settings, start, state};

/// Off Windows nothing is ever focused (tug only runs on Windows; this keeps other builds honest).
#[cfg(not(windows))]
pub fn state() -> FocusState {
    FocusState::default()
}

#[cfg(test)]
mod tests;
