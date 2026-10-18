//! The speech engine seam. Voice typing records one hold into memory and hands it to an
//! [`Engine`]; everything around it (shortcut, microphone, text rules, typing) doesn't care which
//! engine it is.
//!
//! Two engines, both on the PC:
//!
//! - **Accurate**: whisper.cpp (`win/whisper.rs`) with an English speech model downloaded once on
//!   request (`model.rs`). Much better with short commands and code words.
//! - **Basic**: Windows' desktop speech recognizer (SAPI 5, `win/sapi.rs`). Ships with Windows,
//!   any language Windows has speech for, nothing to download. The fallback whenever Accurate
//!   can't run: no model yet, a damaged model, a processor without AVX2, or a language other
//!   than English.
//!
//! Which one a hold uses is [`pick_engine`]'s decision, made from plain values so it's tested.
//! Engines must keep audio on the PC: no network, no files.

use serde::{Deserialize, Serialize};

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
    /// whisper.cpp with the downloaded model.
    Accurate,
    /// Windows' own recognizer.
    Basic,
}

impl EngineChoice {
    /// The saved value, or Accurate (the default: it's used as soon as the model is there).
    pub fn parse(saved: Option<&str>) -> EngineChoice {
        match saved.map(str::trim) {
            Some("basic") => EngineChoice::Basic,
            _ => EngineChoice::Accurate,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EngineChoice::Accurate => "accurate",
            EngineChoice::Basic => "basic",
        }
    }
}

/// Why the accurate engine isn't listening even though it could. Mirrored in
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
}

/// The free memory tug wants before loading the accurate engine. Loaded, it adds about
/// 240 MB to tug's working set (measured with the 78 MB model: 30 MB before, 272 MB after a few
/// holds, 32 MB again once let go; `compares_whisper_with_windows_recognizer` prints it); twice
/// that leaves room for everything else on the PC.
pub const MIN_FREE_MEMORY: u64 = 512 * 1024 * 1024;

/// Whether there's memory to load the accurate engine (`None`: Windows didn't say; go ahead).
pub fn enough_memory(available: Option<u64>) -> bool {
    available.is_none_or(|a| a >= MIN_FREE_MEMORY)
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
    /// The speech model is downloaded and its size is right (a damaged one is found on loading).
    pub model_ready: bool,
    /// This processor can run whisper.cpp ([`cpu_supported`]).
    pub cpu_ok: bool,
    /// Windows' recognizer language, if Windows has one ("409").
    pub sapi_language: Option<&'a str>,
    /// whisper.cpp failed to start with the model on this PC ([`AccurateProblem::Failed`]).
    pub accurate_failed: bool,
}

/// Whether the accurate engine can listen here, whatever was picked: the model is downloaded, the
/// processor can run it, it hasn't failed to start here, and the language is English (or Windows
/// has no speech language at all, so there's nothing else to listen in).
pub fn accurate_usable(i: &EngineInputs) -> bool {
    i.model_ready && i.cpu_ok && !i.accurate_failed && i.sapi_language.is_none_or(is_english)
}

/// Which engine a hold uses, or `None` when nothing can listen (`VoiceOutcome::Unavailable`):
///
/// - Accurate picked and usable → Accurate.
/// - Otherwise Windows' recognizer, if Windows has a speech language.
/// - Accurate picked but Windows has no speech language: Accurate if it's usable (that's covered
///   by the first rule), else nothing.
/// - Basic picked and Windows has no speech language → nothing, even with the model downloaded:
///   the person asked for Windows' recognizer, and Settings says what's missing.
pub fn pick_engine(i: &EngineInputs) -> Option<EngineChoice> {
    if i.choice == EngineChoice::Accurate && accurate_usable(i) {
        Some(EngineChoice::Accurate)
    } else if i.sapi_language.is_some() {
        Some(EngineChoice::Basic)
    } else {
        None
    }
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
            cpu_ok,
            sapi_language: sapi,
            accurate_failed: false,
        }
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
        assert!(enough_memory(None), "Windows didn't say: go ahead");
        assert!(enough_memory(Some(4 << 30)));
        assert!(enough_memory(Some(MIN_FREE_MEMORY)));
        assert!(!enough_memory(Some(MIN_FREE_MEMORY - 1)));
        assert!(!enough_memory(Some(100 << 20)));
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
