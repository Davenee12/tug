//! The wake watch: a 1 s heartbeat on a thread of its own that tells the actor when the PC resumed,
//! so the link is retried the moment Windows thaws rather than after a backed-off timer (which,
//! after a resume, left tug disconnected until it was restarted).
//!
//! It used to be a task on the Bluetooth thread, comparing wall-clock time between ticks; a Windows
//! Bluetooth call blocking that thread while the adapter hung then read as a wake (2026-10-06). Here
//! nothing can hold it up but Windows suspending the process, and the decision (`crate::wake`) goes
//! by Windows' clocks and its resume notification rather than by how late a tick was.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Power::{PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS};
use windows::Win32::System::WindowsProgramming::{QueryInterruptTime, QueryUnbiasedInterruptTime};
use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC};

use super::Event;
use crate::wake::{Beat, Clocks, WakeWatch};

/// Set by Windows' resume notification; read and cleared by the next heartbeat.
static RESUMED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_power_event(_context: *const c_void, kind: u32, _setting: *const c_void) -> u32 {
    // Only the automatic resume: Windows sends it on every resume. PBT_APMRESUMESUSPEND follows only
    // once the user is active again, possibly minutes later, and would relink a link that's fine.
    if kind == PBT_APMRESUMEAUTOMATIC {
        RESUMED.store(true, Ordering::SeqCst);
    }
    ERROR_SUCCESS.0
}

fn clocks() -> Clocks {
    // SAFETY: both only write the value they return / the u64 handed to them.
    let with_sleep = unsafe { QueryInterruptTime() };
    let mut awake = 0u64;
    let _ = unsafe { QueryUnbiasedInterruptTime(&mut awake) };
    Clocks { with_sleep, awake }
}

/// Ask Windows to call `on_power_event` on suspend and resume. Best effort: without it the clocks
/// still catch sleep and hibernate. Returns whether Windows accepted.
fn register_for_resume() -> bool {
    // Registered for the life of the process, so the parameters are leaked rather than freed while
    // Windows may still read them.
    let params: &'static mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS =
        Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_power_event),
            Context: std::ptr::null_mut(),
        }));
    let mut registration = std::ptr::null_mut();
    // SAFETY: `params` lives forever and `on_power_event` only touches an atomic.
    let err = unsafe {
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK,
            HANDLE(std::ptr::from_mut(params).cast()),
            &mut registration,
        )
    };
    if err != ERROR_SUCCESS {
        log::warn!("Windows resume notifications unavailable ({err:?}); wake detection uses the clocks alone");
    }
    err == ERROR_SUCCESS
}

/// Start the wake watch. It sends `Event::Woke` into the actor's queue and stops once the actor's
/// receiver is gone.
pub(super) fn spawn(tx: UnboundedSender<Event>) {
    let _ = register_for_resume();
    let started = std::thread::Builder::new()
        .name("tug-wake-watch".into())
        .spawn(move || {
            let mut watch = WakeWatch::new(clocks());
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let resumed = RESUMED.swap(false, Ordering::SeqCst);
                match watch.beat(clocks(), resumed) {
                    Beat::Normal => {}
                    Beat::Woke { away, cause } => {
                        if tx.send(Event::Woke { slept: away, cause }).is_err() {
                            break;
                        }
                    }
                    Beat::HeldUp { by } => log::info!(
                        "tug was paused for ~{}s without the PC sleeping; not treating it as a wake",
                        by.as_secs()
                    ),
                }
            }
        });
    if let Err(e) = started {
        log::error!("couldn't start the wake watch: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These exercise the real Windows calls (no Bluetooth needed), so a wrong binding shows up here
    // rather than as a wake watch that silently never fires.

    #[test]
    fn the_clocks_read_and_show_no_sleep_while_awake() {
        let a = clocks();
        std::thread::sleep(Duration::from_millis(50));
        let b = clocks();
        assert!(a.with_sleep > 0 && a.awake > 0, "both clocks read: {a:?}");
        assert!(
            b.with_sleep > a.with_sleep && b.awake > a.awake,
            "both advance: {a:?} -> {b:?}"
        );
        assert!(
            crate::wake::slept_between(a, b) < Duration::from_millis(100),
            "no sleep over 50 ms"
        );
    }

    #[test]
    fn windows_accepts_the_resume_notification() {
        assert!(register_for_resume());
    }
}
