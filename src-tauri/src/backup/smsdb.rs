//! Reading Messages' own database (`sms.db`) from a backup: chats, who's in them, every message
//! with its words and attachments. Pure over a `rusqlite::Connection`, so tests run against a tiny
//! made-up database.
//!
//! What iOS keeps, and how tug reads it:
//! - `message`: `text` (NULL on newer iOS, when the words are only in `attributedBody`, a
//!   typedstream), `is_from_me`, `date` (seconds since 2001-01-01 UTC, or nanoseconds from iOS 11),
//!   `service` (iMessage, SMS, RCS), `handle_id` (the other person: the sender of a received text,
//!   the recipient of a sent one in a one-to-one chat).
//! - Tapbacks (`associated_message_type` 2000–3999) are reactions to another message; group events
//!   (`item_type` ≠ 0: renamed, someone added or left) aren't messages. Both are skipped.
//! - Edited (`date_edited`) and unsent (`date_retracted`, or an edit that left nothing) messages.
//! - `chat` and `chat_handle_join`: a chat with more than one other person is a group, with its
//!   `display_name` if it has one. `chat_message_join` puts each message in its chat.
//! - `attachment` and `message_attachment_join`: each attachment's path on the phone
//!   (`~/Library/SMS/Attachments/…`), type, size and whether it's a sticker.
//!
//! Columns that only some iOS versions have are checked for first, so an older or newer
//! database still reads.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::typedstream;

/// Seconds between 1970-01-01 and Apple's epoch, 2001-01-01 (UTC).
const APPLE_EPOCH_UNIX_S: i64 = 978_307_200;

/// A message time in Apple's epoch to ms since 1970: seconds on older iOS, nanoseconds from iOS 11
/// (anything this large can only be nanoseconds: 10^11 seconds is over 3,000 years). 0 (or
/// negative) means not set.
pub fn apple_time_ms(raw: i64) -> Option<i64> {
    if raw <= 0 {
        return None;
    }
    let ms = if raw > 100_000_000_000 {
        raw / 1_000_000
    } else {
        raw.checked_mul(1000)?
    };
    ms.checked_add(APPLE_EPOCH_UNIX_S * 1000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    IMessage,
    Sms,
    Rcs,
    Other,
}

impl Service {
    fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("").to_ascii_lowercase().as_str() {
            "imessage" => Service::IMessage,
            "sms" | "mms" => Service::Sms,
            "rcs" => Service::Rcs,
            _ => Service::Other,
        }
    }
}

/// What kind of thing an attachment is, from its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachmentKind {
    Photo,
    Gif,
    Sticker,
    Video,
    Audio,
    Contact,
    Location,
    File,
}

impl AttachmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentKind::Photo => "photo",
            AttachmentKind::Gif => "gif",
            AttachmentKind::Sticker => "sticker",
            AttachmentKind::Video => "video",
            AttachmentKind::Audio => "audio",
            AttachmentKind::Contact => "contact",
            AttachmentKind::Location => "location",
            AttachmentKind::File => "file",
        }
    }

    /// Pictures and videos are copied to the PC; everything else stays a chip.
    pub fn is_media(self) -> bool {
        matches!(
            self,
            AttachmentKind::Photo | AttachmentKind::Gif | AttachmentKind::Sticker | AttachmentKind::Video
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub guid: String,
    /// Relative to the phone's home folder (`Library/SMS/Attachments/…`), as the backup's manifest
    /// names it. `None` when the phone kept no file (not downloaded, or deleted).
    pub path: Option<String>,
    pub kind: AttachmentKind,
    /// The file extension tug would store it under, from an allowlist (`None`: not stored).
    pub ext: Option<&'static str>,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chat {
    pub id: i64,
    /// Stable across the chat's rows for one group (iMessage and SMS halves share it).
    pub group_key: String,
    pub name: Option<String>,
    /// The other people in it (raw handles, as the phone stores them).
    pub members: Vec<String>,
    pub is_group: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub guid: String,
    pub chat: Option<i64>,
    pub from_me: bool,
    /// The other person's handle, when the message names one.
    pub handle: Option<String>,
    pub body: String,
    pub at_ms: i64,
    pub service: Service,
    pub edited: bool,
    pub unsent: bool,
    pub attachments: Vec<Attachment>,
}

/// Everything read from sms.db, plus counts of what was left out.
#[derive(Debug, Default)]
pub struct SmsData {
    pub chats: HashMap<i64, Chat>,
    /// Oldest first.
    pub messages: Vec<Message>,
    pub skipped_reactions: u64,
    pub skipped_events: u64,
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    rows.collect()
}

/// `column` if the table has it, else a constant in its place.
fn col(have: &HashSet<String>, column: &str, fallback: &str) -> String {
    if have.contains(column) {
        format!("m.{column}")
    } else {
        fallback.to_string()
    }
}

/// The text a message shows: `text`, or the string in `attributedBody`, without the object
/// replacement characters iOS puts where attachments go, trimmed.
pub fn message_body(text: Option<&str>, attributed: Option<&[u8]>) -> String {
    let clean = |s: &str| s.replace('\u{FFFC}', "").trim().to_string();
    let from_text = text.map(clean).unwrap_or_default();
    if !from_text.is_empty() {
        return from_text;
    }
    attributed
        .and_then(typedstream::extract_text)
        .map(|s| clean(&s))
        .unwrap_or_default()
}

/// What an attachment is and what it would be stored as, from its MIME type, UTI and file name.
pub fn classify(
    mime: Option<&str>,
    uti: Option<&str>,
    name: Option<&str>,
    sticker: bool,
) -> (AttachmentKind, Option<&'static str>) {
    let mime = mime.unwrap_or("").to_ascii_lowercase();
    let uti = uti.unwrap_or("").to_ascii_lowercase();
    let ext_from_name = name
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let ext = STORED_EXTENSIONS
        .iter()
        .find(|(e, mimes)| mimes.contains(&mime.as_str()) || *e == ext_from_name)
        .map(|(e, _)| *e);
    let kind = if sticker {
        AttachmentKind::Sticker
    } else if mime == "image/gif" || ext == Some("gif") {
        AttachmentKind::Gif
    } else if mime.starts_with("image/")
        || uti.starts_with("public.jpeg")
        || uti.contains("heic")
        || uti.contains("image")
    {
        AttachmentKind::Photo
    } else if mime.starts_with("video/") || uti.contains("movie") || uti.contains("video") {
        AttachmentKind::Video
    } else if mime.starts_with("audio/") || uti.contains("audio") || ext_from_name == "caf" || ext_from_name == "amr" {
        AttachmentKind::Audio
    } else if mime.contains("location") || name.is_some_and(|n| n.to_ascii_lowercase().ends_with(".loc.vcf")) {
        AttachmentKind::Location
    } else if mime == "text/vcard" || mime == "text/x-vcard" || ext_from_name == "vcf" {
        AttachmentKind::Contact
    } else if ext.is_some_and(|e| VIDEO_EXTENSIONS.contains(&e)) {
        AttachmentKind::Video
    } else if ext.is_some() {
        AttachmentKind::Photo
    } else {
        AttachmentKind::File
    };
    // Only pictures and videos are stored, and only under an extension from the list.
    (kind, if kind.is_media() { ext } else { None })
}

/// The only extensions tug stores attachments under (pictures and videos, nothing that runs),
/// with the MIME types that mean them.
const STORED_EXTENSIONS: &[(&str, &[&str])] = &[
    ("jpg", &["image/jpeg", "image/jpg"]),
    ("jpeg", &[]),
    ("png", &["image/png"]),
    ("gif", &["image/gif"]),
    ("heic", &["image/heic", "image/heic-sequence"]),
    ("heif", &["image/heif", "image/heif-sequence"]),
    ("webp", &["image/webp"]),
    ("mov", &["video/quicktime"]),
    ("mp4", &["video/mp4"]),
    ("m4v", &["video/x-m4v"]),
    ("3gp", &["video/3gpp"]),
];
const VIDEO_EXTENSIONS: &[&str] = &["mov", "mp4", "m4v", "3gp"];

/// The phone's path for an attachment (`~/Library/SMS/…` or `/var/mobile/Library/SMS/…`) as the
/// manifest's relative path, or `None` if it isn't under the Messages folder.
pub fn manifest_path(filename: &str) -> Option<String> {
    let rest = filename
        .strip_prefix("~/")
        .or_else(|| filename.strip_prefix("/var/mobile/"))
        .or_else(|| filename.strip_prefix("/private/var/mobile/"))
        .unwrap_or(filename);
    (rest.starts_with(super::manifest::ATTACHMENTS_PREFIX) && !rest.contains("..")).then(|| rest.to_string())
}

/// Read every chat and message.
pub fn read(conn: &Connection) -> rusqlite::Result<SmsData> {
    let mut data = SmsData::default();
    let handles: HashMap<i64, String> = {
        let mut stmt = conn.prepare("SELECT ROWID, id FROM handle")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    let chat_cols = columns(conn, "chat")?;
    let group_col = if chat_cols.contains("group_id") {
        "NULLIF(c.group_id, '')"
    } else {
        "NULL"
    };
    let style_col = if chat_cols.contains("style") { "c.style" } else { "0" };
    let mut members: HashMap<i64, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT chat_id, handle_id FROM chat_handle_join ORDER BY chat_id, handle_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (chat, handle) = row?;
            if let Some(h) = handles.get(&handle) {
                let list = members.entry(chat).or_default();
                if !list.contains(h) {
                    list.push(h.clone());
                }
            }
        }
    }
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT c.ROWID, c.guid, {group_col}, NULLIF(TRIM(c.display_name), ''), {style_col}, c.chat_identifier FROM chat c"
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        for row in rows {
            let (id, guid, group_id, name, style, identifier) = row?;
            let mut list = members.remove(&id).unwrap_or_default();
            if list.is_empty() {
                // A one-to-one chat whose join row is missing still names its person.
                if let Some(ident) = identifier.filter(|i| !i.starts_with("chat")) {
                    list.push(ident);
                }
            }
            let is_group = list.len() > 1 || style == Some(43);
            data.chats.insert(
                id,
                Chat {
                    id,
                    group_key: group_id.or(guid).unwrap_or_else(|| format!("chat-{id}")),
                    name,
                    members: list,
                    is_group,
                },
            );
        }
    }

    let chat_of: HashMap<i64, i64> = {
        let mut stmt = conn.prepare("SELECT message_id, MIN(chat_id) FROM chat_message_join GROUP BY message_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    let mut attachments: HashMap<i64, Vec<Attachment>> = HashMap::new();
    {
        let att_cols = columns(conn, "attachment")?;
        let a = |c: &str, fallback: &str| {
            if att_cols.contains(c) {
                format!("a.{c}")
            } else {
                fallback.to_string()
            }
        };
        let sql = format!(
            "SELECT j.message_id, a.guid, a.filename, {}, {}, {}, {}, {}
             FROM message_attachment_join j JOIN attachment a ON a.ROWID = j.attachment_id
             ORDER BY j.message_id, a.ROWID",
            a("mime_type", "NULL"),
            a("uti", "NULL"),
            a("transfer_name", "NULL"),
            a("total_bytes", "NULL"),
            a("is_sticker", "0"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })?;
        for row in rows {
            let (message, guid, filename, mime, uti, transfer, bytes, sticker) = row?;
            let name = transfer.as_deref().or(filename.as_deref());
            let (kind, ext) = classify(mime.as_deref(), uti.as_deref(), name, sticker.unwrap_or(0) != 0);
            attachments.entry(message).or_default().push(Attachment {
                guid: guid.unwrap_or_else(|| format!("att-{message}-{}", filename.as_deref().unwrap_or(""))),
                path: filename.as_deref().and_then(manifest_path),
                kind,
                ext,
                bytes: bytes.and_then(|b| u64::try_from(b).ok()).filter(|&b| b > 0),
            });
        }
    }

    let have = columns(conn, "message")?;
    let sql = format!(
        "SELECT m.ROWID, m.guid, m.text, {attributed}, m.handle_id, m.service, m.date, m.is_from_me,
                {assoc}, {item}, {edited}, {retracted}
         FROM message m ORDER BY m.date, m.ROWID",
        attributed = col(&have, "attributedBody", "NULL"),
        assoc = col(&have, "associated_message_type", "0"),
        item = col(&have, "item_type", "0"),
        edited = col(&have, "date_edited", "0"),
        retracted = col(&have, "date_retracted", "0"),
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<Vec<u8>>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<i64>>(8)?,
            r.get::<_, Option<i64>>(9)?,
            r.get::<_, Option<i64>>(10)?,
            r.get::<_, Option<i64>>(11)?,
        ))
    })?;
    for row in rows {
        let (rowid, guid, text, attributed, handle_id, service, date, from_me, assoc, item, edited, retracted) = row?;
        if (2000..4000).contains(&assoc.unwrap_or(0)) {
            data.skipped_reactions += 1;
            continue;
        }
        if item.unwrap_or(0) != 0 {
            data.skipped_events += 1;
            continue;
        }
        let Some(at_ms) = date.and_then(apple_time_ms) else {
            data.skipped_events += 1;
            continue;
        };
        let body = message_body(text.as_deref(), attributed.as_deref());
        let atts = attachments.remove(&rowid).unwrap_or_default();
        let edited = edited.unwrap_or(0) > 0;
        let unsent = retracted.unwrap_or(0) > 0 || (edited && body.is_empty() && atts.is_empty());
        if body.is_empty() && atts.is_empty() && !unsent {
            // Nothing to show (an app balloon tug can't draw, or an empty row).
            data.skipped_events += 1;
            continue;
        }
        data.messages.push(Message {
            guid: guid.unwrap_or_else(|| format!("msg-{rowid}")),
            chat: chat_of.get(&rowid).copied(),
            from_me: from_me.unwrap_or(0) != 0,
            handle: handle_id.filter(|&h| h > 0).and_then(|h| handles.get(&h).cloned()),
            body: if unsent { String::new() } else { body },
            at_ms,
            service: Service::parse(service.as_deref()),
            edited: edited && !unsent,
            unsent,
            attachments: if unsent { Vec::new() } else { atts },
        });
    }
    Ok(data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rusqlite::params;

    /// The tables tug reads, shaped like a recent iOS sms.db (only the columns that matter).
    pub(crate) const SCHEMA: &str = "
        CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT, service TEXT);
        CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT, style INTEGER, chat_identifier TEXT,
                           service_name TEXT, display_name TEXT, group_id TEXT);
        CREATE TABLE chat_handle_join (chat_id INTEGER, handle_id INTEGER);
        CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER, message_date INTEGER);
        CREATE TABLE message (ROWID INTEGER PRIMARY KEY, guid TEXT, text TEXT, attributedBody BLOB,
                              handle_id INTEGER, service TEXT, date INTEGER, is_from_me INTEGER,
                              associated_message_type INTEGER DEFAULT 0, associated_message_guid TEXT,
                              item_type INTEGER DEFAULT 0, date_edited INTEGER DEFAULT 0,
                              date_retracted INTEGER DEFAULT 0, cache_has_attachments INTEGER DEFAULT 0);
        CREATE TABLE attachment (ROWID INTEGER PRIMARY KEY, guid TEXT, filename TEXT, mime_type TEXT,
                                 uti TEXT, transfer_name TEXT, total_bytes INTEGER, is_sticker INTEGER DEFAULT 0);
        CREATE TABLE message_attachment_join (message_id INTEGER, attachment_id INTEGER);
    ";

    /// Seconds since 2001 as iOS 11+ stores them (nanoseconds).
    pub(crate) fn ns(unix_ms: i64) -> i64 {
        (unix_ms - APPLE_EPOCH_UNIX_S * 1000) * 1_000_000
    }

    /// A tiny made-up Messages database: Jane (one-to-one, both directions, a photo, an edit, a
    /// tapback, an attributedBody-only text), and a named group with Sam and Alex.
    pub(crate) fn sample(conn: &Connection) {
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch(
            "INSERT INTO handle VALUES (1, '+15550100001', 'iMessage'), (2, '+15550100002', 'iMessage'),
                                       (3, 'alex@example.com', 'iMessage');
             INSERT INTO chat VALUES (1, 'iMessage;-;+15550100001', 45, '+15550100001', 'iMessage', NULL, 'G-JANE'),
                                     (2, 'iMessage;+;chat123', 43, 'chat123', 'iMessage', 'Weekend crew', 'G-CREW');
             INSERT INTO chat_handle_join VALUES (1, 1), (2, 2), (2, 3);",
        )
        .unwrap();
        let t0 = 1_760_000_000_000i64; // 2025-10-09
        let msg = |rowid: i64,
                   chat: i64,
                   text: Option<&str>,
                   attributed: Option<Vec<u8>>,
                   handle: i64,
                   from_me: bool,
                   at: i64,
                   extra: &str| {
            conn.execute(
                "INSERT INTO message (ROWID, guid, text, attributedBody, handle_id, service, date, is_from_me) VALUES (?1, ?2, ?3, ?4, ?5, 'iMessage', ?6, ?7)",
                params![rowid, format!("GUID-{rowid}"), text, attributed, handle, ns(at), from_me],
            )
            .unwrap();
            if !extra.is_empty() {
                conn.execute(&format!("UPDATE message SET {extra} WHERE ROWID = {rowid}"), [])
                    .unwrap();
            }
            conn.execute("INSERT INTO chat_message_join VALUES (?1, ?2, 0)", params![chat, rowid])
                .unwrap();
        };
        msg(1, 1, Some("dinner at 7?"), None, 1, false, t0, "");
        msg(2, 1, Some("yes! see you there"), None, 1, true, t0 + 60_000, "");
        msg(
            3,
            1,
            None,
            Some(typedstream::tests::stream_for("running 5 min late")),
            1,
            false,
            t0 + 120_000,
            "",
        );
        msg(
            4,
            1,
            Some("Loved “yes! see you there”"),
            None,
            1,
            false,
            t0 + 130_000,
            "associated_message_type = 2000",
        );
        msg(
            5,
            1,
            Some("\u{FFFC}"),
            None,
            1,
            false,
            t0 + 180_000,
            "cache_has_attachments = 1",
        );
        msg(
            6,
            1,
            Some("meet at the corner"),
            None,
            1,
            true,
            t0 + 240_000,
            "date_edited = 1",
        );
        msg(
            7,
            1,
            None,
            None,
            1,
            true,
            t0 + 300_000,
            "date_edited = 1, date_retracted = 1",
        );
        msg(8, 2, Some("who's driving?"), None, 2, false, t0 + 400_000, "");
        msg(9, 2, Some("I can"), None, 3, false, t0 + 410_000, "");
        msg(10, 2, Some("great, thanks"), None, 0, true, t0 + 420_000, "");
        msg(11, 2, None, None, 2, false, t0 + 430_000, "item_type = 2");
        conn.execute_batch(
            "INSERT INTO attachment VALUES
                (1, 'ATT-1', '~/Library/SMS/Attachments/ab/11/ATT-1/IMG_0001.HEIC', 'image/heic', 'public.heic', 'IMG_0001.HEIC', 2048, 0),
                (2, 'ATT-2', '~/Library/SMS/Attachments/cd/22/ATT-2/clip.MOV', 'video/quicktime', 'com.apple.quicktime-movie', 'clip.MOV', 4096, 0);
             INSERT INTO message_attachment_join VALUES (5, 1), (5, 2);",
        )
        .unwrap();
    }

    #[test]
    fn apple_times_in_seconds_and_nanoseconds() {
        // 2025-10-09T08:53:20Z both ways.
        let unix_ms = 1_760_000_000_000;
        assert_eq!(apple_time_ms((unix_ms / 1000) - APPLE_EPOCH_UNIX_S), Some(unix_ms));
        assert_eq!(apple_time_ms(ns(unix_ms)), Some(unix_ms));
        // The epoch itself, one second after, and not set.
        assert_eq!(apple_time_ms(1), Some(APPLE_EPOCH_UNIX_S * 1000 + 1000));
        assert_eq!(apple_time_ms(0), None);
        assert_eq!(apple_time_ms(-5), None);
        // A 2013 text in seconds (iOS 7) isn't mistaken for nanoseconds.
        assert_eq!(
            apple_time_ms(400_000_000),
            Some((400_000_000 + APPLE_EPOCH_UNIX_S) * 1000)
        );
        assert_eq!(
            apple_time_ms(i64::MAX),
            Some(i64::MAX / 1_000_000 + APPLE_EPOCH_UNIX_S * 1000)
        );
    }

    #[test]
    fn reads_chats_messages_and_attachments() {
        let conn = Connection::open_in_memory().unwrap();
        sample(&conn);
        let data = read(&conn).unwrap();
        assert_eq!(data.skipped_reactions, 1);
        assert_eq!(data.skipped_events, 1, "the group rename");
        let bodies: Vec<&str> = data.messages.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(
            bodies,
            vec![
                "dinner at 7?",
                "yes! see you there",
                "running 5 min late",
                "",
                "meet at the corner",
                "",
                "who's driving?",
                "I can",
                "great, thanks"
            ]
        );
        let m = |guid: &str| data.messages.iter().find(|m| m.guid == guid).unwrap();
        assert!(m("GUID-2").from_me);
        assert_eq!(m("GUID-1").handle.as_deref(), Some("+15550100001"));
        assert_eq!(m("GUID-1").at_ms, 1_760_000_000_000);
        assert_eq!(m("GUID-1").service, Service::IMessage);
        assert!(m("GUID-6").edited && !m("GUID-6").unsent);
        assert!(m("GUID-7").unsent && !m("GUID-7").edited);
        let photo = &m("GUID-5").attachments;
        assert_eq!(photo.len(), 2);
        assert_eq!(photo[0].kind, AttachmentKind::Photo);
        assert_eq!(photo[0].ext, Some("heic"));
        assert_eq!(
            photo[0].path.as_deref(),
            Some("Library/SMS/Attachments/ab/11/ATT-1/IMG_0001.HEIC")
        );
        assert_eq!(photo[0].bytes, Some(2048));
        assert_eq!(photo[1].kind, AttachmentKind::Video);
        assert_eq!(photo[1].ext, Some("mov"));

        let crew = &data.chats[&2];
        assert!(crew.is_group);
        assert_eq!(crew.name.as_deref(), Some("Weekend crew"));
        assert_eq!(crew.members, vec!["+15550100002", "alex@example.com"]);
        assert_eq!(crew.group_key, "G-CREW");
        assert!(!data.chats[&1].is_group);
        assert_eq!(m("GUID-10").chat, Some(2));
        assert_eq!(m("GUID-10").handle, None, "a sent group text names no one person");
    }

    #[test]
    fn an_older_database_without_the_newer_columns_still_reads() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT);
             CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT, chat_identifier TEXT, display_name TEXT);
             CREATE TABLE chat_handle_join (chat_id INTEGER, handle_id INTEGER);
             CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);
             CREATE TABLE message (ROWID INTEGER PRIMARY KEY, guid TEXT, text TEXT, handle_id INTEGER,
                                   service TEXT, date INTEGER, is_from_me INTEGER);
             CREATE TABLE attachment (ROWID INTEGER PRIMARY KEY, guid TEXT, filename TEXT, mime_type TEXT);
             CREATE TABLE message_attachment_join (message_id INTEGER, attachment_id INTEGER);
             INSERT INTO handle VALUES (1, '+15550100001');
             INSERT INTO chat VALUES (1, 'SMS;-;+15550100001', '+15550100001', '');
             INSERT INTO chat_handle_join VALUES (1, 1);
             INSERT INTO message VALUES (1, 'G1', 'old school', 1, 'SMS', 400000000, 0);
             INSERT INTO chat_message_join VALUES (1, 1);",
        )
        .unwrap();
        let data = read(&conn).unwrap();
        assert_eq!(data.messages.len(), 1);
        assert_eq!(data.messages[0].service, Service::Sms);
        assert_eq!(data.messages[0].body, "old school");
        assert_eq!(data.chats[&1].name, None, "a blank name is no name");
    }

    #[test]
    fn classifies_attachments_and_stores_only_pictures_and_videos() {
        assert_eq!(
            classify(Some("image/jpeg"), None, Some("a.jpeg"), false),
            (AttachmentKind::Photo, Some("jpg"))
        );
        assert_eq!(
            classify(None, Some("public.heic"), Some("IMG_1.HEIC"), false),
            (AttachmentKind::Photo, Some("heic"))
        );
        assert_eq!(
            classify(Some("image/gif"), None, None, false),
            (AttachmentKind::Gif, Some("gif"))
        );
        assert_eq!(
            classify(Some("image/png"), None, None, true),
            (AttachmentKind::Sticker, Some("png"))
        );
        assert_eq!(
            classify(Some("video/mp4"), None, None, false),
            (AttachmentKind::Video, Some("mp4"))
        );
        assert_eq!(
            classify(None, None, Some("clip.MOV"), false),
            (AttachmentKind::Video, Some("mov"))
        );
        assert_eq!(
            classify(Some("audio/x-caf"), None, Some("Audio Message.caf"), false),
            (AttachmentKind::Audio, None)
        );
        assert_eq!(
            classify(Some("text/vcard"), None, Some("Jane.vcf"), false),
            (AttachmentKind::Contact, None)
        );
        assert_eq!(
            classify(Some("text/x-vlocation"), None, Some("CL.loc.vcf"), false),
            (AttachmentKind::Location, None)
        );
        assert_eq!(
            classify(Some("application/pdf"), None, Some("plan.pdf"), false),
            (AttachmentKind::File, None)
        );
        // Something that could run is never stored, whatever it claims.
        assert_eq!(
            classify(Some("application/x-msdownload"), None, Some("setup.exe"), false),
            (AttachmentKind::File, None)
        );
        assert_eq!(classify(Some("image/svg+xml"), None, Some("a.svg"), false).1, None);
    }

    #[test]
    fn attachment_paths_stay_inside_the_messages_folder() {
        assert_eq!(
            manifest_path("~/Library/SMS/Attachments/ab/11/X/IMG.HEIC").as_deref(),
            Some("Library/SMS/Attachments/ab/11/X/IMG.HEIC")
        );
        assert_eq!(
            manifest_path("/var/mobile/Library/SMS/Attachments/ab/X/a.jpg").as_deref(),
            Some("Library/SMS/Attachments/ab/X/a.jpg")
        );
        assert_eq!(manifest_path("~/Library/SMS/Attachments/../../Keychains/x"), None);
        assert_eq!(manifest_path("~/Documents/a.jpg"), None);
    }

    #[test]
    fn bodies_come_from_text_or_attributed_body() {
        assert_eq!(message_body(Some("  hi  "), None), "hi");
        assert_eq!(message_body(Some("\u{FFFC}look\u{FFFC}"), None), "look");
        assert_eq!(
            message_body(Some(""), Some(&typedstream::tests::stream_for("from the stream"))),
            "from the stream"
        );
        assert_eq!(message_body(None, Some(b"garbage")), "");
        assert_eq!(message_body(None, None), "");
    }
}
