//! Apple model identifier ("iPhone16,2") → the name people know ("iPhone 15 Pro Max"), for
//! `phone_status`. The table is `src/lib/phoneModel.ts`'s (which also knows each design for
//! the sidebar drawing); a test checks the two list the same phones with the same names.

const MODELS: &[(&str, &str)] = &[
    ("iPhone10,1", "iPhone 8"),
    ("iPhone10,4", "iPhone 8"),
    ("iPhone10,2", "iPhone 8 Plus"),
    ("iPhone10,5", "iPhone 8 Plus"),
    ("iPhone10,3", "iPhone X"),
    ("iPhone10,6", "iPhone X"),
    ("iPhone11,2", "iPhone XS"),
    ("iPhone11,4", "iPhone XS Max"),
    ("iPhone11,6", "iPhone XS Max"),
    ("iPhone11,8", "iPhone XR"),
    ("iPhone12,1", "iPhone 11"),
    ("iPhone12,3", "iPhone 11 Pro"),
    ("iPhone12,5", "iPhone 11 Pro Max"),
    ("iPhone12,8", "iPhone SE"),
    ("iPhone13,1", "iPhone 12 mini"),
    ("iPhone13,2", "iPhone 12"),
    ("iPhone13,3", "iPhone 12 Pro"),
    ("iPhone13,4", "iPhone 12 Pro Max"),
    ("iPhone14,4", "iPhone 13 mini"),
    ("iPhone14,5", "iPhone 13"),
    ("iPhone14,2", "iPhone 13 Pro"),
    ("iPhone14,3", "iPhone 13 Pro Max"),
    ("iPhone14,6", "iPhone SE"),
    ("iPhone14,7", "iPhone 14"),
    ("iPhone14,8", "iPhone 14 Plus"),
    ("iPhone15,2", "iPhone 14 Pro"),
    ("iPhone15,3", "iPhone 14 Pro Max"),
    ("iPhone15,4", "iPhone 15"),
    ("iPhone15,5", "iPhone 15 Plus"),
    ("iPhone16,1", "iPhone 15 Pro"),
    ("iPhone16,2", "iPhone 15 Pro Max"),
    ("iPhone17,3", "iPhone 16"),
    ("iPhone17,4", "iPhone 16 Plus"),
    ("iPhone17,1", "iPhone 16 Pro"),
    ("iPhone17,2", "iPhone 16 Pro Max"),
    ("iPhone17,5", "iPhone 16e"),
    ("iPhone18,3", "iPhone 17"),
    ("iPhone18,1", "iPhone 17 Pro"),
    ("iPhone18,2", "iPhone 17 Pro Max"),
    ("iPhone18,4", "iPhone Air"),
    ("iPhone18,5", "iPhone 17e"),
];

/// The marketing name, or `None` for an identifier tug doesn't know.
pub fn model_name(identifier: &str) -> Option<&'static str> {
    let id = identifier.trim();
    MODELS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_known_phones() {
        assert_eq!(model_name("iPhone16,2"), Some("iPhone 15 Pro Max"));
        assert_eq!(model_name(" iPhone10,3 "), Some("iPhone X"));
        assert_eq!(model_name("iPhone99,9"), None);
        assert_eq!(model_name(""), None);
    }

    /// Same identifiers and names as the frontend's table, so the sidebar and AI tools agree.
    #[test]
    fn matches_the_frontend_table() {
        let ts = include_str!("../../../src/lib/phoneModel.ts");
        let mut from_ts = Vec::new();
        for line in ts.lines() {
            let Some(rest) = line.trim().strip_prefix("\"iPhone") else {
                continue;
            };
            let (id, rest) = rest.split_once('"').unwrap();
            let name = rest.split('"').nth(1).unwrap();
            from_ts.push((format!("iPhone{id}"), name.to_string()));
        }
        let ours: Vec<(String, String)> = MODELS.iter().map(|(i, n)| (i.to_string(), n.to_string())).collect();
        assert_eq!(ours, from_ts);
    }
}
