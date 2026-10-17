//! `Manifest.db`: where each file of the phone sits in the backup folder. One row per file in
//! `Files (fileID, domain, relativePath, flags, file)`; the file itself is `<backup>/<first two hex
//! digits of fileID>/<fileID>`. In an encrypted backup the `file` column (an NSKeyedArchiver
//! binary plist) carries the file's own wrapped key, its protection class and its real size.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use plist::Value;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

/// Messages' database.
pub const SMS_DOMAIN: &str = "HomeDomain";
pub const SMS_DB: &str = "Library/SMS/sms.db";
/// Message attachments live under this path, in MediaDomain (HomeDomain on some versions).
pub const ATTACHMENTS_PREFIX: &str = "Library/SMS/Attachments/";
const ATTACHMENT_DOMAINS: [&str; 2] = ["MediaDomain", "HomeDomain"];

/// One file in the backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub file_id: String,
    /// The plaintext size, when the manifest records it.
    pub size: Option<u64>,
    /// An encrypted backup's per-file key: 4-byte little-endian class + wrapped key.
    pub encryption_key: Option<Vec<u8>>,
}

impl FileEntry {
    /// Where the file is inside the backup folder. `None` for an id that isn't 40 hex digits, so a
    /// tampered manifest can't point outside the folder.
    pub fn path_in(&self, backup: &Path) -> Option<PathBuf> {
        let id = &self.file_id;
        let valid = id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit());
        valid.then(|| backup.join(&id[..2]).join(id))
    }
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

/// The file at `domain`/`relative_path`, if the backup has it (regular files only).
pub fn lookup(conn: &Connection, domain: &str, relative_path: &str) -> rusqlite::Result<Option<FileEntry>> {
    conn.query_row(
        "SELECT fileID, file FROM Files WHERE domain = ?1 AND relativePath = ?2 AND flags = 1",
        params![domain, relative_path],
        |r| Ok(entry(r.get(0)?, r.get::<_, Option<Vec<u8>>>(1)?)),
    )
    .optional()
}

/// Every message attachment in the backup, by its path relative to the home folder
/// (`Library/SMS/Attachments/…`). MediaDomain wins when both domains have one.
pub fn attachments(conn: &Connection) -> rusqlite::Result<HashMap<String, FileEntry>> {
    let mut map = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT domain, relativePath, fileID, file FROM Files
         WHERE flags = 1 AND relativePath LIKE 'Library/SMS/Attachments/%' AND domain IN (?1, ?2)",
    )?;
    let rows = stmt.query_map(params![ATTACHMENT_DOMAINS[0], ATTACHMENT_DOMAINS[1]], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            entry(r.get(2)?, r.get::<_, Option<Vec<u8>>>(3)?),
        ))
    })?;
    for row in rows {
        let (domain, path, e) = row?;
        if domain == ATTACHMENT_DOMAINS[0] || !map.contains_key(&path) {
            map.insert(path, e);
        }
    }
    Ok(map)
}

fn entry(file_id: String, blob: Option<Vec<u8>>) -> FileEntry {
    let (size, encryption_key) = blob.as_deref().map(parse_file_blob).unwrap_or((None, None));
    FileEntry {
        file_id,
        size,
        encryption_key,
    }
}

/// The size and wrapped key from a `file` blob (an NSKeyedArchiver plist of an `MBFile`):
/// `$top.root` points into `$objects` at a dictionary whose `Size` is a number and whose
/// `EncryptionKey` points at an `NSData` (`NS.data`). Anything missing is `None`.
pub fn parse_file_blob(blob: &[u8]) -> (Option<u64>, Option<Vec<u8>>) {
    let Ok(root) = Value::from_reader(std::io::Cursor::new(blob)) else {
        return (None, None);
    };
    let Some(root) = root.as_dictionary() else {
        return (None, None);
    };
    let Some(objects) = root.get("$objects").and_then(Value::as_array) else {
        return (None, None);
    };
    let deref = |v: &Value| -> Option<Value> {
        match v {
            Value::Uid(uid) => objects.get(usize::try_from(uid.get()).ok()?).cloned(),
            other => Some(other.clone()),
        }
    };
    let file = root
        .get("$top")
        .and_then(Value::as_dictionary)
        .and_then(|top| top.get("root"))
        .and_then(deref);
    let Some(file) = file.as_ref().and_then(Value::as_dictionary) else {
        return (None, None);
    };
    let size = file.get("Size").and_then(Value::as_unsigned_integer);
    let key = file.get("EncryptionKey").and_then(deref).and_then(|k| match k {
        Value::Data(d) => Some(d),
        Value::Dictionary(d) => d.get("NS.data").and_then(Value::as_data).map(<[u8]>::to_vec),
        _ => None,
    });
    (size, key)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use plist::{Dictionary, Uid};

    /// A `file` blob as an encrypted backup stores it.
    pub(crate) fn file_blob(size: u64, key: Option<&[u8]>) -> Vec<u8> {
        let mut mbfile = Dictionary::new();
        mbfile.insert("$class".into(), Value::Uid(Uid::new(3)));
        mbfile.insert("Size".into(), Value::Integer(size.into()));
        mbfile.insert("ProtectionClass".into(), Value::Integer(3.into()));
        mbfile.insert("RelativePath".into(), Value::Uid(Uid::new(4)));
        if key.is_some() {
            mbfile.insert("EncryptionKey".into(), Value::Uid(Uid::new(2)));
        }
        let mut data = Dictionary::new();
        data.insert("$class".into(), Value::Uid(Uid::new(5)));
        data.insert("NS.data".into(), Value::Data(key.unwrap_or_default().to_vec()));
        let objects = vec![
            Value::String("$null".into()),
            Value::Dictionary(mbfile),
            Value::Dictionary(data),
            Value::Dictionary(Dictionary::from_iter([(
                "$classname".to_string(),
                Value::String("MBFile".into()),
            )])),
            Value::String("Library/SMS/sms.db".into()),
            Value::Dictionary(Dictionary::from_iter([(
                "$classname".to_string(),
                Value::String("NSMutableData".into()),
            )])),
        ];
        let mut top = Dictionary::new();
        top.insert("root".into(), Value::Uid(Uid::new(1)));
        let mut root = Dictionary::new();
        root.insert("$archiver".into(), Value::String("NSKeyedArchiver".into()));
        root.insert("$version".into(), Value::Integer(100000.into()));
        root.insert("$top".into(), Value::Dictionary(top));
        root.insert("$objects".into(), Value::Array(objects));
        let mut out = Vec::new();
        Value::Dictionary(root).to_writer_binary(&mut out).unwrap();
        out
    }

    /// A fake Manifest.db with the given files: (domain, relative path, file id, blob).
    pub(crate) fn make_manifest(path: &Path, files: &[(&str, &str, &str, Option<Vec<u8>>)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE Files (fileID TEXT PRIMARY KEY, domain TEXT, relativePath TEXT, flags INTEGER, file BLOB);",
        )
        .unwrap();
        for (domain, rel, id, blob) in files {
            conn.execute(
                "INSERT INTO Files VALUES (?1, ?2, ?3, 1, ?4)",
                params![id, domain, rel, blob],
            )
            .unwrap();
        }
    }

    #[test]
    fn reads_size_and_key_from_a_file_blob() {
        let (size, key) = parse_file_blob(&file_blob(4096, Some(&[1, 2, 3, 4, 5])));
        assert_eq!(size, Some(4096));
        assert_eq!(key.as_deref(), Some(&[1u8, 2, 3, 4, 5][..]));
        let (size, key) = parse_file_blob(&file_blob(12, None));
        assert_eq!((size, key), (Some(12), None));
        assert_eq!(parse_file_blob(b"not a plist"), (None, None));
        assert_eq!(parse_file_blob(b""), (None, None));
    }

    #[test]
    fn finds_files_and_refuses_ids_that_could_leave_the_folder() {
        let dir = std::env::temp_dir().join(format!("tug-manifest-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("Manifest.db");
        let _ = std::fs::remove_file(&db);
        let sms_id = "3d0d7e5fb2ce288813306e4d4636395e047a3d28";
        make_manifest(
            &db,
            &[
                (SMS_DOMAIN, SMS_DB, sms_id, Some(file_blob(100, Some(&[9; 44])))),
                (
                    "MediaDomain",
                    "Library/SMS/Attachments/ab/12/X/IMG_1.HEIC",
                    "a".repeat(40).as_str(),
                    None,
                ),
                (
                    "HomeDomain",
                    "Library/SMS/Attachments/ab/12/X/IMG_1.HEIC",
                    "b".repeat(40).as_str(),
                    None,
                ),
                (
                    "HomeDomain",
                    "Library/SMS/Attachments/cd/34/Y/clip.mov",
                    "c".repeat(40).as_str(),
                    None,
                ),
                (
                    "CameraRollDomain",
                    "Media/DCIM/100APPLE/IMG_2.HEIC",
                    "d".repeat(40).as_str(),
                    None,
                ),
            ],
        );
        let conn = open(&db).unwrap();
        let sms = lookup(&conn, SMS_DOMAIN, SMS_DB).unwrap().unwrap();
        assert_eq!(sms.size, Some(100));
        assert_eq!(
            sms.path_in(Path::new("B")),
            Some(Path::new("B").join("3d").join(sms_id))
        );
        assert!(lookup(&conn, SMS_DOMAIN, "Library/SMS/nope.db").unwrap().is_none());
        let atts = attachments(&conn).unwrap();
        assert_eq!(atts.len(), 2, "camera roll photos aren't message attachments");
        assert_eq!(
            atts["Library/SMS/Attachments/ab/12/X/IMG_1.HEIC"].file_id,
            "a".repeat(40)
        );
        assert_eq!(atts["Library/SMS/Attachments/cd/34/Y/clip.mov"].file_id, "c".repeat(40));
        drop(conn);
        for bad in ["../../windows/system32", "", "3d0d7e5fb2ce288813306e4d4636395e047a3d2Z"] {
            let e = FileEntry {
                file_id: bad.into(),
                size: None,
                encryption_key: None,
            };
            assert_eq!(e.path_in(Path::new("B")), None, "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
