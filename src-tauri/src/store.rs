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
    // v4: names a contact used to have (before a rename on the phone), so
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
    // v6: a reference (content hash) to the contact's photo from the phone (PBAP PHOTO), when it
    // shares one. The bytes live in files under <app data>/contacts; NULL means initials.
    r#"
    ALTER TABLE contacts ADD COLUMN photo TEXT;
    "#,
    // v7: the MAP message type the phone reported (SMS_GSM, SMS_CDMA, MMS, EMAIL, IM), so tug can
    // tell iMessage (IM) from a plain text. NULL for history from before this, and for sends.
    r#"
    ALTER TABLE messages ADD COLUMN msg_type TEXT;
    "#,
    // v8: names saved with invisible marks (WhatsApp/Snapchat prefix some titles with U+200E) are
    // cleaned, so they join the right conversation and a reconnect's replay matches its row
    // instead of saving a duplicate. New ones are cleaned as they arrive.
    r#"
    UPDATE notifications SET title = strip_invisible(title) WHERE title <> strip_invisible(title);
    UPDATE notifications SET subtitle = strip_invisible(subtitle) WHERE subtitle <> strip_invisible(subtitle);
    UPDATE messages SET sender_name = strip_invisible(sender_name) WHERE sender_name <> strip_invisible(sender_name);
    UPDATE contacts SET name = strip_invisible(name) WHERE name <> strip_invisible(name);
    UPDATE OR IGNORE contact_aliases SET alias = strip_invisible(alias) WHERE alias <> strip_invisible(alias);
    DELETE FROM contact_aliases WHERE alias <> strip_invisible(alias);
    "#,
    // v9: texts from before this one may be missing (a catch-up after time away where the phone's
    // short list was all new), so the conversation can say so instead of looking complete.
    r#"
    ALTER TABLE messages ADD COLUMN gap_before INTEGER NOT NULL DEFAULT 0;
    "#,
    // v10: aliases learned from group-text notifications ("Sam & Alex", "Sam, Alex & 2 others")
    // could retitle every notification from that group as one contact. The learners skip those
    // now; drop any already learned.
    r#"
    DELETE FROM contact_aliases WHERE group_like(alias);
    "#,
    // v11: the last 10 digits of a contact's number (`match_key`), so a text from the same number
    // in another format (national "07700 900123" vs the contact's "+44 7700 900123") still shows
    // the contact's name. A plain column, set on insert, so an older build can still write.
    r#"
    ALTER TABLE contacts ADD COLUMN match_key TEXT;
    UPDATE contacts SET match_key = match_key(address);
    CREATE INDEX contacts_match_key ON contacts (match_key);
    "#,
];

/// A sender name as people see it, matching the UI's `cleanName` (format.ts): trimmed,
/// inner whitespace collapsed, and iOS's inline-reply suffix ("zoe replied to you",
/// "… replied to your message") removed from the end. Also callable from SQL.
pub(crate) fn clean_name(name: &str) -> String {
    let name = crate::text::strip_invisible(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // TODO(localization): iOS rewrites an inline reply's title in the phone's language too, but
    // the exact localized phrases aren't confirmed, so only English is stripped (as in format.ts
    // `IOS_REPLY_SUFFIX`). Add a language here once a real iPhone shows its wording.
    for suffix in [" replied to your message", " replied to you"] {
        let cut = name.len().wrapping_sub(suffix.len());
        if name.len() >= suffix.len() && name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(suffix) {
            return name[..cut].to_string();
        }
    }
    name
}

/// Whether a notification title names several people, as iOS titles a group text ("Sam & Alex",
/// "Sam, Alex & Jo", "Sam & 2 others"). Such a title is never one contact's name. Also callable from
/// SQL as `group_like`.
pub(crate) fn looks_like_group(title: &str) -> bool {
    let t = clean_name(title).to_lowercase();
    if t.contains('&') || t.contains(',') || t.contains(" others") {
        return true;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    // "Sam y 2 más", "Sam et 2 autres", "Sam und 2 weitere", "Sam e altri 2": a count and "others".
    let has_count = words.iter().any(|w| w.chars().all(|c| c.is_ascii_digit()));
    if has_count && words.iter().any(|w| OTHERS_WORDS.contains(w)) {
        return true;
    }
    // "Sam y Alex", "Sam et Alex", "Sam und Alex": two single names joined, nothing else. Kept to
    // one word each side so a name with a joiner in it ("José Ortega y Gasset") stays a name.
    matches!(words.as_slice(), [_, joiner, _] if GROUP_JOINERS.contains(joiner))
}

/// "and" in the iPhone's major languages, as a two-person group title joins the names.
const GROUP_JOINERS: &[&str] = &["and", "y", "et", "und", "e", "en", "och", "og", "i", "ve"];
/// "others"/"more" as a group title counts the rest ("Sam & 2 others").
const OTHERS_WORDS: &[&str] = &[
    "others",
    "more",
    "más",
    "mas",
    "autres",
    "weitere",
    "weiteren",
    "andere",
    "anderen",
    "altri",
    "outros",
    "outras",
    "mais",
    "andra",
    "andre",
    "innych",
    "inne",
    "kişi",
    "другие",
    "других",
];

/// The key that matches one phone number across formats: its last 10 digits, when it has at least
/// 10 (emails and short codes have none, and only match exactly). Also callable from SQL.
pub(crate) fn match_key(address: &str) -> Option<String> {
    if address.contains('@') {
        return None;
    }
    let digits: Vec<char> = address.chars().filter(char::is_ascii_digit).collect();
    (digits.len() >= 10).then(|| digits[digits.len() - 10..].iter().collect())
}

/// How two names are compared (matching the UI's `nameKey`, format.ts): the cleaned name,
/// lower-cased, without emoji variation selectors, so "sam ❤" and "sam ❤️" are one person.
pub(crate) fn name_key(name: &str) -> String {
    clean_name(name)
        .chars()
        .filter(|c| !matches!(c, '\u{FE0E}' | '\u{FE0F}'))
        .collect::<String>()
        .to_lowercase()
}

fn register_functions(conn: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    conn.create_scalar_function("clean_name", 1, flags, |ctx| Ok(clean_name(&ctx.get::<String>(0)?)))?;
    conn.create_scalar_function("name_key", 1, flags, |ctx| {
        Ok(ctx.get::<Option<String>>(0)?.map(|s| name_key(&s)))
    })?;
    conn.create_scalar_function("match_key", 1, flags, |ctx| {
        Ok(ctx.get::<Option<String>>(0)?.and_then(|s| match_key(&s)))
    })?;
    conn.create_scalar_function("group_like", 1, flags, |ctx| {
        Ok(ctx.get::<Option<String>>(0)?.is_some_and(|s| looks_like_group(&s)))
    })?;
    conn.create_scalar_function("strip_invisible", 1, flags, |ctx| {
        Ok(ctx.get::<Option<String>>(0)?.map(|s| crate::text::strip_invisible(&s)))
    })
}

/// The schema version this build expects.
pub const SCHEMA_VERSION: i64 = 1 + MIGRATIONS.len() as i64;

fn migrate(conn: &mut Connection) -> Result<()> {
    // Migrations may call tug's SQL functions (v8 uses strip_invisible).
    register_functions(conn)?;
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
                WHERE name_key(ca.alias) = name_key(n.title)
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
    /// This upsert inserted the row: tug had never stored this notification before. False on an
    /// update and on every read. iOS flags everything it replays after a resubscribe as
    /// pre-existing, including what arrived while the link was down, so this is how the window
    /// tells "new to tug" apart from "seen before, replayed" (and pops up only the former).
    pub fresh: bool,
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
        let fresh = existing.is_none();
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
        let mut stored = conn.query_row(&format!("{SELECT} WHERE n.id = ?1"), [id], |r| {
            map_row(r, Some(n.session))
        })?;
        stored.fresh = fresh;
        Ok(stored)
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

    /// One notification by row id (pop-up presses check what it is before acting).
    #[cfg_attr(not(windows), allow(dead_code))]
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
    /// Hide (or, with `at: None`, unhide) rows by id. The app deletes whole conversations with
    /// `set_conversation_hidden`; this is the row-level primitive the hidden-row tests use.
    #[cfg(test)]
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

    /// Delete a conversation from tug (`hidden: true`), or undo that delete (`hidden: false`
    /// with the same `at`). A conversation is every notification from one of `senders`
    /// ((app id, title) pairs, compared by app id and `name_key` of the title, so padding, case
    /// and the inline-reply suffix don't matter) plus every text to or from one of `addresses`.
    ///
    /// By key rather than by row id: the window only has the newest pages loaded, and hiding
    /// just those let the older part of a deleted conversation come back in Feed scroll and
    /// search. Hiding stamps rows stored up to `at` that weren't already hidden; undo clears
    /// exactly the rows stamped `at`, so an earlier delete of the same person stays deleted.
    pub fn set_conversation_hidden(
        &self,
        senders: &[(String, String)],
        addresses: &[String],
        at: i64,
        hidden: bool,
    ) -> Result<()> {
        let senders = serde_json::to_string(senders).expect("senders serialize");
        let addresses = serde_json::to_string(addresses).expect("addresses serialize");
        let (set, which) = if hidden {
            ("?2", "hidden_at IS NULL AND received_at <= ?2")
        } else {
            ("NULL", "hidden_at = ?2")
        };
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            &format!(
                "UPDATE notifications SET hidden_at = {set}
                 WHERE {which} AND EXISTS (
                     SELECT 1 FROM json_each(?1) s
                     WHERE json_extract(s.value, '$[0]') = notifications.app_id
                       AND name_key(json_extract(s.value, '$[1]')) = name_key(notifications.title))"
            ),
            params![senders, at],
        )?;
        tx.execute(
            &format!(
                "UPDATE messages SET hidden_at = {set}
                 WHERE {which} AND address IN (SELECT value FROM json_each(?1))"
            ),
            params![addresses, at],
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

    /// Settings › Data & privacy › Clear history: everything tug copied from the phone's activity
    /// (notifications, texts and their search index, the recent-calls list, names learned from
    /// notifications) goes. Settings, the pairing and the phone's contacts stay; the iPhone keeps
    /// its own. Texts the phone still lists come back at the next sync, as on a fresh install.
    pub fn clear_history(&self) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM notifications;
             INSERT INTO notifications_fts (notifications_fts) VALUES ('rebuild');
             DELETE FROM messages;
             INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');
             DELETE FROM contact_aliases;",
        )?;
        tx.execute(
            "DELETE FROM settings WHERE key IN (?1, ?2)",
            params![crate::map::calls::RECENT_CALLS, crate::state::keys::LAST_TEXT_SYNC],
        )?;
        tx.commit()
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
        fresh: false,
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
        // Localized reply suffixes aren't guessed at (see the TODO in clean_name): left as sent.
        assert_eq!(clean_name("Zoé a répondu"), "Zoé a répondu");
    }

    #[test]
    fn clear_history_clears_what_it_promises_and_keeps_the_rest() {
        let s = Store::in_memory().unwrap();
        insert(
            &s,
            "s1",
            1,
            EventFlags::default(),
            &attrs("com.apple.MobileSMS", "Jane", "running late"),
        );
        s.conn()
            .execute_batch(
                "INSERT INTO messages (source, handle, direction, address, body, received_at, status)
                     VALUES ('iphone-map', 'h1', 'in', '+15550100001', 'see you at noon', 1, 'received');
                 INSERT INTO contacts (address, name) VALUES ('+15550100001', 'Jane Doe');
                 INSERT INTO contact_aliases (address, alias) VALUES ('+15550100001', 'Janey');",
            )
            .unwrap();
        s.set_setting(crate::map::calls::RECENT_CALLS, "[]").unwrap();
        s.set_setting(crate::state::keys::LAST_TEXT_SYNC, "1").unwrap();
        s.set_setting(crate::state::keys::DEVICE_ID, "phone").unwrap();
        s.set_setting("toasts", "false").unwrap();

        s.clear_history().unwrap();

        let count = |sql: &str| s.conn().query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT count(*) FROM notifications"), 0);
        assert_eq!(count("SELECT count(*) FROM messages"), 0);
        assert_eq!(
            count("SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'noon'"),
            0
        );
        assert_eq!(
            count("SELECT count(*) FROM notifications_fts WHERE notifications_fts MATCH 'late'"),
            0
        );
        assert_eq!(count("SELECT count(*) FROM contact_aliases"), 0);
        assert_eq!(s.setting(crate::map::calls::RECENT_CALLS).unwrap(), None);
        assert_eq!(s.setting(crate::state::keys::LAST_TEXT_SYNC).unwrap(), None);
        // Kept: the phone's contacts, the pairing and settings.
        assert_eq!(count("SELECT count(*) FROM contacts"), 1);
        assert_eq!(
            s.setting(crate::state::keys::DEVICE_ID).unwrap().as_deref(),
            Some("phone")
        );
        assert_eq!(s.setting("toasts").unwrap().as_deref(), Some("false"));
    }

    #[test]
    fn group_titles_in_other_languages() {
        for title in [
            "Sam & Alex",
            "Sam, Alex & Jo",
            "Sam & 2 others",
            "Sam y Alex",
            "Sam et Alex",
            "Sam und Alex",
            "Sam e Alex",
            "Sam en Alex",
            "Sam and Alex",
            "Sam y 2 más",
            "Sam et 2 autres",
            "Sam und 2 weitere",
            "Sam e altri 2",
            "Sam e mais 2",
            "Sam en 2 anderen",
        ] {
            assert!(looks_like_group(title), "{title}");
        }
        for name in [
            "Sam",
            "Sam Lee",
            "José Ortega y Gasset",
            "Mary Ann",
            "Elena 2",
            "Yves",
            "Sam e",
            "Y Alex",
        ] {
            assert!(!looks_like_group(name), "{name}");
        }
    }

    #[test]
    fn upsert_reports_whether_the_row_is_new_to_tug() {
        let s = Store::in_memory().unwrap();
        let replay = EventFlags {
            pre_existing: true,
            ..Default::default()
        };
        let text = attrs("com.apple.MobileSMS", "Jane", "running late");
        // First sighting, live: new.
        let first = insert(&s, "s1", 1, EventFlags::default(), &text);
        assert!(first.fresh);
        // The same UID updated in the same session: not new.
        assert!(!insert(&s, "s1", 1, EventFlags::default(), &text).fresh);
        // Replayed after a resubscribe (new session, new UID, same content): seen before, not new.
        let again = insert(&s, "s2", 7, replay, &text);
        assert_eq!(again.id, first.id);
        assert!(!again.fresh);
        // A text that arrived while the link was down: iOS flags it pre-existing too, but tug
        // never stored it, so it's new.
        let gap = insert(&s, "s2", 8, replay, &attrs("com.apple.MobileSMS", "Jane", "here now"));
        assert!(gap.fresh);
        assert!(gap.flags.pre_existing);
        // Reads never claim new.
        assert!(s.recent(10, None, None).unwrap().iter().all(|n| !n.fresh));
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
    fn names_lose_invisible_marks_and_compare_without_emoji_variants() {
        assert_eq!(clean_name("\u{200E}sam \u{2764}\u{FE0F}"), "sam \u{2764}\u{FE0F}");
        assert_eq!(name_key("\u{200E}sam \u{2764}\u{FE0F}"), name_key("Sam \u{2764}"));
        assert_ne!(name_key("sam \u{2764}"), name_key("sam"));
    }

    /// Back to a real older database: undo the schema later migrations added, so they apply again.
    fn rewind(conn: &Connection, version: i64) {
        if version < 11 {
            conn.execute_batch("DROP INDEX contacts_match_key; ALTER TABLE contacts DROP COLUMN match_key;")
                .unwrap();
        }
        if version < 9 {
            conn.execute_batch("ALTER TABLE messages DROP COLUMN gap_before")
                .unwrap();
        }
        conn.pragma_update(None, "user_version", version).unwrap();
    }

    #[test]
    fn match_key_is_the_last_ten_digits_of_a_real_number() {
        assert_eq!(match_key("+447700900123").as_deref(), Some("7700900123"));
        assert_eq!(match_key("07700900123").as_deref(), Some("7700900123"));
        assert_eq!(match_key("+13025550173").as_deref(), Some("3025550173"));
        assert_eq!(match_key("12345"), None, "short codes match exactly only");
        assert_eq!(match_key("ana1234567890@example.com"), None);
    }

    #[test]
    fn group_titles_are_recognised() {
        for t in [
            "Sam & Alex",
            "Sam, Alex",
            "Sam & 2 others",
            "Sam, Alex & 1 other",
            "\u{200E}Sam & Jo",
        ] {
            assert!(looks_like_group(t), "{t}");
        }
        for t in ["Sam", "zoe \u{1F49C}", "Dr Other", "Mothers Day"] {
            assert!(!looks_like_group(t), "{t}");
        }
    }

    #[test]
    fn upgrading_drops_aliases_learned_from_group_titles() {
        let s = Store::in_memory().unwrap();
        {
            let conn = s.conn();
            conn.execute_batch(
                "INSERT INTO contact_aliases (address, alias) VALUES
                     ('+13025550100', 'Sam & Alex'), ('+13025550100', 'Sam, Alex & 2 others'),
                     ('+13025550100', 'Sammy');",
            )
            .unwrap();
            rewind(&conn, 9);
        }
        migrate(&mut s.conn()).unwrap();
        let left: Vec<String> = {
            let conn = s.conn();
            let mut stmt = conn.prepare("SELECT alias FROM contact_aliases").unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.collect::<Result<_>>().unwrap()
        };
        assert_eq!(left, vec!["Sammy".to_string()]);
    }

    #[test]
    fn a_title_saved_with_an_invisible_mark_is_cleaned_and_its_replay_reuses_the_row() {
        // Rows saved before v8 kept WhatsApp's U+200E; the upgrade cleans them so the phone's
        // replay on the next reconnect (now cleaned on arrival) matches instead of duplicating.
        let s = Store::in_memory().unwrap();
        let marked = attrs("net.whatsapp.WhatsApp", "\u{200E}sam \u{2764}\u{FE0F}", "good night");
        let first = {
            let conn = s.conn();
            conn.execute(
                "INSERT INTO notifications (session, uid, app_id, category, title, subtitle, message, posted_at, received_at, flags)
                 VALUES ('s1', 4, ?1, 4, ?2, '', ?3, ?4, 1000, 0)",
                params![marked.app_id, marked.title, marked.message, marked.date],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            rewind(&conn, 7);
            id
        };
        migrate(&mut s.conn()).unwrap();
        let title: String = s
            .conn()
            .query_row("SELECT title FROM notifications WHERE id = ?1", [first], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "sam \u{2764}\u{FE0F}");
        let pre = EventFlags {
            pre_existing: true,
            ..Default::default()
        };
        let clean = attrs("net.whatsapp.WhatsApp", "sam \u{2764}\u{FE0F}", "good night");
        let again = insert(&s, "s2", 9, pre, &clean);
        assert_eq!(again.id, first);
        assert_eq!(s.recent(10, None, None).unwrap().len(), 1);
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
