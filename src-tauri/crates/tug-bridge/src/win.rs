//! Windows security for the bridge: who the current user is (their SID), security descriptors
//! that let only that user in, writing the token file with one, and checking that whoever owns
//! the pipe a client opened really is this user at normal (medium) integrity or above.

use std::ffi::c_void;
use std::io;
use std::path::Path;

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
    SE_OBJECT_TYPE,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
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

fn plausible_sid(sid: &str) -> bool {
    sid.starts_with("S-") && sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The pipe's descriptor: owned by this user, full control for this user only, nothing
/// inherited (`D:P`). `GA` covers creating more pipe instances. The owner is set explicitly so
/// clients can check it (an elevated process would otherwise default to Administrators).
pub fn pipe_sddl(user_sid: &str) -> String {
    format!("O:{user_sid}D:P(A;;GA;;;{user_sid})")
}

/// The token file's descriptor: as the pipe's, plus a medium mandatory label with no-read-up,
/// so a low-integrity process (a sandboxed browser, say) running as the same user can't read
/// the token, even though the user's own ACE would let it.
pub fn file_sddl(user_sid: &str) -> String {
    format!("O:{user_sid}D:P(A;;GA;;;{user_sid})S:(ML;;NRNWNX;;;ME)")
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
    pub fn pipe(user_sid: &str) -> io::Result<UserOnly> {
        Self::from_sddl(user_sid, pipe_sddl)
    }

    pub fn file(user_sid: &str) -> io::Result<UserOnly> {
        Self::from_sddl(user_sid, file_sddl)
    }

    fn from_sddl(user_sid: &str, make: fn(&str) -> String) -> io::Result<UserOnly> {
        if !plausible_sid(user_sid) {
            return Err(io::Error::other("unexpected SID"));
        }
        let sddl = HSTRING::from(make(user_sid));
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

/// Integrity levels by their mandatory-label RID.
pub const MEDIUM_INTEGRITY: u32 = 0x2000;

fn label_rid(sid: &str) -> Option<u32> {
    Some(match sid {
        "LW" => 0x1000,
        "ME" => 0x2000,
        "MP" => 0x2100,
        "HI" => 0x3000,
        "SI" => 0x4000,
        s => {
            let rid = s.strip_prefix("S-1-16-")?;
            rid.parse().ok()?
        }
    })
}

/// The owner SID and integrity RID in an SDDL string holding `O:` and `S:` (label) parts. An
/// object with no label counts as medium, as Windows treats it.
pub fn owner_and_integrity(sddl: &str) -> (Option<String>, u32) {
    let owner = sddl.find("O:").map(|i| {
        let rest = &sddl[i + 2..];
        let end = ["G:", "D:", "S:"]
            .iter()
            .filter_map(|m| rest.find(m))
            .min()
            .unwrap_or(rest.len());
        rest[..end].to_string()
    });
    let integrity = sddl
        .find("(ML;")
        .and_then(|i| {
            let ace = &sddl[i + 1..];
            let ace = &ace[..ace.find(')')?];
            label_rid(ace.rsplit(';').next()?)
        })
        .unwrap_or(MEDIUM_INTEGRITY);
    (owner, integrity)
}

/// Owned by `user_sid` and at medium integrity or above: something this user's normal processes
/// made, not another user's or a sandboxed process's.
pub fn trusted_owner(sddl: &str, user_sid: &str) -> bool {
    let (owner, integrity) = owner_and_integrity(sddl);
    owner.is_some_and(|o| same_sid(&o, user_sid)) && integrity >= MEDIUM_INTEGRITY
}

/// Whether two SIDs as SDDL writes them are the same account. SDDL abbreviates well-known
/// accounts: the built-in Administrator (RID 500, which is who CI runners and some home PCs run
/// as) comes back as `LA`, not `S-1-5-21-…-500`, so a plain string compare would refuse the
/// user's own pipe and token file. Both sides go through Windows' own parser to compare.
pub fn same_sid(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || matches!((canonical_sid(a), canonical_sid(b)), (Some(x), Some(y)) if x == y)
}

/// A SID string (full or an SDDL alias like `LA`) in its full `S-1-…` form, via Windows.
fn canonical_sid(s: &str) -> Option<String> {
    use windows::Win32::Foundation::HLOCAL;
    use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows::Win32::Security::PSID;
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    // SAFETY: plain FFI; the SID and the string are freed with LocalFree.
    unsafe {
        let mut sid = PSID::default();
        ConvertStringSidToSidW(&HSTRING::from(s), &mut sid).ok()?;
        let mut out = PWSTR::null();
        let converted = ConvertSidToStringSidW(sid, &mut out);
        let _ = LocalFree(Some(HLOCAL(sid.0)));
        converted.ok()?;
        let text = out.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(out.0 as *mut c_void)));
        text
    }
}

fn object_sddl(handle: HANDLE, kind: SE_OBJECT_TYPE) -> io::Result<String> {
    let info = OWNER_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: plain FFI; both returned buffers are freed with LocalFree below.
    unsafe {
        let r = GetSecurityInfo(handle, kind, info, None, None, None, None, Some(&mut sd));
        if r.is_err() {
            return Err(io::Error::from_raw_os_error(r.0 as i32));
        }
        let mut s = PWSTR::null();
        let converted = ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, SDDL_REVISION_1, info, &mut s, None);
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        converted.map_err(win_err)?;
        let out = s.to_string().map_err(|e| io::Error::other(e.to_string()));
        let _ = LocalFree(Some(HLOCAL(s.0 as *mut c_void)));
        out
    }
}

/// The owner and label of the pipe a client opened (`HANDLE` from the client end).
pub fn pipe_sddl_of(handle: HANDLE) -> io::Result<String> {
    object_sddl(handle, SE_KERNEL_OBJECT)
}

/// Write `bytes` to `path` so that only the current user's normal processes can open it: a new
/// temporary file is created with `file_sddl`, written, and renamed over the old one (a rename
/// keeps the new file's descriptor, which overwriting an existing file in place wouldn't set).
pub fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::windows::io::FromRawHandle;
    use windows::Win32::Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE};

    let sid = current_user_sid()?;
    let sec = UserOnly::file(&sid)?;
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

/// The owner and label of a file, for tests.
#[cfg(test)]
fn file_sddl_of(path: &Path) -> io::Result<String> {
    use std::os::windows::io::AsRawHandle;
    let f = std::fs::File::open(path)?;
    object_sddl(
        HANDLE(f.as_raw_handle()),
        windows::Win32::Security::Authorization::SE_FILE_OBJECT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "S-1-5-21-1-2-3-1001";

    #[test]
    fn finds_the_current_user() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(UserOnly::pipe(&sid).is_ok());
        assert!(UserOnly::file(&sid).is_ok());
        assert!(UserOnly::pipe("S-1-5;(A;;GA;;;WD)").is_err());
    }

    #[test]
    fn reads_owner_and_integrity_from_sddl() {
        assert_eq!(
            owner_and_integrity(&format!("O:{ME}")),
            (Some(ME.into()), MEDIUM_INTEGRITY)
        );
        assert_eq!(
            owner_and_integrity(&format!("O:{ME}S:(ML;;NW;;;LW)")),
            (Some(ME.into()), 0x1000)
        );
        assert_eq!(
            owner_and_integrity("O:BAG:SYS:(ML;;NW;;;HI)"),
            (Some("BA".into()), 0x3000)
        );
        assert_eq!(owner_and_integrity(&format!("O:{ME}S:(ML;;NW;;;S-1-16-0)")).1, 0);
    }

    #[test]
    fn only_this_users_medium_objects_are_trusted() {
        assert!(trusted_owner(&format!("O:{ME}"), ME));
        assert!(trusted_owner(&format!("O:{ME}S:(ML;;NW;;;ME)"), ME));
        assert!(trusted_owner(&format!("O:{ME}S:(ML;;NW;;;HI)"), ME));
        // A sandboxed (low integrity) process of the same user, or untrusted.
        assert!(!trusted_owner(&format!("O:{ME}S:(ML;;NW;;;LW)"), ME));
        assert!(!trusted_owner(&format!("O:{ME}S:(ML;;NW;;;S-1-16-0)"), ME));
        // Another user, or nobody named.
        assert!(!trusted_owner("O:S-1-5-21-1-2-3-1002", ME));
        assert!(!trusted_owner("O:BA", ME));
        assert!(!trusted_owner("", ME));
    }

    #[test]
    fn sddl_aliases_compare_as_the_account_they_name() {
        // SDDL writes well-known accounts as aliases (BA = Administrators, SY = LocalSystem).
        assert!(same_sid("BA", "S-1-5-32-544"));
        assert!(same_sid("SY", "S-1-5-18"));
        assert!(!same_sid("BA", "S-1-5-18"));
        assert!(!same_sid("BA", ME));
        assert!(same_sid(ME, ME));
        assert!(!same_sid("", ""));
        assert!(trusted_owner("O:S-1-5-32-544", "BA"));
    }

    #[test]
    fn writes_and_replaces_a_private_labelled_file() {
        let dir = std::env::temp_dir().join(format!("tug-bridge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bridge.token");
        write_private_file(&path, b"one").unwrap();
        write_private_file(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!path.with_extension("tmp").exists());
        // Owned by this user, labelled medium with no-read-up.
        let sddl = file_sddl_of(&path).unwrap();
        let sid = current_user_sid().unwrap();
        let (owner, integrity) = owner_and_integrity(&sddl);
        assert!(owner.is_some_and(|o| same_sid(&o, &sid)), "{sddl}");
        assert_eq!(integrity, MEDIUM_INTEGRITY, "{sddl}");
        assert!(trusted_owner(&sddl, &sid), "{sddl}");
        assert!(sddl.contains("NR"), "no-read-up missing: {sddl}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
