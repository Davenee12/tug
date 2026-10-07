//! What Developer tools keep in tug's settings table, and the pure rules around it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tug_bridge::protocol::{ClientInfo, ClientKind};

/// "true" when the user has switched on "Let AI tools use tug". Off unless set.
pub const ENABLED: &str = "devtools.enabled";
/// JSON `{ "<permission key>": bool }`: only switches the user has touched.
pub const PERMISSIONS: &str = "devtools.permissions";
/// JSON list of `ClientRecord`: the Connected tools list.
pub const CLIENTS: &str = "devtools.clients";
/// How many tools the Connected tools list remembers.
pub const MAX_CLIENTS: usize = 8;

pub fn parse_enabled(v: Option<&str>) -> bool {
    v == Some("true")
}

pub fn parse_permissions(v: Option<&str>) -> BTreeMap<String, bool> {
    v.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

/// A tool that has used tug, for Settings › Developer tools › Connected tools. Mirrored in
/// `src/types/protocol.ts` (`DevToolsClient`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRecord {
    pub name: String,
    pub kind: ClientKind,
    /// Unix ms.
    pub last_used: i64,
}

pub fn parse_clients(v: Option<&str>) -> Vec<ClientRecord> {
    v.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

/// The list after `client` used tug at `now_ms`: newest first, one row per name and kind.
pub fn record_client(mut list: Vec<ClientRecord>, client: &ClientInfo, now_ms: i64) -> Vec<ClientRecord> {
    let name = client.display_name();
    list.retain(|c| !(c.name == name && c.kind == client.kind));
    list.insert(
        0,
        ClientRecord {
            name,
            kind: client.kind,
            last_used: now_ms,
        },
    );
    list.sort_by_key(|c| std::cmp::Reverse(c.last_used));
    list.truncate(MAX_CLIENTS);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, kind: ClientKind) -> ClientInfo {
        ClientInfo {
            name: name.into(),
            kind,
        }
    }

    #[test]
    fn off_unless_switched_on() {
        assert!(!parse_enabled(None));
        assert!(!parse_enabled(Some("false")));
        assert!(!parse_enabled(Some("1")));
        assert!(parse_enabled(Some("true")));
    }

    #[test]
    fn bad_json_is_treated_as_untouched() {
        assert!(parse_permissions(Some("{nope")).is_empty());
        assert_eq!(parse_permissions(Some(r#"{"media":true}"#)).get("media"), Some(&true));
        assert!(parse_clients(Some("[{]")).is_empty());
    }

    #[test]
    fn clients_are_newest_first_and_deduplicated() {
        let l = record_client(Vec::new(), &info("claude-code", ClientKind::Mcp), 10);
        let l = record_client(l, &info("cursor", ClientKind::Mcp), 20);
        let l = record_client(l, &info("tug", ClientKind::Cli), 30);
        let l = record_client(l, &info("claude-code", ClientKind::Mcp), 40);
        let names: Vec<_> = l.iter().map(|c| (c.name.as_str(), c.last_used)).collect();
        assert_eq!(names, vec![("claude-code", 40), ("tug", 30), ("cursor", 20)]);
        let round: Vec<ClientRecord> = parse_clients(Some(&serde_json::to_string(&l).unwrap()));
        assert_eq!(round, l);
    }

    #[test]
    fn the_list_is_capped() {
        let mut l = Vec::new();
        for i in 0..20 {
            l = record_client(l, &info(&format!("tool{i}"), ClientKind::Mcp), i);
        }
        assert_eq!(l.len(), MAX_CLIENTS);
        assert_eq!(l[0].name, "tool19");
    }
}
