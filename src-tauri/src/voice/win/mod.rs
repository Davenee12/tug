//! The Windows side of voice typing. Two threads, both started the first time they're needed:
//!
//! - `tug-voice-key` owns the RegisterHotKey registration (hotkeys belong to the thread that
//!   registered them) and the indicator window, and pumps their messages. A press only notes
//!   which window is in front and hands the hold to the worker, so the queue never waits.
//! - `tug-voice` is a COM thread that runs each hold: microphone open while the key is down,
//!   recognize, shape the text, type it. It also owns the recognizer between holds.
//!
//! RegisterHotKey only reports the press, so the worker watches the shortcut's key with
//! GetAsyncKeyState to know when it's let go: no keyboard hook, nothing else is seen.
//!
//! The decisions live in `voice/mod.rs` and `voice/text.rs`; this only asks Windows and reports.

mod capture;
mod overlay;
mod sapi;
mod system;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG, PM_NOREMOVE, WM_APP,
    WM_HOTKEY,
};

use super::engine::{Engine, EngineError};
use super::{judge_recording, judge_typing, peak, text, VoiceLanguage, VoiceOutcome, MAX_HOLD, TAIL};
use crate::code_fill::Keys;
use overlay::Look;

const HOTKEY_ID: i32 = 1;
/// Posted to the key thread: a registration request is waiting.
const WM_REGISTER: u32 = WM_APP + 1;
/// How often the hold loop checks the key and collects audio.
const TICK: Duration = Duration::from_millis(10);
/// How long a problem stays on the indicator.
const PROBLEM_MS: u32 = 2600;

struct Request {
    keys: Option<Keys>,
    reply: mpsc::Sender<bool>,
}

struct KeyThread {
    id: u32,
    requests: Mutex<mpsc::Sender<Request>>,
}

/// A press of the shortcut, handed to the worker.
struct Press {
    keys: Keys,
    /// The window in front when it was pressed (and its program's path).
    target: Option<system::Foreground>,
}

enum Job {
    Hold(Press),
    Languages(mpsc::Sender<Vec<VoiceLanguage>>),
    Language(Option<String>),
}

static KEY_THREAD: OnceLock<KeyThread> = OnceLock::new();
static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
/// The registered shortcut, read by the key thread on a press.
static REGISTERED: Mutex<Option<Keys>> = Mutex::new(None);
/// One hold at a time: a press while one is being recognized or typed is ignored.
static BUSY: AtomicBool = AtomicBool::new(false);

fn worker() -> Option<&'static Mutex<mpsc::Sender<Job>>> {
    if let Some(w) = WORKER.get() {
        return Some(w);
    }
    let (tx, rx) = mpsc::channel::<Job>();
    let spawned = std::thread::Builder::new()
        .name("tug-voice".into())
        .spawn(move || run_worker(rx));
    if let Err(e) = spawned {
        log::warn!("voice typing unavailable: {e}");
        return None;
    }
    let _ = WORKER.set(Mutex::new(tx));
    WORKER.get()
}

fn send_job(job: Job) -> bool {
    worker().is_some_and(|w| w.lock().unwrap_or_else(|e| e.into_inner()).send(job).is_ok())
}

fn key_thread() -> Option<&'static KeyThread> {
    if let Some(t) = KEY_THREAD.get() {
        return Some(t);
    }
    let (ready_tx, ready_rx) = mpsc::channel::<u32>();
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let spawned = std::thread::Builder::new()
        .name("tug-voice-key".into())
        .spawn(move || run_keys(ready_tx, req_rx));
    if let Err(e) = spawned {
        log::warn!("voice typing shortcut unavailable: {e}");
        return None;
    }
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(id) => {
            let _ = KEY_THREAD.set(KeyThread {
                id,
                requests: Mutex::new(req_tx),
            });
        }
        Err(_) => log::warn!("voice typing shortcut thread didn't start"),
    }
    KEY_THREAD.get()
}

/// Register `keys` (replacing any earlier shortcut), or unregister with `None`. True when the
/// shortcut is now registered.
pub fn register(keys: Option<Keys>) -> bool {
    if keys.is_none() && KEY_THREAD.get().is_none() {
        // Never started: nothing to unregister.
        return false;
    }
    let Some(thread) = key_thread() else {
        return false;
    };
    let (reply, answer) = mpsc::channel();
    let sent = thread
        .requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(Request { keys, reply });
    if sent.is_err() {
        return false;
    }
    // SAFETY: posting a message with no pointers to a thread tug owns.
    if unsafe { PostThreadMessageW(thread.id, WM_REGISTER, WPARAM(0), LPARAM(0)) }.is_err() {
        return false;
    }
    let ok = answer.recv_timeout(Duration::from_secs(2)).unwrap_or(false);
    *REGISTERED.lock().unwrap_or_else(|e| e.into_inner()) = keys.filter(|_| ok);
    ok
}

/// The installed speech languages (listed on the COM thread).
pub fn languages() -> Vec<VoiceLanguage> {
    let (reply, answer) = mpsc::channel();
    if !send_job(Job::Languages(reply)) {
        return Vec::new();
    }
    answer.recv_timeout(Duration::from_secs(10)).unwrap_or_default()
}

/// The language to listen in from now on (`None` when voice typing is off: the recognizer is let go).
pub fn set_language(language: Option<String>) {
    if language.is_none() && WORKER.get().is_none() {
        return;
    }
    send_job(Job::Language(language));
}

pub fn mic_blocked() -> bool {
    system::mic_blocked()
}

pub fn ui_language() -> u16 {
    system::ui_language()
}

fn run_keys(ready: mpsc::Sender<u32>, requests: mpsc::Receiver<Request>) {
    let mut msg = MSG::default();
    // SAFETY: plain Win32 calls on this thread with a valid MSG buffer. The peek creates the
    // thread's message queue, so a registration posted right after `ready` isn't lost.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    overlay::create();
    // SAFETY: as above.
    let _ = ready.send(unsafe { GetCurrentThreadId() });
    loop {
        // SAFETY: as above; GetMessageW returns 0 on WM_QUIT and -1 on error.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        // Thread messages (no window): ours. Window messages: the indicator's.
        if msg.hwnd.is_invalid() {
            match msg.message {
                WM_HOTKEY if msg.wParam.0 == HOTKEY_ID as usize => on_press(),
                WM_REGISTER => {
                    while let Ok(req) = requests.try_recv() {
                        // SAFETY: the hotkey is registered to this thread (no window), id HOTKEY_ID.
                        unsafe {
                            let _ = UnregisterHotKey(None, HOTKEY_ID);
                        }
                        let ok = req.keys.is_some_and(|k| {
                            // SAFETY: as above.
                            unsafe { RegisterHotKey(None, HOTKEY_ID, HOT_KEY_MODIFIERS(k.modifiers()), k.vk()) }.is_ok()
                        });
                        let _ = req.reply.send(ok);
                    }
                }
                _ => {}
            }
        } else {
            // SAFETY: dispatching a message this thread's window received.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

fn on_press() {
    let Some(keys) = *REGISTERED.lock().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };
    if keys.alt {
        // Before anything else, while Alt is still down: letting go of it mustn't open a menu.
        system::mask_menu_key();
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let press = Press {
        keys,
        target: system::foreground(),
    };
    if !send_job(Job::Hold(press)) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn run_worker(jobs: mpsc::Receiver<Job>) {
    // SAFETY: initialising COM for this thread, which lives as long as tug.
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        log::warn!("voice typing: COM didn't start: {e}");
    }
    let mut engine: Option<sapi::Sapi> = None;
    let mut language: Option<String> = None;
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Hold(press) => {
                let outcome = hold(&press, &mut engine, language.as_deref());
                finish(outcome);
                BUSY.store(false, Ordering::SeqCst);
                super::emit_typed(outcome);
            }
            Job::Languages(reply) => {
                let list = sapi::languages()
                    .into_iter()
                    .filter_map(|(desc, attr)| {
                        super::language_id(&attr).map(|id| VoiceLanguage {
                            id,
                            name: super::language_name(&desc),
                        })
                    })
                    .fold(Vec::<VoiceLanguage>::new(), |mut acc, l| {
                        if !acc.iter().any(|x| x.id == l.id) {
                            acc.push(l);
                        }
                        acc
                    });
                let _ = reply.send(list);
            }
            Job::Language(l) => {
                if engine.as_ref().map(|e| e.language()) != l.as_deref() {
                    engine = None;
                }
                language = l;
            }
        }
    }
}

/// The whole shortcut is still down. Letting go of any of its keys ends the hold, so a Space still
/// held after Ctrl and Alt come up can't type spaces into the app while tug listens.
fn still_held(keys: &Keys) -> bool {
    const VK_SHIFT: u8 = 0x10;
    const VK_CONTROL: u8 = 0x11;
    const VK_MENU: u8 = 0x12;
    system::key_down(keys.key)
        && (!keys.ctrl || system::key_down(VK_CONTROL))
        && (!keys.shift || system::key_down(VK_SHIFT))
        && (!keys.alt || system::key_down(VK_MENU))
}

/// What the indicator says at the end of a hold.
fn finish(outcome: VoiceOutcome) {
    let problem = match outcome {
        VoiceOutcome::Typed => return overlay::hide(),
        VoiceOutcome::TooShort => "Hold the keys while you talk",
        VoiceOutcome::NothingHeard => "Didn't catch that",
        VoiceOutcome::MicBlocked => "Microphone blocked. Open tug's Settings to fix it",
        VoiceOutcome::NoMic => "No microphone found",
        VoiceOutcome::NoTarget => "The window changed, so nothing was typed",
        VoiceOutcome::Unavailable => "Voice typing needs a Windows speech language",
        VoiceOutcome::Failed => "Couldn't type that. Try again",
    };
    overlay::show(problem, Look::Problem, PROBLEM_MS);
}

/// One hold, from the press to the typed text. Always ends with the microphone closed.
fn hold(press: &Press, engine: &mut Option<sapi::Sapi>, language: Option<&str>) -> VoiceOutcome {
    let Some(target) = press.target.clone() else {
        log::info!("voice typing: nothing in front to type into");
        return VoiceOutcome::NoTarget;
    };
    let blocked = system::mic_blocked();
    super::note_mic_blocked(blocked);
    if blocked {
        log::info!("voice typing: the microphone is blocked in Windows' privacy settings");
        return VoiceOutcome::MicBlocked;
    }
    let Some(language) = language else {
        return VoiceOutcome::Unavailable;
    };
    if !capture::has_device() {
        log::info!("voice typing: no microphone");
        return VoiceOutcome::NoMic;
    }
    let mut rec = match capture::Recording::start() {
        Ok(r) => r,
        Err(e) => {
            log::warn!("voice typing: couldn't open the microphone ({e:?})");
            return VoiceOutcome::NoMic;
        }
    };
    overlay::show("Listening…", Look::Listening, 0);
    let started = Instant::now();
    while still_held(&press.keys) && started.elapsed() < MAX_HOLD {
        rec.poll();
        std::thread::sleep(TICK);
    }
    let held = started.elapsed();
    let tail_end = Instant::now() + TAIL;
    while Instant::now() < tail_end {
        rec.poll();
        std::thread::sleep(TICK);
    }
    let pcm = rec.finish();
    if let Some(outcome) = judge_recording(held, peak(&pcm)) {
        log::info!("voice typing: {outcome:?} after {} ms", held.as_millis());
        return outcome;
    }

    overlay::show("Typing…", Look::Working, 0);
    if engine.as_ref().is_some_and(|e| e.language() != language) {
        *engine = None;
    }
    if engine.is_none() {
        match sapi::Sapi::new(language) {
            Ok(s) => *engine = Some(s),
            Err(EngineError::Unavailable) => return VoiceOutcome::Unavailable,
            Err(EngineError::Failed(why)) => {
                log::warn!("voice typing: the recognizer didn't start: {why}");
                return VoiceOutcome::Failed;
            }
        }
    }
    let Some(recognizer) = engine.as_mut() else {
        return VoiceOutcome::Failed;
    };
    let t0 = Instant::now();
    let heard = match recognizer.transcribe(&pcm) {
        Ok(heard) => heard,
        Err(e) => {
            // Start fresh next time.
            *engine = None;
            log::warn!("voice typing: recognition failed: {e:?}");
            return match e {
                EngineError::Unavailable => VoiceOutcome::Unavailable,
                EngineError::Failed(_) => VoiceOutcome::Failed,
            };
        }
    };
    let recognized_ms = t0.elapsed().as_millis();
    let typed = text::format(&heard, text::target_for_process(&target.exe));

    // Held Ctrl or Alt would turn the characters into shortcuts.
    crate::code_fill::wait_for_release();
    let same = system::foreground().is_some_and(|f| f.hwnd == target.hwnd);
    if let Some(outcome) = judge_typing(&typed, same) {
        log::info!(
            "voice typing: {outcome:?} ({} ms of speech, recognized in {recognized_ms} ms)",
            held.as_millis()
        );
        return outcome;
    }
    if system::type_strokes(&text::strokes(&typed)) {
        log::info!(
            "voice typing: typed {} characters ({} ms of speech, recognized in {recognized_ms} ms)",
            typed.chars().count(),
            held.as_millis()
        );
        VoiceOutcome::Typed
    } else {
        log::warn!("voice typing: Windows didn't take the keystrokes");
        VoiceOutcome::Failed
    }
}
