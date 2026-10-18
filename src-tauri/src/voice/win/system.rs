//! Small read-only Windows queries and the keystrokes voice typing sends.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::voice::text::Stroke;

/// The window in front and the program it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    pub hwnd: isize,
    /// The program's full path (for its file name only; never logged).
    pub exe: String,
}

/// The window in front, if there is one.
pub fn foreground() -> Option<Foreground> {
    // SAFETY: read-only Win32 queries with valid out-pointers; the process handle is closed.
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut exe = String::new();
        if pid != 0 {
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                    exe = String::from_utf16_lossy(&buf[..len as usize]);
                }
                let _ = CloseHandle(process);
            }
        }
        Some(Foreground {
            hwnd: hwnd.0 as isize,
            exe,
        })
    }
}

/// A string value from the registry, or `None` if it isn't there.
fn reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let key: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let value: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let mut buf = [0u16; 64];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: NUL-terminated names and a buffer of `size` bytes.
    let rc = unsafe {
        RegGetValueW(
            root,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if rc.is_err() {
        return None;
    }
    let chars = (size as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars]);
    Some(s.trim_end_matches('\0').to_string())
}

const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

/// Whether Windows' privacy settings keep desktop apps like tug away from the microphone.
pub fn mic_blocked() -> bool {
    crate::voice::mic_blocked(
        reg_string(HKEY_LOCAL_MACHINE, CONSENT, "Value").as_deref(),
        reg_string(HKEY_CURRENT_USER, CONSENT, "Value").as_deref(),
        reg_string(HKEY_CURRENT_USER, &format!(r"{CONSENT}\NonPackaged"), "Value").as_deref(),
    )
}

/// Windows' display language (an LCID, 0x409 for English, United States).
pub fn ui_language() -> u16 {
    // SAFETY: a read-only query.
    unsafe { GetUserDefaultUILanguage() }
}

/// Whether a key is down right now.
pub fn key_down(vk: u8) -> bool {
    // SAFETY: a read-only key-state query.
    (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000) != 0
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> bool {
    if inputs.is_empty() {
        return true;
    }
    // SAFETY: a slice of fully initialised keyboard INPUTs and their size.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

/// A key with no meaning (0xE8 is unassigned), pressed and let go while the shortcut's Alt is
/// down, so letting go of Alt doesn't open the focused app's menu bar.
pub fn mask_menu_key() {
    let vk = VIRTUAL_KEY(0xE8);
    send(&[key(vk, 0, KEYBD_EVENT_FLAGS(0)), key(vk, 0, KEYEVENTF_KEYUP)]);
}

/// Type `strokes` into the focused window: characters as Unicode keystrokes (as code fill
/// types codes), line breaks as Shift+Enter. True if Windows took them all.
pub fn type_strokes(strokes: &[Stroke]) -> bool {
    let mut inputs = Vec::with_capacity(strokes.len() * 2);
    for s in strokes {
        match *s {
            Stroke::Unit(u) => {
                inputs.push(key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE));
                inputs.push(key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
            Stroke::NewLine => {
                inputs.push(key(VK_SHIFT, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
                inputs.push(key(VK_SHIFT, 0, KEYEVENTF_KEYUP));
            }
        }
    }
    send(&inputs)
}

/// The physical memory free right now, in bytes, or `None` if Windows doesn't say.
pub fn available_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: a read-only query into a struct whose length field is set, as the API requires.
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok().map(|()| m.ullAvailPhys)
}

/// The default microphone's name and the sample rate Windows mixes it at, if there is one. Must
/// run on a COM thread (the voice worker). Read-only: the device isn't opened for recording.
pub fn default_mic() -> Option<(String, u32)> {
    use windows::core::GUID;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{eCapture, eConsole, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator};
    // PKEY_Device_FriendlyName (functiondiscoverykeys_devpkey.h), spelled out rather than
    // pulling in the whole FunctionDiscovery feature for one constant.
    #[allow(non_upper_case_globals)]
    const PKEY_Device_FriendlyName: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToBSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

    // SAFETY: COM calls on a COM thread with valid arguments; the PROPVARIANT is cleared and the
    // mix format freed with CoTaskMemFree, as their APIs require.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let mic = devices.GetDefaultAudioEndpoint(eCapture, eConsole).ok()?;
        let name = mic
            .OpenPropertyStore(STGM_READ)
            .and_then(|store| {
                let mut value = store.GetValue(&PKEY_Device_FriendlyName)?;
                let name = PropVariantToBSTR(&value).map(|b| b.to_string());
                let _ = PropVariantClear(&mut value);
                name
            })
            .unwrap_or_default();
        let rate = mic
            .Activate::<IAudioClient>(CLSCTX_ALL, None)
            .and_then(|client| client.GetMixFormat())
            .map(|format| {
                let rate = if format.is_null() { 0 } else { (*format).nSamplesPerSec };
                CoTaskMemFree(Some(format as *const core::ffi::c_void));
                rate
            })
            .unwrap_or(0);
        Some((name, rate))
    }
}
