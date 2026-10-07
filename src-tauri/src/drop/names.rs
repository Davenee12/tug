//! Safe Windows file names for what the phone sends, and " (2)"-style names when one is taken.

/// Longest name we'll create, in UTF-16 units: well inside NTFS's 255 and leaves room for
/// " (99)" and a long Pictures path.
const MAX_LEN: usize = 150;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹",
    "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²",
    "LPT³", "CONIN$", "CONOUT$",
];

/// The phone's name for a file → a plain file name that's safe to create in the Drop folder.
/// Drops any folder parts, characters Windows forbids, control characters, trailing dots and
/// spaces, and reserved device names (`CON`, `COM1`…), and keeps it a sensible length.
pub fn sanitize(name: &str) -> String {
    // Only the last path component, whichever separator the browser used.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            // Bidi overrides can disguise an extension ("photo<RLO>gpj.exe").
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_start().trim_end_matches(['.', ' ']);
    let mut out = if trimmed.trim_matches(['.', '_', ' ']).is_empty() {
        "file".to_string()
    } else {
        trimmed.to_string()
    };
    // Windows treats "NUL.tar.gz" as the device too: check everything before the first dot.
    let stem = out.split('.').next().unwrap_or("");
    let device = stem.trim_end_matches(' ').to_ascii_uppercase();
    if RESERVED.iter().any(|r| r.to_uppercase() == device) {
        out.insert(0, '_');
    }
    truncate(&out, MAX_LEN)
}

/// `"IMG_1.HEIC"` → `("IMG_1", ".HEIC")`. A leading dot alone isn't an extension.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 && name.len() - i <= 16 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Shorten to `max` UTF-16 units, keeping the extension and whole characters.
fn truncate(name: &str, max: usize) -> String {
    if name.encode_utf16().count() <= max {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    let budget = max.saturating_sub(ext.encode_utf16().count());
    let mut used = 0;
    let mut out = String::new();
    for c in stem.chars() {
        used += c.len_utf16();
        if used > budget {
            break;
        }
        out.push(c);
    }
    let out = out.trim_end_matches(['.', ' ']).to_string();
    format!("{out}{ext}")
}

/// The first of `name`, `name (2)`, `name (3)`… that `taken` says is free.
pub fn unique(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    (2..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !taken(candidate))
        .expect("an unbounded range always finds a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ordinary_names() {
        assert_eq!(sanitize("IMG_0042.HEIC"), "IMG_0042.HEIC");
        assert_eq!(sanitize("Holiday photo (1).jpg"), "Holiday photo (1).jpg");
        assert_eq!(sanitize("résumé – final.pdf"), "résumé – final.pdf");
        assert_eq!(sanitize(".bashrc"), ".bashrc");
    }

    #[test]
    fn strips_folders_and_traversal() {
        assert_eq!(sanitize("../../Windows/System32/evil.dll"), "evil.dll");
        assert_eq!(sanitize("C:\\Users\\x\\secret.txt"), "secret.txt");
        assert_eq!(sanitize(".."), "file");
        assert_eq!(sanitize("folder/"), "file");
        assert_eq!(sanitize(""), "file");
    }

    #[test]
    fn replaces_forbidden_characters() {
        assert_eq!(sanitize("a<b>c:d\"e|f?g*h.txt"), "a_b_c_d_e_f_g_h.txt");
        assert_eq!(sanitize("tab\there\n.txt"), "tab_here_.txt");
        assert_eq!(sanitize("photo\u{202E}gpj.exe"), "photo_gpj.exe");
        assert_eq!(sanitize("name. . ."), "name");
        assert_eq!(sanitize("   spaced.txt  "), "spaced.txt");
    }

    #[test]
    fn avoids_reserved_device_names() {
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("COM1.jpg"), "_COM1.jpg");
        assert_eq!(sanitize("lpt9"), "_lpt9");
        assert_eq!(sanitize("CONSOLE.txt"), "CONSOLE.txt");
        assert_eq!(sanitize("aux .txt"), "_aux .txt");
        assert_eq!(sanitize("NUL.tar.gz"), "_NUL.tar.gz");
    }

    #[test]
    fn limits_length_keeping_the_extension() {
        let long = format!("{}.mov", "a".repeat(400));
        let s = sanitize(&long);
        assert!(s.encode_utf16().count() <= MAX_LEN);
        assert!(s.ends_with(".mov"));
        // Multi-unit characters are never split.
        let emoji = format!("{}.jpg", "📷".repeat(200));
        let s = sanitize(&emoji);
        assert!(s.encode_utf16().count() <= MAX_LEN);
        assert!(s.ends_with(".jpg"));
        assert!(s.trim_end_matches(".jpg").chars().all(|c| c == '📷'));
    }

    #[test]
    fn numbers_collisions() {
        let taken = ["IMG_1.HEIC", "IMG_1 (2).HEIC", "notes"];
        let is_taken = |n: &str| taken.contains(&n);
        assert_eq!(unique("IMG_1.HEIC", is_taken), "IMG_1 (3).HEIC");
        assert_eq!(unique("IMG_2.HEIC", is_taken), "IMG_2.HEIC");
        assert_eq!(unique("notes", is_taken), "notes (2)");
        assert_eq!(unique(".bashrc", |n| n == ".bashrc"), ".bashrc (2)");
    }
}
