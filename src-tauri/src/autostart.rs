//! Start with Windows (Settings › General): tug's own entry in the signed-in user's Run key
//! (`HKCU\...\Run`, value `tug`), plus Task Manager's per-app startup flag next to it
//! (`...\Explorer\StartupApproved\Run`). Only ever this user's keys: never `HKLM`, so it's one
//! person's choice, it can always be turned off without an administrator, and the uninstaller
//! (which runs as the user) removes it.
//!
//! tug used tauri-plugin-autostart before, which tries `HKLM` first. A tug that happened to run
//! elevated (Run as administrator) wrote an unquoted machine-wide entry that started this user's
//! tug for every account, couldn't be turned off from the switch, and outlived uninstalling. When
//! tug runs elevated it moves such an entry, if it points at this tug, back to this user.
//!
//! The rules (`is_enabled`, `enable`, `disable`, `migrate_machine_entry`) work on a `Registry`
//! trait, so they're tested against an in-memory fake; `WinRegistry` is the thin Windows side.

/// The Run key, under `HKCU` (and, for an old machine-wide entry, `HKLM`).
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Task Manager's Startup tab: whether each Run entry is enabled.
pub const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// The value name (the product name, as the plugin used, so existing entries carry on).
pub const VALUE_NAME: &str = "tug";
/// Task Manager's "enabled" flag (what Windows itself writes for an enabled entry).
const APPROVED_ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Which registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// Why a registry call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegError {
    /// Needs an administrator (an `HKLM` write from a non-elevated tug).
    AccessDenied,
    Other(String),
}

impl std::fmt::Display for RegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegError::AccessDenied => f.write_str("access denied"),
            RegError::Other(e) => f.write_str(e),
        }
    }
}

/// The few registry operations autostart needs. A missing key or value reads as `None` and
/// deletes as success.
pub trait Registry {
    fn read_string(&self, hive: Hive, key: &str, name: &str) -> Result<Option<String>, RegError>;
    fn read_binary(&self, hive: Hive, key: &str, name: &str) -> Result<Option<Vec<u8>>, RegError>;
    /// Creates the key if needed.
    fn write_string(&self, hive: Hive, key: &str, name: &str, value: &str) -> Result<(), RegError>;
    /// Creates the key if needed.
    fn write_binary(&self, hive: Hive, key: &str, name: &str, value: &[u8]) -> Result<(), RegError>;
    fn delete(&self, hive: Hive, key: &str, name: &str) -> Result<(), RegError>;
}

/// The Run command for `exe` with `args`: `"C:\path\tug.exe" --minimized`.
pub fn run_command(exe: &str, args: &[&str]) -> String {
    let mut out = format!("\"{}\"", exe.trim().trim_matches('"'));
    for arg in args {
        out.push(' ');
        out.push_str(arg);
    }
    out
}

/// Whether a Run value launches `exe`: quoted (`"C:\x\tug.exe" --minimized`) or the plugin's
/// unquoted form (`C:\Users\Sam Lee\...\tug.exe --minimized`). Case-insensitive, like Windows.
pub fn points_at(value: &str, exe: &str) -> bool {
    let value = value.trim();
    let exe = exe.trim().trim_matches('"').to_lowercase();
    if exe.is_empty() {
        return false;
    }
    if let Some(rest) = value.strip_prefix('"') {
        return rest.split('"').next().is_some_and(|p| p.to_lowercase() == exe);
    }
    let lower = value.to_lowercase();
    lower
        .strip_prefix(&exe)
        .is_some_and(|after| after.is_empty() || after.starts_with(' '))
}

/// Task Manager marks a disabled entry with an odd first byte (3 or 7, followed by when it was
/// disabled); no flag at all means enabled.
fn approved(flag: Option<&[u8]>) -> bool {
    match flag {
        None | Some([]) => true,
        Some(bytes) => bytes[0] & 1 == 0,
    }
}

fn other(e: RegError) -> String {
    format!("couldn't change Start with Windows ({e})")
}

/// The machine-wide entry, if there is one and it starts this tug.
fn machine_entry_for(reg: &dyn Registry, exe: &str) -> Result<bool, RegError> {
    Ok(reg
        .read_string(Hive::LocalMachine, RUN_KEY, VALUE_NAME)?
        .is_some_and(|v| points_at(&v, exe)))
}

/// Whether tug starts when this user signs in: this user's entry (unless switched off in Task
/// Manager), or an old machine-wide one that starts this tug (until it's moved, it still does).
pub fn is_enabled(reg: &dyn Registry, exe: &str) -> Result<bool, String> {
    let user = reg
        .read_string(Hive::CurrentUser, RUN_KEY, VALUE_NAME)
        .map_err(other)?
        .is_some()
        && approved(
            reg.read_binary(Hive::CurrentUser, APPROVED_KEY, VALUE_NAME)
                .map_err(other)?
                .as_deref(),
        );
    if user {
        return Ok(true);
    }
    // Reading HKLM needs no administrator.
    let machine = machine_entry_for(reg, exe).unwrap_or(false)
        && approved(
            reg.read_binary(Hive::LocalMachine, APPROVED_KEY, VALUE_NAME)
                .ok()
                .flatten()
                .as_deref(),
        );
    Ok(machine)
}

/// Turn it on for this user: the Run entry with the path quoted (a user folder like
/// `C:\Users\Sam Lee\...` always launches tug and nothing else), and Task Manager's flag set to
/// enabled (so turning it on in tug also undoes a "Disable" in Task Manager).
pub fn enable(reg: &dyn Registry, exe: &str, args: &[&str]) -> Result<(), String> {
    reg.write_string(Hive::CurrentUser, RUN_KEY, VALUE_NAME, &run_command(exe, args))
        .map_err(other)?;
    if let Err(e) = reg.write_binary(Hive::CurrentUser, APPROVED_KEY, VALUE_NAME, &APPROVED_ENABLED) {
        // The Run entry alone starts tug; the flag only matters after a Task Manager "Disable".
        log::warn!("autostart: couldn't set Task Manager's startup flag ({e})");
    }
    Ok(())
}

/// What to tell the person when only an administrator can finish turning it off.
pub const NEEDS_ADMIN: &str = "tug was set to start for everyone who uses this PC, which only an administrator \
                               can turn off. Right-click tug, choose Run as administrator, and turn this off there.";

/// Turn it off: this user's entry and flag, and an old machine-wide entry that starts this tug.
/// That last one needs an administrator; without one the switch stays on (it's true: tug will
/// still start) and the person hears how to finish.
pub fn disable(reg: &dyn Registry, exe: &str) -> Result<(), String> {
    reg.delete(Hive::CurrentUser, RUN_KEY, VALUE_NAME).map_err(other)?;
    let _ = reg.delete(Hive::CurrentUser, APPROVED_KEY, VALUE_NAME);
    if machine_entry_for(reg, exe).unwrap_or(false) {
        match remove_machine_entry(reg) {
            Ok(()) => log::info!("autostart: removed an all-users Start with Windows entry"),
            Err(RegError::AccessDenied) => return Err(NEEDS_ADMIN.into()),
            Err(e) => return Err(other(e)),
        }
    }
    Ok(())
}

fn remove_machine_entry(reg: &dyn Registry) -> Result<(), RegError> {
    reg.delete(Hive::LocalMachine, RUN_KEY, VALUE_NAME)?;
    let _ = reg.delete(Hive::LocalMachine, APPROVED_KEY, VALUE_NAME);
    Ok(())
}

/// What `migrate_machine_entry` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    /// No machine-wide entry for this tug (or not elevated, so nothing could be done).
    Nothing,
    /// Moved to this user (it was enabled, so this user's entry now starts tug).
    Moved,
    /// Removed (it was disabled in Task Manager, so nothing replaces it).
    Removed,
}

/// Running elevated: move a machine-wide entry that starts this tug back to this user, keeping
/// whether it was on. Entries for anything else (another copy, another user's tug) are left alone.
pub fn migrate_machine_entry(reg: &dyn Registry, exe: &str, args: &[&str], elevated: bool) -> Migration {
    if !elevated || !machine_entry_for(reg, exe).unwrap_or(false) {
        return Migration::Nothing;
    }
    let was_on = approved(
        reg.read_binary(Hive::LocalMachine, APPROVED_KEY, VALUE_NAME)
            .ok()
            .flatten()
            .as_deref(),
    );
    if was_on {
        if let Err(e) = enable(reg, exe, args) {
            // Don't remove the old entry if the new one couldn't be written: tug would stop
            // starting with Windows without anyone choosing that.
            log::warn!("autostart: couldn't move the all-users entry to this user: {e}");
            return Migration::Nothing;
        }
    }
    match remove_machine_entry(reg) {
        Ok(()) if was_on => {
            log::info!("autostart: moved the all-users Start with Windows entry to this user");
            Migration::Moved
        }
        Ok(()) => {
            log::info!("autostart: removed a disabled all-users Start with Windows entry");
            Migration::Removed
        }
        Err(e) => {
            log::warn!("autostart: couldn't remove the all-users entry ({e})");
            Migration::Nothing
        }
    }
}

/// This tug's own path, for the Run entry.
fn current_exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// The Settings switch.
pub fn enabled() -> Result<bool, String> {
    is_enabled(&WinRegistry, &current_exe()?)
}

/// Turn Start with Windows on or off.
pub fn set(on: bool) -> Result<(), String> {
    let exe = current_exe()?;
    if on {
        enable(&WinRegistry, &exe, &[crate::MINIMIZED_ARG])?;
        // Elevated: also move an old all-users entry, so there's only ever this one.
        migrate_machine_entry(&WinRegistry, &exe, &[crate::MINIMIZED_ARG], is_elevated());
        Ok(())
    } else {
        disable(&WinRegistry, &exe)
    }
}

/// At startup: running elevated, tidy away an old all-users entry for this tug.
pub fn tidy_at_startup() {
    if !is_elevated() {
        return;
    }
    if let Ok(exe) = current_exe() {
        migrate_machine_entry(&WinRegistry, &exe, &[crate::MINIMIZED_ARG], true);
    }
}

/// Whether this process runs elevated (Run as administrator, or UAC off for an administrator).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: plain FFI on this process's own token, closed before returning.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// The real registry.
pub struct WinRegistry;

#[cfg(windows)]
mod win {
    use super::{Hive, RegError};
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_EXPAND_SZ,
        REG_OPEN_CREATE_OPTIONS, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
    };

    pub struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: opened by RegOpenKeyExW / RegCreateKeyExW below.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn root(hive: Hive) -> HKEY {
        match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    fn err(what: &str, r: WIN32_ERROR) -> RegError {
        if r == ERROR_ACCESS_DENIED {
            RegError::AccessDenied
        } else {
            RegError::Other(format!("{what} ({})", r.0))
        }
    }

    fn missing(r: WIN32_ERROR) -> bool {
        r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND
    }

    /// Open an existing key, or `None` if it isn't there.
    pub fn open(hive: Hive, key: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>, RegError> {
        let mut h = HKEY::default();
        // SAFETY: plain FFI; the key is closed on drop.
        let r = unsafe { RegOpenKeyExW(root(hive), &HSTRING::from(key), None, access, &mut h) };
        if missing(r) {
            return Ok(None);
        }
        if r.is_err() {
            return Err(err("couldn't open a registry key", r));
        }
        Ok(Some(Key(h)))
    }

    pub fn create(hive: Hive, key: &str) -> Result<Key, RegError> {
        let mut h = HKEY::default();
        // SAFETY: plain FFI; the key is closed on drop.
        let r = unsafe {
            RegCreateKeyExW(
                root(hive),
                &HSTRING::from(key),
                None,
                None,
                REG_OPEN_CREATE_OPTIONS(0),
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut h,
                None,
            )
        };
        if r.is_err() {
            return Err(err("couldn't create a registry key", r));
        }
        Ok(Key(h))
    }

    /// A value's type and bytes, or `None` if it isn't there.
    pub fn query(key: &Key, name: &str) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>, RegError> {
        let name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut len = 0u32;
        // SAFETY: size query, then a read into a buffer of that size.
        unsafe {
            let r = RegQueryValueExW(key.0, &name, None, Some(&mut kind), None, Some(&mut len));
            if missing(r) {
                return Ok(None);
            }
            if r.is_err() {
                return Err(err("couldn't read a registry value", r));
            }
            let mut buf = vec![0u8; len as usize + 2];
            let mut len = buf.len() as u32;
            let r = RegQueryValueExW(
                key.0,
                &name,
                None,
                Some(&mut kind),
                Some(buf.as_mut_ptr()),
                Some(&mut len),
            );
            if missing(r) {
                return Ok(None);
            }
            if r.is_err() {
                return Err(err("couldn't read a registry value", r));
            }
            buf.truncate(len as usize);
            Ok(Some((kind, buf)))
        }
    }

    pub fn utf16_string(kind: REG_VALUE_TYPE, bytes: &[u8]) -> Option<String> {
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return None;
        }
        let wide: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let n = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        Some(String::from_utf16_lossy(&wide[..n]))
    }

    pub fn set(key: &Key, name: &str, kind: REG_VALUE_TYPE, bytes: &[u8]) -> Result<(), RegError> {
        // SAFETY: plain FFI with a buffer alive for the call.
        let r = unsafe { RegSetValueExW(key.0, &HSTRING::from(name), None, kind, Some(bytes)) };
        if r.is_err() {
            return Err(err("couldn't write a registry value", r));
        }
        Ok(())
    }

    pub fn string_bytes(value: &str) -> Vec<u8> {
        value
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    pub fn delete(key: &Key, name: &str) -> Result<(), RegError> {
        // SAFETY: plain FFI.
        let r = unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) };
        if missing(r) {
            return Ok(());
        }
        if r.is_err() {
            return Err(err("couldn't delete a registry value", r));
        }
        Ok(())
    }

    pub const READ: REG_SAM_FLAGS = KEY_QUERY_VALUE;
    pub const WRITE: REG_SAM_FLAGS = KEY_SET_VALUE;
    pub const BINARY: REG_VALUE_TYPE = REG_BINARY;
    pub const SZ: REG_VALUE_TYPE = REG_SZ;
}

#[cfg(windows)]
impl Registry for WinRegistry {
    fn read_string(&self, hive: Hive, key: &str, name: &str) -> Result<Option<String>, RegError> {
        let Some(k) = win::open(hive, key, win::READ)? else {
            return Ok(None);
        };
        Ok(win::query(&k, name)?.and_then(|(kind, bytes)| win::utf16_string(kind, &bytes)))
    }

    fn read_binary(&self, hive: Hive, key: &str, name: &str) -> Result<Option<Vec<u8>>, RegError> {
        let Some(k) = win::open(hive, key, win::READ)? else {
            return Ok(None);
        };
        Ok(win::query(&k, name)?.map(|(_, bytes)| bytes))
    }

    fn write_string(&self, hive: Hive, key: &str, name: &str, value: &str) -> Result<(), RegError> {
        win::set(&win::create(hive, key)?, name, win::SZ, &win::string_bytes(value))
    }

    fn write_binary(&self, hive: Hive, key: &str, name: &str, value: &[u8]) -> Result<(), RegError> {
        win::set(&win::create(hive, key)?, name, win::BINARY, value)
    }

    fn delete(&self, hive: Hive, key: &str, name: &str) -> Result<(), RegError> {
        match win::open(hive, key, win::WRITE)? {
            Some(k) => win::delete(&k, name),
            None => Ok(()),
        }
    }
}

#[cfg(not(windows))]
impl Registry for WinRegistry {
    fn read_string(&self, _: Hive, _: &str, _: &str) -> Result<Option<String>, RegError> {
        Ok(None)
    }
    fn read_binary(&self, _: Hive, _: &str, _: &str) -> Result<Option<Vec<u8>>, RegError> {
        Ok(None)
    }
    fn write_string(&self, _: Hive, _: &str, _: &str, _: &str) -> Result<(), RegError> {
        Err(RegError::Other("only on Windows".into()))
    }
    fn write_binary(&self, _: Hive, _: &str, _: &str, _: &[u8]) -> Result<(), RegError> {
        Err(RegError::Other("only on Windows".into()))
    }
    fn delete(&self, _: Hive, _: &str, _: &str) -> Result<(), RegError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Debug, Clone, PartialEq)]
    enum Val {
        Str(String),
        Bin(Vec<u8>),
    }

    /// An in-memory registry. `admin: false` makes every HKLM write fail like a non-elevated tug.
    struct Fake {
        values: RefCell<HashMap<(Hive, String, String), Val>>,
        admin: bool,
    }

    impl Fake {
        fn new(admin: bool) -> Fake {
            Fake {
                values: RefCell::new(HashMap::new()),
                admin,
            }
        }
        fn put(&self, hive: Hive, key: &str, name: &str, v: Val) {
            self.values
                .borrow_mut()
                .insert((hive, key.to_string(), name.to_string()), v);
        }
        fn get(&self, hive: Hive, key: &str, name: &str) -> Option<Val> {
            self.values
                .borrow()
                .get(&(hive, key.to_string(), name.to_string()))
                .cloned()
        }
        fn guard(&self, hive: Hive) -> Result<(), RegError> {
            if hive == Hive::LocalMachine && !self.admin {
                Err(RegError::AccessDenied)
            } else {
                Ok(())
            }
        }
    }

    impl Registry for Fake {
        fn read_string(&self, hive: Hive, key: &str, name: &str) -> Result<Option<String>, RegError> {
            Ok(match self.get(hive, key, name) {
                Some(Val::Str(s)) => Some(s),
                _ => None,
            })
        }
        fn read_binary(&self, hive: Hive, key: &str, name: &str) -> Result<Option<Vec<u8>>, RegError> {
            Ok(match self.get(hive, key, name) {
                Some(Val::Bin(b)) => Some(b),
                _ => None,
            })
        }
        fn write_string(&self, hive: Hive, key: &str, name: &str, value: &str) -> Result<(), RegError> {
            self.guard(hive)?;
            self.put(hive, key, name, Val::Str(value.into()));
            Ok(())
        }
        fn write_binary(&self, hive: Hive, key: &str, name: &str, value: &[u8]) -> Result<(), RegError> {
            self.guard(hive)?;
            self.put(hive, key, name, Val::Bin(value.to_vec()));
            Ok(())
        }
        fn delete(&self, hive: Hive, key: &str, name: &str) -> Result<(), RegError> {
            self.guard(hive)?;
            self.values
                .borrow_mut()
                .remove(&(hive, key.to_string(), name.to_string()));
            Ok(())
        }
    }

    const EXE: &str = r"C:\Users\Sam Lee\AppData\Local\tug\tug.exe";
    const ARGS: &[&str] = &["--minimized"];
    const DISABLED: [u8; 12] = [0x03, 0, 0, 0, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x01];

    /// What tauri-plugin-autostart wrote when tug ran elevated.
    fn plugin_machine_entry(reg: &Fake) {
        reg.put(
            Hive::LocalMachine,
            RUN_KEY,
            VALUE_NAME,
            Val::Str(format!("{EXE} --minimized")),
        );
        reg.put(
            Hive::LocalMachine,
            APPROVED_KEY,
            VALUE_NAME,
            Val::Bin(APPROVED_ENABLED.to_vec()),
        );
    }

    #[test]
    fn run_command_quotes_the_path() {
        assert_eq!(
            run_command(EXE, &["--minimized"]),
            r#""C:\Users\Sam Lee\AppData\Local\tug\tug.exe" --minimized"#
        );
        // Already quoted, or no args: still exactly one pair of quotes.
        assert_eq!(
            run_command(r#""C:\Program Files\tug\tug.exe""#, &[]),
            r#""C:\Program Files\tug\tug.exe""#
        );
    }

    #[test]
    fn recognises_entries_for_this_tug_only() {
        assert!(points_at(&run_command(EXE, ARGS), EXE));
        // The plugin's unquoted form, any case.
        assert!(points_at(
            r"c:\users\sam lee\appdata\local\tug\TUG.EXE --minimized",
            EXE
        ));
        assert!(points_at(EXE, EXE));
        // Another user's tug, another program, or a longer name that merely starts the same.
        assert!(!points_at(r"C:\Users\Alex\AppData\Local\tug\tug.exe --minimized", EXE));
        assert!(!points_at(r#""C:\Tools\other.exe" --minimized"#, EXE));
        assert!(!points_at(&format!("{EXE}x --minimized"), EXE));
        assert!(!points_at("", ""));
    }

    #[test]
    fn enable_writes_only_this_users_keys_with_the_path_quoted() {
        // Even with an administrator token, nothing goes to HKLM.
        let reg = Fake::new(true);
        enable(&reg, EXE, ARGS).unwrap();
        assert_eq!(
            reg.get(Hive::CurrentUser, RUN_KEY, VALUE_NAME),
            Some(Val::Str(run_command(EXE, ARGS)))
        );
        assert_eq!(
            reg.get(Hive::CurrentUser, APPROVED_KEY, VALUE_NAME),
            Some(Val::Bin(APPROVED_ENABLED.to_vec()))
        );
        assert!(reg.values.borrow().keys().all(|(h, _, _)| *h == Hive::CurrentUser));
        assert!(is_enabled(&reg, EXE).unwrap());
    }

    #[test]
    fn enabling_undoes_a_task_manager_disable() {
        let reg = Fake::new(false);
        enable(&reg, EXE, ARGS).unwrap();
        reg.put(Hive::CurrentUser, APPROVED_KEY, VALUE_NAME, Val::Bin(DISABLED.to_vec()));
        assert!(!is_enabled(&reg, EXE).unwrap(), "disabled in Task Manager reads as off");
        enable(&reg, EXE, ARGS).unwrap();
        assert!(is_enabled(&reg, EXE).unwrap());
    }

    #[test]
    fn disable_removes_this_users_entry_without_an_administrator() {
        let reg = Fake::new(false);
        enable(&reg, EXE, ARGS).unwrap();
        disable(&reg, EXE).unwrap();
        assert!(!is_enabled(&reg, EXE).unwrap());
        assert!(reg.values.borrow().is_empty());
        // Off when it was already off is fine.
        disable(&reg, EXE).unwrap();
    }

    #[test]
    fn an_old_machine_wide_entry_keeps_the_switch_truthful() {
        // Not elevated: tug still starts at sign-in, so the switch says on, and turning it off
        // says how to finish rather than pretending it worked.
        let reg = Fake::new(false);
        plugin_machine_entry(&reg);
        assert!(is_enabled(&reg, EXE).unwrap());
        assert_eq!(disable(&reg, EXE), Err(NEEDS_ADMIN.to_string()));
        assert!(is_enabled(&reg, EXE).unwrap());
        // Disabled in Task Manager: off.
        reg.put(
            Hive::LocalMachine,
            APPROVED_KEY,
            VALUE_NAME,
            Val::Bin(DISABLED.to_vec()),
        );
        assert!(!is_enabled(&reg, EXE).unwrap());
    }

    #[test]
    fn another_users_machine_entry_is_none_of_our_business() {
        let reg = Fake::new(true);
        reg.put(
            Hive::LocalMachine,
            RUN_KEY,
            VALUE_NAME,
            Val::Str(r"C:\Users\Alex\AppData\Local\tug\tug.exe --minimized".into()),
        );
        assert!(!is_enabled(&reg, EXE).unwrap());
        disable(&reg, EXE).unwrap();
        assert_eq!(migrate_machine_entry(&reg, EXE, ARGS, true), Migration::Nothing);
        assert!(reg.get(Hive::LocalMachine, RUN_KEY, VALUE_NAME).is_some());
    }

    #[test]
    fn elevated_moves_the_machine_entry_to_this_user() {
        let reg = Fake::new(true);
        plugin_machine_entry(&reg);
        // Not elevated: nothing happens (it couldn't).
        assert_eq!(migrate_machine_entry(&reg, EXE, ARGS, false), Migration::Nothing);
        assert_eq!(migrate_machine_entry(&reg, EXE, ARGS, true), Migration::Moved);
        assert_eq!(reg.get(Hive::LocalMachine, RUN_KEY, VALUE_NAME), None);
        assert_eq!(reg.get(Hive::LocalMachine, APPROVED_KEY, VALUE_NAME), None);
        assert_eq!(
            reg.get(Hive::CurrentUser, RUN_KEY, VALUE_NAME),
            Some(Val::Str(run_command(EXE, ARGS)))
        );
        assert!(is_enabled(&reg, EXE).unwrap());
        // Once moved, there's nothing left to move.
        assert_eq!(migrate_machine_entry(&reg, EXE, ARGS, true), Migration::Nothing);
    }

    #[test]
    fn elevated_drops_a_disabled_machine_entry_without_turning_it_on() {
        let reg = Fake::new(true);
        plugin_machine_entry(&reg);
        reg.put(
            Hive::LocalMachine,
            APPROVED_KEY,
            VALUE_NAME,
            Val::Bin(DISABLED.to_vec()),
        );
        assert_eq!(migrate_machine_entry(&reg, EXE, ARGS, true), Migration::Removed);
        assert!(reg.values.borrow().is_empty());
        assert!(!is_enabled(&reg, EXE).unwrap());
    }

    #[test]
    fn elevated_disable_removes_the_machine_entry_too() {
        let reg = Fake::new(true);
        plugin_machine_entry(&reg);
        enable(&reg, EXE, ARGS).unwrap();
        disable(&reg, EXE).unwrap();
        assert!(reg.values.borrow().is_empty());
        assert!(!is_enabled(&reg, EXE).unwrap());
    }

    /// The Windows side, against a throwaway key under this user's Software key (never the real
    /// Run key): write, read back, delete, and missing values read as `None`.
    #[cfg(windows)]
    #[test]
    fn windows_registry_round_trip_on_a_temp_key() {
        let key = format!(r"Software\tug-autostart-test-{}", std::process::id());
        let reg = WinRegistry;
        assert_eq!(reg.read_string(Hive::CurrentUser, &key, "x").unwrap(), None);
        reg.write_string(Hive::CurrentUser, &key, "x", &run_command(EXE, ARGS))
            .unwrap();
        reg.write_binary(Hive::CurrentUser, &key, "flag", &DISABLED).unwrap();
        assert_eq!(
            reg.read_string(Hive::CurrentUser, &key, "x").unwrap(),
            Some(run_command(EXE, ARGS))
        );
        assert_eq!(
            reg.read_binary(Hive::CurrentUser, &key, "flag").unwrap(),
            Some(DISABLED.to_vec())
        );
        reg.delete(Hive::CurrentUser, &key, "x").unwrap();
        reg.delete(Hive::CurrentUser, &key, "x").unwrap();
        reg.delete(Hive::CurrentUser, &key, "flag").unwrap();
        assert_eq!(reg.read_string(Hive::CurrentUser, &key, "x").unwrap(), None);
        // Remove the (now empty) test key.
        // SAFETY: plain FFI on a key this test created.
        unsafe {
            let _ = windows::Win32::System::Registry::RegDeleteKeyW(
                windows::Win32::System::Registry::HKEY_CURRENT_USER,
                &windows::core::HSTRING::from(key.as_str()),
            );
        }
        // A key that doesn't exist reads as nothing and deletes as success.
        assert_eq!(
            reg.read_binary(Hive::CurrentUser, r"Software\tug-no-such-key", "x")
                .unwrap(),
            None
        );
        reg.delete(Hive::CurrentUser, r"Software\tug-no-such-key", "x").unwrap();
    }
}
