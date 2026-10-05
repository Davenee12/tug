//! Spike 1 for tug v0.5.7 calling (see ROADMAP.md, "Spikes first").
//!
//! Question: can an unpackaged Win32 exe, given package identity by a *sparse* package, use
//! `Windows.ApplicationModel.Calls` (restricted capability `phoneLineTransportManagement`) to dial
//! through the owner's paired iPhone?
//!
//! The probe walks the whole flow and prints every step, ending in `RESULT: GO` or
//! `RESULT: NO-GO <why>`. It is throwaway code: no tests, no reuse in the app.
//!
//! Usage:
//!   tug-call-spike.exe                     probe only (never dials)
//!   tug-call-spike.exe --dial <number>     probe, then dial <number> through the iPhone
//!   options: --audio remote|local   where call audio goes after dialing (default: remote = phone)
//!            --timeout <secs>       how long to wait for phone lines (default: 15)
//!            --no-window            skip the foreground window (dialing will likely be refused)

#[cfg(not(windows))]
compile_error!("tug-call-spike is Windows-only");

use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::core::{Error, GUID, HSTRING};
use windows::ApplicationModel::Calls::{
    PhoneCallAudioDevice, PhoneCallManager, PhoneCallOperationStatus, PhoneLine,
    PhoneLineTransport, PhoneLineTransportDevice, PhoneLineWatcher, PhoneLineWatcherEventArgs,
};
use windows::ApplicationModel::Package;
use windows::Devices::Enumeration::{DeviceAccessStatus, DeviceInformation};
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, DrawTextW, EndPaint, COLOR_WINDOW, DT_LEFT, DT_WORDBREAK, HBRUSH, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetForegroundWindow,
    GetMessageW, LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    ShowWindow, TranslateMessage, CW_USEDEFAULT, IDC_ARROW, MSG, SW_SHOW, WM_CLOSE, WM_DESTROY,
    WM_PAINT, WNDCLASSW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

struct Args {
    dial: Option<String>,
    audio: PhoneCallAudioDevice,
    timeout: Duration,
    window: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        dial: None,
        audio: PhoneCallAudioDevice::RemoteDevice,
        timeout: Duration::from_secs(15),
        window: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dial" => args.dial = Some(it.next().ok_or("--dial needs a number")?),
            "--audio" => {
                args.audio = match it.next().as_deref() {
                    Some("remote") => PhoneCallAudioDevice::RemoteDevice,
                    Some("local") => PhoneCallAudioDevice::LocalDevice,
                    _ => return Err("--audio must be 'remote' or 'local'".into()),
                }
            }
            "--timeout" => {
                let secs: u64 = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or("--timeout needs a number of seconds")?;
                args.timeout = Duration::from_secs(secs);
            }
            "--no-window" => args.window = false,
            "-h" | "--help" => {
                println!("usage: tug-call-spike [--dial <number>] [--audio remote|local] [--timeout <secs>] [--no-window]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    let go = if args.window {
        run_with_window(args)
    } else {
        probe(&args, None)
    };
    std::process::exit(if go { 0 } else { 1 });
}

// ---------------------------------------------------------------------------------------------
// Foreground window. PhoneLine.Dial/DialWithResultAsync only work when the calling app is in the
// foreground, and a console app's window belongs to the terminal (conhost / Windows Terminal), not
// to us. So the probe shows its own small top-most window and runs the WinRT flow on a worker
// thread while the main thread pumps messages.
// ---------------------------------------------------------------------------------------------

const WINDOW_TEXT: &str =
    "tug call spike is running.\r\n\r\nKeep this window in front (click it if \
needed) until the console prints RESULT. It closes by itself.";

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                rect.left += 16;
                rect.top += 16;
                rect.right -= 16;
                let mut text: Vec<u16> = WINDOW_TEXT.encode_utf16().collect();
                DrawTextW(hdc, &mut text, &mut rect, DT_LEFT | DT_WORDBREAK);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn create_window() -> windows::core::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class_name = windows::core::w!("TugCallSpikeWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(Error::from_thread());
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            class_name,
            windows::core::w!("tug call spike"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            460,
            200,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        Ok(hwnd)
    }
}

fn run_with_window(args: Args) -> bool {
    let hwnd = match create_window() {
        Ok(h) => h,
        Err(e) => {
            println!(
                "WARN could not create the foreground window ({}); continuing without it",
                fmt_err(&e)
            );
            return probe(&args, None);
        }
    };
    // HWND is not Send; pass the raw value to the worker.
    let raw = hwnd.0 as isize;
    let worker = std::thread::spawn(move || {
        let hwnd = HWND(raw as *mut _);
        let go = probe(&args, Some(hwnd));
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        go
    });
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    worker.join().unwrap_or(false)
}

/// Waits (up to 20 s) for our window to be the foreground window. Returns whether it is.
fn ensure_foreground(hwnd: Option<HWND>) -> bool {
    let Some(hwnd) = hwnd else {
        println!("    foreground: no window (--no-window); Windows will likely refuse to dial");
        return false;
    };
    let is_fg = || unsafe { GetForegroundWindow() == hwnd };
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    if is_fg() {
        println!("    foreground: yes (the 'tug call spike' window is in front)");
        return true;
    }
    println!("    foreground: NO. Click the 'tug call spike' window now (waiting up to 20 s)...");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(250));
        if is_fg() {
            println!("    foreground: yes");
            return true;
        }
    }
    println!("    foreground: still no; dialing anyway so we see what Windows says");
    false
}

// ---------------------------------------------------------------------------------------------
// The probe
// ---------------------------------------------------------------------------------------------

fn fmt_err(e: &Error) -> String {
    let msg = e.message();
    let msg = msg.trim();
    if msg.is_empty() {
        format!("HRESULT {:#010x}", e.code().0 as u32)
    } else {
        format!("HRESULT {:#010x}: {msg}", e.code().0 as u32)
    }
}

fn access_name(s: DeviceAccessStatus) -> String {
    match s {
        DeviceAccessStatus::Unspecified => "Unspecified".into(),
        DeviceAccessStatus::Allowed => "Allowed".into(),
        DeviceAccessStatus::DeniedByUser => "DeniedByUser".into(),
        DeviceAccessStatus::DeniedBySystem => "DeniedBySystem".into(),
        other => format!("Unknown({})", other.0),
    }
}

fn op_status_name(s: PhoneCallOperationStatus) -> String {
    match s {
        PhoneCallOperationStatus::Succeeded => "Succeeded".into(),
        PhoneCallOperationStatus::OtherFailure => "OtherFailure".into(),
        PhoneCallOperationStatus::TimedOut => "TimedOut".into(),
        PhoneCallOperationStatus::ConnectionLost => "ConnectionLost".into(),
        PhoneCallOperationStatus::InvalidCallState => "InvalidCallState".into(),
        other => format!("Unknown({})", other.0),
    }
}

fn transport_name(t: PhoneLineTransport) -> String {
    match t {
        PhoneLineTransport::Cellular => "Cellular".into(),
        PhoneLineTransport::VoipApp => "VoipApp".into(),
        PhoneLineTransport::Bluetooth => "Bluetooth".into(),
        other => format!("Unknown({})", other.0),
    }
}

fn audio_name(a: PhoneCallAudioDevice) -> &'static str {
    match a {
        PhoneCallAudioDevice::RemoteDevice => "RemoteDevice (the phone)",
        PhoneCallAudioDevice::LocalDevice => "LocalDevice (this PC)",
        _ => "Unknown",
    }
}

fn verdict(go: Result<(), String>) -> bool {
    println!();
    match go {
        Ok(()) => {
            println!("RESULT: GO");
            true
        }
        Err(why) => {
            println!("RESULT: NO-GO {why}");
            false
        }
    }
}

struct LineInfo {
    id: GUID,
    line: PhoneLine,
    transport_device_id: String,
    transport: Option<PhoneLineTransport>,
    can_dial: bool,
}

enum WatchEvent {
    Added(GUID),
    Updated(GUID),
    Removed(GUID),
    EnumerationCompleted,
    Stopped,
}

fn probe(args: &Args, hwnd: Option<HWND>) -> bool {
    println!("== tug call spike (v0.5.7 Spike 1) ==");
    println!();

    // 1. Package identity -----------------------------------------------------------------
    println!("[1] package identity");
    let has_identity = match Package::Current().and_then(|p| p.Id()) {
        Ok(id) => {
            let full = id.FullName().map(|s| s.to_string()).unwrap_or_default();
            let family = id.FamilyName().map(|s| s.to_string()).unwrap_or_default();
            println!("    identity: {full}");
            println!("    family:   {family}");
            true
        }
        Err(e) => {
            println!("    no identity ({})", fmt_err(&e));
            println!("    (expected for the baseline run; after pack-and-register.ps1 this must show JordanLee.TugCallSpike)");
            false
        }
    };

    // 2. Phone line transport devices -----------------------------------------------------
    println!();
    println!("[2] PhoneLineTransportDevice enumeration");
    let devices = match PhoneLineTransportDevice::GetDeviceSelector()
        .and_then(|sel| DeviceInformation::FindAllAsyncAqsFilter(&sel)?.join())
    {
        Ok(d) => d,
        Err(e) => {
            println!("    enumeration failed: {}", fmt_err(&e));
            return verdict(Err(format!(
                "transport enumeration failed: {}",
                fmt_err(&e)
            )));
        }
    };
    let count = devices.Size().unwrap_or(0);
    println!("    found {count} device(s)");
    let mut first: Option<(String, String)> = None;
    for i in 0..count {
        let Ok(d) = devices.GetAt(i) else { continue };
        let name = d.Name().map(|s| s.to_string()).unwrap_or_default();
        let id = d.Id().map(|s| s.to_string()).unwrap_or_default();
        let enabled = d.IsEnabled().unwrap_or(false);
        println!("    - {name}  enabled={enabled}");
        println!("      id: {id}");
        if first.is_none() && enabled {
            first = Some((name, id));
        }
    }
    let Some((dev_name, dev_id)) = first else {
        return verdict(Err(
            "no enabled phone line transport (is Handsfree Telephony ticked for the iPhone?)"
                .into(),
        ));
    };
    if count > 1 {
        println!("    using the first enabled one: {dev_name}");
    }

    // 3. FromId ------------------------------------------------------------------------------
    println!();
    println!("[3] PhoneLineTransportDevice::FromId");
    let device = match PhoneLineTransportDevice::FromId(&HSTRING::from(dev_id.as_str())) {
        Ok(d) => {
            let transport = d
                .Transport()
                .map(transport_name)
                .unwrap_or_else(|e| fmt_err(&e));
            println!("    ok (transport: {transport})");
            d
        }
        Err(e) => {
            println!("    failed: {}", fmt_err(&e));
            return verdict(Err(format!("FromId failed: {}", fmt_err(&e))));
        }
    };

    // From here on we keep going after failures so one run shows as much as possible.
    let mut failures: Vec<String> = Vec::new();

    // 4. RequestAccessAsync ----------------------------------------------------------------
    println!();
    println!("[4] RequestAccessAsync");
    let access = device.RequestAccessAsync().and_then(|op| op.join());
    let allowed = match &access {
        Ok(s) => {
            println!("    status: {}", access_name(*s));
            if *s == DeviceAccessStatus::DeniedBySystem {
                if has_identity {
                    println!("    hint: turn on Settings > Privacy & security > Phone calls (and \"Let apps make phone calls\"), then run again");
                } else {
                    println!("    hint: expected without package identity; the restricted capability needs the sparse package");
                }
            }
            *s == DeviceAccessStatus::Allowed
        }
        Err(e) => {
            println!("    failed: {}", fmt_err(e));
            false
        }
    };
    if !allowed {
        failures.push(match &access {
            Ok(s) => format!("access {}", access_name(*s)),
            Err(e) => format!("RequestAccessAsync failed: {}", fmt_err(e)),
        });
    }

    // 5. Registration ----------------------------------------------------------------------
    // RegisterApp and ConnectAsync change which app owns the phone line, so (like Sefirah) they
    // only run once access is Allowed. That keeps the no-identity baseline run read-only.
    println!();
    println!("[5] IsRegistered / RegisterApp");
    match device.IsRegistered() {
        Ok(r) => println!("    IsRegistered (before): {r}"),
        Err(e) => println!("    IsRegistered (before) failed: {}", fmt_err(&e)),
    }
    let registered = if !allowed {
        println!("    RegisterApp: skipped (access is not Allowed)");
        false
    } else {
        match device.RegisterApp() {
            Ok(()) => println!("    RegisterApp: ok"),
            Err(e) => println!("    RegisterApp failed: {}", fmt_err(&e)),
        }
        match device.IsRegistered() {
            Ok(r) => {
                println!("    IsRegistered (after):  {r}");
                if !r {
                    println!("    hint: another app (Phone Link?) may own this phone line");
                }
                r
            }
            Err(e) => {
                println!("    IsRegistered (after) failed: {}", fmt_err(&e));
                false
            }
        }
    };
    if allowed && !registered {
        failures.push("not registered as the line's app".into());
    }

    // 6. ConnectAsync ----------------------------------------------------------------------
    println!();
    println!("[6] ConnectAsync");
    let connected = if !allowed {
        println!("    skipped (access is not Allowed)");
        false
    } else {
        match device.ConnectAsync().and_then(|op| op.join()) {
            Ok(c) => {
                println!("    connected: {c}");
                c
            }
            Err(e) => {
                println!("    failed: {}", fmt_err(&e));
                false
            }
        }
    };
    if allowed && !connected {
        failures.push("ConnectAsync did not connect".into());
    }

    // 7. Phone lines -----------------------------------------------------------------------
    println!();
    println!("[7] PhoneCallManager::RequestStoreAsync -> PhoneLineWatcher");
    let lines = match watch_lines(&dev_id, args.timeout) {
        Ok(l) => l,
        Err(e) => {
            println!("    failed: {}", fmt_err(&e));
            failures.push(format!("line watcher failed: {}", fmt_err(&e)));
            Vec::new()
        }
    };
    let ours = pick_line(&lines, &dev_id);
    match &ours {
        Some((l, how)) => println!("    line for our transport: {:?} ({how})", l.id),
        None => {
            println!("    no dialable line for our transport");
            failures.push("no line with CanDial for the iPhone transport".into());
        }
    }

    // 8. Dial (optional) -------------------------------------------------------------------
    if let Some(number) = &args.dial {
        println!();
        println!("[8] DialWithResultAsync({number:?}, \"tug spike\")");
        match &ours {
            Some((l, _)) if failures.is_empty() => {
                if let Err(why) = dial(&l.line, number, args.audio, hwnd) {
                    failures.push(why);
                }
            }
            _ => println!("    skipped: the probe is already NO-GO, not dialing"),
        }
    }

    if failures.is_empty() {
        verdict(Ok(()))
    } else {
        verdict(Err(failures.join("; ")))
    }
}

/// Starts a PhoneLineWatcher and collects lines until enumeration completes. If our transport's
/// line hasn't appeared by then (it can show up only after ConnectAsync settles), keeps listening
/// until `timeout`.
fn watch_lines(dev_id: &str, timeout: Duration) -> windows::core::Result<Vec<LineInfo>> {
    let store = PhoneCallManager::RequestStoreAsync()?.join()?;
    let watcher = store.RequestLineWatcher()?;
    let (tx, rx) = mpsc::channel::<WatchEvent>();

    let t = tx.clone();
    watcher.LineAdded(&TypedEventHandler::<
        PhoneLineWatcher,
        PhoneLineWatcherEventArgs,
    >::new(move |_, a| {
        if let Some(a) = a.as_ref() {
            let _ = t.send(WatchEvent::Added(a.LineId()?));
        }
        Ok(())
    }))?;
    let t = tx.clone();
    watcher.LineUpdated(&TypedEventHandler::<
        PhoneLineWatcher,
        PhoneLineWatcherEventArgs,
    >::new(move |_, a| {
        if let Some(a) = a.as_ref() {
            let _ = t.send(WatchEvent::Updated(a.LineId()?));
        }
        Ok(())
    }))?;
    let t = tx.clone();
    watcher.LineRemoved(&TypedEventHandler::<
        PhoneLineWatcher,
        PhoneLineWatcherEventArgs,
    >::new(move |_, a| {
        if let Some(a) = a.as_ref() {
            let _ = t.send(WatchEvent::Removed(a.LineId()?));
        }
        Ok(())
    }))?;
    let t = tx.clone();
    watcher.EnumerationCompleted(&TypedEventHandler::<
        PhoneLineWatcher,
        windows::core::IInspectable,
    >::new(move |_, _| {
        let _ = t.send(WatchEvent::EnumerationCompleted);
        Ok(())
    }))?;
    let t = tx;
    watcher.Stopped(&TypedEventHandler::<
        PhoneLineWatcher,
        windows::core::IInspectable,
    >::new(move |_, _| {
        let _ = t.send(WatchEvent::Stopped);
        Ok(())
    }))?;
    watcher.Start()?;

    let mut lines: Vec<LineInfo> = Vec::new();
    let mut enumerated = false;
    let deadline = Instant::now() + timeout;
    loop {
        let have_ours = lines
            .iter()
            .any(|l| l.can_dial && l.transport_device_id.eq_ignore_ascii_case(dev_id));
        if enumerated && have_ours {
            break;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            println!(
                "    (stopped waiting after {}s; enumeration completed: {enumerated})",
                timeout.as_secs()
            );
            break;
        }
        match rx.recv_timeout(left) {
            Ok(WatchEvent::Added(id)) | Ok(WatchEvent::Updated(id)) => match describe_line(id) {
                Ok(info) => {
                    lines.retain(|l| l.id != info.id);
                    lines.push(info);
                }
                Err(e) => println!("    line {id:?}: FromIdAsync failed: {}", fmt_err(&e)),
            },
            Ok(WatchEvent::Removed(id)) => {
                println!("    line removed: {id:?}");
                lines.retain(|l| l.id != id);
            }
            Ok(WatchEvent::EnumerationCompleted) => {
                println!("    enumeration completed ({} line(s) so far)", lines.len());
                enumerated = true;
            }
            Ok(WatchEvent::Stopped) => {
                println!("    watcher stopped");
                break;
            }
            Err(_) => continue, // timeout: loop re-checks the deadline
        }
    }
    let _ = watcher.Stop();
    Ok(lines)
}

fn describe_line(id: GUID) -> windows::core::Result<LineInfo> {
    let line = PhoneLine::FromIdAsync(id)?.join()?;
    let transport_device_id = line
        .TransportDeviceId()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let display_name = line
        .DisplayName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let transport = line.Transport().ok();
    let can_dial = line.CanDial().unwrap_or(false);
    println!("    line {id:?}");
    println!("      DisplayName:       {display_name}");
    println!(
        "      Transport:         {}",
        transport.map(transport_name).unwrap_or_else(|| "?".into())
    );
    println!("      TransportDeviceId: {transport_device_id}");
    println!("      CanDial:           {can_dial}");
    Ok(LineInfo {
        id,
        line,
        transport_device_id,
        transport,
        can_dial,
    })
}

/// Prefers the dialable line whose TransportDeviceId is our transport; falls back to the only
/// dialable Bluetooth line if ids don't match (and says so).
fn pick_line<'a>(lines: &'a [LineInfo], dev_id: &str) -> Option<(&'a LineInfo, &'static str)> {
    if let Some(l) = lines
        .iter()
        .find(|l| l.can_dial && l.transport_device_id.eq_ignore_ascii_case(dev_id))
    {
        return Some((l, "matched by TransportDeviceId"));
    }
    let bt: Vec<&LineInfo> = lines
        .iter()
        .filter(|l| l.can_dial && l.transport == Some(PhoneLineTransport::Bluetooth))
        .collect();
    if bt.len() == 1 {
        return Some((
            bt[0],
            "WARN: matched as the only dialable Bluetooth line, id differs",
        ));
    }
    None
}

fn dial(
    line: &PhoneLine,
    number: &str,
    audio: PhoneCallAudioDevice,
    hwnd: Option<HWND>,
) -> Result<(), String> {
    ensure_foreground(hwnd);
    let result = line
        .DialWithResultAsync(&HSTRING::from(number), &HSTRING::from("tug spike"))
        .and_then(|op| op.join())
        .map_err(|e| {
            println!("    DialWithResultAsync failed: {}", fmt_err(&e));
            format!("DialWithResultAsync failed: {}", fmt_err(&e))
        })?;
    let status = result.DialCallStatus().map_err(|e| fmt_err(&e))?;
    println!("    DialCallStatus: {}", op_status_name(status));
    match result.DialedCall() {
        Ok(call) => {
            let call_id = call.CallId().map(|s| s.to_string()).unwrap_or_default();
            println!("    call object returned (CallId: {call_id})");
            match call.ChangeAudioDeviceAsync(audio).and_then(|op| op.join()) {
                Ok(s) => println!(
                    "    ChangeAudioDeviceAsync({}): {}",
                    audio_name(audio),
                    op_status_name(s)
                ),
                Err(e) => println!(
                    "    ChangeAudioDeviceAsync({}) failed: {}",
                    audio_name(audio),
                    fmt_err(&e)
                ),
            }
            match call.AudioDevice() {
                Ok(a) => println!("    call AudioDevice now: {}", audio_name(a)),
                Err(e) => println!("    call AudioDevice read failed: {}", fmt_err(&e)),
            }
        }
        Err(e) => println!("    no call object returned ({})", fmt_err(&e)),
    }
    if status == PhoneCallOperationStatus::Succeeded {
        println!("    the call should now be ringing out on the iPhone; hang up there");
        Ok(())
    } else {
        Err(format!("dial status {}", op_status_name(status)))
    }
}
