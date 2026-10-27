//! The speech engine seam. Voice typing records one hold into memory and hands it to an
//! [`Engine`]; everything around it (shortcut, microphone, text rules, typing) doesn't care which
//! engine it is.
//!
//! Three engines, all on the PC:
//!
//! - **Most accurate**: whisper.cpp (`win/whisper.rs`) with the multilingual large-v3-turbo
//!   model (834 MB, downloaded once: `model.rs`). Understands languages other than English;
//!   much slower (seconds a phrase) and needs the most memory.
//! - **Accurate** (the default): whisper.cpp with an English speech model (78 MB, downloaded once).
//!   Much better than Basic with short commands and code words.
//! - **Basic**: Windows' desktop speech recognizer (SAPI 5, `win/sapi.rs`). Ships with Windows,
//!   any language Windows has speech for, nothing to download. The fallback whenever the others
//!   can't run: no model yet, a damaged model, a processor without AVX2, too little memory, or a
//!   language other than English for Accurate.
//!
//! Which one a hold uses is [`pick_engine`]'s decision, and which model to load given the free
//! memory is [`plan_load`]'s, both made from plain values so they're tested. Engines must keep
//! audio on the PC: no network, no files.

use serde::{Deserialize, Serialize};

use crate::voice::model::ModelId;

/// The sample rate every engine is given: 16 kHz, mono, 16-bit, what speech recognizers prefer.
pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum EngineError {
    /// No recognizer for the language on this PC.
    Unavailable,
    /// The engine failed (with a reason for the log, never the words).
    Failed(String),
}

/// Turns one recording into the words heard.
#[cfg_attr(not(windows), allow(dead_code))]
pub trait Engine {
    /// The language this engine listens in (an id from `VoiceLanguage`, or "en" for Accurate).
    fn language(&self) -> &str;
    /// Recognize `pcm` (16 kHz mono 16-bit). An empty string means nothing was recognized.
    fn transcribe(&mut self, pcm: &[i16]) -> Result<String, EngineError>;
}

/// The engine the person picked in Settings (`ui.voiceEngine`). Mirrored in
/// `src/types/protocol.ts` (`VoiceEngine`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineChoice {
    /// whisper.cpp with the English model.
    Accurate,
    /// whisper.cpp with the multilingual large-v3-turbo model.
    MostAccurate,
    /// Windows' own recognizer.
    Basic,
}

impl EngineChoice {
    /// The saved value, or Accurate (the default: it's used as soon as its model is there).
    pub fn parse(saved: Option<&str>) -> EngineChoice {
        match saved.map(str::trim) {
            Some("basic") => EngineChoice::Basic,
            Some("mostAccurate") => EngineChoice::MostAccurate,
            _ => EngineChoice::Accurate,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EngineChoice::Accurate => "accurate",
            EngineChoice::MostAccurate => "mostAccurate",
            EngineChoice::Basic => "basic",
        }
    }

    /// The speech model this engine runs, if it's a whisper.cpp one.
    pub fn model(self) -> Option<ModelId> {
        match self {
            EngineChoice::Accurate => Some(ModelId::Accurate),
            EngineChoice::MostAccurate => Some(ModelId::MostAccurate),
            EngineChoice::Basic => None,
        }
    }
}

/// Why the whisper.cpp engine picked isn't listening even though it could. Mirrored in
/// `src/types/protocol.ts` (`VoiceStatus.accurateProblem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccurateProblem {
    /// whisper.cpp couldn't start with the (checked) model on this PC. The file is kept and
    /// Basic listens until voice typing is switched on again, the engine is picked again or the
    /// model is downloaded again: it isn't retried on every hold.
    Failed,
    /// Too little free memory to load it on the last hold: that hold used Basic, and the next
    /// one tries again.
    LowMemory,
    /// Too little free memory for Most accurate on the last hold, so Accurate (its model is
    /// downloaded and smaller) listened instead; the next hold tries Most accurate again.
    LowMemoryUsedAccurate,
}

/// The free memory tug wants before loading Accurate. Loaded, it adds about 240 MB to tug's
/// working set (measured with the 78 MB model: 30 MB before, 272 MB after a few holds, 32 MB again
/// once let go; `compares_whisper_with_windows_recognizer` prints it); twice that leaves room for
/// everything else on the PC.
pub const MIN_FREE_MEMORY: u64 = 512 * 1024 * 1024;

/// The free memory tug wants before loading Most accurate. Loading holds the file (834 MB) and
/// the model built from it at once, about 1.75 GB above tug's own at the peak, then settles at
/// about 1.3 GB more than before (measured with the full 30 s window it uses:
/// `compares_whisper_with_windows_recognizer` prints it); twice that, like Accurate, leaves room
/// for everything else on the PC.
pub const MIN_FREE_MEMORY_MOST: u64 = 2560 * 1024 * 1024;

/// The free memory a model needs before it's loaded.
pub fn memory_needed(model: ModelId) -> u64 {
    match model {
        ModelId::Accurate => MIN_FREE_MEMORY,
        ModelId::MostAccurate => MIN_FREE_MEMORY_MOST,
    }
}

/// Whether there's memory to load `model` (`None`: Windows didn't say; go ahead).
pub fn enough_memory_for(model: ModelId, available: Option<u64>) -> bool {
    available.is_none_or(|a| a >= memory_needed(model))
}

/// What to load for a hold, given the free memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadPlan {
    /// The model to load, or `None`: this hold uses Basic.
    pub model: Option<ModelId>,
    /// What Settings should say about it (`None`: the model wanted is the one loaded).
    pub problem: Option<AccurateProblem>,
}

/// Which model to load when `wanted` isn't loaded yet: it, if there's memory for it; for Most
/// accurate with too little, Accurate if its model is downloaded (`accurate_ready`) and there's
/// memory for that; otherwise none, and the hold uses Basic. Settings says which happened.
pub fn plan_load(wanted: ModelId, available: Option<u64>, accurate_ready: bool) -> LoadPlan {
    if enough_memory_for(wanted, available) {
        return LoadPlan {
            model: Some(wanted),
            problem: None,
        };
    }
    if wanted == ModelId::MostAccurate && accurate_ready && enough_memory_for(ModelId::Accurate, available) {
        return LoadPlan {
            model: Some(ModelId::Accurate),
            problem: Some(AccurateProblem::LowMemoryUsedAccurate),
        };
    }
    LoadPlan {
        model: None,
        problem: Some(AccurateProblem::LowMemory),
    }
}

/// The languages Most accurate can be told to listen in (Whisper's codes), after "auto"
/// (detect it from the speech). Mirrors `src/lib/voiceSpokenLanguages.json`, which has their
/// names (a test checks they agree). Whisper knows more; these are the ones it's good at.
pub const SPOKEN_LANGUAGES: &[&str] = &[
    "auto", "en", "es", "fr", "de", "it", "pt", "nl", "pl", "sv", "da", "no", "fi", "cs", "sk", "hu", "ro", "el", "tr",
    "ru", "uk", "ar", "he", "hi", "id", "ms", "vi", "th", "ja", "ko", "zh",
];

/// The saved spoken language for Most accurate, or "auto" if it isn't one tug offers. A
/// `'static` str, as whisper.cpp is handed it.
pub fn spoken_language(saved: Option<&str>) -> &'static str {
    saved
        .map(str::trim)
        .and_then(|s| SPOKEN_LANGUAGES.iter().copied().find(|l| *l == s))
        .unwrap_or("auto")
}

/// Most accurate's language before the person picks one: Windows' display language (a LANGID,
/// 0x409), when it's one tug offers, else "auto". Detecting the language runs the model's encoder
/// a second time for every phrase (measured: 15 s instead of 6 s for a first hold on the test PC),
/// so a known language is the faster default; the person can still pick "Detect automatically".
pub fn default_spoken_language(ui_language: u16) -> &'static str {
    let code = match ui_language & 0x3ff {
        0x01 => "ar",
        0x04 => "zh",
        0x05 => "cs",
        0x06 => "da",
        0x07 => "de",
        0x08 => "el",
        0x09 => "en",
        0x0a => "es",
        0x0b => "fi",
        0x0c => "fr",
        0x0d => "he",
        0x0e => "hu",
        0x10 => "it",
        0x11 => "ja",
        0x12 => "ko",
        0x13 => "nl",
        0x14 => "no",
        0x15 => "pl",
        0x16 => "pt",
        0x18 => "ro",
        0x19 => "ru",
        0x1b => "sk",
        0x1d => "sv",
        0x1e => "th",
        0x1f => "tr",
        0x21 => "id",
        0x22 => "uk",
        0x2a => "vi",
        0x39 => "hi",
        0x3e => "ms",
        _ => "auto",
    };
    spoken_language(Some(code))
}

/// Whether the prompt can be the English commands and coding words: Accurate always, Most
/// accurate unless another language is picked.
pub fn prompt_in_english(language: &str) -> bool {
    matches!(language, "en" | "auto")
}

/// Whether a language id from Windows (hex, "409" is English (US), "809" English (UK)) is English.
/// The accurate engine's model is English-only.
pub fn is_english(language: &str) -> bool {
    u16::from_str_radix(language, 16).is_ok_and(|id| id & 0x3ff == 0x09)
}

/// What decides which engine listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineInputs<'a> {
    pub choice: EngineChoice,
    /// Accurate's model is downloaded and its size is right (a damaged one is found on loading).
    pub model_ready: bool,
    /// Most accurate's model is downloaded and its size is right.
    pub most_ready: bool,
    /// This processor can run whisper.cpp ([`cpu_supported`]).
    pub cpu_ok: bool,
    /// Windows' recognizer language, if Windows has one ("409").
    pub sapi_language: Option<&'a str>,
    /// whisper.cpp failed to start with the picked engine's model on this PC
    /// ([`AccurateProblem::Failed`]).
    pub accurate_failed: bool,
}

/// Whether Accurate can listen here, whatever was picked: its model is downloaded, the processor
/// can run it, it hasn't failed to start here, and the language is English (or Windows has no
/// speech language at all, so there's nothing else to listen in).
pub fn accurate_usable(i: &EngineInputs) -> bool {
    i.model_ready && i.cpu_ok && !i.accurate_failed && i.sapi_language.is_none_or(is_english)
}

/// Whether Most accurate can listen here: its model is downloaded, the processor can run it and
/// it hasn't failed to start. Any language: the model is multilingual.
pub fn most_accurate_usable(i: &EngineInputs) -> bool {
    i.most_ready && i.cpu_ok && !i.accurate_failed
}

/// Which engine a hold uses, or `None` when nothing can listen (`VoiceOutcome::Unavailable`):
///
/// - Most accurate picked and usable → Most accurate.
/// - Most accurate picked but its model isn't there yet (downloading) → Accurate if it's usable,
///   so a person who had Accurate keeps it while the bigger model downloads.
/// - Accurate picked and usable → Accurate.
/// - Otherwise Windows' recognizer, if Windows has a speech language.
/// - Nothing else can listen and Windows has no speech language → nothing. (Basic picked with no
///   Windows speech language is nothing too, even with a model downloaded: the person asked for
///   Windows' recognizer, and Settings says what's missing.)
pub fn pick_engine(i: &EngineInputs) -> Option<EngineChoice> {
    match i.choice {
        EngineChoice::MostAccurate if most_accurate_usable(i) => return Some(EngineChoice::MostAccurate),
        EngineChoice::MostAccurate if !i.most_ready && accurate_usable(i) => return Some(EngineChoice::Accurate),
        EngineChoice::Accurate if accurate_usable(i) => return Some(EngineChoice::Accurate),
        _ => {}
    }
    i.sapi_language.is_some().then_some(EngineChoice::Basic)
}

/// Whether this processor has what tug's whisper.cpp build is compiled for: AVX, AVX2, FMA, F16C
/// and BMI2 (`.cargo/config.toml`). Every Intel CPU since 2013 (Haswell) and every AMD Ryzen has
/// them; some Pentium, Celeron and Atom chips don't, and there whisper.cpp would crash tug with
/// an illegal instruction, so it's never called and Basic listens instead.
pub fn cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx")
            && std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
            && std::arch::is_x86_feature_detected!("f16c")
            && std::arch::is_x86_feature_detected!("bmi2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EngineChoice::*;

    fn inputs(choice: EngineChoice, model_ready: bool, cpu_ok: bool, sapi: Option<&str>) -> EngineInputs<'_> {
        EngineInputs {
            choice,
            model_ready,
            most_ready: false,
            cpu_ok,
            sapi_language: sapi,
            accurate_failed: false,
        }
    }

    fn most(most_ready: bool, accurate_ready: bool, sapi: Option<&str>) -> EngineInputs<'_> {
        EngineInputs {
            most_ready,
            ..inputs(MostAccurate, accurate_ready, true, sapi)
        }
    }

    #[test]
    fn most_accurate_once_its_model_is_there_in_any_language() {
        assert_eq!(pick_engine(&most(true, false, Some("409"))), Some(MostAccurate));
        assert_eq!(
            pick_engine(&most(true, false, Some("407"))),
            Some(MostAccurate),
            "German: it's multilingual"
        );
        assert_eq!(
            pick_engine(&most(true, false, None)),
            Some(MostAccurate),
            "no Windows speech needed"
        );
        let slow = EngineInputs {
            cpu_ok: false,
            ..most(true, true, Some("409"))
        };
        assert_eq!(pick_engine(&slow), Some(Basic), "a processor without AVX2");
        let failed = EngineInputs {
            accurate_failed: true,
            ..most(true, true, Some("409"))
        };
        assert_eq!(pick_engine(&failed), Some(Basic));
    }

    #[test]
    fn accurate_listens_while_most_accurate_downloads() {
        assert_eq!(pick_engine(&most(false, true, Some("409"))), Some(Accurate));
        assert_eq!(
            pick_engine(&most(false, false, Some("409"))),
            Some(Basic),
            "neither model yet"
        );
        assert_eq!(
            pick_engine(&most(false, true, Some("407"))),
            Some(Basic),
            "Accurate is English-only"
        );
        assert_eq!(pick_engine(&most(false, false, None)), None, "nothing can listen");
        assert!(most_accurate_usable(&most(true, false, None)));
        assert!(!most_accurate_usable(&most(false, true, None)));
    }

    #[test]
    fn the_saved_choice_reads_most_accurate() {
        assert_eq!(EngineChoice::parse(Some("mostAccurate")), MostAccurate);
        assert_eq!(MostAccurate.as_str(), "mostAccurate");
        assert_eq!(serde_json::to_string(&MostAccurate).unwrap(), r#""mostAccurate""#);
        assert_eq!(MostAccurate.model(), Some(ModelId::MostAccurate));
        assert_eq!(Accurate.model(), Some(ModelId::Accurate));
        assert_eq!(Basic.model(), None);
    }

    const GB: u64 = 1 << 30;

    #[test]
    fn short_memory_falls_back_to_accurate_then_basic() {
        let plan = |wanted, free: Option<u64>, accurate_ready| plan_load(wanted, free, accurate_ready);
        // Plenty: what was asked for.
        assert_eq!(
            plan(ModelId::MostAccurate, Some(8 * GB), true),
            LoadPlan {
                model: Some(ModelId::MostAccurate),
                problem: None
            }
        );
        assert_eq!(
            plan(ModelId::MostAccurate, None, false).model,
            Some(ModelId::MostAccurate),
            "Windows didn't say"
        );
        // Not enough for Most accurate, enough for Accurate, which is downloaded.
        assert_eq!(
            plan(ModelId::MostAccurate, Some(GB), true),
            LoadPlan {
                model: Some(ModelId::Accurate),
                problem: Some(AccurateProblem::LowMemoryUsedAccurate)
            }
        );
        // ... but Accurate isn't downloaded: Basic.
        assert_eq!(
            plan(ModelId::MostAccurate, Some(GB), false),
            LoadPlan {
                model: None,
                problem: Some(AccurateProblem::LowMemory)
            }
        );
        // Not enough for either.
        assert_eq!(plan(ModelId::MostAccurate, Some(100 << 20), true).model, None);
        assert_eq!(plan(ModelId::Accurate, Some(GB), false).model, Some(ModelId::Accurate));
        assert_eq!(
            plan(ModelId::Accurate, Some(100 << 20), true).problem,
            Some(AccurateProblem::LowMemory)
        );
        assert!(memory_needed(ModelId::MostAccurate) > memory_needed(ModelId::Accurate));
        assert!(enough_memory_for(ModelId::MostAccurate, Some(MIN_FREE_MEMORY_MOST)));
        assert!(!enough_memory_for(
            ModelId::MostAccurate,
            Some(MIN_FREE_MEMORY_MOST - 1)
        ));
        assert_eq!(
            serde_json::to_string(&AccurateProblem::LowMemoryUsedAccurate).unwrap(),
            r#""lowMemoryUsedAccurate""#
        );
    }

    #[test]
    fn spoken_languages_agree_with_the_settings_list() {
        let raw = include_str!("../../../src/lib/voiceSpokenLanguages.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let codes: Vec<&str> = v["languages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, SPOKEN_LANGUAGES);
        assert_eq!(SPOKEN_LANGUAGES[0], "auto");
        for code in SPOKEN_LANGUAGES {
            assert_eq!(spoken_language(Some(code)), *code);
            // Every one is a language whisper.cpp knows (or "auto").
            assert!(*code == "auto" || whisper_lang_known(code), "{code}");
        }
        assert_eq!(spoken_language(None), "auto");
        assert_eq!(spoken_language(Some("klingon")), "auto");
        // Before one is picked: Windows' display language, any region.
        assert_eq!(default_spoken_language(0x409), "en");
        assert_eq!(default_spoken_language(0x809), "en", "English (UK)");
        assert_eq!(default_spoken_language(0x407), "de");
        assert_eq!(default_spoken_language(0xc0a), "es", "Spanish (Spain, modern)");
        assert_eq!(default_spoken_language(0x804), "zh");
        assert_eq!(
            default_spoken_language(0x427),
            "auto",
            "Lithuanian: not offered, so detected"
        );
        assert_eq!(default_spoken_language(0), "auto");
        // Every language Windows can map to is one tug offers (never a code whisper.cpp isn't told).
        for id in 0u16..0x400 {
            assert!(SPOKEN_LANGUAGES.contains(&default_spoken_language(id)), "{id:x}");
        }
        assert!(prompt_in_english("en") && prompt_in_english("auto") && !prompt_in_english("de"));
    }

    /// Whisper's own language list (whisper.cpp's `g_lang`), the codes tug offers among them.
    fn whisper_lang_known(code: &str) -> bool {
        const WHISPER: &[&str] = &[
            "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it", "id", "hi",
            "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur", "hr", "bg", "lt", "la",
            "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn", "et", "mk", "br", "eu", "is", "hy",
        ];
        WHISPER.contains(&code)
    }

    #[test]
    fn basic_after_the_accurate_engine_failed_to_start() {
        let failed = EngineInputs {
            accurate_failed: true,
            ..inputs(Accurate, true, true, Some("409"))
        };
        assert_eq!(pick_engine(&failed), Some(Basic));
        let alone = EngineInputs {
            accurate_failed: true,
            ..inputs(Accurate, true, true, None)
        };
        assert_eq!(pick_engine(&alone), None, "and nothing, without Windows speech");
    }

    #[test]
    fn needs_free_memory_to_load() {
        assert!(
            enough_memory_for(ModelId::Accurate, None),
            "Windows didn't say: go ahead"
        );
        assert!(enough_memory_for(ModelId::Accurate, Some(4 << 30)));
        assert!(enough_memory_for(ModelId::Accurate, Some(MIN_FREE_MEMORY)));
        assert!(!enough_memory_for(ModelId::Accurate, Some(MIN_FREE_MEMORY - 1)));
        assert!(!enough_memory_for(ModelId::Accurate, Some(100 << 20)));
        assert_eq!(
            serde_json::to_string(&AccurateProblem::LowMemory).unwrap(),
            r#""lowMemory""#
        );
    }

    #[test]
    fn the_saved_choice_defaults_to_accurate() {
        assert_eq!(EngineChoice::parse(None), Accurate);
        assert_eq!(EngineChoice::parse(Some("")), Accurate);
        assert_eq!(EngineChoice::parse(Some("accurate")), Accurate);
        assert_eq!(EngineChoice::parse(Some("basic")), Basic);
        assert_eq!(EngineChoice::parse(Some("whisper")), Accurate, "unknown: the default");
        assert_eq!(EngineChoice::parse(Some("mostaccurate")), Accurate, "exactly as saved");
        assert_eq!(Basic.as_str(), "basic");
        assert_eq!(serde_json::to_string(&Accurate).unwrap(), r#""accurate""#);
        assert_eq!(serde_json::from_str::<EngineChoice>(r#""basic""#).unwrap(), Basic);
    }

    #[test]
    fn english_is_any_english_region() {
        assert!(is_english("409"));
        assert!(is_english("809"), "English (UK)");
        assert!(is_english("c09"), "English (Australia)");
        assert!(!is_english("407"), "German");
        assert!(!is_english("40c"), "French");
        assert!(!is_english(""));
        assert!(!is_english("en-US"));
    }

    #[test]
    fn accurate_once_the_model_is_there() {
        assert_eq!(pick_engine(&inputs(Accurate, true, true, Some("409"))), Some(Accurate));
        assert_eq!(pick_engine(&inputs(Accurate, true, true, Some("809"))), Some(Accurate));
    }

    #[test]
    fn basic_until_the_model_is_downloaded() {
        assert_eq!(
            pick_engine(&inputs(Accurate, false, true, Some("409"))),
            Some(Basic),
            "no model yet: Windows' recognizer"
        );
    }

    #[test]
    fn basic_when_accurate_cant_run_here() {
        assert_eq!(
            pick_engine(&inputs(Accurate, true, false, Some("409"))),
            Some(Basic),
            "a processor without AVX2"
        );
        assert_eq!(
            pick_engine(&inputs(Accurate, true, true, Some("407"))),
            Some(Basic),
            "German: the model is English-only"
        );
    }

    #[test]
    fn basic_when_picked_even_with_the_model() {
        assert_eq!(pick_engine(&inputs(Basic, true, true, Some("409"))), Some(Basic));
        assert_eq!(
            pick_engine(&inputs(Basic, true, true, None)),
            None,
            "Basic picked, no Windows speech language: nothing listens (Settings says why)"
        );
    }

    #[test]
    fn accurate_listens_even_without_a_windows_speech_language() {
        assert_eq!(pick_engine(&inputs(Accurate, true, true, None)), Some(Accurate));
        assert_eq!(
            pick_engine(&inputs(Accurate, false, true, None)),
            None,
            "nothing downloaded and no Windows speech: nothing can listen"
        );
        assert!(accurate_usable(&inputs(Basic, true, true, None)));
        assert!(!accurate_usable(&inputs(Accurate, true, true, Some("411"))));
    }

    #[test]
    fn this_build_machine_can_say_what_it_supports() {
        // Not asserted either way (CI runners and laptops differ); it just mustn't panic.
        let _ = cpu_supported();
    }
}
