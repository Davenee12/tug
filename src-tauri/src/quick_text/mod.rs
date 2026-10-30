//! Quick text: a global shortcut that brings up a small "text someone" composer over tug's window
//! from any app. The person picks someone, types, and Enter sends through the iPhone exactly as
//! the Messages view does (the frontend's `sendMessage`); then tug gets out of the way and the app
//! they were in has the keyboard again. Esc does the same without sending.
//!
//! Off until switched on in Settings › Messages: a global shortcut takes its keys away from every
//! other app. It shares code fill's hotkey thread (a second id, `code_fill::Shortcut`), its
//! "taken by another app" handling and its status type, and reaches the UI through the quick
//! actions queue (`QuickAction::QuickText`), so a press is carried out once even while the window
//! is still loading.
//!
//! This module is the pure part: the shortcut list, and where the keyboard goes back to when the
//! composer closes. `win.rs` is the thin I/O (what's in front at the press, giving it back).
//! Nothing here sends anything: only the person's Enter or Send in the composer does.

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub(crate) use win::pressed;

use std::sync::Mutex;

use tauri::AppHandle;

use crate::code_fill::{HotkeyState, HotkeyStatus, Keys};

/// The shortcuts tug offers, in Settings' order; the first is the default. Mirrors
/// `src/lib/quickTextHotkeys.json` (a test reads it). No Ctrl+Alt (AltGr on many keyboard
/// layouts, so it would type a character instead) and nothing as well known as Ctrl+Shift+T.
pub const HOTKEYS: &[&str] = &["ctrl+shift+y", "ctrl+shift+u", "ctrl+shift+h"];
pub const DEFAULT_HOTKEY: &str = HOTKEYS[0];

/// Settings keys the backend reads (written by the frontend's `setSetting`).
pub const SETTING_ENABLED: &str = "ui.quickTextHotkey";
pub const SETTING_KEYS: &str = "ui.quickTextKeys";

/// Off unless switched on: only an explicit "true" registers the shortcut.
pub fn enabled(stored: Option<&str>) -> bool {
    stored == Some("true")
}

/// One of the offered shortcuts, or `None`: tug never registers a shortcut it doesn't list.
pub fn parse_hotkey(id: &str) -> Option<Keys> {
    if !HOTKEYS.contains(&id) {
        return None;
    }
    crate::code_fill::parse_keys(id)
}

/// The saved shortcut, or the default if the saved one isn't offered.
pub fn known_hotkey(id: Option<&str>) -> &'static str {
    id.and_then(|id| HOTKEYS.iter().copied().find(|h| *h == id))
        .unwrap_or(DEFAULT_HOTKEY)
}

static STATUS: Mutex<Option<HotkeyStatus>> = Mutex::new(None);

/// The shortcut's current state (off until `start` or `apply` ran).
pub fn status() -> HotkeyStatus {
    STATUS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or(HotkeyStatus {
            state: HotkeyState::Off,
            keys: DEFAULT_HOTKEY.into(),
        })
}

/// Turn the shortcut on (as `keys`) or off, and say how that went.
pub fn apply(enabled: bool, keys: &str) -> HotkeyStatus {
    let id = known_hotkey(Some(keys));
    let state = if !enabled {
        set_registration(None);
        HotkeyState::Off
    } else if set_registration(parse_hotkey(id)) {
        HotkeyState::On
    } else {
        HotkeyState::InUse
    };
    let s = HotkeyStatus { state, keys: id.into() };
    match state {
        HotkeyState::On => log::info!("quick text shortcut registered ({id})"),
        HotkeyState::InUse => log::info!("quick text shortcut {id} is taken by another app"),
        HotkeyState::Off => log::info!("quick text shortcut off"),
    }
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    s
}

#[cfg(windows)]
fn set_registration(keys: Option<Keys>) -> bool {
    crate::code_fill::register_on_hotkey_thread(crate::code_fill::Shortcut::QuickText, keys)
}

#[cfg(not(windows))]
fn set_registration(_keys: Option<Keys>) -> bool {
    false
}

/// Register the saved shortcut if it was switched on. After `code_fill::start`, which starts the
/// hotkey thread this registers on.
pub fn start(store: &crate::store::Store) {
    let on = enabled(store.setting(SETTING_ENABLED).ok().flatten().as_deref());
    if !on {
        // Nothing to register; Settings reads "off" from `status`.
        return;
    }
    let keys = store.setting(SETTING_KEYS).ok().flatten();
    apply(true, known_hotkey(keys.as_deref()));
}

/// How tug's window was when the shortcut was pressed, so closing the composer can put it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum TugWindow {
    /// On screen, behind the app in front.
    Behind,
    Minimized,
    /// Closed to the tray.
    Hidden,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn tug_window(visible: bool, minimized: bool) -> TugWindow {
    if !visible {
        TugWindow::Hidden
    } else if minimized {
        TugWindow::Minimized
    } else {
        TugWindow::Behind
    }
}

/// What had the keyboard when the shortcut was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Front {
    /// tug's own window: the person was already in tug, so tug stays when the composer closes.
    Tug,
    /// Another app's window (its handle).
    Other(isize),
    /// Nothing (Windows was switching windows).
    Nothing,
}

/// Where the keyboard goes back to when the composer closes: the app the person was in (if
/// any), with tug's window put back as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct GiveBack {
    pub window: Option<isize>,
    pub tug: TugWindow,
}

/// What to remember for a press. Pressed from another app: go back to it. Pressed in tug: the
/// composer is opened (or is still open) over tug, so keep what an earlier press from another app
/// remembered, if anything; with nothing remembered, tug stays when it closes.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn remember(front: Front, tug: TugWindow, waiting: Option<GiveBack>) -> Option<GiveBack> {
    match front {
        Front::Tug => waiting,
        Front::Other(window) => Some(GiveBack {
            window: Some(window),
            tug,
        }),
        Front::Nothing => Some(GiveBack { window: None, tug }),
    }
}

/// Set by a press, taken when the composer closes.
static WAITING: Mutex<Option<GiveBack>> = Mutex::new(None);

#[cfg_attr(not(windows), allow(dead_code))]
fn note_press(front: Front, tug: TugWindow) {
    let mut waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    *waiting = remember(front, tug, *waiting);
}

/// The composer closed. With `give_back` (sent, or Esc), the app the person was in gets the
/// keyboard back and tug's window goes back to how it was; without (the text couldn't be sent and
/// tug is showing why), tug stays. Either way the press is done with.
pub fn done(app: &AppHandle, give_back: bool) {
    let waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(back) = waiting.filter(|_| give_back) else {
        return;
    };
    #[cfg(windows)]
    win::give_back(app, back);
    #[cfg(not(windows))]
    let _ = (app, back);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_the_shared_shortcut_list() {
        let raw = include_str!("../../../src/lib/quickTextHotkeys.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let listed: Vec<&str> = v["hotkeys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap())
            .collect();
        assert_eq!(listed, HOTKEYS);
        for id in HOTKEYS {
            assert!(parse_hotkey(id).is_some(), "{id} parses");
        }
    }

    #[test]
    fn never_offers_altgr_a_well_known_combo_or_another_tug_shortcut() {
        for id in HOTKEYS {
            let k = parse_hotkey(id).unwrap();
            assert!(k.ctrl && !k.alt, "{id}: Ctrl+Alt is AltGr on many layouts");
            assert_ne!(*id, "ctrl+shift+t", "browsers reopen a closed tab");
            assert!(!crate::code_fill::HOTKEYS.contains(id), "{id} is code fill's");
            assert!(!crate::voice::HOTKEYS.contains(id), "{id} is voice typing's");
        }
    }

    #[test]
    fn parses_only_offered_shortcuts() {
        let k = parse_hotkey("ctrl+shift+y").unwrap();
        assert_eq!(
            k,
            Keys {
                ctrl: true,
                shift: true,
                alt: false,
                key: b'Y'
            }
        );
        // Code fill's: never registered as quick text.
        assert_eq!(parse_hotkey("ctrl+shift+v"), None);
        assert_eq!(parse_hotkey("ctrl+shift+t"), None);
        assert_eq!(known_hotkey(Some("ctrl+shift+v")), DEFAULT_HOTKEY);
        assert_eq!(known_hotkey(None), DEFAULT_HOTKEY);
        assert_eq!(known_hotkey(Some("ctrl+shift+u")), "ctrl+shift+u");
    }

    #[test]
    fn off_until_switched_on() {
        assert!(!enabled(None));
        assert!(!enabled(Some("false")));
        assert!(!enabled(Some("")));
        assert!(enabled(Some("true")));
    }

    #[test]
    fn reads_tugs_window_as_it_was() {
        assert_eq!(tug_window(false, false), TugWindow::Hidden);
        assert_eq!(tug_window(false, true), TugWindow::Hidden);
        assert_eq!(tug_window(true, true), TugWindow::Minimized);
        assert_eq!(tug_window(true, false), TugWindow::Behind);
    }

    #[test]
    fn goes_back_only_to_an_app_the_person_was_in() {
        let from_word = remember(Front::Other(42), TugWindow::Hidden, None);
        assert_eq!(
            from_word,
            Some(GiveBack {
                window: Some(42),
                tug: TugWindow::Hidden
            })
        );
        // Pressed in tug: tug stays when the composer closes.
        assert_eq!(remember(Front::Tug, TugWindow::Behind, None), None);
        // Pressed in tug again while the composer from Word is still open: still back to Word.
        assert_eq!(remember(Front::Tug, TugWindow::Behind, from_word), from_word);
        // A newer press from another app replaces the older one.
        assert_eq!(
            remember(Front::Other(7), TugWindow::Minimized, from_word),
            Some(GiveBack {
                window: Some(7),
                tug: TugWindow::Minimized
            })
        );
        // Nothing in front: no app to go back to, but tug still goes back to the tray.
        assert_eq!(
            remember(Front::Nothing, TugWindow::Hidden, None),
            Some(GiveBack {
                window: None,
                tug: TugWindow::Hidden
            })
        );
    }

    #[test]
    fn status_reaches_settings_in_camel_case() {
        let json = serde_json::to_string(&HotkeyStatus {
            state: HotkeyState::InUse,
            keys: "ctrl+shift+y".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"inUse","keys":"ctrl+shift+y"}"#);
    }
}
