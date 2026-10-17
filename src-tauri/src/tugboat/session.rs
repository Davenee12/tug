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

use super::auth::{Auth, Authorized, GuestPolicy, Rejected};
use super::clip::{self, Ack, ClipEvent, ClipState, ClipStatus, Outbox, Payload, Prepared};
use super::crypto::{self, ad, Keys};
use super::names;
use super::pad::{self, Pad, PadEvent, PadReply, SeatNo, FIRST, SECOND};
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
    /// The game controller changed (steering, boost, the phone arriving or going). Only the game
    /// listens; nothing else acts on it.
    fn pad(&self, event: PadEvent);
    /// The PC's clipboard reached the phone, or no phone took it in time and it was dropped.
    fn clip(&self, event: ClipEvent);
}

/// Why an API request failed. Mapped to an HTTP status and a short code (never content).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    /// The session is over (Tugboat closed, or a new code was made).
    Closed,
    Unauthorized,
    /// Another device already has this session.
    InUse,
    /// A two-player game already has its two phones: a third is turned away.
    Full,
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
    /// A controller input, but tug's game isn't asking for one (closed, or never opened).
    NoGame,
    /// Controller inputs faster than any real phone sends them.
    Busy,
    /// Windows wouldn't let tug write where files are saved (Controlled folder access in Windows
    /// Security protects Pictures from apps it doesn't know), and the fallback was refused too.
    Blocked,
    Io,
}

impl ApiError {
    pub fn status(self) -> u16 {
        match self {
            ApiError::Closed => 410,
            ApiError::Unauthorized => 401,
            ApiError::InUse | ApiError::Blocked | ApiError::Full => 403,
            ApiError::NotFound => 404,
            ApiError::BadRequest | ApiError::BadChunk => 400,
            ApiError::TooBig => 413,
            ApiError::NoSpace => 507,
            ApiError::Incomplete | ApiError::Changed | ApiError::Stale | ApiError::NoGame => 409,
            ApiError::Busy => 429,
            ApiError::Io => 500,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            ApiError::Closed => "closed",
            ApiError::Unauthorized => "unauthorized",
            ApiError::InUse => "in-use",
            ApiError::Full => "full",
            ApiError::NotFound => "not-found",
            ApiError::BadRequest => "bad-request",
            ApiError::BadChunk => "bad-chunk",
            ApiError::TooBig => "too-big",
            ApiError::NoSpace => "no-space",
            ApiError::Incomplete => "incomplete",
            ApiError::Stale => "stale",
            ApiError::Changed => "changed",
            ApiError::NoGame => "no-game",
            ApiError::Busy => "busy",
            ApiError::Blocked => "blocked",
            ApiError::Io => "io",
        }
    }
}

/// ERROR_ACCESS_DENIED: what Controlled folder access (and a folder tug may not write to) gives.
const ACCESS_DENIED: i32 = 5;
/// ERROR_DISK_FULL.
const DISK_FULL: i32 = 112;

/// An I/O failure as the page hears it. Access denied is `Io` here, never `Blocked`: Windows also
/// says it when antivirus holds a file tug is renaming, or about tug's own incoming folder, and
/// "Windows Security blocked tug" would send the person to the wrong setting. Only the folder
/// check (`save_folder_checked`, through `Probe` and `choose_folder`) can say `Blocked`.
fn io_err(context: &str, e: std::io::Error) -> ApiError {
    // The error kind only: paths here name the user's files.
    log::warn!("tugboat: {context} failed: {:?}", e.kind());
    match e.raw_os_error() {
        Some(DISK_FULL) => ApiError::NoSpace,
        _ => ApiError::Io,
    }
}

/// Where received files are going, once the first upload checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderBlock {
    /// Windows refused the usual folder, so files go to the fallback (`Downloads\Tugboat`).
    Fallback,
    /// Windows refused both: nothing can be saved until tug is allowed.
    Blocked,
}

/// The outcome of trying to write in a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Ok,
    /// Access denied (Controlled folder access, or permissions).
    Denied,
    /// Some other failure (a missing drive, say): reported as it is, no fallback.
    Failed(ApiError),
}

impl Probe {
    fn from(result: std::io::Result<()>) -> Probe {
        match result {
            Ok(()) => Probe::Ok,
            Err(e) if e.raw_os_error() == Some(ACCESS_DENIED) => Probe::Denied,
            Err(e) => Probe::Failed(io_err("check the Tugboat folder", e)),
        }
    }
}

/// Which folder to save in, from how writing to the usual one (and, if that was refused, the
/// fallback) went. Pure, so the choice is tested without Windows Security.
pub fn choose_folder(preferred: Probe, fallback: Option<Probe>) -> Result<Option<FolderBlock>, ApiError> {
    match (preferred, fallback) {
        (Probe::Ok, _) => Ok(None),
        (Probe::Failed(e), _) => Err(e),
        (Probe::Denied, Some(Probe::Ok)) => Ok(Some(FolderBlock::Fallback)),
        (Probe::Denied, _) => Err(ApiError::Blocked),
    }
}

/// Make sure tug can create a file in `dir`: create the folder, then a uniquely named probe file
/// (named like the cross-drive temp files, so `sweep_temp` removes one ever left behind), and
/// delete it.
pub fn probe_folder(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(format!("{TEMP_PREFIX}probe-{}{TEMP_SUFFIX}", crypto::new_id()));
    OpenOptions::new().write(true).create_new(true).open(&probe)?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

type Prober = dyn Fn(&Path) -> std::io::Result<()> + Send + Sync;

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

/// The phone has the PC's clipboard `id` (`POST /api/clip`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipAckRequest {
    pub id: u32,
}

/// The PC's clipboard, as the page gets it: text, or a picture to fetch like a file on offer.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageClip {
    pub id: u32,
    pub text: Option<String>,
    pub image: Option<PageOffer>,
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
    /// tug's game wants this phone as its controller: the page offers the controller.
    pub game: bool,
    /// The PC's clipboard, sent with "Send clipboard to iPhone", until the phone says it has it.
    pub clip: Option<PageClip>,
    /// This phone is the second controller of a two-player game: it only ever steers (no files, no
    /// text), and waits for the next race when there isn't one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guest: bool,
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
    /// The last clipboard sent to the phone (its kind and state, never its contents).
    pub clip: Option<ClipStatus>,
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

/// Where an offered file's bytes come from.
enum Source {
    /// A file on the PC (dragged onto tug or picked), read chunk by chunk.
    File(PathBuf),
    /// A picture from the clipboard, held in memory only while it's on its way (never on disk).
    Memory(Arc<Vec<u8>>),
}

struct Offer {
    id: String,
    name: String,
    source: Source,
    size: u64,
    mime: &'static str,
    downloads: u32,
    /// Part of a clipboard on its way to the phone: fetched through the clipboard card, not listed
    /// with the files (on the page or in the panel), and withdrawn once delivered or dropped.
    clip: bool,
}

impl Offer {
    fn is_file(&self, path: &Path) -> bool {
        matches!(&self.source, Source::File(p) if p == path)
    }
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
    /// The game controller channel, while tug's game asks for one.
    pad: Option<Pad>,
    /// Where files are saved, once the first upload checked tug may write there.
    save_to: Option<PathBuf>,
    /// Windows refused the usual folder (files go to the fallback), or both.
    block: Option<FolderBlock>,
    /// Refused requests, so a flood is logged as a count (`tally.rs`).
    refused: super::tally::Tally,
    /// The PC's clipboard on its way to the phone (see clip.rs).
    clip: Outbox,
    next_clip_id: u32,
    /// When the second phone of a two-player game was last heard from.
    guest_seen: Option<Instant>,
}

pub struct Session {
    pub keys: Keys,
    /// `Pictures\Tugboat`: where received files go, unless Windows won't let tug write there.
    pub folder: PathBuf,
    /// `Downloads\Tugboat`: used when Windows refuses `folder` (Controlled folder access).
    fallback: Option<PathBuf>,
    /// Checks tug can write in a folder (`probe_folder`; replaced in tests).
    prober: Box<Prober>,
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
            fallback: None,
            prober: Box::new(probe_folder),
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
                pad: None,
                save_to: None,
                block: None,
                refused: super::tally::Tally::default(),
                clip: Outbox::new(),
                next_clip_id: 1,
                guest_seen: None,
            }),
            closed: AtomicBool::new(false),
            sink,
            finishing: Mutex::new(()),
        }
    }

    /// Where to save when Windows refuses the usual folder.
    pub fn with_fallback(mut self, fallback: Option<PathBuf>) -> Session {
        self.fallback = fallback;
        self
    }

    /// Replace the write check (tests stand in for Windows Security with it).
    #[cfg(test)]
    pub fn with_prober(mut self, prober: impl Fn(&Path) -> std::io::Result<()> + Send + Sync + 'static) -> Session {
        self.prober = Box::new(prober);
        self
    }

    /// The folder received files are saved in (the usual one until a check said otherwise).
    pub fn save_folder(&self) -> PathBuf {
        self.state().save_to.clone().unwrap_or_else(|| self.folder.clone())
    }

    /// Whether Windows refused the usual folder (and the fallback).
    pub fn folder_block(&self) -> Option<FolderBlock> {
        self.state().block
    }

    /// The folder to save this upload in: decided by the first upload (can tug write in the usual
    /// folder? if Windows refuses, in the fallback?) and kept for the session. Refused both ways,
    /// it's checked again next time, so allowing tug in Windows Security works without reopening
    /// Tugboat.
    fn save_folder_checked(&self) -> Result<PathBuf, ApiError> {
        if let Some(dir) = self.state().save_to.clone() {
            return Ok(dir);
        }
        let preferred = Probe::from((self.prober)(&self.folder));
        let fallback = match (preferred, &self.fallback) {
            (Probe::Denied, Some(dir)) => Some(Probe::from((self.prober)(dir))),
            _ => None,
        };
        let chosen = choose_folder(preferred, fallback);
        let mut st = self.state();
        let before = st.block;
        let result = match chosen {
            Ok(None) => {
                st.block = None;
                st.save_to = Some(self.folder.clone());
                Ok(self.folder.clone())
            }
            Ok(Some(block)) => {
                let dir = self.fallback.clone().expect("a fallback was checked");
                st.block = Some(block);
                st.save_to = Some(dir.clone());
                Ok(dir)
            }
            Err(ApiError::Blocked) => {
                st.block = Some(FolderBlock::Blocked);
                Err(ApiError::Blocked)
            }
            Err(e) => Err(e),
        };
        let after = st.block;
        drop(st);
        if after != before {
            match after {
                Some(FolderBlock::Fallback) => {
                    log::warn!("tugboat: Windows blocked saving to Pictures; saving to Downloads instead")
                }
                Some(FolderBlock::Blocked) => log::warn!("tugboat: Windows blocked saving received files"),
                None => {}
            }
            self.sink.changed(true);
        }
        result
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
        let held = self.state().refused.flush();
        if held > 0 {
            log::debug!("tugboat: {held} more requests refused before closing");
        }
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
        st.guest_seen = None;
        if let Some(p) = st.pad.as_mut() {
            p.rebound();
        }
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

    /// Check a request's `Authorization` header. The first good one binds the session. While a
    /// two-player game asks for two controllers, one second phone may join too, for steering only:
    /// its page's check-in and its inputs, nothing else.
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
        let controller = is_controller_request(method, path);
        let mut st = self.state();
        let duo = st.pad.as_ref().is_some_and(|p| p.players() == 2);
        let policy = GuestPolicy {
            may_join: duo && controller,
            // Exactly two phones at a time; one that has gone quiet makes room for the next.
            may_replace: st.guest_seen.is_none_or(|t| t.elapsed() >= ACTIVE),
        };
        let result = st.auth.check_with(&self.keys, header, method, path, policy);
        match result {
            Ok(ok) if ok.guest => {
                if !controller {
                    // A second phone only ever steers: never files, text or the panel.
                    return Err(ApiError::InUse);
                }
                st.guest_seen = Some(Instant::now());
                st.last_activity = Instant::now();
                if ok.first {
                    log::info!("tugboat: second controller joined");
                    if let Some(p) = st.pad.as_mut() {
                        p.rebound_seat(SECOND);
                    }
                }
                Ok(ok)
            }
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
                // Logged without anything from the request itself, and at most once a minute
                // with a count: any device on the network can send refused requests endlessly.
                if let Some(held) = st.refused.note(Instant::now()) {
                    log::debug!("tugboat: request refused ({why:?}){}", super::tally::more(held));
                }
                Err(match why {
                    // Turned away from a two-player game's controller: it has its two phones.
                    Rejected::OtherDevice if policy.may_join => ApiError::Full,
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

    /// What the page polls for: files on offer, the latest text from the PC, and the PC's clipboard
    /// while it's on its way. A two-player game's second phone only hears whether there's a race to
    /// steer in: it returns before the clipboard is looked at (`for_page` counts as serving it), and
    /// never gets the clipboard, files or text.
    pub fn page_state(&self, req: &Authorized) -> StateReply {
        let mut st = self.state();
        if req.guest {
            return StateReply {
                offers: Vec::new(),
                text: None,
                game: st.pad.as_ref().is_some_and(|p| p.open_for(SECOND)),
                clip: None,
                guest: true,
            };
        }
        let clip = match st.clip.for_page(Instant::now()) {
            Some((id, Payload::Text(t))) => Some((id, Some(t.clone()), None)),
            Some((id, Payload::Image { offer })) => Some((id, None, Some(offer.clone()))),
            None => None,
        };
        let clip = clip.map(|(id, text, offer)| PageClip {
            id,
            text,
            image: offer.and_then(|o| st.offers.iter().find(|x| x.id == o).map(page_offer)),
        });
        StateReply {
            offers: st.offers.iter().filter(|o| !o.clip).map(page_offer).collect(),
            text: st.pc_text.as_ref().map(|t| PageText {
                id: t.id,
                text: t.text.clone(),
            }),
            game: st.pad.is_some(),
            clip,
            guest: false,
        }
    }

    // --- The shared clipboard (see clip.rs) ---

    /// Send the PC's clipboard to the phone (one press, one clipboard). A picture becomes an offer
    /// held in memory, fetched like any file; a newer press replaces one the phone hasn't taken.
    /// Returns the clipboard's id.
    pub fn send_clip(&self, prepared: Prepared) -> u32 {
        let (payload, offer, kind, size) = match prepared {
            Prepared::Text(t) => {
                let chars = t.chars().count();
                (Payload::Text(t), None, "text", chars)
            }
            Prepared::Image(png) => {
                let size = png.len();
                let offer = Offer {
                    id: crypto::new_id(),
                    name: clip::IMAGE_NAME.to_string(),
                    size: size as u64,
                    source: Source::Memory(Arc::new(png)),
                    mime: "image/png",
                    downloads: 0,
                    clip: true,
                };
                (
                    Payload::Image {
                        offer: offer.id.clone(),
                    },
                    Some(offer),
                    "picture",
                    size,
                )
            }
        };
        let mut st = self.state();
        st.last_activity = Instant::now();
        st.offers.extend(offer);
        let id = st.next_clip_id;
        st.next_clip_id += 1;
        if let Some(old) = st.clip.send(id, payload, Instant::now(), now_ms()) {
            withdraw(&mut st, &old);
        }
        drop(st);
        // Lengths only, never the contents.
        let unit = if kind == "text" { "chars" } else { "bytes" };
        log::info!("tugboat: clipboard sent to the phone ({kind}, {size} {unit})");
        self.sink.changed(true);
        id
    }

    /// The phone says it has clipboard `id`. A picture counts only once the phone really fetched
    /// all of it. The PC lets go of the contents straight away.
    pub fn clip_ack(&self, id: u32) -> Result<(), ApiError> {
        let mut st = self.state();
        if let Some((waiting, offer)) = st.clip.waiting_offer() {
            if waiting == id && st.offers.iter().any(|o| o.id == offer && o.downloads == 0) {
                return Err(ApiError::Incomplete);
            }
        }
        match st.clip.ack(id, Instant::now()) {
            Ack::Delivered { kind, freed } => {
                if let Some(offer) = freed {
                    st.offers.retain(|o| o.id != offer);
                }
                drop(st);
                log::info!("tugboat: the phone has the clipboard");
                self.sink.clip(ClipEvent {
                    kind,
                    state: ClipState::Delivered,
                });
                self.sink.changed(true);
                Ok(())
            }
            Ack::Already => Ok(()),
            Ack::Unknown => Err(ApiError::NotFound),
        }
    }

    /// Drop a clipboard no phone took within two minutes (called every couple of seconds).
    pub fn clip_tick(&self) {
        let mut st = self.state();
        let Some(dropped) = st.clip.tick(Instant::now()) else {
            return;
        };
        withdraw(&mut st, &dropped);
        drop(st);
        log::info!("tugboat: no phone took the clipboard in time, dropped it");
        self.sink.clip(ClipEvent {
            kind: dropped.kind(),
            state: ClipState::Expired,
        });
        self.sink.changed(true);
    }

    // --- The game controller (see pad.rs) ---

    /// tug's game wants a controller (`players` 1), or two (a two-player game, 2): the phone pages
    /// offer it at their next check-in. Asking again changes how many; going back to one lets the
    /// second phone go (the game hears it's gone). Returns whether the channel was newly opened
    /// (the caller starts watching it).
    pub fn pad_open(&self, players: u8) -> bool {
        let mut st = self.state();
        st.last_activity = Instant::now();
        if let Some(p) = st.pad.as_mut() {
            let was = p.players();
            let gone = p.set_players(players, Instant::now());
            let now = p.players();
            drop(st);
            if was != now {
                log::info!("tugboat: game controller for {now} player(s)");
            }
            if let Some(e) = gone {
                self.sink.pad(e);
            }
            return false;
        }
        st.pad = Some(Pad::new(Instant::now(), players));
        drop(st);
        log::info!("tugboat: game controller on ({} player(s))", players.clamp(1, 2));
        true
    }

    /// The game closed: inputs are refused from now on, and the game hears the phones have gone.
    pub fn pad_close(&self) {
        let was = self.state().pad.take();
        if let Some(p) = was {
            log::info!("tugboat: game controller off");
            for seat in [FIRST, SECOND] {
                // The latency probe, once per session (never per input).
                if let Some(s) = p.latency(seat).summary() {
                    log::debug!("tugboat: controller {seat} touch-to-PC latency: {s}");
                }
                if p.connected(seat) {
                    self.sink.pad(PadEvent::gone(seat));
                }
            }
        }
    }

    /// Check the channel is open for this phone and it isn't flooding it, before its body is read.
    pub fn pad_admit(&self, req: &Authorized) -> Result<(), ApiError> {
        let seat = seat_of(req);
        let mut st = self.state();
        let pad = st.pad.as_mut().filter(|p| p.open_for(seat)).ok_or(ApiError::NoGame)?;
        if pad.admit(seat, Instant::now()) {
            Ok(())
        } else {
            Err(ApiError::Busy)
        }
    }

    /// One controller input from the bound phone (or a two-player game's second phone): open it,
    /// check it, and hand the game the new state. It can't do anything else: no files, no
    /// clipboard, no panel.
    pub fn pad_input(&self, req: &Authorized, body: &[u8]) -> Result<PadReply, ApiError> {
        if body.len() > pad::MAX_BODY {
            return Err(ApiError::TooBig);
        }
        let plain = self
            .keys
            .open(&ad::request(&req.client, req.seq), body)
            .ok_or(ApiError::BadRequest)?;
        let decoded = pad::decode(&plain).map_err(|e| match e {
            pad::PadError::TooBig => ApiError::TooBig,
            pad::PadError::Malformed | pad::PadError::OutOfRange => ApiError::BadRequest,
        })?;
        let seat = seat_of(req);
        let mut st = self.state();
        let pad = st.pad.as_mut().filter(|p| p.open_for(seat)).ok_or(ApiError::NoGame)?;
        let event = pad.input(seat, req.seq, decoded, Instant::now());
        let reply = pad.reply(seat);
        drop(st);
        if let Some(e) = event {
            self.sink.pad(e);
        }
        Ok(reply)
    }

    /// Look for a phone that went quiet. `false` once the channel is closed (stop watching).
    pub fn pad_tick(&self) -> bool {
        let mut st = self.state();
        let Some(pad) = st.pad.as_mut() else {
            return false;
        };
        let events = pad.tick(Instant::now());
        drop(st);
        for e in events {
            log::info!("tugboat: game controller {} went quiet", e.seat);
            self.sink.pad(e);
        }
        true
    }

    /// What the next replies tell a phone (paused, its hits so far for a buzz, its player).
    pub fn pad_feedback(&self, seat: SeatNo, reply: PadReply) {
        if let Some(p) = self.state().pad.as_mut() {
            p.set_reply(seat, reply);
        }
    }

    /// The latency probe's summary so far for the first phone (for the manual test harness).
    #[cfg(test)]
    pub fn pad_latency(&self) -> Option<String> {
        self.state().pad.as_ref().and_then(|p| p.latency(FIRST).summary())
    }

    /// The controllers as the game should show them now (seat 2 only in a two-player game).
    pub fn pad_events(&self) -> Vec<PadEvent> {
        let st = self.state();
        let Some(p) = st.pad.as_ref() else {
            return Vec::new();
        };
        [FIRST, SECOND]
            .into_iter()
            .filter(|s| p.open_for(*s))
            .map(|s| p.event(s))
            .collect()
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
        // Before a byte is sent: can tug save there at all? (Controlled folder access refuses
        // Pictures to apps it doesn't know, and would otherwise fail only once the file is in.)
        let folder = self.save_folder_checked()?;
        // Enough room for this file, with a margin. Files already arriving reserved theirs when
        // they started (set_len below), so the free figure already allows for them.
        // Both where it's written and where it ends up (they differ if Pictures is on another drive).
        for dir in [&self.incoming, &folder] {
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
        let dir = final_path.parent().unwrap_or(&self.folder);
        let tmp = dir.join(format!("{TEMP_PREFIX}{id}{TEMP_SUFFIX}"));
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
        let folder = self.save_folder();
        for _ in 0..20 {
            let candidate = names::unique(name, |n| folder.join(n).exists());
            let path = folder.join(&candidate);
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
        enum Src {
            File(PathBuf),
            Memory(Arc<Vec<u8>>),
        }
        let (from, size) = {
            let st = self.state();
            let o = st.offers.iter().find(|o| o.id == id).ok_or(ApiError::NotFound)?;
            let from = match &o.source {
                Source::File(p) => Src::File(p.clone()),
                Source::Memory(b) => Src::Memory(b.clone()),
            };
            (from, o.size)
        };
        let plan = offer_plan(size);
        let len = plan.expected_len(index).ok_or(ApiError::BadRequest)?;
        let start = plan.offset(index);
        let buf = match from {
            Src::Memory(bytes) => usize::try_from(start)
                .ok()
                .and_then(|s| bytes.get(s..s + len))
                .ok_or(ApiError::BadRequest)?
                .to_vec(),
            Src::File(path) => {
                let mut f = File::open(&path).map_err(|_| ApiError::Changed)?;
                if f.metadata().map(|m| m.len()).ok() != Some(size) {
                    return Err(ApiError::Changed);
                }
                let mut buf = vec![0u8; len];
                f.seek(SeekFrom::Start(start))
                    .and_then(|_| f.read_exact(&mut buf))
                    .map_err(|_| ApiError::Changed)?;
                buf
            }
        };
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
            added.push((
                path,
                Offer {
                    id: crypto::new_id(),
                    mime: mime_for(&name),
                    name,
                    source: Source::File(path.clone()),
                    size: meta.len(),
                    downloads: 0,
                    clip: false,
                },
            ));
        }
        let mut st = self.state();
        st.last_activity = Instant::now();
        for (path, o) in added {
            if st.offers.iter().any(|x| x.is_file(path)) {
                continue;
            }
            if st.offers.iter().filter(|x| !x.clip).count() >= MAX_OFFERS {
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
                .filter(|o| !o.clip)
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
            clip: st.clip.status(),
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

fn page_offer(o: &Offer) -> PageOffer {
    PageOffer {
        id: o.id.clone(),
        name: o.name.clone(),
        size: o.size,
        chunk_size: OFFER_CHUNK,
        chunks: offer_plan(o.size).chunks(),
        mime: o.mime.to_string(),
    }
}

/// A clipboard is done with (delivered, dropped or replaced): withdraw its picture, if it had one,
/// so its bytes go too.
fn withdraw(st: &mut State, payload: &Payload) {
    if let Some(offer) = payload.offer() {
        st.offers.retain(|o| o.id != offer);
        st.downloading.remove(offer);
    }
}

/// The two requests a game controller makes: the page's check-in and an input. A two-player
/// game's second phone may make these and nothing else.
fn is_controller_request(method: &str, path: &str) -> bool {
    matches!((method, path), ("GET", "/api/state") | ("POST", "/api/pad"))
}

/// Which seat a controller request is for: the session's own phone is seat 1, the guest seat 2.
fn seat_of(req: &Authorized) -> SeatNo {
    if req.guest {
        SECOND
    } else {
        FIRST
    }
}

fn offer_plan(size: u64) -> Plan {
    // Offers are capped at 1 GB, far inside what a plan allows.
    Plan {
        size,
        chunk_size: OFFER_CHUNK,
    }
}

/// Temp files from a cross-drive save are `~$<id>.tugboat.tmp` in the Tugboat folder (and the
/// write check's probe files `~$probe-<id>.tugboat.tmp`). `names::sanitize` never gives a file
/// from the phone a name of this shape, so the sweep can't delete one.
pub const TEMP_PREFIX: &str = "~$";
pub const TEMP_SUFFIX: &str = ".tugboat.tmp";

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
    fn only_the_folder_check_says_windows_security_blocked_it() {
        // Access denied while saving (antivirus holding the file mid-rename) or in tug's incoming
        // folder is a plain I/O failure, not "Windows Security blocked tug".
        let denied = || std::io::Error::from_raw_os_error(ACCESS_DENIED);
        assert_eq!(io_err("save a received file", denied()), ApiError::Io);
        assert_eq!(io_err("create the incoming folder", denied()), ApiError::Io);
        // The folder check is where it means Controlled folder access.
        assert_eq!(Probe::from(Err(denied())), Probe::Denied);
        assert_eq!(choose_folder(Probe::from(Err(denied())), None), Err(ApiError::Blocked));
        assert_eq!(ApiError::Blocked.code(), "blocked");
        let full = std::io::Error::from_raw_os_error(DISK_FULL);
        assert_eq!(io_err("write a chunk", full), ApiError::NoSpace);
        let other = std::io::Error::from_raw_os_error(32); // sharing violation
        assert_eq!(io_err("write a chunk", other), ApiError::Io);
    }

    #[test]
    fn chooses_the_fallback_only_when_windows_refuses() {
        assert_eq!(choose_folder(Probe::Ok, None), Ok(None));
        assert_eq!(
            choose_folder(Probe::Denied, Some(Probe::Ok)),
            Ok(Some(FolderBlock::Fallback))
        );
        assert_eq!(
            choose_folder(Probe::Denied, Some(Probe::Denied)),
            Err(ApiError::Blocked)
        );
        assert_eq!(choose_folder(Probe::Denied, None), Err(ApiError::Blocked));
        // Another failure isn't papered over by quietly saving somewhere else.
        assert_eq!(
            choose_folder(Probe::Failed(ApiError::NoSpace), None),
            Err(ApiError::NoSpace)
        );
    }

    struct Quiet;
    impl Sink for Quiet {
        fn changed(&self, _urgent: bool) {}
        fn text(&self, _text: &str) {}
        fn pad(&self, _event: PadEvent) {}
        fn clip(&self, _event: ClipEvent) {}
    }

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("tugboat-{tag}-{}", crypto::new_id()))
    }

    fn upload(size: u64) -> UploadRequest {
        UploadRequest {
            name: "IMG_0001.HEIC".into(),
            size,
            chunk_size: 64 * 1024,
        }
    }

    /// Windows Security stand-in: refuses `blocked` folders with ERROR_ACCESS_DENIED, else the
    /// real check.
    fn refusing(blocked: Vec<PathBuf>) -> impl Fn(&Path) -> std::io::Result<()> + Send + Sync {
        move |dir: &Path| {
            if blocked.iter().any(|b| b == dir) {
                Err(std::io::Error::from_raw_os_error(ACCESS_DENIED))
            } else {
                probe_folder(dir)
            }
        }
    }

    #[test]
    fn a_blocked_pictures_folder_falls_back_to_downloads() {
        let (pictures, downloads, incoming) = (temp("pictures"), temp("downloads"), temp("incoming"));
        let s = Session::new(&[7u8; 32], pictures.clone(), incoming.clone(), Arc::new(Quiet))
            .with_fallback(Some(downloads.clone()))
            .with_prober(refusing(vec![pictures.clone()]));
        assert_eq!(s.folder_block(), None);
        s.begin_upload("fileAAAAAAAAAAAAAAAAAA", &upload(10)).unwrap();
        assert_eq!(s.folder_block(), Some(FolderBlock::Fallback));
        assert_eq!(s.save_folder(), downloads);
        // The probe left nothing behind.
        assert_eq!(fs::read_dir(&downloads).unwrap().count(), 0);
        for dir in [&downloads, &incoming] {
            let _ = fs::remove_dir_all(dir);
        }
        assert!(!pictures.exists());
    }

    #[test]
    fn blocked_both_ways_says_so_before_anything_is_sent_and_checks_again() {
        let (pictures, downloads, incoming) = (temp("pictures"), temp("downloads"), temp("incoming"));
        let allowed = Arc::new(AtomicBool::new(false));
        let gate = allowed.clone();
        let refuse = refusing(vec![pictures.clone(), downloads.clone()]);
        let s = Session::new(&[7u8; 32], pictures.clone(), incoming.clone(), Arc::new(Quiet))
            .with_fallback(Some(downloads.clone()))
            .with_prober(move |dir: &Path| {
                if gate.load(Ordering::SeqCst) {
                    probe_folder(dir)
                } else {
                    refuse(dir)
                }
            });
        let id = "fileBBBBBBBBBBBBBBBBBB";
        assert_eq!(s.begin_upload(id, &upload(10)), Err(ApiError::Blocked));
        assert_eq!(s.folder_block(), Some(FolderBlock::Blocked));
        assert_eq!(s.uploads_in_progress(), 0, "nothing was set up for the file");
        // The person allows tug in Windows Security and tries again: no need to reopen Tugboat.
        allowed.store(true, Ordering::SeqCst);
        s.begin_upload(id, &upload(10)).unwrap();
        assert_eq!(s.folder_block(), None);
        assert_eq!(s.save_folder(), pictures);
        for dir in [&pictures, &incoming] {
            let _ = fs::remove_dir_all(dir);
        }
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
