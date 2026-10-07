//! What the `tug` command prints, and its exit codes. Pure.

use tug_bridge::client::ClientError;
use tug_bridge::protocol::{
    printable, CodeResult, ErrorCode, OfferResult, Permission, PhoneStatus, SendOutcome, SendResult, StatusResult,
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
    let mut out = vec![format!("tug {} is running.", s.version), phone_line(&s.phone)];
    if let Some(np) = &s.phone.now_playing {
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

pub fn offer_lines(r: &OfferResult) -> Vec<String> {
    let mut out = Vec::new();
    match r.offered {
        0 => out.push("Nothing was sent.".into()),
        1 => out.push("1 file is waiting for your phone in Tugboat.".into()),
        n => out.push(format!("{n} files are waiting for your phone in Tugboat.")),
    }
    if r.offered > 0 {
        out.push("If your phone isn't connected yet, scan the code in tug with its camera.".into());
    }
    for s in &r.skipped {
        out.push(format!("Skipped {}: {}.", printable(&s.name), printable(&s.reason)));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use tug_bridge::protocol::{BridgeError, NowPlayingInfo, SkippedFile};

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
            permissions: perms,
            phone: p,
        });
        assert_eq!(lines[0], "tug 0.5.12 is running.");
        assert!(lines.iter().any(|l| l == "Now playing: Song — Band (paused)."));
        assert!(lines
            .iter()
            .any(|l| l.contains("Off: Verification codes, Music controls, Send texts.")));
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

    #[test]
    fn offers() {
        let lines = offer_lines(&OfferResult {
            offered: 2,
            skipped: vec![SkippedFile {
                name: "big.mov".into(),
                reason: "too big".into(),
            }],
        });
        assert_eq!(lines[0], "2 files are waiting for your phone in Tugboat.");
        assert_eq!(lines.last().unwrap(), "Skipped big.mov: too big.");
    }
}
