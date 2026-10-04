//! Conversation messages from connectors (first: the iPhone over Bluetooth MAP),
//! stored next to notifications. History grows past the phone's small MAP
//! window because every message seen is kept.

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::store::{Result, Store};

pub const SOURCE_IPHONE_MAP: &str = "iphone-map";

/// Notifications from this app carry the same text as MAP messages, which is how
/// tug learns which contact name belongs to which number.
const MESSAGES_APP: &str = "com.apple.MobileSMS";

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

/// `received` for incoming; outgoing goes `pending` → `accepted` (taken by the
/// iPhone for sending; not proof of delivery) or `failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Received,
    Pending,
    Accepted,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Received => "received",
            Status::Pending => "pending",
            Status::Accepted => "accepted",
            Status::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "pending" => Status::Pending,
            "accepted" => Status::Accepted,
            "failed" => Status::Failed,
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
}

const SELECT: &str = "SELECT m.id, m.source, m.direction, m.address, c.name, m.body, m.sent_at, m.received_at, m.status
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
        // Guard against the phone re-issuing handles across sessions: the same
        // sender, text and phone timestamp is the same message.
        let duplicate = conn
            .query_row(
                "SELECT 1 FROM messages WHERE source = ?1 AND direction = 'in' AND address = ?2 AND body = ?3 AND sent_at IS ?4",
                params![m.source, m.address, m.body, m.sent_at],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if duplicate {
            return Ok(None);
        }
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO messages (source, handle, direction, address, sender_name, body, sent_at, received_at, status)
             VALUES (?1, ?2, 'in', ?3, ?4, ?5, ?6, ?7, 'received')",
            params![m.source, m.handle, m.address, m.sender_name, m.body, m.sent_at, m.received_at],
        )?;
        if inserted == 0 {
            return Ok(None);
        }
        let id = conn.last_insert_rowid();
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
            .map(Some)
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
        conn.execute(
            "UPDATE messages SET status = ?2, handle = COALESCE(?3, handle) WHERE id = ?1 AND direction = 'out'",
            params![id, status.as_str(), handle],
        )?;
        conn.query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], map_row)
    }

    /// Most recent messages, oldest first within the window.
    pub fn recent_messages(&self, limit: u32) -> Result<Vec<StoredMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM ({SELECT} ORDER BY m.received_at DESC, m.id DESC LIMIT ?1) ORDER BY received_at, id"
        ))?;
        let rows = stmt.query_map([limit], map_row)?;
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

    /// Store names from the phone's own contacts (PBAP). These win over names
    /// learned from notifications. Returns how many numbers were saved.
    pub fn save_phonebook(&self, entries: &[(String, String)]) -> Result<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO contacts (address, name) VALUES (?1, ?2)
                 ON CONFLICT (address) DO UPDATE SET name = excluded.name",
            )?;
            for (address, name) in entries {
                n += stmt.execute(params![address, name])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// Learn names for addresses that have none yet, by finding a Messages
    /// notification with exactly the same text as an incoming message. Returns
    /// the newly learned contacts.
    pub fn learn_contacts(&self) -> Result<Vec<Contact>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "INSERT OR IGNORE INTO contacts (address, name)
             SELECT m.address, n.title
             FROM messages m
             JOIN notifications n ON n.app_id = ?1 AND n.message = m.body AND n.title <> ''
             WHERE m.direction = 'in' AND m.body <> ''
               AND m.address NOT IN (SELECT address FROM contacts)
             GROUP BY m.address
             RETURNING address, name",
        )?;
        let rows = stmt.query_map([MESSAGES_APP], |r| {
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
        }
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
        assert!(
            s.insert_incoming(&incoming("H9", "+13025550100", "hi"))
                .unwrap()
                .is_none(),
            "same message under a new handle"
        );
        assert!(s.has_message(SOURCE_IPHONE_MAP, "H1").unwrap());
        assert!(!s.has_message(SOURCE_IPHONE_MAP, "H2").unwrap());
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
        s.save_phonebook(&[("+13026698133".into(), "tay 🤎".into())]).unwrap();
        let n = s
            .save_phonebook(&[
                ("+13026698133".into(), "Tay Jones".into()),
                ("+12142230313".into(), "Daviel".into()),
            ])
            .unwrap();
        assert_eq!(n, 2);
        let names: Vec<String> = s.contacts().unwrap().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["Daviel", "Tay Jones"]);
    }

    #[test]
    fn learns_contact_names_from_matching_notifications() {
        let s = Store::in_memory().unwrap();
        let attrs = NotificationAttributes {
            app_id: MESSAGES_APP.into(),
            title: "tay 🤎".into(),
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
        s.insert_incoming(&incoming("H1", "+13026698133", "omw, 10 mins"))
            .unwrap();
        s.insert_incoming(&incoming("H2", "+12142230313", "unmatched")).unwrap();
        let learned = s.learn_contacts().unwrap();
        assert_eq!(
            learned,
            vec![Contact {
                address: "+13026698133".into(),
                name: "tay 🤎".into()
            }]
        );
        assert!(s.learn_contacts().unwrap().is_empty(), "only new links are returned");
        let msgs = s.recent_messages(10).unwrap();
        assert_eq!(msgs[0].contact_name.as_deref(), Some("tay 🤎"));
        assert_eq!(msgs[1].contact_name, None);
    }
}
