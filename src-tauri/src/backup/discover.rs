//! Finding the iPhone backups on this PC and describing each one from its plists, without
//! opening any of the phone's data.
//!
//! The Apple Devices app and iTunes from apple.com keep backups in
//! `%APPDATA%\Apple\MobileSync\Backup\<device id>` (older iTunes: `%APPDATA%\Apple Computer\…`),
//! iTunes from the Microsoft Store in `%USERPROFILE%\Apple\MobileSync\Backup\<device id>`. Each
//! backup folder has `Info.plist` (device name, model, iOS version, last backup date),
//! `Manifest.plist` (encrypted or not, and the keys) and `Status.plist` (whether it finished).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use plist::Value;
use serde::Serialize;

/// Something that stops a backup from being read, in words the UI chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupProblem {
    /// Made before iOS 10 (`Manifest.mbdb`, no `Manifest.db`): too old to read.
    TooOld,
    /// The backup didn't finish (or is being made right now).
    Incomplete,
    /// The folder isn't a backup tug can make sense of.
    Unreadable,
}

/// One backup, as the picker lists it. Never logged: the device name is the owner's.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// Opaque to the UI: it names the backup back to tug, which keeps the folder itself.
    pub id: String,
    pub device_name: String,
    /// Apple's model identifier ("iPhone15,2"); the UI turns it into a name.
    pub model: Option<String>,
    pub ios_version: Option<String>,
    /// When the backup was last made (ms since 1970).
    pub last_backup: Option<i64>,
    pub size_bytes: u64,
    pub encrypted: bool,
    pub problem: Option<BackupProblem>,
}

/// The folders backups live in, given `%APPDATA%` and `%USERPROFILE%`.
pub fn backup_roots(appdata: Option<&Path>, profile: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(a) = appdata {
        roots.push(a.join("Apple").join("MobileSync").join("Backup"));
        roots.push(a.join("Apple Computer").join("MobileSync").join("Backup"));
    }
    if let Some(p) = profile {
        roots.push(p.join("Apple").join("MobileSync").join("Backup"));
    }
    roots
}

/// This PC's backup folders, from the environment.
pub fn default_roots() -> Vec<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    backup_roots(appdata.as_deref(), profile.as_deref())
}

/// Every backup under these roots, newest first, with the folder each is in.
pub fn list(roots: &[PathBuf]) -> Vec<(BackupInfo, PathBuf)> {
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("Info.plist").is_file() {
                if let Some(info) = describe(&path) {
                    found.push((info, path));
                }
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0.last_backup));
    found
}

/// A folder the person picked: the backup itself, or a folder holding backups (the first, newest,
/// is taken).
pub fn describe_picked(folder: &Path) -> Option<(BackupInfo, PathBuf)> {
    if folder.join("Manifest.plist").is_file() || folder.join("Info.plist").is_file() {
        return describe(folder).map(|i| (i, folder.to_path_buf()));
    }
    list(&[folder.to_path_buf()]).into_iter().next()
}

fn read_plist(path: &Path) -> Option<plist::Dictionary> {
    Value::from_file(path).ok()?.into_dictionary()
}

fn string(d: &plist::Dictionary, key: &str) -> Option<String> {
    d.get(key)
        .and_then(Value::as_string)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn date_ms(d: &plist::Dictionary, key: &str) -> Option<i64> {
    let t: SystemTime = d.get(key)?.as_date()?.into();
    let ms = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_millis();
    i64::try_from(ms).ok()
}

/// Describe one backup folder. `None` only when it has neither plist; otherwise a description,
/// with a problem when it can't be read.
pub fn describe(folder: &Path) -> Option<BackupInfo> {
    let info = read_plist(&folder.join("Info.plist"));
    let manifest = read_plist(&folder.join("Manifest.plist"));
    if info.is_none() && manifest.is_none() {
        return None;
    }
    let lockdown = manifest
        .as_ref()
        .and_then(|m| m.get("Lockdown"))
        .and_then(Value::as_dictionary);
    let pick = |info_key: &str, lockdown_key: &str| {
        info.as_ref()
            .and_then(|i| string(i, info_key))
            .or_else(|| lockdown.and_then(|l| string(l, lockdown_key)))
    };
    let device_name = pick("Device Name", "DeviceName")
        .or_else(|| pick("Display Name", "DeviceName"))
        .unwrap_or_else(|| "iPhone".to_string());
    let encrypted = manifest
        .as_ref()
        .and_then(|m| m.get("IsEncrypted"))
        .and_then(Value::as_boolean)
        .unwrap_or(false);
    let status = read_plist(&folder.join("Status.plist"));
    let last_backup = info
        .as_ref()
        .and_then(|i| date_ms(i, "Last Backup Date"))
        .or_else(|| status.as_ref().and_then(|s| date_ms(s, "Date")))
        .or_else(|| manifest.as_ref().and_then(|m| date_ms(m, "Date")));
    let finished = status
        .as_ref()
        .and_then(|s| string(s, "SnapshotState"))
        .is_none_or(|state| state == "finished");
    let problem = if folder.join("Manifest.db").is_file() {
        if manifest.is_none() {
            Some(BackupProblem::Unreadable)
        } else if !finished {
            Some(BackupProblem::Incomplete)
        } else {
            None
        }
    } else if folder.join("Manifest.mbdb").is_file() {
        Some(BackupProblem::TooOld)
    } else {
        Some(BackupProblem::Incomplete)
    };
    Some(BackupInfo {
        id: backup_id(folder),
        device_name,
        model: pick("Product Type", "ProductType"),
        ios_version: pick("Product Version", "ProductVersion"),
        last_backup,
        size_bytes: folder_size(folder),
        encrypted,
        problem,
    })
}

/// A backup's id: one per folder. The folder's own name is the phone's id, which the same phone's
/// backups in two places (desktop iTunes and the Apple Devices app) share, so a hash of the whole
/// path tells them apart; opaque either way.
fn backup_id(folder: &Path) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(folder.to_string_lossy().as_bytes());
    digest.iter().take(10).map(|b| format!("{b:02x}")).collect()
}

/// The bytes a backup takes up (the files in its 256 hashed subfolders and the top level).
fn folder_size(folder: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![(folder.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                // Backups are two levels deep; don't wander further (or follow anything odd).
                if depth < 2 {
                    stack.push((e.path(), depth + 1));
                }
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use plist::{Date, Dictionary};

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tug-{name}-{}-{}", std::process::id(), crate::state::now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Info.plist, Manifest.plist and Status.plist for a made-up phone.
    pub(crate) fn write_plists(
        folder: &Path,
        name: &str,
        encrypted: bool,
        when: SystemTime,
        extra_manifest: Dictionary,
    ) {
        std::fs::create_dir_all(folder).unwrap();
        let mut info = Dictionary::new();
        info.insert("Device Name".into(), Value::String(name.into()));
        info.insert("Product Type".into(), Value::String("iPhone15,2".into()));
        info.insert("Product Version".into(), Value::String("17.5.1".into()));
        info.insert("Last Backup Date".into(), Value::Date(Date::from(when)));
        Value::Dictionary(info).to_file_xml(folder.join("Info.plist")).unwrap();
        let mut manifest = extra_manifest;
        manifest.insert("IsEncrypted".into(), Value::Boolean(encrypted));
        manifest.insert("Version".into(), Value::String("10.0".into()));
        Value::Dictionary(manifest)
            .to_file_binary(folder.join("Manifest.plist"))
            .unwrap();
        let mut status = Dictionary::new();
        status.insert("SnapshotState".into(), Value::String("finished".into()));
        Value::Dictionary(status)
            .to_file_binary(folder.join("Status.plist"))
            .unwrap();
    }

    #[test]
    fn the_same_phone_backed_up_in_two_places_is_two_backups() {
        let root = temp_dir("discover-twice");
        let (a, b) = (root.join("one"), root.join("two"));
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let new = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_760_000_000);
        for (r, when) in [(&a, old), (&b, new)] {
            let f = r.join("00008110-000A11112222801E");
            write_plists(&f, "Test iPhone", false, when, Dictionary::new());
            std::fs::write(f.join("Manifest.db"), [0u8; 10]).unwrap();
        }
        let found = list(&[a.clone(), b.clone()]);
        assert_eq!(found.len(), 2);
        assert_ne!(found[0].0.id, found[1].0.id);
        assert_eq!(found[0].0.last_backup, Some(1_760_000_000_000), "newest first");
        assert!(found[0].1.starts_with(&b));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn roots_cover_the_apple_devices_app_and_both_itunes() {
        let roots = backup_roots(Some(Path::new("A")), Some(Path::new("P")));
        assert!(roots.contains(&Path::new("A").join("Apple").join("MobileSync").join("Backup")));
        assert!(roots.contains(&Path::new("P").join("Apple").join("MobileSync").join("Backup")));
        assert!(backup_roots(None, None).is_empty());
    }

    #[test]
    fn lists_backups_newest_first_with_what_the_picker_shows() {
        let root = temp_dir("discover");
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let new = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_760_000_000);
        let a = root.join("00008110-000A11112222801E");
        write_plists(&a, "Test iPhone", true, old, Dictionary::new());
        std::fs::write(a.join("Manifest.db"), [0u8; 1000]).unwrap();
        std::fs::create_dir_all(a.join("3d")).unwrap();
        std::fs::write(
            a.join("3d").join("3d0d7e5fb2ce288813306e4d4636395e047a3d28"),
            [0u8; 5000],
        )
        .unwrap();
        let b = root.join("00008030-000B33334444802E");
        write_plists(&b, "Other iPhone", false, new, Dictionary::new());
        std::fs::write(b.join("Manifest.db"), [0u8; 10]).unwrap();
        // An old-format backup, one still being made, and a folder that's no backup at all.
        let c = root.join("old");
        write_plists(&c, "Old iPhone", false, old, Dictionary::new());
        std::fs::write(c.join("Manifest.mbdb"), [0u8; 10]).unwrap();
        let d = root.join("unfinished");
        write_plists(&d, "Busy iPhone", false, old, Dictionary::new());
        std::fs::create_dir_all(root.join("not-a-backup")).unwrap();

        let found = list(&[root.clone(), root.join("missing")]);
        let names: Vec<&str> = found.iter().map(|(i, _)| i.device_name.as_str()).collect();
        assert_eq!(names[0], "Other iPhone", "newest first");
        assert_eq!(found.len(), 4);
        let test = &found.iter().find(|(i, _)| i.device_name == "Test iPhone").unwrap().0;
        assert!(test.encrypted);
        assert_eq!(test.model.as_deref(), Some("iPhone15,2"));
        assert_eq!(test.ios_version.as_deref(), Some("17.5.1"));
        assert_eq!(test.last_backup, Some(1_700_000_000_000));
        assert_eq!(test.problem, None);
        assert!(test.size_bytes >= 6000, "{}", test.size_bytes);
        assert_eq!(test.id.len(), 20);
        assert!(!test.id.contains("00008110"), "opaque");
        let problem = |n: &str| found.iter().find(|(i, _)| i.device_name == n).unwrap().0.problem;
        assert_eq!(problem("Old iPhone"), Some(BackupProblem::TooOld));
        assert_eq!(problem("Busy iPhone"), Some(BackupProblem::Incomplete));

        // Picking the folder of backups takes the newest; picking one backup takes that one.
        assert_eq!(describe_picked(&root).unwrap().0.device_name, "Other iPhone");
        assert_eq!(describe_picked(&a).unwrap().0.device_name, "Test iPhone");
        assert!(describe_picked(&root.join("not-a-backup")).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
