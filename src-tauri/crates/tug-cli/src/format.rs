//! What the `tug` command prints, and its exit codes. Pure.

use tug_bridge::client::ClientError;
use tug_bridge::protocol::{
    printable, CodeResult, ErrorCode, OfferOutcome, OfferResult, Permission, PhoneStatus, Progress, SendOutcome,
    SendResult, StatusResult,
};

/// Exit codes: scripts can tell "didn't work" from "couldn't reach tug".
pub const OK: i32 = 0;
/// It ran, but: no code, not sent, a switch is off, rate limited, phone not connected.
pub const FAILED: i32 = 1;
/// Typed wrong.
pub const USAGE: i32 = 2;
/// tug isn't running, AI tools are off, or access isn't set up.
pub const UNREACHABLE: i32 = 3;

pub fn exit_code(e: &ClientError) -> i32 {
    match e {
        ClientError::NotRunning | ClientError::NotSetUp | ClientError::Impostor => UNREACHABLE,
        ClientError::Bridge(b) => match b.code {
            ErrorCode::Off | ErrorCode::Unauthorized | ErrorCode::Version => UNREACHABLE,
            ErrorCode::Invalid => USAGE,
            _ => FAILED,
        },
        ClientError::Timeout | ClientError::SendTimeout | ClientError::Other(_) => FAILED,
    }
}

pub fn age(seconds: i64) -> String {
    match seconds {
        s if s < 45 => "just now".into(),
        s if s < 90 => "1 minute ago".into(),
        s if s < 3600 => format!("{} minutes ago", (s + 30) / 60),
        s if s < 5400 => "1 hour ago".into(),
        s => format!("{} hours ago", (s + 1800) / 3600),
    }
}

/// `tug code`: the code alone goes to stdout (so `tug code | clip` works); this line goes to stderr.
/// Sender names, song titles and file names come from other people: printable keeps escape
/// sequences and invisible characters out of the terminal.
pub fn code_detail(c: &CodeResult) -> String {
    let (from, app) = (printable(&c.from), printable(&c.app));
    let from = if from.trim().is_empty() || from == app {
        app
    } else {
        format!("{from} ({app})")
    };
    let copied = if c.copied { " Copied to the clipboard." } else { "" };
    format!("From {from}, {}.{copied}", age(c.age_seconds))
}

pub fn phone_line(p: &PhoneStatus) -> String {
    let phone = p.model.as_deref().unwrap_or("iPhone");
    match p.connection.as_str() {
        "no_phone" => "iPhone: not set up in tug yet.".into(),
        "connected" => {
            let mut s = format!("{phone}: connected");
            if let Some(b) = p.battery_percent {
                s.push_str(&format!(", battery {b}%"));
            }
            if !p.texts {
                s.push_str(" (texts not connected)");
            }
            s.push('.');
            s
        }
        "connecting" => format!("{phone}: connecting…"),
        _ => format!("{phone}: not connected."),
    }
}

pub fn status_lines(s: &StatusResult) -> Vec<String> {
    let mut out = vec![format!("tug {} is running.", s.version)];
    match &s.phone {
        Some(p) => out.push(phone_line(p)),
        None => out.push(format!(
            "iPhone: hidden (\"{}\" is off in tug › Settings › Developer tools).",
            Permission::PhoneStatus.label()
        )),
    }
    if let Some(np) = s.phone.as_ref().and_then(|p| p.now_playing.as_ref()) {
        let title = np.title.as_deref().map_or_else(|| "Unknown".into(), printable);
        let by = np
            .artist
            .as_deref()
            .map(|a| format!(" — {}", printable(a)))
            .unwrap_or_default();
        out.push(format!("Now playing: {title}{by} ({}).", np.state));
    }
    let names = |on: bool| {
        Permission::ALL
            .into_iter()
            .filter(|p| s.permissions.get(p).copied().unwrap_or(false) == on)
            .map(|p| p.label())
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push("AI tools: on.".into());
    let (on, off) = (names(true), names(false));
    if !on.is_empty() {
        out.push(format!("  On: {on}."));
    }
    if !off.is_empty() {
        out.push(format!("  Off: {off}."));
    }
    out
}

/// `tug text`: the outcome sentence and whether it counts as success.
pub fn send_outcome(r: &SendResult) -> (String, bool) {
    match r.outcome {
        SendOutcome::Sent => (
            format!(
                "Sent to {}.{}",
                printable(&r.to),
                r.detail
                    .as_deref()
                    .map(|d| format!(" {}", printable(d)))
                    .unwrap_or_default()
            ),
            true,
        ),
        SendOutcome::Declined => (
            format!("Not sent: you chose Don't send for {}.", printable(&r.to)),
            false,
        ),
        SendOutcome::TimedOut => ("Not sent: nobody confirmed it in tug within 2 minutes.".into(), false),
        SendOutcome::Cancelled => ("Not sent: it was cancelled in tug.".into(), false),
        SendOutcome::Failed => (
            format!(
                "Not sent: the phone didn't take it{}.",
                r.detail
                    .as_deref()
                    .map(|d| format!(" ({})", printable(d)))
                    .unwrap_or_default()
            ),
            false,
        ),
        // Saved in tug and still on its way: counted as success, so a script that retries on
        // failure can't send it twice.
        SendOutcome::Queued => (
            format!(
                "Queued for {}: tug is still waiting for your iPhone, so it may still send. Check the conversation in tug. Don't send it again.",
                printable(&r.to)
            ),
            true,
        ),
    }
}

/// "report.pdf", or "3 files": what `tug boat` is sending, in a sentence.
pub fn files_named(names: &[String]) -> String {
    match names {
        [one] => printable(one),
        many => format!("{} files", many.len()),
    }
}

/// "2 minutes", "90 seconds".
pub fn seconds_in_words(secs: u32) -> String {
    match secs {
        60 => "1 minute".into(),
        s if s % 60 == 0 => format!("{} minutes", s / 60),
        1 => "1 second".into(),
        s => format!("{s} seconds"),
    }
}

/// What `tug text` says on stderr as it goes: once tug shows its card, and once it's sending.
pub fn text_progress(p: &Progress) -> Option<&'static str> {
    match p {
        Progress::Confirming => Some("Check tug: Send or Don't send (it won't send by itself; 2 minutes)…"),
        Progress::Sending => Some("Sending…"),
        Progress::Waiting { .. } => None,
    }
}

/// `tug boat`'s progress, one line on stderr each time it changes.
pub fn boat_progress(p: &Progress) -> String {
    match p {
        Progress::Confirming => "Check tug: Send or Don't send (nothing is sent by itself; 2 minutes)…".into(),
        // Only texts are told this; said plainly if it ever comes.
        Progress::Sending => "Sending…".into(),
        Progress::Waiting {
            names,
            phone_open,
            fetched,
            total,
        } => {
            let what = files_named(names);
            if *fetched > 0 {
                format!("Your iPhone has downloaded {fetched} of {total}…")
            } else if *phone_open {
                let next_to = if names.len() == 1 { what } else { "each file".into() };
                format!("On your iPhone, tap Get next to {next_to} in Tugboat…")
            } else {
                format!("Scan the code in tug to receive {what} on your iPhone…")
            }
        }
    }
}

/// `tug boat`'s answer: the sentence for stdout, and whether it counts as success (exit 0): the
/// phone downloaded everything, or with `--no-wait`, it's on offer.
pub fn boat_outcome(r: &OfferResult) -> (String, bool) {
    let what = files_named(&r.names);
    let held = seconds_in_words(r.hold_secs);
    match r.outcome {
        OfferOutcome::Delivered => (
            format!("Delivered: your iPhone downloaded {what}. Tap Save there to keep it."),
            true,
        ),
        OfferOutcome::Offered => (
            format!(
                "{what} {} waiting for your iPhone in Tugboat for the next {held}.",
                if r.names.len() == 1 { "is" } else { "are" }
            ),
            true,
        ),
        OfferOutcome::TimedOut => (
            format!("Waiting for your iPhone timed out after {held}, so tug stopped offering {what}."),
            false,
        ),
        OfferOutcome::Declined => ("Not sent: you chose Don't send in tug.".into(), false),
        OfferOutcome::NotConfirmed => ("Not sent: nobody confirmed it in tug within 2 minutes.".into(), false),
        OfferOutcome::Cancelled => (
            format!(
                "Not sent: {}",
                r.detail
                    .as_deref()
                    .map(printable)
                    .unwrap_or_else(|| "it was cancelled in tug.".into())
            ),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use tug_bridge::protocol::{BridgeError, NowPlayingInfo};

    fn phone(connection: &str) -> PhoneStatus {
        PhoneStatus {
            connected: connection == "connected",
            connection: connection.into(),
            model: Some("iPhone 15 Pro Max".into()),
            battery_percent: Some(82),
            notifications: true,
            texts: true,
            now_playing: None,
        }
    }

    #[test]
    fn exit_codes_separate_unreachable_from_failed() {
        assert_eq!(exit_code(&ClientError::NotRunning), UNREACHABLE);
        assert_eq!(
            exit_code(&ClientError::Bridge(BridgeError::new(ErrorCode::Off, "off"))),
            UNREACHABLE
        );
        assert_eq!(
            exit_code(&ClientError::Bridge(BridgeError::new(ErrorCode::ToolOff, "x"))),
            FAILED
        );
        assert_eq!(
            exit_code(&ClientError::Bridge(BridgeError::new(ErrorCode::Invalid, "x"))),
            USAGE
        );
        assert_eq!(exit_code(&ClientError::Timeout), FAILED);
    }

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(
            ClientError::NotRunning.to_string(),
            "tug isn't running. Open tug and try again."
        );
        for e in [ClientError::NotSetUp, ClientError::Timeout, ClientError::Impostor] {
            let s = e.to_string();
            assert!(s.ends_with('.'), "{s}");
            assert!(s.chars().next().unwrap().is_alphabetic(), "{s}");
        }
    }

    #[test]
    fn ages() {
        assert_eq!(age(5), "just now");
        assert_eq!(age(70), "1 minute ago");
        assert_eq!(age(150), "3 minutes ago");
        assert_eq!(age(4000), "1 hour ago");
        assert_eq!(age(7200), "2 hours ago");
    }

    #[test]
    fn code_detail_names_sender_and_age() {
        let c = CodeResult {
            code: "482913".into(),
            from: "Example Bank".into(),
            app: "Messages".into(),
            received_at: "2026-10-07T09:30:00Z".into(),
            age_seconds: 120,
            copied: true,
        };
        assert_eq!(
            code_detail(&c),
            "From Example Bank (Messages), 2 minutes ago. Copied to the clipboard."
        );
        // A sender name can't reach the terminal as escape sequences or hidden characters.
        let evil = CodeResult {
            from: "Bank\u{1b}]0;pwned\u{7}\u{202E}\n".into(),
            ..c
        };
        let line = code_detail(&evil);
        assert!(!line.chars().any(|ch| ch.is_control() || ch == '\u{202E}'), "{line:?}");
    }

    #[test]
    fn phone_lines() {
        assert_eq!(
            phone_line(&phone("connected")),
            "iPhone 15 Pro Max: connected, battery 82%."
        );
        assert_eq!(phone_line(&phone("disconnected")), "iPhone 15 Pro Max: not connected.");
        assert_eq!(phone_line(&phone("no_phone")), "iPhone: not set up in tug yet.");
    }

    #[test]
    fn status_lists_switches() {
        let mut p = phone("connected");
        p.now_playing = Some(NowPlayingInfo {
            title: Some("Song".into()),
            artist: Some("Band".into()),
            album: None,
            state: "paused".into(),
            app: None,
        });
        let perms = Permission::ALL
            .into_iter()
            .map(|p| (p, p.default_on()))
            .collect::<BTreeMap<_, _>>();
        let lines = status_lines(&StatusResult {
            version: "0.5.12".into(),
            permissions: perms.clone(),
            phone: Some(p),
        });
        assert_eq!(lines[0], "tug 0.5.12 is running.");
        assert!(lines.iter().any(|l| l == "Now playing: Song — Band (paused)."));
        assert!(lines
            .iter()
            .any(|l| l.contains("Off: Verification codes, Music controls, Send texts, Send files to your iPhone.")));

        // Phone status switched off: tug sends no phone, and the line says why.
        let lines = status_lines(&StatusResult {
            version: "0.5.12".into(),
            permissions: perms,
            phone: None,
        });
        assert!(lines[1].starts_with("iPhone: hidden ("), "{lines:?}");
        assert!(!lines.iter().any(|l| l.starts_with("Now playing")));
    }

    #[test]
    fn send_outcomes() {
        let r = |outcome| SendResult {
            outcome,
            to: "Sam".into(),
            detail: None,
        };
        assert_eq!(send_outcome(&r(SendOutcome::Sent)), ("Sent to Sam.".into(), true));
        assert!(!send_outcome(&r(SendOutcome::Declined)).1);
        assert!(!send_outcome(&r(SendOutcome::TimedOut)).1);
        assert!(send_outcome(&r(SendOutcome::TimedOut)).0.contains("2 minutes"));
        // Still on its way in tug: never a failure (a retrying script would send it twice).
        let (line, ok) = send_outcome(&r(SendOutcome::Queued));
        assert!(ok);
        assert!(line.contains("Don't send it again"), "{line}");
        let unsure = SendResult {
            detail: Some("It may have sent; check your iPhone to be sure. Don't send it again.".into()),
            ..r(SendOutcome::Sent)
        };
        assert!(send_outcome(&unsure).0.contains("Don't send it again"));
    }

    #[test]
    fn a_send_that_ran_out_of_time_says_not_to_resend() {
        assert_eq!(exit_code(&ClientError::SendTimeout), FAILED);
        assert!(ClientError::SendTimeout.to_string().contains("don't send it again"));
    }

    fn result(outcome: OfferOutcome, names: &[&str]) -> OfferResult {
        OfferResult {
            outcome,
            names: names.iter().map(|s| s.to_string()).collect(),
            total_bytes: 2_100_000,
            hold_secs: 120,
            detail: None,
        }
    }

    #[test]
    fn boat_outcomes_and_exit_codes() {
        let (line, ok) = boat_outcome(&result(OfferOutcome::Delivered, &["report.pdf"]));
        assert!(ok);
        assert!(
            line.starts_with("Delivered: your iPhone downloaded report.pdf."),
            "{line}"
        );
        let (line, ok) = boat_outcome(&result(OfferOutcome::Offered, &["a.png", "b.png"]));
        assert!(ok, "--no-wait counts once it's on offer");
        assert_eq!(
            line,
            "2 files are waiting for your iPhone in Tugboat for the next 2 minutes."
        );
        for refused in [
            OfferOutcome::TimedOut,
            OfferOutcome::Declined,
            OfferOutcome::NotConfirmed,
            OfferOutcome::Cancelled,
        ] {
            let (line, ok) = boat_outcome(&result(refused, &["report.pdf"]));
            assert!(!ok, "{refused:?}");
            assert!(line.ends_with('.'), "{line}");
        }
        assert!(boat_outcome(&result(OfferOutcome::TimedOut, &["report.pdf"]))
            .0
            .starts_with("Waiting for your iPhone timed out after 2 minutes"));
        let mut gone = result(OfferOutcome::Cancelled, &["x"]);
        gone.detail = Some("Tugboat was closed in tug.".into());
        assert_eq!(boat_outcome(&gone).0, "Not sent: Tugboat was closed in tug.");
    }

    #[test]
    fn boat_progress_lines() {
        let waiting = |names: &[&str], phone_open, fetched| Progress::Waiting {
            names: names.iter().map(|s| s.to_string()).collect(),
            phone_open,
            fetched,
            total: names.len(),
        };
        assert!(boat_progress(&Progress::Confirming).starts_with("Check tug: Send or Don't send"));
        assert!(text_progress(&Progress::Confirming)
            .unwrap()
            .starts_with("Check tug: Send or Don't send"));
        assert_eq!(text_progress(&Progress::Sending), Some("Sending…"));
        assert_eq!(
            boat_progress(&waiting(&["report.pdf"], false, 0)),
            "Scan the code in tug to receive report.pdf on your iPhone…"
        );
        assert_eq!(
            boat_progress(&waiting(&["a.png", "b.png"], true, 0)),
            "On your iPhone, tap Get next to each file in Tugboat…"
        );
        assert_eq!(
            boat_progress(&waiting(&["a.png", "b.png"], true, 1)),
            "Your iPhone has downloaded 1 of 2…"
        );
        // A file name can't put escape sequences in the terminal.
        let line = boat_progress(&waiting(&["evil\u{1b}[2J.txt"], false, 0));
        assert!(!line.contains('\u{1b}'), "{line:?}");
        assert_eq!(seconds_in_words(90), "90 seconds");
        assert_eq!(seconds_in_words(60), "1 minute");
        assert_eq!(seconds_in_words(600), "10 minutes");
    }
}
