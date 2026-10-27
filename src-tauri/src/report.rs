//! "Report a problem" (Settings › About), and noticing that tug didn't close normally last time.
//!
//! Nothing here sends anything. A report is a GitHub issue the person opens in their browser,
//! already filled in with the version and Windows version, and with the diagnostics (redacted, see
//! `diagnostics.rs`) on the clipboard to paste and read through first. The issue title starts with
//! `[v<version>]` so `node scripts/rollout.mjs status` can count the problems reported against a
//! release while it rolls out.
//!
//! A small file, `running`, holds the version while tug runs and goes on every normal way out
//! (Quit, Windows ending the session, an update's installer; `tidy_before_exit`). If it's still
//! there at the next start, that run ended some other way: a crash, but also a power cut or
//! End task. tug then offers, once per version, to report it.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

/// The file in tug's local data folder that says a run is in progress.
pub const RUNNING_FILE: &str = "running";
/// The version tug last offered a "didn't close normally" report for (once per version).
pub const OFFERED_SETTING: &str = "report.uncleanOffered";

const NEW_ISSUE: &str = "https://github.com/Davenee12/tug/issues/new";

/// The title prefix that ties an issue to a version (`scripts/rollout.mjs`, `reportPrefix`).
pub fn title_prefix(version: &str) -> String {
    format!("[v{version}]")
}

/// Percent-encoding for a query value: everything but RFC 3986's unreserved characters.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The new-issue page, on the bug report form, with what tug already knows filled in. `unclean`:
/// it's about tug not closing normally last time.
pub fn issue_url(version: &str, windows: &str, unclean: bool) -> String {
    let title = if unclean {
        format!("{} tug closed unexpectedly", title_prefix(version))
    } else {
        format!("{} ", title_prefix(version))
    };
    let mut fields: Vec<(&str, String)> = vec![
        ("template", "bug_report.yml".into()),
        ("title", title),
        ("tug-version", version.into()),
        ("windows-version", windows.into()),
    ];
    if unclean {
        fields.push((
            "actual",
            "tug closed unexpectedly (tug noticed it didn't close normally the last time it ran).".into(),
        ));
    }
    let query: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
    format!("{NEW_ISSUE}?{}", query.join("&"))
}

/// Whether to offer a report at this start: the last run of *this* version didn't close normally,
/// and tug hasn't offered for this version before. (A run of an older version that didn't close
/// normally isn't this release's problem.)
pub fn offer_unclean(previous_run: Option<&str>, current: &str, offered_for: Option<&str>) -> bool {
    previous_run.map(str::trim) == Some(current) && offered_for != Some(current)
}

/// At startup: what the last run left (`Some(version)` if it didn't close normally), and this run
/// marked as in progress.
pub fn begin_run(dir: &Path, version: &str) -> Option<String> {
    let file = dir.join(RUNNING_FILE);
    let previous = fs::read_to_string(&file).ok();
    if let Err(e) = fs::create_dir_all(dir).and_then(|_| fs::write(&file, version)) {
        log::warn!("report: couldn't mark this run: {e}");
    }
    previous
}

/// What the last run left, read once at startup (`start`).
static LAST_RUN: OnceLock<Option<String>> = OnceLock::new();

/// At startup, once: remember how the last run ended and mark this one as running.
pub fn start(dir: &Path, version: &str) {
    let previous = begin_run(dir, version);
    if let Some(v) = &previous {
        log::info!("report: the last run (tug {}) didn't close normally", v.trim());
    }
    let _ = LAST_RUN.set(previous);
}

/// The version of a last run that didn't close normally, if any.
pub fn last_run() -> Option<&'static str> {
    LAST_RUN.get().and_then(|p| p.as_deref())
}

/// On every normal way out: this run closed normally.
pub fn end_run(dir: &Path) {
    match fs::remove_file(dir.join(RUNNING_FILE)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("report: couldn't mark this run as closed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_issue_is_prefilled_and_tied_to_the_version() {
        let url = issue_url("0.5.22", "Microsoft Windows [Version 10.0.26100.2033]", false);
        assert!(url.starts_with("https://github.com/Davenee12/tug/issues/new?template=bug_report.yml&"));
        assert!(url.contains("&title=%5Bv0.5.22%5D%20&"), "{url}");
        assert!(url.contains("&tug-version=0.5.22&"));
        assert!(url.contains("&windows-version=Microsoft%20Windows%20%5BVersion%2010.0.26100.2033%5D"));
        assert!(!url.contains("actual="));
        // Nothing a shell or Windows could reinterpret (open_url refuses spaces and controls).
        assert!(!url.chars().any(|c| c.is_whitespace() || c.is_control()));
    }

    #[test]
    fn an_unclean_exit_report_says_so() {
        let url = issue_url("0.5.22", "unknown", true);
        assert!(
            url.contains("title=%5Bv0.5.22%5D%20tug%20closed%20unexpectedly"),
            "{url}"
        );
        assert!(url.contains("&actual=tug%20closed%20unexpectedly"));
        assert_eq!(title_prefix("0.5.22"), "[v0.5.22]");
    }

    #[test]
    fn encoding_keeps_only_unreserved_characters() {
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode("a b&c=d#e?"), "a%20b%26c%3Dd%23e%3F");
        assert_eq!(encode("é"), "%C3%A9");
        assert_eq!(encode("line\nnext"), "line%0Anext");
    }

    #[test]
    fn a_report_is_offered_once_for_this_versions_unclean_exit() {
        assert!(offer_unclean(Some("0.5.22"), "0.5.22", None));
        assert!(offer_unclean(Some("0.5.22\n"), "0.5.22", Some("0.5.21")));
        assert!(
            !offer_unclean(Some("0.5.22"), "0.5.22", Some("0.5.22")),
            "already offered"
        );
        assert!(!offer_unclean(None, "0.5.22", None), "closed normally");
        assert!(!offer_unclean(Some("0.5.21"), "0.5.22", None), "an older version's run");
    }

    #[test]
    fn the_running_file_tells_a_normal_close_from_any_other() {
        let dir = std::env::temp_dir().join(format!("tug-report-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(begin_run(&dir, "0.5.22"), None, "first run ever");
        end_run(&dir);
        assert_eq!(begin_run(&dir, "0.5.22"), None, "closed normally");
        // No end_run: it crashed (or the power went).
        assert_eq!(begin_run(&dir, "0.5.22").as_deref(), Some("0.5.22"));
        end_run(&dir);
        end_run(&dir);
        let _ = fs::remove_dir_all(&dir);
    }
}
