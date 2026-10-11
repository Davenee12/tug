//! Running tallies the inventory reports: ANCS categories, flags and which attributes come back,
//! vCard property NAMES per contact, MAP listing types and MNS event types. Only names, counts and
//! system strings are kept — never a title, message, number or contact value. Pure, unit-tested.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::safe_text;
use crate::ancs::{Category, EventId, NotificationAttributes, NotificationEvent};

/// Every ANCS CategoryID, in spec order (so the report lists zeros too).
const CATEGORIES: [Category; 12] = [
    Category::Other,
    Category::IncomingCall,
    Category::MissedCall,
    Category::Voicemail,
    Category::Social,
    Category::Schedule,
    Category::Email,
    Category::News,
    Category::HealthAndFitness,
    Category::BusinessAndFinance,
    Category::Location,
    Category::Entertainment,
];

/// The most distinct action labels kept, and their longest length: they're the phone's own
/// system strings ("Clear", "Mark as Read"), but a cap keeps a misbehaving app from filling it.
const MAX_LABELS: usize = 24;
const MAX_LABEL_LEN: usize = 32;

/// ANCS since tug started.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AncsTally {
    pub added: u32,
    pub modified: u32,
    pub removed: u32,
    /// Added notifications per CategoryID (replays after a reconnect count again; see `preExisting`).
    pub categories: BTreeMap<String, u32>,
    /// Added notifications carrying each EventFlag.
    pub flags: BTreeMap<String, u32>,
    /// Notifications whose details came back.
    pub details_fetched: u32,
    /// How many of those had each optional attribute non-empty.
    pub attributes_present: BTreeMap<String, u32>,
    /// MessageSize (attribute 4) isn't part of tug's request, so it can't come back.
    pub message_size: String,
    /// The distinct positive/negative action labels the phone sent.
    pub action_labels: BTreeSet<String>,
}

impl AncsTally {
    /// An empty tally that still lists every category, flag and attribute (with zeros).
    pub fn with_all_categories() -> Self {
        let mut t = Self {
            message_size: "not requested by tug".into(),
            ..Default::default()
        };
        for c in CATEGORIES {
            t.categories.insert(c.as_str().to_string(), 0);
        }
        for f in ["silent", "important", "preExisting", "positiveAction", "negativeAction"] {
            t.flags.insert(f.to_string(), 0);
        }
        for a in ["subtitle", "date", "positiveActionLabel", "negativeActionLabel"] {
            t.attributes_present.insert(a.to_string(), 0);
        }
        t
    }

    /// One Notification Source packet.
    pub fn event(&mut self, ev: &NotificationEvent) {
        match ev.event {
            EventId::Modified => self.modified += 1,
            EventId::Removed => self.removed += 1,
            EventId::Added => {
                self.added += 1;
                *self.categories.entry(ev.category.as_str().to_string()).or_default() += 1;
                let f = ev.flags;
                for (name, on) in [
                    ("silent", f.silent),
                    ("important", f.important),
                    ("preExisting", f.pre_existing),
                    ("positiveAction", f.positive_action),
                    ("negativeAction", f.negative_action),
                ] {
                    if on {
                        *self.flags.entry(name.to_string()).or_default() += 1;
                    }
                }
            }
        }
    }

    /// One notification's details. Only presence is counted; the action labels (system strings)
    /// are kept, redacted and capped.
    pub fn details(&mut self, a: &NotificationAttributes) {
        self.details_fetched += 1;
        for (name, present) in [
            ("subtitle", !a.subtitle.is_empty()),
            ("date", a.date.is_some()),
            ("positiveActionLabel", !a.positive_label.is_empty()),
            ("negativeActionLabel", !a.negative_label.is_empty()),
        ] {
            if present {
                *self.attributes_present.entry(name.to_string()).or_default() += 1;
            }
        }
        for label in [&a.positive_label, &a.negative_label] {
            let label = safe_text(label.trim(), MAX_LABEL_LEN);
            if !label.is_empty() && self.action_labels.len() < MAX_LABELS {
                self.action_labels.insert(label);
            }
        }
    }
}

/// vCard property names across one phonebook pull: how many contacts carry each property (a
/// contact with two EMAILs counts once), and the TEL types. Values are never looked at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VcardFieldCounts {
    pub contacts: u32,
    pub fields: BTreeMap<String, u32>,
    pub tel_types: BTreeMap<String, u32>,
}

/// What an unlisted property name or TEL type is reported as: a vCard can carry names an app or
/// the user made up (a custom label typed into TYPE), and those could say something personal.
const CUSTOM: &str = "custom";

/// vCard property names the counts may show by name: RFC 6350's, plus the vCard 2.1/3.0 ones
/// PBAP's property selector defines (LABEL, MAILER, AGENT, CLASS, SORT-STRING). `X-` extension
/// names (fixed vendor tokens such as X-ABRELATEDNAMES) are shown too; anything else is "custom".
const KNOWN_PROPERTIES: [&str; 40] = [
    "SOURCE",
    "KIND",
    "XML",
    "FN",
    "N",
    "NICKNAME",
    "PHOTO",
    "BDAY",
    "ANNIVERSARY",
    "GENDER",
    "ADR",
    "TEL",
    "EMAIL",
    "IMPP",
    "LANG",
    "TZ",
    "GEO",
    "TITLE",
    "ROLE",
    "LOGO",
    "ORG",
    "MEMBER",
    "RELATED",
    "CATEGORIES",
    "NOTE",
    "PRODID",
    "REV",
    "SOUND",
    "UID",
    "CLIENTPIDMAP",
    "URL",
    "KEY",
    "FBURL",
    "CALADRURI",
    "CALURI",
    "LABEL",
    "MAILER",
    "AGENT",
    "CLASS",
    "SORT-STRING",
];

/// TEL types the counts may show by name: RFC 6350's (with the general HOME/WORK), the vCard
/// 2.1/3.0 ones, and Apple's fixed labels (IPHONE, MAIN, OTHER). Anything else — a label the user
/// typed, which iOS can put in TYPE — is "custom".
const KNOWN_TEL_TYPES: [&str; 19] = [
    "TEXT",
    "VOICE",
    "FAX",
    "CELL",
    "VIDEO",
    "PAGER",
    "TEXTPHONE",
    "HOME",
    "WORK",
    "PREF",
    "MSG",
    "BBS",
    "MODEM",
    "CAR",
    "ISDN",
    "PCS",
    "IPHONE",
    "MAIN",
    "OTHER",
];

/// A property or type name made safe: upper-case letters, digits and dashes only, capped.
fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw.trim().to_ascii_uppercase().chars().take(40).collect();
    let ok = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then_some(name)
}

/// How a (clean) property name is counted: by name if it's standard or an `X-` extension,
/// otherwise as "custom".
fn property_label(name: String) -> String {
    if KNOWN_PROPERTIES.contains(&name.as_str()) || name.starts_with("X-") {
        name
    } else {
        CUSTOM.to_string()
    }
}

/// How a TEL type is counted: by name if it's a standard (or Apple's fixed) type, else "custom".
fn tel_type_label(raw: &str) -> String {
    match clean_name(raw.trim_matches('"')) {
        Some(t) if KNOWN_TEL_TYPES.contains(&t.as_str()) => t,
        _ => CUSTOM.to_string(),
    }
}

/// Count property names per contact in a PBAP phonebook object.
pub fn count_vcard_fields(raw: &str) -> VcardFieldCounts {
    let mut out = VcardFieldCounts::default();
    let mut fields: BTreeSet<String> = BTreeSet::new();
    let mut tel_types: BTreeSet<String> = BTreeSet::new();
    let mut in_card = false;
    for line in raw.split('\n').map(|l| l.trim_end_matches('\r')) {
        // Folded continuations (and base64 photo lines) carry no property name.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = head.split(';');
        let key = parts.next().unwrap_or("").rsplit('.').next().unwrap_or("");
        let Some(key) = clean_name(key) else { continue };
        match key.as_str() {
            "BEGIN" if value.trim().eq_ignore_ascii_case("VCARD") => {
                in_card = true;
                fields.clear();
                tel_types.clear();
            }
            "END" if value.trim().eq_ignore_ascii_case("VCARD") => {
                if in_card {
                    out.contacts += 1;
                    for f in fields.iter() {
                        *out.fields.entry(f.clone()).or_default() += 1;
                    }
                    for t in tel_types.iter() {
                        *out.tel_types.entry(t.clone()).or_default() += 1;
                    }
                }
                in_card = false;
            }
            "VERSION" => {}
            _ if in_card => {
                if key == "TEL" {
                    for p in parts {
                        let types = match p.split_once('=') {
                            Some((k, v)) if k.trim().eq_ignore_ascii_case("TYPE") => v,
                            Some(_) => continue,
                            // vCard 2.1 writes bare types: `TEL;CELL;VOICE:`.
                            None => p,
                        };
                        for t in types.split(',').filter(|t| !t.trim().is_empty()) {
                            tel_types.insert(tel_type_label(t));
                        }
                    }
                }
                fields.insert(property_label(key));
            }
            _ => {}
        }
    }
    out
}

/// A MAP folder or message-type name made safe for the log: plain characters only, capped.
pub fn clean_token(raw: &str) -> Option<String> {
    let t: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .take(32)
        .collect();
    let t = safe_text(&t, 32);
    (!t.is_empty()).then_some(t)
}

/// Folder names from a MAP folder-listing object (`<folder name="inbox"/>`).
pub fn folder_names(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<folder") {
        rest = &rest[i + 7..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        if let Some(name) = attribute(tag, "name").and_then(|n| clean_token(&n)) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
        rest = &rest[end..];
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before_ok = i == 0 || rest.as_bytes()[i - 1].is_ascii_whitespace();
        let after = rest[i + name.len()..].trim_start();
        if before_ok {
            if let Some(v) = after.strip_prefix('=') {
                let v = v.trim_start();
                let quote = v.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let body = &v[1..];
                    return body.find(quote).map(|e| body[..e].to_string());
                }
            }
        }
        rest = &rest[i + name.len()..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ancs::parse_notification_source;

    /// The serialized tally must carry none of the made-up personal text a fixture put in.
    fn assert_omits(json: &str, fixture_text: &[&str]) {
        for text in fixture_text {
            assert!(!json.contains(text), "fixture text {text:?} reached the tally: {json}");
        }
    }

    #[test]
    fn tallies_categories_flags_and_events() {
        let mut t = AncsTally::with_all_categories();
        // Added, flags important|positive|negative, category Schedule (5).
        t.event(&parse_notification_source(&[0, 0x1A, 5, 1, 1, 0, 0, 0]).unwrap());
        // Added, pre-existing, Voicemail (3).
        t.event(&parse_notification_source(&[0, 0x04, 3, 1, 2, 0, 0, 0]).unwrap());
        // Modified and removed don't count as new.
        t.event(&parse_notification_source(&[1, 0, 5, 1, 1, 0, 0, 0]).unwrap());
        t.event(&parse_notification_source(&[2, 0, 5, 1, 1, 0, 0, 0]).unwrap());
        assert_eq!((t.added, t.modified, t.removed), (2, 1, 1));
        assert_eq!(t.categories["schedule"], 1);
        assert_eq!(t.categories["voicemail"], 1);
        assert_eq!(t.categories["entertainment"], 0, "every category is listed");
        assert_eq!(t.categories.len(), 12);
        assert_eq!(t.flags["important"], 1);
        assert_eq!(t.flags["preExisting"], 1);
        assert_eq!(t.flags["silent"], 0);
    }

    #[test]
    fn details_count_presence_and_keep_only_labels() {
        let mut t = AncsTally::with_all_categories();
        t.details(&NotificationAttributes {
            app_id: "com.apple.MobileSMS".into(),
            title: "Jane Doe".into(),
            subtitle: "Family".into(),
            message: "call me on 3025550142".into(),
            date: Some("2026-10-06T10:00:00".into()),
            positive_label: "Mark as Read".into(),
            negative_label: "Clear".into(),
        });
        t.details(&NotificationAttributes {
            negative_label: "Clear".into(),
            ..Default::default()
        });
        assert_eq!(t.details_fetched, 2);
        assert_eq!(t.attributes_present["subtitle"], 1);
        assert_eq!(t.attributes_present["date"], 1);
        assert_eq!(t.attributes_present["negativeActionLabel"], 2);
        assert_eq!(
            t.action_labels.iter().cloned().collect::<Vec<_>>(),
            vec!["Clear", "Mark as Read"]
        );
        let json = serde_json::to_string(&t).unwrap();
        assert_omits(&json, &["Jane", "Family", "3025550142", "MobileSMS"]);
    }

    #[test]
    fn action_labels_are_capped_and_redacted() {
        let mut t = AncsTally::with_all_categories();
        for i in 0..40 {
            t.details(&NotificationAttributes {
                positive_label: format!("Action {i}"),
                ..Default::default()
            });
        }
        assert_eq!(t.action_labels.len(), MAX_LABELS);
        let mut t = AncsTally::with_all_categories();
        t.details(&NotificationAttributes {
            positive_label: "Call 3025550142".into(),
            ..Default::default()
        });
        assert!(t.action_labels.contains("Call [number]"));
    }

    #[test]
    fn counts_vcard_property_names_per_contact() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Jane Doe\r\nN:Doe;Jane;;;\r\n\
            TEL;TYPE=CELL:+13025550142\r\nTEL;type=HOME,VOICE:302 555 0100\r\n\
            EMAIL;TYPE=INTERNET:jane@example.com\r\nEMAIL:jane2@example.com\r\n\
            item1.ADR;TYPE=HOME:;;1 Main St;Dover;DE;19901;USA\r\nBDAY:1990-01-02\r\n\
            X-ABRELATEDNAMES:John\r\nNOTE:Met at: the park\r\n folded: continuation\r\nEND:VCARD\r\n\
            BEGIN:VCARD\r\nVERSION:2.1\r\nFN:Bob\r\nTEL;CELL;VOICE:555 0101\r\n\
            PHOTO;ENCODING=BASE64;TYPE=JPEG:/9j/4AAQ\r\n  SkZJRgABAQ\r\n\r\nEND:VCARD\r\n";
        let c = count_vcard_fields(raw);
        assert_eq!(c.contacts, 2);
        assert_eq!(c.fields["FN"], 2);
        assert_eq!(c.fields["TEL"], 2);
        assert_eq!(c.fields["EMAIL"], 1, "two emails on one contact count once");
        assert_eq!(c.fields["ADR"], 1, "group prefix dropped");
        assert_eq!(c.fields["BDAY"], 1);
        assert_eq!(c.fields["X-ABRELATEDNAMES"], 1);
        assert_eq!(c.fields["PHOTO"], 1);
        assert!(!c.fields.contains_key("VERSION"));
        assert_eq!(c.tel_types["CELL"], 2);
        assert_eq!(c.tel_types["HOME"], 1);
        assert_eq!(c.tel_types["VOICE"], 2);
        let json = serde_json::to_string(&c).unwrap();
        assert_omits(
            &json,
            &["Jane", "3025550142", "example.com", "Main St", "1990", "John", "park"],
        );
    }

    #[test]
    fn junk_and_unlisted_property_names_never_show() {
        // `http://…` looks like a property named HTTP: not a vCard name, so only "custom" shows.
        let c = count_vcard_fields("BEGIN:VCARD\nFN:A\nhttp://x.y/z:1\n1BAD:x\nEND:VCARD\n");
        assert_eq!(c.contacts, 1);
        assert_eq!(c.fields.keys().cloned().collect::<Vec<_>>(), vec!["FN", "custom"]);
        assert_eq!(count_vcard_fields("").contacts, 0);
    }

    #[test]
    fn only_standard_and_extension_property_names_are_shown() {
        let raw = "BEGIN:VCARD\nVERSION:3.0\nFN:A\nNOTE:x\nLABEL:x\nX-ABLABEL:Mobile\n\
            JANEDOEBIRTHDAY:x\nSECRETCLUB:x\nEND:VCARD\n";
        let c = count_vcard_fields(raw);
        assert_eq!(
            c.fields.keys().cloned().collect::<Vec<_>>(),
            vec!["FN", "LABEL", "NOTE", "X-ABLABEL", "custom"]
        );
        assert_eq!(c.fields["custom"], 1, "made-up names count once per contact, as custom");
        let json = serde_json::to_string(&c).unwrap();
        assert_omits(&json, &["JANEDOE", "SECRETCLUB", "Mobile"]);
    }

    #[test]
    fn only_standard_tel_types_are_shown() {
        assert_eq!(tel_type_label("cell"), "CELL");
        assert_eq!(tel_type_label("\"voice\""), "VOICE");
        assert_eq!(tel_type_label("iPhone"), "IPHONE", "Apple's fixed label");
        assert_eq!(tel_type_label("pref"), "PREF");
        assert_eq!(tel_type_label("Grandma"), "custom", "a label the user typed");
        assert_eq!(tel_type_label("X-WORKMOBILE"), "custom");
        assert_eq!(tel_type_label("Dave's work"), "custom");
        let raw = "BEGIN:VCARD\nTEL;TYPE=CELL,Grandma:1\nTEL;type=HOME;type=\"Mum's\":2\n\
            TEL;CELL;BESTFRIEND:3\nTEL;TYPE=:4\nEND:VCARD\n";
        let c = count_vcard_fields(raw);
        assert_eq!(
            c.tel_types.keys().cloned().collect::<Vec<_>>(),
            vec!["CELL", "HOME", "custom"]
        );
        let json = serde_json::to_string(&c).unwrap();
        assert_omits(&json, &["GRANDMA", "Grandma", "Mum", "BESTFRIEND"]);
    }

    #[test]
    fn reads_folder_names_from_a_listing() {
        let xml = r#"<?xml version="1.0"?><folder-listing version="1.0">
            <folder name="inbox"/><folder name = 'sent' created="2026"/><folder name="deleted"/>
            <folder name="inbox"/><folder name="&lt;script&gt;"/></folder-listing>"#;
        assert_eq!(folder_names(xml), vec!["inbox", "sent", "deleted", "ltscriptgt"]);
        assert!(folder_names("").is_empty());
    }

    #[test]
    fn tokens_are_cleaned_and_redacted() {
        assert_eq!(clean_token("SMS_GSM").as_deref(), Some("SMS_GSM"));
        assert_eq!(clean_token("  ").as_deref(), None);
        assert_eq!(clean_token("3025550142").as_deref(), Some("[number]"));
    }
}
