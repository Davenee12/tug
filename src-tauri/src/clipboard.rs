//! Put text on the Windows clipboard (one-time codes, text from Tugboat), take an auto-copied code
//! off it again, and read it when the person sends their clipboard to the phone. Done natively
//! because the webview's Clipboard API can be refused depending on focus and permissions.

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

/// Put something secret-ish on the clipboard (verification codes, whether auto-copied or copied
/// by hand; Tugboat's link, which carries the session secret): kept out of Win+V clipboard history
/// and cloud clipboard sync, and not flushed, so it goes when tug exits.
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

/// What's on the clipboard, for Tugboat's "Send clipboard to iPhone": its text (refused unread
/// past `max_text_bytes`), else a picture as the clipboard's `CF_DIB`, else nothing or something
/// tug can't send. Only called when the person asks; contents are never logged.
///
/// Win32 rather than WinRT, off the main thread, for the same reason as `clear_if_holds`.
#[cfg(windows)]
pub fn read_for_sharing(max_text_bytes: usize) -> Result<crate::tugboat::clip::Reading, String> {
    use crate::tugboat::clip::{Reading, MAX_IMAGE_PIXELS};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, CountClipboardFormats, IsClipboardFormatAvailable, OpenClipboard,
    };

    const CF_DIB: u32 = 8;
    const CF_UNICODETEXT: u32 = 13;
    // A 32-bit bitmap at the pixel limit, plus room for its header and masks.
    let max_dib = MAX_IMAGE_PIXELS as usize * 4 + 4096;
    let err = |e: windows::core::Error| e.message().to_string();
    let mut opened = Err(String::new());
    for _ in 0..10 {
        // SAFETY: no owner window; closed below on every path.
        opened = unsafe { OpenClipboard(None) }.map_err(err);
        if opened.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    opened.map_err(|_| "Another app is using the clipboard. Try again in a moment.".to_string())?;
    // SAFETY: the clipboard is open on this thread for everything below.
    let reading = unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() {
            // Each UTF-16 unit is at least one UTF-8 byte, so no terminator within this many units
            // means more than `max_text_bytes`.
            match clipboard_units(CF_UNICODETEXT, max_text_bytes + 1) {
                Some(Some(units)) => Reading::Text(String::from_utf16_lossy(&units)),
                Some(None) => Reading::TooLong,
                None => Reading::Other,
            }
        } else if IsClipboardFormatAvailable(CF_DIB).is_ok() {
            match clipboard_bytes(CF_DIB, max_dib) {
                Some(Some(bytes)) => Reading::Image(bytes),
                Some(None) => Reading::ImageTooBig,
                None => Reading::Other,
            }
        } else if CountClipboardFormats() == 0 {
            Reading::Empty
        } else {
            Reading::Other
        }
    };
    // SAFETY: opened above on this thread.
    let _ = unsafe { CloseClipboard() };
    Ok(reading)
}

/// A NUL-terminated UTF-16 clipboard format, up to `max_units` (`Some(None)` if it's longer).
/// `None` if Windows wouldn't hand it over.
///
/// # Safety
/// The clipboard must be open on this thread.
#[cfg(windows)]
unsafe fn clipboard_units(format: u32, max_units: usize) -> Option<Option<Vec<u16>>> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::GetClipboardData;
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    let handle = GetClipboardData(format).ok()?;
    let memory = HGLOBAL(handle.0);
    let units = GlobalSize(memory) / 2;
    let ptr = GlobalLock(memory) as *const u16;
    if ptr.is_null() {
        return None;
    }
    // Reads stay within GlobalSize and stop at the terminator.
    let slice = std::slice::from_raw_parts(ptr, units.min(max_units + 1));
    let text = match slice.iter().position(|&u| u == 0) {
        Some(end) => Some(slice[..end].to_vec()),
        // No terminator anywhere in the block: take it all, if that fits.
        None if units <= max_units => Some(slice.to_vec()),
        None => None,
    };
    let _ = GlobalUnlock(memory);
    Some(text)
}

/// A clipboard format's bytes, up to `max` (`Some(None)` if it's bigger). `None` if Windows
/// wouldn't hand it over.
///
/// # Safety
/// The clipboard must be open on this thread.
#[cfg(windows)]
unsafe fn clipboard_bytes(format: u32, max: usize) -> Option<Option<Vec<u8>>> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::GetClipboardData;
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    let handle = GetClipboardData(format).ok()?;
    let memory = HGLOBAL(handle.0);
    let size = GlobalSize(memory);
    if size > max {
        return Some(None);
    }
    let ptr = GlobalLock(memory) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(memory);
    Some(Some(bytes))
}

#[cfg(not(windows))]
pub fn read_for_sharing(_max_text_bytes: usize) -> Result<crate::tugboat::clip::Reading, String> {
    Err("The clipboard is only supported on Windows".into())
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
