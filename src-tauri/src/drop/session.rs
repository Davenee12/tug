//! One Drop session: its keys, who's bound to it, files coming in and on offer, and texts. The
//! HTTP handlers (`server.rs`) and the Tauri glue (`mod.rs`) both work through this; nothing here
//! knows about Tauri, so the integration test can run a whole session over a socket.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::auth::{Auth, Authorized, Rejected};
use super::crypto::{self, ad, Keys};
use super::names;
use super::upload::{Plan, PlanError, Received};

/// Largest file the PC offers to the phone: the page assembles it in memory before saving.
pub const MAX_OFFER: u64 = 1024 * 1024 * 1024;
/// Chunk size for files going to the phone.
pub const OFFER_CHUNK: u32 = 2 * 1024 * 1024;
const MAX_OFFERS: usize = 50;
const MAX_UPLOADS: usize = 500;
/// Longest text either way, in bytes.
pub const MAX_TEXT: usize = 256 * 1024;
const MAX_TEXTS: usize = 20;
/// Room to leave on the disk beyond what's being received.
const DISK_MARGIN: u64 = 256 * 1024 * 1024;
/// The page polls every ~2 s while it's on screen; quieter than this and it's in the background.
const ACTIVE: Duration = Duration::from_secs(8);

/// What the session tells the outside world.
pub trait Sink: Send + Sync {
    /// Something the Drop panel shows changed. `urgent` for state changes; progress isn't, and
    /// may be coalesced.
    fn changed(&self, urgent: bool);
    /// The phone sent text: it goes on the PC clipboard.
    fn text(&self, text: &str);
}

/// Why an API request failed. Mapped to an HTTP status and a short code (never content).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    /// The session is over (Drop closed, or a new code was made).
    Closed,
    Unauthorized,
    /// Another device already has this session.
    InUse,
    NotFound,
    BadRequest,
    /// A chunk didn't decrypt, or had the wrong length.
    BadChunk,
    TooBig,
    NoSpace,
    /// Finish asked for before every chunk arrived.
    Incomplete,
    /// A file on offer changed or vanished on the PC.
    Changed,
    Io,
}

impl ApiError {
    pub fn status(self) -> u16 {
        match self {
            ApiError::Closed => 410,
            ApiError::Unauthorized => 401,
            ApiError::InUse => 403,
            ApiError::NotFound => 404,
            ApiError::BadRequest | ApiError::BadChunk => 400,
            ApiError::TooBig => 413,
            ApiError::NoSpace => 507,
            ApiError::Incomplete | ApiError::Changed => 409,
            ApiError::Io => 500,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            ApiError::Closed => "closed",
            ApiError::Unauthorized => "unauthorized",
            ApiError::InUse => "in-use",
            ApiError::NotFound => "not-found",
            ApiError::BadRequest => "bad-request",
            ApiError::BadChunk => "bad-chunk",
            ApiError::TooBig => "too-big",
            ApiError::NoSpace => "no-space",
            ApiError::Incomplete => "incomplete",
            ApiError::Changed => "changed",
            ApiError::Io => "io",
        }
    }
}

fn io_err(context: &str, e: std::io::Error) -> ApiError {
    // The error kind only: paths here name the user's files.
    log::warn!("drop: {context} failed: {:?}", e.kind());
    if e.raw_os_error() == Some(112) {
        // ERROR_DISK_FULL
        return ApiError::NoSpace;
    }
    ApiError::Io
}

// --- Messages the page sends and receives (inside sealed bodies) ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    pub name: String,
    pub size: u64,
    pub chunk_size: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadReply {
    /// Chunks already written, as `[start, end)` ranges: the page sends the rest.
    pub received: Vec<[u32; 2]>,
    pub chunks: u32,
    /// Set once the file is saved (a finish whose reply the phone never got).
    pub saved_as: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FinishReply {
    pub saved_as: String,
}

#[derive(Debug, Deserialize)]
pub struct TextRequest {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageOffer {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub chunk_size: u32,
    pub chunks: u32,
    #[serde(rename = "type")]
    pub mime: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageText {
    pub id: u32,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateReply {
    pub offers: Vec<PageOffer>,
    pub text: Option<PageText>,
}

// --- What the Drop panel shows (mirrored in src/types/protocol.ts) ---

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DropIncoming {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub received: u64,
    pub done: bool,
    /// Full path once saved, for "Show in folder".
    pub path: Option<String>,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DropOffer {
    pub id: String,
    pub name: String,
    pub size: u64,
    /// Times the phone has fetched the whole file.
    pub downloads: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DropText {
    pub id: u32,
    pub text: String,
    pub at: i64,
}

/// The session part of `DropStatus`.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub phone: Option<String>,
    pub phone_active: bool,
    pub incoming: Vec<DropIncoming>,
    pub outgoing: Vec<DropOffer>,
    pub texts: Vec<DropText>,
    pub sent_text: Option<String>,
}

/// A file offered to the phone that couldn't be added, and why (for the panel's message).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub name: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    TooBig,
    Folder,
    Unreadable,
    TooMany,
}

enum Phase {
    Sending,
    Saved { path: PathBuf, name: String },
}

struct Upload {
    id: String,
    name: String,
    plan: Plan,
    received: Received,
    bytes: u64,
    phase: Phase,
    at: i64,
}

struct Offer {
    id: String,
    name: String,
    path: PathBuf,
    size: u64,
    mime: &'static str,
    downloads: u32,
}

struct State {
    auth: Auth,
    phone: Option<String>,
    last_seen: Option<Instant>,
    last_activity: Instant,
    uploads: Vec<Upload>,
    offers: Vec<Offer>,
    texts: Vec<DropText>,
    pc_text: Option<PageText>,
    next_text_id: u32,
}

pub struct Session {
    pub keys: Keys,
    /// `Pictures\tug Drop`.
    pub folder: PathBuf,
    /// Where unfinished uploads are written (tug's local app data, not the Pictures folder, so a
    /// OneDrive-synced Pictures never uploads half-received files). Emptied on start and stop.
    pub incoming: PathBuf,
    state: Mutex<State>,
    closed: AtomicBool,
    sink: Arc<dyn Sink>,
    /// One finish at a time, so two files can't claim the same free name.
    finishing: Mutex<()>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Session {
    pub fn new(secret: &[u8], folder: PathBuf, incoming: PathBuf, sink: Arc<dyn Sink>) -> Session {
        Session {
            keys: Keys::derive(secret),
            folder,
            incoming,
            state: Mutex::new(State {
                auth: Auth::new(),
                phone: None,
                last_seen: None,
                last_activity: Instant::now(),
                uploads: Vec::new(),
                offers: Vec::new(),
                texts: Vec::new(),
                pc_text: None,
                next_text_id: 1,
            }),
            closed: AtomicBool::new(false),
            sink,
            finishing: Mutex::new(()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock leaves plain data behind; keep serving it.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn part_path(&self, id: &str) -> PathBuf {
        self.incoming.join(format!("{id}.part"))
    }

    // --- Lifetime ---

    /// End the session: every later request gets "closed", even on a connection still open.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Time since the last request (or action in the panel).
    pub fn idle_for(&self) -> Duration {
        self.state().last_activity.elapsed()
    }

    pub fn touch(&self) {
        self.state().last_activity = Instant::now();
    }

    pub fn phone_active(&self) -> bool {
        self.state().last_seen.is_some_and(|t| t.elapsed() < ACTIVE)
    }

    // --- Requests from the phone ---

    /// Check a request's `Authorization` header. The first good one binds the session.
    pub fn authorize(
        &self,
        header: Option<&str>,
        method: &str,
        path: &str,
        user_agent: Option<&str>,
    ) -> Result<Authorized, ApiError> {
        if self.is_closed() {
            return Err(ApiError::Closed);
        }
        let mut st = self.state();
        let result = st.auth.check(&self.keys, header, method, path);
        match result {
            Ok(ok) => {
                let was_active = st.last_seen.is_some_and(|t| t.elapsed() < ACTIVE);
                st.last_seen = Some(Instant::now());
                st.last_activity = Instant::now();
                if ok.first {
                    st.phone = Some(device_label(user_agent.unwrap_or("")).to_string());
                    log::info!("drop: phone connected");
                }
                drop(st);
                if ok.first || !was_active {
                    self.sink.changed(true);
                }
                Ok(ok)
            }
            Err(why) => {
                // Logged without anything from the request itself.
                log::debug!("drop: request refused ({why:?})");
                Err(match why {
                    Rejected::OtherDevice => ApiError::InUse,
                    _ => ApiError::Unauthorized,
                })
            }
        }
    }

    /// Open a sealed JSON request body.
    pub fn open_json<T: for<'de> Deserialize<'de>>(&self, req: &Authorized, body: &[u8]) -> Result<T, ApiError> {
        let plain = self
            .keys
            .open(&ad::request(&req.client, req.seq), body)
            .ok_or_else(|| {
                log::debug!("drop: request body failed to decrypt");
                ApiError::BadRequest
            })?;
        serde_json::from_slice(&plain).map_err(|_| ApiError::BadRequest)
    }

    /// Seal a JSON response to `req`.
    pub fn seal_json<T: Serialize>(&self, req: &Authorized, value: &T) -> Vec<u8> {
        let json = serde_json::to_vec(value).expect("reply types always serialize");
        self.keys.seal(&ad::response(&req.client, req.seq), &json)
    }

    /// What the page polls for: files on offer and the latest text from the PC.
    pub fn page_state(&self) -> StateReply {
        let st = self.state();
        StateReply {
            offers: st
                .offers
                .iter()
                .map(|o| PageOffer {
                    id: o.id.clone(),
                    name: o.name.clone(),
                    size: o.size,
                    chunk_size: OFFER_CHUNK,
                    chunks: offer_plan(o.size).chunks(),
                    mime: o.mime.to_string(),
                })
                .collect(),
            text: st.pc_text.as_ref().map(|t| PageText {
                id: t.id,
                text: t.text.clone(),
            }),
        }
    }

    /// Start (or resume) receiving a file. Idempotent: asking again for the same id reports
    /// which chunks already arrived, which is how the page resumes after Safari was paused.
    pub fn begin_upload(&self, id: &str, req: &UploadRequest) -> Result<UploadReply, ApiError> {
        if !crypto::valid_id(id) {
            return Err(ApiError::BadRequest);
        }
        {
            let st = self.state();
            if let Some(u) = st.uploads.iter().find(|u| u.id == id) {
                if u.plan.size != req.size || u.plan.chunk_size != req.chunk_size {
                    return Err(ApiError::BadRequest);
                }
                return Ok(UploadReply {
                    received: u.received.ranges(),
                    chunks: u.plan.chunks(),
                    saved_as: match &u.phase {
                        Phase::Saved { name, .. } => Some(name.clone()),
                        Phase::Sending => None,
                    },
                });
            }
            if st.uploads.len() >= MAX_UPLOADS {
                return Err(ApiError::TooBig);
            }
        }
        let plan = Plan::new(req.size, req.chunk_size).map_err(|e| match e {
            PlanError::TooBig => ApiError::TooBig,
            PlanError::BadChunkSize => ApiError::BadRequest,
        })?;
        fs::create_dir_all(&self.incoming).map_err(|e| io_err("create the incoming folder", e))?;
        fs::create_dir_all(&self.folder).map_err(|e| io_err("create the Drop folder", e))?;
        // Enough room for this file and whatever else is still arriving, with a margin.
        let pending: u64 = {
            let st = self.state();
            st.uploads
                .iter()
                .filter(|u| matches!(u.phase, Phase::Sending))
                .map(|u| u.plan.size - u.bytes)
                .sum()
        };
        // Both where it's written and where it ends up (they differ if Pictures is on another drive).
        for dir in [&self.incoming, &self.folder] {
            if let Some(free) = free_space(dir) {
                if free < req.size + pending + DISK_MARGIN {
                    return Err(ApiError::NoSpace);
                }
            }
        }
        let part = File::create(self.part_path(id)).map_err(|e| io_err("create a partial file", e))?;
        // Reserve the space up front, so a full disk shows now rather than at 90%.
        part.set_len(req.size).map_err(|e| io_err("size a partial file", e))?;
        let chunks = plan.chunks();
        let mut st = self.state();
        // Lost a race with the same request? Keep the first.
        if !st.uploads.iter().any(|u| u.id == id) {
            st.uploads.push(Upload {
                id: id.to_string(),
                name: names::sanitize(&req.name),
                plan,
                received: Received::new(chunks),
                bytes: 0,
                phase: Phase::Sending,
                at: now_ms(),
            });
        }
        drop(st);
        self.sink.changed(true);
        Ok(UploadReply {
            received: Vec::new(),
            chunks,
            saved_as: None,
        })
    }

    /// Decrypt one chunk and write it into the partial file.
    pub fn write_chunk(&self, id: &str, index: u32, sealed: &[u8]) -> Result<(), ApiError> {
        let plan = {
            let st = self.state();
            let u = st.uploads.iter().find(|u| u.id == id).ok_or(ApiError::NotFound)?;
            if let Phase::Saved { .. } = u.phase {
                return Ok(()); // a late retry of a chunk of a file already saved
            }
            if u.received.has(index) {
                return Ok(());
            }
            u.plan
        };
        let expected = plan.expected_len(index).ok_or(ApiError::BadRequest)?;
        let plain = self.keys.open(&ad::up(id, index), sealed).ok_or_else(|| {
            log::debug!("drop: chunk {index} failed to decrypt");
            ApiError::BadChunk
        })?;
        if plain.len() != expected {
            log::debug!("drop: chunk {index} has the wrong length");
            return Err(ApiError::BadChunk);
        }
        let mut f = OpenOptions::new()
            .write(true)
            .open(self.part_path(id))
            .map_err(|e| io_err("open a partial file", e))?;
        f.seek(SeekFrom::Start(plan.offset(index)))
            .and_then(|_| f.write_all(&plain))
            .map_err(|e| io_err("write a chunk", e))?;
        let mut st = self.state();
        let u = st.uploads.iter_mut().find(|u| u.id == id).ok_or(ApiError::NotFound)?;
        if u.received.mark(index) {
            u.bytes += expected as u64;
        }
        drop(st);
        self.sink.changed(false);
        Ok(())
    }

    /// Every chunk is in: check the size and move it into the Drop folder under a free name.
    pub fn finish_upload(&self, id: &str) -> Result<FinishReply, ApiError> {
        let _one_at_a_time = self.finishing.lock().unwrap_or_else(|p| p.into_inner());
        let (name, size) = {
            let st = self.state();
            let u = st.uploads.iter().find(|u| u.id == id).ok_or(ApiError::NotFound)?;
            if let Phase::Saved { name, .. } = &u.phase {
                return Ok(FinishReply { saved_as: name.clone() });
            }
            if !u.received.complete() {
                return Err(ApiError::Incomplete);
            }
            (u.name.clone(), u.plan.size)
        };
        let part = self.part_path(id);
        let len = fs::metadata(&part)
            .map_err(|e| io_err("check a partial file", e))?
            .len();
        if len != size {
            return Err(ApiError::Incomplete);
        }
        let (final_name, final_path) = self.reserve_name(&name)?;
        // Replaces the empty placeholder `reserve_name` created, which is what makes the name ours.
        // Across drives a rename can't work (ERROR_NOT_SAME_DEVICE), so copy instead.
        let moved = match fs::rename(&part, &final_path) {
            Err(e) if e.raw_os_error() == Some(17) => fs::copy(&part, &final_path).and_then(|_| fs::remove_file(&part)),
            other => other,
        };
        if let Err(e) = moved {
            let _ = fs::remove_file(&final_path);
            return Err(io_err("save a received file", e));
        }
        log::info!("drop: saved a file from the phone ({size} bytes)");
        let mut st = self.state();
        if let Some(u) = st.uploads.iter_mut().find(|u| u.id == id) {
            u.phase = Phase::Saved {
                path: final_path,
                name: final_name.clone(),
            };
        }
        drop(st);
        self.sink.changed(true);
        Ok(FinishReply { saved_as: final_name })
    }

    /// Claim a free name in the Drop folder by creating it empty (`create_new` fails if another
    /// file got there first, so nothing is ever overwritten).
    fn reserve_name(&self, name: &str) -> Result<(String, PathBuf), ApiError> {
        for _ in 0..20 {
            let candidate = names::unique(name, |n| self.folder.join(n).exists());
            let path = self.folder.join(&candidate);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok((candidate, path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err("name a received file", e)),
            }
        }
        Err(ApiError::Io)
    }

    /// The page gave up on a file (the user cancelled it).
    pub fn cancel_upload(&self, id: &str) -> Result<(), ApiError> {
        let mut st = self.state();
        let i = st.uploads.iter().position(|u| u.id == id).ok_or(ApiError::NotFound)?;
        if let Phase::Saved { .. } = st.uploads[i].phase {
            return Ok(());
        }
        st.uploads.remove(i);
        drop(st);
        let _ = fs::remove_file(self.part_path(id));
        self.sink.changed(true);
        Ok(())
    }

    /// One sealed chunk of a file on offer.
    pub fn read_offer_chunk(&self, id: &str, index: u32) -> Result<Vec<u8>, ApiError> {
        let (path, size) = {
            let st = self.state();
            let o = st.offers.iter().find(|o| o.id == id).ok_or(ApiError::NotFound)?;
            (o.path.clone(), o.size)
        };
        let plan = offer_plan(size);
        let len = plan.expected_len(index).ok_or(ApiError::BadRequest)?;
        let mut f = File::open(&path).map_err(|_| ApiError::Changed)?;
        if f.metadata().map(|m| m.len()).ok() != Some(size) {
            return Err(ApiError::Changed);
        }
        let mut buf = vec![0u8; len];
        f.seek(SeekFrom::Start(plan.offset(index)))
            .and_then(|_| f.read_exact(&mut buf))
            .map_err(|_| ApiError::Changed)?;
        let sealed = self.keys.seal(&ad::down(id, index), &buf);
        if index + 1 == plan.chunks() {
            let mut st = self.state();
            if let Some(o) = st.offers.iter_mut().find(|o| o.id == id) {
                o.downloads += 1;
            }
            drop(st);
            self.sink.changed(true);
        }
        Ok(sealed)
    }

    /// Text from the phone: kept for the panel and put on the clipboard.
    pub fn phone_text(&self, text: String) -> Result<(), ApiError> {
        if text.len() > MAX_TEXT {
            return Err(ApiError::TooBig);
        }
        if text.trim().is_empty() {
            return Err(ApiError::BadRequest);
        }
        let mut st = self.state();
        let id = st.next_text_id;
        st.next_text_id += 1;
        st.texts.insert(
            0,
            DropText {
                id,
                text: text.clone(),
                at: now_ms(),
            },
        );
        st.texts.truncate(MAX_TEXTS);
        drop(st);
        self.sink.text(&text);
        self.sink.changed(true);
        Ok(())
    }

    // --- Actions from the PC ---

    /// Offer files to the phone. Returns what couldn't be added.
    pub fn offer(&self, paths: &[PathBuf]) -> Vec<Skipped> {
        let mut skipped = Vec::new();
        let mut added = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let meta = match fs::metadata(path) {
                Ok(m) => m,
                Err(_) => {
                    skipped.push(Skipped {
                        name,
                        reason: SkipReason::Unreadable,
                    });
                    continue;
                }
            };
            if meta.is_dir() {
                skipped.push(Skipped {
                    name,
                    reason: SkipReason::Folder,
                });
                continue;
            }
            if meta.len() > MAX_OFFER {
                skipped.push(Skipped {
                    name,
                    reason: SkipReason::TooBig,
                });
                continue;
            }
            added.push(Offer {
                id: crypto::new_id(),
                mime: mime_for(&name),
                name,
                path: path.clone(),
                size: meta.len(),
                downloads: 0,
            });
        }
        let mut st = self.state();
        st.last_activity = Instant::now();
        for o in added {
            if st.offers.iter().any(|x| x.path == o.path) {
                continue;
            }
            if st.offers.len() >= MAX_OFFERS {
                skipped.push(Skipped {
                    name: o.name,
                    reason: SkipReason::TooMany,
                });
                continue;
            }
            st.offers.push(o);
        }
        drop(st);
        self.sink.changed(true);
        skipped
    }

    pub fn remove_offer(&self, id: &str) {
        let mut st = self.state();
        st.offers.retain(|o| o.id != id);
        st.last_activity = Instant::now();
        drop(st);
        self.sink.changed(true);
    }

    /// Text for the phone to copy (replaces the previous one; empty clears it).
    pub fn set_pc_text(&self, text: &str) -> Result<(), String> {
        if text.len() > MAX_TEXT {
            return Err("That text is too long to send.".into());
        }
        let mut st = self.state();
        st.last_activity = Instant::now();
        if text.trim().is_empty() {
            st.pc_text = None;
        } else {
            let id = st.next_text_id;
            st.next_text_id += 1;
            st.pc_text = Some(PageText {
                id,
                text: text.to_string(),
            });
        }
        drop(st);
        self.sink.changed(true);
        Ok(())
    }

    /// Whether `path` is a file this session saved (so "Show in folder" only opens those).
    pub fn saved_path(&self, path: &Path) -> bool {
        self.state()
            .uploads
            .iter()
            .any(|u| matches!(&u.phase, Phase::Saved { path: p, .. } if p == path))
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.state();
        Snapshot {
            phone: st.phone.clone(),
            phone_active: st.last_seen.is_some_and(|t| t.elapsed() < ACTIVE),
            incoming: st
                .uploads
                .iter()
                .rev()
                .map(|u| DropIncoming {
                    id: u.id.clone(),
                    name: match &u.phase {
                        Phase::Saved { name, .. } => name.clone(),
                        Phase::Sending => u.name.clone(),
                    },
                    size: u.plan.size,
                    received: u.bytes,
                    done: matches!(u.phase, Phase::Saved { .. }),
                    path: match &u.phase {
                        Phase::Saved { path, .. } => Some(path.to_string_lossy().into_owned()),
                        Phase::Sending => None,
                    },
                    at: u.at,
                })
                .collect(),
            outgoing: st
                .offers
                .iter()
                .map(|o| DropOffer {
                    id: o.id.clone(),
                    name: o.name.clone(),
                    size: o.size,
                    downloads: o.downloads,
                })
                .collect(),
            texts: st.texts.clone(),
            sent_text: st.pc_text.as_ref().map(|t| t.text.clone()),
        }
    }

    /// Files still arriving (the panel asks before closing over them).
    #[cfg(test)]
    pub fn sending(&self) -> usize {
        self.state()
            .uploads
            .iter()
            .filter(|u| matches!(u.phase, Phase::Sending))
            .count()
    }
}

fn offer_plan(size: u64) -> Plan {
    // Offers are capped at 1 GB, far inside what a plan allows.
    Plan {
        size,
        chunk_size: OFFER_CHUNK,
    }
}

/// Remove unfinished uploads (an abandoned or crashed session's `.part` files).
pub fn clean_incoming(dir: &Path) {
    if dir.exists() {
        if let Err(e) = fs::remove_dir_all(dir) {
            log::warn!("drop: couldn't remove unfinished files: {:?}", e.kind());
        }
    }
}

/// "iPhone", "iPad", "Android phone" or "phone", from the page's User-Agent.
pub fn device_label(user_agent: &str) -> &'static str {
    if user_agent.contains("iPhone") {
        "iPhone"
    } else if user_agent.contains("iPad") {
        "iPad"
    } else if user_agent.contains("Android") {
        "Android phone"
    } else {
        "phone"
    }
}

/// A content type for the phone's download, so Safari knows what it's saving.
pub fn mime_for(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

/// Free bytes on the volume holding `dir`.
#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is a NUL-terminated UTF-16 path; `free` outlives the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

#[cfg(not(windows))]
fn free_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_devices() {
        assert_eq!(
            device_label("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15"),
            "iPhone"
        );
        assert_eq!(device_label("Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X)"), "iPad");
        assert_eq!(
            device_label("Mozilla/5.0 (Linux; Android 14; Pixel 8)"),
            "Android phone"
        );
        assert_eq!(device_label(""), "phone");
    }

    #[test]
    fn content_types() {
        assert_eq!(mime_for("IMG_1.HEIC"), "image/heic");
        assert_eq!(mime_for("clip.MOV"), "video/quicktime");
        assert_eq!(mime_for("noext"), "application/octet-stream");
    }

    #[test]
    fn errors_map_to_statuses() {
        assert_eq!(ApiError::Closed.status(), 410);
        assert_eq!(ApiError::InUse.status(), 403);
        assert_eq!(ApiError::NoSpace.code(), "no-space");
    }
}
