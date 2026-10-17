//! Reading a file out of an iPhone backup: AES-256-CBC with a zero IV and PKCS#7 padding for an
//! encrypted backup, a plain copy otherwise. Files are streamed in chunks (a video can be gigabytes),
//! hashed as they're written (SHA-256, for deduplicating attachments), and checked for a way out
//! between chunks so Cancel is quick.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use aes::cipher::common::array::Array;
use aes::cipher::{BlockModeDecrypt, KeyIvInit};
use sha2::{Digest, Sha256};

use super::keybag::Key;

type Decryptor = cbc::Decryptor<aes::Aes256>;

const BLOCK: usize = 16;
/// Read size: a multiple of the block size.
const CHUNK: usize = 1 << 20;

#[derive(Debug)]
pub enum CopyError {
    Io(io::Error),
    /// The data isn't whole AES blocks, so it isn't what the manifest says it is.
    Corrupt,
    Cancelled,
}

impl From<io::Error> for CopyError {
    fn from(e: io::Error) -> Self {
        CopyError::Io(e)
    }
}

/// What a copy wrote: its length and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub bytes: u64,
    pub sha256: [u8; 32],
}

/// Decrypt a whole buffer in place (Manifest.db and other small files). Returns the plaintext
/// length: the padding is stripped when it's valid PKCS#7, and the result is cut to `size` when the
/// manifest gives one.
pub fn decrypt_in_place(key: &Key, buf: &mut [u8], size: Option<u64>) -> Result<usize, CopyError> {
    if !buf.len().is_multiple_of(BLOCK) {
        return Err(CopyError::Corrupt);
    }
    let mut dec = Decryptor::new((&**key).into(), &[0u8; BLOCK].into());
    decrypt_blocks(&mut dec, buf);
    Ok(plain_len(buf, size))
}

fn decrypt_blocks(dec: &mut Decryptor, buf: &mut [u8]) {
    let (blocks, rest) = Array::slice_as_chunks_mut(buf);
    debug_assert!(rest.is_empty());
    dec.decrypt_blocks(blocks);
}

/// How much of decrypted data is the file: `size` when the manifest says (never more than there
/// is), else without a valid PKCS#7 pad.
fn plain_len(data: &[u8], size: Option<u64>) -> usize {
    if let Some(size) = size {
        return usize::try_from(size).map_or(data.len(), |s| s.min(data.len()));
    }
    match data.last() {
        Some(&pad) if (1..=BLOCK as u8).contains(&pad) && data.len() >= pad as usize => {
            let start = data.len() - pad as usize;
            if data[start..].iter().all(|&b| b == pad) {
                start
            } else {
                data.len()
            }
        }
        _ => data.len(),
    }
}

/// The output side of a copy: writes, hashes and counts.
struct Sink {
    out: io::BufWriter<File>,
    hasher: Sha256,
    written: u64,
}

impl Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn finish(mut self) -> io::Result<Copied> {
        self.out.flush()?;
        Ok(Copied {
            bytes: self.written,
            sha256: self.hasher.finalize().into(),
        })
    }
}

/// Decrypt (with `key`) or copy (without) `src` into `dst`, chunk by chunk. `go_on()` is asked
/// between chunks; false stops with `Cancelled` (the caller deletes the partial file).
pub fn copy_file(
    src: &Path,
    dst: &Path,
    key: Option<&Key>,
    size: Option<u64>,
    mut go_on: impl FnMut() -> bool,
) -> Result<Copied, CopyError> {
    let mut input = File::open(src)?;
    let mut sink = Sink {
        out: io::BufWriter::new(File::create(dst)?),
        hasher: Sha256::new(),
        written: 0,
    };
    let mut buf = vec![0u8; CHUNK];
    let Some(key) = key else {
        loop {
            if !go_on() {
                return Err(CopyError::Cancelled);
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                return Ok(sink.finish()?);
            }
            sink.write(&buf[..n])?;
        }
    };
    let mut dec = Decryptor::new((&**key).into(), &[0u8; BLOCK].into());
    // The last block holds the padding, so the newest decrypted block waits until the next chunk
    // (or the end) shows whether it's the last.
    let mut held: Vec<u8> = Vec::with_capacity(BLOCK);
    let mut filled = 0usize;
    loop {
        if !go_on() {
            return Err(CopyError::Cancelled);
        }
        let n = input.read(&mut buf[filled..])?;
        filled += n;
        let eof = n == 0;
        let whole = filled - filled % BLOCK;
        if whole > 0 && (filled == CHUNK || eof) {
            decrypt_blocks(&mut dec, &mut buf[..whole]);
            sink.write(&held)?;
            sink.write(&buf[..whole - BLOCK])?;
            held.clear();
            held.extend_from_slice(&buf[whole - BLOCK..whole]);
            buf.copy_within(whole..filled, 0);
            filled -= whole;
        }
        if eof {
            if filled != 0 {
                return Err(CopyError::Corrupt);
            }
            break;
        }
    }
    // The final block, without its padding (or cut to the manifest's size).
    let keep = match size {
        Some(size) => usize::try_from(size.saturating_sub(sink.written)).map_or(held.len(), |s| s.min(held.len())),
        None => plain_len(&held, None),
    };
    sink.write(&held[..keep])?;
    let copied = sink.finish()?;
    match size {
        // A manifest that says less than the earlier blocks held gets exactly what it says (it
        // can't happen for a real file, whose padding is all in the last block).
        Some(size) if copied.bytes > size => {
            File::options().write(true).open(dst)?.set_len(size)?;
            copied_from_disk(dst)
        }
        _ => Ok(copied),
    }
}

/// Hash a file already written (the rare cut-to-size case above).
fn copied_from_disk(path: &Path) -> Result<Copied, CopyError> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut bytes = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        bytes += n as u64;
    }
    Ok(Copied {
        bytes,
        sha256: hasher.finalize().into(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aes::cipher::block_padding::Pkcs7;
    use aes::cipher::BlockModeEncrypt;
    use zeroize::Zeroizing;

    pub(crate) fn encrypt(key: &[u8; 32], plain: &[u8]) -> Vec<u8> {
        let mut buf = plain.to_vec();
        buf.resize(plain.len() + BLOCK, 0);
        let len = cbc::Encryptor::<aes::Aes256>::new(key.into(), &[0u8; BLOCK].into())
            .encrypt_padded::<Pkcs7>(&mut buf, plain.len())
            .unwrap()
            .len();
        buf.truncate(len);
        buf
    }

    fn key(b: u8) -> Key {
        Zeroizing::new([b; 32])
    }

    #[test]
    fn decrypts_in_place_and_strips_the_padding() {
        let plain = b"SQLite format 3\0 and then some pages".to_vec();
        let mut buf = encrypt(&[7; 32], &plain);
        let n = decrypt_in_place(&key(7), &mut buf, None).unwrap();
        assert_eq!(&buf[..n], &plain[..]);
        let mut buf = encrypt(&[7; 32], &plain);
        let n = decrypt_in_place(&key(7), &mut buf, Some(6)).unwrap();
        assert_eq!(&buf[..n], b"SQLite");
        assert!(matches!(
            decrypt_in_place(&key(7), &mut [0u8; 15], None),
            Err(CopyError::Corrupt)
        ));
    }

    #[test]
    fn streams_files_of_every_awkward_size() {
        let dir = std::env::temp_dir().join(format!("tug-decrypt-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for len in [0usize, 1, 15, 16, 17, CHUNK - 1, CHUNK, CHUNK + 1, CHUNK * 2 + 33] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let src = dir.join("src");
            let dst = dir.join("dst");
            std::fs::write(&src, encrypt(&[9; 32], &plain)).unwrap();
            for size in [None, Some(len as u64)] {
                let got = copy_file(&src, &dst, Some(&key(9)), size, || true).unwrap();
                assert_eq!(std::fs::read(&dst).unwrap(), plain, "len {len} size {size:?}");
                assert_eq!(got.bytes, len as u64);
                assert_eq!(got.sha256, <[u8; 32]>::from(Sha256::digest(&plain)));
            }
            // Unencrypted backups: a plain copy, hashed the same way.
            std::fs::write(&src, &plain).unwrap();
            let got = copy_file(&src, &dst, None, None, || true).unwrap();
            assert_eq!(std::fs::read(&dst).unwrap(), plain);
            assert_eq!(got.sha256, <[u8; 32]>::from(Sha256::digest(&plain)));
        }
        // Cut to what the manifest says.
        std::fs::write(dir.join("src"), encrypt(&[9; 32], b"0123456789abcdefXYZ")).unwrap();
        let got = copy_file(&dir.join("src"), &dir.join("dst"), Some(&key(9)), Some(10), || true).unwrap();
        assert_eq!(std::fs::read(dir.join("dst")).unwrap(), b"0123456789");
        assert_eq!(got.bytes, 10);
        // Not whole blocks: not a backup file.
        std::fs::write(dir.join("src"), [1u8; 20]).unwrap();
        assert!(matches!(
            copy_file(&dir.join("src"), &dir.join("dst"), Some(&key(9)), None, || true),
            Err(CopyError::Corrupt)
        ));
        // Cancel stops between chunks.
        std::fs::write(dir.join("src"), encrypt(&[9; 32], &vec![0u8; CHUNK * 3])).unwrap();
        let mut asks = 0;
        let r = copy_file(&dir.join("src"), &dir.join("dst"), Some(&key(9)), None, || {
            asks += 1;
            asks < 2
        });
        assert!(matches!(r, Err(CopyError::Cancelled)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
