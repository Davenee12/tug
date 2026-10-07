//! "Add tug to PATH" in Settings › Developer tools: puts tug's `bin` folder (which holds only
//! `tug.exe`, the command) on the user's PATH so `tug` works in any new terminal. Only ever on a
//! click, only the user's own PATH (HKCU\Environment, no admin), and "Remove from PATH" undoes
//! it. The installer never touches PATH.

/// One PATH entry compared the way Windows treats them: case-insensitive, trailing `\` ignored.
fn same_dir(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_matches('"').trim_end_matches(['\\', '/']).to_lowercase();
    !a.trim().is_empty() && norm(a) == norm(b)
}

pub fn contains(path: &str, dir: &str) -> bool {
    path.split(';').any(|p| same_dir(p, dir))
}

/// PATH with `dir` appended, or `None` if it's already there.
pub fn with_dir(path: &str, dir: &str) -> Option<String> {
    if contains(path, dir) {
        return None;
    }
    let trimmed = path.trim_end_matches(';');
    Some(if trimmed.is_empty() {
        dir.to_string()
    } else {
        format!("{trimmed};{dir}")
    })
}

/// PATH without `dir`, or `None` if it wasn't there. Every other entry is kept as written.
pub fn without_dir(path: &str, dir: &str) -> Option<String> {
    if !contains(path, dir) {
        return None;
    }
    Some(
        path.split(';')
            .filter(|p| !same_dir(p, dir))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[cfg(windows)]
mod registry {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, LPARAM, WPARAM};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
        KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: opened by RegOpenKeyExW below.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(write: bool) -> Result<Key, String> {
        let mut key = HKEY::default();
        let access = if write {
            KEY_QUERY_VALUE | KEY_SET_VALUE
        } else {
            KEY_QUERY_VALUE
        };
        // SAFETY: plain FFI; the key is closed on drop.
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w!("Environment"), None, access, &mut key) };
        if r.is_err() {
            return Err(format!("couldn't open your environment settings ({})", r.0));
        }
        Ok(Key(key))
    }

    /// The user's own PATH exactly as stored (unexpanded), or empty if there isn't one.
    pub fn read() -> Result<String, String> {
        let key = open(false)?;
        let mut kind = REG_VALUE_TYPE::default();
        let mut len = 0u32;
        // SAFETY: size query, then a read into a buffer of that size.
        unsafe {
            let r = RegQueryValueExW(key.0, w!("Path"), None, Some(&mut kind), None, Some(&mut len));
            if r == ERROR_FILE_NOT_FOUND {
                return Ok(String::new());
            }
            if r.is_err() {
                return Err(format!("couldn't read PATH ({})", r.0));
            }
            let mut buf = vec![0u16; (len as usize).div_ceil(2) + 1];
            let mut len = (buf.len() * 2) as u32;
            let r = RegQueryValueExW(
                key.0,
                w!("Path"),
                None,
                Some(&mut kind),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut len),
            );
            if r.is_err() {
                return Err(format!("couldn't read PATH ({})", r.0));
            }
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                return Err("PATH isn't text".into());
            }
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Ok(String::from_utf16_lossy(&buf[..n]))
        }
    }

    /// Save the user's PATH (as REG_EXPAND_SZ, so `%VARS%` keep working) and tell running apps,
    /// so terminals opened from now on see it.
    pub fn write(value: &str) -> Result<(), String> {
        let key = open(true)?;
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        // SAFETY: plain FFI with a correctly sized, NUL-terminated buffer.
        let r = unsafe { RegSetValueExW(key.0, w!("Path"), None, REG_EXPAND_SZ, Some(bytes)) };
        if r.is_err() {
            return Err(format!("couldn't save PATH ({})", r.0));
        }
        let env = HSTRING::from("Environment");
        // SAFETY: broadcast with a 2 s cap per window, so a hung app can't hang tug.
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(PCWSTR(env.as_ptr()).0 as isize),
                SMTO_ABORTIFHUNG,
                2000,
                None,
            );
        }
        Ok(())
    }
}

/// Whether `dir` is on the user's PATH.
#[cfg(windows)]
pub fn is_on_path(dir: &str) -> bool {
    registry::read().map(|p| contains(&p, dir)).unwrap_or(false)
}

#[cfg(windows)]
pub fn add(dir: &str) -> Result<(), String> {
    match with_dir(&registry::read()?, dir) {
        Some(p) => registry::write(&p),
        None => Ok(()),
    }
}

#[cfg(windows)]
pub fn remove(dir: &str) -> Result<(), String> {
    match without_dir(&registry::read()?, dir) {
        Some(p) => registry::write(&p),
        None => Ok(()),
    }
}

#[cfg(not(windows))]
pub fn is_on_path(_dir: &str) -> bool {
    false
}
#[cfg(not(windows))]
pub fn add(_dir: &str) -> Result<(), String> {
    Err("Only on Windows".into())
}
#[cfg(not(windows))]
pub fn remove(_dir: &str) -> Result<(), String> {
    Err("Only on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\me\AppData\Local\tug\bin";

    #[test]
    fn adds_once() {
        assert_eq!(with_dir("", DIR).as_deref(), Some(DIR));
        let p = r"%USERPROFILE%\.cargo\bin;C:\tools;";
        let added = with_dir(p, DIR).unwrap();
        assert_eq!(added, format!(r"%USERPROFILE%\.cargo\bin;C:\tools;{DIR}"));
        assert_eq!(with_dir(&added, DIR), None);
        // Already there, written differently.
        assert_eq!(with_dir(r"c:\users\ME\appdata\local\tug\bin\;C:\x", DIR), None);
    }

    #[test]
    fn removes_only_its_own_entry() {
        let p = format!(r"C:\a;{DIR};%LOCALAPPDATA%\b;{DIR}\");
        assert_eq!(without_dir(&p, DIR).as_deref(), Some(r"C:\a;%LOCALAPPDATA%\b"));
        assert_eq!(without_dir(r"C:\a;C:\b", DIR), None);
        assert!(!contains(r"C:\a;;C:\b", ""));
    }

    #[test]
    fn quoted_entries_count() {
        assert!(contains(&format!("\"{DIR}\";C:\\x"), DIR));
    }
}
