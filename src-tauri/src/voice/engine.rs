//! The speech engine seam. Voice typing records one hold into memory and hands it to an
//! [`Engine`]; everything around it (shortcut, microphone, text rules, typing) doesn't care which
//! engine it is.
//!
//! Today's engine is Windows' desktop speech recognizer (SAPI 5, `win/sapi.rs`): it ships with
//! Windows, runs offline and needs nothing extra to build. A local Whisper engine (whisper.cpp)
//! would slot in here later, if tug can build it without new tools for contributors.
//!
//! Engines must keep audio on the PC: no network, no files.

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
    /// The language this engine listens in (an id from `VoiceLanguage`).
    fn language(&self) -> &str;
    /// Recognize `pcm` (16 kHz mono 16-bit). An empty string means nothing was recognized.
    fn transcribe(&mut self, pcm: &[i16]) -> Result<String, EngineError>;
}
