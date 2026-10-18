//! The accurate engine: whisper.cpp (through `whisper-rs`), on the CPU, with the English model
//! from `voice/model.rs`. Nothing leaves the PC: whisper.cpp reads the model file once and works
//! on the recording in memory.
//!
//! - Loaded on the first hold that needs it (in parallel with the recording, see `win/mod.rs`),
//!   kept while voice typing is on, dropped when it's switched off or Basic is picked.
//! - Each hold: the recording is trimmed, levelled and padded (`voice/audio.rs`), decoded
//!   greedily as one short English segment with the vocabulary prompt (`voice/vocab.rs`), and the
//!   result tidied (`vocab::clean`) for `text::format`.
//! - The encoder looks at a 15 s window (more for a long hold, see [`audio_ctx`]) instead of
//!   Whisper's full 30 s: a short phrase is encoded in about half the time, with no loss of
//!   accuracy on the phrases measured.
//! - whisper.cpp's own logging is switched off for good: in debug builds it can print the tokens
//!   it decodes, and voice typing never logs what was said.

use std::sync::Once;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use crate::voice::audio;
use crate::voice::engine::{Engine, EngineError, SAMPLE_RATE};
use crate::voice::vocab;

/// The most prompt tokens whisper.cpp uses (half its 448-token text context).
const MAX_PROMPT_TOKENS: usize = 224;
/// Encoder positions per second of audio (whisper's mel frames are 10 ms, halved by the encoder).
const POSITIONS_PER_SECOND: usize = 50;
/// The full window, 30 s.
const FULL_AUDIO_CTX: usize = 1500;

static QUIET: Once = Once::new();

/// Send whisper.cpp's and ggml's log lines nowhere (whisper-rs's hooks with no logging backend).
fn quiet_logs() {
    QUIET.call_once(whisper_rs::install_logging_hooks);
}

/// How many threads recognition uses: the physical cores (about half the logical processors on
/// a PC with hyper-threading), at most 8; more only adds contention for a short phrase.
pub fn threads() -> i32 {
    let logical = std::thread::available_parallelism().map_or(4, |n| n.get());
    (logical / 2).clamp(1, 8) as i32
}

/// The smallest encoder window: 15.4 s of Whisper's 30. Measured on the test phrases
/// (`compares_whisper_with_windows_recognizer`): windows sized to just the audio made the base
/// model repeat itself and drop words, while 768 was as accurate as the full window in under half
/// the time.
const MIN_WINDOW: usize = 768;

/// The encoder window for `samples` of 16 kHz audio: [`MIN_WINDOW`], or more for a long hold (what's
/// there plus a little, in multiples of 64), never more than Whisper's 30 s.
pub fn audio_ctx(samples: usize) -> i32 {
    let positions = samples * POSITIONS_PER_SECOND / SAMPLE_RATE as usize + 64;
    (positions.div_ceil(64) * 64).clamp(MIN_WINDOW, FULL_AUDIO_CTX) as i32
}

/// The most tokens one hold may produce: about eight a second of audio plus a few. Speech runs at
/// three or four, so this only stops a decode stuck repeating itself from taking seconds.
pub fn max_tokens(samples: usize) -> i32 {
    (16 + samples * 8 / SAMPLE_RATE as usize).min(220) as i32
}

/// Settings a test can change; the defaults are what tug uses.
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    pub threads: i32,
    /// The encoder window: `None` for just the audio there is ([`audio_ctx`]), or a fixed one
    /// (1500 is Whisper's full 30 s).
    pub window: Option<i32>,
    pub prompt: bool,
    pub flash_attn: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            threads: threads(),
            window: None,
            prompt: true,
            flash_attn: false,
        }
    }
}

pub struct Whisper {
    state: WhisperState,
    prompt_tokens: Vec<i32>,
    tuning: Tuning,
    // Last: dropped after the state that came from it.
    _ctx: WhisperContext,
}

fn failed(what: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Failed(format!("{what}: {e}"))
}

impl Whisper {
    /// Load the model from its bytes (already checked against its hash by the caller, on these
    /// very bytes, so nothing can change the file between the check and the load).
    pub fn load(bytes: &[u8]) -> Result<Self, EngineError> {
        Self::load_tuned(bytes, Tuning::default())
    }

    pub fn load_tuned(bytes: &[u8], tuning: Tuning) -> Result<Self, EngineError> {
        quiet_logs();
        let mut params = WhisperContextParameters::default();
        // No GPU backend is compiled in; say so rather than rely on the default.
        params.use_gpu(false);
        params.flash_attn(tuning.flash_attn);
        let ctx =
            WhisperContext::new_from_buffer_with_params(bytes, params).map_err(|e| failed("loading the model", e))?;
        let state = ctx.create_state().map_err(|e| failed("the model's state", e))?;
        let prompt_tokens = if tuning.prompt {
            let mut t = ctx
                .tokenize(&vocab::prompt(), MAX_PROMPT_TOKENS * 2)
                .map_err(|e| failed("the prompt", e))?;
            // whisper.cpp keeps the end of an overlong prompt, and uses at most one less than
            // half its text context; keep the start (the examples and the commands), which matter
            // most. (The prompt is about 135 tokens today: `prompt_len` in the test report.)
            t.truncate(MAX_PROMPT_TOKENS - 1);
            t
        } else {
            Vec::new()
        };
        Ok(Whisper {
            state,
            prompt_tokens,
            tuning,
            _ctx: ctx,
        })
    }

    /// How many prompt tokens are in use (for the test report).
    #[cfg(test)]
    pub fn prompt_len(&self) -> usize {
        self.prompt_tokens.len()
    }
}

impl Engine for Whisper {
    fn language(&self) -> &str {
        "en"
    }

    fn transcribe(&mut self, pcm: &[i16]) -> Result<String, EngineError> {
        let speech = audio::trim_silence(pcm);
        let speech_secs = speech.len() as f32 / SAMPLE_RATE as f32;
        let samples = audio::prepare(pcm);
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_n_threads(self.tuning.threads);
        p.set_language(Some("en"));
        p.set_translate(false);
        p.set_detect_language(false);
        // Each hold stands alone: nothing carried over from the last one.
        p.set_no_context(true);
        p.set_single_segment(true);
        p.set_no_timestamps(true);
        p.set_print_special(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_suppress_blank(true);
        // No "[BLANK_AUDIO]" or "(music)" tokens.
        p.set_suppress_nst(true);
        p.set_temperature(0.0);
        p.set_audio_ctx(self.tuning.window.unwrap_or_else(|| audio_ctx(samples.len())));
        p.set_max_tokens(max_tokens(samples.len()));
        if !self.prompt_tokens.is_empty() {
            p.set_tokens(&self.prompt_tokens);
        }
        self.state.full(p, &samples).map_err(|e| failed("recognition", e))?;
        let mut heard = String::new();
        for segment in self.state.as_iter() {
            if let Ok(text) = segment.to_str_lossy() {
                heard.push_str(&text);
                heard.push(' ');
            }
        }
        let heard = vocab::clean(&heard);
        if vocab::is_prompt_echo(&heard, speech_secs) {
            // Whisper repeated its own prompt (a cough or a click it couldn't make out): nothing
            // was said that should be typed. Logged without the words.
            log::info!(
                "voice typing: dropped {} words that only repeated the prompt",
                heard.split_whitespace().count()
            );
            return Ok(String::new());
        }
        Ok(heard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use crate::voice::model;
    use crate::voice::text::{format, target_for_process};
    use crate::voice::win::sapi::{self, tests::speak};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    #[test]
    fn uses_physical_cores_and_a_window_that_fits_the_audio() {
        let t = threads();
        assert!((1..=8).contains(&t), "{t}");
        // A short phrase gets the smallest window that's still accurate.
        assert_eq!(audio_ctx(32_000), MIN_WINDOW as i32);
        assert_eq!(audio_ctx(audio::MIN_SAMPLES), MIN_WINDOW as i32);
        // A 20 s hold: 1000 positions + margin, a multiple of 64.
        let ctx = audio_ctx(16_000 * 20);
        assert_eq!(ctx % 64, 0);
        assert!((1064..=1152).contains(&ctx), "{ctx}");
        assert_eq!(audio_ctx(16_000 * 60), FULL_AUDIO_CTX as i32, "capped at 30 s");
        assert!(max_tokens(16_000 * 2) >= 24 && max_tokens(16_000 * 2) <= 40);
        assert_eq!(max_tokens(16_000 * 60), 220, "never past the text context");
    }

    /// The models to try: every `.bin` in `TUG_WHISPER_MODELS` (a folder), else the one tug
    /// downloads, where tug keeps it.
    fn models() -> Vec<PathBuf> {
        if let Ok(dir) = std::env::var("TUG_WHISPER_MODELS") {
            let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
                .map(|r| r.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
            v.retain(|p| p.extension().is_some_and(|e| e == "bin"));
            v.sort();
            return v;
        }
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let p = Path::new(&local)
            .join("dev.davejames.tug")
            .join(model::DIR)
            .join(model::MODEL.file);
        if p.exists() {
            vec![p]
        } else {
            Vec::new()
        }
    }

    /// The phrases from the issue (first three), then more code dictation; where each is typed
    /// and what should come out.
    const PHRASES: &[(&str, &str, &str)] = &[
        ("check logs", "code.exe", "check logs"),
        (
            "const camel case user id equals five semicolon",
            "code.exe",
            "const userId = 5;",
        ),
        (
            "hello comma how are you question mark",
            "notepad.exe",
            "Hello, how are you?",
        ),
        ("run the tests", "code.exe", "run the tests"),
        (
            "print open paren quote hello quote close paren",
            "code.exe",
            "print(\"hello\")",
        ),
        ("self dot name equals name", "code.exe", "self.name = name"),
        ("git push", "windowsterminal.exe", "git push"),
        ("snake case max retries equals three", "code.exe", "max_retries = 3"),
    ];

    /// Number words and digits count the same ("five" and "5" are both right).
    fn same_number(s: &str) -> String {
        s.replace("five", "5").replace("three", "3")
    }

    /// Character similarity, 0 to 1 (1 - edit distance / longer length).
    fn similarity(a: &str, b: &str) -> f64 {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.iter().enumerate() {
            let mut cur = vec![i + 1; b.len() + 1];
            for (j, cb) in b.iter().enumerate() {
                cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
            }
            prev = cur;
        }
        let longest = a.len().max(b.len()).max(1);
        1.0 - prev[b.len()] as f64 / longest as f64
    }

    #[test]
    fn similarity_scores() {
        assert_eq!(similarity("abc", "abc"), 1.0);
        assert_eq!(similarity("", ""), 1.0);
        assert!((similarity("check logs", "At Los") - 0.3).abs() < 0.11);
    }

    /// The same speech as a laptop microphone in a room might give it: quieter, with steady hiss.
    fn noisy(pcm: &[i16]) -> Vec<i16> {
        let mut seed = 0x2545_f491_u32;
        pcm.iter()
            .map(|&s| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let hiss = (seed % 1201) as i32 - 600;
                (i32::from(s) / 3 + hiss).clamp(-32768, 32767) as i16
            })
            .collect()
    }

    /// The same speech through a Bluetooth headset's hands-free link: 8 kHz, so nothing above
    /// 4 kHz (averaged pairs, each sample repeated).
    fn phone(pcm: &[i16]) -> Vec<i16> {
        pcm.chunks(2)
            .flat_map(|c| {
                let avg = (c.iter().map(|&s| i32::from(s)).sum::<i32>() / c.len() as i32) as i16;
                [avg; 2].into_iter().take(c.len())
            })
            .collect()
    }

    struct Row {
        engine: String,
        audio: &'static str,
        said: &'static str,
        heard: String,
        typed: String,
        right: bool,
        score: f64,
        ms: u128,
    }

    struct Sample {
        audio: &'static str,
        said: &'static str,
        exe: &'static str,
        want: &'static str,
        pcm: Vec<i16>,
    }

    fn row(engine: &str, s: &Sample, heard: String, ms: u128) -> Row {
        let typed = format(&heard, target_for_process(s.exe));
        let a = same_number(&typed);
        let b = same_number(s.want);
        Row {
            engine: engine.to_string(),
            audio: s.audio,
            said: s.said,
            right: a == b,
            score: similarity(&a, &b),
            heard,
            typed,
            ms,
        }
    }

    /// Whisper against Windows' recognizer on this PC, with Windows' text-to-speech standing in
    /// for a voice (like the SAPI test): the phrases voice typing gets wrong most, clean, with
    /// room noise, and through a hands-free headset. Prints what each heard, what would be typed,
    /// whether that's exactly right, a similarity score and how long recognition took. Needs a
    /// downloaded model (or `TUG_WHISPER_MODELS`, a folder of ggml .bin files), Windows' English
    /// speech recognizer and a voice, so it's run by hand:
    /// `cargo test --manifest-path src-tauri/Cargo.toml -p tug --lib voice::win::whisper -- --ignored --nocapture`
    /// `TUG_WHISPER_WINDOWS=256,512,768,full` compares encoder windows (see below).
    #[test]
    #[ignore]
    fn compares_whisper_with_windows_recognizer() {
        // SAFETY: COM for this test thread (SAPI and text-to-speech are COM).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let models = models();
        assert!(
            !models.is_empty(),
            "no model: download it in tug, or set TUG_WHISPER_MODELS to a folder of ggml .bin files"
        );
        let mut samples: Vec<Sample> = Vec::new();
        for kind in ["clean", "noisy", "phone"] {
            for (said, exe, want) in PHRASES {
                let clean = speak(said);
                let pcm = match kind {
                    "noisy" => noisy(&clean),
                    "phone" => phone(&clean),
                    _ => clean,
                };
                samples.push(Sample {
                    audio: kind,
                    said,
                    exe,
                    want,
                    pcm,
                });
            }
        }
        let mut rows = Vec::new();

        match sapi::Sapi::new("409") {
            Ok(mut s) => {
                for sample in &samples {
                    let t = Instant::now();
                    let heard = s.transcribe(&sample.pcm).unwrap_or_default();
                    rows.push(row("Basic (SAPI)", sample, heard, t.elapsed().as_millis()));
                }
            }
            Err(e) => println!("Windows' recognizer isn't available here: {e:?}"),
        }

        // `TUG_WHISPER_WINDOWS=256,512,768,full` tries those encoder windows ("tug" is tug's own
        // rule), each with this PC's thread count and with 4 threads (a typical laptop);
        // otherwise just tug's own settings.
        let mut variants: Vec<(String, Tuning)> = Vec::new();
        match std::env::var("TUG_WHISPER_WINDOWS") {
            Ok(list) => {
                for w in list.split(',').map(str::trim) {
                    let window = match w {
                        "tug" => None,
                        "full" => Some(FULL_AUDIO_CTX as i32),
                        n => Some(n.parse().expect("a window size, tug or full")),
                    };
                    for threads in [Tuning::default().threads, 4] {
                        variants.push((
                            format!("window {w}, {threads} threads"),
                            Tuning {
                                window,
                                threads,
                                ..Tuning::default()
                            },
                        ));
                    }
                }
            }
            Err(_) => variants.push(("default".into(), Tuning::default())),
        }

        for path in &models {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let mb = std::fs::metadata(path).map(|m| m.len() / 1_000_000).unwrap_or(0);
            for (label, tuning) in &variants {
                let before = working_set();
                let t = Instant::now();
                let bytes = std::fs::read(path).expect("model readable");
                let mut w = match Whisper::load_tuned(&bytes, *tuning) {
                    Ok(w) => w,
                    Err(e) => {
                        println!("{name}: didn't load: {e:?}");
                        continue;
                    }
                };
                let with_file = working_set();
                drop(bytes);
                let load_ms = t.elapsed().as_millis();
                // A first hold warms whisper.cpp's buffers; it's reported on its own, and the
                // rows below are the steady state from the second hold on.
                let t = Instant::now();
                let _ = w.transcribe(&samples[0].pcm);
                println!(
                    "{name} ({mb} MB, {label}): loaded in {load_ms} ms, first hold {} ms, {} threads, {} prompt tokens",
                    t.elapsed().as_millis(),
                    tuning.threads,
                    w.prompt_len()
                );
                for sample in &samples {
                    let t = Instant::now();
                    let heard = w.transcribe(&sample.pcm).expect("recognized");
                    rows.push(row(
                        &format!("Accurate {name} [{label}]"),
                        sample,
                        heard,
                        t.elapsed().as_millis(),
                    ));
                }
                let heard = w.transcribe(&noisy(&vec![0i16; 24_000])).expect("hiss is fine");
                println!("   hiss only (voice typing doesn't get this far: audio::has_speech) → {heard:?}");
                let (ws_before, ws_file, ws_loaded) = (before, with_file, working_set());
                println!(
                    "   memory: working set {} MB before loading, {} MB while the file and the model were both in memory, {} MB loaded after holds (+{} MB); peak {} MB",
                    ws_before.0 >> 20,
                    ws_file.0 >> 20,
                    ws_loaded.0 >> 20,
                    ws_loaded.0.saturating_sub(ws_before.0) >> 20,
                    ws_loaded.1 >> 20
                );
                drop(w);
                println!("   memory: {} MB after letting it go", working_set().0 >> 20);
            }
        }

        println!();
        println!("| engine | audio | said | heard | typed | right | similarity | ms |");
        println!("|---|---|---|---|---|---|---|---|");
        for r in &rows {
            println!(
                "| {} | {} | {} | {:?} | {:?} | {} | {:.2} | {} |",
                r.engine,
                r.audio,
                r.said,
                r.heard,
                r.typed,
                if r.right { "yes" } else { "no" },
                r.score,
                r.ms
            );
        }
        println!();
        let mut engines: Vec<&str> = rows.iter().map(|r| r.engine.as_str()).collect();
        engines.dedup();
        for e in engines {
            for kind in ["clean", "noisy", "phone"] {
                let mine: Vec<&Row> = rows.iter().filter(|r| r.engine == e && r.audio == kind).collect();
                if mine.is_empty() {
                    continue;
                }
                let right = mine.iter().filter(|r| r.right).count();
                let score = mine.iter().map(|r| r.score).sum::<f64>() / mine.len() as f64;
                let ms = mine.iter().map(|r| r.ms).sum::<u128>() / mine.len() as u128;
                let worst = mine.iter().map(|r| r.ms).max().unwrap_or(0);
                println!(
                    "{e} / {kind}: {right}/{} exactly right, similarity {score:.2}, mean {ms} ms, worst {worst} ms",
                    mine.len()
                );
            }
        }
    }

    /// This process's working set and its peak, in bytes.
    fn working_set() -> (u64, u64) {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut c = PROCESS_MEMORY_COUNTERS::default();
        // SAFETY: a read-only query about this process into a correctly sized struct.
        let ok = unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut c,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        if ok.is_err() {
            return (0, 0);
        }
        (c.WorkingSetSize as u64, c.PeakWorkingSetSize as u64)
    }

    /// Bytes that aren't a model (not one at all, or empty) fail to load cleanly: an error, never
    /// a crash, so tug can fall back to Windows' recognizer. (A file that is the right size but
    /// damaged never gets this far: its hash is checked first.)
    #[test]
    fn bytes_that_arent_a_model_fail_to_load_without_crashing() {
        let r = Whisper::load(&vec![0x5au8; 4096]);
        assert!(matches!(r, Err(EngineError::Failed(_))), "{:?}", r.as_ref().err());
        assert!(Whisper::load(&[0u8; 4]).is_err(), "too short to be anything");
    }
}
