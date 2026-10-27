//! The accurate engine's speech model: one file, downloaded once when the person asks, from a
//! fixed address, checked against a fixed SHA-256 before it's ever used.
//!
//! - **Where from.** The official whisper.cpp models on Hugging Face, at a pinned commit, so the
//!   address always serves the same bytes. Redirects (Hugging Face hands the file to its CDN) are
//!   followed over HTTPS only; whatever arrives must match [`MODEL`]'s hash or it's thrown away.
//! - **Where to.** `speech\` under tug's local app data (not the roaming profile: it's large).
//!   The download goes to `<file>.part` and is renamed only after the hash matches, so a model
//!   file that exists is always a whole, checked one (until something outside tug changes it,
//!   which loading finds: see `win/whisper.rs`).
//! - **Resuming.** An interrupted download (no network, tug closed) leaves its `.part`; the next
//!   one asks for the rest with a Range request and starts over if the server won't. Cancel
//!   deletes the `.part`: the person said they don't want it.
//! - **Privacy.** This is the only network use voice typing has, and it carries nothing about the
//!   person: a plain GET for a public file. Recognition itself never touches the network.
//!
//! The download is written against [`Fetch`], so the tests drive it with a fake server.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// A speech model tug can download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// The file name on disk (and on the server).
    pub file: &'static str,
    pub url: &'static str,
    /// Its exact size in bytes.
    pub size: u64,
    /// Its SHA-256, lowercase hex.
    pub sha256: &'static str,
}

/// whisper.cpp's English "base" model, 8-bit quantized (ggml q8_0), from the commit of
/// huggingface.co/ggerganov/whisper.cpp it was checked against. Measured against base.en (16-bit,
/// 141 MB), small.en (465 MB) and small.en q5_1 (181 MB) on the test phrases (clean, noisy and
/// hands-free; `win/whisper.rs`): exactly as accurate as base.en at about half the size and a
/// little faster, and small was no more accurate at three to four times the time. See
/// docs/ARCHITECTURE.md (Voice typing).
pub const MODEL: ModelSpec = ModelSpec {
    file: "ggml-base.en-q8_0.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en-q8_0.bin",
    size: 81_781_811,
    sha256: "a4d4a0768075e13cfd7e19df3ae2dbc4a68d37d36a7dad45e8410c9a34f8c87e",
};

/// The folder under tug's local app data that holds the model.
pub const DIR: &str = "speech";

/// Where the model is. Mirrored in `src/types/protocol.ts` (`VoiceModelState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelState {
    /// Not downloaded (or a download is paused part way: `received` says how far).
    Missing,
    Downloading,
    /// Downloaded and checked.
    Ready,
}

/// The model's status for Settings. Mirrored in `src/types/protocol.ts` (`VoiceModel`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub state: ModelState,
    /// Bytes downloaded so far (a paused download's `.part` counts).
    pub received: u64,
    /// The model's size in bytes.
    pub total: u64,
    /// What went wrong with the last download, or with the model on loading, for the person.
    pub error: Option<String>,
}

impl ModelStatus {
    pub fn new(state: ModelState, received: u64, total: u64) -> Self {
        ModelStatus {
            state,
            received,
            total,
            error: None,
        }
    }
}

/// Why a download stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// The person cancelled it.
    Cancelled,
    /// The connection failed or dropped (what happened, for the log). The `.part` is kept.
    Network(String),
    /// The server answered with something other than the file.
    Http(u16),
    /// More bytes than the model's size arrived.
    TooBig,
    /// The whole file arrived but isn't the model (wrong hash).
    Mismatch,
    /// Writing to the disk failed (full, or not allowed).
    Disk(String),
}

impl DownloadError {
    /// What Settings says, or `None` for a cancel (that's not a problem).
    pub fn message(&self) -> Option<&'static str> {
        Some(match self {
            DownloadError::Cancelled => return None,
            DownloadError::Http(code) if source_unavailable(*code) => {
                "The speech model's download site isn't available right now. Try again later."
            }
            DownloadError::Network(_) | DownloadError::Http(_) => {
                "The download stopped. Check your internet connection and try again; it picks up where it left off."
            }
            DownloadError::TooBig | DownloadError::Mismatch => {
                "The downloaded file wasn't the speech model, so tug deleted it. Try again."
            }
            DownloadError::Disk(_) => {
                "tug couldn't save the speech model. Check there's free space on this PC and try again."
            }
        })
    }

    /// The detail for tug.log (never anything about the person).
    pub fn detail(&self) -> String {
        match self {
            DownloadError::Cancelled => "cancelled".into(),
            DownloadError::Network(e) => format!("network: {e}"),
            DownloadError::Http(code) => format!("HTTP {code}"),
            DownloadError::TooBig => "more data than the model's size".into(),
            DownloadError::Mismatch => "SHA-256 mismatch".into(),
            DownloadError::Disk(e) => format!("disk: {e}"),
        }
    }
}

/// Whether an HTTP status means the file isn't being served (gone, refused or rate-limited),
/// rather than a connection that dropped: trying again straight away won't help, so Settings
/// doesn't promise to pick up where it left off.
pub fn source_unavailable(code: u16) -> bool {
    // A 3xx only ends up here when a redirect was refused (off Hugging Face, not HTTPS, or too
    // many), which resuming won't fix either.
    matches!(code, 300..=399 | 401 | 403 | 404 | 410 | 429 | 451)
}

/// Where the download may be redirected: Hugging Face itself and its download servers
/// (`*.hf.co`, which includes the `cdn` and `xethub` hosts). Anywhere else is refused, even over
/// HTTPS, though the hash would catch a wrong file anyway.
pub fn allowed_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "huggingface.co" || host == "hf.co" || host.ends_with(".hf.co")
}

/// What the server sent back for a request.
pub struct Opened {
    /// The status code (200 the whole file, 206 the rest of it from the offset asked for).
    pub status: u16,
    pub body: Box<dyn Read + Send>,
}

/// The HTTP layer: GET `url`, from byte `from` (0: the whole file).
pub trait Fetch {
    fn open(&mut self, url: &str, from: u64) -> Result<Opened, DownloadError>;
}

pub fn model_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(spec.file)
}

pub fn part_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(format!("{}.part", spec.file))
}

/// The model, if it's there at its exact size. (The hash is checked when it's downloaded and
/// again when it's loaded, not on every look.)
pub fn installed(dir: &Path, spec: &ModelSpec) -> Option<PathBuf> {
    let path = model_path(dir, spec);
    fs::metadata(&path)
        .is_ok_and(|m| m.is_file() && m.len() == spec.size)
        .then_some(path)
}

/// How much of a paused download is on disk.
pub fn partial(dir: &Path, spec: &ModelSpec) -> u64 {
    fs::metadata(part_path(dir, spec))
        .map(|m| m.len().min(spec.size))
        .unwrap_or(0)
}

/// The status from what's on disk (not downloading).
pub fn status_on_disk(dir: &Path, spec: &ModelSpec) -> ModelStatus {
    match installed(dir, spec) {
        Some(_) => ModelStatus::new(ModelState::Ready, spec.size, spec.size),
        None => ModelStatus::new(ModelState::Missing, partial(dir, spec), spec.size),
    }
}

/// The SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether the file at `path` is exactly the model: its size and its hash.
pub fn verify(path: &Path, spec: &ModelSpec) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() == spec.size)
        && sha256_file(path).is_ok_and(|h| h.eq_ignore_ascii_case(spec.sha256))
}

/// Whether `bytes` (the model file, read into memory) are exactly the model: its size and its hash.
/// Loading checks the very bytes it hands to whisper.cpp, so nothing can change in between.
pub fn verify_bytes(bytes: &[u8], spec: &ModelSpec) -> bool {
    if bytes.len() as u64 != spec.size {
        return false;
    }
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    hex.eq_ignore_ascii_case(spec.sha256)
}

/// Delete the model and any paused download (a damaged model, or the person removing it).
pub fn remove(dir: &Path, spec: &ModelSpec) {
    let _ = fs::remove_file(model_path(dir, spec));
    let _ = fs::remove_file(part_path(dir, spec));
}

/// How much is read and written at a time; also how often a cancel is noticed.
const CHUNK: usize = 64 * 1024;

fn disk(e: std::io::Error) -> DownloadError {
    DownloadError::Disk(e.to_string())
}

/// Download `spec` into `dir`, resuming a `.part` if there is one, and return the model's path
/// once its hash matches. `progress(received, total)` is called as bytes arrive (the caller
/// decides how often to pass it on). `cancel` stops it within one chunk and deletes the `.part`.
pub fn download(
    fetch: &mut dyn Fetch,
    spec: &ModelSpec,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, DownloadError> {
    fs::create_dir_all(dir).map_err(disk)?;
    let target = model_path(dir, spec);
    if installed(dir, spec).is_some() {
        if verify(&target, spec) {
            progress(spec.size, spec.size);
            return Ok(target);
        }
        let _ = fs::remove_file(&target);
    }
    let part = part_path(dir, spec);
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > spec.size {
        let _ = fs::remove_file(&part);
        have = 0;
    }
    if let Err(e) = fetch_into(fetch, spec, &part, have, cancel, progress) {
        // A cancel or an overlong file leaves nothing behind; anything else keeps the `.part` to
        // resume from.
        if matches!(e, DownloadError::Cancelled | DownloadError::TooBig) {
            let _ = fs::remove_file(&part);
        }
        return Err(e);
    }
    if !verify(&part, spec) {
        let _ = fs::remove_file(&part);
        return Err(DownloadError::Mismatch);
    }
    fs::rename(&part, &target).map_err(disk)?;
    Ok(target)
}

/// Fill `part` up to the model's size, starting from the `have` bytes already in it.
fn fetch_into(
    fetch: &mut dyn Fetch,
    spec: &ModelSpec,
    part: &Path,
    mut have: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), DownloadError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(DownloadError::Cancelled);
    }
    if have == spec.size {
        progress(have, spec.size);
        return Ok(());
    }
    let opened = fetch.open(spec.url, have)?;
    let append = match opened.status {
        206 if have > 0 => true,
        // The whole file: the server ignored the range (or there was nothing to resume).
        200 => false,
        code => return Err(DownloadError::Http(code)),
    };
    if !append {
        have = 0;
    }
    let mut file = if append {
        OpenOptions::new().append(true).open(part)
    } else {
        File::create(part)
    }
    .map_err(disk)?;
    progress(have, spec.size);
    let mut body = opened.body;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(DownloadError::Cancelled);
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let _ = file.flush();
                return Err(DownloadError::Network(e.to_string()));
            }
        };
        if have + n as u64 > spec.size {
            return Err(DownloadError::TooBig);
        }
        file.write_all(&buf[..n]).map_err(disk)?;
        have += n as u64;
        progress(have, spec.size);
    }
    file.sync_all().map_err(disk)?;
    if have < spec.size {
        return Err(DownloadError::Network(format!(
            "the connection closed at {have} of {} bytes",
            spec.size
        )));
    }
    Ok(())
}

/// Whether to pass a progress update on: at most every `step` bytes, and always the last one.
pub fn progress_due(last_sent: u64, received: u64, total: u64, step: u64) -> bool {
    received == total || received >= last_sent.saturating_add(step) || received < last_sent
}

/// The real HTTP layer: reqwest on Tauri's async runtime, driven from the download thread.
pub struct HttpFetch {
    client: reqwest::Client,
}

impl HttpFetch {
    pub fn new() -> Result<Self, DownloadError> {
        // Built inside the runtime: reqwest's connector needs Tokio's reactor and timer, and the
        // download runs on a plain thread of its own.
        tauri::async_runtime::block_on(async { Self::build() })
    }

    fn build() -> Result<Self, DownloadError> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            // A stalled transfer fails (and can be resumed) instead of hanging.
            .read_timeout(std::time::Duration::from_secs(30))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let host_ok = attempt.url().host_str().is_some_and(allowed_host);
                if attempt.previous().len() >= 5 || attempt.url().scheme() != "https" || !host_ok {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .user_agent(concat!("tug/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| DownloadError::Network(e.to_string()))?;
        Ok(HttpFetch { client })
    }
}

struct HttpBody {
    response: reqwest::Response,
    pending: Vec<u8>,
    pos: usize,
}

impl Read for HttpBody {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.pending.len() {
            let response = &mut self.response;
            match tauri::async_runtime::block_on(async move { response.chunk().await }) {
                Ok(Some(chunk)) => {
                    self.pending = chunk.to_vec();
                    self.pos = 0;
                }
                Ok(None) => return Ok(0),
                Err(e) => return Err(std::io::Error::other(e.to_string())),
            }
        }
        let n = out.len().min(self.pending.len() - self.pos);
        out[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Fetch for HttpFetch {
    fn open(&mut self, url: &str, from: u64) -> Result<Opened, DownloadError> {
        let mut request = self.client.get(url);
        if from > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={from}-"));
        }
        // `send` starts its timers when called, so it's called inside the runtime.
        let response = tauri::async_runtime::block_on(async move { request.send().await })
            .map_err(|e| DownloadError::Network(e.to_string()))?;
        Ok(Opened {
            status: response.status().as_u16(),
            body: Box::new(HttpBody {
                response,
                pending: Vec::new(),
                pos: 0,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A tiny stand-in model: 300 KB of known bytes (several chunks).
    fn bytes() -> Vec<u8> {
        (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn sha_hex(data: &[u8]) -> String {
        Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A spec for `data`, leaked so it's `'static` like the real one.
    fn spec_for(data: &[u8]) -> ModelSpec {
        ModelSpec {
            file: "test-model.bin",
            url: "https://example.com/test-model.bin",
            size: data.len() as u64,
            sha256: Box::leak(sha_hex(data).into_boxed_str()),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tug-model-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// How the fake server behaves.
    #[derive(Clone, Copy, PartialEq)]
    enum Server {
        /// Honours Range with 206.
        Ranges,
        /// Ignores Range: always 200 with the whole file.
        NoRanges,
        /// Sends only the first `n` bytes, then the connection fails.
        DropsAt(usize),
        /// Answers with this status and no file.
        Status(u16),
    }

    struct FakeServer {
        data: Vec<u8>,
        behaviour: Server,
        /// The `from` of each request.
        requests: Vec<u64>,
    }

    struct Failing {
        inner: Cursor<Vec<u8>>,
    }

    impl Read for Failing {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.inner.read(buf)? {
                0 => Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset")),
                n => Ok(n),
            }
        }
    }

    impl Fetch for FakeServer {
        fn open(&mut self, _url: &str, from: u64) -> Result<Opened, DownloadError> {
            self.requests.push(from);
            let all = self.data.clone();
            Ok(match self.behaviour {
                Server::Status(code) => Opened {
                    status: code,
                    body: Box::new(Cursor::new(Vec::new())),
                },
                Server::NoRanges => Opened {
                    status: 200,
                    body: Box::new(Cursor::new(all)),
                },
                Server::Ranges => Opened {
                    status: if from > 0 { 206 } else { 200 },
                    body: Box::new(Cursor::new(all[from as usize..].to_vec())),
                },
                Server::DropsAt(n) => Opened {
                    status: if from > 0 { 206 } else { 200 },
                    body: Box::new(Failing {
                        inner: Cursor::new(all[from as usize..n.max(from as usize)].to_vec()),
                    }),
                },
            })
        }
    }

    fn server(data: &[u8], behaviour: Server) -> FakeServer {
        FakeServer {
            data: data.to_vec(),
            behaviour,
            requests: Vec::new(),
        }
    }

    fn run(fake: &mut FakeServer, spec: &ModelSpec, dir: &Path) -> (Result<PathBuf, DownloadError>, Vec<(u64, u64)>) {
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let r = download(fake, spec, dir, &cancel, &mut |got, total| seen.push((got, total)));
        (r, seen)
    }

    #[test]
    fn downloads_checks_the_hash_and_renames_into_place() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("fresh");
        let mut fake = server(&data, Server::Ranges);
        let (r, seen) = run(&mut fake, &spec, &dir);
        let path = r.expect("downloaded");
        assert_eq!(fs::read(&path).unwrap(), data);
        assert!(!part_path(&dir, &spec).exists(), "no temp file left");
        assert_eq!(installed(&dir, &spec), Some(path.clone()));
        assert_eq!(status_on_disk(&dir, &spec).state, ModelState::Ready);
        assert_eq!(fake.requests, vec![0]);
        assert_eq!(seen.last(), Some(&(data.len() as u64, data.len() as u64)));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "progress only goes up");

        // Already there: checked, nothing fetched.
        let (r, _) = run(&mut fake, &spec, &dir);
        assert_eq!(r, Ok(path));
        assert_eq!(fake.requests, vec![0], "no second request");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_file_is_deleted_and_never_installed() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("mismatch");
        let mut tampered = data.clone();
        tampered[1234] ^= 0xff;
        let mut fake = server(&tampered, Server::Ranges);
        let (r, _) = run(&mut fake, &spec, &dir);
        assert_eq!(r, Err(DownloadError::Mismatch));
        assert!(!part_path(&dir, &spec).exists(), "the bad bytes are gone");
        assert!(!model_path(&dir, &spec).exists(), "and never became the model");
        assert_eq!(
            status_on_disk(&dir, &spec),
            ModelStatus {
                state: ModelState::Missing,
                received: 0,
                total: spec.size,
                error: None
            }
        );
        assert!(DownloadError::Mismatch.message().unwrap().contains("deleted"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn too_many_bytes_stop_the_download() {
        let data = bytes();
        let spec = spec_for(&data[..1000]);
        let dir = temp_dir("toobig");
        let mut fake = server(&data, Server::NoRanges);
        let (r, _) = run(&mut fake, &spec, &dir);
        assert_eq!(r, Err(DownloadError::TooBig));
        assert!(!part_path(&dir, &spec).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dropped_connection_keeps_the_part_and_resumes_with_a_range() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("resume");
        let mut fake = server(&data, Server::DropsAt(100_000));
        let (r, _) = run(&mut fake, &spec, &dir);
        assert!(matches!(r, Err(DownloadError::Network(_))), "{r:?}");
        assert_eq!(partial(&dir, &spec), 100_000, "what arrived is kept");
        assert_eq!(
            status_on_disk(&dir, &spec).received,
            100_000,
            "Settings can say how far"
        );

        let mut fake = server(&data, Server::Ranges);
        let (r, seen) = run(&mut fake, &spec, &dir);
        let path = r.expect("resumed to the end");
        assert_eq!(fake.requests, vec![100_000], "asked only for the rest");
        assert_eq!(
            seen.first(),
            Some(&(100_000, data.len() as u64)),
            "progress starts where it was"
        );
        assert_eq!(fs::read(&path).unwrap(), data, "the halves join up exactly");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_that_ignores_the_range_starts_over() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("norange");
        fs::create_dir_all(&dir).unwrap();
        fs::write(part_path(&dir, &spec), &data[..50_000]).unwrap();
        let mut fake = server(&data, Server::NoRanges);
        let (r, _) = run(&mut fake, &spec, &dir);
        let path = r.expect("downloaded from the start");
        assert_eq!(fake.requests, vec![50_000]);
        assert_eq!(fs::read(&path).unwrap(), data, "not appended to the old part");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_whole_part_is_just_checked() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("wholepart");
        fs::create_dir_all(&dir).unwrap();
        fs::write(part_path(&dir, &spec), &data).unwrap();
        let mut fake = server(&data, Server::Ranges);
        let (r, _) = run(&mut fake, &spec, &dir);
        assert!(r.is_ok());
        assert!(fake.requests.is_empty(), "nothing left to fetch");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_part_is_thrown_away() {
        let data = bytes();
        let spec = spec_for(&data[..2000]);
        let dir = temp_dir("bigpart");
        fs::create_dir_all(&dir).unwrap();
        fs::write(part_path(&dir, &spec), &data[..5000]).unwrap();
        let mut fake = server(&data[..2000], Server::Ranges);
        let (r, _) = run(&mut fake, &spec, &dir);
        assert!(r.is_ok());
        assert_eq!(fake.requests, vec![0]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_stops_and_deletes_the_part() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("cancel");
        let mut fake = server(&data, Server::Ranges);
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let r = download(&mut fake, &spec, &dir, &cancel, &mut |got, _| {
            calls += 1;
            // Cancel once some data has arrived, as the button would.
            if got > 0 {
                cancel.store(true, Ordering::SeqCst);
            }
        });
        assert_eq!(r, Err(DownloadError::Cancelled));
        assert!(!part_path(&dir, &spec).exists(), "temp file cleaned up");
        assert!(!model_path(&dir, &spec).exists());
        assert!(calls >= 1);
        assert_eq!(DownloadError::Cancelled.message(), None, "a cancel isn't an error");

        // Cancelled before it starts: nothing is fetched.
        let mut fake = server(&data, Server::Ranges);
        let r = download(&mut fake, &spec, &dir, &AtomicBool::new(true), &mut |_, _| {});
        assert_eq!(r, Err(DownloadError::Cancelled));
        assert!(fake.requests.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_http_error_is_reported_and_keeps_what_was_there() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("http");
        fs::create_dir_all(&dir).unwrap();
        fs::write(part_path(&dir, &spec), &data[..10]).unwrap();
        let mut fake = server(&data, Server::Status(503));
        let (r, _) = run(&mut fake, &spec, &dir);
        assert_eq!(r, Err(DownloadError::Http(503)));
        assert_eq!(partial(&dir, &spec), 10);
        assert!(DownloadError::Http(503)
            .message()
            .unwrap()
            .contains("picks up where it left off"));
        for code in [403, 404, 429] {
            let msg = DownloadError::Http(code).message().unwrap();
            assert!(msg.contains("isn't available right now"), "{code}: {msg}");
            assert!(!msg.contains("picks up"), "{code}: no promise to resume");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_model_on_disk_is_downloaded_again() {
        let data = bytes();
        let spec = spec_for(&data);
        let dir = temp_dir("damaged");
        fs::create_dir_all(&dir).unwrap();
        let mut damaged = data.clone();
        damaged[0] ^= 1;
        fs::write(model_path(&dir, &spec), &damaged).unwrap();
        assert!(installed(&dir, &spec).is_some(), "the right size passes the quick look");
        assert!(!verify(&model_path(&dir, &spec), &spec), "but not the hash");
        assert!(!verify_bytes(&damaged, &spec), "nor in memory, as loading checks it");
        assert!(verify_bytes(&data, &spec));
        assert!(!verify_bytes(&data[..10], &spec), "short");
        let mut fake = server(&data, Server::Ranges);
        let (r, _) = run(&mut fake, &spec, &dir);
        assert_eq!(fs::read(r.unwrap()).unwrap(), data);
        remove(&dir, &spec);
        assert_eq!(status_on_disk(&dir, &spec).state, ModelState::Missing);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_is_passed_on_in_steps() {
        assert!(!progress_due(0, 5, 100, 10));
        assert!(progress_due(0, 10, 100, 10));
        assert!(progress_due(90, 100, 100, 50), "the last one always");
        assert!(progress_due(50, 0, 100, 10), "starting over");
    }

    /// The real download from Hugging Face, with tug's real HTTP layer: redirects to the CDN, a
    /// resumed Range request and the hash check. Downloads the model (about 80 MB), so it's run by
    /// hand: `cargo test --manifest-path src-tauri/Cargo.toml -p tug --lib voice::model -- --ignored --nocapture`
    /// With `TUG_WHISPER_MODELS` pointing at a folder holding the model, its first 5 MB are put in
    /// place as a paused download first, so the resume is tested against the real server.
    #[test]
    #[ignore]
    fn downloads_the_real_model_and_resumes() {
        let dir = temp_dir("real");
        fs::create_dir_all(&dir).unwrap();
        if let Ok(have) = std::env::var("TUG_WHISPER_MODELS") {
            if let Ok(bytes) = fs::read(Path::new(&have).join(MODEL.file)) {
                fs::write(part_path(&dir, &MODEL), &bytes[..5_000_000]).unwrap();
                println!("resuming from 5,000,000 bytes");
            }
        }
        let mut fetch = HttpFetch::new().expect("client");
        let started = std::time::Instant::now();
        let mut first = None;
        let r = download(&mut fetch, &MODEL, &dir, &AtomicBool::new(false), &mut |got, _| {
            first.get_or_insert(got);
        });
        let path = r.expect("downloaded and checked");
        println!(
            "downloaded {} bytes in {} s (progress started at {:?})",
            fs::metadata(&path).unwrap().len(),
            started.elapsed().as_secs(),
            first
        );
        assert!(verify(&path, &MODEL));
        assert!(!part_path(&dir, &MODEL).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn redirects_only_to_hugging_face() {
        assert!(allowed_host("huggingface.co"));
        assert!(allowed_host("cdn.hf.co"));
        assert!(allowed_host("us.aws.cdn.hf.co"));
        assert!(allowed_host("cas-bridge.xethub.hf.co"));
        assert!(allowed_host("HuggingFace.co."));
        assert!(!allowed_host("huggingface.co.example.com"));
        assert!(!allowed_host("evilhf.co"));
        assert!(!allowed_host("example.com"));
        assert!(!allowed_host("objects.githubusercontent.com"));
        assert!(source_unavailable(404) && source_unavailable(429) && !source_unavailable(503));
        assert!(source_unavailable(302), "a refused redirect");
    }

    #[test]
    fn the_pinned_model_is_well_formed() {
        assert_eq!(MODEL.sha256.len(), 64);
        assert!(MODEL
            .sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(MODEL
            .url
            .starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/"));
        assert!(MODEL.url.ends_with(MODEL.file));
        // Pinned to a commit, never a branch that can move.
        let rev = MODEL.url.split('/').nth(6).unwrap();
        assert_eq!(rev.len(), 40, "{rev}");
        assert!(rev.chars().all(|c| c.is_ascii_hexdigit()));
        const { assert!(MODEL.size > 50_000_000) };
    }
}
