//! The Windows side of voice typing. Two threads, both started the first time they're needed:
//!
//! - `tug-voice-key` owns the RegisterHotKey registration (hotkeys belong to the thread that
//!   registered them) and the indicator window, and pumps their messages. A press only notes
//!   which window is in front and hands the hold to the worker, so the queue never waits.
//! - `tug-voice` is a COM thread that runs each hold: microphone open while the key is down,
//!   recognize, shape the text, type it. It also owns the engines between holds: a whisper.cpp
//!   one (Accurate or Most accurate, loaded on a helper thread while the first hold records, with
//!   Accurate standing in for Most accurate when memory is short) and Windows' recognizer (Basic,
//!   and the fallback). All are let go when voice typing goes off.
//!
//! RegisterHotKey only reports the press, so the worker watches the shortcut's key with
//! GetAsyncKeyState to know when it's let go: no keyboard hook, nothing else is seen.
//!
//! Starting can fail without taking tug down: a thread that doesn't start, an indicator window
//! Windows won't create, a shortcut it refuses, or speech recognition that doesn't answer each
//! comes back as a `StartError`, and `voice::apply` switches voice typing back off and says why.
//!
//! The decisions live in `voice/mod.rs` and `voice/text.rs`; this only asks Windows and reports.

mod capture;
mod overlay;
mod sapi;
mod system;
mod whisper;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG, PM_NOREMOVE, WM_APP,
    WM_HOTKEY,
};

use super::engine::{AccurateProblem, Engine, EngineChoice, EngineError};
use super::model::ModelId;
use super::{
    audio, judge_recording, judge_typing, model, peak, pill_problem, text, vocab, EngineConfig, StartError,
    VoiceLanguage, VoiceOutcome, MAX_HOLD, TAIL,
};
use crate::callback::guarded;
use crate::code_fill::Keys;
use overlay::Look;

const HOTKEY_ID: i32 = 1;
/// Posted to the key thread: a registration request is waiting.
const WM_REGISTER: u32 = WM_APP + 1;
/// How often the hold loop checks the key and collects audio.
const TICK: Duration = Duration::from_millis(10);
/// How long a problem stays on the indicator.
const PROBLEM_MS: u32 = 2600;
/// How long to wait for the key thread to start, or to answer a registration.
const KEY_WAIT: Duration = Duration::from_secs(2);
/// How long to wait for speech recognition to list its languages.
const LANGUAGES_WAIT: Duration = Duration::from_secs(10);
/// How long to wait for the speech thread to look at the default microphone.
const MIC_WAIT: Duration = Duration::from_secs(3);

/// A registration for the key thread. The answer: `Ok(true)` registered, `Ok(false)` another app
/// has the shortcut (or there was nothing to register), `Err` anything else Windows said.
struct Request {
    keys: Option<Keys>,
    reply: mpsc::Sender<Result<bool, String>>,
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
    Languages(mpsc::Sender<Result<Vec<VoiceLanguage>, String>>),
    /// What to listen with from now on.
    Configure(EngineConfig),
    /// Is the default microphone a phone-call headset?
    Mic(mpsc::Sender<bool>),
}

/// A loaded whisper.cpp engine and which model it runs (Most accurate's, or Accurate's, which may
/// be standing in for it when memory was short).
struct Loaded {
    model: ModelId,
    whisper: whisper::Whisper,
}

/// The speech thread's engines and what they're for.
#[derive(Default)]
struct Engines {
    config: EngineConfig,
    basic: Option<sapi::Sapi>,
    accurate: Option<Loaded>,
}

/// Whether a configuration runs a whisper.cpp engine.
fn whisper_engine(config: &EngineConfig) -> bool {
    config.engine.and_then(EngineChoice::model).is_some()
}

impl Engines {
    /// Take a new configuration, letting go of any engine it no longer uses.
    fn configure(&mut self, config: EngineConfig) {
        if !whisper_engine(&config)
            || config.engine != self.config.engine
            || config.model != self.config.model
            || config.fallback != self.config.fallback
            || config.spoken != self.config.spoken
        {
            self.accurate = None;
        }
        // Basic is let go when it isn't the engine (it's loaded again if Accurate ever needs to
        // fall back) or when the language changes.
        if config.engine != Some(EngineChoice::Basic)
            || self.basic.as_ref().map(|e| e.language()) != config.language.as_deref()
        {
            self.basic = None;
        }
        self.config = config;
    }
}

static KEY_THREAD: OnceLock<KeyThread> = OnceLock::new();
/// Held while the key thread is being started, so two requests at once start one thread.
static KEY_STARTING: Mutex<()> = Mutex::new(());
static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
/// Held while the worker is being started.
static WORKER_STARTING: Mutex<()> = Mutex::new(());
/// The registered shortcut, read by the key thread on a press.
static REGISTERED: Mutex<Option<Keys>> = Mutex::new(None);
/// One hold at a time: a press while one is being recognized or typed is ignored.
static BUSY: AtomicBool = AtomicBool::new(false);

/// A hold is being listened to, recognized or typed.
pub fn busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

fn worker() -> Result<&'static Mutex<mpsc::Sender<Job>>, StartError> {
    if let Some(w) = WORKER.get() {
        return Ok(w);
    }
    let _starting = WORKER_STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(w) = WORKER.get() {
        return Ok(w);
    }
    log::info!("voice typing: starting the speech thread");
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("tug-voice".into())
        .spawn(move || run_worker(rx))
        .map_err(|e| StartError::Speech(format!("the speech thread didn't start: {e}")))?;
    let _ = WORKER.set(Mutex::new(tx));
    WORKER
        .get()
        .ok_or_else(|| StartError::Speech("the speech thread wasn't kept".into()))
}

fn send_job(job: Job) -> Result<(), StartError> {
    worker()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(job)
        .map_err(|_| StartError::Speech("the speech thread has stopped".into()))
}

fn key_thread() -> Result<&'static KeyThread, StartError> {
    if let Some(t) = KEY_THREAD.get() {
        return Ok(t);
    }
    let _starting = KEY_STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = KEY_THREAD.get() {
        return Ok(t);
    }
    log::info!("voice typing: starting the shortcut thread");
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("tug-voice-key".into())
        .spawn(move || run_keys(ready_tx, req_rx))
        .map_err(|e| StartError::Shortcut(format!("the shortcut thread didn't start: {e}")))?;
    match ready_rx.recv_timeout(KEY_WAIT) {
        Ok(Ok(id)) => {
            let _ = KEY_THREAD.set(KeyThread {
                id,
                requests: Mutex::new(req_tx),
            });
            log::info!("voice typing: shortcut thread ready");
        }
        // The thread has ended; the next attempt starts a new one.
        Ok(Err(why)) => return Err(StartError::Indicator(why)),
        Err(_) => {
            return Err(StartError::Shortcut(format!(
                "the shortcut thread didn't start within {} s",
                KEY_WAIT.as_secs()
            )))
        }
    }
    KEY_THREAD
        .get()
        .ok_or_else(|| StartError::Shortcut("the shortcut thread wasn't kept".into()))
}

/// Register `keys` (replacing any earlier shortcut), or unregister with `None`. `Ok(true)` when
/// the shortcut is now registered, `Ok(false)` when another app has it (or after unregistering).
pub fn register(keys: Option<Keys>) -> Result<bool, StartError> {
    let result = request_registration(keys);
    let registered = matches!(result, Ok(true));
    *REGISTERED.lock().unwrap_or_else(|e| e.into_inner()) = keys.filter(|_| registered);
    result
}

fn request_registration(keys: Option<Keys>) -> Result<bool, StartError> {
    if keys.is_none() && KEY_THREAD.get().is_none() {
        // Never started: nothing to unregister.
        return Ok(false);
    }
    let thread = key_thread()?;
    if keys.is_some() {
        log::info!("voice typing: registering the shortcut");
    }
    let (reply, answer) = mpsc::channel();
    thread
        .requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(Request { keys, reply })
        .map_err(|_| StartError::Shortcut("the shortcut thread has stopped".into()))?;
    // SAFETY: posting a message with no pointers to a thread tug owns.
    unsafe { PostThreadMessageW(thread.id, WM_REGISTER, WPARAM(0), LPARAM(0)) }
        .map_err(|e| StartError::Shortcut(format!("couldn't reach the shortcut thread: {e}")))?;
    match answer.recv_timeout(KEY_WAIT) {
        Ok(Ok(registered)) => {
            if keys.is_some() {
                log::info!(
                    "voice typing: shortcut {}",
                    if registered {
                        "registered"
                    } else {
                        "taken by another app"
                    }
                );
            }
            Ok(registered)
        }
        Ok(Err(why)) => Err(StartError::Shortcut(why)),
        Err(_) => Err(StartError::Shortcut(format!(
            "the shortcut thread didn't answer within {} s",
            KEY_WAIT.as_secs()
        ))),
    }
}

/// The installed speech languages (listed on the COM thread), or why they couldn't be.
pub fn languages() -> Result<Vec<VoiceLanguage>, StartError> {
    let (reply, answer) = mpsc::channel();
    send_job(Job::Languages(reply))?;
    match answer.recv_timeout(LANGUAGES_WAIT) {
        Ok(listed) => listed.map_err(StartError::Speech),
        Err(_) => Err(StartError::Speech(format!(
            "speech recognition didn't list its languages within {} s",
            LANGUAGES_WAIT.as_secs()
        ))),
    }
}

/// What to listen with from now on (nothing when voice typing is off: both engines are let go).
pub fn configure(config: EngineConfig) {
    if config.engine.is_none() && WORKER.get().is_none() {
        return;
    }
    if let Err(e) = send_job(Job::Configure(config)) {
        log::warn!("voice typing: {}", e.detail());
    }
}

/// Whether the default microphone is a phone-call headset (asked on the speech thread, which has
/// COM), or `None` if it didn't answer in time (it may be in the middle of a hold).
pub fn phone_mic() -> Option<bool> {
    let (reply, answer) = mpsc::channel();
    send_job(Job::Mic(reply)).ok()?;
    answer.recv_timeout(MIC_WAIT).ok()
}

/// Ask Windows about the default microphone (on a COM thread).
fn default_mic_is_phone() -> bool {
    system::default_mic().is_some_and(|(name, rate)| super::phone_call_mic(&name, rate))
}

pub fn mic_blocked() -> bool {
    system::mic_blocked()
}

pub fn ui_language() -> u16 {
    system::ui_language()
}

fn run_keys(ready: mpsc::Sender<Result<u32, String>>, requests: mpsc::Receiver<Request>) {
    let mut msg = MSG::default();
    // SAFETY: plain Win32 calls on this thread with a valid MSG buffer. The peek creates the
    // thread's message queue, so a registration posted right after `ready` isn't lost.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    // The indicator is created here, on the thread that pumps its messages below, and lives as
    // long as this thread does.
    log::info!("voice typing: creating the listening indicator");
    if let Err(why) = overlay::create() {
        log::warn!("voice typing: the listening indicator couldn't be created: {why}");
        let _ = ready.send(Err(why));
        return;
    }
    log::info!("voice typing: listening indicator ready");
    // SAFETY: as above.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    loop {
        // SAFETY: as above; GetMessageW returns 0 on WM_QUIT and -1 on error.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            log::warn!("voice typing: the shortcut thread's message loop ended ({})", got.0);
            break;
        }
        // Thread messages (no window): ours. Window messages: the indicator's.
        if msg.hwnd.is_invalid() {
            match msg.message {
                WM_HOTKEY if msg.wParam.0 == HOTKEY_ID as usize => guarded("voice typing shortcut", on_press, || ()),
                WM_REGISTER => {
                    while let Ok(req) = requests.try_recv() {
                        let _ = req.reply.send(apply_registration(req.keys));
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

/// On the key thread: replace the registration with `keys` (or none).
fn apply_registration(keys: Option<Keys>) -> Result<bool, String> {
    // SAFETY: the hotkey is registered to this thread (no window), id HOTKEY_ID.
    unsafe {
        let _ = UnregisterHotKey(None, HOTKEY_ID);
    }
    let Some(k) = keys else {
        return Ok(false);
    };
    // SAFETY: as above.
    match unsafe { RegisterHotKey(None, HOTKEY_ID, HOT_KEY_MODIFIERS(k.modifiers()), k.vk()) } {
        Ok(()) => Ok(true),
        Err(e) if e.code() == ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() => Ok(false),
        Err(e) => Err(format!("RegisterHotKey failed: {e}")),
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
    if let Err(e) = send_job(Job::Hold(press)) {
        log::warn!("voice typing: {}", e.detail());
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn run_worker(jobs: mpsc::Receiver<Job>) {
    // SAFETY: initialising COM for this thread, which lives as long as tug.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|e| format!("COM didn't start: {e}"));
    if let Err(why) = &com {
        log::warn!("voice typing: {why}");
    }
    let mut engines = Engines::default();
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Hold(press) => {
                let outcome = guarded(
                    "voice typing hold",
                    || hold(&press, &mut engines),
                    || VoiceOutcome::Failed,
                );
                finish(outcome);
                BUSY.store(false, Ordering::SeqCst);
                super::emit_typed(outcome);
                // After the typing, so it never delays it: Settings says when the microphone is
                // a phone-call headset.
                if com.is_ok() {
                    super::note_phone_mic(guarded("voice typing microphone", default_mic_is_phone, || false));
                }
            }
            Job::Languages(reply) => {
                let listed = match &com {
                    Err(why) => Err(why.clone()),
                    Ok(()) => guarded("voice typing languages", list_languages, || {
                        Err("listing the languages failed".into())
                    }),
                };
                let _ = reply.send(listed);
            }
            Job::Configure(config) => engines.configure(config),
            Job::Mic(reply) => {
                let phone = com.is_ok() && guarded("voice typing microphone", default_mic_is_phone, || false);
                let _ = reply.send(phone);
            }
        }
    }
}

/// The installed speech languages, one entry per language id.
fn list_languages() -> Result<Vec<VoiceLanguage>, String> {
    Ok(sapi::languages()?
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
        }))
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

/// What the indicator says at the end of a hold (`pill_problem`).
fn finish(outcome: VoiceOutcome) {
    match pill_problem(outcome) {
        Some(problem) => overlay::show(problem, Look::Problem, PROBLEM_MS),
        None => overlay::hide(),
    }
}

/// Why a whisper.cpp engine didn't load.
enum LoadError {
    /// The file isn't exactly the model (wrong size or hash) or is gone: it's deleted.
    Damaged(String),
    /// whisper.cpp couldn't start with the checked model: the file is kept, Basic listens.
    Failed(String),
}

/// Load a whisper.cpp engine: read the model file once, check those very bytes against the
/// model's hash, and hand the same bytes to whisper.cpp, to listen in `language`. Nothing can
/// change the file between the check and the load, and whisper.cpp never sees a damaged or swapped
/// file. (Whether there's memory for it is decided first: `engine::plan_load`.)
fn load_whisper(path: &std::path::Path, model: ModelId, language: &'static str) -> Result<Loaded, LoadError> {
    let t0 = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => LoadError::Damaged("it's gone".into()),
        _ => LoadError::Failed(format!("couldn't read it: {e}")),
    })?;
    if !model::verify_bytes(&bytes, model.spec()) {
        return Err(LoadError::Damaged("it doesn't match its SHA-256".into()));
    }
    let whisper = whisper::Whisper::load_tuned(&bytes, whisper::Tuning::for_model(model), language)
        .map_err(|e| LoadError::Failed(format!("{e:?}")))?;
    drop(bytes);
    log::info!(
        "voice typing: {} engine loaded in {} ms ({} threads, language {language})",
        model.as_str(),
        t0.elapsed().as_millis(),
        whisper::threads()
    );
    Ok(Loaded { model, whisper })
}

/// A whisper.cpp engine being loaded on a helper thread while the person talks (the first hold
/// after switching on waits for the recording, not the recording and then the model), with what
/// Settings should say about it once it's in.
struct Loading {
    handle: Option<std::thread::JoinHandle<Result<Loaded, LoadError>>>,
    /// The model being loaded (`None`: nothing to load; the plan's problem still applies).
    model: Option<ModelId>,
    problem: Option<AccurateProblem>,
}

/// The model the configuration wants and its file, when the engine is a whisper.cpp one.
fn wanted(config: &EngineConfig) -> Option<(ModelId, std::path::PathBuf)> {
    let model = config.engine.and_then(EngineChoice::model)?;
    Some((model, config.model.clone()?))
}

/// Decide what to load for this hold given the free memory (`engine::plan_load`) and start it.
/// Nothing when the model wanted is already loaded; when memory is short for Most accurate,
/// Accurate (if it can stand in), and nothing at all when there's too little for either.
fn start_loading(engines: &Engines) -> Option<Loading> {
    let (want, path) = wanted(&engines.config)?;
    if engines.accurate.as_ref().is_some_and(|l| l.model == want) {
        return None;
    }
    let available = system::available_memory();
    let plan = crate::voice::engine::plan_load(want, available, engines.config.fallback.is_some());
    if let Some(free) = available.filter(|_| plan.problem.is_some()) {
        log::info!(
            "voice typing: {} MB of memory free, too little for the {} engine; {}",
            free >> 20,
            want.as_str(),
            match plan.model {
                Some(m) => format!("using the {} engine for this hold", m.as_str()),
                None => "using the basic engine for this hold".into(),
            }
        );
    }
    let load = plan
        .model
        .filter(|m| engines.accurate.as_ref().is_none_or(|l| l.model != *m));
    let handle = load.and_then(|m| {
        let file = if m == want {
            path.clone()
        } else {
            engines.config.fallback.clone()?
        };
        let language = if m == ModelId::Accurate {
            "en"
        } else {
            engines.config.spoken
        };
        std::thread::Builder::new()
            .name("tug-voice-load".into())
            .spawn(move || load_whisper(&file, m, language))
            .map_err(|e| log::warn!("voice typing: couldn't start loading the {} engine ({e})", m.as_str()))
            .ok()
    });
    Some(Loading {
        handle,
        model: load,
        problem: plan.problem,
    })
}

/// Collect the helper thread's engine. This hold falls back to something smaller if it didn't
/// load: a file that isn't the model is deleted (`model_damaged`); low memory is noted and tried
/// again next hold; any other failure keeps the file, notes it and stops trying until voice typing
/// (or the engine) is picked again. A fallback (Accurate standing in) that fails is just dropped.
fn finish_loading(engines: &mut Engines, loading: Option<Loading>) {
    let Some(loading) = loading else {
        return;
    };
    let want = engines.config.engine.and_then(EngineChoice::model);
    let result = match (loading.handle, loading.model) {
        (Some(handle), Some(model)) => Some((
            model,
            handle
                .join()
                .unwrap_or_else(|_| Err(LoadError::Failed("loading it panicked".into()))),
        )),
        _ => None,
    };
    let Some((model, result)) = result else {
        // Nothing was loaded: too little memory for anything new. Accurate, already loaded as a
        // stand-in, keeps listening (it's in memory anyway); otherwise this hold uses Basic.
        super::note_accurate(match (&engines.accurate, loading.problem) {
            (Some(_), Some(_)) => Some(AccurateProblem::LowMemoryUsedAccurate),
            (_, problem) => problem,
        });
        return;
    };
    let is_fallback = Some(model) != want;
    match result {
        Ok(loaded) => {
            engines.accurate = Some(loaded);
            super::note_accurate(loading.problem);
        }
        Err(LoadError::Damaged(why)) => {
            super::model_damaged(model, &why);
            if is_fallback {
                engines.config.fallback = None;
            } else {
                engines.config.model = None;
            }
        }
        Err(LoadError::Failed(why)) if is_fallback => {
            log::warn!(
                "voice typing: the {} engine couldn't start ({why}); using the basic engine",
                model.as_str()
            );
            engines.config.fallback = None;
            super::note_accurate(Some(AccurateProblem::LowMemory));
        }
        Err(LoadError::Failed(why)) => {
            log::warn!(
                "voice typing: the {} engine couldn't start ({why}); keeping the model and using the basic engine",
                model.as_str()
            );
            super::note_accurate(Some(AccurateProblem::Failed));
            engines.config.model = None;
        }
    }
}

/// Recognize one recording with the configured engine, with the prompt for the program in front
/// (`focus`) and the person's hints. A whisper.cpp engine falls back to Basic (when Windows has a
/// speech language) if it isn't loaded or fails. `Ok((heard, engine used))`.
fn recognize(
    engines: &mut Engines,
    pcm: &[i16],
    focus: vocab::Focus,
    hints: &vocab::Hints,
) -> Result<(String, &'static str), VoiceOutcome> {
    if whisper_engine(&engines.config) {
        if let Some(loaded) = engines.accurate.as_mut() {
            loaded.whisper.set_prompt(focus, hints);
            match loaded.whisper.transcribe(pcm) {
                Ok(heard) => return Ok((heard, loaded.model.as_str())),
                Err(e) => {
                    // Start fresh next time; this hold goes to Basic.
                    log::warn!(
                        "voice typing: the {} engine failed ({e:?}); using the basic engine for this hold",
                        loaded.model.as_str()
                    );
                    engines.accurate = None;
                }
            }
        } else {
            log::info!("voice typing: no whisper.cpp engine is ready; using the basic engine for this hold");
        }
    }
    let Some(language) = engines
        .config
        .language
        .clone()
        .filter(|_| engines.config.engine.is_some())
    else {
        return Err(VoiceOutcome::Unavailable);
    };
    if engines.basic.as_ref().is_some_and(|e| e.language() != language) {
        engines.basic = None;
    }
    if engines.basic.is_none() {
        match sapi::Sapi::new(&language) {
            Ok(s) => engines.basic = Some(s),
            Err(EngineError::Unavailable) => return Err(VoiceOutcome::Unavailable),
            Err(EngineError::Failed(why)) => {
                log::warn!("voice typing: the recognizer didn't start: {why}");
                return Err(VoiceOutcome::Failed);
            }
        }
    }
    let Some(basic) = engines.basic.as_mut() else {
        return Err(VoiceOutcome::Failed);
    };
    match basic.transcribe(pcm) {
        Ok(heard) => Ok((heard, "basic")),
        Err(e) => {
            // Start fresh next time.
            engines.basic = None;
            log::warn!("voice typing: recognition failed: {e:?}");
            Err(match e {
                EngineError::Unavailable => VoiceOutcome::Unavailable,
                EngineError::Failed(_) => VoiceOutcome::Failed,
            })
        }
    }
}

/// One hold, from the press to the typed text. Always ends with the microphone closed.
fn hold(press: &Press, engines: &mut Engines) -> VoiceOutcome {
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
    if engines.config.engine.is_none() {
        return VoiceOutcome::Unavailable;
    }
    if !capture::has_device() {
        log::info!("voice typing: no microphone");
        return VoiceOutcome::NoMic;
    }
    let mut rec = match capture::Recording::start() {
        Ok(r) => r,
        Err(capture::OpenError::NoDevice) => {
            log::info!("voice typing: no usable microphone");
            return VoiceOutcome::NoMic;
        }
        // There is a microphone; it just couldn't be had right now (another app holding it
        // exclusively, or Windows switching devices as a hands-free call starts or ends). The hold
        // ends here and voice typing stays on: the next hold tries again.
        Err(e) => {
            log::warn!("voice typing: couldn't open the microphone ({e:?})");
            return VoiceOutcome::MicBusy;
        }
    };
    overlay::show("Listening…", Look::Listening, 0);
    let loading = start_loading(engines);
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
    if loading.is_some() {
        overlay::show("Typing…", Look::Working, 0);
    }
    // Collected even when this hold ends early, so the engine is ready for the next one and no
    // helper thread outlives the hold.
    finish_loading(engines, loading);
    if let Some(outcome) = judge_recording(held, peak(&pcm)) {
        log::info!("voice typing: {outcome:?} after {} ms", held.as_millis());
        return outcome;
    }
    if !audio::has_speech(&pcm) {
        // Steady noise (a fan, hiss) and nothing that rises above it: recognizers invent words
        // from that, so nothing is typed.
        log::info!(
            "voice typing: NothingHeard (no speech above the noise) after {} ms",
            held.as_millis()
        );
        return VoiceOutcome::NothingHeard;
    }

    overlay::show("Typing…", Look::Working, 0);
    let t0 = Instant::now();
    // The prompt leans on what the program in front is for, with the person's own words.
    let focus = vocab::focus_for_process(&target.exe);
    let hints = super::hints();
    let (heard, engine) = match recognize(engines, &pcm, focus, &hints) {
        Ok(r) => r,
        Err(outcome) => return outcome,
    };
    let recognized_ms = t0.elapsed().as_millis();
    // The person's spellings of their names and words (case only), whichever engine heard them.
    let heard = vocab::respell(&heard, &hints);
    let typed = text::format(&heard, text::target_for_process(&target.exe));

    // Held Ctrl or Alt would turn the characters into shortcuts: wait for them, and if they're
    // still down when the wait runs out, type nothing (the pill says to let go).
    let released = crate::code_fill::wait_for_release();
    let same = system::foreground().is_some_and(|f| f.hwnd == target.hwnd);
    if let Some(outcome) = judge_typing(&typed, released, same) {
        log::info!(
            "voice typing: {outcome:?} ({} ms of speech, recognized by the {engine} engine in {recognized_ms} ms)",
            held.as_millis()
        );
        return outcome;
    }
    if system::type_strokes(&text::strokes(&typed)) {
        log::info!(
            "voice typing: typed {} characters ({} ms of speech, recognized by the {engine} engine in {recognized_ms} ms)",
            typed.chars().count(),
            held.as_millis()
        );
        VoiceOutcome::Typed
    } else {
        log::warn!("voice typing: Windows didn't take the keystrokes");
        VoiceOutcome::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::{apply, VoiceState, HOTKEYS};

    /// Wait up to 2 s for the indicator to be shown (or hidden).
    fn indicator_becomes(visible: bool) -> bool {
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if overlay::visible() == visible {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Switching voice typing on is what crashed v0.5.18: Windows painted the new, still hidden
    /// indicator before it had any text, and the empty text reached DrawTextW. This runs the
    /// switch's own path (`voice::apply`, as `set_voice_typing` calls it) on this PC's real
    /// desktop, shows and hides the indicator (with text, and empty while it's on screen), and
    /// switches it off again, 20 times. Without the fix the process dies on the first round with
    /// 0xc000041d. It needs a desktop session and a Windows speech language, so it's run by hand:
    /// `cargo test --manifest-path src-tauri/Cargo.toml -p tug --lib voice::win::tests -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn switching_on_and_off_twenty_times_never_crashes() {
        // A shortcut nothing else on this PC holds, if there is one (taken ones still start the
        // thread and the indicator, which is what's being tested).
        let keys = HOTKEYS
            .iter()
            .copied()
            .find(|k| {
                let s = apply(true, k, None);
                println!("{k}: {:?}", s.state);
                assert_eq!(
                    s.start_error, None,
                    "{k}: a taken shortcut is inUse, not a failed start"
                );
                apply(false, k, None);
                s.state == VoiceState::On
            })
            .unwrap_or(HOTKEYS[0]);
        println!("using {keys}");
        for round in 1..=20 {
            let on = apply(true, keys, None);
            assert_eq!(on.start_error, None, "round {round}: {on:?}");
            assert!(on.enabled, "round {round}: still on");
            assert!(
                matches!(on.state, VoiceState::On | VoiceState::InUse),
                "round {round}: {:?}",
                on.state
            );
            overlay::show("Listening…", Look::Listening, 0);
            assert!(indicator_becomes(true), "round {round}: indicator shown");
            overlay::show("Typing…", Look::Working, 0);
            // Nothing to say, on screen: painted with no text.
            overlay::show("", Look::Problem, 0);
            std::thread::sleep(Duration::from_millis(30));
            assert!(overlay::visible(), "round {round}: still up");
            overlay::hide();
            assert!(indicator_becomes(false), "round {round}: indicator hidden");
            // A problem that hides by itself.
            overlay::show("Didn't catch that", Look::Problem, 50);
            assert!(indicator_becomes(true), "round {round}: problem shown");
            assert!(indicator_becomes(false), "round {round}: problem hid itself");

            let off = apply(false, keys, None);
            assert_eq!(off.state, VoiceState::Off, "round {round}");
            assert!(!off.enabled);
            assert!(REGISTERED.lock().unwrap().is_none(), "round {round}: shortcut let go");
        }
        println!("20 rounds: on, indicator shown and hidden, off");
    }

    /// Stands in for another dictation app on the same PC, on a thread of its own: it holds a
    /// hold-to-talk shortcut (RegisterHotKey), watches every key (a WH_KEYBOARD_LL hook), shows its
    /// own topmost layered overlay and keeps the microphone open, all until it's told to quit.
    struct OtherApp {
        thread: u32,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl OtherApp {
        fn start(keys: Keys) -> OtherApp {
            use windows::core::w;
            use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{
                CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
                SetLayeredWindowAttributes, SetWindowsHookExW, ShowWindow, UnhookWindowsHookEx, LWA_ALPHA,
                SW_SHOWNOACTIVATE, WH_KEYBOARD_LL, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_POPUP,
            };

            extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
                // SAFETY: passing the event on unchanged, as a well-behaved hook does.
                unsafe { CallNextHookEx(None, code, wparam, lparam) }
            }
            extern "system" fn other_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
                // SAFETY: default handling for the stand-in's own window.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }

            let (ready_tx, ready_rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                let mut msg = MSG::default();
                // SAFETY: Win32 calls on this thread with valid arguments; everything is undone
                // before the thread ends.
                unsafe {
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let hotkey = RegisterHotKey(None, 7, HOT_KEY_MODIFIERS(keys.modifiers()), keys.vk());
                    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0);
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(other_proc),
                        lpszClassName: w!("tugTestOtherOverlay"),
                        ..Default::default()
                    };
                    RegisterClassW(&class);
                    let overlay = CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                        w!("tugTestOtherOverlay"),
                        w!("other overlay"),
                        WS_POPUP,
                        40,
                        40,
                        120,
                        30,
                        None,
                        None,
                        None,
                        None,
                    );
                    if let Ok(o) = overlay {
                        let _ = SetLayeredWindowAttributes(o, COLORREF(0), 200, LWA_ALPHA);
                        let _ = ShowWindow(o, SW_SHOWNOACTIVATE);
                    }
                    let mic = capture::Recording::start();
                    println!(
                        "other app: shortcut {:?}, keyboard hook {:?}, overlay {:?}, microphone {:?}",
                        hotkey.is_ok(),
                        hook.is_ok(),
                        overlay.is_ok(),
                        mic.as_ref().map(|_| ()).map_err(|e| *e)
                    );
                    let _ = ready_tx.send(GetCurrentThreadId());
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    drop(mic);
                    if let Ok(o) = overlay {
                        let _ = DestroyWindow(o);
                    }
                    if let Ok(h) = hook {
                        let _ = UnhookWindowsHookEx(h);
                    }
                    let _ = UnregisterHotKey(None, 7);
                }
            });
            let thread = ready_rx.recv_timeout(Duration::from_secs(5)).expect("stand-in started");
            OtherApp {
                thread,
                handle: Some(handle),
            }
        }
    }

    impl Drop for OtherApp {
        fn drop(&mut self) {
            use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;
            // SAFETY: a message with no pointers to the stand-in's thread.
            unsafe {
                let _ = PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// Voice typing next to another dictation app that already holds the shortcut, a keyboard
    /// hook, a topmost overlay and the microphone: switching on never crashes. The taken shortcut
    /// is reported as in use (Settings offers the others), another shortcut works, tug's
    /// indicator shows and hides above the other overlay, and the microphone opens alongside or
    /// says it's busy. Run by hand like the test above.
    #[test]
    #[ignore]
    fn coexists_with_another_dictation_app() {
        let taken = HOTKEYS[3];
        let free = HOTKEYS[1];
        let other = OtherApp::start(crate::voice::parse_hotkey(taken).expect("offered"));
        for round in 1..=5 {
            let s = apply(true, taken, None);
            assert_eq!(s.start_error, None, "round {round}: {s:?}");
            assert_eq!(s.state, VoiceState::InUse, "round {round}: the other app has {taken}");
            assert!(s.enabled);

            let s = apply(true, free, None);
            assert_eq!(s.start_error, None, "round {round}: {s:?}");
            assert!(matches!(s.state, VoiceState::On | VoiceState::InUse), "{:?}", s.state);
            overlay::show("Listening…", Look::Listening, 0);
            assert!(indicator_becomes(true), "round {round}: indicator shown");
            // What a hold does first, with the other app holding the microphone.
            match capture::Recording::start() {
                Ok(rec) => println!(
                    "round {round}: microphone opened alongside ({} samples)",
                    rec.finish().len()
                ),
                Err(e) => println!("round {round}: microphone busy ({e:?}), the pill would say so"),
            }
            // The key polling a hold does, with the other app's hook in place.
            let _ = still_held(&crate::voice::parse_hotkey(free).expect("offered"));
            overlay::hide();
            assert!(indicator_becomes(false), "round {round}: indicator hidden");
            assert_eq!(apply(false, free, None).state, VoiceState::Off);
        }
        drop(other);
        println!("5 rounds next to another dictation app: no crash");
    }
}
