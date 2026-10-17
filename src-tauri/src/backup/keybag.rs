//! An encrypted iPhone backup's keys: the keybag in `Manifest.plist`, the backup password turned
//! into the key that unwraps it, and the AES key unwrap of the class keys and file keys.
//!
//! The keybag (`BackupKeyBag`) is a list of tag/length/value items: four ASCII bytes of tag, a
//! 32-bit big-endian length, then the value. The first items describe the bag (`VERS`, `TYPE`,
//! `UUID`, `WRAP`, `SALT`, `ITER`, and from iOS 10.2 `DPWT`, `DPIC`, `DPSL`); each later `UUID`
//! starts one class key (`CLAS`, `WRAP`, `KTYP`, `WPKY`).
//!
//! The password becomes the key-encryption key in two steps (iOS 10.2 and later):
//! `PBKDF2-HMAC-SHA256(password, DPSL, DPIC rounds)` and then `PBKDF2-HMAC-SHA1(that, SALT, ITER
//! rounds)`, 32 bytes each. Older backups skip the first step. DPIC is in the millions, which is
//! the point (it's what makes guessing a password slow), so the first step reports progress and
//! can be cancelled. Each class key wrapped with the password (`WRAP` bit 2) is then RFC 3394 AES
//! key-unwrapped with it; a failed integrity check means the password was wrong.
//!
//! Nothing here keeps the password or a key longer than asked: keys are `Zeroizing`, wiped when
//! they're dropped, and nothing is ever logged.

use std::collections::HashMap;

use aes_kw::{KeyInit, KwAes256};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

/// A class key wrapped with the backup password.
const WRAP_PASSCODE: u32 = 2;

/// A 32-byte AES key, wiped on drop.
pub type Key = Zeroizing<[u8; 32]>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeybagError {
    /// Not a keybag tug can read (cut off, or missing what the derivation needs).
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassKey {
    pub class: u32,
    pub wrap: u32,
    /// The class key, AES-wrapped (40 bytes for a 32-byte key).
    pub wrapped: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Keybag {
    pub salt: Vec<u8>,
    pub iterations: u32,
    /// iOS 10.2 and later: the first, slow PBKDF2-SHA256 step's salt and rounds.
    pub dp_salt: Option<Vec<u8>>,
    pub dp_iterations: Option<u32>,
    pub class_keys: Vec<ClassKey>,
}

impl Keybag {
    /// Read a `BackupKeyBag` blob.
    pub fn parse(data: &[u8]) -> Result<Self, KeybagError> {
        let mut bag = Keybag::default();
        let mut current: Option<ClassKey> = None;
        let mut seen_uuid = false;
        let mut at = 0usize;
        while at < data.len() {
            let tag = data.get(at..at + 4).ok_or(KeybagError::Malformed)?;
            let len_bytes = data.get(at + 4..at + 8).ok_or(KeybagError::Malformed)?;
            let len = u32::from_be_bytes(len_bytes.try_into().map_err(|_| KeybagError::Malformed)?) as usize;
            let value = data.get(at + 8..at + 8 + len).ok_or(KeybagError::Malformed)?;
            at += 8 + len;
            let int = || -> Result<u32, KeybagError> {
                Ok(u32::from_be_bytes(
                    value.try_into().map_err(|_| KeybagError::Malformed)?,
                ))
            };
            match tag {
                b"UUID" => {
                    // The first UUID is the bag's own; each one after it starts a class key.
                    if seen_uuid {
                        if let Some(done) = current.take() {
                            bag.class_keys.push(done);
                        }
                        current = Some(ClassKey {
                            class: 0,
                            wrap: 0,
                            wrapped: Vec::new(),
                        });
                    }
                    seen_uuid = true;
                }
                b"CLAS" => {
                    if let Some(k) = current.as_mut() {
                        k.class = int()?;
                    }
                }
                b"WRAP" => {
                    if let Some(k) = current.as_mut() {
                        k.wrap = int()?;
                    }
                }
                b"WPKY" => {
                    if let Some(k) = current.as_mut() {
                        k.wrapped = value.to_vec();
                    }
                }
                b"SALT" if current.is_none() => bag.salt = value.to_vec(),
                b"ITER" if current.is_none() => bag.iterations = int()?,
                b"DPSL" if current.is_none() => bag.dp_salt = Some(value.to_vec()),
                b"DPIC" if current.is_none() => bag.dp_iterations = Some(int()?),
                _ => {}
            }
        }
        if let Some(done) = current.take() {
            bag.class_keys.push(done);
        }
        if bag.salt.is_empty() || bag.iterations == 0 || bag.class_keys.is_empty() {
            return Err(KeybagError::Malformed);
        }
        if bag.iterations > MAX_ITERATIONS {
            return Err(KeybagError::Malformed);
        }
        if bag.dp_salt.is_some() != bag.dp_iterations.is_some() {
            return Err(KeybagError::Malformed);
        }
        Ok(bag)
    }

    /// How many slow rounds the derivation takes (for progress): the SHA-256 step's, or the
    /// SHA-1 step's for an older backup.
    pub fn slow_rounds(&self) -> u32 {
        self.dp_iterations.unwrap_or(self.iterations)
    }

    /// The backup password as the key that unwraps the class keys. `tick(done)` is called every
    /// so often with the rounds done so far (out of `slow_rounds`); returning false cancels, and
    /// then this returns `None`.
    pub fn derive_key(&self, password: &str, mut tick: impl FnMut(u32) -> bool) -> Option<Key> {
        let first: Zeroizing<Vec<u8>> = match (&self.dp_salt, self.dp_iterations) {
            (Some(salt), Some(rounds)) => {
                Zeroizing::new(pbkdf2_sha256_cancellable(password.as_bytes(), salt, rounds, &mut tick)?.to_vec())
            }
            _ => Zeroizing::new(password.as_bytes().to_vec()),
        };
        let mut key = Zeroizing::new([0u8; 32]);
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(&first, &self.salt, self.iterations, key.as_mut_slice());
        tick(self.slow_rounds());
        Some(key)
    }

    /// Unwrap every password-wrapped class key. `None` when any of them fails its integrity check,
    /// which is what a wrong password looks like.
    pub fn unlock(&self, key: &Key) -> Option<ClassKeys> {
        let mut keys = HashMap::new();
        for ck in self.class_keys.iter().filter(|k| k.wrap & WRAP_PASSCODE != 0) {
            let unwrapped = unwrap_key(key, &ck.wrapped)?;
            let fixed: [u8; 32] = unwrapped.as_slice().try_into().ok()?;
            keys.insert(ck.class, Zeroizing::new(fixed));
        }
        if keys.is_empty() {
            return None;
        }
        Some(ClassKeys(keys))
    }
}

/// The unwrapped class keys, by protection class. Wiped when dropped.
pub struct ClassKeys(HashMap<u32, Key>);

impl ClassKeys {
    /// A file's (or the manifest's) own key from its 4-byte little-endian protection class and
    /// 40-byte wrapped key, as backups store them (`ManifestKey`, a file's `EncryptionKey`).
    pub fn unwrap_tagged(&self, tagged: &[u8]) -> Option<Key> {
        let class = u32::from_le_bytes(tagged.get(..4)?.try_into().ok()?);
        self.unwrap_for_class(class, tagged.get(4..)?)
    }

    /// A key wrapped with the given class's key.
    pub fn unwrap_for_class(&self, class: u32, wrapped: &[u8]) -> Option<Key> {
        let kek = self.0.get(&class)?;
        let unwrapped = unwrap_key(kek, wrapped)?;
        Some(Zeroizing::new(unwrapped.as_slice().try_into().ok()?))
    }
}

/// RFC 3394 AES-256 key unwrap; `None` if the data isn't a wrapped key or its check fails.
pub fn unwrap_key(kek: &[u8; 32], wrapped: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if wrapped.len() < 24 || !wrapped.len().is_multiple_of(8) {
        return None;
    }
    let kw = KwAes256::new(kek.into());
    let mut out = Zeroizing::new(vec![0u8; wrapped.len() - 8]);
    kw.unwrap_key(wrapped, &mut out).ok()?;
    Some(out)
}

/// The most rounds the SHA-1 step (`ITER`) may ask for. That step can't be cancelled, and iOS uses
/// 10,000 (the slow, cancellable SHA-256 step carries the millions), so a bag asking for more than
/// this is refused rather than holding tug for hours.
const MAX_ITERATIONS: u32 = 10_000_000;

/// The rounds between two progress ticks: often enough for a smooth bar, rare enough to cost
/// nothing (a few hundred ticks for iOS's ten million rounds).
const TICK_ROUNDS: u32 = 32_768;

/// PBKDF2-HMAC-SHA256 for a 32-byte key (one block), with progress and a way out. The same
/// computation as `pbkdf2::pbkdf2_hmac::<Sha256>` (a test checks they agree), done here by hand
/// only so a ten-million-round derivation can report how far it is and stop when asked.
fn pbkdf2_sha256_cancellable(
    password: &[u8],
    salt: &[u8],
    rounds: u32,
    tick: &mut impl FnMut(u32) -> bool,
) -> Option<Zeroizing<[u8; 32]>> {
    let prf = <Hmac<Sha256> as KeyInit>::new_from_slice(password).ok()?;
    let mut first = prf.clone();
    first.update(salt);
    first.update(&1u32.to_be_bytes());
    let mut u = Zeroizing::new([0u8; 32]);
    u.copy_from_slice(&first.finalize().into_bytes());
    let mut t = u.clone();
    for done in 1..rounds.max(1) {
        let mut m = prf.clone();
        m.update(u.as_slice());
        u.copy_from_slice(&m.finalize().into_bytes());
        for (a, b) in t.iter_mut().zip(u.iter()) {
            *a ^= b;
        }
        if done % TICK_ROUNDS == 0 && !tick(done) {
            return None;
        }
    }
    Some(t)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn tlv(tag: &[u8; 4], value: &[u8]) -> Vec<u8> {
        let mut v = tag.to_vec();
        v.extend_from_slice(&(value.len() as u32).to_be_bytes());
        v.extend_from_slice(value);
        v
    }

    pub(crate) fn wrap(kek: &[u8; 32], key: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; key.len() + 8];
        KwAes256::new(kek.into()).wrap_key(key, &mut out).unwrap();
        out
    }

    /// A keybag as a backup made with `password` would carry, with the given class keys, plus the
    /// key-encryption key it derives to. Rounds are tiny so tests run fast; the format is the real one.
    pub(crate) fn make_keybag(password: &str, classes: &[(u32, [u8; 32])]) -> (Vec<u8>, [u8; 32]) {
        let (salt, iter, dpsl, dpic) = (
            b"0123456789abcdefghij".to_vec(),
            7u32,
            b"dp-salt-0123456789ab".to_vec(),
            9u32,
        );
        let mut first = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &dpsl, dpic, &mut first);
        let mut kek = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(&first, &salt, iter, &mut kek);
        let mut bag = Vec::new();
        bag.extend(tlv(b"VERS", &4u32.to_be_bytes()));
        bag.extend(tlv(b"TYPE", &1u32.to_be_bytes()));
        bag.extend(tlv(b"UUID", &[0xAA; 16]));
        bag.extend(tlv(b"HMCK", &[0x11; 40]));
        bag.extend(tlv(b"WRAP", &0u32.to_be_bytes()));
        bag.extend(tlv(b"SALT", &salt));
        bag.extend(tlv(b"ITER", &iter.to_be_bytes()));
        bag.extend(tlv(b"DPWT", &1u32.to_be_bytes()));
        bag.extend(tlv(b"DPIC", &dpic.to_be_bytes()));
        bag.extend(tlv(b"DPSL", &dpsl));
        for (class, key) in classes {
            bag.extend(tlv(b"UUID", &[*class as u8; 16]));
            bag.extend(tlv(b"CLAS", &class.to_be_bytes()));
            bag.extend(tlv(b"WRAP", &3u32.to_be_bytes()));
            bag.extend(tlv(b"KTYP", &0u32.to_be_bytes()));
            bag.extend(tlv(b"WPKY", &wrap(&kek, key)));
        }
        (bag, kek)
    }

    #[test]
    fn key_unwrap_matches_rfc_3394() {
        // RFC 3394 §4.6: 256 bits of key data wrapped with a 256-bit KEK.
        let kek: [u8; 32] = hex("000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F");
        let wrapped = hex::<40>("28C9F404C4B810F4CBCCB35CFB87F8263F5786E2D80ED326CBC7F0E71A99F43BFB988B9B7A02DD21");
        let key = unwrap_key(&kek, &wrapped).unwrap();
        assert_eq!(
            key.as_slice(),
            hex::<32>("00112233445566778899AABBCCDDEEFF000102030405060708090A0B0C0D0E0F")
        );
        // One bit off anywhere fails the check.
        let mut tampered = wrapped;
        tampered[17] ^= 1;
        assert!(unwrap_key(&kek, &tampered).is_none());
        assert!(unwrap_key(&kek, &wrapped[..20]).is_none(), "too short");
    }

    #[test]
    fn the_progress_pbkdf2_matches_the_reference() {
        // RFC 7914 §11's PBKDF2-HMAC-SHA256 vector, and the crate's own result for other sizes.
        let mut never = |_| true;
        let got = pbkdf2_sha256_cancellable(b"passwd", b"salt", 1, &mut never).unwrap();
        assert_eq!(
            got.as_slice(),
            hex::<32>("55AC046E56E3089FEC1691C22544B605F94185216DDE0465E68B9D57C20DACBC")
        );
        for rounds in [2, 3, 100, TICK_ROUNDS + 5] {
            let mut want = [0u8; 32];
            pbkdf2::pbkdf2_hmac::<Sha256>(b"pass phrase", b"some salt", rounds, &mut want);
            let got = pbkdf2_sha256_cancellable(b"pass phrase", b"some salt", rounds, &mut never).unwrap();
            assert_eq!(got.as_slice(), want, "{rounds} rounds");
        }
    }

    #[test]
    fn the_slow_step_reports_progress_and_can_be_cancelled() {
        let mut ticks = Vec::new();
        let done = pbkdf2_sha256_cancellable(b"pw", b"salt", TICK_ROUNDS * 3 + 1, &mut |d| {
            ticks.push(d);
            true
        });
        assert!(done.is_some());
        assert_eq!(ticks, vec![TICK_ROUNDS, TICK_ROUNDS * 2, TICK_ROUNDS * 3]);
        let stopped = pbkdf2_sha256_cancellable(b"pw", b"salt", TICK_ROUNDS * 3, &mut |_| false);
        assert!(stopped.is_none());
    }

    #[test]
    fn a_keybag_unlocks_with_the_right_password_only() {
        let class3 = [0x33; 32];
        let class4 = [0x44; 32];
        let (blob, kek) = make_keybag("correct horse", &[(3, class3), (4, class4)]);
        let bag = Keybag::parse(&blob).unwrap();
        assert_eq!(bag.class_keys.len(), 2);
        assert_eq!(bag.slow_rounds(), 9);
        let key = bag.derive_key("correct horse", |_| true).unwrap();
        assert_eq!(*key, kek);
        let keys = bag.unlock(&key).unwrap();
        // A file key wrapped with class 3, tagged the way a backup stores it.
        let file_key = [0x5A; 32];
        let mut tagged = 3u32.to_le_bytes().to_vec();
        tagged.extend(wrap(&class3, &file_key));
        assert_eq!(*keys.unwrap_tagged(&tagged).unwrap(), file_key);
        assert!(keys.unwrap_tagged(&tagged[..10]).is_none());
        let mut wrong_class = 4u32.to_le_bytes().to_vec();
        wrong_class.extend(wrap(&class3, &file_key));
        assert!(
            keys.unwrap_tagged(&wrong_class).is_none(),
            "wrapped with another class's key"
        );

        let wrong = bag.derive_key("Correct horse", |_| true).unwrap();
        assert!(bag.unlock(&wrong).is_none(), "a wrong password unlocks nothing");
    }

    #[test]
    fn an_older_keybag_without_the_sha256_step_still_derives() {
        let mut bag = Keybag::parse(&make_keybag("pw", &[(1, [1; 32])]).0).unwrap();
        bag.dp_salt = None;
        bag.dp_iterations = None;
        let mut want = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"pw", &bag.salt, bag.iterations, &mut want);
        assert_eq!(*bag.derive_key("pw", |_| true).unwrap(), want);
    }

    #[test]
    fn a_keybag_asking_for_endless_sha1_rounds_is_refused() {
        let (blob, _) = make_keybag("pw", &[(3, [3; 32])]);
        let with_iter = |n: u32| {
            let at = blob.windows(8).position(|w| w == b"ITER\0\0\0\x04").unwrap() + 8;
            let mut b = blob.clone();
            b[at..at + 4].copy_from_slice(&n.to_be_bytes());
            b
        };
        assert_eq!(
            Keybag::parse(&with_iter(MAX_ITERATIONS)).unwrap().iterations,
            MAX_ITERATIONS
        );
        assert_eq!(
            Keybag::parse(&with_iter(MAX_ITERATIONS + 1)),
            Err(KeybagError::Malformed)
        );
        assert_eq!(Keybag::parse(&with_iter(u32::MAX)), Err(KeybagError::Malformed));
    }

    #[test]
    fn malformed_keybags_are_refused_not_panicked_on() {
        let (blob, _) = make_keybag("pw", &[(3, [3; 32])]);
        for cut in 0..blob.len() {
            // Any cut either still parses (it ended between items) or is refused.
            let _ = Keybag::parse(&blob[..cut]);
        }
        assert_eq!(Keybag::parse(b""), Err(KeybagError::Malformed));
        assert_eq!(Keybag::parse(b"SALT\xFF\xFF\xFF\xFFxx"), Err(KeybagError::Malformed));
        // A bag with no class keys can't unlock anything.
        assert!(Keybag::parse(&tlv(b"SALT", b"s")).is_err());
    }

    pub(crate) fn hex<const N: usize>(s: &str) -> [u8; N] {
        let bytes: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        bytes.try_into().unwrap()
    }
}
