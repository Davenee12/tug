//! Windows security for the bridge: who the current user is (their SID), a security descriptor
//! that lets only that user in, and writing the token file with it.

use std::ffi::c_void;
use std::io;
use std::path::Path;

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

fn win_err(e: windows::core::Error) -> io::Error {
    io::Error::other(e.message())
}

/// The current user's SID as a string (`S-1-5-21-…`).
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: standard token query; every handle and buffer is owned here and released below.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(win_err)?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len.max(1) as usize];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len);
        let _ = CloseHandle(token);
        got.map_err(win_err)?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).map_err(win_err)?;
        let out = s.to_string().map_err(|e| io::Error::other(e.to_string()));
        let _ = LocalFree(Some(HLOCAL(s.0 as *mut c_void)));
        out
    }
}

/// A security descriptor (from SDDL) plus the SECURITY_ATTRIBUTES pointing at it, freed on drop.
pub struct UserOnly {
    sd: PSECURITY_DESCRIPTOR,
    attrs: SECURITY_ATTRIBUTES,
}

// SAFETY: the descriptor is immutable after creation and only read by the kernel.
unsafe impl Send for UserOnly {}
unsafe impl Sync for UserOnly {}

impl UserOnly {
    /// Full control for this user only, nothing inherited from the parent (`D:P`). `GA` covers
    /// creating more pipe instances and reading/writing a file.
    pub fn new(user_sid: &str) -> io::Result<UserOnly> {
        if !user_sid.starts_with("S-") || !user_sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(io::Error::other("unexpected SID"));
        }
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{user_sid})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: plain FFI; the descriptor is freed with LocalFree in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None)
                .map_err(win_err)?;
        }
        Ok(UserOnly {
            sd,
            attrs: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            },
        })
    }

    /// For `ServerOptions::create_with_security_attributes_raw`.
    pub fn as_raw(&self) -> *mut c_void {
        &self.attrs as *const SECURITY_ATTRIBUTES as *mut c_void
    }

    pub fn attrs(&self) -> &SECURITY_ATTRIBUTES {
        &self.attrs
    }
}

impl Drop for UserOnly {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.sd.0)));
        }
    }
}

/// Write `bytes` to `path` so that only the current user can open it: a new temporary file is
/// created with a user-only ACL, written, and renamed over the old one (a rename keeps the new
/// file's ACL, which overwriting an existing file in place wouldn't set).
pub fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE};

    let sid = current_user_sid()?;
    let sec = UserOnly::new(&sid)?;
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let wide = HSTRING::from(tmp.as_os_str());
    const GENERIC_WRITE: u32 = 0x4000_0000;
    // SAFETY: plain FFI; the returned handle is owned by the File below.
    let handle = unsafe {
        CreateFileW(
            &wide,
            GENERIC_WRITE,
            FILE_SHARE_NONE,
            Some(sec.attrs() as *const SECURITY_ATTRIBUTES),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(win_err)?
    };
    // SAFETY: a fresh, valid file handle we own.
    let mut file = unsafe { std::fs::File::from_raw_handle(handle.0) };
    let written = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_current_user() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(UserOnly::new(&sid).is_ok());
        assert!(UserOnly::new("S-1-5;(A;;GA;;;WD)").is_err());
    }

    #[test]
    fn writes_and_replaces_a_private_file() {
        let dir = std::env::temp_dir().join(format!("tug-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bridge.token");
        write_private_file(&path, b"one").unwrap();
        write_private_file(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
