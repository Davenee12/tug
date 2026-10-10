//! Code fill: a newly arrived verification code goes onto the clipboard (the frontend decides
//! when, `src/lib/codeFill.ts`, and calls `copy_code`), and a global shortcut types the newest
//! code into whatever box has focus.
//!
//! This module is the pure part: what counts as a code, which code to type, when an auto-copied
//! code may be cleared, the shortcut list, and the keystrokes. `win.rs` is the thin I/O (the
//! hotkey thread, SendInput, the foreground check).
//!
//! Security rules, enforced here rather than trusted from the webview: only digit codes that
//! `find_code` produced are copied or typed, never anything older than [`MAX_AGE_MS`], and logs
//! say what happened without the code.

#[cfg(windows)]
mod win;

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;

/// A code older than this is never copied or typed (mirrors `CODE_FILL_MAX_AGE_MS` in codeFill.ts).
pub const MAX_AGE_MS: i64 = 10 * 60 * 1000;
/// An auto-copied code leaves the clipboard after this long, if it's still there.
pub const CLEAR_AFTER: Duration = Duration::from_secs(120);
/// The shortcuts tug offers, in Settings' order; the first is the default. Mirrors
/// `src/lib/codeHotkeys.json` (a test reads it).
pub const HOTKEYS: &[&str] = &["ctrl+shift+v", "ctrl+alt+v", "ctrl+shift+alt+v", "ctrl+alt+c"];
pub const DEFAULT_HOTKEY: &str = HOTKEYS[0];

/// Settings keys the backend reads (written by the frontend's `setSetting`).
pub const SETTING_ENABLED: &str = "ui.typeCodeHotkey";
pub const SETTING_KEYS: &str = "ui.typeCodeKeys";

/// What `find_code` hands out: 4 to 8 ASCII digits. Anything else (free text from the webview)
/// is refused by `copy_code` and never typed.
pub fn is_code(s: &str) -> bool {
    (4..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// Clear the clipboard only if it still holds the code tug put there: if the person copied
/// something since, it's theirs and stays.
pub fn should_clear(clipboard: Option<&str>, code: &str) -> bool {
    clipboard.map(str::trim) == Some(code)
}

/// A shortcut from [`HOTKEYS`], as RegisterHotKey wants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// An uppercase ASCII letter, which is also its virtual-key code.
    pub key: u8,
}

const MOD_ALT: u32 = 0x1;
const MOD_CONTROL: u32 = 0x2;
const MOD_SHIFT: u32 = 0x4;
/// Holding the keys down doesn't repeat the press (and so doesn't type the code twice).
const MOD_NOREPEAT: u32 = 0x4000;

impl Keys {
    pub fn modifiers(&self) -> u32 {
        let mut m = MOD_NOREPEAT;
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.alt {
            m |= MOD_ALT;
        }
        m
    }

    pub fn vk(&self) -> u32 {
        u32::from(self.key)
    }
}

/// One of the offered shortcuts, or `None`: tug never registers a shortcut it doesn't list.
pub fn parse_hotkey(id: &str) -> Option<Keys> {
    if !HOTKEYS.contains(&id) {
        return None;
    }
    let mut keys = Keys {
        ctrl: false,
        shift: false,
        alt: false,
        key: 0,
    };
    for part in id.split('+') {
        match part {
            "ctrl" => keys.ctrl = true,
            "shift" => keys.shift = true,
            "alt" => keys.alt = true,
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_lowercase() => {
                keys.key = k.as_bytes()[0].to_ascii_uppercase()
            }
            _ => return None,
        }
    }
    (keys.key != 0 && keys.ctrl).then_some(keys)
}

/// The saved shortcut, or the default if the saved one isn't offered.
pub fn known_hotkey(id: Option<&str>) -> &'static str {
    id.and_then(|id| HOTKEYS.iter().copied().find(|h| *h == id))
        .unwrap_or(DEFAULT_HOTKEY)
}

/// The newest item carrying a code, received at or after `since`: `(received at, code, payload)`.
/// Newest wins across notifications and texts; an item without a code doesn't count.
pub fn newest<'a, T>(items: impl IntoIterator<Item = (i64, &'a str, T)>, since: i64) -> Option<(i64, String, T)> {
    let mut best: Option<(i64, String, T)> = None;
    for (at, text, payload) in items {
        if at < since || best.as_ref().is_some_and(|b| b.0 >= at) {
            continue;
        }
        if let Some(found) = crate::codes::find_code(text) {
            best = Some((at, found.code, payload));
        }
    }
    best
}

/// The keystrokes that type `text` as Unicode characters, not keys: `(UTF-16 unit, key up)`.
/// Typed this way the keyboard layout and the clipboard don't matter.
pub fn keystrokes(text: &str) -> Vec<(u16, bool)> {
    text.encode_utf16().flat_map(|u| [(u, false), (u, true)]).collect()
}

/// What a press of the shortcut did, told to the frontend (`code-filled`). Never the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum FillOutcome {
    Typed,
    /// No code in the last 10 minutes.
    NoCode,
    /// tug's own window (or nothing) is in front: there's nowhere to type it.
    NoTarget,
    /// Windows didn't take all the keystrokes.
    Failed,
}

/// The `code-filled` event. Mirrored in `src/types/protocol.ts` (`CodeFilled`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct CodeFilled {
    pub outcome: FillOutcome,
    /// Who the typed code came from.
    pub from: Option<String>,
}

/// What to do for one press.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Plan {
    Type { code: String, from: String },
    Report(FillOutcome),
}

/// Decide a press: never type into tug's own window (or into nothing), and only a fresh code.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn decide(fresh: Option<(String, String)>, target_ok: bool) -> Plan {
    if !target_ok {
        return Plan::Report(FillOutcome::NoTarget);
    }
    match fresh {
        Some((code, from)) if is_code(&code) => Plan::Type { code, from },
        _ => Plan::Report(FillOutcome::NoCode),
    }
}

/// The shortcut's state for Settings. Mirrored in `src/types/protocol.ts` (`CodeHotkeyStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub state: HotkeyState,
    pub keys: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyState {
    Off,
    On,
    /// Another app registered it first.
    InUse,
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
        HotkeyState::On => log::info!("type-the-code shortcut registered ({id})"),
        HotkeyState::InUse => log::info!("type-the-code shortcut {id} is taken by another app"),
        HotkeyState::Off => log::info!("type-the-code shortcut off"),
    }
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    s
}

#[cfg(windows)]
fn set_registration(keys: Option<Keys>) -> bool {
    win::register(keys)
}

#[cfg(not(windows))]
fn set_registration(_keys: Option<Keys>) -> bool {
    false
}

/// Start the hotkey thread and register the saved shortcut (on unless switched off).
pub fn start(app: &AppHandle, store: &crate::store::Store) {
    #[cfg(windows)]
    win::start(app.clone());
    #[cfg(not(windows))]
    let _ = app;
    let enabled = store.setting(SETTING_ENABLED).ok().flatten().as_deref() != Some("false");
    let keys = store.setting(SETTING_KEYS).ok().flatten();
    apply(enabled, known_hotkey(keys.as_deref()));
}

/// After [`CLEAR_AFTER`], take `code` off the clipboard if it's still what's there.
pub fn clear_later(code: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLEAR_AFTER).await;
        let cleared = tauri::async_runtime::spawn_blocking(move || crate::clipboard::clear_if_holds(&code)).await;
        match cleared {
            Ok(Ok(true)) => log::info!("auto-copied code cleared from the clipboard"),
            Ok(Ok(false)) => log::info!("clipboard changed since the code was copied; left alone"),
            Ok(Err(e)) => log::warn!("couldn't clear the auto-copied code: {e}"),
            Err(e) => log::warn!("couldn't clear the auto-copied code: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_find_code_shapes_are_codes() {
        for ok in ["4829", "482193", "48291337"] {
            assert!(is_code(ok), "{ok}");
        }
        for bad in [
            "",
            "482",
            "482913370",
            "48-2913",
            "G-482913",
            "abc123",
            "４８２９",
            "1234\n",
        ] {
            assert!(!is_code(bad), "{bad:?}");
        }
    }

    #[test]
    fn clears_only_its_own_code() {
        assert!(should_clear(Some("482193"), "482193"));
        assert!(should_clear(Some(" 482193\r\n"), "482193"));
        assert!(!should_clear(Some("something I copied"), "482193"));
        assert!(!should_clear(Some("771204"), "482193"), "a newer code stays");
        assert!(!should_clear(None, "482193"), "an empty or picture clipboard stays");
    }

    #[test]
    fn agrees_with_the_shared_shortcut_list() {
        let raw = include_str!("../../../src/lib/codeHotkeys.json");
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
    fn parses_shortcuts_for_register_hot_key() {
        let k = parse_hotkey("ctrl+shift+v").unwrap();
        assert_eq!(
            k,
            Keys {
                ctrl: true,
                shift: true,
                alt: false,
                key: b'V'
            }
        );
        assert_eq!(k.modifiers(), MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT);
        assert_eq!(k.vk(), 0x56);
        assert_eq!(
            parse_hotkey("ctrl+alt+c").unwrap().modifiers(),
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT
        );
        // Not offered: never registered, even if it would parse.
        assert_eq!(parse_hotkey("ctrl+v"), None);
        assert_eq!(parse_hotkey("alt+f4"), None);
        assert_eq!(known_hotkey(Some("ctrl+v")), DEFAULT_HOTKEY);
        assert_eq!(known_hotkey(None), DEFAULT_HOTKEY);
        assert_eq!(known_hotkey(Some("ctrl+alt+v")), "ctrl+alt+v");
    }

    #[test]
    fn newest_code_wins_within_the_window() {
        let items = vec![
            (1_000, "Your code is 111111", "old"),
            (5_000, "omw, 10 mins", "newest but no code"),
            (4_000, "Your code is 222222", "newer"),
            (3_000, "Your code is 333333", "older"),
        ];
        let (at, code, from) = newest(items.clone(), 0).unwrap();
        assert_eq!((at, code.as_str(), from), (4_000, "222222", "newer"));
        // Past the window: nothing, rather than an old code.
        assert_eq!(newest(items.clone(), 4_001), None);
        assert_eq!(newest(Vec::<(i64, &str, ())>::new(), 0), None);
    }

    #[test]
    fn keystrokes_press_and_release_each_character() {
        assert_eq!(
            keystrokes("48"),
            vec![(0x34, false), (0x34, true), (0x38, false), (0x38, true)]
        );
        assert!(keystrokes("").is_empty());
    }

    #[test]
    fn never_types_into_tug_or_without_a_fresh_code() {
        let fresh = || Some(("482193".to_string(), "Chase".to_string()));
        assert_eq!(
            decide(fresh(), true),
            Plan::Type {
                code: "482193".into(),
                from: "Chase".into()
            }
        );
        assert_eq!(decide(fresh(), false), Plan::Report(FillOutcome::NoTarget));
        assert_eq!(decide(None, true), Plan::Report(FillOutcome::NoCode));
        assert_eq!(decide(None, false), Plan::Report(FillOutcome::NoTarget));
        assert_eq!(
            decide(Some(("not a code".into(), "x".into())), true),
            Plan::Report(FillOutcome::NoCode)
        );
    }

    #[test]
    fn events_reach_the_frontend_in_camel_case() {
        let json = serde_json::to_string(&CodeFilled {
            outcome: FillOutcome::NoTarget,
            from: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"outcome":"noTarget","from":null}"#);
        let json = serde_json::to_string(&HotkeyStatus {
            state: HotkeyState::InUse,
            keys: "ctrl+shift+v".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"inUse","keys":"ctrl+shift+v"}"#);
    }
}
