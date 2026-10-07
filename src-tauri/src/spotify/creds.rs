//! The Spotify refresh token, stored in Windows Credential Manager (a generic credential,
//! machine-local) rather than in tug's plaintext settings store. Access tokens are kept only in
//! memory; the client id lives in settings (it isn't a secret).

/// The Credential Manager target name under which the refresh token is kept.
#[cfg(not(test))]
pub const TARGET: &str = "tug/spotify-refresh-token";
/// Tests use their own entry, so a real Spotify login on the dev machine can't leak into them
/// (it made `status_reports_client_id_and_redirect` fail once a real account connected).
#[cfg(test)]
pub const TARGET: &str = "tug/test-spotify-refresh-token";

pub use imp::{delete, load, store};

#[cfg(windows)]
mod imp {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    /// HRESULT_FROM_WIN32(ERROR_NOT_FOUND): no such credential.
    const NOT_FOUND: windows::core::HRESULT = windows::core::HRESULT(0x8007_0490_u32 as i32);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Write (or overwrite) the secret for `target`.
    pub fn store(target: &str, secret: &str) -> Result<(), String> {
        let mut name = wide(target);
        let mut blob = secret.as_bytes().to_vec();
        // SAFETY: a zeroed CREDENTIALW is valid (all-null pointers, zero counts); the fields we
        // set point at `name`/`blob`, which outlive the CredWriteW call below.
        let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
        cred.Type = CRED_TYPE_GENERIC;
        cred.TargetName = PWSTR(name.as_mut_ptr());
        cred.CredentialBlobSize = blob.len() as u32;
        cred.CredentialBlob = blob.as_mut_ptr();
        cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
        unsafe { CredWriteW(&cred, 0) }.map_err(|e| e.message().to_string())
    }

    /// Read the secret for `target`, or `Ok(None)` when there is none.
    pub fn load(target: &str) -> Result<Option<String>, String> {
        let name = wide(target);
        let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `name` is a valid null-terminated wide string; `ptr` receives an owned
        // credential that we copy out of and free with CredFree before returning.
        match unsafe { CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, Some(0), &mut ptr) } {
            Ok(()) => {
                let secret = unsafe {
                    let cred = &*ptr;
                    let bytes = std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
                    let s = String::from_utf8_lossy(bytes).into_owned();
                    CredFree(ptr as *const _);
                    s
                };
                Ok(Some(secret))
            }
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(e.message().to_string()),
        }
    }

    /// Remove the secret for `target` (no error if it was already gone).
    pub fn delete(target: &str) -> Result<(), String> {
        let name = wide(target);
        // SAFETY: `name` is a valid null-terminated wide string for the duration of the call.
        match unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, Some(0)) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == NOT_FOUND => Ok(()),
            Err(e) => Err(e.message().to_string()),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn store(_target: &str, _secret: &str) -> Result<(), String> {
        Err("Credential Manager is only available on Windows".into())
    }
    pub fn load(_target: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    pub fn delete(_target: &str) -> Result<(), String> {
        Ok(())
    }
}
