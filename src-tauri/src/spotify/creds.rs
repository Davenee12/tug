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
        // SAFETY: `name` is a valid null-terminated wide string; on success `ptr` receives a
        // credential that Windows allocated, which `ReadCredential` takes ownership of.
        match unsafe { CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, Some(0), &mut ptr) } {
            Ok(()) => {
                let cred = ReadCredential(ptr);
                Ok(Some(String::from_utf8_lossy(cred.blob()).into_owned()))
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

    /// A credential `CredReadW` allocated. It's freed with `CredFree` exactly once, when this is
    /// dropped, so the bytes borrowed from it can't outlive it and no path (an early return, a
    /// panic) leaks it or frees it twice.
    struct ReadCredential(*mut CREDENTIALW);

    impl ReadCredential {
        /// The secret's bytes, or none if Windows handed back no credential at all.
        fn blob(&self) -> &[u8] {
            let ptr = self.0;
            if ptr.is_null() {
                return &[];
            }
            // SAFETY: non-null and from a successful CredReadW, so it points at a CREDENTIALW
            // that stays valid until `drop` frees it; the borrow of `self` keeps it alive.
            unsafe { blob_bytes(&*ptr) }
        }
    }

    impl Drop for ReadCredential {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: allocated by CredReadW and freed only here.
                unsafe { CredFree(self.0 as *const _) };
            }
        }
    }

    /// The bytes of a credential's blob. An empty secret may come back as a null blob pointer,
    /// and a slice can't be built from a null pointer even with length zero.
    ///
    /// # Safety
    /// A non-null `CredentialBlob` must point at `CredentialBlobSize` readable bytes that outlive
    /// the returned slice (true of a credential from CredReadW until it's freed).
    unsafe fn blob_bytes(cred: &CREDENTIALW) -> &[u8] {
        if cred.CredentialBlob.is_null() || cred.CredentialBlobSize == 0 {
            return &[];
        }
        std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_empty_or_missing_blob_reads_as_no_bytes() {
            // SAFETY: a zeroed CREDENTIALW is valid (null pointers, zero counts).
            let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { blob_bytes(&cred) }, b"");
            // A size with no buffer must not be trusted.
            cred.CredentialBlobSize = 5;
            assert_eq!(unsafe { blob_bytes(&cred) }, b"");
            // A buffer with no size is empty.
            let mut bytes = b"token".to_vec();
            cred.CredentialBlob = bytes.as_mut_ptr();
            cred.CredentialBlobSize = 0;
            assert_eq!(unsafe { blob_bytes(&cred) }, b"");
        }

        #[test]
        fn a_blob_reads_exactly_its_size() {
            let mut bytes = b"token-and-more".to_vec();
            // SAFETY: as above; the blob points at `bytes`, which outlives the reads.
            let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
            cred.CredentialBlob = bytes.as_mut_ptr();
            cred.CredentialBlobSize = 5;
            assert_eq!(unsafe { blob_bytes(&cred) }, b"token");
        }

        #[test]
        fn a_null_credential_is_never_dereferenced_or_freed() {
            let cred = ReadCredential(std::ptr::null_mut());
            assert!(cred.blob().is_empty());
            // Dropping it here must not call CredFree on null.
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
