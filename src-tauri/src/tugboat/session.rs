//! One Tugboat session: its keys, who's bound to it, files coming in and on offer, and texts. The
//! HTTP handlers (`server.rs`) and the Tauri glue (`mod.rs`) both work through this; nothing here
//! knows about Tauri, so the integration test can run a whole session over a socket.

use std::collections::HashMap;
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
/// A transfer counts as moving while its chunks keep coming at least this often.
const MOVING: Duration = Duration::from_secs(15);
/// Mark-of-the-Web: files from the phone are treated like downloads from the internet, so
/// SmartScreen checks programs and Office opens documents in Protected View.
const ZONE_IDENTIFIER: &str = "[ZoneTransfer]\r\nZoneId=3\r\n";

/// What the session tells the outside world.
pub trait Sink: Send + Sync {
    /// Something the Tugboat panel shows changed. `urgent` for state changes; progress isn't, and
    /// may be coalesced.
    fn changed(&self, urgent: bool);
    /// The phone sent text: it goes on the PC clipboard.
    fn text(&self, text: &str);
}

/// Why an API request failed. Mapped to an HTTP status and a short code (never content).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    /// The session is over (Tugboat closed, or a new code was made).
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
    /// The request's sequence number was already used or is too old: the page retries with a
    /// fresh one (never "expired", which ends the page).
    Stale,
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
            ApiError::Incomplete | ApiError::Changed | ApiError::Stale => 409,
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
            ApiError::Stale => "stale",
            ApiError::Changed => "changed",
            ApiError::Io => "io",
        }
    }
}

fn io_err(context: &str, e: std::io::Error) -> ApiError {
    // The error kind only: paths here name the user's files.
    log::warn!("tugboat: {context} failed: {:?}", e.kind());
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

// --- What the Tugboat panel shows (mirrored in src/types/protocol.ts) ---

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TugboatIncoming {
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
pub struct TugboatOffer {
    pub id: String,
    pub name: String,
    pub size: u64,
    /// Times the phone has fetched the whole file.
    pub downloads: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TugboatText {
    pub id: u32,
    pub text: String,
    pub at: i64,
}

/// The session part of `TugboatStatus`.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub phone: Option<String>,
    pub phone_active: bool,
    pub incoming: Vec<TugboatIncoming>,
    pub outgoing: Vec<TugboatOffer>,
    pub texts: Vec<TugboatText>,
    pub sent_text: Option<String>,
    /// The phone is downloading a file from the PC right now.
    pub sending: bool,
    /// Chunks of a file from the phone arrived just now. An upload the phone abandoned (Safari
    /// closed mid-file) stays unfinished in `incoming`, so "unfinished" isn't "moving".
    pub receiving: bool,
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
    /// When the last chunk from the phone was written.
    last_upload: Option<Instant>,
    /// Files the phone is part-way through downloading, and when it last fetched a chunk.
    downloading: HashMap<String, Instant>,
    uploads: Vec<Upload>,
    offers: Vec<Offer>,
    texts: Vec<TugboatText>,
    pc_text: Option<PageText>,
    next_text_id: u32,
}

pub struct Session {
    pub keys: Keys,
    /// `Pictures\Tugboat`.
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
                last_upload: None,
                downloading: HashMap::new(),
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

    /// The advertised address changed: the next phone to scan the new code becomes the phone (it
    /// arrives as a new browser origin, so even the same phone has a new client id). The panel goes
    /// back to waiting; replay protection carries on.
    pub fn unbind(&self) {
        let mut st = self.state();
        st.auth.unbind();
        st.phone = None;
        st.last_seen = None;
        drop(st);
        self.sink.changed(true);
    }

    /// Chunks of a file from the phone arrived just now.
    pub fn receiving(&self) -> bool {
        self.state().last_upload.is_some_and(|t| t.elapsed() < MOVING)
    }

    /// The phone is downloading a file from the PC right now.
    pub fn sending(&self) -> bool {
        self.state().downloading.values().any(|t| t.elapsed() < MOVING)
    }

    /// Files are moving either way right now (chunks arriving or being fetched).
    pub fn transferring(&self) -> bool {
        let st = self.state();
        st.last_upload.is_some_and(|t| t.elapsed() < MOVING) || st.downloading.values().any(|t| t.elapsed() < MOVING)
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
                    log::info!("tugboat: phone connected");
                }
                drop(st);
                if ok.first || !was_active {
                    self.sink.changed(true);
                }
                Ok(ok)
            }
            Err(why) => {
                // Logged without anything from the request itself.
                log::debug!("tugboat: request refused ({why:?})");
                Err(match why {
                    Rejected::OtherDevice => ApiError::InUse,
                    Rejected::Replayed => ApiError::Stale,
                    Rejected::Malformed | Rejected::BadMac => ApiError::Unauthorized,
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
                log::debug!("tugboat: request body failed to decrypt");
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
        fs::create_dir_all(&self.folder).map_err(|e| io_err("create the Tugboat folder", e))?;
        // Enough room for this file, with a margin. Files already arriving reserved theirs when
        // they started (set_len below), so the free figure already allows for them.
        // Both where it's written and where it ends up (they differ if Pictures is on another drive).
        for dir in [&self.incoming, &self.folder] {
            if let Some(free) = free_space(dir) {
                if free < req.size + DISK_MARGIN {
                    return Err(ApiError::NoSpace);
                }
            }
        }
        // Never truncate: a second begin racing the first mustn't zero chunks already written.
        let part = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.part_path(id))
            .map_err(|e| io_err("create a partial file", e))?;
        // Reserve the space up front, so a full disk shows now rather than at 90%.
        part.set_len(req.size).map_err(|e| io_err("size a partial file", e))?;
        // Tugboat closed while this was being set up: close() runs before the folder is cleaned,
        // so checking now can't miss it. Don't leave a pre-sized partial behind.
        if self.is_closed() {
            drop(part);
            let _ = fs::remove_file(self.part_path(id));
            return Err(ApiError::Closed);
        }
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
            log::debug!("tugboat: chunk {index} failed to decrypt");
            ApiError::BadChunk
        })?;
        if plain.len() != expected {
            log::debug!("tugboat: chunk {index} has the wrong length");
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
        st.last_upload = Some(Instant::now());
        drop(st);
        self.sink.changed(false);
        Ok(())
    }

    /// Every chunk is in: check the size and move it into the Tugboat folder under a free name.
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
            Err(e) if e.raw_os_error() == Some(17) => self.copy_across_drives(id, &part, &final_path),
            other => other,
        };
        if let Err(e) = moved {
            let _ = fs::remove_file(&final_path);
            return Err(io_err("save a received file", e));
        }
        mark_from_internet(&final_path);
        log::info!("tugboat: saved a file from the phone ({size} bytes)");
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

    /// Pictures is on another drive: copy into a hidden temp file next to the destination, then
    /// rename that over the reserved name, so the real name never holds a half-copied file (and a
    /// crash mid-copy leaves only a temp file, swept on the next start).
    fn copy_across_drives(&self, id: &str, part: &Path, final_path: &Path) -> std::io::Result<()> {
        let tmp = self.folder.join(format!("{TEMP_PREFIX}{id}{TEMP_SUFFIX}"));
        let copied = fs::copy(part, &tmp).and_then(|_| {
            set_hidden(&tmp, true);
            fs::rename(&tmp, final_path)
        });
        if let Err(e) = copied {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        set_hidden(final_path, false);
        // The file is saved; a partial that won't delete is cleaned up when Tugboat closes.
        if let Err(e) = fs::remove_file(part) {
            log::warn!(
                "tugboat: couldn't remove a partial file after copying it: {:?}",
                e.kind()
            );
        }
        Ok(())
    }

    /// Claim a free name in the Tugboat folder by creating it empty (`create_new` fails if another
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
        // Not while it's being finished: the saved file would vanish from the panel.
        let _not_mid_finish = self.finishing.lock().unwrap_or_else(|p| p.into_inner());
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
        let mut st = self.state();
        let urgent = if index + 1 == plan.chunks() {
            st.downloading.remove(id);
            if let Some(o) = st.offers.iter_mut().find(|o| o.id == id) {
                o.downloads += 1;
            }
            log::info!("tugboat: phone downloaded a file ({size} bytes)");
            true
        } else {
            // The first chunk (or the first after a pause) tells the panel a download is on.
            let was_moving = st.downloading.get(id).is_some_and(|t| t.elapsed() < MOVING);
            st.downloading.insert(id.to_string(), Instant::now());
            !was_moving
        };
        drop(st);
        if urgent {
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
            TugboatText {
                id,
                text: text.clone(),
                at: now_ms(),
            },
        );
        st.texts.truncate(MAX_TEXTS);
        drop(st);
        log::info!("tugboat: text from the phone ({} chars)", text.chars().count());
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
                .map(|u| TugboatIncoming {
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
                .map(|o| TugboatOffer {
                    id: o.id.clone(),
                    name: o.name.clone(),
                    size: o.size,
                    downloads: o.downloads,
                })
                .collect(),
            texts: st.texts.clone(),
            sent_text: st.pc_text.as_ref().map(|t| t.text.clone()),
            sending: st.downloading.values().any(|t| t.elapsed() < MOVING),
            receiving: st.last_upload.is_some_and(|t| t.elapsed() < MOVING),
        }
    }

    /// Files still arriving (the panel asks before closing over them).
    #[cfg(test)]
    pub fn uploads_in_progress(&self) -> usize {
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

/// Temp files from a cross-drive save are `~$<id>.tugboat.tmp` in the Tugboat folder.
const TEMP_PREFIX: &str = "~$";
const TEMP_SUFFIX: &str = ".tugboat.tmp";

/// Remove temp files a crash left mid-copy in the Tugboat folder (cross-drive saves).
pub fn sweep_temp(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX) {
            if let Err(e) = fs::remove_file(entry.path()) {
                log::warn!("tugboat: couldn't remove a leftover temp file: {:?}", e.kind());
            }
        }
    }
}

/// Write the Mark-of-the-Web next to a received file (see `ZONE_IDENTIFIER`). FAT and exFAT
/// drives can't hold it; that's logged, not fatal.
#[cfg(windows)]
fn mark_from_internet(path: &Path) {
    let stream = format!("{}:Zone.Identifier", path.display());
    if let Err(e) = fs::write(stream, ZONE_IDENTIFIER) {
        log::warn!("tugboat: couldn't mark a received file as downloaded: {:?}", e.kind());
    }
}

#[cfg(not(windows))]
fn mark_from_internet(_path: &Path) {
    let _ = ZONE_IDENTIFIER;
}

/// Set or clear the hidden attribute (the cross-drive temp file is hidden while it exists).
#[cfg(windows)]
fn set_hidden(path: &Path, hidden: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let attrs = if hidden {
        FILE_ATTRIBUTE_HIDDEN
    } else {
        FILE_ATTRIBUTE_NORMAL
    };
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    let _ = unsafe { SetFileAttributesW(PCWSTR(wide.as_ptr()), attrs) };
}

#[cfg(not(windows))]
fn set_hidden(_path: &Path, _hidden: bool) {}

/// Remove unfinished uploads (an abandoned or crashed session's `.part` files).
pub fn clean_incoming(dir: &Path) {
    if dir.exists() {
        if let Err(e) = fs::remove_dir_all(dir) {
            log::warn!("tugboat: couldn't remove unfinished files: {:?}", e.kind());
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

    #[test]
    fn sweeps_only_leftover_cross_drive_temp_files() {
        let dir = std::env::temp_dir().join(format!("tugboat-sweep-{}", crypto::new_id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("~$fileAAAAAAAAAAAAAAAAAA.tugboat.tmp"), b"half a copy").unwrap();
        fs::write(dir.join("~$report.docx"), b"office lock file").unwrap();
        fs::write(dir.join("IMG_0001.HEIC"), b"a photo").unwrap();
        sweep_temp(&dir);
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["IMG_0001.HEIC".to_string(), "~$report.docx".to_string()]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
