//! Normalising message addresses so one person is one conversation, however the
//! phone formats their number.

/// `+1 (302) 555-0173`, `13025550173`, `3025550173` → `+13025550173`.
/// Emails are lower-cased. Short codes and other numbers keep their digits.
pub fn normalize(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains('@') {
        return raw.to_lowercase();
    }
    let plus = raw.starts_with('+');
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    match (plus, digits.len()) {
        (_, 0) => raw.to_string(),
        (true, _) => format!("+{digits}"),
        // North American numbers without a country code.
        (false, 10) => format!("+1{digits}"),
        (false, 11) if digits.starts_with('1') => format!("+{digits}"),
        (false, _) => digits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_nanp_variants_to_one_key() {
        for raw in [
            "+1 (302) 555-0173",
            "13025550173",
            "3025550173",
            "302.555.0173",
            "+13025550173",
        ] {
            assert_eq!(normalize(raw), "+13025550173", "{raw}");
        }
    }

    #[test]
    fn keeps_international_short_codes_and_emails() {
        assert_eq!(normalize("+44 20 7946 0958"), "+442079460958");
        assert_eq!(normalize("72975"), "72975");
        assert_eq!(normalize(" Zoe@Example.com "), "zoe@example.com");
        assert_eq!(normalize("Unknown"), "Unknown");
    }
}
