//! The Windows side of code fill: a `tug-hotkey` thread that owns the RegisterHotKey
//! registrations (hotkeys belong to the thread that registered them, so registering, unregistering
//! and the presses all happen on it), and typing a code with SendInput Unicode keystrokes. Quick
//! text's shortcut is registered on the same thread (`quick_text`), under its own id.
//!
//! The decisions live in `mod.rs`; this file only asks Windows and reports.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, SendInput, UnregisterHotKey, HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE,
    WM_APP, WM_HOTKEY,
};

use super::{decide, judge_typing, keystrokes, newest, CodeFilled, FillOutcome, Keys, Plan, MAX_AGE_MS};

/// The shortcuts registered on this thread. Each has its own id, so one can change or be taken by
/// another app without touching the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// Type the latest code.
    CodeFill,
    /// Text someone from any app (`quick_text`).
    QuickText,
}

impl Shortcut {
    fn id(self) -> i32 {
        match self {
            Shortcut::CodeFill => 1,
            Shortcut::QuickText => 2,
        }
    }
}

/// Posted to the hotkey thread: a registration request is waiting.
const WM_REGISTER: u32 = WM_APP + 1;
/// How much recent history to look through for the newest code.
const SCAN: u32 = 200;
/// How long to wait for the shortcut's keys to be let go before typing (held Ctrl would turn
/// the digits into Ctrl+digit shortcuts).
const RELEASE_WAIT: Duration = Duration::from_millis(1500);

struct Request {
    shortcut: Shortcut,
    keys: Option<Keys>,
    reply: mpsc::Sender<bool>,
}

struct Thread {
    id: u32,
    requests: Mutex<mpsc::Sender<Request>>,
}

static THREAD: OnceLock<Thread> = OnceLock::new();
/// One fill at a time: a second press while typing is ignored.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Start the `tug-hotkey` thread (once). Returns when it's ready for registrations.
pub fn start(app: AppHandle) {
    if THREAD.get().is_some() {
        return;
    }
    let (ready_tx, ready_rx) = mpsc::channel::<u32>();
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let spawned = std::thread::Builder::new()
        .name("tug-hotkey".into())
        .spawn(move || run(app, ready_tx, req_rx));
    if let Err(e) = spawned {
        return log::warn!("global shortcuts (type the code, quick text) unavailable: {e}");
    }
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(id) => {
            let _ = THREAD.set(Thread {
                id,
                requests: Mutex::new(req_tx),
            });
        }
        Err(_) => log::warn!("global shortcut thread didn't start"),
    }
}

/// Register `keys` as `shortcut` (replacing what it was before), or just unregister it with
/// `None`. True when the shortcut is now registered.
pub fn register(shortcut: Shortcut, keys: Option<Keys>) -> bool {
    let Some(thread) = THREAD.get() else {
        return false;
    };
    let (reply, answer) = mpsc::channel();
    let sent = thread
        .requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(Request { shortcut, keys, reply });
    if sent.is_err() {
        return false;
    }
    // SAFETY: posting a message with no pointers to a thread tug owns.
    if unsafe { PostThreadMessageW(thread.id, WM_REGISTER, WPARAM(0), LPARAM(0)) }.is_err() {
        return false;
    }
    answer.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
}

fn run(app: AppHandle, ready: mpsc::Sender<u32>, requests: mpsc::Receiver<Request>) {
    let mut msg = MSG::default();
    // SAFETY: plain Win32 calls on this thread with a valid MSG buffer. The peek creates the
    // thread's message queue, so a registration posted right after `ready` isn't lost.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        let _ = ready.send(GetCurrentThreadId());
    }
    loop {
        // SAFETY: as above; GetMessageW returns 0 on WM_QUIT and -1 on error.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        match msg.message {
            WM_HOTKEY if msg.wParam.0 == Shortcut::CodeFill.id() as usize => on_press(&app),
            WM_HOTKEY if msg.wParam.0 == Shortcut::QuickText.id() as usize => crate::quick_text::pressed(&app),
            WM_REGISTER => {
                while let Ok(req) = requests.try_recv() {
                    let id = req.shortcut.id();
                    // SAFETY: the hotkey is registered to this thread (no window), under its own id.
                    unsafe {
                        let _ = UnregisterHotKey(None, id);
                    }
                    let ok = req.keys.is_some_and(|k| {
                        // SAFETY: as above.
                        unsafe { RegisterHotKey(None, id, HOT_KEY_MODIFIERS(k.modifiers()), k.vk()) }.is_ok()
                    });
                    let _ = req.reply.send(ok);
                }
            }
            _ => {}
        }
    }
}

fn on_press(app: &AppHandle) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    // Off the hotkey thread, so waiting for the keys to come up doesn't hold its queue.
    let spawned = std::thread::Builder::new().name("tug-code-fill".into()).spawn(move || {
        let outcome = fill(&app);
        BUSY.store(false, Ordering::SeqCst);
        if let Err(e) = app.emit(crate::state::events::CODE_FILLED, outcome) {
            log::warn!("emit code-filled failed: {e}");
        }
    });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn fill(app: &AppHandle) -> CodeFilled {
    let report = |outcome| CodeFilled { outcome, from: None };
    let target = foreground_target();
    let fresh = newest_code(app);
    match decide(fresh, target.is_some()) {
        Plan::Report(outcome) => {
            match outcome {
                FillOutcome::NoCode => log::info!("type-the-code shortcut: no recent code"),
                _ => log::info!("type-the-code shortcut: tug's own window or nothing in front; not typed"),
            }
            report(outcome)
        }
        Plan::Type { code, from } => {
            // Held Ctrl, Alt or Shift would turn the digits into shortcuts in the app: wait for
            // them, and if one is still down when the wait runs out, type nothing. The person may
            // also have switched windows while letting go: type only where they pressed.
            let released = wait_for_release();
            match judge_typing(released, foreground_target() == target) {
                Some(FillOutcome::KeysHeld) => {
                    log::info!("type-the-code shortcut: keys still held after the wait; not typed");
                    return report(FillOutcome::KeysHeld);
                }
                Some(outcome) => {
                    log::info!("type-the-code shortcut: the window changed; not typed");
                    return report(outcome);
                }
                None => {}
            }
            if type_text(&code) {
                log::info!("code filled by the shortcut");
                CodeFilled {
                    outcome: FillOutcome::Typed,
                    from: Some(from),
                }
            } else {
                log::warn!("code fill: Windows didn't take the keystrokes");
                report(FillOutcome::Failed)
            }
        }
    }
}

/// The window in front, unless it's tug's own (or there's none): never type into tug.
fn foreground_target() -> Option<isize> {
    // SAFETY: read-only Win32 queries with a valid out-pointer.
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid != 0 && pid != GetCurrentProcessId()).then_some(hwnd.0 as isize)
    }
}

/// The newest code from the last 10 minutes, from a notification or a text, with who sent it.
fn newest_code(app: &AppHandle) -> Option<(String, String)> {
    let state = app.try_state::<crate::commands::AppState>()?;
    let store = &state.shared.store;
    let live = state.shared.live_session();
    let since = crate::state::now_ms() - MAX_AGE_MS;
    let notifications = store.recent(SCAN, None, live.as_deref()).unwrap_or_default();
    let messages = store.recent_messages(SCAN).unwrap_or_default();
    let from_note = |n: &crate::store::StoredNotification| {
        let title = n.title.trim();
        if title.is_empty() {
            n.app_name.clone().unwrap_or_default()
        } else {
            title.to_string()
        }
    };
    let items = notifications
        .iter()
        .map(|n| {
            let text = if n.message.is_empty() { &n.subtitle } else { &n.message };
            (n.received_at, text.as_str(), from_note(n))
        })
        .chain(
            messages
                .iter()
                .filter(|m| m.direction == crate::messages::Direction::In)
                .map(|m| {
                    let from = m.contact_name.clone().unwrap_or_else(|| m.address.clone());
                    (m.received_at, m.body.as_str(), from)
                }),
        );
    newest(items, since).map(|(_, code, from)| (code, from))
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: a read-only key-state query.
    (unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000) != 0
}

/// Wait (briefly) for Ctrl, Shift, Alt and Windows to be let go. True once they're all up; false
/// if one was still down when the wait ran out (typing then would turn characters into shortcuts).
pub(crate) fn wait_for_release() -> bool {
    let started = Instant::now();
    loop {
        let held = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .into_iter()
            .any(key_down);
        if !held {
            return true;
        }
        if started.elapsed() >= RELEASE_WAIT {
            return false;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Type `text` as Unicode keystrokes into the focused window. True if Windows took them all.
fn type_text(text: &str) -> bool {
    let inputs: Vec<INPUT> = keystrokes(text)
        .into_iter()
        .map(|(unit, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: if up {
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                    } else {
                        KEYEVENTF_UNICODE
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
        .collect();
    // SAFETY: a slice of fully initialised keyboard INPUTs and their size.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}
