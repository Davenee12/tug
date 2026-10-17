//! Bringing a backup's messages into tug's history: deciding what's new, merging with the texts
//! tug already has from Bluetooth, and writing it in resumable batches.
//!
//! - Every imported text is a `messages` row with source `iphone-backup`, keyed by the backup's
//!   message guid, so running the import again (after a cancel, a crash or a newer backup) adds
//!   only what isn't there yet.
//! - A text tug already has from Bluetooth isn't added twice: same direction, same person (by
//!   `store::match_key`), same words, and close in time. The Bluetooth copy stays; the backup only
//!   adds what it lacked (the photos of a text Bluetooth showed as "Attachment: 1 Photo", or the
//!   group it was sent in).
//! - People are tug's: a number the backup writes as `+447700900123` joins the conversation tug
//!   already has under `07700 900123` (the same last 10 digits), and contacts' names apply as usual.
//! - Photos and videos are copied in a second pass, through a `FileSink`, so they can be skipped
//!   and filled in later.
//!
//! Nothing here logs a word of any message, a name or a number.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::smsdb::{Message, Service, SmsData};
use crate::messages::SOURCE_IPHONE_BACKUP;
use crate::store::{match_key, Result, Store};

/// Settings key: when the history in tug was brought in, and from a backup made when (JSON).
pub const IMPORT_RECORD: &str = "backup_import";

/// A Bluetooth text tug got within this of the backup's time is the same text (tug saw it live).
const LIVE_TOLERANCE_MS: i64 = 10 * 60 * 1000;
/// The phone's own time for a Bluetooth text, when it carries a zone: a clock's worth of slack.
const ZONED_TOLERANCE_MS: i64 = 2 * 60 * 1000;
/// The phone's time without a zone is its local time, read here as if UTC: it can be off by the
/// phone's offset from UTC, up to 14 hours.
const NAIVE_TOLERANCE_MS: i64 = 14 * 60 * 60 * 1000 + ZONED_TOLERANCE_MS;

/// Rows per transaction: a crash or cancel loses at most one batch, and the store is never held
/// for long.
const BATCH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub texts: bool,
    pub photos: bool,
}

/// What an import would add, shown before it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    /// Texts tug doesn't have yet.
    pub texts: u64,
    /// Texts tug already has (an earlier import, or the same text from Bluetooth).
    pub already: u64,
    /// Conversations the new texts are in.
    pub conversations: u64,
    /// Photos and videos not copied yet, and their size.
    pub photos: u64,
    pub videos: u64,
    pub media_bytes: u64,
    /// Texts in a new conversation that only photos are in (when texts are skipped): counted so
    /// "photos only" can say how many texts come with them.
    pub photo_texts: u64,
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub texts: u64,
    pub photos: u64,
    pub conversations: u64,
    /// Texts tug already had.
    pub already: u64,
    /// Photos the backup listed but didn't have the file for.
    pub missing: u64,
}

/// Where photos and videos go: copies the backup's file at `path` (relative to the phone's home)
/// and returns its stored key and size, or `None` when the backup doesn't have it.
pub trait FileSink {
    fn store(&mut self, path: &str, ext: &str) -> std::result::Result<Option<(String, u64)>, SinkError>;
}

#[derive(Debug)]
pub enum SinkError {
    Cancelled,
    /// Couldn't write (the disk is full, or the folder is gone).
    Failed,
}

#[derive(Debug)]
pub enum ImportError {
    Cancelled,
    /// tug's own database refused a write (never logged in detail: it could quote a text).
    Store,
    Files,
}

impl From<rusqlite::Error> for ImportError {
    fn from(_: rusqlite::Error) -> Self {
        ImportError::Store
    }
}

/// Progress for the bar: texts first, then photos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Texts,
    Photos,
}

/// How each backup message lands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A new row: the address (one-to-one, or the sender in a group; "" for one's own group text)
    /// and the group.
    New { address: String, group: Option<String> },
    /// Imported before (that row).
    Imported(i64),
    /// The same text tug has from Bluetooth (that row), and the group it was really in.
    Bluetooth { id: i64, group: Option<String> },
    /// Nobody to put it with (no handle, no chat).
    Skip,
}

struct Plan<'a> {
    items: Vec<(&'a Message, Target)>,
    /// Groups the new rows need: id → (name, members).
    groups: HashMap<String, (Option<String>, Vec<String>)>,
}

/// The id tug keeps for a backup's group chat: opaque and fixed-length, the same every import.
pub fn group_id(group_key: &str) -> String {
    let digest = Sha256::digest(group_key.as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("g{hex}")
}

/// How a number or email is compared: the last 10 digits of a number, else the whole thing in
/// lower case.
fn person_key(address: &str) -> String {
    match_key(address).unwrap_or_else(|| address.trim().to_lowercase())
}

/// A text's words as compared for duplicates: trimmed, Windows line ends unified, without the
/// "Attachment: 1 Photo" line iOS puts in a photo's Bluetooth copy.
fn compare_body(body: &str) -> String {
    body.replace("\r\n", "\n")
        .replace('\u{FFFC}', "")
        .lines()
        .filter(|l| !is_attachment_line(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// "Attachment: 1 Photo", "Attachments: 2 Videos" (the words Bluetooth sends for a photo).
fn is_attachment_line(line: &str) -> bool {
    let l = line.trim().to_lowercase();
    let Some(rest) = l.strip_prefix("attachments").or_else(|| l.strip_prefix("attachment")) else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix(':') else {
        return false;
    };
    let rest = rest.trim_start();
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return false;
    }
    let words: Vec<&str> = rest[digits..].split_whitespace().collect();
    // The kinds iOS names (as `src/lib/attachments.ts` reads them), so "Attachment: 2 are below"
    // stays someone's words.
    const KINDS: &[&str] = &[
        "photo",
        "image",
        "picture",
        "pic",
        "video",
        "movie",
        "sticker",
        "memoji",
        "audio",
        "voice",
        "gif",
        "file",
        "document",
        "pdf",
        "contact",
        "vcard",
        "location",
        "attachment",
    ];
    (1..=2).contains(&words.len())
        && words.iter().all(|w| w.chars().all(char::is_alphabetic))
        && KINDS
            .iter()
            .any(|k| words[0].strip_suffix('s').unwrap_or(words[0]) == *k || words[0] == *k)
}

/// An ISO time as tug stores a phone's (`2026-10-04T19:11:17`, maybe with fractions and a `Z`) to
/// ms since 1970 read as UTC, and whether it had the `Z`.
pub fn parse_iso_ms(s: &str) -> Option<(i64, bool)> {
    let zoned = s.ends_with('Z');
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, mo, da) = (
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<i64>().ok()?,
    );
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.split(':');
    let (h, mi, se) = (
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let ms_frac = if frac.is_empty() {
        0
    } else {
        let f: String = frac.chars().take(3).collect();
        f.parse::<i64>().ok()? * 10i64.pow(3 - f.len() as u32)
    };
    let days = days_from_civil(y, mo, da);
    Some((((days * 24 + h) * 60 + mi) * 60_000 + se * 1000 + ms_frac, zoned))
}

/// ms since 1970 as `YYYY-MM-DDTHH:MM:SSZ` (how tug stores a zoned phone time).
pub fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// Howard Hinnant's days-from-civil and its inverse (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A Bluetooth text that could be the same as a backup's.
struct Candidate {
    id: i64,
    received_at: i64,
    sent_at: Option<(i64, bool)>,
}

impl Candidate {
    /// How close it is to `at` (smaller is better), if close enough to be the same text at all.
    fn distance(&self, at: i64) -> Option<i64> {
        let live = (self.received_at - at).abs();
        if live <= LIVE_TOLERANCE_MS {
            return Some(live);
        }
        let (sent, zoned) = self.sent_at?;
        let d = (sent - at).abs();
        if zoned {
            (d <= ZONED_TOLERANCE_MS).then_some(d)
        } else {
            // Ranked after every live match: a zone guess is the weaker evidence.
            (d <= NAIVE_TOLERANCE_MS).then_some(LIVE_TOLERANCE_MS + d)
        }
    }
}

/// tug's own view of people and texts, read once per plan.
struct Known {
    /// person key → the address tug already uses for them.
    addresses: HashMap<String, String>,
    /// (direction, person key, compared words) → Bluetooth texts.
    texts: HashMap<(bool, String, String), Vec<Candidate>>,
    /// The backup's message guids already imported → row id.
    imported: HashMap<String, i64>,
}

fn load_known(store: &Store) -> Result<Known> {
    let conn = store.conn();
    let mut addresses = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT address FROM messages WHERE source <> ?1 AND address <> '' GROUP BY address ORDER BY MAX(received_at)",
        )?;
        let rows = stmt.query_map([SOURCE_IPHONE_BACKUP], |r| r.get::<_, String>(0))?;
        for a in rows {
            let a = a?;
            // The most recent use wins (rows come oldest first).
            addresses.insert(person_key(&a), a);
        }
    }
    {
        // A contact's number, when exactly one contact has that key and no text uses it yet.
        let mut by_key: HashMap<String, HashSet<String>> = HashMap::new();
        let mut stmt = conn.prepare("SELECT address FROM contacts")?;
        for a in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let a = a?;
            by_key.entry(person_key(&a)).or_default().insert(a);
        }
        for (key, set) in by_key {
            if set.len() == 1 {
                addresses
                    .entry(key)
                    .or_insert_with(|| set.into_iter().next().unwrap_or_default());
            }
        }
    }
    let mut texts: HashMap<(bool, String, String), Vec<Candidate>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, direction, address, body, sent_at, received_at FROM messages
             WHERE source <> ?1 AND hidden_at IS NULL AND address <> ''",
        )?;
        let rows = stmt.query_map([SOURCE_IPHONE_BACKUP], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (id, direction, address, body, sent_at, received_at) = row?;
            let key = (direction == "out", person_key(&address), compare_body(&body));
            texts.entry(key).or_default().push(Candidate {
                id,
                received_at,
                sent_at: sent_at.as_deref().and_then(parse_iso_ms),
            });
        }
    }
    let mut imported = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT handle, id FROM messages WHERE source = ?1 AND handle IS NOT NULL")?;
        for row in stmt.query_map([SOURCE_IPHONE_BACKUP], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })? {
            let (guid, id) = row?;
            imported.insert(guid, id);
        }
    }
    Ok(Known {
        addresses,
        texts,
        imported,
    })
}

/// The address tug uses for a backup handle: the one it already has for that person, else the
/// handle written the way tug writes numbers.
fn resolve(known: &Known, handle: &str) -> String {
    let normalized = crate::map::address::normalize(handle);
    known
        .addresses
        .get(&person_key(&normalized))
        .cloned()
        .unwrap_or(normalized)
}

fn plan<'a>(store: &Store, data: &'a SmsData) -> Result<Plan<'a>> {
    let known = load_known(store)?;
    let mut used: HashSet<i64> = HashSet::new();
    let mut groups = HashMap::new();
    let mut items = Vec::with_capacity(data.messages.len());
    for m in &data.messages {
        let chat = m.chat.and_then(|c| data.chats.get(&c));
        let group = chat.filter(|c| c.is_group).map(|c| group_id(&c.group_key));
        let address = match (&group, m.from_me) {
            (Some(_), true) => Some(String::new()),
            (Some(_), false) => m.handle.as_deref().map(|h| resolve(&known, h)),
            (None, _) => m
                .handle
                .as_deref()
                .or_else(|| chat.and_then(|c| c.members.first()).map(String::as_str))
                .map(|h| resolve(&known, h)),
        };
        let target = if let Some(&id) = known.imported.get(&m.guid) {
            Target::Imported(id)
        } else if let Some(address) = address {
            let same = (!m.unsent && !address.is_empty())
                .then(|| {
                    known
                        .texts
                        .get(&(m.from_me, person_key(&address), compare_body(&m.body)))
                })
                .flatten()
                .and_then(|cands| {
                    cands
                        .iter()
                        .filter(|c| !used.contains(&c.id))
                        .filter_map(|c| c.distance(m.at_ms).map(|d| (d, c.id)))
                        .min()
                })
                .map(|(_, id)| id);
            match same {
                Some(id) => {
                    used.insert(id);
                    Target::Bluetooth {
                        id,
                        group: group.clone(),
                    }
                }
                None => Target::New {
                    address,
                    group: group.clone(),
                },
            }
        } else {
            Target::Skip
        };
        if let (Some(gid), Some(chat)) = (&group, chat) {
            if matches!(target, Target::New { .. } | Target::Bluetooth { .. }) && !groups.contains_key(gid) {
                let members = chat.members.iter().map(|h| resolve(&known, h)).collect();
                groups.insert(gid.clone(), (chat.name.clone(), members));
            }
        }
        items.push((m, target));
    }
    Ok(Plan { items, groups })
}

fn wanted(m: &Message, opts: Options) -> bool {
    opts.texts || (opts.photos && m.attachments.iter().any(|a| a.ext.is_some()))
}

fn conversation_key(target: &Target) -> Option<String> {
    match target {
        Target::New { group: Some(g), .. } => Some(g.clone()),
        Target::New { address, group: None } => Some(person_key(address)),
        _ => None,
    }
}

/// What importing would add, against what tug has now.
pub fn scan(store: &Store, data: &SmsData, sizes: &dyn Fn(&str) -> Option<u64>) -> Result<Scan> {
    let plan = plan(store, data)?;
    let stored = stored_keys(store)?;
    let mut scan = Scan::default();
    let mut convs = HashSet::new();
    for (m, target) in &plan.items {
        match target {
            Target::New { .. } => {
                scan.texts += 1;
                if m.attachments.iter().any(|a| a.ext.is_some()) {
                    scan.photo_texts += 1;
                }
                if let Some(k) = conversation_key(target) {
                    convs.insert(k);
                }
            }
            Target::Imported(_) | Target::Bluetooth { .. } => scan.already += 1,
            Target::Skip => {}
        }
        if matches!(target, Target::Skip) {
            continue;
        }
        for a in m.attachments.iter().filter(|a| a.ext.is_some()) {
            if stored.contains(&a.guid) {
                continue;
            }
            let Some(path) = a.path.as_deref() else { continue };
            let Some(size) = sizes(path) else { continue };
            if a.kind == super::smsdb::AttachmentKind::Video {
                scan.videos += 1;
            } else {
                scan.photos += 1;
            }
            scan.media_bytes += size;
        }
    }
    scan.conversations = convs.len() as u64;
    Ok(scan)
}

/// The backup attachment guids whose file is already on this PC.
fn stored_keys(store: &Store) -> Result<HashSet<String>> {
    let conn = store.conn();
    let mut stmt = conn.prepare("SELECT source_key FROM message_attachments WHERE source = ?1 AND file IS NOT NULL")?;
    let rows = stmt.query_map([SOURCE_IPHONE_BACKUP], |r| r.get::<_, String>(0))?;
    rows.collect()
}

fn msg_type(m: &Message) -> &'static str {
    match m.service {
        Service::IMessage => "IM",
        Service::Rcs => "RCS",
        Service::Sms if !m.attachments.is_empty() => "MMS",
        Service::Sms | Service::Other => "SMS_GSM",
    }
}

/// Import. `progress(stage, done, total)` is called after each batch; returning false cancels
/// (what was written so far stays, and a later import carries on from there).
pub fn import(
    store: &Store,
    data: &SmsData,
    opts: Options,
    backup_date: Option<i64>,
    sink: &mut dyn FileSink,
    progress: &mut dyn FnMut(Stage, u64, u64) -> bool,
) -> std::result::Result<Summary, ImportError> {
    let plan = plan(store, data)?;
    let mut summary = Summary::default();
    let record = serde_json::json!({ "backupDate": backup_date, "importedAt": crate::state::now_ms() });
    store.set_setting(IMPORT_RECORD, &record.to_string())?;
    {
        let mut conn = store.conn();
        let tx = conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO message_groups (id, name, members, source) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name, members = excluded.members",
            )?;
            for (id, (name, members)) in &plan.groups {
                let members = serde_json::to_string(members).expect("members serialize");
                upsert.execute(params![id, name, members, SOURCE_IPHONE_BACKUP])?;
            }
        }
        tx.commit()?;
    }

    let work: Vec<&(&Message, Target)> = plan
        .items
        .iter()
        .filter(|(m, t)| !matches!(t, Target::Skip) && wanted(m, opts))
        .collect();
    let total = work.len() as u64;
    let mut convs = HashSet::new();
    for (done, batch) in work.chunks(BATCH).enumerate() {
        let mut conn = store.conn();
        let tx = conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO messages (source, handle, direction, address, body, sent_at, received_at,
                     status, msg_type, group_id, edited, unsent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            let mut regroup = tx.prepare("UPDATE messages SET backup_group = ?2 WHERE id = ?1 AND group_id IS NULL")?;
            let mut attach = tx.prepare(
                "INSERT OR IGNORE INTO message_attachments (message_id, source, source_key, kind, ext, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (m, target) in batch.iter().map(|x| (&x.0, &x.1)) {
                let row = match target {
                    Target::New { address, group } => {
                        let n = insert.execute(params![
                            SOURCE_IPHONE_BACKUP,
                            m.guid,
                            if m.from_me { "out" } else { "in" },
                            address,
                            m.body,
                            iso_utc(m.at_ms),
                            m.at_ms,
                            if m.from_me { "sent" } else { "received" },
                            msg_type(m),
                            group,
                            m.edited,
                            m.unsent,
                        ])?;
                        if n == 0 {
                            continue;
                        }
                        summary.texts += 1;
                        if let Some(k) = conversation_key(target) {
                            convs.insert(k);
                        }
                        tx.last_insert_rowid()
                    }
                    Target::Imported(id) => {
                        summary.already += 1;
                        *id
                    }
                    Target::Bluetooth { id, group } => {
                        summary.already += 1;
                        if let Some(g) = group {
                            regroup.execute(params![id, g])?;
                        }
                        *id
                    }
                    Target::Skip => continue,
                };
                for a in &m.attachments {
                    attach.execute(params![
                        row,
                        SOURCE_IPHONE_BACKUP,
                        a.guid,
                        a.kind.as_str(),
                        // Kept even when photos are skipped, so a later import can still copy them.
                        a.ext,
                        a.bytes.and_then(|b| i64::try_from(b).ok()),
                    ])?;
                }
            }
        }
        tx.commit()?;
        drop(conn);
        let done = ((done + 1) * BATCH).min(work.len()) as u64;
        if !progress(Stage::Texts, done, total) {
            return Err(ImportError::Cancelled);
        }
    }
    summary.conversations = convs.len() as u64;

    if opts.photos {
        copy_files(store, data, sink, progress, &mut summary)?;
    }
    Ok(summary)
}

/// Copy every photo and video still missing a file (this run's, and any an earlier, cancelled or
/// photo-less run left), each file once however many texts share it.
fn copy_files(
    store: &Store,
    data: &SmsData,
    sink: &mut dyn FileSink,
    progress: &mut dyn FnMut(Stage, u64, u64) -> bool,
    summary: &mut Summary,
) -> std::result::Result<(), ImportError> {
    let by_guid: HashMap<&str, &super::smsdb::Attachment> = data
        .messages
        .iter()
        .flat_map(|m| m.attachments.iter())
        .filter(|a| a.ext.is_some() && a.path.is_some())
        .map(|a| (a.guid.as_str(), a))
        .collect();
    let pending: Vec<(i64, String)> = {
        let conn = store.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source_key FROM message_attachments
             WHERE source = ?1 AND file IS NULL AND ext IS NOT NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([SOURCE_IPHONE_BACKUP], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_>>()?
    };
    let pending: Vec<(i64, &super::smsdb::Attachment)> = pending
        .into_iter()
        .filter_map(|(id, key)| by_guid.get(key.as_str()).map(|a| (id, *a)))
        .collect();
    let total = pending.len() as u64;
    let mut copied: HashMap<&str, (String, u64)> = HashMap::new();
    for (i, (id, a)) in pending.iter().enumerate() {
        let (Some(path), Some(ext)) = (a.path.as_deref(), a.ext) else {
            continue;
        };
        let stored = match copied.get(a.guid.as_str()) {
            Some(done) => Some(done.clone()),
            None => match sink.store(path, ext) {
                Ok(Some(done)) => {
                    copied.insert(a.guid.as_str(), done.clone());
                    summary.photos += 1;
                    Some(done)
                }
                Ok(None) => {
                    summary.missing += 1;
                    None
                }
                Err(SinkError::Cancelled) => return Err(ImportError::Cancelled),
                Err(SinkError::Failed) => return Err(ImportError::Files),
            },
        };
        if let Some((key, bytes)) = stored {
            store.conn().execute(
                "UPDATE message_attachments SET file = ?2, bytes = ?3 WHERE id = ?1",
                params![id, key, i64::try_from(bytes).unwrap_or(i64::MAX)],
            )?;
        }
        let done = i as u64 + 1;
        if (done.is_multiple_of(20) || done == total) && !progress(Stage::Photos, done, total) {
            return Err(ImportError::Cancelled);
        }
    }
    Ok(())
}

/// What's in tug from backups now, for Settings and the "From your iPhone backup" label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub texts: u64,
    pub photos: u64,
    /// When the backup it came from was made (ms since 1970), if known.
    pub backup_date: Option<i64>,
}

pub fn imported(store: &Store) -> Result<Imported> {
    let backup_date = store
        .setting(IMPORT_RECORD)?
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|v| v.get("backupDate").and_then(serde_json::Value::as_i64));
    let conn = store.conn();
    let texts: i64 = conn.query_row(
        "SELECT count(*) FROM messages WHERE source = ?1",
        [SOURCE_IPHONE_BACKUP],
        |r| r.get(0),
    )?;
    let photos: i64 = conn.query_row(
        "SELECT count(DISTINCT file) FROM message_attachments WHERE source = ?1 AND file IS NOT NULL",
        [SOURCE_IPHONE_BACKUP],
        |r| r.get(0),
    )?;
    Ok(Imported {
        texts: texts as u64,
        photos: photos as u64,
        backup_date: if texts > 0 || photos > 0 { backup_date } else { None },
    })
}

/// Remove imported history: every imported text, every attachment row (on Bluetooth texts too),
/// the groups, the record, and the group a Bluetooth text was moved into. Returns how many texts
/// went. The caller deletes the files.
pub fn remove_imported(store: &Store) -> Result<u64> {
    let mut conn = store.conn();
    let tx = conn.transaction()?;
    let n = tx.execute("DELETE FROM messages WHERE source = ?1", [SOURCE_IPHONE_BACKUP])?;
    tx.execute(
        "DELETE FROM message_attachments WHERE source = ?1",
        [SOURCE_IPHONE_BACKUP],
    )?;
    tx.execute("DELETE FROM message_groups WHERE source = ?1", [SOURCE_IPHONE_BACKUP])?;
    tx.execute(
        "UPDATE messages SET backup_group = NULL WHERE backup_group IS NOT NULL",
        [],
    )?;
    tx.execute("DELETE FROM settings WHERE key = ?1", [IMPORT_RECORD])?;
    tx.commit()?;
    Ok(n as u64)
}

/// The stored file keys still referenced (for deleting the rest).
pub fn file_keys(store: &Store) -> Result<HashSet<String>> {
    let conn = store.conn();
    let mut stmt = conn.prepare("SELECT DISTINCT file FROM message_attachments WHERE file IS NOT NULL")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// One attachment's stored file key, by row id (the window names attachments by id only).
pub fn attachment_file(store: &Store, id: i64) -> Result<Option<(String, String)>> {
    store
        .conn()
        .query_row(
            "SELECT file, kind FROM message_attachments WHERE id = ?1 AND file IS NOT NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::smsdb::{self, tests::sample};
    use crate::messages::{IncomingMessage, SOURCE_IPHONE_MAP};
    use rusqlite::Connection;

    const T0: i64 = 1_760_000_000_000;

    fn data() -> SmsData {
        let conn = Connection::open_in_memory().unwrap();
        sample(&conn);
        smsdb::read(&conn).unwrap()
    }

    /// Hands out made-up file keys, or nothing for paths it's told are missing.
    #[derive(Default)]
    struct FakeSink {
        stored: Vec<String>,
        missing: HashSet<String>,
        cancel_after: Option<usize>,
    }

    impl FileSink for FakeSink {
        fn store(&mut self, path: &str, ext: &str) -> std::result::Result<Option<(String, u64)>, SinkError> {
            if self.cancel_after == Some(self.stored.len()) {
                return Err(SinkError::Cancelled);
            }
            if self.missing.contains(path) {
                return Ok(None);
            }
            self.stored.push(path.to_string());
            Ok(Some((format!("{:032x}.{ext}", self.stored.len()), 100)))
        }
    }

    fn all() -> Options {
        Options {
            texts: true,
            photos: true,
        }
    }

    fn run(store: &Store, data: &SmsData, opts: Options, sink: &mut FakeSink) -> Summary {
        import(store, data, opts, Some(T0), sink, &mut |_, _, _| true).unwrap()
    }

    #[test]
    fn imports_texts_both_ways_with_groups_photos_and_edits() {
        let store = Store::in_memory().unwrap();
        store
            .save_phonebook(&[("+15550100001".into(), "Jane Doe".into())])
            .unwrap();
        let data = data();
        let mut sink = FakeSink::default();
        let s = run(&store, &data, all(), &mut sink);
        assert_eq!(s.texts, 9);
        assert_eq!(s.photos, 2);
        assert_eq!(s.conversations, 2, "Jane and the group");
        assert_eq!(s.already, 0);

        let msgs = store.recent_messages(100).unwrap();
        let jane: Vec<_> = msgs.iter().filter(|m| m.group_id.is_none()).collect();
        assert!(jane.iter().all(|m| m.address == "+15550100001"));
        assert!(jane.iter().all(|m| m.contact_name.as_deref() == Some("Jane Doe")));
        let sent = jane.iter().find(|m| m.body == "yes! see you there").unwrap();
        assert_eq!(sent.direction, crate::messages::Direction::Out);
        assert_eq!(sent.status, crate::messages::Status::Sent);
        assert_eq!(sent.received_at, T0 + 60_000);
        assert_eq!(sent.sent_at.as_deref(), Some("2025-10-09T08:54:20Z"));
        assert_eq!(sent.msg_type.as_deref(), Some("IM"));
        assert!(jane.iter().find(|m| m.body == "meet at the corner").unwrap().edited);
        assert!(jane.iter().any(|m| m.unsent && m.body.is_empty()));
        let photo = jane.iter().find(|m| !m.attachments.is_empty()).unwrap();
        assert_eq!(photo.attachments.len(), 2);
        assert!(photo.attachments.iter().all(|a| a.stored));
        assert_eq!(photo.attachments[0].kind, "photo");
        assert_eq!(photo.attachments[1].kind, "video");

        let group: Vec<_> = msgs.iter().filter(|m| m.group_id.is_some()).collect();
        assert_eq!(group.len(), 3);
        let mine = group.iter().find(|m| m.body == "great, thanks").unwrap();
        assert_eq!(mine.address, "", "one's own group text names no one person");
        let gid = group[0].group_id.clone().unwrap();
        assert_eq!(gid, group_id("G-CREW"));
        let groups = store.message_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name.as_deref(), Some("Weekend crew"));
        assert_eq!(groups[0].members, vec!["+15550100002", "alex@example.com"]);
        // A group's texts never join a one-to-one conversation.
        let alex_one_to_one = store
            .conversation_messages(&["alex@example.com".into()], None, 100)
            .unwrap();
        assert!(alex_one_to_one.is_empty());
        assert_eq!(store.conversation_messages(&[], Some(&gid), 100).unwrap().len(), 3);
        // Searchable like any text.
        assert_eq!(store.search_messages("driving", 10).unwrap().len(), 1);

        let state = imported(&store).unwrap();
        assert_eq!((state.texts, state.photos, state.backup_date), (9, 2, Some(T0)));
    }

    #[test]
    fn importing_again_adds_nothing_and_a_cancelled_import_carries_on() {
        let store = Store::in_memory().unwrap();
        let data = data();
        // Cancelled during the photos: the texts are in, the photos aren't.
        let mut sink = FakeSink {
            cancel_after: Some(0),
            ..Default::default()
        };
        let r = import(&store, &data, all(), Some(T0), &mut sink, &mut |_, _, _| true);
        assert!(matches!(r, Err(ImportError::Cancelled)));
        assert_eq!(imported(&store).unwrap().texts, 9);
        assert_eq!(imported(&store).unwrap().photos, 0);
        // Run again: no text twice, and the photos are filled in.
        let mut sink = FakeSink::default();
        let s = run(&store, &data, all(), &mut sink);
        assert_eq!(s.texts, 0);
        assert_eq!(s.already, 9);
        assert_eq!(s.photos, 2);
        assert_eq!(store.recent_messages(100).unwrap().len(), 9);
        // And once more: nothing at all.
        let mut sink = FakeSink::default();
        let s = run(&store, &data, all(), &mut sink);
        assert_eq!((s.texts, s.photos), (0, 0));
        assert!(sink.stored.is_empty());
        let scan = scan(&store, &data, &|_| Some(10)).unwrap();
        assert_eq!((scan.texts, scan.photos, scan.videos, scan.already), (0, 0, 0, 9));
    }

    #[test]
    fn cancelling_between_batches_keeps_what_was_written() {
        let store = Store::in_memory().unwrap();
        let data = data();
        let r = import(
            &store,
            &data,
            all(),
            Some(T0),
            &mut FakeSink::default(),
            &mut |_, _, _| false,
        );
        assert!(matches!(r, Err(ImportError::Cancelled)));
        let first = imported(&store).unwrap().texts;
        assert!(first > 0, "the first batch is kept");
        let s = run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!(s.texts + first, 9);
    }

    #[test]
    fn texts_tug_has_from_bluetooth_are_not_added_twice() {
        let store = Store::in_memory().unwrap();
        // Jane's text arrived live over Bluetooth, in her national format, a few seconds late;
        // the photo came as Bluetooth's words; tug's own reply went out from tug.
        store
            .insert_incoming(&IncomingMessage {
                source: SOURCE_IPHONE_MAP,
                handle: "H1",
                address: "5550100001",
                sender_name: None,
                body: "dinner at 7?",
                sent_at: Some("2025-10-09T04:53:20"),
                received_at: T0 + 4_000,
                unread_on_phone: false,
                msg_type: None,
            })
            .unwrap();
        store
            .insert_incoming(&IncomingMessage {
                source: SOURCE_IPHONE_MAP,
                handle: "H2",
                address: "5550100001",
                sender_name: None,
                body: "Attachment: 2 Photos",
                // Caught up hours later: only the phone's time (local, 4 hours behind UTC) matches.
                sent_at: Some("2025-10-09T04:56:20"),
                received_at: T0 + 6 * 3_600_000,
                unread_on_phone: false,
                msg_type: None,
            })
            .unwrap();
        let out = store
            .insert_outgoing(SOURCE_IPHONE_MAP, "5550100001", "yes! see you there", T0 + 59_000)
            .unwrap();
        store
            .set_outgoing_status(out.id, crate::messages::Status::Accepted, None)
            .unwrap();
        // Sam's group text came over Bluetooth as a one-to-one text.
        store
            .insert_incoming(&IncomingMessage {
                source: SOURCE_IPHONE_MAP,
                handle: "H3",
                address: "+15550100002",
                sender_name: None,
                body: "who's driving?",
                sent_at: None,
                received_at: T0 + 400_500,
                unread_on_phone: false,
                msg_type: None,
            })
            .unwrap();
        let data = data();
        let scan = scan(&store, &data, &|_| Some(10)).unwrap();
        assert_eq!((scan.texts, scan.already), (5, 4));
        let s = run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!((s.texts, s.already), (5, 4));
        let msgs = store.recent_messages(100).unwrap();
        assert_eq!(msgs.len(), 9, "four from Bluetooth, five new");
        // Each person's texts are in one conversation, under the address tug already used.
        assert!(msgs
            .iter()
            .filter(|m| m.group_id.is_none())
            .all(|m| m.address == "5550100001"));
        let photo = msgs.iter().find(|m| m.body == "Attachment: 2 Photos").unwrap();
        assert_eq!(photo.source, SOURCE_IPHONE_MAP, "the Bluetooth copy stays");
        assert_eq!(photo.attachments.len(), 2, "and gets the backup's photos");
        let sam = msgs.iter().find(|m| m.body == "who's driving?").unwrap();
        assert_eq!(
            sam.group_id.as_deref(),
            Some(group_id("G-CREW").as_str()),
            "moved into its group"
        );
        assert_eq!(
            store
                .conversation_messages(&["+15550100002".into()], None, 10)
                .unwrap()
                .len(),
            0
        );

        // Remove imported history: the Bluetooth texts are exactly as they were.
        assert_eq!(remove_imported(&store).unwrap(), 5);
        let left = store.recent_messages(100).unwrap();
        assert_eq!(left.len(), 4);
        assert!(left
            .iter()
            .all(|m| m.source == SOURCE_IPHONE_MAP && m.attachments.is_empty() && m.group_id.is_none()));
        assert!(store.message_groups().unwrap().is_empty());
        assert_eq!(imported(&store).unwrap(), Imported::default());
        assert!(file_keys(&store).unwrap().is_empty());
    }

    #[test]
    fn the_same_words_far_apart_are_two_texts() {
        let store = Store::in_memory().unwrap();
        store
            .insert_incoming(&IncomingMessage {
                source: SOURCE_IPHONE_MAP,
                handle: "H1",
                address: "+15550100001",
                sender_name: None,
                body: "dinner at 7?",
                sent_at: Some("2025-10-10T04:53:20Z"),
                received_at: T0 + 86_400_000,
                unread_on_phone: false,
                msg_type: None,
            })
            .unwrap();
        let s = run(&store, &data(), all(), &mut FakeSink::default());
        assert_eq!(s.already, 0, "a day apart, with the zone known");
    }

    #[test]
    fn photos_can_be_skipped_and_added_later() {
        let store = Store::in_memory().unwrap();
        let data = data();
        let s = run(
            &store,
            &data,
            Options {
                texts: true,
                photos: false,
            },
            &mut FakeSink::default(),
        );
        assert_eq!((s.texts, s.photos), (9, 0));
        let photo = store
            .recent_messages(100)
            .unwrap()
            .into_iter()
            .find(|m| !m.attachments.is_empty())
            .unwrap();
        assert!(
            photo.attachments.iter().all(|a| !a.stored),
            "the kind is known, the file isn't here"
        );
        let scan = scan(&store, &data, &|_| Some(1000)).unwrap();
        assert_eq!(
            (scan.texts, scan.photos, scan.videos, scan.media_bytes),
            (0, 1, 1, 2000),
            "still to copy"
        );
        // Photos only, later: nothing new in texts, the photos come in.
        let s = run(
            &store,
            &data,
            Options {
                texts: false,
                photos: true,
            },
            &mut FakeSink::default(),
        );
        assert_eq!((s.texts, s.photos), (0, 2));
        let photo = store
            .recent_messages(100)
            .unwrap()
            .into_iter()
            .find(|m| !m.attachments.is_empty())
            .unwrap();
        assert!(photo.attachments.iter().all(|a| a.stored));
    }

    #[test]
    fn photos_only_brings_their_texts_and_counts_missing_files() {
        let store = Store::in_memory().unwrap();
        let data = data();
        let mut sink = FakeSink::default();
        sink.missing
            .insert("Library/SMS/Attachments/cd/22/ATT-2/clip.MOV".into());
        let s = run(
            &store,
            &data,
            Options {
                texts: false,
                photos: true,
            },
            &mut sink,
        );
        assert_eq!(s.texts, 1, "only the text the photos came in");
        assert_eq!((s.photos, s.missing), (1, 1));
    }

    #[test]
    fn scan_counts_what_would_come_in() {
        let store = Store::in_memory().unwrap();
        let data = data();
        let sizes = |p: &str| if p.ends_with(".MOV") { Some(4096) } else { Some(2048) };
        let s = scan(&store, &data, &sizes).unwrap();
        assert_eq!(s.texts, 9);
        assert_eq!(s.conversations, 2);
        assert_eq!((s.photos, s.videos, s.media_bytes), (1, 1, 6144));
        assert_eq!(s.photo_texts, 1);
        // A photo the backup doesn't have isn't counted.
        let s = scan(&store, &data, &|p: &str| p.ends_with(".MOV").then_some(4096)).unwrap();
        assert_eq!((s.photos, s.videos), (0, 1));
    }

    #[test]
    fn dates_and_iso_round_trip() {
        assert_eq!(iso_utc(T0), "2025-10-09T08:53:20Z");
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(parse_iso_ms("2025-10-09T08:53:20Z"), Some((T0, true)));
        assert_eq!(parse_iso_ms("2025-10-09T08:53:20"), Some((T0, false)));
        assert_eq!(parse_iso_ms("2025-10-09T08:53:20.25Z"), Some((T0 + 250, true)));
        assert_eq!(parse_iso_ms("2025-13-09T08:53:20"), None);
        assert_eq!(parse_iso_ms("yesterday"), None);
        for ms in [0, T0, 978_307_200_000, 4_102_444_800_000] {
            assert_eq!(parse_iso_ms(&iso_utc(ms)), Some((ms, true)));
        }
    }

    #[test]
    fn bluetooth_photo_words_are_not_part_of_a_text() {
        assert_eq!(compare_body("Attachment: 1 Photo"), "");
        assert_eq!(compare_body("look at this\nAttachment: 2 Photos"), "look at this");
        assert_eq!(compare_body("Attachments: 1 Audio Message"), "");
        assert_eq!(compare_body("Attachment: 2 are below"), "Attachment: 2 are below");
        assert_eq!(compare_body("  ok\r\n"), "ok");
        assert!(!is_attachment_line("attachment"));
    }

    #[test]
    fn group_ids_are_stable_and_opaque() {
        assert_eq!(group_id("G-CREW"), group_id("G-CREW"));
        assert_ne!(group_id("G-CREW"), group_id("G-OTHER"));
        assert_eq!(group_id("x").len(), 17);
        assert!(!group_id("chat123456").contains("chat"));
    }
}
