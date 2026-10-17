//! Voice typing ("voice to code"): hold a shortcut, talk, let go, and tug types what you said
//! into whatever has focus, using speech recognition that runs on this PC.
//!
//! Off by default. While it's on, tug registers one global shortcut (the same RegisterHotKey
//! approach as code fill's type-the-code shortcut). Pressing it opens the microphone; letting go
//! closes it, the recording is recognized on the PC, shaped for where it's going (`text`), and
//! typed with SendInput Unicode keystrokes. One hold, one insertion: a second press while one is
//! being typed is ignored, and every path ends with the microphone closed and the indicator gone.
//!
//! Recognition is behind [`engine::Engine`]. Today's engine is Windows' own desktop speech
//! recognizer (SAPI 5, the offline "Microsoft Speech Recognizer" that ships with Windows): no
//! download, no network, nothing extra to build. Audio never leaves the PC and is never written
//! to disk: it lives in memory for one hold and is dropped. Logs carry lengths and durations,
//! never what was said.
//!
//! This file is the pure part (shortcuts, settings, what a hold means, the status the UI reads)
//! plus the bookkeeping; `win/` is the thin Windows I/O.

pub mod engine;
pub mod text;
#[cfg(windows)]
mod win;

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;

use crate::code_fill::Keys;

/// The status event (`VoiceStatus`), sent when it changes.
pub const STATUS_EVENT: &str = "voice-status";
/// What one hold did (`VoiceTyped`), never what was said.
pub const TYPED_EVENT: &str = "voice-typed";

/// Settings keys (written by `set_voice_typing`, read at startup).
pub const SETTING_ENABLED: &str = "ui.voiceTyping";
pub const SETTING_KEYS: &str = "ui.voiceKeys";
pub const SETTING_LANGUAGE: &str = "ui.voiceLanguage";

/// The hold-to-talk shortcuts tug offers, in Settings' order; the first is the default. Mirrors
/// `src/lib/voiceHotkeys.json` (a test reads it). None clash with tug's own shortcuts, code fill's,
/// or common Windows and VS Code ones, and none is an AltGr character on common layouts.
pub const HOTKEYS: &[&str] = &[
    "ctrl+alt+space",
    "ctrl+shift+alt+space",
    "ctrl+alt+f12",
    "ctrl+shift+f11",
];
pub const DEFAULT_HOTKEY: &str = HOTKEYS[0];

/// Shorter than this is a tap, not a hold: nothing is recognized.
pub const MIN_HOLD: Duration = Duration::from_millis(300);
/// A hold stops listening by itself after this long.
pub const MAX_HOLD: Duration = Duration::from_secs(60);
/// Keep listening this long after the keys come up, so the last word isn't clipped.
pub const TAIL: Duration = Duration::from_millis(200);
/// A recording whose loudest sample is below this heard nothing (16-bit PCM; about -40 dBFS).
pub const SILENCE_PEAK: i16 = 330;

const VK_SPACE: u8 = 0x20;
const VK_F1: u8 = 0x70;

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
            "space" => keys.key = VK_SPACE,
            f if f.len() > 1 && f.starts_with('f') => {
                let n: u8 = f[1..].parse().ok().filter(|n| (1..=24).contains(n))?;
                keys.key = VK_F1 + n - 1;
            }
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

/// A speech language installed on this PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceLanguage {
    /// Windows' language id in hex, as the recognizer lists it ("409" is English, United States).
    pub id: String,
    /// "English (US)".
    pub name: String,
}

/// "Microsoft Speech Recognizer 8.0 for Windows (English - US)" → "English (US)".
pub fn language_name(description: &str) -> String {
    let inner = description.rfind('(').and_then(|start| {
        description[start + 1..]
            .find(')')
            .map(|end| &description[start + 1..start + 1 + end])
    });
    match inner {
        Some(inner) => match inner.split_once(" - ") {
            Some((lang, region)) => format!("{} ({})", lang.trim(), region.trim()),
            None => inner.trim().to_string(),
        },
        None => description.trim().to_string(),
    }
}

/// The recognizer's language attribute ("409;9") → its first id, lowercase ("409").
pub fn language_id(attribute: &str) -> Option<String> {
    let first = attribute.split(';').next()?.trim().to_ascii_lowercase();
    (!first.is_empty() && first.chars().all(|c| c.is_ascii_hexdigit())).then_some(first)
}

/// Which language to listen in: the saved one if it's installed, else one matching Windows'
/// display language (same language, any region), else English (US), else the first installed.
pub fn pick_language(saved: Option<&str>, available: &[VoiceLanguage], ui_lang: u16) -> Option<String> {
    let has = |id: &str| available.iter().any(|l| l.id == id);
    if let Some(s) = saved.filter(|s| has(s)) {
        return Some(s.to_string());
    }
    let exact = format!("{ui_lang:x}");
    if has(&exact) {
        return Some(exact);
    }
    let primary = ui_lang & 0x3ff;
    if let Some(l) = available
        .iter()
        .find(|l| u16::from_str_radix(&l.id, 16).is_ok_and(|id| id & 0x3ff == primary))
    {
        return Some(l.id.clone());
    }
    if has("409") {
        return Some("409".into());
    }
    available.first().map(|l| l.id.clone())
}

/// Whether Windows' privacy settings keep desktop apps from the microphone: the device-wide
/// switch (HKLM), "Microphone access" for this account, or "Let desktop apps access your
/// microphone" (each a `Value` of "Allow" or "Deny"; missing means allowed).
pub fn mic_blocked(device: Option<&str>, user: Option<&str>, desktop_apps: Option<&str>) -> bool {
    [device, user, desktop_apps]
        .into_iter()
        .any(|v| v.is_some_and(|v| v.trim().eq_ignore_ascii_case("deny")))
}

/// What one hold did, told to the UI (`voice-typed`). Never the words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceOutcome {
    Typed,
    /// Let go too quickly to be a hold.
    TooShort,
    /// The microphone gave only silence, or nothing was recognized.
    NothingHeard,
    /// Windows' privacy settings block the microphone.
    MicBlocked,
    /// No microphone (or it couldn't be opened).
    NoMic,
    /// There's a microphone but it couldn't be opened right now: another app has it to itself,
    /// or Windows switched it (a Bluetooth hands-free call starting or ending) as it was opened.
    MicBusy,
    /// The window changed before typing, so nothing was typed.
    NoTarget,
    /// Ctrl, Alt, Shift or Windows was still held after the wait, so nothing was typed: the
    /// characters would have gone in as shortcuts.
    KeysHeld,
    /// Windows has no speech recognizer for the language.
    Unavailable,
    /// Recognition or typing failed.
    Failed,
}

/// The `voice-typed` event. Mirrored in `src/types/protocol.ts` (`VoiceTyped`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceTyped {
    pub outcome: VoiceOutcome,
}

/// Before recognizing: a tap or a silent recording ends the hold here.
pub fn judge_recording(held: Duration, peak: i16) -> Option<VoiceOutcome> {
    if held < MIN_HOLD {
        Some(VoiceOutcome::TooShort)
    } else if peak < SILENCE_PEAK {
        Some(VoiceOutcome::NothingHeard)
    } else {
        None
    }
}

/// The loudest sample, as a magnitude.
pub fn peak(pcm: &[i16]) -> i16 {
    pcm.iter().map(|s| s.saturating_abs()).max().unwrap_or(0)
}

/// After recognizing: type only real text, only once the modifier keys are up (`keys_released`:
/// the wait for them didn't run out), and only into the window the shortcut was pressed in.
pub fn judge_typing(text: &str, keys_released: bool, same_window: bool) -> Option<VoiceOutcome> {
    if text.trim().is_empty() {
        Some(VoiceOutcome::NothingHeard)
    } else if !keys_released {
        Some(VoiceOutcome::KeysHeld)
    } else if !same_window {
        Some(VoiceOutcome::NoTarget)
    } else {
        None
    }
}

/// What the indicator pill says at the end of a hold, or `None` when it just goes (typed).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pill_problem(outcome: VoiceOutcome) -> Option<&'static str> {
    Some(match outcome {
        VoiceOutcome::Typed => return None,
        VoiceOutcome::TooShort => "Hold the keys while you talk",
        VoiceOutcome::NothingHeard => "Didn't catch that",
        VoiceOutcome::MicBlocked => "Microphone blocked. Open tug's Settings to fix it",
        VoiceOutcome::NoMic => "No microphone found",
        VoiceOutcome::MicBusy => "The microphone is busy right now. Try again in a moment",
        VoiceOutcome::NoTarget => "The window changed, so nothing was typed",
        VoiceOutcome::KeysHeld => "Let go of the keys",
        VoiceOutcome::Unavailable => "Voice typing needs a Windows speech language",
        VoiceOutcome::Failed => "Couldn't type that. Try again",
    })
}

/// The shortcut's state for Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceState {
    Off,
    On,
    /// Another app registered the shortcut first.
    InUse,
    /// This PC has no Windows speech recognizer to listen with.
    Unavailable,
}

/// Everything Settings shows. Mirrored in `src/types/protocol.ts` (`VoiceStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub enabled: bool,
    pub state: VoiceState,
    pub keys: String,
    /// The language listened in (an id from `languages`), if any is installed.
    pub language: Option<String>,
    pub languages: Vec<VoiceLanguage>,
    /// Windows' privacy settings block the microphone (checked when Settings asks, and on each hold).
    pub mic_blocked: bool,
    /// The last attempt to turn it on failed, so it was switched back off: what to tell the person
    /// ("Voice typing couldn't start: …"). `None` otherwise.
    pub start_error: Option<String>,
}

/// The state for a request: off unless asked for, unavailable without a recognizer.
pub fn state_for(enabled: bool, has_language: bool, registered: bool) -> VoiceState {
    match (enabled, has_language, registered) {
        (false, _, _) => VoiceState::Off,
        (true, false, _) => VoiceState::Unavailable,
        (true, true, true) => VoiceState::On,
        (true, true, false) => VoiceState::InUse,
    }
}

/// Something Windows refused while voice typing was starting. Never a crash: the switch goes back
/// off and Settings says what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The shortcut's thread didn't start, or Windows refused the shortcut for a reason other
    /// than another app having it (that's `VoiceState::InUse`, which isn't a failure).
    Shortcut(String),
    /// The listening indicator's window couldn't be created.
    Indicator(String),
    /// Windows' speech recognition didn't answer when asked for its languages.
    Speech(String),
}

impl StartError {
    /// What went wrong, for the person (no codes; the detail goes to the log).
    pub fn reason(&self) -> &'static str {
        match self {
            StartError::Shortcut(_) => "Windows didn't accept the shortcut",
            StartError::Indicator(_) => "Windows couldn't show the listening indicator",
            StartError::Speech(_) => "Windows' speech recognition didn't respond",
        }
    }

    /// What Windows said, for tug.log.
    pub fn detail(&self) -> &str {
        match self {
            StartError::Shortcut(d) | StartError::Indicator(d) | StartError::Speech(d) => d,
        }
    }

    /// The line Settings shows.
    pub fn message(&self) -> String {
        format!("Voice typing couldn't start: {}.", self.reason())
    }
}

/// Where a request to turn voice typing on or off ends up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// What the switch shows (and what's saved): back off when starting failed.
    pub enabled: bool,
    pub state: VoiceState,
    pub start_error: Option<String>,
}

/// Settle a request. `attempt` is how starting went: `Ok(registered)` (false: another app has the
/// shortcut, or there's no language to listen in), or what failed. Turning it off can't fail.
pub fn settle(enabled: bool, has_language: bool, attempt: Result<bool, StartError>) -> Settled {
    if !enabled {
        return Settled {
            enabled: false,
            state: VoiceState::Off,
            start_error: None,
        };
    }
    match attempt {
        Ok(registered) => Settled {
            enabled: true,
            state: state_for(true, has_language, registered),
            start_error: None,
        },
        Err(e) => Settled {
            enabled: false,
            state: VoiceState::Off,
            start_error: Some(e.message()),
        },
    }
}

static STATUS: Mutex<Option<VoiceStatus>> = Mutex::new(None);
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);

fn default_status() -> VoiceStatus {
    VoiceStatus {
        enabled: false,
        state: VoiceState::Off,
        keys: DEFAULT_HOTKEY.into(),
        language: None,
        languages: Vec::new(),
        mic_blocked: false,
        start_error: None,
    }
}

/// The current status, with a fresh look at the microphone privacy switch.
pub fn status() -> VoiceStatus {
    let mut s = STATUS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(default_status);
    let blocked = mic_blocked_now();
    if blocked != s.mic_blocked {
        s.mic_blocked = blocked;
        publish(s.clone());
    }
    s
}

/// Store and emit a status if it changed.
fn publish(next: VoiceStatus) {
    let changed = {
        let mut cur = STATUS.lock().unwrap_or_else(|e| e.into_inner());
        let changed = cur.as_ref() != Some(&next);
        *cur = Some(next.clone());
        changed
    };
    if changed {
        if let Some(app) = APP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            use tauri::Emitter;
            if let Err(e) = app.emit(STATUS_EVENT, next) {
                log::warn!("emit voice-status failed: {e}");
            }
        }
    }
}

/// Tell the UI what a hold did.
#[cfg_attr(not(windows), allow(dead_code))]
fn emit_typed(outcome: VoiceOutcome) {
    if let Some(app) = APP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        use tauri::Emitter;
        if let Err(e) = app.emit(TYPED_EVENT, VoiceTyped { outcome }) {
            log::warn!("emit voice-typed failed: {e}");
        }
    }
}

/// Note what the microphone switch said on a hold, so Settings shows it without a refresh.
#[cfg_attr(not(windows), allow(dead_code))]
fn note_mic_blocked(blocked: bool) {
    let cur = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(mut s) = cur {
        if s.mic_blocked != blocked {
            s.mic_blocked = blocked;
            publish(s);
        }
    }
}

/// Turn voice typing on (with this shortcut and language) or off, and say how that went. Starting
/// never takes tug down: if Windows refuses a step, it's logged, the switch goes back off and the
/// status says why (`start_error`). Each step is logged before it's tried, so tug.log shows how
/// far it got.
pub fn apply(enabled: bool, keys: &str, language: Option<&str>) -> VoiceStatus {
    let id = known_hotkey(Some(keys));
    log::info!("voice typing: turning {} ({id})", if enabled { "on" } else { "off" });
    log::info!("voice typing: listing the speech languages");
    let listed = installed_languages();
    match &listed {
        Ok(l) => log::info!("voice typing: {} speech language(s) installed", l.len()),
        Err(e) => log::warn!("voice typing: couldn't list the speech languages: {}", e.detail()),
    }
    let languages = listed.clone().unwrap_or_default();
    let language = pick_language(language, &languages, ui_language());
    let attempt = match (enabled, listed) {
        (false, _) => Ok(false),
        (true, Err(e)) => Err(e),
        (true, Ok(_)) if language.is_none() => Ok(false),
        (true, Ok(_)) => set_registration(parse_hotkey(id)),
    };
    if let Err(e) = &attempt {
        log::warn!("voice typing couldn't start: {}", e.detail());
    }
    let settled = settle(enabled, language.is_some(), attempt);
    let on = settled.state == VoiceState::On;
    if !on {
        // Nothing to undo if it never got as far as the shortcut thread.
        if let Err(e) = set_registration(None) {
            log::warn!("voice typing: couldn't let go of the shortcut: {}", e.detail());
        }
    }
    match settled.state {
        VoiceState::On => log::info!(
            "voice typing on ({id}, language {})",
            language.as_deref().unwrap_or("-")
        ),
        VoiceState::InUse => log::info!("voice typing shortcut {id} is taken by another app"),
        VoiceState::Unavailable => log::info!("voice typing: no Windows speech recognizer installed"),
        VoiceState::Off if settled.start_error.is_some() => log::info!("voice typing switched back off"),
        VoiceState::Off => log::info!("voice typing off"),
    }
    set_engine_language(if on { language.clone() } else { None });
    let s = VoiceStatus {
        enabled: settled.enabled,
        state: settled.state,
        keys: id.into(),
        language,
        languages,
        mic_blocked: mic_blocked_now(),
        start_error: settled.start_error,
    };
    publish(s.clone());
    s
}

/// Start the shortcut thread and turn voice typing on if the owner switched it on.
pub fn start(app: &AppHandle, store: &crate::store::Store) {
    *APP.lock().unwrap_or_else(|e| e.into_inner()) = Some(app.clone());
    let enabled = store.setting(SETTING_ENABLED).ok().flatten().as_deref() == Some("true");
    if !enabled {
        // Nothing to register and no recognizer to load until it's switched on: Settings reads
        // the installed languages when it opens.
        return;
    }
    let keys = store.setting(SETTING_KEYS).ok().flatten();
    let language = store.setting(SETTING_LANGUAGE).ok().flatten();
    // Off the setup path: listing recognizers and registering take a moment. If it can't start,
    // Settings shows it off with the reason; the saved choice stays on, so the next start tries
    // again (what failed may have been passing).
    let spawned = std::thread::Builder::new()
        .name("tug-voice-start".into())
        .spawn(move || {
            apply(true, known_hotkey(keys.as_deref()), language.as_deref());
        });
    if let Err(e) = spawned {
        log::warn!("voice typing: couldn't start at launch ({e})");
    }
}

/// Save the choice and apply it. Called from the `set_voice_typing` command. What's saved is
/// where it ended up: turning it on and failing saves it as off, matching the switch.
pub fn set(store: &crate::store::Store, enabled: bool, keys: &str, language: Option<&str>) -> VoiceStatus {
    let s = apply(enabled, keys, language);
    let saves = [
        (SETTING_ENABLED, if s.enabled { "true" } else { "false" }.to_string()),
        (SETTING_KEYS, s.keys.clone()),
        (SETTING_LANGUAGE, s.language.clone().unwrap_or_default()),
    ];
    for (k, v) in saves {
        if let Err(e) = store.set_setting(k, &v) {
            log::warn!("couldn't save {k}: {e}");
        }
    }
    s
}

/// The installed languages, listed once the first time they're needed. Settings calls this
/// before voice typing is on, so the language list is there to choose from.
pub fn refresh_languages() -> VoiceStatus {
    let mut s = status();
    if s.languages.is_empty() {
        s.languages = installed_languages().unwrap_or_else(|e| {
            log::warn!("voice typing: couldn't list the speech languages: {}", e.detail());
            Vec::new()
        });
        s.language = pick_language(s.language.as_deref(), &s.languages, ui_language());
        if s.enabled && s.languages.is_empty() {
            s.state = VoiceState::Unavailable;
        }
        publish(s.clone());
    }
    s
}

#[cfg(windows)]
fn installed_languages() -> Result<Vec<VoiceLanguage>, StartError> {
    win::languages()
}
#[cfg(not(windows))]
fn installed_languages() -> Result<Vec<VoiceLanguage>, StartError> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn ui_language() -> u16 {
    win::ui_language()
}
#[cfg(not(windows))]
fn ui_language() -> u16 {
    0x409
}

/// Register the shortcut (`None`: let it go). `Ok(false)`: another app has it.
#[cfg(windows)]
fn set_registration(keys: Option<Keys>) -> Result<bool, StartError> {
    win::register(keys)
}
#[cfg(not(windows))]
fn set_registration(_keys: Option<Keys>) -> Result<bool, StartError> {
    Ok(false)
}

#[cfg(windows)]
fn set_engine_language(language: Option<String>) {
    win::set_language(language);
}
#[cfg(not(windows))]
fn set_engine_language(_language: Option<String>) {}

/// Whether voice typing is in the middle of a hold right now (listening, recognizing or typing):
/// an update restart waits for it.
#[cfg(windows)]
pub fn busy() -> bool {
    win::busy()
}
#[cfg(not(windows))]
pub fn busy() -> bool {
    false
}

#[cfg(windows)]
fn mic_blocked_now() -> bool {
    win::mic_blocked()
}
#[cfg(not(windows))]
fn mic_blocked_now() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_the_shared_shortcut_list() {
        let raw = include_str!("../../../src/lib/voiceHotkeys.json");
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
    fn never_clashes_with_tug_or_code_fill() {
        for id in HOTKEYS {
            assert!(!crate::code_fill::HOTKEYS.contains(id), "{id} is code fill's");
            for taken in ["ctrl+k", "ctrl+n", "ctrl+shift+c", "ctrl+shift+v", "ctrl+,"] {
                assert_ne!(*id, taken);
            }
        }
    }

    #[test]
    fn parses_shortcuts_for_register_hot_key() {
        let k = parse_hotkey("ctrl+alt+space").unwrap();
        assert_eq!(
            k,
            Keys {
                ctrl: true,
                shift: false,
                alt: true,
                key: 0x20
            }
        );
        assert_eq!(k.vk(), 0x20);
        assert_eq!(k.modifiers(), 0x2 | 0x1 | 0x4000);
        assert_eq!(parse_hotkey("ctrl+alt+f12").unwrap().key, 0x7b);
        assert_eq!(parse_hotkey("ctrl+shift+f11").unwrap().key, 0x7a);
        // Not offered: never registered.
        assert_eq!(parse_hotkey("ctrl+space"), None);
        assert_eq!(parse_hotkey("alt+f4"), None);
        assert_eq!(known_hotkey(Some("ctrl+k")), DEFAULT_HOTKEY);
        assert_eq!(known_hotkey(None), DEFAULT_HOTKEY);
        assert_eq!(known_hotkey(Some("ctrl+alt+f12")), "ctrl+alt+f12");
    }

    #[test]
    fn names_and_ids_from_the_recognizer() {
        assert_eq!(
            language_name("Microsoft Speech Recognizer 8.0 for Windows (English - US)"),
            "English (US)"
        );
        assert_eq!(
            language_name("Microsoft Speech Recognizer 8.0 for Windows (Chinese Simplified - PRC)"),
            "Chinese Simplified (PRC)"
        );
        assert_eq!(language_name("Some engine (German)"), "German");
        assert_eq!(language_name("Plain"), "Plain");
        assert_eq!(language_id("409;9"), Some("409".into()));
        assert_eq!(language_id("809"), Some("809".into()));
        assert_eq!(language_id("C0A"), Some("c0a".into()));
        assert_eq!(language_id(""), None);
        assert_eq!(language_id("en-US"), None);
    }

    fn langs(ids: &[&str]) -> Vec<VoiceLanguage> {
        ids.iter()
            .map(|id| VoiceLanguage {
                id: id.to_string(),
                name: id.to_string(),
            })
            .collect()
    }

    #[test]
    fn picks_a_language() {
        let l = langs(&["407", "409", "809"]);
        assert_eq!(
            pick_language(Some("407"), &l, 0x409),
            Some("407".into()),
            "the saved one"
        );
        assert_eq!(
            pick_language(Some("40c"), &l, 0x809),
            Some("809".into()),
            "saved one gone: Windows' language"
        );
        assert_eq!(
            pick_language(None, &l, 0xc07),
            Some("407".into()),
            "German (Austria) → German"
        );
        assert_eq!(
            pick_language(None, &l, 0x411),
            Some("409".into()),
            "no Japanese → English (US)"
        );
        assert_eq!(pick_language(None, &langs(&["40c"]), 0x411), Some("40c".into()));
        assert_eq!(pick_language(Some("409"), &[], 0x409), None, "nothing installed");
    }

    #[test]
    fn microphone_privacy_switches() {
        assert!(!mic_blocked(None, None, None));
        assert!(!mic_blocked(Some("Allow"), Some("Allow"), Some("Allow")));
        assert!(mic_blocked(Some("Deny"), Some("Allow"), Some("Allow")));
        assert!(mic_blocked(None, Some("Deny"), None));
        assert!(mic_blocked(None, None, Some("deny")), "desktop apps switch");
    }

    #[test]
    fn a_hold_is_judged_before_and_after_recognizing() {
        let ms = Duration::from_millis;
        assert_eq!(judge_recording(ms(120), 9000), Some(VoiceOutcome::TooShort));
        assert_eq!(judge_recording(ms(2000), 40), Some(VoiceOutcome::NothingHeard));
        assert_eq!(judge_recording(ms(2000), 9000), None);
        assert_eq!(judge_typing("", true, true), Some(VoiceOutcome::NothingHeard));
        assert_eq!(judge_typing("  ", true, false), Some(VoiceOutcome::NothingHeard));
        assert_eq!(judge_typing("userId", true, false), Some(VoiceOutcome::NoTarget));
        assert_eq!(judge_typing("userId", true, true), None);
        assert_eq!(peak(&[3, -9000, 200]), 9000);
        assert_eq!(peak(&[i16::MIN]), i16::MAX);
        assert_eq!(peak(&[]), 0);
    }

    #[test]
    fn keys_still_held_after_the_wait_means_nothing_is_typed() {
        // Ctrl, Alt or Shift still down once the wait ran out: typing would fire shortcuts.
        assert_eq!(judge_typing("userId", false, true), Some(VoiceOutcome::KeysHeld));
        assert_eq!(
            judge_typing("userId", false, false),
            Some(VoiceOutcome::KeysHeld),
            "nothing is typed either way; the keys are what to fix"
        );
        assert_eq!(pill_problem(VoiceOutcome::KeysHeld), Some("Let go of the keys"));
        // Nothing heard is still said first: there was nothing to type.
        assert_eq!(judge_typing(" ", false, true), Some(VoiceOutcome::NothingHeard));
        assert_eq!(pill_problem(VoiceOutcome::Typed), None);
        let json = serde_json::to_string(&VoiceTyped {
            outcome: VoiceOutcome::KeysHeld,
        })
        .unwrap();
        assert_eq!(json, r#"{"outcome":"keysHeld"}"#);
    }

    #[test]
    fn states_tell_the_truth() {
        assert_eq!(state_for(false, true, false), VoiceState::Off);
        assert_eq!(state_for(true, false, false), VoiceState::Unavailable);
        assert_eq!(state_for(true, true, true), VoiceState::On);
        assert_eq!(state_for(true, true, false), VoiceState::InUse);
    }

    #[test]
    fn a_failed_start_puts_the_switch_back_off_and_says_why() {
        for (e, reason) in [
            (
                StartError::Indicator("CreateWindowExW failed (0x80070578)".into()),
                "Voice typing couldn't start: Windows couldn't show the listening indicator.",
            ),
            (
                StartError::Shortcut("the shortcut thread didn't answer".into()),
                "Voice typing couldn't start: Windows didn't accept the shortcut.",
            ),
            (
                StartError::Speech("COM didn't start".into()),
                "Voice typing couldn't start: Windows' speech recognition didn't respond.",
            ),
        ] {
            let s = settle(true, true, Err(e.clone()));
            assert!(!s.enabled, "{e:?}: the switch goes back off");
            assert_eq!(s.state, VoiceState::Off);
            assert_eq!(s.start_error.as_deref(), Some(reason));
            // Windows' detail is for the log, never the person.
            assert!(!reason.contains(e.detail()));
        }
    }

    #[test]
    fn a_working_start_or_turning_it_off_has_no_error() {
        assert_eq!(
            settle(true, true, Ok(true)),
            Settled {
                enabled: true,
                state: VoiceState::On,
                start_error: None
            }
        );
        // Another app having the shortcut, or no language, isn't a failed start: it stays on and
        // Settings offers the fix.
        assert_eq!(settle(true, true, Ok(false)).state, VoiceState::InUse);
        assert!(settle(true, true, Ok(false)).enabled);
        assert_eq!(settle(true, false, Ok(false)).state, VoiceState::Unavailable);
        // Off is off, whatever starting would have said.
        let off = settle(false, true, Err(StartError::Shortcut("x".into())));
        assert_eq!(
            off,
            Settled {
                enabled: false,
                state: VoiceState::Off,
                start_error: None
            }
        );
    }

    #[test]
    fn reaches_the_frontend_in_camel_case() {
        let json = serde_json::to_string(&VoiceTyped {
            outcome: VoiceOutcome::MicBlocked,
        })
        .unwrap();
        assert_eq!(json, r#"{"outcome":"micBlocked"}"#);
        let json = serde_json::to_string(&VoiceStatus {
            enabled: true,
            state: VoiceState::InUse,
            keys: "ctrl+alt+space".into(),
            language: Some("409".into()),
            languages: vec![VoiceLanguage {
                id: "409".into(),
                name: "English (US)".into(),
            }],
            mic_blocked: false,
            start_error: None,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"enabled":true,"state":"inUse","keys":"ctrl+alt+space","language":"409","languages":[{"id":"409","name":"English (US)"}],"micBlocked":false,"startError":null}"#
        );
    }
}
