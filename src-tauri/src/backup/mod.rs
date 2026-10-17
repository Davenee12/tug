//! Bring in your full message history from a local iPhone backup.
//!
//! Over Bluetooth iOS shares only the ~10 newest incoming texts: never the ones sent from the
//! phone, and no photos. A backup the Apple Devices app (or iTunes) made on this PC has all of
//! them, so tug can read it, once, as a snapshot. Everything stays on this PC: nothing here
//! touches the network.
//!
//! The pieces, each pure and tested with made-up data, then the thin part that runs them:
//! - `discover`: the backups on this PC, from their plists.
//! - `keybag`, `decrypt`, `manifest`: an encrypted backup's keys, its files, and where they are.
//! - `smsdb`, `typedstream`: Messages' own database, and the words in `attributedBody`.
//! - `import`: what's new, merged with tug's Bluetooth texts, written in resumable batches.
//! - `files`: stored photos and videos, previews (`wic` turns HEIC into JPEG on Windows), and the
//!   private scratch folder for decrypted files, swept at every start.
//! - `BackupService` (here): one job at a time on its own thread, with progress, Cancel and a
//!   status the window follows (`backup-import` events).
//!
//! What's logged: counts and timings only. Never a password, a word of a message, a name, a
//! number, a file name or a path.

pub mod clock;
pub mod decrypt;
pub mod discover;
pub mod files;
pub mod import;
pub mod keybag;
pub mod manifest;
pub mod smsdb;
pub mod typedstream;
#[cfg(windows)]
mod wic;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use zeroize::Zeroizing;

use crate::store::Store;
use discover::{BackupInfo, BackupProblem};
use files::TempDir;
use import::{FileSink, ImportError, Options, Scan, SinkError, Stage, Summary};
use keybag::{ClassKeys, Keybag};
use manifest::FileEntry;
use smsdb::SmsData;

/// The window's event: the import's status, whenever it changes.
pub const EVENT: &str = "backup-import";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    /// Turning the password into the backup's keys (deliberately slow).
    Unlocking,
    /// Reading Messages' database out of the backup.
    Reading,
    /// Read: the scan says what an import would add.
    Ready,
    Importing,
    Done,
    Cancelled,
    Failed,
}

/// Why a backup couldn't be read or imported; the window says it in its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    NeedsPassword,
    WrongPassword,
    TooOld,
    Incomplete,
    Unreadable,
    /// The backup has no Messages database (Messages in iCloud keeps them off the backup).
    NoMessages,
    /// Couldn't write to this PC (the disk is probably full).
    CantWrite,
    Failed,
}

impl From<BackupProblem> for Problem {
    fn from(p: BackupProblem) -> Self {
        match p {
            BackupProblem::TooOld => Problem::TooOld,
            BackupProblem::Incomplete => Problem::Incomplete,
            BackupProblem::Unreadable => Problem::Unreadable,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    /// Counts every change, so a late event can't undo a newer one in the window.
    pub rev: u64,
    pub phase: Phase,
    /// The backup being read or imported.
    pub backup: Option<BackupInfo>,
    /// 0–1 through the current step, when it can be measured.
    pub progress: Option<f64>,
    /// Importing: texts first, then photos.
    pub stage: Option<ImportStage>,
    pub done: u64,
    pub total: u64,
    pub scan: Option<Scan>,
    pub summary: Option<Summary>,
    pub problem: Option<Problem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportStage {
    Texts,
    Photos,
}

impl BackupStatus {
    fn idle() -> Self {
        BackupStatus {
            rev: 0,
            phase: Phase::Idle,
            backup: None,
            progress: None,
            stage: None,
            done: 0,
            total: 0,
            scan: None,
            summary: None,
            problem: None,
        }
    }
}

/// An unlocked, read backup, waiting for "Bring it in". Its keys are wiped and its scratch folder
/// deleted when it's dropped (closed, imported, cancelled or failed).
struct Session {
    root: PathBuf,
    info: BackupInfo,
    temp: TempDir,
    keys: Option<ClassKeys>,
    data: SmsData,
    attachments: HashMap<String, FileEntry>,
}

struct State {
    status: BackupStatus,
    /// Backups the window may name, by id (listed or picked): the window never names a folder.
    known: HashMap<String, (PathBuf, BackupInfo)>,
    session: Option<Arc<Session>>,
    /// The running job's cancel flag.
    job: Option<Arc<AtomicBool>>,
    /// The last whole percent sent, so progress doesn't flood the window.
    last_sent: Option<(Phase, i64)>,
}

struct Inner {
    app: Option<AppHandle>,
    store: Arc<Store>,
    local_dir: PathBuf,
    state: Mutex<State>,
}

/// The import, for the window's commands.
#[derive(Clone)]
pub struct BackupService {
    inner: Arc<Inner>,
}

impl BackupService {
    pub fn new(app: AppHandle, store: Arc<Store>, local_dir: PathBuf) -> Self {
        Self::build(Some(app), store, local_dir)
    }

    fn build(app: Option<AppHandle>, store: Arc<Store>, local_dir: PathBuf) -> Self {
        BackupService {
            inner: Arc::new(Inner {
                app,
                store,
                local_dir,
                state: Mutex::new(State {
                    status: BackupStatus::idle(),
                    known: HashMap::new(),
                    session: None,
                    job: None,
                    last_sent: None,
                }),
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn local_dir(&self) -> &Path {
        &self.inner.local_dir
    }

    pub fn status(&self) -> BackupStatus {
        self.state().status.clone()
    }

    /// Change the status and tell the window. Progress-only changes are sent at most once per
    /// whole percent.
    fn update(&self, f: impl FnOnce(&mut BackupStatus)) {
        let mut st = self.state();
        f(&mut st.status);
        st.status.rev += 1;
        let pct = st.status.progress.map_or(-1, |p| (p * 100.0).floor() as i64);
        let key = (st.status.phase, pct);
        if st.last_sent == Some(key) {
            return;
        }
        st.last_sent = Some(key);
        let status = st.status.clone();
        drop(st);
        if let Some(app) = &self.inner.app {
            let _ = app.emit(EVENT, status);
        }
    }

    /// The backups on this PC, newest first.
    pub fn list(&self) -> Vec<BackupInfo> {
        let found = discover::list(&discover::default_roots());
        let mut st = self.state();
        found
            .into_iter()
            .map(|(info, path)| {
                st.known.insert(info.id.clone(), (path, info.clone()));
                info
            })
            .collect()
    }

    /// A folder the person picked in Windows' folder picker.
    pub fn picked(&self, folder: &Path) -> Option<BackupInfo> {
        let (info, path) = discover::describe_picked(folder)?;
        self.state().known.insert(info.id.clone(), (path, info.clone()));
        Some(info)
    }

    /// Unlock (with `password` for an encrypted backup) and read a backup, on a thread of its own.
    /// Progress and the outcome arrive as status events: `Ready` with a scan, or `Failed` with why.
    pub fn open(&self, id: &str, password: Option<String>) -> Result<(), String> {
        // Held for as short a time as possible, and wiped when it goes.
        let password = password.map(Zeroizing::new);
        let mut st = self.state();
        if st.job.is_some() {
            return Err("tug is already reading a backup.".into());
        }
        let (folder, info) = st.known.get(id).cloned().ok_or("Choose the backup again.")?;
        let cancel = Arc::new(AtomicBool::new(false));
        st.job = Some(cancel.clone());
        st.session = None;
        drop(st);
        self.update(|s| {
            *s = BackupStatus {
                rev: s.rev,
                phase: if info.encrypted {
                    Phase::Unlocking
                } else {
                    Phase::Reading
                },
                backup: Some(info.clone()),
                ..BackupStatus::idle()
            }
        });
        let me = self.clone();
        std::thread::Builder::new()
            .name("tug-backup".into())
            .spawn(move || me.open_job(folder, info, password, cancel))
            .map_err(|_| self.not_started())?;
        Ok(())
    }

    fn open_job(
        &self,
        folder: PathBuf,
        info: BackupInfo,
        password: Option<Zeroizing<String>>,
        cancel: Arc<AtomicBool>,
    ) {
        let started = Instant::now();
        // Read and compare without holding the service's lock, so Cancel and the status answer at
        // once however big the backup is.
        let result = self.read_backup(&folder, info, password, &cancel).and_then(|session| {
            log::info!(
                "backup: read {} messages in {} chats ({} reactions and {} other rows left out) in {:.1} s",
                session.data.messages.len(),
                session.data.chats.len(),
                session.data.skipped_reactions,
                session.data.skipped_events,
                started.elapsed().as_secs_f64()
            );
            match import::scan(&self.inner.store, &session.data, session.info.last_backup, &|p| {
                sizes(&session, p)
            }) {
                Ok(scan) => Ok((session, scan)),
                Err(_) => {
                    log::warn!("backup: couldn't compare the backup with tug's history");
                    self.fail(Problem::Failed);
                    Err(Phase::Failed)
                }
            }
        });
        // One press, one action: a Cancel that landed while it read or compared still cancels.
        let result = match result {
            Ok(_) if cancel.load(Ordering::Relaxed) => Err(Phase::Cancelled),
            other => other,
        };
        let mut st = self.state();
        st.job = None;
        match result {
            Ok((session, scan)) => {
                st.session = Some(Arc::new(session));
                drop(st);
                self.update(|s| {
                    s.phase = Phase::Ready;
                    s.progress = None;
                    s.scan = Some(scan);
                });
            }
            Err(Phase::Cancelled) => {
                drop(st);
                log::info!(
                    "backup: reading cancelled after {:.1} s",
                    started.elapsed().as_secs_f64()
                );
                self.update(|s| {
                    s.phase = Phase::Cancelled;
                    s.progress = None;
                });
            }
            // Failed: the status already says why.
            Err(_) => {}
        }
    }

    /// Whether reading or importing is under way (Clear history and Remove imported history wait
    /// for it, so nothing it writes afterwards outlives them).
    pub fn busy(&self) -> bool {
        self.state().job.is_some()
    }

    /// The job's thread couldn't start: nothing is running after all.
    fn not_started(&self) -> String {
        self.state().job = None;
        self.fail(Problem::Failed);
        "Couldn't start. Try again.".to_string()
    }

    fn fail(&self, problem: Problem) {
        self.update(|s| {
            s.phase = Phase::Failed;
            s.progress = None;
            s.problem = Some(problem);
        });
    }

    /// Returns the read session, or the phase it stopped in (`Cancelled`, or `Failed` after the
    /// status says why).
    fn read_backup(
        &self,
        folder: &Path,
        info: BackupInfo,
        password: Option<Zeroizing<String>>,
        cancel: &AtomicBool,
    ) -> Result<Session, Phase> {
        let fail = |p: Problem, why: &str| {
            log::warn!("backup: couldn't read it: {why}");
            self.fail(p);
            Phase::Failed
        };
        if let Some(p) = info.problem {
            return Err(fail(p.into(), "not a readable backup"));
        }
        let manifest = plist::Value::from_file(folder.join("Manifest.plist"))
            .ok()
            .and_then(plist::Value::into_dictionary)
            .ok_or_else(|| fail(Problem::Unreadable, "Manifest.plist"))?;
        let temp = TempDir::create(&self.inner.local_dir).map_err(|_| fail(Problem::CantWrite, "scratch folder"))?;

        let keys = if info.encrypted {
            let Some(password) = password else {
                return Err(fail(Problem::NeedsPassword, "no password"));
            };
            let bag = manifest
                .get("BackupKeyBag")
                .and_then(plist::Value::as_data)
                .and_then(|d| Keybag::parse(d).ok())
                .ok_or_else(|| fail(Problem::Unreadable, "keybag"))?;
            let total = bag.slow_rounds().max(1) as f64;
            let t = Instant::now();
            let key = bag
                .derive_key(&password, |done| {
                    self.update(|s| s.progress = Some((done as f64 / total).min(1.0)));
                    !cancel.load(Ordering::Relaxed)
                })
                .ok_or(Phase::Cancelled)?;
            drop(password);
            let keys = bag.unlock(&key).ok_or_else(|| {
                log::info!(
                    "backup: the password didn't unlock it ({:.1} s)",
                    t.elapsed().as_secs_f64()
                );
                self.fail(Problem::WrongPassword);
                Phase::Failed
            })?;
            log::info!("backup: unlocked in {:.1} s", t.elapsed().as_secs_f64());
            Some(keys)
        } else {
            None
        };
        if cancel.load(Ordering::Relaxed) {
            return Err(Phase::Cancelled);
        }
        self.update(|s| {
            s.phase = Phase::Reading;
            s.progress = None;
        });

        // Manifest.db: decrypted (or copied) into the scratch folder, read, and deleted.
        let manifest_db = temp.file("Manifest.db");
        match &keys {
            Some(keys) => {
                let key = manifest
                    .get("ManifestKey")
                    .and_then(plist::Value::as_data)
                    .and_then(|k| keys.unwrap_tagged(k))
                    .ok_or_else(|| fail(Problem::Unreadable, "manifest key"))?;
                let mut bytes =
                    std::fs::read(folder.join("Manifest.db")).map_err(|_| fail(Problem::Unreadable, "Manifest.db"))?;
                let n = decrypt::decrypt_in_place(&key, &mut bytes, None)
                    .map_err(|_| fail(Problem::Unreadable, "Manifest.db"))?;
                let written = std::fs::write(&manifest_db, &bytes[..n]);
                zeroize::Zeroize::zeroize(&mut bytes);
                written.map_err(|_| fail(Problem::CantWrite, "Manifest.db copy"))?;
            }
            None => {
                std::fs::copy(folder.join("Manifest.db"), &manifest_db)
                    .map_err(|_| fail(Problem::Unreadable, "Manifest.db"))?;
            }
        }
        let conn = manifest::open(&manifest_db).map_err(|_| fail(Problem::Unreadable, "Manifest.db open"))?;
        let sms = manifest::lookup(&conn, manifest::SMS_DOMAIN, manifest::SMS_DB)
            .map_err(|_| fail(Problem::Unreadable, "Manifest.db query"))?
            .ok_or_else(|| fail(Problem::NoMessages, "no Messages database"))?;
        let extras: Vec<(&str, Option<FileEntry>)> = ["-wal", "-shm"]
            .iter()
            .map(|suffix| {
                (
                    *suffix,
                    manifest::lookup(&conn, manifest::SMS_DOMAIN, &format!("{}{suffix}", manifest::SMS_DB))
                        .ok()
                        .flatten(),
                )
            })
            .collect();
        let attachments = manifest::attachments(&conn).map_err(|_| fail(Problem::Unreadable, "attachments list"))?;
        drop(conn);
        let _ = std::fs::remove_file(&manifest_db);

        let sms_db = temp.file("sms.db");
        let extract = |entry: &FileEntry, dest: &Path| -> Result<(), Phase> {
            let src = entry
                .path_in(folder)
                .ok_or_else(|| fail(Problem::Unreadable, "file id"))?;
            let key = match &keys {
                Some(k) => Some(
                    entry
                        .encryption_key
                        .as_deref()
                        .and_then(|t| k.unwrap_tagged(t))
                        .ok_or_else(|| fail(Problem::Unreadable, "file key"))?,
                ),
                None => None,
            };
            decrypt::copy_file(&src, dest, key.as_ref(), entry.size, || !cancel.load(Ordering::Relaxed)).map_err(
                |e| match e {
                    decrypt::CopyError::Cancelled => Phase::Cancelled,
                    _ => fail(Problem::Unreadable, "Messages database"),
                },
            )?;
            Ok(())
        };
        extract(&sms, &sms_db)?;
        for (suffix, entry) in &extras {
            if let Some(entry) = entry {
                // A write-ahead log the backup kept: SQLite applies it when the database opens.
                extract(entry, &temp.file(&format!("sms.db{suffix}")))?;
            }
        }
        // Opened writable only so SQLite can apply a write-ahead log the backup kept; the file is
        // untrusted, so its schema gets no say (no triggers or views running tug's functions) and
        // SQLite's defensive mode refuses anything that could corrupt or escape it.
        let data = rusqlite::Connection::open(&sms_db)
            .and_then(|c| {
                c.pragma_update(None, "trusted_schema", "OFF")?;
                c.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
                smsdb::read(&c)
            })
            .map_err(|_| fail(Problem::Unreadable, "Messages database tables"))?;
        // The decrypted database has done its job; only what's in memory is needed now.
        for name in ["sms.db", "sms.db-wal", "sms.db-shm"] {
            let _ = std::fs::remove_file(temp.file(name));
        }
        if data.messages.is_empty() {
            return Err(fail(Problem::NoMessages, "no messages"));
        }
        Ok(Session {
            root: folder.to_path_buf(),
            info,
            temp,
            keys,
            data,
            attachments,
        })
    }

    /// Bring the read backup in: texts, and photos if asked. On its own thread; progress, the
    /// summary or the problem arrive as status events. Closes the session when it ends.
    pub fn import(&self, texts: bool, photos: bool) -> Result<(), String> {
        if !texts && !photos {
            return Err("Choose texts, photos or both.".into());
        }
        let mut st = self.state();
        if st.job.is_some() {
            return Err("tug is already working on a backup.".into());
        }
        let session = st.session.clone().ok_or("Choose the backup again.")?;
        let cancel = Arc::new(AtomicBool::new(false));
        st.job = Some(cancel.clone());
        drop(st);
        self.update(|s| {
            s.phase = Phase::Importing;
            s.stage = Some(ImportStage::Texts);
            s.progress = Some(0.0);
            s.done = 0;
            s.total = 0;
            s.summary = None;
            s.problem = None;
        });
        let me = self.clone();
        std::thread::Builder::new()
            .name("tug-backup".into())
            .spawn(move || me.import_job(session, Options { texts, photos }, cancel))
            .map_err(|_| self.not_started())?;
        Ok(())
    }

    fn import_job(&self, session: Arc<Session>, opts: Options, cancel: Arc<AtomicBool>) {
        let started = Instant::now();
        let mut sink = BackupSink {
            session: &session,
            local_dir: &self.inner.local_dir,
            cancel: &cancel,
            next: AtomicU64::new(0),
        };
        let result = import::import(
            &self.inner.store,
            &session.data,
            opts,
            session.info.last_backup,
            &mut sink,
            &mut |stage, done, total| {
                self.update(|s| {
                    s.stage = Some(match stage {
                        Stage::Texts => ImportStage::Texts,
                        Stage::Photos => ImportStage::Photos,
                    });
                    s.done = done;
                    s.total = total;
                    s.progress = Some(if total == 0 { 1.0 } else { done as f64 / total as f64 });
                });
                !cancel.load(Ordering::Relaxed)
            },
        );
        {
            let mut st = self.state();
            st.job = None;
            // The keys, the messages in memory and the scratch folder go now.
            st.session = None;
        }

        drop(session);
        let secs = started.elapsed().as_secs_f64();
        match result {
            Ok(summary) => {
                log::info!(
                    "backup: imported {} texts, {} photos and {} videos in {} conversations ({} already in tug, {} files missing) in {secs:.1} s",
                    summary.texts,
                    summary.photos,
                    summary.videos,
                    summary.conversations,
                    summary.already,
                    summary.missing
                );
                self.update(|s| {
                    s.phase = Phase::Done;
                    s.progress = None;
                    s.summary = Some(summary);
                });
            }
            Err(ImportError::Cancelled) => {
                log::info!(
                    "backup: import cancelled after {secs:.1} s (what was in stays; importing again carries on)"
                );
                self.update(|s| {
                    s.phase = Phase::Cancelled;
                    s.progress = None;
                });
            }
            Err(ImportError::Files) => {
                log::warn!("backup: import stopped: couldn't write a photo to this PC");
                self.fail(Problem::CantWrite);
            }
            Err(ImportError::Store) => {
                log::warn!("backup: import stopped: couldn't write to tug's history");
                self.fail(Problem::Failed);
            }
        }
    }

    /// Cancel what's running (reading or importing), or close a read backup.
    pub fn cancel(&self) {
        let mut st = self.state();
        if let Some(job) = &st.job {
            job.store(true, Ordering::Relaxed);
            return;
        }
        st.session = None;
        drop(st);
        self.update(|s| {
            *s = BackupStatus {
                rev: s.rev,
                ..BackupStatus::idle()
            }
        });
    }
}

/// A backup file's plaintext size: from the manifest, else the file itself.
fn sizes(session: &Session, path: &str) -> Option<u64> {
    let entry = session.attachments.get(path)?;
    entry.size.or_else(|| {
        entry
            .path_in(&session.root)
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
    })
}

/// Copies photos and videos out of the backup into the attachments folder.
struct BackupSink<'a> {
    session: &'a Session,
    local_dir: &'a Path,
    cancel: &'a AtomicBool,
    next: AtomicU64,
}

/// Windows' "disk full" errors.
fn disk_full(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(39) | Some(112)) || e.kind() == std::io::ErrorKind::StorageFull
}

impl FileSink for BackupSink<'_> {
    fn store(&mut self, path: &str, ext: &str) -> Result<Option<(String, u64)>, SinkError> {
        let Some(entry) = self.session.attachments.get(path) else {
            return Ok(None);
        };
        let Some(src) = entry.path_in(&self.session.root).filter(|p| p.is_file()) else {
            return Ok(None);
        };
        let key = match &self.session.keys {
            Some(keys) => match entry.encryption_key.as_deref().and_then(|t| keys.unwrap_tagged(t)) {
                Some(k) => Some(k),
                None => return Ok(None),
            },
            None => None,
        };
        let tmp = self
            .session
            .temp
            .file(&format!("a{}", self.next.fetch_add(1, Ordering::Relaxed)));
        let copied = decrypt::copy_file(&src, &tmp, key.as_ref(), entry.size, || {
            !self.cancel.load(Ordering::Relaxed)
        });
        let copied = match copied {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return match e {
                    decrypt::CopyError::Cancelled => Err(SinkError::Cancelled),
                    decrypt::CopyError::Io(e) if disk_full(&e) => Err(SinkError::Failed),
                    // One damaged file doesn't stop the rest.
                    _ => Ok(None),
                };
            }
        };
        match files::adopt(self.local_dir, &tmp, &copied.sha256, ext) {
            Ok(key) => Ok(Some((key, copied.bytes))),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                if disk_full(&e) {
                    Err(SinkError::Failed)
                } else {
                    Ok(None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The whole path against a made-up encrypted backup on disk: plists, keybag, encrypted
    //! Manifest.db, encrypted sms.db and an encrypted photo, all generated here.
    use super::*;
    use crate::backup::decrypt::tests::encrypt;
    use crate::backup::discover::tests::{temp_dir, write_plists};
    use crate::backup::keybag::tests::{make_keybag, wrap};
    use crate::backup::manifest::tests::{file_blob, make_manifest};
    use plist::{Dictionary, Value};
    use std::time::{Duration, SystemTime};

    const PASSWORD: &str = "made-up password";

    fn tagged(class: u32, class_key: &[u8; 32], key: &[u8; 32]) -> Vec<u8> {
        let mut t = class.to_le_bytes().to_vec();
        t.extend(wrap(class_key, key));
        t
    }

    /// Writes an encrypted backup of the sample sms.db (with its photo) into `folder`.
    fn make_encrypted_backup(folder: &Path, scratch: &Path) {
        let class3 = [0x33u8; 32];
        let (bag, _) = make_keybag(PASSWORD, &[(3, class3), (4, [0x44; 32])]);
        let manifest_key = [0x4D; 32];
        let mut extra = Dictionary::new();
        extra.insert("BackupKeyBag".into(), Value::Data(bag));
        extra.insert("ManifestKey".into(), Value::Data(tagged(3, &class3, &manifest_key)));
        write_plists(
            folder,
            "Test iPhone",
            true,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000),
            extra,
        );

        // sms.db, encrypted with its own key.
        let plain_sms = scratch.join("plain-sms.db");
        let conn = rusqlite::Connection::open(&plain_sms).unwrap();
        smsdb::tests::sample(&conn);
        drop(conn);
        let sms_key = [0x51; 32];
        let sms_bytes = std::fs::read(&plain_sms).unwrap();
        let sms_id = "3d0d7e5fb2ce288813306e4d4636395e047a3d28";
        std::fs::create_dir_all(folder.join("3d")).unwrap();
        std::fs::write(folder.join("3d").join(sms_id), encrypt(&sms_key, &sms_bytes)).unwrap();
        // The photo; the video is listed but missing from the backup.
        let photo = b"\xFF\xD8\xFFpretend this is a HEIC photo".to_vec();
        let photo_key = [0x50; 32];
        let photo_id = "ab".repeat(20);
        std::fs::create_dir_all(folder.join("ab")).unwrap();
        std::fs::write(folder.join("ab").join(&photo_id), encrypt(&photo_key, &photo)).unwrap();

        let manifest_plain = scratch.join("plain-manifest.db");
        make_manifest(
            &manifest_plain,
            &[
                (
                    manifest::SMS_DOMAIN,
                    manifest::SMS_DB,
                    sms_id,
                    Some(file_blob(sms_bytes.len() as u64, Some(&tagged(3, &class3, &sms_key)))),
                ),
                (
                    "MediaDomain",
                    "Library/SMS/Attachments/ab/11/ATT-1/IMG_0001.HEIC",
                    &photo_id,
                    Some(file_blob(photo.len() as u64, Some(&tagged(3, &class3, &photo_key)))),
                ),
            ],
        );
        let manifest_bytes = std::fs::read(&manifest_plain).unwrap();
        std::fs::write(folder.join("Manifest.db"), encrypt(&manifest_key, &manifest_bytes)).unwrap();
    }

    fn wait_for(service: &BackupService, phases: &[Phase]) -> BackupStatus {
        for _ in 0..2000 {
            let s = service.status();
            if phases.contains(&s.phase) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out in {:?}", service.status().phase);
    }

    #[test]
    fn an_encrypted_backup_unlocks_reads_and_imports_and_leaves_nothing_decrypted() {
        let root = temp_dir("backup-e2e");
        let folder = root.join("00008110-000A11112222801E");
        let local = root.join("local");
        std::fs::create_dir_all(&local).unwrap();
        make_encrypted_backup(&folder, &root);
        let store = Arc::new(Store::in_memory().unwrap());
        let service = BackupService::build(None, store.clone(), local.clone());
        let info = service.picked(&folder).unwrap();
        assert!(info.encrypted);

        // No password, then a wrong one.
        service.open(&info.id, None).unwrap();
        let s = wait_for(&service, &[Phase::Failed, Phase::Ready]);
        assert_eq!(s.problem, Some(Problem::NeedsPassword));
        service.open(&info.id, Some("wrong".into())).unwrap();
        let s = wait_for(&service, &[Phase::Failed, Phase::Ready]);
        assert_eq!(s.problem, Some(Problem::WrongPassword));

        service.open(&info.id, Some(PASSWORD.into())).unwrap();
        let s = wait_for(&service, &[Phase::Failed, Phase::Ready]);
        assert_eq!(s.phase, Phase::Ready, "{:?}", s.problem);
        let scan = s.scan.unwrap();
        assert_eq!(scan.texts, 6);
        assert_eq!(scan.group_texts, 3, "the group chat isn't brought in");
        assert_eq!((scan.photos, scan.videos), (1, 0), "the video isn't in the backup");
        // While it waits for "Bring it in", nothing decrypted is on disk.
        let scratch = local.join(files::TEMP_DIR);
        let leftovers: Vec<_> = walk(&scratch);
        assert!(leftovers.is_empty(), "{leftovers:?}");

        service.import(true, true).unwrap();
        let s = wait_for(&service, &[Phase::Done, Phase::Failed, Phase::Cancelled]);
        assert_eq!(s.phase, Phase::Done, "{:?}", s.problem);
        let summary = s.summary.unwrap();
        assert_eq!((summary.texts, summary.photos, summary.missing), (6, 1, 1));
        // The photo is stored, decrypted and named by its content; the scratch folder is gone.
        let keys = import::file_keys(&store).unwrap();
        assert_eq!(keys.len(), 1);
        let key = keys.into_iter().next().unwrap();
        assert!(key.ends_with(".heic"));
        let stored = std::fs::read(files::attachment_path(&local, &key).unwrap()).unwrap();
        assert!(stored.starts_with(b"\xFF\xD8\xFFpretend"));
        assert!(walk(&scratch).is_empty());

        // Opened again and imported again: nothing new.
        service.open(&info.id, Some(PASSWORD.into())).unwrap();
        let s = wait_for(&service, &[Phase::Failed, Phase::Ready]);
        assert_eq!(s.scan.unwrap().texts, 0);
        service.cancel();
        assert_eq!(service.status().phase, Phase::Idle);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancelling_the_unlock_stops_it_and_cleans_up() {
        let root = temp_dir("backup-cancel");
        let folder = root.join("backup");
        let local = root.join("local");
        std::fs::create_dir_all(&local).unwrap();
        make_encrypted_backup(&folder, &root);
        let service = BackupService::build(None, Arc::new(Store::in_memory().unwrap()), local.clone());
        let info = service.picked(&folder).unwrap();
        service.open(&info.id, Some(PASSWORD.into())).unwrap();
        service.cancel();
        let s = wait_for(&service, &[Phase::Cancelled, Phase::Ready, Phase::Failed]);
        // A tiny test keybag may finish before the cancel lands; either way nothing is left behind
        // once the session is closed.
        if s.phase == Phase::Ready {
            service.cancel();
        }
        assert!(walk(&local.join(files::TEMP_DIR)).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for e in entries.flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    out.push(e.path());
                }
            }
        }
        out
    }
}
