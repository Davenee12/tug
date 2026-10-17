//! "Copy diagnostics": a plain-text report the owner can send for support. It gathers the
//! app and Windows versions, the live `DeviceStatus`, the non-sensitive settings flags and the
//! tail of the rotated log files, with phone numbers, emails, the phone's own name, Bluetooth
//! addresses and the Windows profile folder (with its 8.3 short and URL-encoded forms) and account
//! name masked out (tug never logs contact
//! names, device names or message text). The pieces
//! that read files or the OS live in the command; the redaction and the report layout are pure
//! here so they can be unit-tested without hardware.

use std::fs;
use std::path::Path;

use crate::state::{DeviceStatus, RadioState};

/// How many log lines to include (the plugin keeps five rotated 2 MB files).
pub const LOG_TAIL_LINES: usize = 300;

/// A one-line, number-free summary of the Bluetooth adapter from what tug already knows.
/// Deeper adapter queries are async WinRT and not cheap from the clipboard's sync thread, so
/// the report sticks to the radio state and whether the adapter can act as a peripheral.
pub fn bluetooth_summary(status: &DeviceStatus) -> String {
    let radio = match status.radio {
        RadioState::On => "radio on",
        RadioState::Off => "radio off",
        RadioState::Unavailable => "no adapter",
        RadioState::Unknown => "radio unknown",
    };
    let peripheral = match status.peripheral_supported {
        Some(true) => "peripheral role supported",
        Some(false) => "peripheral role unsupported",
        None => "peripheral role unknown",
    };
    format!("{radio}, {peripheral}")
}

/// The Windows version string (`cmd /c ver`), or "unknown" off Windows or on failure. Cheap.
pub fn os_version() -> String {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up over tug while it asks.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/c", "ver"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(not(windows))]
    {
        format!("{} (not Windows)", std::env::consts::OS)
    }
}

/// Mask anything that could identify a person: long digit runs (phone numbers, account
/// numbers) and email addresses. Deliberately blunt — it is safe to over-redact a support
/// report. Digit *groups* joined by spaces, dashes, dots or brackets are treated as one run,
/// so `+1 (302) 555-0142`, `302-555-0142` and `3025550142` all go once they reach seven
/// digits, while short numbers (`v0.5.7`, a `76%` battery, a year) and ISO dates like
/// `2026-10-05` stay so the log keeps its timestamps.
pub fn redact(text: &str) -> String {
    redact_emails(&redact_number_runs(&redact_bluetooth_addresses(text)))
}

/// Who's signed in to Windows, kept out of the report: the profile folder (`C:\Users\Sam Lee`,
/// which shows up in every logged path), its 8.3 short form (`C:\Users\SAMLEE~1`, which some
/// Windows APIs hand back) and the account name.
#[derive(Debug, Clone, Default)]
pub struct WindowsUser {
    pub profile: Option<String>,
    /// The profile folder's 8.3 short form, when Windows has one that differs.
    pub short_profile: Option<String>,
    pub name: Option<String>,
}

/// How long the short-path lookup may take before the report goes without it.
const SHORT_PATH_LIMIT: std::time::Duration = std::time::Duration::from_millis(500);

impl WindowsUser {
    pub fn current() -> WindowsUser {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let profile = var("USERPROFILE");
        let short_profile = profile
            .clone()
            .and_then(short_path_limited)
            .filter(|s| Some(s) != profile.as_ref());
        WindowsUser {
            profile,
            short_profile,
            name: var("USERNAME"),
        }
    }
}

/// `short_path` on a helper thread, given up on after `SHORT_PATH_LIMIT` (a file-system call).
fn short_path_limited(long: String) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("tug-short-path".into())
        .spawn(move || {
            let _ = tx.send(short_path(&long));
        })
        .ok()?;
    rx.recv_timeout(SHORT_PATH_LIMIT).ok().flatten()
}

/// The 8.3 short form of an existing path (GetShortPathNameW), if Windows has one.
#[cfg(windows)]
fn short_path(long: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let long = HSTRING::from(long);
    // SAFETY: a size query, then a call into a buffer of that size; both only read `long`.
    unsafe {
        let needed = GetShortPathNameW(&long, None);
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u16; needed as usize];
        let n = GetShortPathNameW(&long, Some(&mut buf)) as usize;
        (n > 0 && n < buf.len()).then(|| String::from_utf16_lossy(&buf[..n]))
    }
}

#[cfg(not(windows))]
fn short_path(_long: &str) -> Option<String> {
    None
}

/// Account names that are also ordinary words: replacing them would fill the report with
/// `[user]` ("the user tapped…", "Run as administrator") and hide nothing.
const COMMON_NAMES: &[&str] = &["user", "owner", "admin", "administrator", "guest"];

/// Whether the bare account name is worth masking on its own: four or more characters and not a
/// common word. (The profile folder is always masked.)
fn maskable_name(name: &str) -> bool {
    name.chars().count() >= 4 && !COMMON_NAMES.iter().any(|w| w.eq_ignore_ascii_case(name))
}

/// Percent-encode everything but unreserved characters and `keep` (as in a URL).
fn percent_encode(s: &str, keep: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || keep.contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The ways a folder shows up in a log: as is, forward slashes, the doubled backslashes of a
/// debug-printed path, and URL-encoded (as a URL's path, `C:/Users/Sam%20Lee`, and as one encoded
/// component, `C%3A%5CUsers%5CSam%20Lee`, either slash).
fn path_variants(path: &str) -> Vec<String> {
    let slash = path.replace('\\', "/");
    let mut out: Vec<String> = Vec::new();
    for v in [
        path.to_string(),
        slash.clone(),
        path.replace('\\', "\\\\"),
        percent_encode(&slash, b"/:"),
        percent_encode(path, b""),
        percent_encode(&slash, b""),
    ] {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// Replace the profile folder (and its 8.3 short form) with `%USERPROFILE%` (any case, either
/// slash, doubled backslashes, URL-encoded), then the account name, as a whole word (and
/// URL-encoded), with `[user]` when it's long enough and not a common word.
pub fn redact_user(text: &str, user: &WindowsUser) -> String {
    let mut out = text.to_string();
    let folders = [user.profile.as_deref(), user.short_profile.as_deref()];
    for folder in folders
        .into_iter()
        .flatten()
        .map(|p| p.trim().trim_end_matches(['\\', '/']))
        .filter(|p| p.len() >= 3)
    {
        for v in path_variants(folder) {
            out = replace_ignoring_case(&out, &v, "%USERPROFILE%", false);
        }
    }
    if let Some(name) = user.name.as_deref().map(str::trim).filter(|n| maskable_name(n)) {
        // Encoded, it's only different when the name has a space or symbol, which makes it
        // specific enough to mask without word boundaries (it often follows another `%20`).
        let encoded = percent_encode(name, b"");
        if encoded != name {
            out = replace_ignoring_case(&out, &encoded, "[user]", false);
        }
        out = replace_ignoring_case(&out, name, "[user]", true);
    }
    out
}

/// Replace every case-insensitive occurrence of `needle`; with `whole_word`, only where it isn't
/// part of a longer word.
fn replace_ignoring_case(text: &str, needle: &str, with: &str, whole_word: bool) -> String {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return text.to_string();
    }
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let matches = i + needle.len() <= chars.len()
            && needle.iter().enumerate().all(|(k, &n)| same(chars[i + k].1, n))
            && (!whole_word
                || (!word(i.checked_sub(1).map(|j| chars[j].1)) && !word(chars.get(i + needle.len()).map(|c| c.1))));
        if matches {
            out.push_str(with);
            i += needle.len();
        } else {
            out.push(chars[i].1);
            i += 1;
        }
    }
    out
}

/// Mask the phone's own name(s) (e.g. "Jordan's iPhone" names a person) wherever they appear.
pub fn redact_names(text: &str, names: &[String]) -> String {
    let mut out = text.to_string();
    for name in names.iter().map(|n| n.trim()).filter(|n| n.len() >= 3) {
        out = out.replace(name, "[phone]");
    }
    out
}

/// Bluetooth addresses (`00:11:22:33:44:55`, also inside Windows device ids) identify the phone.
fn redact_bluetooth_addresses(text: &str) -> String {
    let b = text.as_bytes();
    let is_hex = |c: u8| c.is_ascii_hexdigit();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let mac = i + 17 <= b.len()
            && (0..6).all(|k| is_hex(b[i + 3 * k]) && is_hex(b[i + 3 * k + 1]))
            && (0..5).all(|k| b[i + 3 * k + 2] == b':')
            // No leading boundary: Windows glues the address onto "BluetoothLE…" (hex "E").
            && (i + 17 == b.len() || !is_hex(b[i + 17]));
        if mac {
            out.push_str("[address]");
            i += 17;
        } else {
            let ch = text[i..].chars().next().expect("in bounds");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// The status snapshot without what identifies the phone: its name and device ids.
pub fn anonymize_status(status: &DeviceStatus) -> DeviceStatus {
    let mut s = status.clone();
    if let Some(d) = s.device.as_mut() {
        d.id = "[device id]".into();
        d.name = "[phone]".into();
    }
    if s.texts_device.is_some() {
        s.texts_device = Some("[phone]".into());
    }
    s
}

const NUMBER_SEP: &[u8] = b" -.()+";

fn is_number_sep(c: u8) -> bool {
    NUMBER_SEP.contains(&c)
}

/// Replace phone/account-number-shaped runs (seven or more digits, optionally grouped) with
/// `[number]`, keeping short numbers and ISO dates verbatim.
fn redact_number_runs(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let starts = c.is_ascii_digit() || ((c == b'+' || c == b'(') && digit_follows(b, i + 1));
        if starts {
            let run = scan_number(b, i);
            if run.digits >= 7 && !is_date_shape(&run.groups, run.all_dash) {
                out.push_str("[number]");
            } else {
                out.push_str(&text[i..run.end]);
            }
            i = run.end;
        } else {
            let end = (i + utf8_len(c)).min(text.len());
            out.push_str(&text[i..end]);
            i = end;
        }
    }
    out
}

/// A digit appears within two separators of `j` (so a lone `+` or `(` isn't a number start).
fn digit_follows(b: &[u8], j: usize) -> bool {
    let mut k = j;
    let mut seps = 0;
    while k < b.len() && is_number_sep(b[k]) && seps < 2 {
        k += 1;
        seps += 1;
    }
    k < b.len() && b[k].is_ascii_digit()
}

struct NumberRun {
    /// Byte index just past the run (a trailing `)` is included, trailing separators aren't).
    end: usize,
    digits: usize,
    /// The length of each digit group, left to right.
    groups: Vec<usize>,
    /// Every separator between groups was a dash (helps spot ISO dates).
    all_dash: bool,
}

/// Walk a number run starting at `start` (a digit, or a `+`/`(` marker), crossing up to two
/// separators between digit groups.
fn scan_number(b: &[u8], start: usize) -> NumberRun {
    let mut i = start;
    if b[i] == b'+' || b[i] == b'(' {
        i += 1;
    }
    let mut groups = Vec::new();
    let mut digits = 0;
    let mut all_dash = true;
    let mut last_digit_end = i;
    loop {
        let gstart = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let glen = i - gstart;
        if glen == 0 {
            break;
        }
        groups.push(glen);
        digits += glen;
        last_digit_end = i;
        // Try to cross up to two separators to the next digit group.
        let mut j = i;
        let mut seps = 0;
        while j < b.len() && is_number_sep(b[j]) && seps < 2 {
            if b[j] != b'-' {
                all_dash = false;
            }
            j += 1;
            seps += 1;
        }
        if seps > 0 && j < b.len() && b[j].is_ascii_digit() {
            i = j;
        } else {
            break;
        }
    }
    let mut end = last_digit_end;
    if end < b.len() && b[end] == b')' {
        end += 1;
    }
    NumberRun {
        end,
        digits,
        groups,
        all_dash,
    }
}

/// `YYYY-MM-DD` (three dash-joined groups of 4, 2, 2) is a date, not a number to redact.
fn is_date_shape(groups: &[usize], all_dash: bool) -> bool {
    all_dash && groups == [4, 2, 2]
}

/// Length in bytes of the UTF-8 char that starts with `first`.
fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Replace `local@domain.tld` addresses with `[email]`, keeping any wrapping punctuation
/// (`<…>`, a trailing comma) around them.
fn redact_emails(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if is_email_char(c) || c == b'@' {
            let start = i;
            while i < b.len() && (is_email_char(b[i]) || b[i] == b'@') {
                i += 1;
            }
            let run = &text[start..i];
            // An address doesn't end in punctuation; keep whatever trails it.
            let core = run.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
            if is_email(core) {
                out.push_str("[email]");
                out.push_str(&run[core.len()..]);
            } else {
                out.push_str(run);
            }
        } else {
            let end = (i + utf8_len(c)).min(text.len());
            out.push_str(&text[i..end]);
            i = end;
        }
    }
    out
}

/// Exactly one `@`, a non-empty local part, and a domain with a dot ending in a letter/digit.
fn is_email(s: &str) -> bool {
    let at = match s.find('@') {
        Some(a) => a,
        None => return false,
    };
    let local = &s[..at];
    let domain = &s[at + 1..];
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && domain.bytes().all(|b| is_email_char(b) || b == b'.')
        && domain.chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric())
}

fn is_email_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

/// The gathered, non-sensitive facts for the report. Strings are inserted verbatim except the
/// log lines, which are redacted line by line.
pub struct Report<'a> {
    pub app_version: &'a str,
    pub windows_version: &'a str,
    pub bluetooth: &'a str,
    /// The `DeviceStatus` snapshot as pretty JSON (already free of message bodies and numbers).
    pub status_json: &'a str,
    /// `(label, value)` settings flags, non-sensitive only.
    pub settings: &'a [(String, String)],
    pub log_lines: &'a [String],
    /// The phone's name(s), masked wherever they appear in the log tail.
    pub phone_names: &'a [String],
    /// The Windows profile folder and account name, masked everywhere.
    pub user: &'a WindowsUser,
}

impl Report<'_> {
    /// Build the full plain-text report. Pure: redaction is applied to the status snapshot and
    /// every log line here, so the result is safe to share.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("tug diagnostics\n");
        out.push_str("===============\n\n");
        out.push_str(&format!("app version:     {}\n", self.app_version));
        out.push_str(&format!("windows version: {}\n", self.windows_version));
        out.push_str(&format!("bluetooth:       {}\n", self.bluetooth));
        out.push('\n');

        out.push_str("device status\n-------------\n");
        out.push_str(&redact(&redact_user(self.status_json, self.user)));
        out.push_str("\n\n");

        out.push_str("settings\n--------\n");
        if self.settings.is_empty() {
            out.push_str("(none)\n");
        } else {
            for (k, v) in self.settings {
                out.push_str(&format!("{k}: {v}\n"));
            }
        }
        out.push('\n');

        out.push_str(&format!("recent log ({} lines)\n", self.log_lines.len()));
        out.push_str("----------\n");
        if self.log_lines.is_empty() {
            out.push_str("(no log files found)\n");
        } else {
            for line in self.log_lines {
                out.push_str(&redact(&redact_user(&redact_names(line, self.phone_names), self.user)));
                out.push('\n');
            }
        }
        out
    }
}

/// Read the last `n` lines across the rotated `*.log` files in `dir`, newest file last so the
/// report ends with the most recent entries. Missing dir or unreadable files yield an empty
/// list rather than an error: a report without logs is still useful.
pub fn recent_log_lines(dir: &Path, n: usize) -> Vec<String> {
    let mut files: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
            .collect(),
        Err(_) => return Vec::new(),
    };
    // Oldest first by modified time, so concatenating keeps chronological order.
    files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());

    let mut lines: Vec<String> = Vec::new();
    for entry in &files {
        if let Ok(text) = fs::read_to_string(entry.path()) {
            lines.extend(text.lines().map(str::to_owned));
        }
    }
    let start = lines.len().saturating_sub(n);
    lines.split_off(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_phone_numbers_in_many_shapes() {
        assert_eq!(redact("call +13025550142 now"), "call [number] now");
        assert_eq!(redact("from (302) 555-0142 today"), "from [number] today");
        assert_eq!(redact("num 302-555-0142."), "num [number].");
        assert_eq!(redact("digits 3025550142 end"), "digits [number] end");
        assert_eq!(redact("intl +1 302 555 0142 end"), "intl [number] end");
    }

    #[test]
    fn keeps_short_numbers_and_versions() {
        assert_eq!(redact("tug v0.5.7"), "tug v0.5.7");
        assert_eq!(redact("battery 76%"), "battery 76%");
        assert_eq!(redact("year 2026 code 482913"), "year 2026 code 482913");
        // Exactly 7 digits is masked; 6 is kept.
        assert_eq!(redact("id 123456 and 1234567"), "id 123456 and [number]");
    }

    #[test]
    fn masks_email_addresses_but_keeps_punctuation() {
        assert_eq!(redact("to jordan.lee@example.com please"), "to [email] please");
        assert_eq!(redact("<someone@example.com>"), "<[email]>");
        assert_eq!(redact("ping a@b.co, thanks"), "ping [email], thanks");
    }

    #[test]
    fn plain_text_is_untouched() {
        let s = "the texts pairing is broken; pair again";
        assert_eq!(redact(s), s);
    }

    #[test]
    fn redacts_a_realistic_log_line() {
        let line = "2026-10-05T10:11:12 [INFO] message from +13025550142 to jordan@example.com saved";
        assert_eq!(
            redact(line),
            "2026-10-05T10:11:12 [INFO] message from [number] to [email] saved"
        );
    }

    #[test]
    fn report_renders_sections_and_redacts_status_and_logs() {
        let settings = vec![
            ("advertise".to_string(), "true".to_string()),
            ("ui.toasts".to_string(), "true".to_string()),
        ];
        let logs = vec!["called +13025550142".to_string(), "all good".to_string()];
        let report = Report {
            app_version: "0.5.7",
            windows_version: "Windows 11",
            bluetooth: "radio on, peripheral supported",
            status_json: "{\n  \"textsDevice\": \"call 3025550142\"\n}",
            settings: &settings,
            log_lines: &logs,
            phone_names: &[],
            user: &WindowsUser::default(),
        };
        let out = report.render();
        assert!(out.contains("app version:     0.5.7"));
        assert!(out.contains("advertise: true"));
        assert!(out.contains("called [number]"));
        assert!(out.contains("\"textsDevice\": \"call [number]\""));
        assert!(!out.contains("3025550142"));
        assert!(out.contains("recent log (2 lines)"));
    }

    #[test]
    fn the_phone_is_never_named_or_addressed() {
        let status = DeviceStatus {
            device: Some(crate::state::PairedDevice {
                id: "BluetoothLE#BluetoothLE00:11:22:33:44:55-66:77:88:99:aa:bb".into(),
                name: "My iPhone".into(),
                model: Some("iPhone16,2".into()),
            }),
            texts_device: Some("My iPhone".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&anonymize_status(&status)).unwrap();
        assert!(!json.contains("My iPhone") && !json.contains("88:99"), "{json}");

        let names = vec!["My iPhone".to_string()];
        let logs = vec![
            "message access connected to My iPhone".to_string(),
            "connecting to BluetoothLE#BluetoothLE00:11:22:33:44:55-66:77:88:99:aa:bb".to_string(),
            "[2026-10-05][14:52:21][tug_lib::map][INFO] build 10.0.26200".to_string(),
        ];
        let out = Report {
            app_version: "0.5.7",
            windows_version: "Windows 11",
            bluetooth: "radio on",
            status_json: &json,
            settings: &[],
            log_lines: &logs,
            phone_names: &names,
            user: &WindowsUser::default(),
        }
        .render();
        assert!(!out.contains("My iPhone"), "{out}");
        assert!(out.contains("connected to [phone]"));
        assert!(out.contains("BluetoothLE#BluetoothLE[address]-[address]"));
        assert!(out.contains("[2026-10-05][14:52:21]"), "times stay readable: {out}");
    }

    #[test]
    fn the_short_and_url_encoded_profile_folder_are_masked_too() {
        let user = WindowsUser {
            profile: Some(r"C:\Users\Sam Lee".into()),
            short_profile: Some(r"C:\Users\SAMLEE~1".into()),
            name: Some("Sam Lee".into()),
        };
        // The 8.3 short form some Windows APIs hand back, either slash.
        assert_eq!(
            redact_user(
                r"temp C:\Users\SAMLEE~1\AppData\Local\Temp\x and c:/users/samlee~1/y",
                &user
            ),
            r"temp %USERPROFILE%\AppData\Local\Temp\x and %USERPROFILE%/y"
        );
        // URL-encoded: a file URL's path, and the whole path as one encoded component.
        assert_eq!(
            redact_user("open file:///C:/Users/Sam%20Lee/Pictures/a.png", &user),
            "open file:///%USERPROFILE%/Pictures/a.png"
        );
        assert_eq!(
            redact_user("dir=C%3A%5CUsers%5CSam%20Lee%5CDownloads&x=1", &user),
            "dir=%USERPROFILE%%5CDownloads&x=1"
        );
        assert_eq!(
            redact_user("dir=c%3a%2fusers%2fsam%20lee%2fx", &user),
            "dir=%USERPROFILE%%2fx"
        );
        // The account name URL-encoded on its own.
        assert_eq!(redact_user("OneDrive%20-%20Sam%20Lee", &user), "OneDrive%20-%20[user]");
    }

    /// The Windows side: an existing folder has a short form (the same path when 8.3 names are
    /// off), a missing one has none, and the time-limited wrapper agrees.
    #[cfg(windows)]
    #[test]
    fn short_path_reads_an_existing_folder_only() {
        let temp = std::env::temp_dir().display().to_string();
        assert!(short_path(&temp).is_some_and(|s| !s.is_empty()));
        assert_eq!(short_path(r"C:\tug-no-such-folder\x"), None);
        assert_eq!(short_path_limited(temp.clone()), short_path(&temp));
    }

    #[test]
    fn short_or_common_account_names_arent_masked_on_their_own() {
        // "[user]" everywhere the word "user" appears would be noise and hide nothing; the profile
        // folder is still masked.
        for name in ["user", "Owner", "ADMIN", "Administrator", "guest", "al", "Jo"] {
            let user = WindowsUser {
                profile: Some(format!(r"C:\Users\{name}")),
                short_profile: None,
                name: Some(name.into()),
            };
            let line = format!(r"the user tapped Run as administrator; owner guest al Jo; C:\Users\{name}\x");
            let out = redact_user(&line, &user);
            assert!(
                out.starts_with("the user tapped Run as administrator; owner guest al Jo;"),
                "{out}"
            );
            assert!(out.ends_with(r"%USERPROFILE%\x"), "{out}");
        }
        // Four or more characters, not a common word: masked.
        let user = WindowsUser {
            profile: None,
            short_profile: None,
            name: Some("Jordan".into()),
        };
        assert_eq!(redact_user("signed in as jordan.", &user), "signed in as [user].");
        assert!(maskable_name("Robin") && !maskable_name("Kim") && !maskable_name("Guest"));
    }

    #[test]
    fn the_windows_user_is_never_named() {
        let user = WindowsUser {
            profile: Some(r"C:\Users\Sam Lee".into()),
            short_profile: None,
            name: Some("Sam Lee".into()),
        };
        assert_eq!(
            redact_user(
                r"couldn't open c:\users\SAM LEE\AppData\Roaming\x\tug.db: locked",
                &user
            ),
            r"couldn't open %USERPROFILE%\AppData\Roaming\x\tug.db: locked"
        );
        assert_eq!(
            redact_user(r#"path "C:\\Users\\Sam Lee\\Pictures" and C:/Users/Sam Lee/x"#, &user),
            r#"path "%USERPROFILE%\\Pictures" and %USERPROFILE%/x"#
        );
        // The account name elsewhere (another drive, OneDrive's folder name), as a whole word.
        assert_eq!(
            redact_user(r"D:\Sam Lee\files; OneDrive - Sam Lee", &user),
            r"D:\[user]\files; OneDrive - [user]"
        );
        // A short account name is left alone as a word; its profile folder still goes.
        let short = WindowsUser {
            profile: Some(r"C:\Users\al".into()),
            short_profile: None,
            name: Some("al".into()),
        };
        assert_eq!(
            redact_user(r"all good, al: C:\Users\al\x", &short),
            r"all good, al: %USERPROFILE%\x"
        );
        // Nothing known about the user: untouched.
        assert_eq!(redact_user(r"C:\Users\x", &WindowsUser::default()), r"C:\Users\x");

        let logs = vec![r"[INFO] set the database aside as C:\Users\Sam Lee\AppData\tug.db.bad".to_string()];
        let out = Report {
            app_version: "0.5.16",
            windows_version: "Windows 11",
            bluetooth: "radio on",
            status_json: "{}",
            settings: &[],
            log_lines: &logs,
            phone_names: &[],
            user: &user,
        }
        .render();
        assert!(!out.contains("Sam"), "{out}");
        assert!(out.contains(r"%USERPROFILE%\AppData\tug.db.bad"), "{out}");
    }

    #[test]
    fn bluetooth_summary_is_number_free_and_readable() {
        let on = DeviceStatus {
            radio: RadioState::On,
            peripheral_supported: Some(true),
            ..Default::default()
        };
        assert_eq!(bluetooth_summary(&on), "radio on, peripheral role supported");
        let off = DeviceStatus {
            radio: RadioState::Off,
            peripheral_supported: Some(false),
            ..Default::default()
        };
        assert_eq!(bluetooth_summary(&off), "radio off, peripheral role unsupported");
        assert_eq!(
            bluetooth_summary(&DeviceStatus::default()),
            "radio unknown, peripheral role unknown"
        );
    }

    #[test]
    fn missing_log_dir_is_empty_not_an_error() {
        let lines = recent_log_lines(Path::new("does/not/exist"), 300);
        assert!(lines.is_empty());
    }

    #[test]
    fn recent_log_lines_takes_the_tail_across_files() {
        let dir = std::env::temp_dir().join(format!("tug-diag-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("tug.2026.log");
        let new = dir.join("tug.log");
        fs::write(&old, "a\nb\nc\n").unwrap();
        // Make the second file newer so it sorts last.
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&new, "d\ne\nf\n").unwrap();
        let lines = recent_log_lines(&dir, 4);
        assert_eq!(lines, vec!["c", "d", "e", "f"]);
        fs::remove_dir_all(&dir).ok();
    }
}
