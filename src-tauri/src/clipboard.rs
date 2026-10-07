//! Put text on the Windows clipboard (one-time codes). Done natively because the
//! webview's Clipboard API can be refused depending on focus and permissions.

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

#[cfg(not(windows))]
pub fn set_text(_text: &str) -> Result<(), String> {
    Err("The clipboard is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn set_text_private(_text: &str) -> Result<(), String> {
    Err("The clipboard is only supported on Windows".into())
}
