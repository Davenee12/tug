//! "Copy diagnostics": a plain-text report the owner can send for support. It gathers the
//! app and Windows versions, the live `DeviceStatus`, the non-sensitive settings flags and the
//! tail of the rotated log files, with phone numbers, emails, the phone's own name and Bluetooth
//! addresses masked out (tug never logs contact names or message text). The pieces
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
        std::process::Command::new("cmd")
            .args(["/c", "ver"])
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

/// Mask the phone's own name(s) (e.g. "My iPhone" names a person) wherever they appear.
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
        out.push_str(&redact(self.status_json));
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
                out.push_str(&redact(&redact_names(line, self.phone_names)));
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
        assert_eq!(redact("to dave.james@example.com please"), "to [email] please");
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
        let line = "2026-10-05T10:11:12 [INFO] message from +13025550142 to dave@example.com saved";
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
        }
        .render();
        assert!(!out.contains("My iPhone"), "{out}");
        assert!(out.contains("connected to [phone]"));
        assert!(out.contains("BluetoothLE#BluetoothLE[address]-[address]"));
        assert!(out.contains("[2026-10-05][14:52:21]"), "times stay readable: {out}");
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
