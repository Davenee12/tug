//! Hide previews while you share your screen or present: while a meeting can see this PC's
//! screen, tug's pop-ups say only which app something is from ("Messages · New message"), and the
//! window blurs texts and names until the person chooses Show anyway.
//!
//! This module is the pure part: which of the things Windows can tell us mean "someone may be
//! watching this screen" ([`evaluate`]), what a pop-up says instead ([`redact`]), and the
//! state the window reads ([`ShareState`]). `win.rs` is the thin I/O: it asks Windows, cheaply,
//! and only while tug could show something (a pop-up is about to appear, or tug's window is on
//! screen), and keeps checking while hiding is on so it clears as soon as sharing stops.
//!
//! **The signals, and where each comes from.** Every one is something Windows or the app
//! itself makes visible to any program running as the same user; none needs admin rights.
//!
//! - `SHQueryUserNotificationState` (shell32), the call Windows provides so apps know when not
//!   to interrupt: `QUNS_PRESENTATION_MODE` (Windows' presentation settings are on),
//!   `QUNS_BUSY` (a full-screen app, or presentation settings), `QUNS_RUNNING_D3D_FULL_SCREEN`
//!   (an exclusive full-screen game or video) and `QUNS_APP` (a full-screen Store app).
//!   <https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shqueryusernotificationstate>
//!   Full-screen states only count when the foreground window really covers its monitor and
//!   isn't the desktop, so a stray answer can't leave previews hidden.
//! - The display is duplicated (Windows+P "Duplicate", the usual projector setup):
//!   `QueryDisplayConfig(QDC_DATABASE_CURRENT)` reports `DISPLAYCONFIG_TOPOLOGY_CLONE`. Windows
//!   11 itself turns on Do not disturb "when I'm duplicating my display".
//!   <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-querydisplayconfig>
//! - Windows that exist only while sharing or presenting, matched by the owning program and
//!   its window class (stable across versions and languages), never by window title alone.
//!   See [`WINDOW_RULES`] for each one and its source.
//!
//! **Not used, and why:** the yellow border Windows draws around a captured window or screen
//! (Windows.Graphics.Capture) has no API to ask whether a capture is running; Windows' own Do
//! not disturb / Focus state is only readable through undocumented calls (WNF state names),
//! which tug won't depend on (Windows.UI.Shell.FocusSessionManager reports the Clock app's
//! focus sessions, not Do not disturb); Discord and Slack show nothing on the PC while you
//! stream, so there is nothing local to see; Webex's share bar and Zoom's and Teams' coloured
//! borders have no documented window to match (guessing would be worse than missing them);
//! Firefox's sharing indicator also shows for a camera or microphone alone, so it can't say
//! "sharing". Whatever app shares, a full-screen app or a duplicated display still counts.
//!
//! **Privacy:** window titles are never read except for the few windows a rule names by
//! program and class, never kept and never logged; the log names the signal only.

#[cfg(windows)]
mod win;

use serde::Serialize;

use crate::toast::xml::{ToastKind, ToastSpec};

/// The Settings switch (Settings › Notifications), written by the frontend's `setSetting`. On
/// unless switched off.
pub const SETTING: &str = "ui.hidePreviewsWhileSharing";
/// The event the window hears when hiding starts or stops (payload: [`ShareState`]).
pub const EVENT: &str = "screen-share";

/// Whether the setting's stored value means on (anything but "false": on by default).
pub fn enabled_from_setting(value: Option<&str>) -> bool {
    value != Some("false")
}

/// Why previews are hidden, for the window's banner. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// A meeting app or browser is sharing the screen.
    Sharing,
    /// A slideshow, Windows' presentation settings, or a duplicated display.
    Presenting,
    /// An app or game is full screen.
    FullScreen,
}

/// One thing that says someone may be watching this screen. Only its name is ever logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    /// A meeting app's or browser's share window ([`WINDOW_RULES`]).
    ShareWindow(&'static str),
    /// A slideshow window ([`WINDOW_RULES`]).
    SlideshowWindow(&'static str),
    /// `QUNS_PRESENTATION_MODE`: Windows' presentation settings are on.
    PresentationMode,
    /// Windows+P "Duplicate": the same picture on a second screen or projector.
    DuplicateDisplay,
    /// `QUNS_BUSY` / `QUNS_APP`, confirmed by the foreground window covering its monitor.
    FullScreen,
    /// `QUNS_RUNNING_D3D_FULL_SCREEN`, confirmed the same way.
    FullScreenExclusive,
}

impl Signal {
    /// A short, fixed name for the log: never a window title or anything the person wrote.
    pub fn name(self) -> &'static str {
        match self {
            Signal::ShareWindow(name) | Signal::SlideshowWindow(name) => name,
            Signal::PresentationMode => "presentation-mode",
            Signal::DuplicateDisplay => "duplicate-display",
            Signal::FullScreen => "full-screen",
            Signal::FullScreenExclusive => "full-screen-exclusive",
        }
    }

    pub fn reason(self) -> Reason {
        match self {
            Signal::ShareWindow(_) => Reason::Sharing,
            Signal::SlideshowWindow(_) | Signal::PresentationMode | Signal::DuplicateDisplay => Reason::Presenting,
            Signal::FullScreen | Signal::FullScreenExclusive => Reason::FullScreen,
        }
    }

    /// When several signals agree, the most specific one names the reason: sharing over
    /// presenting over full screen (a shared slideshow is "sharing").
    fn rank(self) -> u8 {
        match self.reason() {
            Reason::Sharing => 0,
            Reason::Presenting => 1,
            Reason::FullScreen => 2,
        }
    }
}

/// What the window reads: whether previews are hidden right now, and why. Mirrored in
/// `src/types/protocol.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShareState {
    pub active: bool,
    pub reason: Option<Reason>,
}

impl ShareState {
    pub fn from_signal(signal: Option<Signal>) -> Self {
        ShareState {
            active: signal.is_some(),
            reason: signal.map(Signal::reason),
        }
    }
}

// `SHQueryUserNotificationState` answers (shellapi.h, QUERY_USER_NOTIFICATION_STATE).
pub const QUNS_NOT_PRESENT: i32 = 1;
pub const QUNS_BUSY: i32 = 2;
pub const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
pub const QUNS_PRESENTATION_MODE: i32 = 4;
pub const QUNS_ACCEPTS_NOTIFICATIONS: i32 = 5;
pub const QUNS_QUIET_TIME: i32 = 6;
pub const QUNS_APP: i32 = 7;

/// What the shell's notification state means here. `full_screen_confirmed` is whether the
/// foreground window covers its whole monitor and isn't the desktop or taskbar: the full-screen
/// answers only count with it, so a stray "busy" can't leave previews hidden. Presentation mode
/// is a setting the person turned on, so it counts as it is. "Not present" (locked, screen saver)
/// and "quiet time" (the first hour after signing in) aren't anyone watching.
pub fn quns_signal(state: i32, full_screen_confirmed: bool) -> Option<Signal> {
    match state {
        QUNS_PRESENTATION_MODE => Some(Signal::PresentationMode),
        QUNS_BUSY | QUNS_APP if full_screen_confirmed => Some(Signal::FullScreen),
        QUNS_RUNNING_D3D_FULL_SCREEN if full_screen_confirmed => Some(Signal::FullScreenExclusive),
        QUNS_NOT_PRESENT | QUNS_QUIET_TIME | QUNS_ACCEPTS_NOTIFICATIONS => None,
        _ => None,
    }
}

/// A screen rectangle (left, top, right, bottom), as Windows reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Window classes that cover the monitor without being a full-screen app: the desktop and the
/// taskbars. Explorer can be the foreground window when nothing is.
const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// Whether the foreground window is really full screen: it covers its whole monitor (not just
/// the work area a maximized window gets) and isn't the desktop or a taskbar.
pub fn covers_monitor(window: Rect, monitor: Rect, class: &str) -> bool {
    if SHELL_CLASSES.contains(&class) || monitor.right <= monitor.left || monitor.bottom <= monitor.top {
        return false;
    }
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

/// How a rule matches a window's title, for the few windows whose class alone is shared with
/// ordinary windows of the same program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleMatch {
    /// The class and program are enough; the title is never read.
    Any,
    /// The title starts with this (compared case-insensitively).
    StartsWith(&'static str),
    /// The title ends with one of these (compared case-insensitively).
    EndsWithAny(&'static [&'static str]),
}

impl TitleMatch {
    fn matches(self, title: Option<&str>) -> bool {
        let lower = || title.map(|t| t.trim().to_lowercase()).unwrap_or_default();
        match self {
            TitleMatch::Any => true,
            TitleMatch::StartsWith(prefix) => title.is_some() && lower().starts_with(&prefix.to_lowercase()),
            TitleMatch::EndsWithAny(endings) => {
                let t = lower();
                title.is_some() && endings.iter().any(|e| t.ends_with(&e.to_lowercase()))
            }
        }
    }
}

/// A window that exists only while sharing or presenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRule {
    /// The signal's name in the log.
    pub name: &'static str,
    pub reason: Reason,
    /// The owning program's file name, lowercase (any of these).
    pub exe: &'static [&'static str],
    /// The window class, exactly.
    pub class: &'static str,
    pub title: TitleMatch,
}

/// The windows tug recognises. Each is checked against the owning program as well as the class,
/// so another program's window with the same class name can't trip it.
pub const WINDOW_RULES: &[WindowRule] = &[
    // Zoom's floating meeting controls, shown while you share (any visible window of this class
    // is how other tools tell Zoom is sharing).
    // <https://github.com/sircharlo/meeting-media-manager/blob/fdbfe2069c698c4efef0fb56bb12ecd094653a21/src-electron/zoom-helper/ZoomHelper.cs>
    // <https://github.com/Aternity/Application-Signatures/blob/3bfc93faedc15bb3624dc4516bc802826c749447/Zoom/Stop%20Sharing.xml>
    WindowRule {
        name: "zoom-share",
        reason: Reason::Sharing,
        exe: &["zoom.exe"],
        class: "ZPFloatToolbarClass",
        title: TitleMatch::Any,
    },
    // New Teams' "Sharing control bar | Microsoft Teams" while you present. Its class is shared
    // with Teams' other windows, so the title decides; Teams shows it in English in the cases
    // documented so far.
    // <https://github.com/mroter93/TeamsHider/blob/a2a4cacfa894904ecf2b0dcb6aec706e5643a481/TeamsHider/Program.cs>
    WindowRule {
        name: "teams-share",
        reason: Reason::Sharing,
        exe: &["ms-teams.exe"],
        class: "TeamsWebView",
        title: TitleMatch::StartsWith("Sharing control bar"),
    },
    // Classic Teams (Electron): the same bar.
    // <https://github.com/Krishnakrish77/TeamsOverlayHider/blob/main/AutoHotKey/TeamsOverlayHider.ahk>
    WindowRule {
        name: "teams-share",
        reason: Reason::Sharing,
        exe: &["teams.exe"],
        class: "Chrome_WidgetWin_1",
        title: TitleMatch::StartsWith("Sharing control bar"),
    },
    // Chromium browsers' "meet.google.com is sharing your screen." bar (Stop sharing / Hide),
    // for Google Meet and any other site. Ordinary browser windows end in the browser's name, so
    // the endings can't match a page title. English wording; a browser in another language says
    // it differently (full-screen and duplicate still apply). Tab sharing is left out: it shows
    // one tab, never tug or its pop-ups.
    // <https://chromium.googlesource.com/chromium/src/+/main/chrome/app/generated_resources.grd>
    // <https://chromium.googlesource.com/chromium/src/+/main/chrome/browser/ui/views/screen_capture_notification_ui_views.cc>
    WindowRule {
        name: "browser-share",
        reason: Reason::Sharing,
        exe: &["chrome.exe", "msedge.exe", "brave.exe", "vivaldi.exe", "opera.exe"],
        class: "Chrome_WidgetWin_1",
        title: TitleMatch::EndsWithAny(&[
            "is sharing your screen.",
            "is sharing your screen and audio.",
            "is sharing a window.",
            "is sharing a window and audio.",
        ]),
    },
    // PowerPoint's slide show window (the audience's view) and presenter view.
    // <https://github.com/obsproject/obs-studio/blob/f1c0a116441bdb7a1de7896f810009ea487b8508/plugins/win-capture/window-capture.c>
    WindowRule {
        name: "powerpoint-slideshow",
        reason: Reason::Presenting,
        exe: &["powerpnt.exe"],
        class: "screenClass",
        title: TitleMatch::Any,
    },
    WindowRule {
        name: "powerpoint-slideshow",
        reason: Reason::Presenting,
        exe: &["powerpnt.exe"],
        class: "PodiumParent",
        title: TitleMatch::Any,
    },
];

/// A visible top-level window, as much of it as the rules need. `win.rs` fills `exe` only for
/// classes some rule names, and `title` only when that rule matches by title.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TopWindow {
    pub class: String,
    /// The owning program's file name, lowercase; empty when not needed or not readable.
    pub exe: String,
    /// Only for windows a title rule needs; never stored or logged.
    pub title: Option<String>,
}

/// Whether any rule cares about windows of this class (so the owner is worth looking up).
pub fn class_of_interest(class: &str) -> bool {
    WINDOW_RULES.iter().any(|r| r.class == class)
}

/// Whether a rule for this class and program matches by title (so the title must be read).
pub fn needs_title(class: &str, exe: &str) -> bool {
    WINDOW_RULES
        .iter()
        .any(|r| r.class == class && r.exe.contains(&exe) && r.title != TitleMatch::Any)
}

/// The rule this window matches, if any.
pub fn window_signal(w: &TopWindow) -> Option<Signal> {
    WINDOW_RULES
        .iter()
        .find(|r| r.class == w.class && r.exe.contains(&w.exe.as_str()) && r.title.matches(w.title.as_deref()))
        .map(|r| match r.reason {
            Reason::Sharing => Signal::ShareWindow(r.name),
            _ => Signal::SlideshowWindow(r.name),
        })
}

/// One look at the screen: everything [`evaluate`] combines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// `SHQueryUserNotificationState`'s answer; `None` if the call failed.
    pub quns: Option<i32>,
    /// The foreground window covers its monitor and isn't the shell ([`covers_monitor`]).
    pub foreground_full_screen: bool,
    /// The display is duplicated (Windows+P "Duplicate").
    pub duplicating: bool,
    /// Visible top-level windows of a class some rule names.
    pub windows: Vec<TopWindow>,
}

/// The strongest signal in this snapshot, or `None` when nothing says anyone is watching.
pub fn evaluate(s: &Snapshot) -> Option<Signal> {
    let mut signals: Vec<Signal> = s.windows.iter().filter_map(window_signal).collect();
    if let Some(sig) = s.quns.and_then(|q| quns_signal(q, s.foreground_full_screen)) {
        signals.push(sig);
    }
    if s.duplicating {
        signals.push(Signal::DuplicateDisplay);
    }
    signals.into_iter().min_by_key(|sig| sig.rank())
}

/// What hiding started or stopped, for one INFO line in the log (signal names only).
pub fn transition(before: Option<Signal>, after: Option<Signal>) -> Option<String> {
    match (before, after) {
        (None, Some(a)) => Some(format!("hiding previews: {}", a.name())),
        (Some(b), None) => Some(format!("showing previews again ({} ended)", b.name())),
        (Some(b), Some(a)) if b.reason() != a.reason() => {
            Some(format!("still hiding previews: {} (was {})", a.name(), b.name()))
        }
        _ => None,
    }
}

/// A pop-up as it shows while previews are hidden: which app it's from and what kind of thing
/// it is, nothing the sender wrote, not even who sent it. The buttons that reveal nothing stay
/// (Mark read, Copy code, Call back, Clear); the reply box goes, since it names the person and
/// typing a reply in front of a meeting is the thing to avoid. tug's own pop-ups (`Info`) are
/// left alone.
pub fn redact(spec: &ToastSpec) -> ToastSpec {
    let app = spec.app.trim();
    let (title, body) = match spec.kind {
        ToastKind::Info => return spec.clone(),
        ToastKind::Message => (app_or_tug(app), "New message"),
        ToastKind::Notification => (app_or_tug(app), "New notification"),
        ToastKind::Call => ("Incoming call".to_string(), ""),
        ToastKind::MissedCall => ("Missed call".to_string(), ""),
    };
    ToastSpec {
        id: spec.id,
        title,
        body: body.to_string(),
        name: String::new(),
        reply_to: None,
        mark_read: spec.mark_read,
        code: spec.code.clone(),
        call_back: spec.call_back,
        clear: spec.clear,
        app: spec.app.clone(),
        kind: spec.kind,
    }
}

fn app_or_tug(app: &str) -> String {
    if app.is_empty() {
        "tug".to_string()
    } else {
        app.to_string()
    }
}

/// A follow-up note ("Sent to Zoe" and the text) while previews are hidden: the generic title
/// and no body.
pub fn redact_note(generic_title: &str) -> (String, String) {
    (generic_title.to_string(), String::new())
}

#[cfg(windows)]
pub use win::{previews_hidden_now, set_enabled, start, watch};

/// Off Windows nothing is ever hidden (tug only runs on Windows; this keeps other builds honest).
#[cfg(not(windows))]
pub fn previews_hidden_now() -> bool {
    false
}

#[cfg(test)]
mod tests;
