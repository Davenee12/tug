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

#[cfg(not(windows))]
pub fn set_text(_text: &str) -> Result<(), String> {
    Err("The clipboard is only supported on Windows".into())
}
