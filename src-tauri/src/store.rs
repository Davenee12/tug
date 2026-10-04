//! Local notification history (SQLite + FTS5) and key/value settings.
//!
//! iOS only keeps what is on the lock screen; tug keeps everything it has
//! seen so the feed is searchable after the phone has cleared it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::ancs::{Category, EventFlags, NotificationAttributes};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS notifications (
    id             INTEGER PRIMARY KEY,
    session        TEXT    NOT NULL,
    uid            INTEGER NOT NULL,
    app_id         TEXT    NOT NULL,
    category       TEXT    NOT NULL,
    title          TEXT    NOT NULL DEFAULT '',
    subtitle       TEXT    NOT NULL DEFAULT '',
    message        TEXT    NOT NULL DEFAULT '',
    posted_at      TEXT,
    received_at    INTEGER NOT NULL,
    flags          INTEGER NOT NULL,
    positive_label TEXT    NOT NULL DEFAULT '',
    negative_label TEXT    NOT NULL DEFAULT '',
    removed_at     INTEGER
);
CREATE INDEX IF NOT EXISTS notifications_received ON notifications (received_at DESC);
CREATE INDEX IF NOT EXISTS notifications_session_uid ON notifications (session, uid);
CREATE INDEX IF NOT EXISTS notifications_identity ON notifications (app_id, posted_at, title);

CREATE VIRTUAL TABLE IF NOT EXISTS notifications_fts USING fts5 (
    title, subtitle, message, content = 'notifications', content_rowid = 'id'
);
CREATE TRIGGER IF NOT EXISTS notifications_ai AFTER INSERT ON notifications BEGIN
    INSERT INTO notifications_fts (rowid, title, subtitle, message) VALUES (new.id, new.title, new.subtitle, new.message);
END;
CREATE TRIGGER IF NOT EXISTS notifications_ad AFTER DELETE ON notifications BEGIN
    INSERT INTO notifications_fts (notifications_fts, rowid, title, subtitle, message) VALUES ('delete', old.id, old.title, old.subtitle, old.message);
END;
CREATE TRIGGER IF NOT EXISTS notifications_au AFTER UPDATE OF title, subtitle, message ON notifications BEGIN
    INSERT INTO notifications_fts (notifications_fts, rowid, title, subtitle, message) VALUES ('delete', old.id, old.title, old.subtitle, old.message);
    INSERT INTO notifications_fts (rowid, title, subtitle, message) VALUES (new.id, new.title, new.subtitle, new.message);
END;

CREATE TABLE IF NOT EXISTS apps (
    app_id       TEXT PRIMARY KEY,
    display_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const SELECT: &str =
    "SELECT n.id, n.session, n.uid, n.app_id, a.display_name, n.category, n.title, n.subtitle, n.message,
            n.posted_at, n.received_at, n.flags, n.positive_label, n.negative_label, n.removed_at
     FROM notifications n LEFT JOIN apps a ON a.app_id = n.app_id";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredNotification {
    pub id: i64,
    pub app_id: String,
    pub app_name: Option<String>,
    pub category: String,
    pub title: String,
    pub subtitle: String,
    pub message: String,
    pub posted_at: Option<String>,
    pub received_at: i64,
    pub flags: EventFlags,
    pub positive_label: String,
    pub negative_label: String,
    pub removed_at: Option<i64>,
    /// Still on the phone in the current connection, so actions can be sent.
    pub live: bool,
    #[serde(skip)]
    pub session: String,
    #[serde(skip)]
    pub uid: u32,
}

pub struct NewNotification<'a> {
    pub session: &'a str,
    pub uid: u32,
    pub category: Category,
    pub flags: EventFlags,
    pub attrs: &'a NotificationAttributes,
    pub received_at: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

pub type Result<T> = rusqlite::Result<T>;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent, so keep going.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Insert or update a notification and return the stored row.
    ///
    /// iOS re-sends every notification still on the phone (flagged pre-existing)
    /// each time we subscribe, with UIDs that are only valid for that connection.
    /// Those are matched to the existing row by content instead of duplicated.
    pub fn upsert_notification(&self, n: &NewNotification) -> Result<StoredNotification> {
        let conn = self.conn();
        let a = n.attrs;
        let by_uid: Option<i64> = conn
            .query_row(
                "SELECT id FROM notifications WHERE session = ?1 AND uid = ?2",
                params![n.session, n.uid],
                |r| r.get(0),
            )
            .optional()?;
        let existing = match by_uid {
            Some(id) => Some(id),
            None if n.flags.pre_existing => conn
                .query_row(
                    "SELECT id FROM notifications
                     WHERE app_id = ?1 AND posted_at IS ?2 AND title = ?3 AND subtitle = ?4 AND message = ?5
                     ORDER BY id DESC LIMIT 1",
                    params![a.app_id, a.date, a.title, a.subtitle, a.message],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        let id = match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE notifications SET session = ?2, uid = ?3, category = ?4, title = ?5, subtitle = ?6,
                         message = ?7, posted_at = ?8, flags = ?9, positive_label = ?10, negative_label = ?11,
                         removed_at = NULL
                     WHERE id = ?1",
                    params![
                        id,
                        n.session,
                        n.uid,
                        n.category.as_str(),
                        a.title,
                        a.subtitle,
                        a.message,
                        a.date,
                        n.flags.bits(),
                        a.positive_label,
                        a.negative_label
                    ],
                )?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO notifications (session, uid, app_id, category, title, subtitle, message, posted_at,
                         received_at, flags, positive_label, negative_label)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        n.session,
                        n.uid,
                        a.app_id,
                        n.category.as_str(),
                        a.title,
                        a.subtitle,
                        a.message,
                        a.date,
                        n.received_at,
                        n.flags.bits(),
                        a.positive_label,
                        a.negative_label
                    ],
                )?;
                conn.last_insert_rowid()
            }
        };
        conn.query_row(&format!("{SELECT} WHERE n.id = ?1"), [id], |r| {
            map_row(r, Some(n.session))
        })
    }

    /// Mark a notification removed from the phone. Returns its row id if known.
    pub fn mark_removed(&self, session: &str, uid: u32, at: i64) -> Result<Option<i64>> {
        self.conn()
            .query_row(
                "UPDATE notifications SET removed_at = ?3 WHERE session = ?1 AND uid = ?2 AND removed_at IS NULL RETURNING id",
                params![session, uid, at],
                |r| r.get(0),
            )
            .optional()
    }

    /// After the iPhone has replayed everything still on it for `live_session`,
    /// any open row from an earlier session was cleared while tug was away.
    /// Marks those removed and returns their ids.
    pub fn sweep_stale(&self, live_session: &str, at: i64) -> Result<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "UPDATE notifications SET removed_at = ?2 WHERE removed_at IS NULL AND session != ?1 RETURNING id",
        )?;
        let rows = stmt.query_map(params![live_session, at], |r| r.get(0))?;
        rows.collect()
    }

    #[cfg(test)]
    pub fn get(&self, id: i64, live_session: Option<&str>) -> Result<Option<StoredNotification>> {
        self.conn()
            .query_row(&format!("{SELECT} WHERE n.id = ?1"), [id], |r| map_row(r, live_session))
            .optional()
    }

    /// Newest first; pass the smallest id already loaded as `before_id` to page.
    pub fn recent(
        &self,
        limit: u32,
        before_id: Option<i64>,
        live_session: Option<&str>,
    ) -> Result<Vec<StoredNotification>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("{SELECT} WHERE n.id < ?1 ORDER BY n.id DESC LIMIT ?2"))?;
        let rows = stmt.query_map(params![before_id.unwrap_or(i64::MAX), limit], |r| {
            map_row(r, live_session)
        })?;
        rows.collect()
    }

    /// Prefix search across title, subtitle and message, plus app name.
    pub fn search(&self, query: &str, limit: u32, live_session: Option<&str>) -> Result<Vec<StoredNotification>> {
        let Some(fts) = fts_query(query) else {
            return self.recent(limit, None, live_session);
        };
        let like = format!(
            "%{}%",
            query
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "{SELECT} WHERE n.id IN (SELECT rowid FROM notifications_fts WHERE notifications_fts MATCH ?1)
                 OR a.display_name LIKE ?2 ESCAPE '\\' OR n.app_id LIKE ?2 ESCAPE '\\'
             ORDER BY n.id DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![fts, like, limit], |r| map_row(r, live_session))?;
        rows.collect()
    }

    pub fn set_app_name(&self, app_id: &str, name: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO apps (app_id, display_name) VALUES (?1, ?2)
             ON CONFLICT (app_id) DO UPDATE SET display_name = excluded.display_name",
            params![app_id, name],
        )?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.conn().execute_batch(
            "DELETE FROM notifications; INSERT INTO notifications_fts (notifications_fts) VALUES ('rebuild');",
        )
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()
    }

    pub fn settings(&self) -> Result<HashMap<String, String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn().execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }
}

fn map_row(r: &Row, live_session: Option<&str>) -> Result<StoredNotification> {
    let session: String = r.get(1)?;
    let removed_at: Option<i64> = r.get(14)?;
    Ok(StoredNotification {
        id: r.get(0)?,
        live: removed_at.is_none() && live_session == Some(session.as_str()),
        session,
        uid: r.get(2)?,
        app_id: r.get(3)?,
        app_name: r.get(4)?,
        category: r.get(5)?,
        title: r.get(6)?,
        subtitle: r.get(7)?,
        message: r.get(8)?,
        posted_at: r.get(9)?,
        received_at: r.get(10)?,
        flags: EventFlags::from_bits(r.get(11)?),
        positive_label: r.get(12)?,
        negative_label: r.get(13)?,
        removed_at,
    })
}

/// Turn free text into a safe FTS5 query: every token quoted, prefix-matched, ANDed.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(app: &str, title: &str, message: &str) -> NotificationAttributes {
        NotificationAttributes {
            app_id: app.into(),
            title: title.into(),
            message: message.into(),
            date: Some("2026-10-04T15:30:12".into()),
            ..Default::default()
        }
    }

    fn insert(
        store: &Store,
        session: &str,
        uid: u32,
        flags: EventFlags,
        a: &NotificationAttributes,
    ) -> StoredNotification {
        store
            .upsert_notification(&NewNotification {
                session,
                uid,
                category: Category::Social,
                flags,
                attrs: a,
                received_at: 1_000,
            })
            .unwrap()
    }

    #[test]
    fn inserts_and_lists_newest_first() {
        let s = Store::in_memory().unwrap();
        let a = insert(
            &s,
            "s1",
            1,
            EventFlags::default(),
            &attrs("com.apple.MobileSMS", "Jane", "hi"),
        );
        let b = insert(
            &s,
            "s1",
            2,
            EventFlags::default(),
            &attrs("com.apple.MobileSMS", "Sam", "yo"),
        );
        let rows = s.recent(10, None, Some("s1")).unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![b.id, a.id]);
        assert!(rows.iter().all(|r| r.live));
        assert!(s.recent(10, None, Some("s2")).unwrap().iter().all(|r| !r.live));
        assert_eq!(s.recent(10, Some(b.id), None).unwrap().len(), 1, "paging");
    }

    #[test]
    fn pre_existing_on_reconnect_reuses_row() {
        let s = Store::in_memory().unwrap();
        let a = attrs("com.apple.MobileSMS", "Jane", "hi");
        let first = insert(&s, "s1", 7, EventFlags::default(), &a);
        let pre = EventFlags {
            pre_existing: true,
            ..Default::default()
        };
        let again = insert(&s, "s2", 3, pre, &a);
        assert_eq!(first.id, again.id);
        assert!(again.live);
        assert_eq!(s.recent(10, None, None).unwrap().len(), 1);
    }

    #[test]
    fn modified_updates_in_place() {
        let s = Store::in_memory().unwrap();
        let first = insert(&s, "s1", 7, EventFlags::default(), &attrs("x", "Jane", "hi"));
        let second = insert(&s, "s1", 7, EventFlags::default(), &attrs("x", "Jane", "hi again"));
        assert_eq!(first.id, second.id);
        assert_eq!(second.message, "hi again");
        assert_eq!(s.search("again", 10, None).unwrap().len(), 1, "FTS follows updates");
        assert_eq!(s.search("hi", 10, None).unwrap().len(), 1);
    }

    #[test]
    fn sweep_closes_rows_the_phone_did_not_replay() {
        let s = Store::in_memory().unwrap();
        let kept = attrs("x", "Jane", "still on the lock screen");
        let gone = attrs("x", "Sam", "cleared while tug was off");
        insert(&s, "s1", 1, EventFlags::default(), &kept);
        let stale = insert(&s, "s1", 2, EventFlags::default(), &gone);
        // Reconnect: iOS replays only what's still on the phone.
        let pre = EventFlags {
            pre_existing: true,
            ..Default::default()
        };
        let replayed = insert(&s, "s2", 7, pre, &kept);
        assert_eq!(s.sweep_stale("s2", 5_000).unwrap(), vec![stale.id]);
        assert_eq!(s.get(replayed.id, Some("s2")).unwrap().unwrap().removed_at, None);
        assert_eq!(s.get(stale.id, None).unwrap().unwrap().removed_at, Some(5_000));
        assert!(s.sweep_stale("s2", 6_000).unwrap().is_empty(), "idempotent");
    }

    #[test]
    fn removal_clears_live() {
        let s = Store::in_memory().unwrap();
        let n = insert(&s, "s1", 7, EventFlags::default(), &attrs("x", "Jane", "hi"));
        assert_eq!(s.mark_removed("s1", 7, 2_000).unwrap(), Some(n.id));
        assert_eq!(s.mark_removed("s1", 7, 3_000).unwrap(), None, "already removed");
        let got = s.get(n.id, Some("s1")).unwrap().unwrap();
        assert_eq!(got.removed_at, Some(2_000));
        assert!(!got.live);
    }

    #[test]
    fn search_handles_prefixes_quotes_and_app_names() {
        let s = Store::in_memory().unwrap();
        insert(
            &s,
            "s1",
            1,
            EventFlags::default(),
            &attrs("net.whatsapp.WhatsApp", "Mum", "Dinner at 7?"),
        );
        insert(
            &s,
            "s1",
            2,
            EventFlags::default(),
            &attrs("com.apple.MobileSMS", "Bank", "Code 123456"),
        );
        s.set_app_name("net.whatsapp.WhatsApp", "WhatsApp").unwrap();
        assert_eq!(s.search("dinn", 10, None).unwrap().len(), 1);
        assert_eq!(
            s.search("whatsapp", 10, None).unwrap()[0].app_name.as_deref(),
            Some("WhatsApp")
        );
        assert!(s.search("\"unbalanced", 10, None).is_ok());
        assert!(s.search("100%", 10, None).unwrap().is_empty());
        assert_eq!(
            s.search("   ", 10, None).unwrap().len(),
            2,
            "blank query falls back to recent"
        );
    }

    #[test]
    fn clear_history_empties_search_index() {
        let s = Store::in_memory().unwrap();
        insert(&s, "s1", 1, EventFlags::default(), &attrs("x", "Jane", "hello"));
        s.clear_history().unwrap();
        assert!(s.search("hello", 10, None).unwrap().is_empty());
        assert!(s.recent(10, None, None).unwrap().is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let s = Store::in_memory().unwrap();
        s.set_setting("k", "v1").unwrap();
        s.set_setting("k", "v2").unwrap();
        assert_eq!(s.setting("k").unwrap().as_deref(), Some("v2"));
        assert_eq!(s.settings().unwrap().len(), 1);
        s.delete_setting("k").unwrap();
        assert_eq!(s.setting("k").unwrap(), None);
    }
}
