//! The Windows side of Focus mode: one look ([`look`]) and the `tug-focus-watch` thread that turns
//! looks into `focus-state` events.
//!
//! **Cost.** While Focus mode is off the thread sleeps on a condition variable and never wakes.
//! While on, it looks every 2 s, asking only about the switches that are on: one shell call and
//! the foreground window's size (full screen), a read of the ConsentStore's microphone and webcam
//! keys (meetings: a few dozen small registry values), and the foreground window's program
//! (coding). None of these sends a message to another program, so none can hang on one; and
//! nothing waits on this thread (pop-ups are decided in the window from the last event), so a
//! slow look only delays the next event.
//!
//! The decisions are in `mod.rs`; this file only asks Windows and reports.

use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use tauri::{AppHandle, Emitter};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_QWORD,
};

use super::{
    apps_in_use, filetime_from_unix_ms, is_coding, non_packaged_key, reason, transition, CapabilityUse, Debounce,
    FocusSettings, FocusState, Look, EVENT, POLL, SETTING,
};
use crate::store::Store;

struct Inner {
    settings: FocusSettings,
    debounce: Debounce,
}

struct Monitor {
    app: AppHandle,
    inner: Mutex<Inner>,
    wake: Condvar,
}

static MONITOR: OnceLock<Monitor> = OnceLock::new();

impl Monitor {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Tell the window (and the log, by category) when the state changed. Called with the lock
    /// held, so a Settings change and a look can't reach the window in the wrong order.
    fn report(&self, before: FocusState, change: Option<FocusState>, meeting_apps: usize) {
        let Some(now) = change else { return };
        if let Some(line) = transition(before, now, meeting_apps) {
            log::info!("focus mode: {line}");
        }
        if let Err(e) = self.app.emit(EVENT, now) {
            log::warn!("emit {EVENT} failed: {e}");
        }
    }
}

/// Start watching (once): read the setting and start the `tug-focus-watch` thread, which sleeps
/// until Focus mode is on.
pub fn start(app: &AppHandle, store: &Store) {
    let settings = FocusSettings::from_setting(store.setting(SETTING).ok().flatten().as_deref());
    let monitor = Monitor {
        app: app.clone(),
        inner: Mutex::new(Inner {
            settings,
            debounce: Debounce::default(),
        }),
        wake: Condvar::new(),
    };
    if MONITOR.set(monitor).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("tug-focus-watch".into()).spawn(run);
    if let Err(e) = spawned {
        log::warn!("focus mode: watcher unavailable ({e}); pop-ups are never held");
    }
}

fn run() {
    let Some(m) = MONITOR.get() else { return };
    // tug's own entry in the ConsentStore (voice typing uses the microphone), read once.
    let own = std::env::current_exe()
        .map(|p| non_packaged_key(&p.to_string_lossy()))
        .unwrap_or_default();
    loop {
        let settings = {
            let mut g = m.lock();
            // Nothing to look for: sleep until Focus mode is switched on.
            while !g.settings.watching() {
                g = m.wake.wait(g).unwrap_or_else(|e| e.into_inner());
            }
            g.settings
        };
        // Outside the lock: a Settings change doesn't wait for a look.
        let found = look(&settings, &own);
        {
            let mut g = m.lock();
            // Switched off (or changed) during the look: that change already said what's true.
            if g.settings == settings {
                let before = g.debounce.state();
                let change = g.debounce.update(Instant::now(), reason(&settings, &found));
                m.report(before, change, found.meeting_apps);
            }
        }
        let g = m.lock();
        let _ = m.wake.wait_timeout(g, POLL);
    }
}

/// What the window reads at startup, before any event (focused or not right now).
pub fn state() -> FocusState {
    MONITOR.get().map(|m| m.lock().debounce.state()).unwrap_or_default()
}

/// The Settings switches changed. Off, or the switch for the reason pop-ups are held turned off,
/// ends the focus at once, so the window shows its summary straight away; on starts looking.
pub fn set_settings(settings: FocusSettings) {
    let Some(m) = MONITOR.get() else { return };
    {
        let mut g = m.lock();
        g.settings = settings;
        let before = g.debounce.state();
        if before.reason.is_some_and(|r| !settings.allows(r)) {
            let change = g.debounce.reset();
            m.report(before, change, 0);
        }
    }
    m.wake.notify_all();
}

/// One look, for the switches that are on.
fn look(settings: &FocusSettings, own: &str) -> Look {
    Look {
        full_screen: settings.full_screen && crate::share_privacy::full_screen_now(),
        meeting_apps: if settings.meetings { meeting_apps(own) } else { 0 },
        coding: settings.coding && crate::voice::foreground_exe().is_some_and(|exe| is_coding(&exe)),
    }
}

const CONSENT_STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

/// How many other apps are using the microphone or camera right now. A key that can't be read
/// counts as no one using it: Focus mode holds pop-ups only when it knows.
fn meeting_apps(own: &str) -> usize {
    let mut entries = Vec::new();
    for capability in ["microphone", "webcam"] {
        let path = format!(r"{CONSENT_STORE}\{capability}");
        let Some(store) = Key::open(HKEY_CURRENT_USER, &path) else {
            continue;
        };
        // Store apps are named right under the capability; desktop programs under NonPackaged.
        for name in store.subkeys() {
            if name.eq_ignore_ascii_case("NonPackaged") {
                if let Some(desktop) = Key::open(store.0, &name) {
                    for program in desktop.subkeys() {
                        entries.push(desktop.usage(program));
                    }
                }
            } else {
                entries.push(store.usage(name));
            }
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    apps_in_use(&entries, own, filetime_from_unix_ms(now))
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened by RegOpenKeyExW in `open`.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Plenty for a desktop program's path, the longest key name here (registry key names are at
/// most 255 characters).
const NAME_MAX: usize = 260;
/// A ConsentStore holds an entry per app that ever asked; this caps a damaged one.
const MAX_SUBKEYS: u32 = 4096;

impl Key {
    /// Open `path` under `parent` to read, or `None` if it isn't there or can't be read.
    fn open(parent: HKEY, path: &str) -> Option<Key> {
        let mut h = HKEY::default();
        // SAFETY: plain FFI; the key is closed on drop.
        let r = unsafe { RegOpenKeyExW(parent, &HSTRING::from(path), None, KEY_READ, &mut h) };
        r.is_ok().then_some(Key(h))
    }

    /// The names of this key's subkeys.
    fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut buf = [0u16; NAME_MAX];
        for i in 0..MAX_SUBKEYS {
            let mut len = buf.len() as u32;
            // SAFETY: the buffer and its length (in characters) go together; the rest is unused.
            let r = unsafe {
                RegEnumKeyExW(
                    self.0,
                    i,
                    Some(PWSTR(buf.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            // A name too long to be one of these entries: skip it, keep going.
            if r == ERROR_MORE_DATA {
                continue;
            }
            if r.is_err() {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]));
        }
        names
    }

    /// One app's entry: its last start and stop times (0 when missing). The stop is read first,
    /// since most apps aren't using the microphone and that answers it.
    fn usage(&self, name: String) -> CapabilityUse {
        let stop = self.qword(&name, "LastUsedTimeStop");
        let start = if stop == 0 {
            self.qword(&name, "LastUsedTimeStart")
        } else {
            0
        };
        CapabilityUse { key: name, start, stop }
    }

    fn qword(&self, subkey: &str, value: &str) -> u64 {
        let mut data = 0u64;
        let mut size = std::mem::size_of::<u64>() as u32;
        // SAFETY: `data` is a u64 and `size` says so; the names are NUL-terminated HSTRINGs.
        let r = unsafe {
            RegGetValueW(
                self.0,
                &HSTRING::from(subkey),
                &HSTRING::from(value),
                RRF_RT_REG_QWORD,
                None,
                Some(&mut data as *mut u64 as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        if r.is_ok() {
            data
        } else {
            0
        }
    }
}
