//! Put text on the Windows clipboard (one-time codes), and take an auto-copied code off it
//! again. Done natively because the webview's Clipboard API can be refused depending on focus
//! and permissions.

#[cfg(windows)]
pub fn set_text(text: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::DataTransfer::{Clipboard, DataPackage};

    let err = |e: windows::core::Error| e.message().to_string();
    let package = DataPackage::new().map_err(err)?;
    package.SetText(&HSTRING::from(text)).map_err(err)?;
    Clipboard::SetContent(&package).map_err(err)?;
    // Keep it on the clipboard even after tug exits.
    Clipboard::Flush().map_err(err)
}

/// Put something secret-ish on the clipboard (Tugboat's link, which carries the session secret):
/// kept out of Win+V clipboard history and cloud clipboard sync, and not flushed, so it goes
/// when tug exits.
#[cfg(windows)]
pub fn set_text_private(text: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::DataTransfer::{Clipboard, ClipboardContentOptions, DataPackage};

    let err = |e: windows::core::Error| e.message().to_string();
    let package = DataPackage::new().map_err(err)?;
    package.SetText(&HSTRING::from(text)).map_err(err)?;
    let options = ClipboardContentOptions::new().map_err(err)?;
    options.SetIsAllowedInHistory(false).map_err(err)?;
    options.SetIsRoamable(false).map_err(err)?;
    if Clipboard::SetContentWithOptions(&package, &options).map_err(err)? {
        Ok(())
    } else {
        Err("Windows didn't accept the clipboard content".into())
    }
}

/// Take an auto-copied code off the clipboard, but only if the clipboard still holds it
/// (`code_fill::should_clear`): something the person copied since is theirs. True if cleared.
///
/// Win32 rather than WinRT: it can run off the main thread (reading the text may ask tug's own
/// main thread to render it, which only works if that thread isn't the one waiting). Only the
/// first few characters are read: anything longer isn't a code.
#[cfg(windows)]
pub fn clear_if_holds(code: &str) -> Result<bool, String> {
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard};

    let err = |e: windows::core::Error| e.message().to_string();
    // Another app may have the clipboard open for a moment.
    let mut opened = Err(String::new());
    for _ in 0..10 {
        // SAFETY: no owner window; closed below on every path.
        opened = unsafe { OpenClipboard(None) }.map_err(err);
        if opened.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    opened?;
    let result = (|| {
        if !crate::code_fill::should_clear(clipboard_text_start().as_deref(), code) {
            return Ok(false);
        }
        // SAFETY: the clipboard is open on this thread.
        unsafe { EmptyClipboard() }.map_err(err)?;
        Ok(true)
    })();
    // SAFETY: opened above on this thread.
    let _ = unsafe { CloseClipboard() };
    result
}

/// The clipboard's text, if it's short (at most 32 UTF-16 units); a longer text comes back as
/// an empty string (not a code), no text as `None`. The clipboard must be open on this thread.
#[cfg(windows)]
fn clipboard_text_start() -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    const MAX_UNITS: usize = 32;
    // SAFETY: the handle comes from the open clipboard; reads stay within GlobalSize and stop at
    // the terminator; the memory is unlocked before returning.
    unsafe {
        IsClipboardFormatAvailable(CF_UNICODETEXT).ok()?;
        let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
        let memory = HGLOBAL(handle.0);
        let units = (GlobalSize(memory) / 2).min(MAX_UNITS + 1);
        let ptr = GlobalLock(memory) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let slice = std::slice::from_raw_parts(ptr, units);
        let text = match slice.iter().position(|&u| u == 0) {
            Some(end) => String::from_utf16_lossy(&slice[..end]),
            None => String::new(),
        };
        let _ = GlobalUnlock(memory);
        Some(text)
    }
}

#[cfg(not(windows))]
pub fn clear_if_holds(_code: &str) -> Result<bool, String> {
    Err("The clipboard is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn set_text(_text: &str) -> Result<(), String> {
    Err("The clipboard is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn set_text_private(_text: &str) -> Result<(), String> {
    Err("The clipboard is only supported on Windows".into())
}
