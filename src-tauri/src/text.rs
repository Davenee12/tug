//! Text clean-up shared by everything that stores or compares names from the phone.

/// Invisible formatting characters some apps put in names: WhatsApp and Snapchat prefix some
/// notification titles with a LEFT-TO-RIGHT MARK (U+200E), which made "‎sam ❤️" a different
/// conversation from "sam ❤️" (and lost their photo). Covers bidi marks, embeddings and isolates,
/// zero-width space, word joiner and invisible operators, the BOM and the soft hyphen. Zero-width
/// joiner/non-joiner stay: emoji sequences (👨‍👩‍👧) and some scripts need them.
pub fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// `s` without invisible formatting characters (see `is_invisible`).
pub fn strip_invisible(s: &str) -> String {
    s.chars().filter(|c| !is_invisible(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_marks_apps_hide_in_names() {
        assert_eq!(strip_invisible("\u{200E}sam \u{2764}\u{FE0F}"), "sam \u{2764}\u{FE0F}");
        assert_eq!(strip_invisible("\u{202A}Zoe\u{202C}\u{200F}"), "Zoe");
        assert_eq!(strip_invisible("\u{FEFF}Sam\u{200B}"), "Sam");
        assert_eq!(strip_invisible("Jo\u{00AD}anne\u{2066}"), "Joanne");
    }

    #[test]
    fn keeps_emoji_joiners_and_ordinary_text() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(strip_invisible(family), family);
        assert_eq!(strip_invisible("marco 🤎"), "marco 🤎");
        assert_eq!(strip_invisible(""), "");
    }
}
