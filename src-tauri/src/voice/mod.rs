//! Voice typing ("voice to code"): hold a shortcut, talk, let go, and tug types what you said
//! into whatever has focus, using speech recognition that runs on this PC.
//!
//! Off by default. While it's on, tug registers one global shortcut (the same RegisterHotKey
//! approach as code fill's type-the-code shortcut). Pressing it opens the microphone; letting go
//! closes it, the recording is recognized on the PC, shaped for where it's going (`text`), and
//! typed with SendInput Unicode keystrokes. One hold, one insertion: a second press while one is
//! being typed is ignored, and every path ends with the microphone closed and the indicator gone.
//!
//! Recognition is behind [`engine::Engine`], with three engines that all run on the PC: Accurate
//! (whisper.cpp with an English speech model), Most accurate (whisper.cpp with the multilingual
//! large-v3-turbo model; both downloaded once in the background: `model.rs`, `win/whisper.rs`)
//! and Basic (Windows' own offline SAPI 5 recognizer, nothing to download: `win/sapi.rs`).
//! Accurate is the default once its model is there; until then, and wherever the picked engine
//! can't run, something smaller listens ([`engine::pick_engine`]). Audio never leaves the PC and
//! is never written to disk: it lives in memory for one hold and is dropped. The model downloads
//! are the only network use, and carry nothing about the person. The whisper.cpp engines are
//! given the person's own words (their dictionary, and their most frequent contacts' names:
//! [`vocab::Hints`]) as hints; those stay on the PC. Logs carry lengths, counts and durations,
//! never what was said or the hints.
//!
//! This file is the pure part (shortcuts, settings, what a hold means, the status the UI reads)
//! plus the bookkeeping (status, the model downloads, the hints); `win/` is the thin Windows I/O.

pub mod audio;
pub mod engine;
pub mod model;
pub mod text;
pub mod vocab;
#[cfg(windows)]
mod win;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;

use crate::code_fill::Keys;
use crate::store::Store;
use engine::{AccurateProblem, EngineChoice, EngineInputs};
use model::{ModelId, ModelState, ModelStatus};
use vocab::Hints;

/// The status event (`VoiceStatus`), sent when it changes.
pub const STATUS_EVENT: &str = "voice-status";
/// What one hold did (`VoiceTyped`), never what was said.
pub const TYPED_EVENT: &str = "voice-typed";

/// Settings keys (written by `set_voice_typing`, read at startup).
pub const SETTING_ENABLED: &str = "ui.voiceTyping";
pub const SETTING_KEYS: &str = "ui.voiceKeys";
pub const SETTING_LANGUAGE: &str = "ui.voiceLanguage";
/// "accurate" (the default), "mostAccurate" or "basic".
pub const SETTING_ENGINE: &str = "ui.voiceEngine";
/// v0.5.19's Settings offer: "true" once it was answered. A "Not now" there (with nothing
/// downloaded since) counts as declining Accurate's download ([`declined`]).
pub const SETTING_MODEL_OFFERED: &str = "ui.voiceModelOffered";
/// The language Most accurate listens in: "auto" or one of [`engine::SPOKEN_LANGUAGES`]; until
/// it's picked, Windows' display language ([`engine::default_spoken_language`]).
pub const SETTING_SPOKEN_LANGUAGE: &str = "ui.voiceSpokenLanguage";
/// "false" when contacts' names shouldn't be hints (on by default).
pub const SETTING_CONTACT_HINTS: &str = "ui.voiceContactHints";
/// The personal dictionary, a JSON array of words. The person's own words: never logged, and not
/// in the diagnostics' settings list.
pub const SETTING_DICTIONARY: &str = "ui.voiceDictionary";
/// "shown" once the "Get voice typing ready now?" card has been on screen: never again.
pub const SETTING_READY_PROMPT: &str = "ui.voiceReadyPrompt";
/// Followed by a model's id: "true" after the person cancelled its download (or deleted the
/// models), so turning voice typing on doesn't fetch it again by itself; "false" once they ask for
/// it (Download, or picking its engine).
pub const SETTING_DECLINED_PREFIX: &str = "ui.voiceModelDeclined.";

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

/// Whether the default microphone is a phone-call headset: a Bluetooth headset's hands-free link
/// (Windows names it "… Hands-Free …") or anything Windows mixes at 16 kHz or less. Those carry
/// narrow, compressed audio, and every recognizer does worse with it; Settings says so.
pub fn phone_call_mic(name: &str, mix_rate: u32) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("hands-free") || name.contains("handsfree") || (mix_rate > 0 && mix_rate <= 16_000)
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
    /// No window was in front when the keys were pressed, so there was nothing to type into.
    NoWindow,
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
        VoiceOutcome::NoWindow => "Click where the text should go, then try again",
        VoiceOutcome::KeysHeld => "Let go of the keys",
        VoiceOutcome::Unavailable => "Voice typing needs a speech language. Open tug's Settings",
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
    /// Nothing can listen: Windows has no speech language, and the accurate engine can't stand
    /// in (no model yet, or Basic was picked).
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
    /// The engine picked in Settings.
    pub engine: EngineChoice,
    /// The engine a hold would use right now ([`engine::pick_engine`]); `None` when nothing can
    /// listen. Settings uses it to say "Basic until the model is downloaded".
    pub active_engine: Option<EngineChoice>,
    /// Accurate's speech model.
    pub model: ModelStatus,
    /// Most accurate's speech model.
    pub most_accurate_model: ModelStatus,
    /// What Most accurate listens in: "auto" or one of [`engine::SPOKEN_LANGUAGES`].
    pub spoken_language: String,
    /// Contacts' names are hints for the whisper.cpp engines.
    pub contact_hints: bool,
    /// This processor can run the whisper.cpp engines ([`engine::cpu_supported`]).
    pub accurate_supported: bool,
    /// Offer the one-time "Get voice typing ready now?" card ([`ready_prompt_due`]).
    pub ready_prompt: bool,
    /// The default microphone is a phone-call headset ([`phone_call_mic`]).
    pub phone_mic: bool,
    /// Why the whisper.cpp engine picked isn't listening although it could, if it isn't.
    pub accurate_problem: Option<AccurateProblem>,
}

impl VoiceStatus {
    pub fn model_of(&self, id: ModelId) -> &ModelStatus {
        match id {
            ModelId::Accurate => &self.model,
            ModelId::MostAccurate => &self.most_accurate_model,
        }
    }

    pub fn model_of_mut(&mut self, id: ModelId) -> &mut ModelStatus {
        match id {
            ModelId::Accurate => &mut self.model,
            ModelId::MostAccurate => &mut self.most_accurate_model,
        }
    }
}

/// What the speech thread listens with. Built from the status by [`engine_config`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineConfig {
    /// `None` while voice typing is off or nothing can listen: every engine is let go.
    pub engine: Option<EngineChoice>,
    /// Windows' recognizer language (Basic, and the whisper.cpp engines' fallback).
    pub language: Option<String>,
    /// The model file, when the engine is Accurate or Most accurate.
    pub model: Option<PathBuf>,
    /// Accurate's model file, when the engine is Most accurate and Accurate could listen: what
    /// loads instead when there isn't memory for Most accurate ([`engine::plan_load`]).
    pub fallback: Option<PathBuf>,
    /// What the engine listens in: "en" for Accurate, Most accurate's language (or "auto").
    pub spoken: &'static str,
}

/// The inputs to [`engine::pick_engine`] from a status.
fn engine_inputs(s: &VoiceStatus) -> EngineInputs<'_> {
    EngineInputs {
        choice: s.engine,
        model_ready: s.model.state == ModelState::Ready,
        most_ready: s.most_accurate_model.state == ModelState::Ready,
        cpu_ok: s.accurate_supported,
        sapi_language: s.language.as_deref(),
        accurate_failed: s.accurate_problem == Some(AccurateProblem::Failed),
    }
}

/// Where each model is on disk, by [`ModelId::index`].
pub type ModelFiles<'a> = [Option<&'a Path>; 2];

/// What the speech thread should listen with for this status (`files`: where the models are).
pub fn engine_config(s: &VoiceStatus, files: ModelFiles) -> EngineConfig {
    if s.state != VoiceState::On {
        return EngineConfig::default();
    }
    let inputs = engine_inputs(s);
    let engine = engine::pick_engine(&inputs);
    let file = |m: ModelId| files[m.index()].map(Path::to_path_buf);
    let most = engine == Some(EngineChoice::MostAccurate);
    EngineConfig {
        engine,
        language: s.language.clone(),
        model: engine.and_then(EngineChoice::model).and_then(file),
        fallback: (most && engine::accurate_usable(&inputs))
            .then(|| file(ModelId::Accurate))
            .flatten(),
        spoken: if most {
            engine::spoken_language(Some(&s.spoken_language))
        } else {
            "en"
        },
    }
}

/// Which model to download by itself when voice typing is turned on (or an engine is picked): the
/// picked whisper.cpp engine's, if it isn't here or on its way, this PC can run it, the person
/// hasn't declined it ([`declined`]), and (for Accurate, which is English-only) Windows' speech
/// language is English or there is none. A paused download resumes.
pub fn auto_download(s: &VoiceStatus, declined: impl Fn(ModelId) -> bool) -> Option<ModelId> {
    if !s.enabled || !s.accurate_supported {
        return None;
    }
    let id = s.engine.model()?;
    if id == ModelId::Accurate && !s.language.as_deref().is_none_or(engine::is_english) {
        return None;
    }
    (s.model_of(id).state == ModelState::Missing && !declined(id)).then_some(id)
}

/// Whether the person declined a model's automatic download: its saved answer ("true" after a
/// Cancel or deleting the models, "false" once they asked for it), or, with no answer yet, for
/// Accurate only, "Not now" to v0.5.19's offer (`legacy_offered`) with nothing downloaded since.
pub fn declined(id: ModelId, saved: Option<&str>, legacy_offered: bool, nothing_on_disk: bool) -> bool {
    match saved.map(str::trim) {
        Some("true") => true,
        Some(_) => false,
        None => id == ModelId::Accurate && legacy_offered && nothing_on_disk,
    }
}

/// Whether to offer the one-time "Get voice typing ready now?" card: voice typing has never been
/// set up (never switched on or off, no model or part of one on disk), the card has never been
/// shown, and this PC can run the whisper.cpp engines. Checked at startup, so it's the first launch
/// after an install, or after the update that brought the card; once it's shown it's saved and
/// never offered again, whatever the answer.
pub fn ready_prompt_due(ever_set_up: bool, shown_before: bool, model_on_disk: bool, cpu_ok: bool) -> bool {
    !ever_set_up && !shown_before && !model_on_disk && cpu_ok
}

/// The personal dictionary from its saved JSON, tidied ([`vocab::normalize_dictionary`]); empty
/// when there's none or it can't be read.
pub fn parse_dictionary(saved: Option<&str>) -> Vec<String> {
    let words: Vec<String> = saved.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    vocab::normalize_dictionary(&words)
}

/// The state for a request: off unless asked for, unavailable when no engine can listen.
pub fn state_for(enabled: bool, can_listen: bool, registered: bool) -> VoiceState {
    match (enabled, can_listen, registered) {
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
/// shortcut, or no engine can listen), or what failed. Turning it off can't fail.
pub fn settle(enabled: bool, can_listen: bool, attempt: Result<bool, StartError>) -> Settled {
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
            state: state_for(true, can_listen, registered),
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
/// Held for the whole of [`apply`] (and [`resettle`]'s look and apply): the switch, Settings,
/// a download finishing and startup can each turn voice typing on or off, and one at a time keeps
/// the shortcut, the status and the saved switch in agreement.
static APPLYING: Mutex<()> = Mutex::new(());
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);
/// tug's database: the settings voice typing saves and the contacts its hints come from.
static STORE: OnceLock<Arc<Store>> = OnceLock::new();
/// Where the speech models live (`speech\` under tug's local app data), set at startup. Local app
/// data, so they stay through updates (the installer doesn't touch it).
static MODEL_DIR: OnceLock<PathBuf> = OnceLock::new();
/// A model's download is running (by [`ModelId::index`]).
static DOWNLOADING: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// Set by Cancel; that download stops within one chunk.
static CANCEL_DOWNLOAD: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

fn model_status_now(id: ModelId) -> ModelStatus {
    match MODEL_DIR.get() {
        Some(dir) => model::status_on_disk(dir, id.spec()),
        None => ModelStatus::new(ModelState::Missing, 0, id.spec().size),
    }
}

fn default_status() -> VoiceStatus {
    VoiceStatus {
        enabled: false,
        state: VoiceState::Off,
        keys: DEFAULT_HOTKEY.into(),
        language: None,
        languages: Vec::new(),
        mic_blocked: false,
        start_error: None,
        engine: EngineChoice::Accurate,
        active_engine: None,
        model: model_status_now(ModelId::Accurate),
        most_accurate_model: model_status_now(ModelId::MostAccurate),
        spoken_language: "auto".into(),
        contact_hints: true,
        accurate_supported: engine::cpu_supported(),
        ready_prompt: false,
        phone_mic: false,
        accurate_problem: None,
    }
}

/// A saved setting, if tug's database is there.
fn saved(key: &str) -> Option<String> {
    STORE.get().and_then(|s| s.setting(key).ok().flatten())
}

/// Save a setting (logged by its key if it fails, never its value).
fn save(key: &str, value: &str) {
    match STORE.get() {
        Some(store) => {
            if let Err(e) = store.set_setting(key, value) {
                log::warn!("couldn't save {key}: {e}");
            }
        }
        None => log::warn!("couldn't save {key}: no database yet"),
    }
}

/// Whether the person declined this model's automatic download ([`declined`]).
fn declined_now(id: ModelId, s: &VoiceStatus) -> bool {
    let m = s.model_of(id);
    declined(
        id,
        saved(&format!("{SETTING_DECLINED_PREFIX}{}", id.as_str())).as_deref(),
        saved(SETTING_MODEL_OFFERED).as_deref() == Some("true"),
        m.state == ModelState::Missing && m.received == 0,
    )
}

fn set_declined(id: ModelId, declined: bool) {
    save(
        &format!("{SETTING_DECLINED_PREFIX}{}", id.as_str()),
        if declined { "true" } else { "false" },
    );
}

// ---- The person's own words for the prompt ----

/// How long the hints are used before they're read again (new texts change who's texted most).
const HINTS_MAX_AGE: Duration = Duration::from_secs(10 * 60);
/// Texts in this window count first when ranking contacts.
const RECENT_TEXTS: Duration = Duration::from_secs(90 * 24 * 60 * 60);

struct HintsCache {
    hints: Hints,
    read_at: Option<Instant>,
    refreshing: bool,
}

static HINTS: Mutex<HintsCache> = Mutex::new(HintsCache {
    hints: Hints {
        words: Vec::new(),
        names: Vec::new(),
    },
    read_at: None,
    refreshing: false,
});

/// Read the dictionary and (when switched on) the most frequent contacts' names from the database.
fn read_hints(contact_hints: bool) -> Hints {
    let Some(store) = STORE.get() else {
        return Hints::default();
    };
    let words = parse_dictionary(store.setting(SETTING_DICTIONARY).ok().flatten().as_deref());
    let names = if contact_hints {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        let since = now_ms - RECENT_TEXTS.as_millis() as i64;
        // A few extra: some names are dropped when they're tidied (`vocab::clean_entry`).
        store
            .frequent_contact_names(vocab::MAX_NAMES as u32 + 10, since)
            .unwrap_or_else(|e| {
                log::warn!("voice typing: couldn't read contacts for the hints: {e}");
                Vec::new()
            })
    } else {
        Vec::new()
    };
    Hints::new(&words, &names)
}

/// Read the hints again now (after the dictionary or the contacts switch changed, or at start).
/// Logs how many, never which.
fn refresh_hints() {
    let hints = read_hints(current().contact_hints);
    log::info!(
        "voice typing: {} dictionary words and {} contact names for the hints",
        hints.words.len(),
        hints.names.len()
    );
    let mut cache = HINTS.lock().unwrap_or_else(|e| e.into_inner());
    cache.hints = hints;
    cache.read_at = Some(Instant::now());
    cache.refreshing = false;
}

/// [`refresh_hints`] on a thread of its own.
fn refresh_hints_soon() {
    {
        let mut cache = HINTS.lock().unwrap_or_else(|e| e.into_inner());
        if cache.refreshing {
            return;
        }
        cache.refreshing = true;
    }
    if let Err(e) = std::thread::Builder::new()
        .name("tug-voice-hints".into())
        .spawn(refresh_hints)
    {
        log::warn!("voice typing: couldn't read the hints ({e})");
        HINTS.lock().unwrap_or_else(|e| e.into_inner()).refreshing = false;
    }
}

/// The hints for a hold (from the speech thread): what was read last, read again in the background
/// when it's older than [`HINTS_MAX_AGE`] (the hold doesn't wait for the database).
#[cfg_attr(not(windows), allow(dead_code))]
fn hints() -> Hints {
    let (hints, stale) = {
        let cache = HINTS.lock().unwrap_or_else(|e| e.into_inner());
        let stale = cache.read_at.is_none_or(|t| t.elapsed() > HINTS_MAX_AGE);
        (cache.hints.clone(), stale)
    };
    if stale {
        refresh_hints_soon();
    }
    hints
}

/// The status as last published (or the default), without looking at anything again.
fn current() -> VoiceStatus {
    STATUS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(default_status)
}

/// Each model's file, if it's downloaded.
fn model_files() -> [Option<PathBuf>; 2] {
    ModelId::ALL.map(|id| MODEL_DIR.get().and_then(|dir| model::installed(dir, id.spec())))
}

/// The speech thread's configuration for a status, with the models where they are.
fn config_for(s: &VoiceStatus) -> EngineConfig {
    let files = model_files();
    engine_config(s, [files[0].as_deref(), files[1].as_deref()])
}

/// The current status, with a fresh look at the microphone privacy switch.
pub fn status() -> VoiceStatus {
    let blocked = mic_blocked_now();
    update(|s| s.mic_blocked = blocked)
}

/// Change the status under its lock (so the download thread, the speech thread and Settings never
/// undo each other's changes), work out `active_engine` from the rest, emit it if it changed, and
/// tell the speech thread if what it listens with changed. The speech thread is told while the
/// lock is still held, so two updates' configurations can never reach it in the wrong order.
fn update(change: impl FnOnce(&mut VoiceStatus)) -> VoiceStatus {
    let (before, after) = {
        let mut cur = STATUS.lock().unwrap_or_else(|e| e.into_inner());
        let before = cur.clone();
        let mut next = before.clone().unwrap_or_else(default_status);
        change(&mut next);
        next.active_engine = engine::pick_engine(&engine_inputs(&next));
        *cur = Some(next.clone());
        let was = before
            .as_ref()
            .map(|b| engine_config(b, [None, None]))
            .unwrap_or_default();
        if was != engine_config(&next, [None, None]) {
            set_engine_config(config_for(&next));
        }
        (before, next)
    };
    if before.as_ref() != Some(&after) {
        if let Some(app) = APP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            use tauri::Emitter;
            if let Err(e) = app.emit(STATUS_EVENT, after.clone()) {
                log::warn!("emit voice-status failed: {e}");
            }
        }
    }
    after
}

/// Tell the speech thread what to listen with, from the status as it is now (under the lock, like
/// `update`).
fn configure_now() {
    let cur = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    let s = cur.clone().unwrap_or_else(default_status);
    set_engine_config(config_for(&s));
}

/// Whether voice typing is switched on but its state no longer matches whether anything can
/// listen: the model arrived (or was picked) on a PC with no Windows speech language, so it can
/// listen now but no shortcut is registered; or Basic was picked (or the model was lost) on such a
/// PC, so the shortcut is registered but nothing can listen.
pub fn needs_resettle(s: &VoiceStatus) -> bool {
    if !s.enabled {
        return false;
    }
    let can_listen = engine::pick_engine(&engine_inputs(s)).is_some();
    let listening = matches!(s.state, VoiceState::On | VoiceState::InUse);
    can_listen != listening
}

/// Turn voice typing on again with its shortcut and language when what can listen changed under
/// it ([`needs_resettle`]), so the shortcut and Settings match. Waits for the speech thread (it
/// lists the languages), so never call it from that thread.
fn resettle() {
    // Looked at under the lock: a switch-off that got there first isn't undone.
    let _one_at_a_time = APPLYING.lock().unwrap_or_else(|e| e.into_inner());
    let s = current();
    if needs_resettle(&s) {
        log::info!(
            "voice typing: what can listen changed while it was {:?}; settling again",
            s.state
        );
        apply_now(true, &s.keys, s.language.as_deref());
    }
}

/// [`resettle`] on a thread of its own, for callers on the speech thread.
#[cfg_attr(not(windows), allow(dead_code))]
fn resettle_soon() {
    if let Err(e) = std::thread::Builder::new()
        .name("tug-voice-resettle".into())
        .spawn(resettle)
    {
        log::warn!("voice typing: couldn't settle again ({e})");
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
    update(|s| s.mic_blocked = blocked);
}

/// Turn voice typing on (with this shortcut and language) or off, and say how that went. Starting
/// never takes tug down: if Windows refuses a step, it's logged, the switch goes back off and the
/// status says why (`start_error`). Each step is logged before it's tried, so tug.log shows how
/// far it got.
pub fn apply(enabled: bool, keys: &str, language: Option<&str>) -> VoiceStatus {
    let _one_at_a_time = APPLYING.lock().unwrap_or_else(|e| e.into_inner());
    apply_now(enabled, keys, language)
}

/// [`apply`], with [`APPLYING`] already held.
fn apply_now(enabled: bool, keys: &str, language: Option<&str>) -> VoiceStatus {
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
    let mut probe = current();
    probe.language = language.clone();
    let can_listen = engine::pick_engine(&engine_inputs(&probe)).is_some();
    // A failed listing only stops voice typing when nothing else can listen: with the speech
    // model there, Accurate listens without Windows' languages.
    let attempt = match (enabled, listed) {
        (false, _) => Ok(false),
        (true, _) if can_listen => set_registration(parse_hotkey(id)),
        (true, Err(e)) => Err(e),
        (true, Ok(_)) => Ok(false),
    };
    if let Err(e) = &attempt {
        log::warn!("voice typing couldn't start: {}", e.detail());
    }
    let settled = settle(enabled, can_listen, attempt);
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
        VoiceState::Unavailable => {
            log::info!("voice typing: nothing to listen with (no Windows speech language, no usable speech model)")
        }
        VoiceState::Off if settled.start_error.is_some() => log::info!("voice typing switched back off"),
        VoiceState::Off => log::info!("voice typing off"),
    }
    let blocked = mic_blocked_now();
    // The engine, the model and the microphone stay as they are now (a download may be running).
    let s = update(|s| {
        s.enabled = settled.enabled;
        s.state = settled.state;
        s.keys = id.into();
        s.language = language;
        s.languages = languages;
        s.mic_blocked = blocked;
        s.start_error = settled.start_error;
    });
    if on {
        log::info!(
            "voice typing: listening with the {} engine",
            s.active_engine.map_or("-", EngineChoice::as_str)
        );
    }
    // Always, so a fresh start (or a changed language) reaches the speech thread.
    configure_now();
    s
}

/// Start the shortcut thread and turn voice typing on if the owner switched it on.
pub fn start(app: &AppHandle, store: &Arc<Store>) {
    *APP.lock().unwrap_or_else(|e| e.into_inner()) = Some(app.clone());
    let _ = STORE.set(store.clone());
    {
        use tauri::Manager;
        match app.path().app_local_data_dir() {
            Ok(dir) => {
                let _ = MODEL_DIR.set(dir.join(model::DIR));
            }
            Err(e) => log::warn!("voice typing: no local app data folder for the speech models ({e})"),
        }
    }
    let engine = EngineChoice::parse(saved(SETTING_ENGINE).as_deref());
    let spoken = match saved(SETTING_SPOKEN_LANGUAGE) {
        Some(s) => engine::spoken_language(Some(&s)),
        None => engine::default_spoken_language(ui_language()),
    };
    let contact_hints = saved(SETTING_CONTACT_HINTS).as_deref() != Some("false");
    let ever_set_up = saved(SETTING_ENABLED).is_some();
    let prompt_shown = saved(SETTING_READY_PROMPT).is_some();
    let s = update(|s| {
        s.engine = engine;
        s.spoken_language = spoken.into();
        s.contact_hints = contact_hints;
        s.model = model_status_now(ModelId::Accurate);
        s.most_accurate_model = model_status_now(ModelId::MostAccurate);
        let on_disk = ModelId::ALL
            .iter()
            .any(|id| s.model_of(*id).state == ModelState::Ready || s.model_of(*id).received > 0);
        s.ready_prompt = ready_prompt_due(ever_set_up, prompt_shown, on_disk, s.accurate_supported);
    });
    let on_disk = |m: &ModelStatus| match m.state {
        ModelState::Ready => "downloaded",
        _ => "not downloaded",
    };
    log::info!(
        "voice typing: {} engine picked, speech models: accurate {}, most accurate {}{}",
        s.engine.as_str(),
        on_disk(&s.model),
        on_disk(&s.most_accurate_model),
        if s.accurate_supported {
            ""
        } else {
            " (this processor can't run them)"
        }
    );
    let enabled = saved(SETTING_ENABLED).as_deref() == Some("true");
    if !enabled {
        // Nothing to register and no recognizer to load until it's switched on: Settings reads
        // the installed languages when it opens.
        return;
    }
    let keys = saved(SETTING_KEYS);
    let language = saved(SETTING_LANGUAGE);
    // Off the setup path: listing recognizers and registering take a moment. If it can't start,
    // Settings shows it off with the reason; the saved choice stays on, so the next start tries
    // again (what failed may have been passing).
    let spawned = std::thread::Builder::new()
        .name("tug-voice-start".into())
        .spawn(move || {
            refresh_hints();
            apply(true, known_hotkey(keys.as_deref()), language.as_deref());
            // A download that was paused (tug closed part way) carries on.
            download_automatically();
        });
    if let Err(e) = spawned {
        log::warn!("voice typing: couldn't start at launch ({e})");
    }
}

/// Save the choice and apply it. Called from the `set_voice_typing` command. What's saved is
/// where it ended up: turning it on and failing saves it as off, matching the switch. Turning it on
/// starts the picked engine's model download in the background if it isn't here
/// ([`auto_download`]).
pub fn set(enabled: bool, keys: &str, language: Option<&str>) -> VoiceStatus {
    // Switching it on (or off) gives the whisper.cpp engine another chance if it failed to start.
    update(|s| s.accurate_problem = None);
    let s = apply(enabled, keys, language);
    save(SETTING_ENABLED, if s.enabled { "true" } else { "false" });
    save(SETTING_KEYS, &s.keys);
    save(SETTING_LANGUAGE, &s.language.clone().unwrap_or_default());
    if s.enabled {
        refresh_hints_soon();
        download_automatically();
    }
    current()
}

/// Start the picked engine's model download if [`auto_download`] says so.
fn download_automatically() {
    let s = current();
    if let Some(id) = auto_download(&s, |id| declined_now(id, &s)) {
        log::info!("voice typing: fetching the {} engine's speech model", id.as_str());
        start_download(id);
    }
}

/// Pick the engine (Settings) and save it. A whisper.cpp engine is loaded on the next hold that
/// uses it, and its model starts downloading now if it isn't here (picking it is asking for it);
/// picking Basic lets the loaded model go now.
pub fn set_engine(choice: EngineChoice) -> VoiceStatus {
    save(SETTING_ENGINE, choice.as_str());
    log::info!("voice typing: {} engine picked", choice.as_str());
    update(|s| {
        s.engine = choice;
        s.accurate_problem = None;
    });
    if let Some(id) = choice.model() {
        set_declined(id, false);
        if current().enabled {
            download_automatically();
        }
    }
    // With no Windows speech language, the engine decides whether anything can listen at all.
    resettle();
    current()
}

/// The language Most accurate listens in ("auto" or one of [`engine::SPOKEN_LANGUAGES`]; anything
/// else is "auto"), saved. The loaded engine is let go and loads again in the new language.
pub fn set_spoken_language(language: &str) -> VoiceStatus {
    let language = engine::spoken_language(Some(language));
    save(SETTING_SPOKEN_LANGUAGE, language);
    log::info!("voice typing: most accurate listens in {language}");
    update(|s| s.spoken_language = language.into())
}

/// Whether contacts' names are hints, saved; the hints are read again straight away.
pub fn set_contact_hints(on: bool) -> VoiceStatus {
    save(SETTING_CONTACT_HINTS, if on { "true" } else { "false" });
    let s = update(|s| s.contact_hints = on);
    refresh_hints();
    s
}

/// The personal dictionary, as saved.
pub fn dictionary() -> Vec<String> {
    parse_dictionary(saved(SETTING_DICTIONARY).as_deref())
}

/// Save the personal dictionary (tidied: [`vocab::normalize_dictionary`]) and use it from the next
/// hold. Returns it as saved. The words aren't logged.
pub fn set_dictionary(words: &[String]) -> Vec<String> {
    let words = vocab::normalize_dictionary(words);
    save(
        SETTING_DICTIONARY,
        &serde_json::to_string(&words).unwrap_or_else(|_| "[]".into()),
    );
    refresh_hints();
    words
}

/// The "Get voice typing ready now?" card has been shown: never offer it by itself again.
pub fn ready_prompt_shown() -> VoiceStatus {
    save(SETTING_READY_PROMPT, "shown");
    update(|s| s.ready_prompt = false)
}

/// Download (or resume) a model because the person asked (Download, Try again, the card): it
/// counts as wanting it, so it's fetched automatically again later if it's lost.
pub fn download_model(id: ModelId) -> VoiceStatus {
    set_declined(id, false);
    start_download(id)
}

/// Start downloading a model (or resume a paused download) on a thread of its own. Progress
/// arrives as status events; once the model is checked, its engine takes over from the next hold.
/// A second call while that model downloads does nothing; the other model can download at the
/// same time.
fn start_download(id: ModelId) -> VoiceStatus {
    let i = id.index();
    let spec = id.spec();
    let Some(dir) = MODEL_DIR.get().cloned() else {
        return update(|s| {
            s.model_of_mut(id).error = Some("tug couldn't find its data folder to save the speech model in.".into())
        });
    };
    if !current().accurate_supported {
        return current();
    }
    if DOWNLOADING[i].swap(true, Ordering::SeqCst) {
        return current();
    }
    CANCEL_DOWNLOAD[i].store(false, Ordering::SeqCst);
    let started = update(|s| {
        let m = s.model_of_mut(id);
        m.state = ModelState::Downloading;
        m.received = model::partial(&dir, spec);
        m.error = None;
    });
    log::info!(
        "voice typing: downloading the {} speech model ({} of {} bytes already here)",
        id.as_str(),
        started.model_of(id).received,
        spec.size
    );
    let spawned = std::thread::Builder::new()
        .name("tug-voice-model".into())
        .spawn(move || {
            let t0 = std::time::Instant::now();
            let mut sent = 0u64;
            let step = spec.size / 100;
            let result = model::HttpFetch::new().and_then(|mut fetch| {
                model::download(&mut fetch, spec, &dir, &CANCEL_DOWNLOAD[i], &mut |got, total| {
                    if model::progress_due(sent, got, total, step) {
                        sent = got;
                        update(|s| s.model_of_mut(id).received = got);
                    }
                })
            });
            match &result {
                Ok(_) => log::info!(
                    "voice typing: {} speech model downloaded and checked in {} s",
                    id.as_str(),
                    t0.elapsed().as_secs()
                ),
                Err(e) => log::warn!(
                    "voice typing: {} speech model download stopped: {}",
                    id.as_str(),
                    e.detail()
                ),
            }
            let ok = result.is_ok();
            update(|s| {
                *s.model_of_mut(id) = model::status_on_disk(&dir, spec);
                s.model_of_mut(id).error = result.err().and_then(|e| e.message()).map(str::to_string);
                if ok && s.engine.model() == Some(id) {
                    s.accurate_problem = None;
                }
            });
            // Only now: a Download clicked earlier would have its status overwritten by this one.
            DOWNLOADING[i].store(false, Ordering::SeqCst);
            // On a PC with no Windows speech language, the model is what lets voice typing listen:
            // register the shortcut now rather than leave it unavailable.
            resettle();
        });
    if let Err(e) = spawned {
        DOWNLOADING[i].store(false, Ordering::SeqCst);
        log::warn!("voice typing: the download thread didn't start ({e})");
        return update(|s| {
            let m = s.model_of_mut(id);
            m.state = ModelState::Missing;
            m.error = Some("tug couldn't start the download. Try again.".into());
        });
    }
    current()
}

/// Stop a model's running download and delete what it had (Cancel). The person said no: it isn't
/// started again by itself ([`declined`]) until they ask for it.
pub fn cancel_download(id: ModelId) -> VoiceStatus {
    set_declined(id, true);
    if DOWNLOADING[id.index()].load(Ordering::SeqCst) {
        log::info!("voice typing: {} speech model download cancelled", id.as_str());
        CANCEL_DOWNLOAD[id.index()].store(true, Ordering::SeqCst);
    }
    current()
}

/// Delete every downloaded model and paused download (Settings' "Delete downloaded models"):
/// running downloads are cancelled (they delete their own part files), neither model is fetched
/// again by itself, and whatever listened with a model goes back to Basic.
pub fn delete_models() -> VoiceStatus {
    let Some(dir) = MODEL_DIR.get() else {
        return current();
    };
    for id in ModelId::ALL {
        set_declined(id, true);
        CANCEL_DOWNLOAD[id.index()].store(true, Ordering::SeqCst);
        model::remove(dir, id.spec());
    }
    log::info!("voice typing: speech models deleted");
    update(|s| {
        for id in ModelId::ALL {
            let downloading = DOWNLOADING[id.index()].load(Ordering::SeqCst);
            let m = s.model_of_mut(id);
            *m = model_status_now(id);
            if downloading {
                // Its thread is stopping; it says so when it has.
                m.state = ModelState::Downloading;
            }
        }
        s.accurate_problem = None;
    });
    resettle();
    current()
}

/// A model failed its check or didn't load (called from the speech thread): delete it, say so,
/// and go back to something smaller until it's downloaded again.
#[cfg_attr(not(windows), allow(dead_code))]
fn model_damaged(id: ModelId, why: &str) {
    log::warn!(
        "voice typing: the {} speech model is damaged ({why}); deleting it",
        id.as_str()
    );
    if let Some(dir) = MODEL_DIR.get() {
        model::remove(dir, id.spec());
    }
    update(|s| {
        let m = s.model_of_mut(id);
        *m = model_status_now(id);
        m.error = Some("The speech model was damaged, so tug deleted it. Download it again.".into());
    });
    // Without Windows speech, nothing can listen now: say so and let the shortcut go. Off this
    // (speech) thread, since settling waits for it.
    resettle_soon();
}

/// What happened when the speech thread last tried to load the accurate engine: `None` loaded,
/// or why it didn't. A failure to start keeps Basic listening (and settles again, in case nothing
/// else can listen); low memory only affects that one hold.
#[cfg_attr(not(windows), allow(dead_code))]
fn note_accurate(problem: Option<AccurateProblem>) {
    let before = current().accurate_problem;
    if before == problem {
        return;
    }
    update(|s| s.accurate_problem = problem);
    if problem == Some(AccurateProblem::Failed) || before == Some(AccurateProblem::Failed) {
        resettle_soon();
    }
}

/// Note what the default microphone is (from the speech thread), so Settings can say when it's a
/// phone-call headset.
#[cfg_attr(not(windows), allow(dead_code))]
fn note_phone_mic(phone: bool) {
    if current().phone_mic != phone {
        log::info!(
            "voice typing: the default microphone is {}a phone-call headset",
            if phone { "" } else { "not " }
        );
        update(|s| s.phone_mic = phone);
    }
}

/// The installed languages, listed once the first time they're needed. Settings calls this
/// before voice typing is on, so the language list is there to choose from. Also takes a fresh
/// look at the default microphone and the model on disk.
pub fn refresh_languages() -> VoiceStatus {
    if let Some(phone) = check_phone_mic() {
        note_phone_mic(phone);
    }
    for id in ModelId::ALL {
        if !DOWNLOADING[id.index()].load(Ordering::SeqCst) {
            // The model on disk now (deleted by hand, or a paused download), keeping any message.
            let on_disk = model_status_now(id);
            update(|s| {
                let m = s.model_of_mut(id);
                m.state = on_disk.state;
                m.received = on_disk.received;
            });
        }
    }
    let s = status();
    if s.languages.is_empty() {
        let languages = installed_languages().unwrap_or_else(|e| {
            log::warn!("voice typing: couldn't list the speech languages: {}", e.detail());
            Vec::new()
        });
        let language = pick_language(s.language.as_deref(), &languages, ui_language());
        update(|s| {
            s.languages = languages;
            s.language = language;
        });
    }
    // If what can listen no longer matches the state (the model was deleted by hand, say), settle
    // again so the shortcut and Settings agree.
    resettle();
    current()
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
fn set_engine_config(config: EngineConfig) {
    win::configure(config);
}
#[cfg(not(windows))]
fn set_engine_config(_config: EngineConfig) {}

/// Whether the default microphone is a phone-call headset, asked on the speech thread (COM).
#[cfg(windows)]
fn check_phone_mic() -> Option<bool> {
    win::phone_mic()
}
#[cfg(not(windows))]
fn check_phone_mic() -> Option<bool> {
    None
}

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
            engine: EngineChoice::MostAccurate,
            active_engine: Some(EngineChoice::Basic),
            model: ModelStatus {
                state: ModelState::Downloading,
                received: 10,
                total: 20,
                error: None,
            },
            most_accurate_model: ModelStatus::new(ModelState::Missing, 0, 30),
            spoken_language: "auto".into(),
            contact_hints: true,
            accurate_supported: true,
            ready_prompt: false,
            phone_mic: true,
            accurate_problem: Some(AccurateProblem::LowMemoryUsedAccurate),
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"enabled":true,"state":"inUse","keys":"ctrl+alt+space","language":"409","languages":[{"id":"409","name":"English (US)"}],"micBlocked":false,"startError":null,"engine":"mostAccurate","activeEngine":"basic","model":{"state":"downloading","received":10,"total":20,"error":null},"mostAccurateModel":{"state":"missing","received":0,"total":30,"error":null},"spokenLanguage":"auto","contactHints":true,"accurateSupported":true,"readyPrompt":false,"phoneMic":true,"accurateProblem":"lowMemoryUsedAccurate"}"#
        );
    }

    fn on_status(engine: EngineChoice, model: ModelState, language: Option<&str>) -> VoiceStatus {
        let mut s = default_status();
        s.enabled = true;
        s.state = VoiceState::On;
        s.engine = engine;
        s.model.state = model;
        s.accurate_supported = true;
        s.language = language.map(str::to_string);
        s
    }

    #[test]
    fn the_speech_thread_gets_the_engine_the_status_says() {
        let file = Path::new("C:/models/ggml.bin");
        let files: ModelFiles = [Some(file), None];
        let c = engine_config(
            &on_status(EngineChoice::Accurate, ModelState::Ready, Some("409")),
            files,
        );
        assert_eq!(c.engine, Some(EngineChoice::Accurate));
        assert_eq!(c.model.as_deref(), Some(file));
        assert_eq!(c.spoken, "en");
        assert_eq!(c.fallback, None);
        assert_eq!(
            c.language.as_deref(),
            Some("409"),
            "Basic stays available as the fallback"
        );

        let c = engine_config(
            &on_status(EngineChoice::Accurate, ModelState::Missing, Some("409")),
            [None, None],
        );
        assert_eq!(c.engine, Some(EngineChoice::Basic), "no model yet");
        assert_eq!(c.model, None);

        let c = engine_config(
            &on_status(EngineChoice::Accurate, ModelState::Downloading, Some("409")),
            [None, None],
        );
        assert_eq!(c.engine, Some(EngineChoice::Basic), "still downloading");

        let c = engine_config(&on_status(EngineChoice::Basic, ModelState::Ready, Some("409")), files);
        assert_eq!(c.engine, Some(EngineChoice::Basic));
        assert_eq!(c.model, None, "Basic picked: the model isn't loaded");

        let mut off = on_status(EngineChoice::Accurate, ModelState::Ready, Some("409"));
        off.state = VoiceState::Off;
        assert_eq!(engine_config(&off, files), EngineConfig::default(), "off: all let go");
        let mut busy = on_status(EngineChoice::Accurate, ModelState::Ready, Some("409"));
        busy.state = VoiceState::InUse;
        assert_eq!(
            engine_config(&busy, files),
            EngineConfig::default(),
            "no shortcut: nothing loaded"
        );
    }

    #[test]
    fn most_accurate_gets_its_model_its_language_and_accurate_to_fall_back_on() {
        let small = Path::new("C:/models/base.bin");
        let big = Path::new("C:/models/turbo.bin");
        let mut s = on_status(EngineChoice::MostAccurate, ModelState::Ready, Some("409"));
        s.most_accurate_model.state = ModelState::Ready;
        s.spoken_language = "de".into();
        let c = engine_config(&s, [Some(small), Some(big)]);
        assert_eq!(c.engine, Some(EngineChoice::MostAccurate));
        assert_eq!(c.model.as_deref(), Some(big));
        assert_eq!(c.fallback.as_deref(), Some(small), "Accurate, if memory is short");
        assert_eq!(c.spoken, "de");
        // A spoken language tug doesn't offer is "auto".
        s.spoken_language = "xx".into();
        assert_eq!(engine_config(&s, [Some(small), Some(big)]).spoken, "auto");
        // German Windows speech: Accurate (English-only) can't stand in.
        s.language = Some("407".into());
        let c = engine_config(&s, [Some(small), Some(big)]);
        assert_eq!(c.engine, Some(EngineChoice::MostAccurate));
        assert_eq!(c.fallback, None);
        // Most accurate's model still downloading: Accurate listens, in English.
        let mut s = on_status(EngineChoice::MostAccurate, ModelState::Ready, Some("409"));
        s.most_accurate_model.state = ModelState::Downloading;
        let c = engine_config(&s, [Some(small), None]);
        assert_eq!(c.engine, Some(EngineChoice::Accurate));
        assert_eq!(c.model.as_deref(), Some(small));
        assert_eq!(c.spoken, "en");
    }

    #[test]
    fn turning_it_on_fetches_the_picked_engines_model_unless_declined() {
        let never = |_| false;
        let s = on_status(EngineChoice::Accurate, ModelState::Missing, Some("409"));
        assert_eq!(auto_download(&s, never), Some(ModelId::Accurate));
        let s = on_status(EngineChoice::MostAccurate, ModelState::Missing, Some("407"));
        assert_eq!(
            auto_download(&s, never),
            Some(ModelId::MostAccurate),
            "any language: it's multilingual"
        );
        // A paused download resumes.
        let mut paused = on_status(EngineChoice::Accurate, ModelState::Missing, Some("409"));
        paused.model.received = 1000;
        assert_eq!(auto_download(&paused, never), Some(ModelId::Accurate));
        // Not when...
        let s = on_status(EngineChoice::Accurate, ModelState::Missing, Some("409"));
        assert_eq!(auto_download(&s, |_| true), None, "the person cancelled it before");
        let mut off = s.clone();
        off.enabled = false;
        assert_eq!(auto_download(&off, never), None, "voice typing is off");
        let mut slow = s.clone();
        slow.accurate_supported = false;
        assert_eq!(auto_download(&slow, never), None, "this PC can't run it");
        for state in [ModelState::Ready, ModelState::Downloading] {
            let here = on_status(EngineChoice::Accurate, state, Some("409"));
            assert_eq!(auto_download(&here, never), None, "{state:?}");
        }
        assert_eq!(
            auto_download(&on_status(EngineChoice::Basic, ModelState::Missing, Some("409")), never),
            None
        );
        assert_eq!(
            auto_download(
                &on_status(EngineChoice::Accurate, ModelState::Missing, Some("40c")),
                never
            ),
            None,
            "Accurate is English-only: no point fetching it for French"
        );
        assert_eq!(
            auto_download(&on_status(EngineChoice::Accurate, ModelState::Missing, None), never),
            Some(ModelId::Accurate),
            "no Windows speech at all: the model is what can listen"
        );
    }

    #[test]
    fn a_cancel_or_an_old_not_now_is_remembered() {
        assert!(declined(ModelId::Accurate, Some("true"), false, true));
        assert!(
            !declined(ModelId::Accurate, Some("false"), true, true),
            "asked for since"
        );
        assert!(!declined(ModelId::Accurate, None, false, true));
        // v0.5.19's "Not now" with nothing downloaded since counts as no.
        assert!(declined(ModelId::Accurate, None, true, true));
        assert!(
            !declined(ModelId::Accurate, None, true, false),
            "it was downloaded after all"
        );
        assert!(
            !declined(ModelId::MostAccurate, None, true, true),
            "that offer was Accurate's"
        );
        assert!(declined(ModelId::MostAccurate, Some("true"), false, true));
    }

    #[test]
    fn the_ready_card_is_offered_once_to_someone_who_never_set_it_up() {
        assert!(ready_prompt_due(false, false, false, true));
        assert!(
            !ready_prompt_due(true, false, false, true),
            "voice typing was switched on or off before"
        );
        assert!(!ready_prompt_due(false, true, false, true), "shown before: never again");
        assert!(!ready_prompt_due(false, false, true, true), "a model is already here");
        assert!(!ready_prompt_due(false, false, false, false), "this PC can't run it");
    }

    #[test]
    fn the_dictionary_reads_back_tidied() {
        assert_eq!(
            parse_dictionary(Some(r#"["Zephyrine"," kubernetes ","zephyrine",""]"#)),
            ["Zephyrine", "kubernetes"]
        );
        assert!(parse_dictionary(None).is_empty());
        assert!(parse_dictionary(Some("not json")).is_empty());
        assert!(parse_dictionary(Some("{}")).is_empty());
    }

    /// The state `apply(true, …)` settles on for this status (what `resettle` re-runs), with the
    /// shortcut free to register.
    fn settled_state(s: &VoiceStatus) -> VoiceState {
        let can_listen = engine::pick_engine(&engine_inputs(s)).is_some();
        settle(true, can_listen, Ok(can_listen)).state
    }

    #[test]
    fn downloading_the_model_with_no_windows_speech_turns_voice_typing_on() {
        // No Windows speech language, no model: on, but nothing can listen.
        let mut s = on_status(EngineChoice::Accurate, ModelState::Missing, None);
        s.state = settled_state(&s);
        assert_eq!(s.state, VoiceState::Unavailable);
        assert!(!needs_resettle(&s), "nothing changed yet");
        // The download finishes: the model can listen, so it settles again, and comes out on.
        s.model.state = ModelState::Ready;
        assert!(needs_resettle(&s), "unavailable, but the model can listen now");
        s.state = settled_state(&s);
        assert_eq!(s.state, VoiceState::On);
        assert!(!needs_resettle(&s), "settled");
    }

    #[test]
    fn picking_basic_with_no_windows_speech_says_nothing_can_listen() {
        let mut s = on_status(EngineChoice::Accurate, ModelState::Ready, None);
        s.state = settled_state(&s);
        assert_eq!(s.state, VoiceState::On, "the model listens");
        // Basic picked: the shortcut mustn't stay registered with nothing behind it.
        s.engine = EngineChoice::Basic;
        assert!(needs_resettle(&s));
        s.state = settled_state(&s);
        assert_eq!(s.state, VoiceState::Unavailable);
        // The same when the model is lost instead.
        let mut s = on_status(EngineChoice::Accurate, ModelState::Ready, None);
        s.model.state = ModelState::Missing;
        assert!(needs_resettle(&s), "the model was deleted");
        assert_eq!(settled_state(&s), VoiceState::Unavailable);
    }

    #[test]
    fn nothing_to_settle_when_off_or_when_windows_speech_listens_anyway() {
        let mut off = on_status(EngineChoice::Accurate, ModelState::Ready, None);
        off.enabled = false;
        off.state = VoiceState::Off;
        assert!(!needs_resettle(&off));
        // With a Windows speech language, Basic always can: the model arriving changes the engine,
        // not whether voice typing is on.
        let mut s = on_status(EngineChoice::Accurate, ModelState::Missing, Some("409"));
        assert!(!needs_resettle(&s));
        s.model.state = ModelState::Ready;
        assert!(!needs_resettle(&s));
        s.state = VoiceState::InUse;
        assert!(
            !needs_resettle(&s),
            "another app has the shortcut: that's not a settle problem"
        );
    }

    #[test]
    fn a_phone_call_headset_is_recognised() {
        assert!(phone_call_mic("Headset (WH-1000XM4 Hands-Free AG Audio)", 16_000));
        assert!(phone_call_mic("Headset Microphone (Example Buds Hands-Free)", 0));
        assert!(phone_call_mic("Headset (Example Handsfree)", 48_000));
        assert!(phone_call_mic("Microphone (USB Audio)", 8_000), "8 kHz is a phone line");
        assert!(!phone_call_mic("Microphone Array (Realtek(R) Audio)", 48_000));
        assert!(
            !phone_call_mic("Headset (WH-1000XM4)", 48_000),
            "the same headset in its stereo mode"
        );
        assert!(!phone_call_mic("", 0), "unknown: no note");
    }
}
