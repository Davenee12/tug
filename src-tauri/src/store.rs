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

-- Messages from any connector (first: the iPhone over Bluetooth MAP).
CREATE TABLE IF NOT EXISTS messages (
    id          INTEGER PRIMARY KEY,
    source      TEXT    NOT NULL,
    handle      TEXT,
    direction   TEXT    NOT NULL CHECK (direction IN ('in', 'out')),
    address     TEXT    NOT NULL,
    sender_name TEXT,
    body        TEXT    NOT NULL,
    sent_at     TEXT,
    received_at INTEGER NOT NULL,
    status      TEXT    NOT NULL,
    UNIQUE (source, handle)
);
CREATE INDEX IF NOT EXISTS messages_address ON messages (address, received_at);

-- Names learned for addresses (e.g. by matching a notification to a MAP message).
CREATE TABLE IF NOT EXISTS contacts (
    address TEXT PRIMARY KEY,
    name    TEXT NOT NULL
);
"#;

/// Schema changes after the baseline above, applied in order and recorded in
/// `PRAGMA user_version`, so an existing install upgrades in place. Version 1 is
/// the baseline `SCHEMA`; entry `i` takes the database to version `i + 2`.
/// Append only — never edit a shipped migration.
const MIGRATIONS: &[&str] = &[
    // v2: full-text search over messages (for universal search), back-filled.
    r#"
    CREATE VIRTUAL TABLE messages_fts USING fts5 (body, content = 'messages', content_rowid = 'id');
    CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
        INSERT INTO messages_fts (rowid, body) VALUES (new.id, new.body);
    END;
    CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
        INSERT INTO messages_fts (messages_fts, rowid, body) VALUES ('delete', old.id, old.body);
    END;
    CREATE TRIGGER messages_au AFTER UPDATE OF body ON messages BEGIN
        INSERT INTO messages_fts (messages_fts, rowid, body) VALUES ('delete', old.id, old.body);
        INSERT INTO messages_fts (rowid, body) VALUES (new.id, new.body);
    END;
    INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');
    "#,
    // v3: whether the phone still lists the message as unread, so opening it in tug
    // marks it read on the phone once. Existing history counts as already read.
    // Plus an index of notifications still open, for the sweep after each reconnect
    // (a full scan took ~170 ms at 50k rows).
    r#"
    ALTER TABLE messages ADD COLUMN unread_on_phone INTEGER NOT NULL DEFAULT 0;
    CREATE INDEX notifications_open ON notifications (session) WHERE removed_at IS NULL;
    "#,
    // v4: names a contact used to have ("zoe 💜" before Dave renamed her "zoe"), so
    // notifications that arrived under an old name stay in the same conversation.
    r#"
    CREATE TABLE contact_aliases (
        address TEXT NOT NULL,
        alias   TEXT NOT NULL,
        PRIMARY KEY (address, alias)
    );
    CREATE INDEX contact_aliases_alias ON contact_aliases (alias);
    "#,
    // v5: conversations deleted in tug (local only, undoable). Rows are hidden rather than
    // removed: the phone re-sends its recent texts and on-screen notifications on every
    // reconnect, and a hidden row keeps those copies hidden instead of reappearing.
    r#"
    ALTER TABLE notifications ADD COLUMN hidden_at INTEGER;
    ALTER TABLE messages ADD COLUMN hidden_at INTEGER;
    "#,
];

/// A sender name as people see it, matching the UI's `cleanName` (format.ts): trimmed,
/// inner whitespace collapsed, and iOS's inline-reply suffix ("zoe replied to you",
/// "… replied to your message") removed from the end. Also callable from SQL.
pub(crate) fn clean_name(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    for suffix in [" replied to your message", " replied to you"] {
        let cut = name.len().wrapping_sub(suffix.len());
        if name.len() >= suffix.len() && name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(suffix) {
            return name[..cut].to_string();
        }
    }
    name
}

fn register_functions(conn: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "clean_name",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(clean_name(&ctx.get::<String>(0)?)),
    )
}

/// The schema version this build expects.
pub const SCHEMA_VERSION: i64 = 1 + MIGRATIONS.len() as i64;

fn migrate(conn: &mut Connection) -> Result<()> {
    let mut version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version == 0 {
        // Fresh database, or one from before versioning: the baseline is in place.
        conn.pragma_update(None, "user_version", 1)?;
        version = 1;
    }
    if version > SCHEMA_VERSION {
        log::warn!("database is schema v{version}, newer than this build (v{SCHEMA_VERSION}); not migrating");
        return Ok(());
    }
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let target = i as i64 + 2;
        if version < target {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
            log::info!("database migrated to schema v{target}");
            version = target;
        }
    }
    Ok(())
}

// A Messages notification that arrived under a contact's old name is shown under the
// current one, when the old name points at exactly one contact and nobody has it now.
const SELECT: &str = "SELECT n.id, n.session, n.uid, n.app_id, a.display_name, n.category,
            CASE WHEN n.app_id = 'com.apple.MobileSMS' THEN COALESCE((
                SELECT CASE WHEN COUNT(DISTINCT c.name) = 1 THEN MIN(c.name) END
                FROM contact_aliases ca JOIN contacts c ON c.address = ca.address
                WHERE lower(ca.alias) = lower(clean_name(n.title))
                  AND NOT EXISTS (SELECT 1 FROM contacts c2 WHERE lower(c2.name) = lower(ca.alias))
            ), n.title) ELSE n.title END,
            n.subtitle, n.message, n.posted_at, n.received_at, n.flags, n.positive_label, n.negative_label, n.removed_at,
            n.hidden_at
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
    /// Part of a conversation deleted in tug: kept so the phone's re-sent copy stays
    /// hidden, but never shown.
    #[serde(skip)]
    pub hidden: bool,
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

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        register_functions(&conn)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
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
                       AND session != ?6
                     ORDER BY id DESC LIMIT 1",
                    params![a.app_id, a.date, a.title, a.subtitle, a.message, n.session],
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
        let mut stmt = conn.prepare(&format!(
            "{SELECT} WHERE n.id < ?1 AND n.hidden_at IS NULL ORDER BY n.id DESC LIMIT ?2"
        ))?;
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
        let like = like_pattern(query);
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "{SELECT} WHERE (n.id IN (SELECT rowid FROM notifications_fts WHERE notifications_fts MATCH ?1)
                 OR a.display_name LIKE ?2 ESCAPE '\\' OR n.app_id LIKE ?2 ESCAPE '\\')
               AND n.hidden_at IS NULL
             ORDER BY n.id DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(params![fts, like, limit], |r| map_row(r, live_session))?;
        rows.collect()
    }

    /// Delete a conversation from tug (`Some(time)`), or undo that (`None`). Local only:
    /// nothing on the phone changes.
    pub fn set_hidden(&self, notifications: &[i64], messages: &[i64], at: Option<i64>) -> Result<()> {
        let ids = |v: &[i64]| serde_json::to_string(v).expect("ids serialize");
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE notifications SET hidden_at = ?2 WHERE id IN (SELECT value FROM json_each(?1))",
            params![ids(notifications), at],
        )?;
        tx.execute(
            "UPDATE messages SET hidden_at = ?2 WHERE id IN (SELECT value FROM json_each(?1))",
            params![ids(messages), at],
        )?;
        tx.commit()
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
        hidden: r.get::<_, Option<i64>>(15)?.is_some(),
    })
}

/// `%text%` for LIKE … ESCAPE '\', with the user's `\ % _` taken literally.
pub(crate) fn like_pattern(query: &str) -> String {
    let escaped = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// Turn free text into a safe FTS5 query: every token quoted, prefix-matched, ANDed.
pub(crate) fn fts_query(input: &str) -> Option<String> {
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
    fn clean_name_matches_the_ui() {
        assert_eq!(clean_name("  marco  "), "marco");
        assert_eq!(clean_name("zoe 💜 replied to you"), "zoe 💜");
        assert_eq!(clean_name("Zoe Replied To Your Message"), "Zoe");
        assert_eq!(clean_name("Mary  Ann"), "Mary Ann");
        assert_eq!(clean_name("Replied to you Club"), "Replied to you Club");
        assert_eq!(
            clean_name("replied to you"),
            "replied to you",
            "a bare phrase isn't a suffix"
        );
        assert_eq!(clean_name(""), "");
    }

    #[test]
    fn fresh_database_is_at_the_current_schema_version() {
        let s = Store::in_memory().unwrap();
        let v: i64 = s.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn existing_v1_database_upgrades_in_place_and_keeps_its_data() {
        // A database as shipped in v0.5.x: baseline schema, no version recorded.
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO messages (source, handle, direction, address, body, received_at, status)
             VALUES ('iphone-map', 'H1', 'in', '+13025550100', 'dinner at 7?', 1000, 'received')",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // The old message is searchable after the upgrade (back-filled index).
        let hits: i64 = conn
            .query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'dinner'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
        // Running again is a no-op.
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn a_deleted_conversation_stays_hidden_when_the_phone_resends_it() {
        let s = Store::in_memory().unwrap();
        let jane = insert(
            &s,
            "s1",
            1,
            EventFlags::default(),
            &attrs("com.apple.MobileSMS", "Jane", "dinner?"),
        );
        let sam = insert(
            &s,
            "s1",
            2,
            EventFlags::default(),
            &attrs("com.apple.MobileSMS", "Sam", "yo"),
        );
        s.set_hidden(&[jane.id], &[], Some(5_000)).unwrap();
        let ids = |rows: Vec<StoredNotification>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(s.recent(10, None, None).unwrap()), vec![sam.id]);
        assert!(s.search("dinner", 10, None).unwrap().is_empty(), "not in search either");
        // Reconnect: iOS replays it under a new session and UID; it must not come back.
        let replay = EventFlags {
            pre_existing: true,
            ..Default::default()
        };
        let again = insert(&s, "s2", 9, replay, &attrs("com.apple.MobileSMS", "Jane", "dinner?"));
        assert_eq!(again.id, jane.id);
        assert!(again.hidden, "the actor skips emitting it");
        assert_eq!(ids(s.recent(10, None, None).unwrap()), vec![sam.id]);
        // Undo.
        s.set_hidden(&[jane.id], &[], None).unwrap();
        assert_eq!(ids(s.recent(10, None, None).unwrap()), vec![sam.id, jane.id]);
        assert_eq!(ids(s.search("dinner", 10, None).unwrap()), vec![jane.id]);
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
    fn two_identical_notifications_both_survive_a_reconnect() {
        let s = Store::in_memory().unwrap();
        let a = attrs("com.example.promo", "Sale", "50% off today");
        let first = insert(&s, "s1", 1, EventFlags::default(), &a);
        let second = insert(&s, "s1", 2, EventFlags::default(), &a);
        assert_ne!(first.id, second.id);
        // Reconnect: iOS replays both as pre-existing with new UIDs.
        let pre = EventFlags {
            pre_existing: true,
            ..Default::default()
        };
        let r1 = insert(&s, "s2", 10, pre, &a);
        let r2 = insert(&s, "s2", 11, pre, &a);
        assert_ne!(r1.id, r2.id, "each replay binds to its own row");
        assert!(s.sweep_stale("s2", 9_000).unwrap().is_empty(), "neither is swept");
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
