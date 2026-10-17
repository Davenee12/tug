//! Bringing a backup's messages into tug's history: deciding what's new, merging with the texts
//! tug already has from Bluetooth, and writing it in resumable batches.
//!
//! - Every imported text is a `messages` row with source `iphone-backup`, keyed by the backup's
//!   message guid, so running the import again (after a cancel, a crash or a newer backup) adds
//!   only what isn't there yet.
//! - Only one-to-one conversations come in: Messages in tug holds the conversations it can reply
//!   to, and iOS gives Bluetooth no way to send to a group, so group chats stay on the iPhone
//!   (counted, so the wizard can say so).
//! - A text tug already has from Bluetooth isn't added twice: same direction, same conversation
//!   (the person, by `store::match_key`), same words, and close in time: tug got it within minutes
//!   of the backup's time, or the phone's own time for it is within a clock's slack, in UTC. A
//!   phone time without a zone is turned into UTC with an offset tug knows (`clock`), never guessed;
//!   with none known it isn't compared. Each Bluetooth text pairs with one backup text at most, and
//!   one that happened after the backup was made can't be in it. The Bluetooth copy stays; the
//!   backup only adds what it lacked (the photos of a text Bluetooth showed as "Attachment: 1
//!   Photo").
//! - The other way round (Bluetooth lists a text after the import that the backup already has), the
//!   Bluetooth copy is kept but hidden behind the backup's row (`dup_of`), never thrown away, so
//!   Remove imported history brings it back.
//! - People are tug's: a number the backup writes as `+447700900123` joins the conversation tug
//!   already has under `07700 900123` (the same last 10 digits), and contacts' names apply as usual.
//! - Photos and videos are copied in a second pass, through a `FileSink`, so they can be skipped
//!   and filled in later.
//!
//! Nothing here logs a word of any message, a name or a number.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::clock;
use super::smsdb::{Message, Service, SmsData};
use crate::messages::SOURCE_IPHONE_BACKUP;
use crate::store::{match_key, Result, Store};

/// Settings key: when the history in tug was brought in, and from a backup made when (JSON).
pub const IMPORT_RECORD: &str = "backup_import";

/// A Bluetooth text tug got within this of the backup's time is the same text (tug saw it live).
const LIVE_TOLERANCE_MS: i64 = 10 * 60 * 1000;
/// The phone's own time for a Bluetooth text, in UTC (it carried a zone, or `clock` turned it into
/// UTC with a known offset): a clock's worth of slack. Also how far past the backup's date a text
/// may be and still be in it.
const ZONED_TOLERANCE_MS: i64 = 2 * 60 * 1000;

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
    /// Texts in group chats, which aren't brought in.
    pub group_texts: u64,
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub texts: u64,
    /// Pictures (photos, GIFs, stickers) and videos copied to this PC.
    pub photos: u64,
    pub videos: u64,
    pub conversations: u64,
    /// Texts tug already had.
    pub already: u64,
    /// Photos the backup listed but didn't have the file for.
    pub missing: u64,
    /// Texts in group chats, left out.
    pub group_texts: u64,
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
    /// A new row, in the conversation with this address.
    New { address: String },
    /// Imported before (that row).
    Imported(i64),
    /// The same text tug has from Bluetooth (that row).
    Bluetooth { id: i64 },
    /// In a group chat: left out (tug can't reply to a group).
    Group,
    /// Nobody to put it with (no handle, no chat).
    Skip,
}

struct Plan<'a> {
    items: Vec<(&'a Message, Target)>,
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
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
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

/// A Bluetooth text's phone time in UTC: as stored when it carries a zone, else turned into UTC
/// with an offset tug knows (`clock`), else none (a zoneless time is never guessed at).
fn phone_time(sent_at: Option<&str>, zone: clock::Resolver) -> Option<i64> {
    let (ms, zoned) = parse_iso_ms(sent_at?)?;
    if zoned {
        Some(ms)
    } else {
        zone.to_utc(ms)
    }
}

/// A Bluetooth text that could be the same as a backup's.
struct Candidate {
    id: i64,
    received_at: i64,
    /// The phone's time for it, in UTC, when known (`phone_time`).
    sent_at: Option<i64>,
}

impl Candidate {
    /// When it happened, as well as tug knows: the phone's time, else when tug got it.
    fn time(&self) -> i64 {
        self.sent_at.unwrap_or(self.received_at)
    }

    /// How close it is to `at` (smaller is better), if close enough to be the same text at all.
    fn distance(&self, at: i64) -> Option<i64> {
        let live = (self.received_at - at).abs();
        if live <= LIVE_TOLERANCE_MS {
            return Some(live);
        }
        let d = (self.sent_at? - at).abs();
        (d <= ZONED_TOLERANCE_MS).then_some(d)
    }

    /// Whether a backup made at `backup_date` could hold it: not if it happened after (beyond a
    /// clock's slack). An unknown date rules nothing out.
    fn before(&self, backup_date: Option<i64>) -> bool {
        backup_date.is_none_or(|d| self.time() <= d.saturating_add(ZONED_TOLERANCE_MS))
    }
}

/// The import record (`IMPORT_RECORD`): `None` when nothing was ever imported, else the newest
/// backup's date, if known.
fn import_record(conn: &rusqlite::Connection) -> Result<Option<Option<i64>>> {
    let json: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [IMPORT_RECORD], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(json.map(|json| {
        serde_json::from_str::<serde_json::Value>(&json)
            .ok()
            .and_then(|v| v.get("backupDate").and_then(serde_json::Value::as_i64))
    }))
}

/// The imported row that already holds this incoming Bluetooth text (same conversation, same
/// words, close in time, by the same rules the import uses the other way round), if any. A row
/// stands in for one Bluetooth text at most. Uses the partial index on incoming bodies, so it
/// costs a lookup, not a scan.
pub(crate) fn backup_has(
    conn: &rusqlite::Connection,
    zone: clock::Resolver,
    address: &str,
    body: &str,
    sent_at: Option<&str>,
    received_at: i64,
) -> Result<Option<i64>> {
    let words = compare_body(body);
    // Only a photo's Bluetooth words ("Attachment: 1 Photo"), or nothing: too little to tell one
    // text from another, so it's never taken for the backup's.
    if words.is_empty() {
        return Ok(None);
    }
    let incoming = Candidate {
        id: 0,
        received_at,
        sent_at: phone_time(sent_at, zone),
    };
    // Nothing imported, or newer than the newest backup brought in: the backup can't have it.
    match import_record(conn)? {
        None => return Ok(None),
        Some(date) if !incoming.before(date) => return Ok(None),
        Some(_) => {}
    }
    let key = person_key(address);
    let mut stmt = conn.prepare_cached(
        "SELECT m.id, m.address, m.received_at FROM messages m
         WHERE m.direction = 'in' AND m.body IN (?1, ?2) AND m.source = ?3 AND m.hidden_at IS NULL
           AND m.unsent = 0 AND NOT EXISTS (SELECT 1 FROM messages d WHERE d.dup_of = m.id)",
    )?;
    let rows = stmt.query_map(params![body, words, SOURCE_IPHONE_BACKUP], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
    })?;
    let mut best: Option<(i64, i64)> = None;
    for row in rows {
        let (id, a, at) = row?;
        if person_key(&a) != key {
            continue;
        }
        if let Some(d) = incoming.distance(at) {
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, id));
            }
        }
    }
    Ok(best.map(|(_, id)| id))
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

/// tug's people and Bluetooth texts, with each text's phone time read in UTC (`zone`), leaving out
/// any that happened after the backup was made.
fn load_known(store: &Store, zone: clock::Resolver, backup_date: Option<i64>) -> Result<Known> {
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
            // A send that failed (or never went) isn't the phone's text, even with the same words.
            "SELECT id, direction, address, body, sent_at, received_at FROM messages
             WHERE source <> ?1 AND hidden_at IS NULL AND address <> '' AND status NOT IN ('failed', 'pending')",
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
            let candidate = Candidate {
                id,
                received_at,
                sent_at: phone_time(sent_at.as_deref(), zone),
            };
            if !candidate.before(backup_date) {
                continue;
            }
            let key = (direction == "out", person_key(&address), compare_body(&body));
            texts.entry(key).or_default().push(candidate);
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

/// How each of the backup's messages lands. Group chats are left out; a one-to-one text is matched
/// only with Bluetooth texts in the same conversation.
fn plan<'a>(store: &Store, data: &'a SmsData, backup_date: Option<i64>) -> Result<Plan<'a>> {
    let known = load_known(store, store.local_zone().resolver(), backup_date)?;
    let mut used: HashSet<i64> = HashSet::new();
    let mut items = Vec::with_capacity(data.messages.len());
    for m in &data.messages {
        let chat = m.chat.and_then(|c| data.chats.get(&c));
        if chat.is_some_and(|c| c.is_group) {
            items.push((m, Target::Group));
            continue;
        }
        let address = m
            .handle
            .as_deref()
            .or_else(|| chat.and_then(|c| c.members.first()).map(String::as_str))
            .map(|h| resolve(&known, h));
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
                    Target::Bluetooth { id }
                }
                None => Target::New { address },
            }
        } else {
            Target::Skip
        };
        items.push((m, target));
    }
    Ok(Plan { items })
}

fn wanted(m: &Message, opts: Options) -> bool {
    opts.texts || (opts.photos && m.attachments.iter().any(|a| a.ext.is_some()))
}

fn conversation_key(target: &Target) -> Option<String> {
    match target {
        Target::New { address } => Some(person_key(address)),
        _ => None,
    }
}

/// What importing would add, against what tug has now.
pub fn scan(
    store: &Store,
    data: &SmsData,
    backup_date: Option<i64>,
    sizes: &dyn Fn(&str) -> Option<u64>,
) -> Result<Scan> {
    let plan = plan(store, data, backup_date)?;
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
            Target::Group => scan.group_texts += 1,
            Target::Skip => {}
        }
        if matches!(target, Target::Skip | Target::Group) {
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
    let plan = plan(store, data, backup_date)?;
    let mut summary = Summary {
        group_texts: plan.items.iter().filter(|(_, t)| *t == Target::Group).count() as u64,
        ..Summary::default()
    };
    // The newest backup brought in so far: history is in tug up to its date (the label says so,
    // and a Bluetooth text after it can't be a backup's).
    let previous = import_record(&store.conn())?.flatten();
    let newest = match (previous, backup_date) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => b.or(a),
    };
    let record = serde_json::json!({ "backupDate": newest, "importedAt": crate::state::now_ms() });
    store.set_setting(IMPORT_RECORD, &record.to_string())?;

    let work: Vec<&(&Message, Target)> = plan
        .items
        .iter()
        .filter(|(m, t)| !matches!(t, Target::Skip | Target::Group) && wanted(m, opts))
        .collect();
    let total = work.len() as u64;
    let mut convs = HashSet::new();
    for (done, batch) in work.chunks(BATCH).enumerate() {
        let mut conn = store.conn();
        let tx = conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO messages (source, handle, direction, address, body, sent_at, received_at,
                     status, msg_type, edited, unsent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            let mut attach = tx.prepare(
                "INSERT OR IGNORE INTO message_attachments (message_id, source, source_key, kind, ext, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (m, target) in batch.iter().map(|x| (&x.0, &x.1)) {
                let row = match target {
                    Target::New { address } => {
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
                    Target::Bluetooth { id } => {
                        summary.already += 1;
                        *id
                    }
                    Target::Group | Target::Skip => continue,
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
                    if a.kind == super::smsdb::AttachmentKind::Video {
                        summary.videos += 1;
                    } else {
                        summary.photos += 1;
                    }
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

/// Remove imported history: every imported text, every attachment row (on Bluetooth texts too)
/// and the record. A Bluetooth text hidden behind an imported one (`dup_of`) comes back, hidden
/// only if that imported text was (its conversation deleted). Returns how many texts went. The
/// caller deletes the files.
pub fn remove_imported(store: &Store) -> Result<u64> {
    let mut conn = store.conn();
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE messages SET hidden_at = (SELECT b.hidden_at FROM messages b WHERE b.id = messages.dup_of),
             dup_of = NULL
         WHERE dup_of IS NOT NULL",
        [],
    )?;
    let n = tx.execute("DELETE FROM messages WHERE source = ?1", [SOURCE_IPHONE_BACKUP])?;
    tx.execute(
        "DELETE FROM message_attachments WHERE source = ?1",
        [SOURCE_IPHONE_BACKUP],
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
    use crate::backup::clock::LocalZone;
    use crate::backup::smsdb::{self, tests::sample, Chat};
    use crate::messages::{IncomingMessage, SOURCE_IPHONE_MAP};
    use rusqlite::Connection;

    const T0: i64 = 1_760_000_000_000;
    const H: i64 = 3_600_000;
    /// When the sample backup was made: a day after its texts.
    const BACKUP: i64 = T0 + 24 * H;

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
        import(store, data, opts, Some(BACKUP), sink, &mut |_, _, _| true).unwrap()
    }

    #[test]
    fn imports_texts_both_ways_with_photos_and_edits() {
        let store = Store::in_memory().unwrap();
        store
            .save_phonebook(&[("+15550100001".into(), "Jane Doe".into())])
            .unwrap();
        let data = data();
        let mut sink = FakeSink::default();
        let s = run(&store, &data, all(), &mut sink);
        assert_eq!(s.texts, 6);
        assert_eq!((s.photos, s.videos), (1, 1));
        assert_eq!(s.conversations, 1, "Jane's");
        assert_eq!(s.already, 0);
        assert_eq!(s.group_texts, 3, "the group chat stays on the phone");

        let msgs = store.recent_messages(100).unwrap();
        let jane: Vec<_> = msgs.iter().collect();
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
        // Searchable like any text.
        assert_eq!(store.search_messages("corner", 10).unwrap().len(), 1);

        let state = imported(&store).unwrap();
        assert_eq!((state.texts, state.photos, state.backup_date), (6, 2, Some(BACKUP)));
    }

    #[test]
    fn group_chats_are_not_imported() {
        let store = Store::in_memory().unwrap();
        // Sam's text from the group reached tug over Bluetooth, as Bluetooth shows it: from Sam.
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
        let scan = scan(&store, &data, Some(BACKUP), &|_| Some(10)).unwrap();
        assert_eq!((scan.texts, scan.group_texts, scan.conversations), (6, 3, 1));
        let s = run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!((s.texts, s.group_texts, s.already), (6, 3, 0));
        let msgs = store.recent_messages(100).unwrap();
        for words in ["I can", "great, thanks"] {
            assert!(msgs.iter().all(|m| m.body != words), "{words} stays on the phone");
        }
        // No conversation with Alex (only ever in the group), and Sam's Bluetooth text is untouched.
        assert!(msgs.iter().all(|m| m.address != "alex@example.com"));
        let sam: Vec<_> = msgs.iter().filter(|m| m.address == "+15550100002").collect();
        assert_eq!(sam.len(), 1);
        assert_eq!(sam[0].source, SOURCE_IPHONE_MAP);
        assert!(sam[0].attachments.is_empty());
        // Imported again: still nothing from the group.
        let s = run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!((s.texts, s.group_texts), (0, 3));
        assert_eq!(imported(&store).unwrap().texts, 6);
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
        let r = import(&store, &data, all(), Some(BACKUP), &mut sink, &mut |_, _, _| true);
        assert!(matches!(r, Err(ImportError::Cancelled)));
        assert_eq!(imported(&store).unwrap().texts, 6);
        assert_eq!(imported(&store).unwrap().photos, 0);
        // Run again: no text twice, and the photos are filled in.
        let mut sink = FakeSink::default();
        let s = run(&store, &data, all(), &mut sink);
        assert_eq!(s.texts, 0);
        assert_eq!(s.already, 6);
        assert_eq!((s.photos, s.videos), (1, 1));
        assert_eq!(store.recent_messages(100).unwrap().len(), 6);
        // And once more: nothing at all.
        let mut sink = FakeSink::default();
        let s = run(&store, &data, all(), &mut sink);
        assert_eq!((s.texts, s.photos), (0, 0));
        assert!(sink.stored.is_empty());
        let scan = scan(&store, &data, Some(BACKUP), &|_| Some(10)).unwrap();
        assert_eq!((scan.texts, scan.photos, scan.videos, scan.already), (0, 0, 0, 6));
    }

    #[test]
    fn cancelling_between_batches_keeps_what_was_written() {
        let store = Store::in_memory().unwrap();
        let data = data();
        let r = import(
            &store,
            &data,
            all(),
            Some(BACKUP),
            &mut FakeSink::default(),
            &mut |_, _, _| false,
        );
        assert!(matches!(r, Err(ImportError::Cancelled)));
        let first = imported(&store).unwrap().texts;
        assert!(first > 0, "the first batch is kept");
        let s = run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!(s.texts + first, 6);
    }

    #[test]
    fn texts_tug_has_from_bluetooth_are_not_added_twice() {
        let store = Store::in_memory().unwrap();
        // The phone is four hours behind UTC, and tug knows it.
        store.set_local_zone(LocalZone::Fixed(-240));
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
        let data = data();
        let scan = scan(&store, &data, Some(BACKUP), &|_| Some(10)).unwrap();
        assert_eq!((scan.texts, scan.already), (3, 3));
        let s = run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!((s.texts, s.already), (3, 3));
        let msgs = store.recent_messages(100).unwrap();
        assert_eq!(msgs.len(), 6, "three from Bluetooth, three new");
        // Jane's texts are in one conversation, under the address tug already used.
        assert!(msgs.iter().all(|m| m.address == "5550100001"));
        let photo = msgs.iter().find(|m| m.body == "Attachment: 2 Photos").unwrap();
        assert_eq!(photo.source, SOURCE_IPHONE_MAP, "the Bluetooth copy stays");
        assert_eq!(photo.attachments.len(), 2, "and gets the backup's photos");

        // Remove imported history: the Bluetooth texts are exactly as they were.
        assert_eq!(remove_imported(&store).unwrap(), 3);
        let left = store.recent_messages(100).unwrap();
        assert_eq!(left.len(), 3);
        assert!(left
            .iter()
            .all(|m| m.source == SOURCE_IPHONE_MAP && m.attachments.is_empty()));
        assert_eq!(imported(&store).unwrap(), Imported::default());
        assert!(file_keys(&store).unwrap().is_empty());
    }

    #[test]
    fn bluetooth_listing_a_text_after_the_import_does_not_add_it_twice() {
        let store = Store::in_memory().unwrap();
        store.set_local_zone(LocalZone::Fixed(-240));
        run(&store, &data(), all(), &mut FakeSink::default());
        let catch_up = |handle, body, sent_at, received_at| IncomingMessage {
            source: SOURCE_IPHONE_MAP,
            handle,
            address: "5550100001",
            sender_name: None,
            body,
            sent_at,
            received_at,
            unread_on_phone: true,
            msg_type: None,
        };
        // The phone lists Jane's text hours later (message access was off), in local time.
        let late = catch_up("H7", "dinner at 7?", Some("2025-10-09T04:53:20"), T0 + 5 * 3_600_000);
        assert!(
            store.insert_incoming(&late).unwrap().is_none(),
            "the backup already has it"
        );
        // Her photo, as Bluetooth words, has no words to compare: it's never taken for another
        // text, even at the same moment.
        let photo = catch_up("H8", "Attachment: 2 Photos", Some("2025-10-09T08:56:20Z"), T0 + 400_000);
        assert!(store.insert_incoming(&photo).unwrap().is_some());
        // A new text with the same words a day later is new.
        let again = catch_up("H9", "dinner at 7?", Some("2025-10-10T08:53:20Z"), T0 + 86_400_000);
        assert!(store.insert_incoming(&again).unwrap().is_some());
        assert_eq!(store.recent_messages(100).unwrap().len(), 8);
    }

    #[test]
    fn a_failed_send_from_tug_is_not_the_phones_text() {
        let store = Store::in_memory().unwrap();
        let out = store
            .insert_outgoing(SOURCE_IPHONE_MAP, "+15550100001", "yes! see you there", T0 + 59_000)
            .unwrap();
        store
            .set_outgoing_status(out.id, crate::messages::Status::Failed, None)
            .unwrap();
        let s = run(&store, &data(), all(), &mut FakeSink::default());
        assert_eq!(s.already, 0, "the backup's real sent text comes in");
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

    /// One "ok" from Jane at `at`, in a one-to-one chat.
    fn one_ok(at: i64) -> SmsData {
        let mut data = SmsData::default();
        data.chats.insert(
            1,
            Chat {
                id: 1,
                members: vec!["+15550100001".into()],
                is_group: false,
            },
        );
        data.messages.push(Message {
            guid: "GUID-OK".into(),
            chat: Some(1),
            from_me: false,
            handle: Some("+15550100001".into()),
            body: "ok".into(),
            at_ms: at,
            service: Service::IMessage,
            edited: false,
            unsent: false,
            attachments: Vec::new(),
        });
        data
    }

    /// An incoming Bluetooth "ok" from Jane, with the phone's zoneless local time for `utc` (the
    /// phone four hours behind UTC), got by tug at `received_at`.
    fn bluetooth_ok(handle: &'static str, local: &'static str, received_at: i64) -> IncomingMessage<'static> {
        IncomingMessage {
            source: SOURCE_IPHONE_MAP,
            handle,
            address: "+15550100001",
            sender_name: None,
            body: "ok",
            sent_at: Some(local),
            received_at,
            unread_on_phone: false,
            msg_type: None,
        }
    }

    #[test]
    fn a_repeated_ok_hours_apart_is_two_texts_bluetooth_first() {
        // Bluetooth has an "ok" from 3 hours after the backup's "ok", with no zone on its time.
        // Read as if UTC (off by the phone's 4 hours) it looked an hour from the backup's, and the
        // old 14-hour allowance merged them, so the backup's "ok" never came in.
        let store = Store::in_memory().unwrap();
        store.set_local_zone(LocalZone::Fixed(-240));
        store
            .insert_incoming(&bluetooth_ok("H1", "2025-10-09T07:53:20", T0 + 3 * H))
            .unwrap();
        let data = one_ok(T0);
        let s = scan(&store, &data, Some(BACKUP), &|_| None).unwrap();
        assert_eq!((s.texts, s.already), (1, 0), "two different texts");
        run(&store, &data, all(), &mut FakeSink::default());
        assert_eq!(store.recent_messages(10).unwrap().len(), 2);

        // The same "ok" caught up by Bluetooth hours later is the backup's own: matched.
        let store = Store::in_memory().unwrap();
        store.set_local_zone(LocalZone::Fixed(-240));
        store
            .insert_incoming(&bluetooth_ok("H1", "2025-10-09T04:53:20", T0 + 5 * H))
            .unwrap();
        let s = scan(&store, &data, Some(BACKUP), &|_| None).unwrap();
        assert_eq!((s.texts, s.already), (0, 1));

        // With the phone's offset unknown, a zoneless time isn't compared at all.
        let store = Store::in_memory().unwrap();
        store.set_local_zone(LocalZone::Unknown);
        store
            .insert_incoming(&bluetooth_ok("H1", "2025-10-09T04:53:20", T0 + 5 * H))
            .unwrap();
        let s = scan(&store, &data, Some(BACKUP), &|_| None).unwrap();
        assert_eq!((s.texts, s.already), (1, 0), "not guessed");
    }

    #[test]
    fn a_repeated_ok_hours_apart_is_two_texts_backup_first() {
        // The backup's "ok" is in; Jane sends "ok" again 3 hours later and Bluetooth lists it with
        // no zone on its time. It used to be dropped as the backup's.
        let store = Store::in_memory().unwrap();
        store.set_local_zone(LocalZone::Fixed(-240));
        run(&store, &one_ok(T0), all(), &mut FakeSink::default());
        let later = bluetooth_ok("H1", "2025-10-09T07:53:20", T0 + 3 * H);
        assert!(store.insert_incoming(&later).unwrap().is_some(), "a new text");
        // The backup's own "ok", caught up late, isn't added twice.
        let same = bluetooth_ok("H2", "2025-10-09T04:53:20", T0 + 5 * H);
        assert!(store.insert_incoming(&same).unwrap().is_none());
        assert_eq!(store.recent_messages(10).unwrap().len(), 2);

        // Offset unknown: never matched by a zoneless time.
        let store = Store::in_memory().unwrap();
        store.set_local_zone(LocalZone::Unknown);
        run(&store, &one_ok(T0), all(), &mut FakeSink::default());
        let same = bluetooth_ok("H2", "2025-10-09T04:53:20", T0 + 5 * H);
        assert!(store.insert_incoming(&same).unwrap().is_some());
    }

    /// Every row, hidden or not: (source, body, hidden, dup_of).
    fn all_rows(store: &Store) -> Vec<(String, String, bool, Option<i64>)> {
        let conn = store.conn();
        let mut stmt = conn
            .prepare("SELECT source, body, hidden_at IS NOT NULL, dup_of FROM messages ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn a_bluetooth_copy_of_an_imported_text_is_hidden_not_dropped() {
        let store = Store::in_memory().unwrap();
        run(&store, &one_ok(T0), all(), &mut FakeSink::default());
        let backup_row = store.recent_messages(10).unwrap()[0].id;
        // The phone lists the backup's "ok" seconds after it arrived: the same text.
        let copy = IncomingMessage {
            sent_at: Some("2025-10-09T08:53:21Z"),
            ..bluetooth_ok("H1", "", T0 + 3_000)
        };
        assert!(store.insert_incoming(&copy).unwrap().is_none(), "nothing new to show");
        assert_eq!(store.recent_messages(10).unwrap().len(), 1, "shown once");
        let rows = all_rows(&store);
        assert_eq!(rows.len(), 2, "kept, not thrown away");
        assert_eq!(
            rows[1],
            (SOURCE_IPHONE_MAP.to_string(), "ok".to_string(), true, Some(backup_row))
        );
        // Listed again: still one copy.
        assert!(store.insert_incoming(&copy).unwrap().is_none());
        assert_eq!(all_rows(&store).len(), 2);
        // A second "ok" moments later is a text of its own: the backup's row stands in for one.
        let second = IncomingMessage {
            handle: "H2",
            sent_at: Some("2025-10-09T08:53:29Z"),
            ..bluetooth_ok("H2", "", T0 + 9_000)
        };
        assert!(store.insert_incoming(&second).unwrap().is_some());

        // Remove imported history: the hidden Bluetooth copy comes back.
        assert_eq!(remove_imported(&store).unwrap(), 1);
        let left = store.recent_messages(10).unwrap();
        assert_eq!(left.len(), 2);
        assert!(left.iter().all(|m| m.source == SOURCE_IPHONE_MAP && m.body == "ok"));
        assert!(all_rows(&store).iter().all(|r| r.3.is_none()));
    }

    #[test]
    fn removing_imported_history_keeps_a_deleted_conversation_deleted() {
        let store = Store::in_memory().unwrap();
        run(&store, &one_ok(T0), all(), &mut FakeSink::default());
        let copy = IncomingMessage {
            sent_at: Some("2025-10-09T08:53:21Z"),
            ..bluetooth_ok("H1", "", T0 + 3_000)
        };
        assert!(store.insert_incoming(&copy).unwrap().is_none());
        store
            .set_conversation_hidden(&[], &["+15550100001".into()], T0 + H, true)
            .unwrap();
        remove_imported(&store).unwrap();
        assert!(store.recent_messages(10).unwrap().is_empty(), "still deleted");
        assert_eq!(all_rows(&store).len(), 1);
    }

    #[test]
    fn a_bluetooth_text_after_the_backup_was_made_is_never_the_backups() {
        // Backed up right after Jane's "ok"; 8 minutes later she sends "ok" again. Tug got it
        // within the 10 minutes that count as "seen live", but it happened after the backup.
        let store = Store::in_memory().unwrap();
        let data = one_ok(T0);
        import(
            &store,
            &data,
            all(),
            Some(T0),
            &mut FakeSink::default(),
            &mut |_, _, _| true,
        )
        .unwrap();
        let after = IncomingMessage {
            sent_at: Some("2025-10-09T09:01:20Z"),
            ..bluetooth_ok("H1", "", T0 + 8 * 60_000)
        };
        assert!(store.insert_incoming(&after).unwrap().is_some());

        // Bluetooth first, the other way round: the later "ok" isn't taken for the backup's.
        let store = Store::in_memory().unwrap();
        store.insert_incoming(&after).unwrap();
        let s = scan(&store, &data, Some(T0), &|_| None).unwrap();
        assert_eq!((s.texts, s.already), (1, 0));
    }

    #[test]
    fn unsent_and_wordless_texts_never_match() {
        let store = Store::in_memory().unwrap();
        let mut data = one_ok(T0);
        data.messages[0].body = String::new();
        data.messages[0].attachments = vec![smsdb::Attachment {
            guid: "ATT".into(),
            path: None,
            kind: smsdb::AttachmentKind::Photo,
            ext: Some("jpg"),
            bytes: None,
        }];
        run(&store, &data, all(), &mut FakeSink::default());
        // A photo text and Bluetooth's words for it, at the same moment: nothing to compare.
        let photo = IncomingMessage {
            body: "Attachment: 1 Photo",
            sent_at: Some("2025-10-09T08:53:20Z"),
            ..bluetooth_ok("H1", "", T0 + 2_000)
        };
        assert!(store.insert_incoming(&photo).unwrap().is_some());
        // An unsent text is never what a Bluetooth text matches, whatever its row holds.
        let store = Store::in_memory().unwrap();
        run(&store, &one_ok(T0), all(), &mut FakeSink::default());
        store.conn().execute("UPDATE messages SET unsent = 1", []).unwrap();
        let ok = IncomingMessage {
            sent_at: Some("2025-10-09T08:53:20Z"),
            ..bluetooth_ok("H1", "", T0 + 2_000)
        };
        assert!(store.insert_incoming(&ok).unwrap().is_some());
    }

    #[test]
    fn the_newest_backups_date_is_kept() {
        let store = Store::in_memory().unwrap();
        let data = one_ok(T0);
        import(
            &store,
            &data,
            all(),
            Some(BACKUP),
            &mut FakeSink::default(),
            &mut |_, _, _| true,
        )
        .unwrap();
        // An older backup brought in afterwards doesn't move the date back.
        import(
            &store,
            &data,
            all(),
            Some(T0),
            &mut FakeSink::default(),
            &mut |_, _, _| true,
        )
        .unwrap();
        assert_eq!(imported(&store).unwrap().backup_date, Some(BACKUP));
        import(&store, &data, all(), None, &mut FakeSink::default(), &mut |_, _, _| {
            true
        })
        .unwrap();
        assert_eq!(imported(&store).unwrap().backup_date, Some(BACKUP));
        import(
            &store,
            &data,
            all(),
            Some(BACKUP + H),
            &mut FakeSink::default(),
            &mut |_, _, _| true,
        )
        .unwrap();
        assert_eq!(imported(&store).unwrap().backup_date, Some(BACKUP + H));
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
        assert_eq!((s.texts, s.photos), (6, 0));
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
        let scan = scan(&store, &data, Some(BACKUP), &|_| Some(1000)).unwrap();
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
        assert_eq!((s.texts, s.photos, s.videos), (0, 1, 1));
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
        let s = scan(&store, &data, Some(BACKUP), &sizes).unwrap();
        assert_eq!(s.texts, 6);
        assert_eq!(s.conversations, 1);
        assert_eq!(s.group_texts, 3);
        assert_eq!((s.photos, s.videos, s.media_bytes), (1, 1, 6144));
        assert_eq!(s.photo_texts, 1);
        // A photo the backup doesn't have isn't counted.
        let s = scan(&store, &data, Some(BACKUP), &|p: &str| {
            p.ends_with(".MOV").then_some(4096)
        })
        .unwrap();
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
}
