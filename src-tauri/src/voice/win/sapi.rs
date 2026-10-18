//! The engine: Windows' desktop speech recognizer (SAPI 5's in-process recognizer with its
//! dictation grammar). It's the "Microsoft Speech Recognizer" Windows installs with each speech
//! language, and it runs entirely on the PC.
//!
//! Each hold's recording is handed over as an in-memory stream (never a file), recognized to the
//! end of the stream, and the words collected. The recognizer, its context and the dictation
//! grammar are kept between holds (loading dictation takes a moment); any failure drops them and
//! the next hold starts fresh.
//!
//! Must be used on a COM thread (the voice worker initialises COM).

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::{w, IUnknown, Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Media::Audio::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use windows::Win32::Media::Speech::{
    IEnumSpObjectTokens, ISpObjectToken, ISpObjectTokenCategory, ISpRecoContext, ISpRecoGrammar, ISpRecoResult,
    ISpRecognizer, ISpStream, SpInprocRecognizer, SpObjectTokenCategory, SpStream, SPCAT_RECOGNIZERS,
    SPEI_END_SR_STREAM, SPEI_FALSE_RECOGNITION, SPEI_RECOGNITION, SPET_LPARAM_IS_OBJECT, SPET_LPARAM_IS_POINTER,
    SPET_LPARAM_IS_STRING, SPET_LPARAM_IS_TOKEN, SPEVENT, SPEVENTENUM, SPLO_STATIC, SPRST_ACTIVE, SPRST_INACTIVE,
    SPRS_ACTIVE, SPRS_INACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IStream, CLSCTX_ALL};
use windows::Win32::UI::Shell::SHCreateMemStream;

use crate::voice::engine::{Engine, EngineError, SAMPLE_RATE};

/// SPDFID_WaveFormatEx: the stream's format is the WAVEFORMATEX that comes with it.
const SPDFID_WAVE_FORMAT_EX: GUID = GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);
/// SP_GETWHOLEPHRASE: every element of the phrase.
const SP_GETWHOLEPHRASE: u32 = u32::MAX;
/// SPFEI_FLAGCHECK: the two reserved bits every SAPI event-interest mask carries.
const SPFEI_FLAGCHECK: u64 = (1 << 30) | (1 << 33);
/// How long recognition may take beyond the recording's own length.
const SLACK: Duration = Duration::from_secs(10);

fn spfei(e: SPEVENTENUM) -> u64 {
    (1u64 << e.0) | SPFEI_FLAGCHECK
}

fn failed(what: &str, e: windows::core::Error) -> EngineError {
    EngineError::Failed(format!("{what}: {e}"))
}

/// The format of every recording: 16 kHz, mono, 16-bit PCM.
pub fn wave_format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    }
}

/// A COM string SAPI allocated, copied out and freed.
///
/// SAFETY: `p` must be null or a CoTaskMemAlloc'd, NUL-terminated UTF-16 string.
unsafe fn take_string(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

/// The recognizers Windows lists: `(description, language attribute)` each.
fn recognizer_tokens() -> windows::core::Result<Vec<(ISpObjectToken, String, String)>> {
    // SAFETY: COM calls on a COM thread; every out-pointer is valid, and strings SAPI returns are
    // freed by `take_string`.
    unsafe {
        let category: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        category.SetId(SPCAT_RECOGNIZERS, false)?;
        let tokens: IEnumSpObjectTokens = category.EnumTokens(PCWSTR::null(), PCWSTR::null())?;
        let mut out = Vec::new();
        loop {
            let mut token: [Option<ISpObjectToken>; 1] = [None];
            let mut fetched = 0u32;
            if tokens.Next(1, token.as_mut_ptr(), Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(token) = token[0].take() else { break };
            let description = token
                .GetStringValue(PCWSTR::null())
                .ok()
                .and_then(|p| take_string(p))
                .unwrap_or_default();
            let language = token
                .OpenKey(w!("Attributes"))
                .and_then(|k| k.GetStringValue(w!("Language")))
                .ok()
                .and_then(|p| take_string(p))
                .unwrap_or_default();
            out.push((token, description, language));
        }
        Ok(out)
    }
}

/// The installed speech languages: `(description, language attribute)`, or what SAPI said when it
/// couldn't list them.
pub fn languages() -> Result<Vec<(String, String)>, String> {
    recognizer_tokens()
        .map(|tokens| tokens.into_iter().map(|(_, d, l)| (d, l)).collect())
        .map_err(|e| format!("couldn't list speech recognizers: {e}"))
}

/// An in-memory stream SAPI reads a recording from (or writes speech into, for the test).
fn memory_stream(pcm: &[u8]) -> Result<(ISpStream, IStream), EngineError> {
    // SAFETY: COM calls on a COM thread; `wave_format()` outlives the call that copies it.
    unsafe {
        let base = SHCreateMemStream(Some(pcm)).ok_or_else(|| EngineError::Failed("no memory stream".into()))?;
        let stream: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL).map_err(|e| failed("stream", e))?;
        let format = wave_format();
        stream
            .SetBaseStream(&base, &SPDFID_WAVE_FORMAT_EX, &format)
            .map_err(|e| failed("stream format", e))?;
        Ok((stream, base))
    }
}

pub struct Sapi {
    language: String,
    recognizer: ISpRecognizer,
    context: ISpRecoContext,
    grammar: ISpRecoGrammar,
}

impl Sapi {
    /// A recognizer for `language` (an id like "409"), ready for its first recording.
    pub fn new(language: &str) -> Result<Self, EngineError> {
        let tokens = recognizer_tokens().map_err(|e| failed("recognizers", e))?;
        let token = tokens
            .into_iter()
            .find(|(_, _, attr)| crate::voice::language_id(attr).as_deref() == Some(language))
            .map(|(t, _, _)| t)
            .ok_or(EngineError::Unavailable)?;
        // SAFETY: COM calls on a COM thread with valid arguments.
        unsafe {
            let recognizer: ISpRecognizer =
                CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL).map_err(|e| failed("recognizer", e))?;
            recognizer.SetRecognizer(&token).map_err(|e| failed("engine", e))?;
            // The in-process recognizer wants an input before it makes a context: a moment of silence.
            let (silence, _base) = memory_stream(&[0u8; 3200])?;
            recognizer.SetInput(&silence, true).map_err(|e| failed("input", e))?;
            let context = recognizer.CreateRecoContext().map_err(|e| failed("context", e))?;
            context.SetNotifyWin32Event().map_err(|e| failed("notify", e))?;
            let interest = spfei(SPEI_RECOGNITION) | spfei(SPEI_FALSE_RECOGNITION) | spfei(SPEI_END_SR_STREAM);
            context
                .SetInterest(interest, interest)
                .map_err(|e| failed("interest", e))?;
            let grammar = context.CreateGrammar(0).map_err(|e| failed("grammar", e))?;
            grammar
                .LoadDictation(PCWSTR::null(), SPLO_STATIC)
                .map_err(|e| failed("dictation", e))?;
            Ok(Self {
                language: language.to_string(),
                recognizer,
                context,
                grammar,
            })
        }
    }

    /// Take one event off the queue, returning the recognized text if it's a recognition, and
    /// release whatever SAPI attached to it. `(event id, text)`.
    ///
    /// SAFETY: `ev` must have just come from `GetEvents` and not been released.
    unsafe fn consume(ev: &SPEVENT) -> (i32, Option<String>) {
        let id = ev._bitfield & 0xffff;
        let kind = (ev._bitfield >> 16) & 0xffff;
        let p = ev.lParam.0 as *mut c_void;
        if p.is_null() {
            return (id, None);
        }
        let mut text = None;
        if kind == SPET_LPARAM_IS_OBJECT.0 || kind == SPET_LPARAM_IS_TOKEN.0 {
            // We own the reference the event carried; dropping it releases it.
            let unknown = IUnknown::from_raw(p);
            if id == SPEI_RECOGNITION.0 {
                if let Ok(result) = unknown.cast::<ISpRecoResult>() {
                    let mut words = PWSTR::null();
                    if result.GetText(0, SP_GETWHOLEPHRASE, true, &mut words, None).is_ok() {
                        text = take_string(words);
                    }
                }
            }
        } else if kind == SPET_LPARAM_IS_POINTER.0 || kind == SPET_LPARAM_IS_STRING.0 {
            CoTaskMemFree(Some(p as *const c_void));
        }
        (id, text)
    }
}

impl Engine for Sapi {
    fn language(&self) -> &str {
        &self.language
    }

    fn transcribe(&mut self, pcm: &[i16]) -> Result<String, EngineError> {
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let (stream, _base) = memory_stream(&bytes)?;
        let audio = Duration::from_millis(pcm.len() as u64 * 1000 / u64::from(SAMPLE_RATE));
        let deadline = Instant::now() + audio + SLACK;
        let mut heard: Vec<String> = Vec::new();
        // SAFETY: COM calls on a COM thread; each event is consumed (and released) exactly once.
        let ended = unsafe {
            let _ = self.recognizer.SetRecoState(SPRST_INACTIVE);
            self.recognizer
                .SetInput(&stream, true)
                .map_err(|e| failed("input", e))?;
            self.grammar
                .SetDictationState(SPRS_ACTIVE)
                .map_err(|e| failed("dictation on", e))?;
            self.recognizer
                .SetRecoState(SPRST_ACTIVE)
                .map_err(|e| failed("start", e))?;
            let mut ended = false;
            while !ended && Instant::now() < deadline {
                let _ = self.context.WaitForNotifyEvent(100);
                loop {
                    let mut ev = SPEVENT::default();
                    let mut got = 0u32;
                    if self.context.GetEvents(1, &mut ev, &mut got).is_err() || got == 0 {
                        break;
                    }
                    let (id, text) = Self::consume(&ev);
                    heard.extend(text.filter(|t| !t.trim().is_empty()));
                    if id == SPEI_END_SR_STREAM.0 {
                        ended = true;
                    }
                }
            }
            let _ = self.grammar.SetDictationState(SPRS_INACTIVE);
            let _ = self.recognizer.SetRecoState(SPRST_INACTIVE);
            ended
        };
        if !ended {
            return Err(EngineError::Failed(format!(
                "recognition didn't finish within {} s",
                (audio + SLACK).as_secs()
            )));
        }
        Ok(heard.join(" "))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPCAT_VOICES};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED, STREAM_SEEK_SET};

    /// Speak `text` with Windows' text-to-speech into 16 kHz PCM, as a stand-in for a microphone.
    /// (The Whisper comparison in `whisper.rs` uses it too.)
    pub(crate) fn speak(text: &str) -> Vec<i16> {
        speak_with(text, None)
    }

    /// Speak `text` with a voice for `language` (an id like "40c", French), or `None` when this PC
    /// has no voice for it (Windows' English voices would read it with English sounds, which is a
    /// test of nothing).
    pub(crate) fn speak_in(text: &str, language: &str) -> Option<Vec<i16>> {
        // SAFETY: COM calls on a COM thread with valid arguments.
        let token = unsafe {
            let category: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL).ok()?;
            category.SetId(SPCAT_VOICES, false).ok()?;
            let wanted: Vec<u16> = format!("Language={language}").encode_utf16().chain([0]).collect();
            let tokens: IEnumSpObjectTokens = category.EnumTokens(PCWSTR(wanted.as_ptr()), PCWSTR::null()).ok()?;
            let mut token: [Option<ISpObjectToken>; 1] = [None];
            let mut fetched = 0u32;
            tokens.Next(1, token.as_mut_ptr(), Some(&mut fetched)).ok()?;
            token[0].take()?
        };
        Some(speak_with(text, Some(&token)))
    }

    fn speak_with(text: &str, voice_token: Option<&ISpObjectToken>) -> Vec<i16> {
        unsafe {
            let (stream, base) = memory_stream(&[]).unwrap();
            let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL).unwrap();
            if let Some(t) = voice_token {
                voice.SetVoice(t).unwrap();
            }
            voice.SetOutput(&stream, true).unwrap();
            let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
            voice.Speak(PCWSTR(wide.as_ptr()), 0, None).unwrap();
            base.Seek(0, STREAM_SEEK_SET, None).unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let mut read = 0u32;
                let _ = base.Read(buf.as_mut_ptr() as *mut c_void, buf.len() as u32, Some(&mut read));
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..read as usize]);
            }
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect()
        }
    }

    /// Recognizes speech end to end on this PC, with text-to-speech standing in for a voice.
    /// Needs Windows' English (US) speech recognizer and a voice, so it's run by hand:
    /// `cargo test --manifest-path src-tauri/Cargo.toml voice::win::sapi -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn recognizes_spoken_words_offline() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let langs = languages().expect("recognizers listed");
        println!("recognizers: {langs:?}");
        let mut sapi = Sapi::new("409").expect("English (US) recognizer");
        for phrase in [
            "hello world",
            "open the file please",
            "print open paren quote hello quote close paren",
            "const camel case user id equals five semicolon",
            "hello comma how are you question mark new line thanks period",
            "self dot name",
        ] {
            let pcm = speak(phrase);
            assert!(pcm.len() > 8000, "speech was synthesized");
            let started = Instant::now();
            let heard = sapi.transcribe(&pcm).expect("recognized");
            println!(
                "said {phrase:?} → heard {heard:?} ({} ms of audio, {} ms to recognize)",
                pcm.len() / 16,
                started.elapsed().as_millis()
            );
            assert!(!heard.trim().is_empty());
            println!(
                "   as code: {:?}   as prose: {:?}",
                crate::voice::text::format(&heard, crate::voice::text::target_for_process("code.exe")),
                crate::voice::text::format(&heard, crate::voice::text::target_for_process("notepad.exe"))
            );
        }
        // Silence: nothing, not an error.
        let heard = sapi.transcribe(&vec![0i16; 16000]).expect("silence is fine");
        println!("silence → {heard:?}");
    }
}
