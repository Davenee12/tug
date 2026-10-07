//! Conversation messages from connectors (first: the iPhone over Bluetooth MAP),
//! stored next to notifications. History grows past the phone's small MAP
//! window because every message seen is kept.

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::store::{fts_query, like_pattern, Result, Store};

pub const SOURCE_IPHONE_MAP: &str = "iphone-map";

/// Notifications from this app carry the same text as MAP messages, which is how
/// tug learns which contact name belongs to which number.
const MESSAGES_APP: &str = "com.apple.MobileSMS";

/// A notification and a MAP message are the same text only if they arrived this
/// close together (the notification triggers the MAP fetch).
const LEARN_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Handles are stable across sessions (verified on hardware), so content-based
/// dedupe only guards against a phone re-listing an old message under a new
/// handle. A row stored this recently is a different message, not a re-listing.
const RELIST_GUARD_MS: i64 = 60 * 1000;

/// A `SendingSuccess`/`SendingFailure` report arrives within seconds of the send it's about. Only
/// sends this recent are candidates, so an old send that never got a report (stuck at `accepted`)
/// can't absorb a stray report meant for a newer one.
pub const CONFIRM_WINDOW_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
        }
    }
}

/// `received` for incoming; outgoing goes `pending` → `accepted` (taken by the iPhone for
/// sending; not proof of delivery) → `sent` (the phone's MAP `SendingSuccess` event confirmed
/// it left), or `failed` (the push failed, or a `SendingFailure` event came back). Without live
/// texts (`map::mns`) a send stops at `accepted`; the UI shows both as "Sent". `unconfirmed`: the
/// whole text went out but the phone's answer never came, so it may have sent ("check your
/// iPhone"); never retried automatically, and a later send report still settles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Received,
    Pending,
    Accepted,
    Sent,
    Failed,
    Unconfirmed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Received => "received",
            Status::Pending => "pending",
            Status::Accepted => "accepted",
            Status::Sent => "sent",
            Status::Failed => "failed",
            Status::Unconfirmed => "unconfirmed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "pending" => Status::Pending,
            "accepted" => Status::Accepted,
            "sent" => Status::Sent,
            "failed" => Status::Failed,
            "unconfirmed" => Status::Unconfirmed,
            _ => Status::Received,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: i64,
    pub source: String,
    pub direction: Direction,
    pub address: String,
    /// Learned contact name for the address, if any.
    pub contact_name: Option<String>,
    pub body: String,
    /// Phone-local ISO time from the phone, when known.
    pub sent_at: Option<String>,
    pub received_at: i64,
    pub status: Status,
    /// The MAP message type the phone reported (SMS_GSM, SMS_CDMA, MMS, EMAIL, IM); `None` for
    /// history from before tug stored it, and for messages tug sent.
    pub msg_type: Option<String>,
    /// Texts from before this one may be missing: it arrived in a catch-up where every text the
    /// phone listed was new, so the phone's short list (iOS lists about 10) may have cut off older
    /// ones from while tug was away. The UI says so above it.
    pub gap_before: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub address: String,
    pub name: String,
}

pub struct IncomingMessage<'a> {
    pub source: &'a str,
    pub handle: &'a str,
    pub address: &'a str,
    pub sender_name: Option<&'a str>,
    pub body: &'a str,
    pub sent_at: Option<&'a str>,
    pub received_at: i64,
    /// The phone lists it as unread.
    pub unread_on_phone: bool,
    /// The listing's `type` attribute (SMS_GSM, IM, …); `None` when the phone didn't give one.
    pub msg_type: Option<&'a str>,
}

// A name the phone sent with the message stands in until the number is a known contact.
const SELECT: &str = "SELECT m.id, m.source, m.direction, m.address, COALESCE(c.name, m.sender_name), m.body, m.sent_at, m.received_at, m.status, m.msg_type, m.gap_before
     FROM messages m LEFT JOIN contacts c ON c.address = m.address";

fn map_row(r: &Row) -> Result<StoredMessage> {
    let direction: String = r.get(2)?;
    let status: String = r.get(8)?;
    Ok(StoredMessage {
        id: r.get(0)?,
        source: r.get(1)?,
        direction: if direction == "out" {
            Direction::Out
        } else {
            Direction::In
        },
        address: r.get(3)?,
        contact_name: r.get(4)?,
        body: r.get(5)?,
        sent_at: r.get(6)?,
        received_at: r.get(7)?,
        status: Status::parse(&status),
        msg_type: r.get(9)?,
        gap_before: r.get(10)?,
    })
}

impl Store {
    pub fn has_message(&self, source: &str, handle: &str) -> Result<bool> {
        self.conn()
            .query_row(
                "SELECT 1 FROM messages WHERE source = ?1 AND handle = ?2",
                params![source, handle],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
    }

    /// Store an incoming message; `None` if this handle was already stored.
    pub fn insert_incoming(&self, m: &IncomingMessage) -> Result<Option<StoredMessage>> {
        let conn = self.conn();
        // Guard against the phone re-listing an *old* message under a new handle:
        // same sender, text and phone timestamp, stored a while ago. Two genuinely
        // identical texts ("ok", "ok") arrive close together and are both kept, and
        // without a phone timestamp there's nothing reliable to compare.
        let duplicate = m.sent_at.is_some()
            && conn
                .query_row(
                    "SELECT 1 FROM messages
                     WHERE source = ?1 AND direction = 'in' AND address = ?2 AND body = ?3 AND sent_at = ?4
                       AND received_at < ?5",
                    params![m.source, m.address, m.body, m.sent_at, m.received_at - RELIST_GUARD_MS],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
        if duplicate {
            return Ok(None);
        }
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO messages (source, handle, direction, address, sender_name, body, sent_at, received_at, status, unread_on_phone, msg_type)
             VALUES (?1, ?2, 'in', ?3, ?4, ?5, ?6, ?7, 'received', ?8, ?9)",
            params![
                m.source,
                m.handle,
                m.address,
                m.sender_name,
                m.body,
                m.sent_at,
                m.received_at,
                m.unread_on_phone,
                m.msg_type
            ],
        )?;
        if inserted == 0 {
            return Ok(None);
        }
        let id = conn.last_insert_rowid();
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
            .map(Some)
    }

    /// Handles of the given incoming messages that the phone still lists as unread.
    pub fn unread_on_phone(&self, source: &str, ids: &[i64]) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT handle FROM messages
             WHERE id IN (SELECT value FROM json_each(?1)) AND source = ?2
               AND direction = 'in' AND unread_on_phone = 1 AND handle IS NOT NULL",
        )?;
        let ids = serde_json::to_string(ids).expect("ids serialize");
        let rows = stmt.query_map(params![ids, source], |r| r.get(0))?;
        rows.collect()
    }

    /// The phone now lists this message as read (marked by tug, or read on the phone).
    pub fn set_read_on_phone(&self, source: &str, handle: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE messages SET unread_on_phone = 0 WHERE source = ?1 AND handle = ?2 AND unread_on_phone = 1",
            params![source, handle],
        )?;
        Ok(())
    }

    /// Record a reply before sending it, so it shows immediately as pending.
    pub fn insert_outgoing(&self, source: &str, address: &str, body: &str, at: i64) -> Result<StoredMessage> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO messages (source, direction, address, body, received_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                source,
                Direction::Out.as_str(),
                address,
                body,
                at,
                Status::Pending.as_str()
            ],
        )?;
        let id = conn.last_insert_rowid();
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
    }

    pub fn set_outgoing_status(&self, id: i64, status: Status, handle: Option<&str>) -> Result<StoredMessage> {
        let conn = self.conn();
        let update = |handle: Option<&str>| {
            conn.execute(
                "UPDATE messages SET status = ?2, handle = COALESCE(?3, handle) WHERE id = ?1 AND direction = 'out'",
                params![id, status.as_str(), handle],
            )
        };
        match update(handle) {
            // The phone handed back a handle another stored message already has. The send
            // itself worked, so record the status without the handle rather than leaving the
            // message stuck on "Sending…".
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
                log::warn!("phone reused message handle {handle:?}; keeping the sent message without it");
                update(None)?;
            }
            other => {
                other?;
            }
        }
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
    }

    /// Outgoing messages not yet confirmed sent (still `pending` or `accepted`) and sent within
    /// `CONFIRM_WINDOW_MS` of `now`, for matching a MAP `Sending{Success,Failure}` event to the send
    /// it reports (see `map::mns::choose_outgoing`). Returns `(id, handle, received_at)`.
    pub fn outgoing_unconfirmed(&self, source: &str, now: i64) -> Result<Vec<(i64, Option<String>, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, handle, received_at FROM messages
             WHERE source = ?1 AND direction = 'out' AND status IN ('pending', 'accepted', 'unconfirmed')
               AND received_at > ?2",
        )?;
        let rows = stmt.query_map(params![source, now - CONFIRM_WINDOW_MS], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.collect()
    }

    /// At startup nothing is sending, so a send still `pending` was cut off by a crash or quit
    /// before the phone answered. Mark it failed (with Retry) instead of "Sending..." forever.
    /// Returns how many were marked.
    pub fn fail_interrupted_sends(&self) -> Result<usize> {
        self.conn().execute(
            "UPDATE messages SET status = 'failed' WHERE direction = 'out' AND status = 'pending'",
            [],
        )
    }

    /// Put a failed send back to `pending` for another try of the *same* message (same row, same
    /// number), dated `at` so a send report for it falls inside `CONFIRM_WINDOW_MS`. `None` if the
    /// row isn't a failed outgoing message (already retried, or not one of ours).
    pub fn retry_outgoing(&self, id: i64, at: i64) -> Result<Option<StoredMessage>> {
        let conn = self.conn();
        let n = conn.execute(
            "UPDATE messages SET status = 'pending', handle = NULL, received_at = ?2
             WHERE id = ?1 AND direction = 'out' AND status = 'failed'",
            params![id, at],
        )?;
        if n == 0 {
            return Ok(None);
        }
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
            .map(Some)
    }

    /// One outgoing message by id; `None` if there's no such outgoing row.
    pub fn outgoing(&self, id: i64) -> Result<Option<StoredMessage>> {
        self.conn()
            .query_row(
                &format!("{SELECT} WHERE m.id = ?1 AND m.direction = 'out'"),
                [id],
                map_row,
            )
            .optional()
    }

    /// Flag a message as the first one after a possible gap (see `StoredMessage::gap_before`).
    pub fn mark_gap_before(&self, id: i64) -> Result<StoredMessage> {
        let conn = self.conn();
        conn.execute("UPDATE messages SET gap_before = 1 WHERE id = ?1", [id])?;
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
    }

    /// Most recent messages, oldest first within the window.
    pub fn recent_messages(&self, limit: u32) -> Result<Vec<StoredMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM ({SELECT} WHERE m.hidden_at IS NULL ORDER BY m.received_at DESC, m.id DESC LIMIT ?1)
             ORDER BY received_at, id"
        ))?;
        let rows = stmt.query_map([limit], map_row)?;
        rows.collect()
    }

    /// Messages whose text matches every word (prefix match), newest first.
    pub fn search_messages(&self, query: &str, limit: u32) -> Result<Vec<StoredMessage>> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "{SELECT} WHERE m.id IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1)
               AND m.hidden_at IS NULL
             ORDER BY m.received_at DESC, m.id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![fts, limit], map_row)?;
        rows.collect()
    }

    /// Contacts whose name contains the text, or whose number contains the digits typed.
    pub fn search_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let digits: String = q.chars().filter(char::is_ascii_digit).collect();
        // Fewer than 3 digits would match almost every number; use a pattern that can't match.
        let number_pattern = if digits.len() >= 3 {
            format!("%{digits}%")
        } else {
            "\u{0}".into()
        };
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT address, name FROM contacts
             WHERE name LIKE ?1 ESCAPE '\\' OR address LIKE ?2
             ORDER BY name LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![like_pattern(q), number_pattern, limit], |r| {
            Ok(Contact {
                address: r.get(0)?,
                name: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn contacts(&self) -> Result<Vec<Contact>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT address, name FROM contacts ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(Contact {
                address: r.get(0)?,
                name: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// Store names from the phone's own contacts (PBAP). These win over names learned from
    /// notifications. Each entry is `(address, name)`. Returns how many numbers were saved.
    ///
    /// Names and photos are saved by two separate PBAP passes: this fast one (names and numbers,
    /// no photos) and the slower background photo pass (`update_contact_photos`). So this never
    /// touches the `photo` column — a new contact starts with no photo (initials), and an existing
    /// one keeps the reference the photo pass gave it rather than having it wiped every fast sync.
    pub fn save_phonebook(&self, entries: &[(String, String)]) -> Result<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut n = 0;
        {
            // A renamed contact keeps their old name as an alias, so history under it stays theirs.
            let mut remember = tx.prepare(
                "INSERT OR IGNORE INTO contact_aliases (address, alias)
                 SELECT address, name FROM contacts WHERE address = ?1 AND name <> ?2",
            )?;
            let mut forget = tx.prepare("DELETE FROM contact_aliases WHERE address = ?1 AND alias = ?2")?;
            let mut stmt = tx.prepare(
                "INSERT INTO contacts (address, name) VALUES (?1, ?2)
                 ON CONFLICT (address) DO UPDATE SET name = excluded.name",
            )?;
            for (address, name) in entries {
                remember.execute(params![address, name])?;
                forget.execute(params![address, name])?;
                n += stmt.execute(params![address, name])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// Update only the photo reference of contacts the phone already gave us a name for (the fast
    /// `save_phonebook` runs first), keyed by number. Each entry is `(address, photo)`, where
    /// `photo` is the key of a stored photo file (`crate::contact_photos`) or `None`. This pass is
    /// authoritative over photos: a `None` clears a reference the contact no longer has, so the
    /// caller runs the orphan-file cleanup afterwards. A number with no contact row is skipped
    /// (nothing to attach a face to yet). Never adds, removes or renames contacts.
    pub fn update_contact_photos(&self, entries: &[(String, Option<String>)]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE contacts SET photo = ?2 WHERE address = ?1")?;
            for (address, photo) in entries {
                stmt.execute(params![address, photo])?;
            }
        }
        tx.commit()
    }

    /// The photo reference for a contact, looked up by phone number or by name. A number must match
    /// an address exactly (the caller normalises it first); a name matches case-insensitively and
    /// only when every contact of that name shares one photo, so two different people with the same
    /// name never borrow each other's face. `None` when there's no photo to show.
    pub fn contact_photo_key(&self, number: Option<&str>, name: Option<&str>) -> Result<Option<String>> {
        let conn = self.conn();
        if let Some(number) = number {
            let by_number: Option<String> = conn
                .query_row(
                    "SELECT photo FROM contacts WHERE address = ?1 AND photo IS NOT NULL",
                    params![number],
                    |r| r.get(0),
                )
                .optional()?;
            if by_number.is_some() {
                return Ok(by_number);
            }
        }
        if let Some(name) = name {
            return conn.query_row(
                "SELECT CASE WHEN COUNT(DISTINCT photo) = 1 THEN MIN(photo) END
                 FROM contacts WHERE name_key(name) = name_key(?1) AND photo IS NOT NULL",
                params![name],
                |r| r.get(0),
            );
        }
        Ok(None)
    }

    /// Every photo reference the contacts table still points at, for cleaning up orphaned files.
    pub fn photo_keys(&self) -> Result<std::collections::HashSet<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT DISTINCT photo FROM contacts WHERE photo IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    }

    /// Learn old names of known contacts from history: Messages notifications whose text
    /// matches messages from that contact, close in time, under a name that differs from
    /// the contact's current one. Covers renames that happened before tug kept aliases.
    ///
    /// Stricter than `learn_contacts`, because a wrong alias moves someone else's
    /// notifications into this contact's conversation: each text must have come from
    /// exactly one stored sender *and* appear under exactly one name, and the pairing must
    /// hold for at least two different texts. (One shared "ok" from an unsaved sender whose
    /// own text never reached tug is a coincidence, not a rename.) Returns how many were learned.
    pub fn learn_aliases(&self) -> Result<usize> {
        self.conn().execute(
            "INSERT OR IGNORE INTO contact_aliases (address, alias)
             SELECT address, alias FROM (
                 SELECT m.address AS address, clean_name(n.title) AS alias, COUNT(DISTINCT m.body) AS evidence
                 FROM messages m
                 JOIN contacts c ON c.address = m.address
                 JOIN notifications n
                   ON n.app_id = ?1 AND n.message = m.body AND abs(n.received_at - m.received_at) <= ?2
                 WHERE m.direction = 'in' AND m.body <> ''
                   AND clean_name(n.title) <> '' AND name_key(n.title) <> name_key(c.name)
                   AND (SELECT COUNT(DISTINCT m2.address) FROM messages m2
                        WHERE m2.direction = 'in' AND m2.body = m.body) = 1
                   AND (SELECT COUNT(DISTINCT clean_name(n2.title)) FROM notifications n2
                        WHERE n2.app_id = ?1 AND n2.message = m.body) = 1
                 GROUP BY m.address, alias
             )
             WHERE evidence >= 2",
            params![MESSAGES_APP, LEARN_WINDOW_MS],
        )
    }

    /// Learn names for addresses that have none yet, by finding a Messages
    /// notification with the same text as an incoming message, arriving close in
    /// time. Only unambiguous matches count: the text must have come from exactly
    /// one sender and match exactly one name — otherwise two people who both sent
    /// "ok" could swap names. Returns the newly learned contacts.
    pub fn learn_contacts(&self) -> Result<Vec<Contact>> {
        let conn = self.conn();
        // Names as the UI shows them (clean_name mirrors format.ts cleanName).
        let mut stmt = conn.prepare(
            "WITH titled AS (
                 SELECT message, received_at, clean_name(title) AS name FROM notifications WHERE app_id = ?1
             )
             INSERT OR IGNORE INTO contacts (address, name)
             SELECT address, name FROM (
                 SELECT m.address AS address, MIN(n.name) AS name, COUNT(DISTINCT n.name) AS names
                 FROM messages m
                 JOIN titled n
                   ON n.message = m.body AND n.name <> '' AND abs(n.received_at - m.received_at) <= ?2
                 WHERE m.direction = 'in' AND m.body <> ''
                   AND m.address NOT IN (SELECT address FROM contacts)
                   AND (SELECT COUNT(DISTINCT m2.address) FROM messages m2
                        WHERE m2.direction = 'in' AND m2.body = m.body) = 1
                   AND (SELECT COUNT(DISTINCT n2.name) FROM titled n2 WHERE n2.message = m.body) = 1
                 GROUP BY m.address
             )
             WHERE names = 1
             RETURNING address, name",
        )?;
        let rows = stmt.query_map(params![MESSAGES_APP, LEARN_WINDOW_MS], |r| {
            Ok(Contact {
                address: r.get(0)?,
                name: r.get(1)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ancs::{Category, EventFlags, NotificationAttributes};
    use crate::store::NewNotification;

    fn incoming<'a>(handle: &'a str, address: &'a str, body: &'a str) -> IncomingMessage<'a> {
        IncomingMessage {
            source: SOURCE_IPHONE_MAP,
            handle,
            address,
            sender_name: None,
            body,
            sent_at: Some("2026-10-04T19:11:17"),
            received_at: 1_000,
            unread_on_phone: false,
            msg_type: None,
        }
    }

    fn notify(s: &Store, uid: u32, title: &str, message: &str, at: i64) {
        let attrs = NotificationAttributes {
            app_id: MESSAGES_APP.into(),
            title: title.into(),
            message: message.into(),
            ..Default::default()
        };
        s.upsert_notification(&NewNotification {
            session: "s1",
            uid,
            category: Category::Social,
            flags: EventFlags::default(),
            attrs: &attrs,
            received_at: at,
        })
        .unwrap();
    }

    #[test]
    fn identical_texts_close_together_are_both_kept() {
        let s = Store::in_memory().unwrap();
        // Two "ok"s in the same clock second: different handles, both real.
        assert!(s
            .insert_incoming(&incoming("H1", "+13025550100", "ok"))
            .unwrap()
            .is_some());
        let mut second = incoming("H2", "+13025550100", "ok");
        second.received_at = 1_005;
        assert!(
            s.insert_incoming(&second).unwrap().is_some(),
            "second identical text kept"
        );
        // Without a phone timestamp there's nothing to compare: rely on handles.
        let mut undated = incoming("H3", "+13025550100", "ok");
        undated.sent_at = None;
        assert!(s.insert_incoming(&undated).unwrap().is_some());
    }

    #[test]
    fn stores_and_returns_the_message_type() {
        let s = Store::in_memory().unwrap();
        let imessage = IncomingMessage {
            msg_type: Some("IM"),
            ..incoming("H1", "+13025550100", "sent blue")
        };
        let stored = s.insert_incoming(&imessage).unwrap().unwrap();
        assert_eq!(stored.msg_type.as_deref(), Some("IM"));
        // A plain SMS, and one the phone didn't type, round-trip too.
        let sms = IncomingMessage {
            msg_type: Some("SMS_GSM"),
            ..incoming("H2", "+13025550100", "sent green")
        };
        assert_eq!(
            s.insert_incoming(&sms).unwrap().unwrap().msg_type.as_deref(),
            Some("SMS_GSM")
        );
        assert_eq!(
            s.insert_incoming(&incoming("H3", "+13025550100", "no type"))
                .unwrap()
                .unwrap()
                .msg_type,
            None
        );
        // It survives a reload (recent_messages reads the same column).
        let types: Vec<_> = s.recent_messages(10).unwrap().into_iter().map(|m| m.msg_type).collect();
        assert_eq!(types, vec![Some("IM".into()), Some("SMS_GSM".into()), None]);
    }

    #[test]
    fn old_message_relisted_under_a_new_handle_is_ignored() {
        let s = Store::in_memory().unwrap();
        s.insert_incoming(&incoming("H1", "+13025550100", "see you at 5"))
            .unwrap();
        let mut relisted = incoming("H9", "+13025550100", "see you at 5");
        relisted.received_at = 1_000 + 2 * RELIST_GUARD_MS;
        assert!(s.insert_incoming(&relisted).unwrap().is_none());
    }

    #[test]
    fn does_not_learn_a_name_when_two_people_sent_the_same_text() {
        let s = Store::in_memory().unwrap();
        notify(&s, 1, "Alice", "ok", 950);
        notify(&s, 2, "Bob", "ok", 960);
        s.insert_incoming(&incoming("H1", "+13015550101", "ok")).unwrap();
        s.insert_incoming(&incoming("H2", "+13025550102", "ok")).unwrap();
        assert!(s.learn_contacts().unwrap().is_empty(), "ambiguous: never guess");
    }

    #[test]
    fn does_not_learn_from_a_text_another_sender_also_sent() {
        let s = Store::in_memory().unwrap();
        // Only Bob's notification is on record, but Alice sent the same text too.
        notify(&s, 2, "Bob", "happy birthday!", 960);
        s.insert_incoming(&incoming("H1", "+13015550101", "happy birthday!"))
            .unwrap();
        s.insert_incoming(&incoming("H2", "+13025550102", "happy birthday!"))
            .unwrap();
        assert!(s.learn_contacts().unwrap().is_empty());
    }

    #[test]
    fn does_not_learn_from_a_match_far_apart_in_time() {
        let s = Store::in_memory().unwrap();
        notify(&s, 1, "Alice", "running late", 1_000);
        let mut m = incoming("H1", "+13015550101", "running late");
        m.received_at = 1_000 + LEARN_WINDOW_MS + 1;
        s.insert_incoming(&m).unwrap();
        assert!(s.learn_contacts().unwrap().is_empty());
    }

    #[test]
    fn learns_the_sender_not_the_inline_reply_title() {
        let s = Store::in_memory().unwrap();
        notify(&s, 1, "zoe 💜 replied to you", "Yes", 990);
        s.insert_incoming(&incoming("H1", "+13025550173", "Yes")).unwrap();
        let learned = s.learn_contacts().unwrap();
        assert_eq!(learned[0].name, "zoe 💜");
    }

    #[test]
    fn reply_suffix_is_stripped_only_at_the_end() {
        let s = Store::in_memory().unwrap();
        // Used to come out as "Zoer message"; a name that merely contains the phrase stays whole.
        notify(&s, 1, "Zoe Replied To Your Message ", "Yes", 990);
        s.insert_incoming(&incoming("H1", "+13025550173", "Yes")).unwrap();
        notify(&s, 2, "Replied to you Club", "Sure", 990);
        s.insert_incoming(&incoming("H2", "+12145550186", "Sure")).unwrap();
        let mut names: Vec<String> = s.learn_contacts().unwrap().into_iter().map(|c| c.name).collect();
        names.sort();
        assert_eq!(names, vec!["Replied to you Club", "Zoe"]);
    }

    fn titles(s: &Store) -> Vec<String> {
        s.recent(50, None, None).unwrap().into_iter().map(|n| n.title).collect()
    }

    #[test]
    fn renaming_a_contact_keeps_their_history_together() {
        let s = Store::in_memory().unwrap();
        s.save_phonebook(&[("+13025550173".into(), "zoe 💜".into())]).unwrap();
        notify(&s, 1, "zoe 💜", "omw", 1_000);
        notify(&s, 2, "zoe 💜 replied to you", "Yes", 2_000);
        // The contact is renamed on the phone (heart dropped); the next contacts sync brings it over.
        s.save_phonebook(&[("+13025550173".into(), "zoe".into())]).unwrap();
        notify(&s, 3, "zoe", "hi again", 3_000);
        assert_eq!(
            titles(&s),
            vec!["zoe", "zoe", "zoe"],
            "old notifications show the current name"
        );
        // Renaming back: "zoe" becomes the old name, and "zoe 💜" is current again (titles
        // that need no rewriting come back as iOS sent them; the UI cleans the reply suffix).
        s.save_phonebook(&[("+13025550173".into(), "zoe 💜".into())]).unwrap();
        assert_eq!(titles(&s), vec!["zoe 💜", "zoe 💜 replied to you", "zoe 💜"]);
    }

    #[test]
    fn learns_old_names_from_history() {
        let s = Store::in_memory().unwrap();
        // Renamed before tug kept aliases: the contact is already "zoe", history says "zoe 💜".
        s.save_phonebook(&[("+13025550173".into(), "zoe".into())]).unwrap();
        notify(&s, 1, "zoe 💜", "dinner at 7?", 990);
        s.insert_incoming(&incoming("H1", "+13025550173", "dinner at 7?"))
            .unwrap();
        assert_eq!(s.learn_aliases().unwrap(), 0, "one matching text isn't enough evidence");
        notify(&s, 2, "zoe 💜", "running late", 990);
        s.insert_incoming(&incoming("H2", "+13025550173", "running late"))
            .unwrap();
        assert_eq!(s.learn_aliases().unwrap(), 1);
        assert_eq!(s.learn_aliases().unwrap(), 0, "learned once");
        assert_eq!(titles(&s), vec!["zoe", "zoe"]);
    }

    #[test]
    fn a_shared_short_text_never_hands_someone_elses_name_to_a_contact() {
        let s = Store::in_memory().unwrap();
        s.save_phonebook(&[("+15550000001".into(), "Mom".into())]).unwrap();
        // Mom texts "ok" and "see you soon" (her own notifications never reached tug)...
        s.insert_incoming(&incoming("H1", "+15550000001", "ok")).unwrap();
        s.insert_incoming(&incoming("H2", "+15550000001", "see you soon"))
            .unwrap();
        // ...and an unsaved Coach also texts "ok"; his own text scrolled out before tug synced.
        notify(&s, 1, "Coach", "ok", 990);
        assert_eq!(s.learn_aliases().unwrap(), 0, "a single coincidence");
        // Both texts show up under two names: neither proves anything.
        notify(&s, 2, "Mom", "ok", 995);
        notify(&s, 3, "Coach", "see you soon", 990);
        notify(&s, 4, "Mom", "see you soon", 995);
        assert_eq!(s.learn_aliases().unwrap(), 0);
        assert_eq!(titles(&s), vec!["Mom", "Coach", "Mom", "Coach"]);
    }

    #[test]
    fn old_names_never_take_over_someone_elses() {
        let s = Store::in_memory().unwrap();
        // Sam used to be called "Alex"; there's also a different, current Alex.
        s.save_phonebook(&[("+15550000001".into(), "Alex".into())]).unwrap();
        s.save_phonebook(&[
            ("+15550000001".into(), "Sam".into()),
            ("+15550000002".into(), "Alex".into()),
        ])
        .unwrap();
        notify(&s, 1, "Alex", "hey", 1_000);
        assert_eq!(titles(&s), vec!["Alex"], "a current Alex keeps the name");

        // Two people who both used to be "Jo": ambiguous, so leave it as iOS showed it.
        let t = Store::in_memory().unwrap();
        t.save_phonebook(&[
            ("+15550000003".into(), "Jo".into()),
            ("+15550000004".into(), "Jo".into()),
        ])
        .unwrap();
        t.save_phonebook(&[
            ("+15550000003".into(), "Jo A".into()),
            ("+15550000004".into(), "Jo B".into()),
        ])
        .unwrap();
        notify(&t, 1, "Jo", "hi", 1_000);
        assert_eq!(titles(&t), vec!["Jo"]);
    }

    #[test]
    fn tracks_which_messages_are_still_unread_on_the_phone() {
        let s = Store::in_memory().unwrap();
        let unread = IncomingMessage {
            unread_on_phone: true,
            ..incoming("H1", "+13025550173", "are you up?")
        };
        let a = s.insert_incoming(&unread).unwrap().unwrap();
        let b = s
            .insert_incoming(&incoming("H2", "+13025550173", "already read"))
            .unwrap()
            .unwrap();
        let out = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550173", "yes", 2_000)
            .unwrap();
        let ids = [a.id, b.id, out.id, 999];
        assert_eq!(s.unread_on_phone(SOURCE_IPHONE_MAP, &ids).unwrap(), vec!["H1"]);
        s.set_read_on_phone(SOURCE_IPHONE_MAP, "H1").unwrap();
        assert!(
            s.unread_on_phone(SOURCE_IPHONE_MAP, &ids).unwrap().is_empty(),
            "marked once, not again"
        );
        assert!(s.unread_on_phone(SOURCE_IPHONE_MAP, &[]).unwrap().is_empty());
    }

    #[test]
    fn name_sent_with_the_message_shows_until_the_contact_is_known() {
        let s = Store::in_memory().unwrap();
        let m = IncomingMessage {
            sender_name: Some("Zoe"),
            ..incoming("H1", "+13025550173", "hi")
        };
        assert_eq!(
            s.insert_incoming(&m).unwrap().unwrap().contact_name.as_deref(),
            Some("Zoe")
        );
        s.save_phonebook(&[("+13025550173".into(), "zoe 💜".into())]).unwrap();
        assert_eq!(
            s.recent_messages(10).unwrap()[0].contact_name.as_deref(),
            Some("zoe 💜")
        );
    }

    #[test]
    fn searches_message_text_and_contacts() {
        let s = Store::in_memory().unwrap();
        s.insert_incoming(&incoming("H1", "+13025550173", "dinner at 7?"))
            .unwrap();
        s.insert_incoming(&incoming("H2", "+13025550173", "running late"))
            .unwrap();
        s.insert_outgoing(SOURCE_IPHONE_MAP, "+13025550173", "Dinner sounds great", 2_000)
            .unwrap();
        let hits: Vec<String> = s
            .search_messages("dinn", 10)
            .unwrap()
            .into_iter()
            .map(|m| m.body)
            .collect();
        assert_eq!(
            hits,
            vec!["Dinner sounds great", "dinner at 7?"],
            "prefix, case-insensitive, newest first"
        );
        assert!(s.search_messages("   ", 10).unwrap().is_empty());
        assert!(s.search_messages("\"unbalanced", 10).is_ok());

        s.save_phonebook(&[
            ("+13025550173".into(), "zoe 💜".into()),
            ("+12145550186".into(), "Priya".into()),
        ])
        .unwrap();
        assert_eq!(s.search_contacts("pri", 10).unwrap()[0].name, "Priya");
        assert_eq!(
            s.search_contacts("555-017", 10).unwrap()[0].name,
            "zoe 💜",
            "by number digits"
        );
        assert!(
            s.search_contacts("12", 10).unwrap().is_empty(),
            "too few digits to mean a number"
        );
        assert!(s.search_contacts("100%", 10).unwrap().is_empty());
    }

    #[test]
    fn incoming_is_deduplicated_by_handle() {
        let s = Store::in_memory().unwrap();
        assert!(s
            .insert_incoming(&incoming("H1", "+13025550100", "hi"))
            .unwrap()
            .is_some());
        assert!(s
            .insert_incoming(&incoming("H1", "+13025550100", "hi"))
            .unwrap()
            .is_none());
        assert!(s.has_message(SOURCE_IPHONE_MAP, "H1").unwrap());
        assert!(!s.has_message(SOURCE_IPHONE_MAP, "H2").unwrap());
    }

    #[test]
    fn deleted_texts_stay_hidden_and_arent_synced_again() {
        let s = Store::in_memory().unwrap();
        let a = s
            .insert_incoming(&incoming("H1", "+13025550100", "dinner?"))
            .unwrap()
            .unwrap();
        s.insert_incoming(&incoming("H2", "+13025550199", "hello")).unwrap();
        s.set_hidden(&[], &[a.id], Some(5_000)).unwrap();
        let bodies = |v: Vec<StoredMessage>| v.into_iter().map(|m| m.body).collect::<Vec<_>>();
        assert_eq!(bodies(s.recent_messages(10).unwrap()), vec!["hello"]);
        assert!(s.search_messages("dinner", 10).unwrap().is_empty());
        // The phone still lists it; the sync sees it's stored and doesn't bring it back.
        assert!(s.has_message(SOURCE_IPHONE_MAP, "H1").unwrap());
        assert!(s
            .insert_incoming(&incoming("H1", "+13025550100", "dinner?"))
            .unwrap()
            .is_none());
        s.set_hidden(&[], &[a.id], None).unwrap();
        assert_eq!(s.recent_messages(10).unwrap().len(), 2);
    }

    #[test]
    fn a_reused_handle_doesnt_leave_a_sent_message_pending() {
        let s = Store::in_memory().unwrap();
        s.insert_incoming(&incoming("H7", "+13025550100", "hey")).unwrap();
        let m = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "hi back", 2_000)
            .unwrap();
        // iOS assigns the sent text a handle an inbox message already has.
        let m = s.set_outgoing_status(m.id, Status::Accepted, Some("H7")).unwrap();
        assert_eq!(m.status, Status::Accepted);
    }

    #[test]
    fn outgoing_moves_from_pending_to_accepted() {
        let s = Store::in_memory().unwrap();
        let m = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "on my way", 2_000)
            .unwrap();
        assert_eq!((m.direction, m.status), (Direction::Out, Status::Pending));
        let m = s.set_outgoing_status(m.id, Status::Accepted, Some("A00E")).unwrap();
        assert_eq!(m.status, Status::Accepted);
        assert!(s.has_message(SOURCE_IPHONE_MAP, "A00E").unwrap(), "handle recorded");
        // Several pending sends without handles don't collide on UNIQUE(source, handle).
        s.insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "a", 3_000)
            .unwrap();
        s.insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "b", 3_001)
            .unwrap();
    }

    #[test]
    fn outgoing_unconfirmed_lists_pending_and_accepted_only() {
        let s = Store::in_memory().unwrap();
        let pending = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "a", 1_000)
            .unwrap();
        let accepted = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "b", 2_000)
            .unwrap();
        s.set_outgoing_status(accepted.id, Status::Accepted, Some("A1"))
            .unwrap();
        let confirmed = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "c", 3_000)
            .unwrap();
        s.set_outgoing_status(confirmed.id, Status::Sent, Some("A2")).unwrap();
        let failed = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "d", 4_000)
            .unwrap();
        s.set_outgoing_status(failed.id, Status::Failed, None).unwrap();

        let mut got = s.outgoing_unconfirmed(SOURCE_IPHONE_MAP, 5_000).unwrap();
        got.sort_by_key(|(id, ..)| *id);
        assert_eq!(
            got,
            vec![(pending.id, None, 1_000), (accepted.id, Some("A1".into()), 2_000)]
        );
    }

    #[test]
    fn outgoing_unconfirmed_ignores_old_sends() {
        let s = Store::in_memory().unwrap();
        let now = 100 * 60 * 1000;
        let old = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "old", now - CONFIRM_WINDOW_MS - 1)
            .unwrap();
        s.set_outgoing_status(old.id, Status::Accepted, Some("A1")).unwrap();
        let recent = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "new", now - 5_000)
            .unwrap();
        let got = s.outgoing_unconfirmed(SOURCE_IPHONE_MAP, now).unwrap();
        assert_eq!(got, vec![(recent.id, None, now - 5_000)]);
    }

    #[test]
    fn interrupted_sends_are_failed_at_startup() {
        let s = Store::in_memory().unwrap();
        let stuck = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "a", 1_000)
            .unwrap();
        let accepted = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "b", 2_000)
            .unwrap();
        s.set_outgoing_status(accepted.id, Status::Accepted, Some("A1"))
            .unwrap();
        s.insert_incoming(&incoming("H1", "+13025550100", "hi")).unwrap();
        assert_eq!(s.fail_interrupted_sends().unwrap(), 1);
        let all = s.recent_messages(10).unwrap();
        let status = |id| all.iter().find(|m| m.id == id).unwrap().status;
        assert_eq!(status(stuck.id), Status::Failed);
        assert_eq!(status(accepted.id), Status::Accepted, "accepted sends left alone");
        assert!(all
            .iter()
            .any(|m| m.direction == Direction::In && m.status == Status::Received));
    }

    #[test]
    fn retry_reuses_the_same_row_and_number() {
        let s = Store::in_memory().unwrap();
        let m = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "hi", 1_000)
            .unwrap();
        assert!(
            s.retry_outgoing(m.id, 2_000).unwrap().is_none(),
            "only failed sends retry"
        );
        s.set_outgoing_status(m.id, Status::Failed, None).unwrap();
        let again = s.retry_outgoing(m.id, 9_000).unwrap().expect("retried");
        assert_eq!(again.id, m.id);
        assert_eq!(again.address, "+13025550100");
        assert_eq!(again.body, "hi");
        assert_eq!(again.status, Status::Pending);
        assert_eq!(again.received_at, 9_000);
        assert_eq!(s.recent_messages(10).unwrap().len(), 1, "no duplicate row");
        assert!(
            s.retry_outgoing(m.id, 9_500).unwrap().is_none(),
            "a pending retry can't double up"
        );
    }

    #[test]
    fn an_unconfirmed_send_round_trips_and_can_still_be_settled() {
        let s = Store::in_memory().unwrap();
        let m = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "hi", 1_000)
            .unwrap();
        let m = s.set_outgoing_status(m.id, Status::Unconfirmed, None).unwrap();
        assert_eq!(m.status, Status::Unconfirmed);
        assert_eq!(s.outgoing_unconfirmed(SOURCE_IPHONE_MAP, 2_000).unwrap().len(), 1);
        assert!(
            s.retry_outgoing(m.id, 2_000).unwrap().is_none(),
            "never retried: it may have sent"
        );
        assert_eq!(s.fail_interrupted_sends().unwrap(), 0, "not swept as interrupted");
    }

    #[test]
    fn outgoing_by_id_is_only_sends() {
        let s = Store::in_memory().unwrap();
        let out = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "hi", 1_000)
            .unwrap();
        let inc = s
            .insert_incoming(&incoming("H1", "+13025550100", "yo"))
            .unwrap()
            .unwrap();
        assert_eq!(s.outgoing(out.id).unwrap(), Some(out));
        assert_eq!(s.outgoing(inc.id).unwrap(), None);
        assert_eq!(s.outgoing(9_999).unwrap(), None);
    }

    #[test]
    fn gap_marker_round_trips() {
        let s = Store::in_memory().unwrap();
        let m = s
            .insert_incoming(&incoming("H1", "+13025550100", "hi"))
            .unwrap()
            .unwrap();
        assert!(!m.gap_before);
        assert!(s.mark_gap_before(m.id).unwrap().gap_before);
    }

    #[test]
    fn a_sent_status_round_trips() {
        let s = Store::in_memory().unwrap();
        let m = s
            .insert_outgoing(SOURCE_IPHONE_MAP, "+13025550100", "hi", 2_000)
            .unwrap();
        let m = s.set_outgoing_status(m.id, Status::Sent, Some("A9")).unwrap();
        assert_eq!(m.status, Status::Sent);
    }

    #[test]
    fn recent_messages_are_oldest_first_within_the_window() {
        let s = Store::in_memory().unwrap();
        for (i, h) in ["A", "B", "C"].iter().enumerate() {
            let mut m = incoming(h, "+13025550100", h);
            m.received_at = 1_000 + i as i64;
            s.insert_incoming(&m).unwrap();
        }
        let got: Vec<i64> = s.recent_messages(2).unwrap().iter().map(|m| m.received_at).collect();
        assert_eq!(got, vec![1_001, 1_002]);
    }

    #[test]
    fn phonebook_names_override_learned_ones() {
        let s = Store::in_memory().unwrap();
        s.save_phonebook(&[("+13025550173".into(), "zoe 💜".into())]).unwrap();
        let n = s
            .save_phonebook(&[
                ("+13025550173".into(), "Zoe Park".into()),
                ("+12145550186".into(), "Priya".into()),
            ])
            .unwrap();
        assert_eq!(n, 2);
        let names: Vec<String> = s.contacts().unwrap().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["Priya", "Zoe Park"]);
    }

    #[test]
    fn photo_references_round_trip_and_resolve_by_number_or_name() {
        let s = Store::in_memory().unwrap();
        // The fast sync saves names first; the photo pass attaches the photo afterwards.
        s.save_phonebook(&[
            ("+13025550173".into(), "Zoe".into()),
            ("+12145550186".into(), "Priya".into()),
        ])
        .unwrap();
        s.update_contact_photos(&[
            ("+13025550173".into(), Some("abc123".into())),
            ("+12145550186".into(), None),
        ])
        .unwrap();
        // By number: exact address match.
        assert_eq!(
            s.contact_photo_key(Some("+13025550173"), None).unwrap().as_deref(),
            Some("abc123")
        );
        assert_eq!(
            s.contact_photo_key(Some("+12145550186"), None).unwrap(),
            None,
            "no photo"
        );
        // By name: falls back when the number is unknown.
        assert_eq!(
            s.contact_photo_key(Some("+1999"), Some("Zoe")).unwrap().as_deref(),
            Some("abc123")
        );
        assert_eq!(
            s.contact_photo_key(None, Some("zoe")).unwrap().as_deref(),
            Some("abc123"),
            "case-insensitive"
        );
        // Only non-null photo keys are listed, for cleanup.
        assert_eq!(s.photo_keys().unwrap(), ["abc123".to_string()].into_iter().collect());
        // A photo pass that no longer sees the photo clears the reference.
        s.update_contact_photos(&[("+13025550173".into(), None)]).unwrap();
        assert_eq!(s.contact_photo_key(Some("+13025550173"), None).unwrap(), None);
        assert!(s.photo_keys().unwrap().is_empty());
    }

    #[test]
    fn a_fast_sync_without_photos_keeps_the_ones_the_photo_pass_found() {
        let s = Store::in_memory().unwrap();
        // Photo pass gave Zoe a face.
        s.save_phonebook(&[("+13025550173".into(), "Zoe".into())]).unwrap();
        s.update_contact_photos(&[("+13025550173".into(), Some("abc123".into()))])
            .unwrap();
        // A later fast sync (names only, no photos) must not wipe it — the regression this fixes.
        s.save_phonebook(&[("+13025550173".into(), "Zoe".into())]).unwrap();
        assert_eq!(
            s.contact_photo_key(Some("+13025550173"), None).unwrap().as_deref(),
            Some("abc123"),
            "fast sync preserves the photo reference"
        );
        // A brand-new contact the fast sync adds simply has no photo yet (initials).
        s.save_phonebook(&[("+12145550186".into(), "Priya".into())]).unwrap();
        assert_eq!(s.contact_photo_key(Some("+12145550186"), None).unwrap(), None);
        // The photo pass only touches contacts that already exist; an unknown number is skipped.
        s.update_contact_photos(&[("+19998887777".into(), Some("deadbeef".into()))])
            .unwrap();
        assert!(s.contacts().unwrap().iter().all(|c| c.address != "+19998887777"));
    }

    #[test]
    fn a_name_two_people_share_lends_no_photo() {
        let s = Store::in_memory().unwrap();
        s.save_phonebook(&[
            ("+15550000001".into(), "Sam".into()),
            ("+15550000002".into(), "Sam".into()),
        ])
        .unwrap();
        s.update_contact_photos(&[
            ("+15550000001".into(), Some("aaaa1111".into())),
            ("+15550000002".into(), Some("bbbb2222".into())),
        ])
        .unwrap();
        // Two different Sams with different photos: a name lookup must stay ambiguous.
        assert_eq!(s.contact_photo_key(None, Some("Sam")).unwrap(), None);
        // ...but each number still resolves to its own.
        assert_eq!(
            s.contact_photo_key(Some("+15550000001"), None).unwrap().as_deref(),
            Some("aaaa1111")
        );
    }

    #[test]
    fn learns_contact_names_from_matching_notifications() {
        let s = Store::in_memory().unwrap();
        let attrs = NotificationAttributes {
            app_id: MESSAGES_APP.into(),
            title: "zoe 💜".into(),
            message: "omw, 10 mins".into(),
            ..Default::default()
        };
        s.upsert_notification(&NewNotification {
            session: "s1",
            uid: 1,
            category: Category::Social,
            flags: EventFlags::default(),
            attrs: &attrs,
            received_at: 900,
        })
        .unwrap();
        s.insert_incoming(&incoming("H1", "+13025550173", "omw, 10 mins"))
            .unwrap();
        s.insert_incoming(&incoming("H2", "+12145550186", "unmatched")).unwrap();
        let learned = s.learn_contacts().unwrap();
        assert_eq!(
            learned,
            vec![Contact {
                address: "+13025550173".into(),
                name: "zoe 💜".into()
            }]
        );
        assert!(s.learn_contacts().unwrap().is_empty(), "only new links are returned");
        let msgs = s.recent_messages(10).unwrap();
        assert_eq!(msgs[0].contact_name.as_deref(), Some("zoe 💜"));
        assert_eq!(msgs[1].contact_name, None);
    }
}
