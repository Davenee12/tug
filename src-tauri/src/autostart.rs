//! Start with Windows. tauri-plugin-autostart (auto-launch) writes the HKCU Run entry as
//! `C:\path\tug.exe --minimized` with the path unquoted; a path with a space in it (a user folder
//! such as `C:\Users\Sam Lee\...`) then depends on Windows guessing where the program name ends.
//! After the plugin enables autostart, tug rewrites its own entry with the path quoted.

/// The Run key the plugin writes to (under HKCU).
#[cfg_attr(not(windows), allow(dead_code))]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The value name: the plugin uses the app's product name.
#[cfg_attr(not(windows), allow(dead_code))]
const VALUE_NAME: &str = "tug";

/// The Run command for `exe` with `args`: `"C:\path\tug.exe" --minimized`.
pub fn run_command(exe: &str, args: &[&str]) -> String {
    let mut out = format!("\"{}\"", exe.trim().trim_matches('"'));
    for arg in args {
        out.push(' ');
        out.push_str(arg);
    }
    out
}

/// Rewrite tug's HKCU Run entry with the exe path quoted. Leaves it alone when there's no entry
/// (autostart off, or written somewhere else) or it's already right.
#[cfg(windows)]
pub fn quote_run_value() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let want = run_command(&exe.display().to_string(), &[crate::MINIMIZED_ARG]);
    let key = registry::Key::open_run()?;
    match key.read(VALUE_NAME)? {
        Some(current) if current != want => key.write(VALUE_NAME, &want),
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
pub fn quote_run_value() -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
mod registry {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
        KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };

    pub struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: opened by RegOpenKeyExW below.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    impl Key {
        pub fn open_run() -> Result<Key, String> {
            let mut key = HKEY::default();
            // SAFETY: plain FFI; the key is closed on drop.
            let r = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(super::RUN_KEY),
                    None,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    &mut key,
                )
            };
            if r.is_err() {
                return Err(format!("couldn't open the Run key ({})", r.0));
            }
            Ok(Key(key))
        }

        /// A text value, or None if it isn't there.
        pub fn read(&self, name: &str) -> Result<Option<String>, String> {
            let name = HSTRING::from(name);
            let mut kind = REG_VALUE_TYPE::default();
            let mut len = 0u32;
            // SAFETY: size query, then a read into a buffer of that size.
            unsafe {
                let r = RegQueryValueExW(self.0, &name, None, Some(&mut kind), None, Some(&mut len));
                if r == ERROR_FILE_NOT_FOUND {
                    return Ok(None);
                }
                if r.is_err() {
                    return Err(format!("couldn't read the Run entry ({})", r.0));
                }
                let mut buf = vec![0u16; (len as usize).div_ceil(2) + 1];
                let mut len = (buf.len() * 2) as u32;
                let r = RegQueryValueExW(
                    self.0,
                    &name,
                    None,
                    Some(&mut kind),
                    Some(buf.as_mut_ptr() as *mut u8),
                    Some(&mut len),
                );
                if r.is_err() {
                    return Err(format!("couldn't read the Run entry ({})", r.0));
                }
                if kind != REG_SZ && kind != REG_EXPAND_SZ {
                    return Ok(None);
                }
                let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                Ok(Some(String::from_utf16_lossy(&buf[..n])))
            }
        }

        pub fn write(&self, name: &str, value: &str) -> Result<(), String> {
            let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: a view of the u16 buffer as bytes, alive for the call.
            let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
            // SAFETY: plain FFI with a correctly sized, NUL-terminated buffer.
            let r = unsafe { RegSetValueExW(self.0, &HSTRING::from(name), None, REG_SZ, Some(bytes)) };
            if r.is_err() {
                return Err(format!("couldn't write the Run entry ({})", r.0));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_quotes_the_path() {
        assert_eq!(
            run_command(r"C:\Users\Sam Lee\AppData\Local\tug\tug.exe", &["--minimized"]),
            r#""C:\Users\Sam Lee\AppData\Local\tug\tug.exe" --minimized"#
        );
        // Already quoted, or no args: still exactly one pair of quotes.
        assert_eq!(
            run_command(r#""C:\Program Files\tug\tug.exe""#, &[]),
            r#""C:\Program Files\tug\tug.exe""#
        );
    }
}
