//! Automatic updates: every decision, as plain functions with tests. `mod.rs` does the I/O (the
//! plugin's check and download, the schedule's clock, the event to the window) and asks these.
//!
//! What they decide: whether this build can update at all (a real public key, an installed copy),
//! whether a release is newer (never a downgrade), when the next background check is due, what a
//! check's answer leads to, and whether applying is allowed right now (never during a call, a
//! Tugboat transfer or voice typing).

use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;

/// Where the updater looks: the `latest.json` attached to the newest published GitHub release
/// (drafts aren't served here, so nothing reaches anyone until the owner publishes).
pub const ENDPOINT: &str = "https://github.com/Davenee12/tug/releases/latest/download/latest.json";

/// A release's page on GitHub: its notes, and for the portable exe the download.
pub fn release_page(version: &str) -> Option<String> {
    // Only a real version goes into the address (it came from the network for a new release).
    semver::Version::parse(version)
        .ok()
        .map(|v| format!("https://github.com/Davenee12/tug/releases/tag/v{v}"))
}

/// The first background check waits this long after tug starts, so startup (Bluetooth, texts,
/// the window) goes first and a quick open-and-close doesn't touch the network.
pub const STARTUP_DELAY: Duration = Duration::from_secs(90);
/// Then one check every 6 hours.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// After a failed check or download (offline, GitHub down, a bad signature): try again in an hour.
pub const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60 * 60);

/// Whether `pubkey` (tauri.conf.json › plugins › updater › pubkey) is a minisign public key as
/// `npx tauri signer generate` writes it: base64 of an "untrusted comment:" line and a base64 key
/// line holding the algorithm ("Ed"), an 8-byte key id and a 32-byte Ed25519 key. Anything else
/// (empty, a placeholder) means updates aren't set up for this build, and tug never checks.
pub fn pubkey_configured(pubkey: &str) -> bool {
    let b64 = base64::engine::general_purpose::STANDARD;
    let Ok(decoded) = b64.decode(pubkey.trim()) else {
        return false;
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return false;
    };
    let mut lines = text.lines();
    let (Some(comment), Some(key)) = (lines.next(), lines.next()) else {
        return false;
    };
    if !comment.starts_with("untrusted comment:") {
        return false;
    }
    matches!(b64.decode(key.trim()), Ok(k) if k.len() == 42 && k.starts_with(b"Ed"))
}

/// Whether this build may look for updates at all. Without a real key nothing touches the network;
/// debug builds never do either (they aren't what the releases replace).
pub fn updates_enabled(debug_build: bool, pubkey: &str) -> bool {
    !debug_build && pubkey_configured(pubkey)
}

/// The portable exe has no installer to run an update, so it can only point at the download. An
/// installed tug has NSIS's `uninstall.exe` beside it (per-user install, `%LOCALAPPDATA%\tug`).
pub fn is_portable(uninstaller_beside_exe: bool) -> bool {
    !uninstaller_beside_exe
}

/// Whether `candidate` is a newer version than `current`. Equal, older or unreadable versions are
/// never an update: tug refuses to downgrade, whatever the server says.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (semver::Version::parse(current), semver::Version::parse(candidate)) {
        (Ok(c), Ok(n)) => is_newer_version(&c, &n),
        _ => false,
    }
}

/// `is_newer` for parsed versions (the plugin's version comparator).
pub fn is_newer_version(current: &semver::Version, candidate: &semver::Version) -> bool {
    candidate > current
}

/// Whether a background check is due. `started_ms` is when tug started, `last_attempt_ms` the last
/// check (background or Check for updates). A clock that went backwards counts as due rather than
/// waiting out the difference.
pub fn check_due(now_ms: i64, started_ms: i64, last_attempt_ms: Option<i64>, last_failed: bool) -> bool {
    let elapsed = |since: i64| u64::try_from(now_ms.saturating_sub(since)).ok();
    match last_attempt_ms {
        None => elapsed(started_ms).is_none_or(|ms| ms >= STARTUP_DELAY.as_millis() as u64),
        Some(at) => {
            let wait = if last_failed {
                RETRY_AFTER_FAILURE
            } else {
                CHECK_INTERVAL
            };
            elapsed(at).is_none_or(|ms| ms >= wait.as_millis() as u64)
        }
    }
}

/// Where the updater is. `Idle`: set up, nothing checked yet. `Available`: a newer release was
/// found but isn't downloaded (the portable exe, or automatic downloads are off). `Ready`:
/// downloaded and its signature checked; one click applies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    NotConfigured,
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Ready,
    Applying,
    Failed,
}

/// Which step failed, so the window can say "Couldn't check" or "Couldn't update".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    Check,
    Download,
    Apply,
}

/// What a check's answer leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfterCheck {
    pub phase: Phase,
    /// Start downloading now (the phase is `Downloading`).
    pub download: bool,
}

/// A check found `found` (a newer release, or none): download it quietly when this copy can
/// install and automatic downloads are on; otherwise just say it's there.
pub fn after_check(found: bool, auto: bool, portable: bool) -> AfterCheck {
    match (found, portable, auto) {
        (false, _, _) => AfterCheck {
            phase: Phase::UpToDate,
            download: false,
        },
        (true, true, _) | (true, false, false) => AfterCheck {
            phase: Phase::Available,
            download: false,
        },
        (true, false, true) => AfterCheck {
            phase: Phase::Downloading,
            download: true,
        },
    }
}

/// Whether the background schedule should run a check now. Never while one is already running,
/// a download is going or an update waits to be applied (it would only fetch the same thing), and
/// only with automatic updates on.
pub fn background_check(phase: Phase, auto: bool, due: bool) -> bool {
    auto && due && matches!(phase, Phase::Idle | Phase::UpToDate | Phase::Available | Phase::Failed)
}

/// Whether Check for updates can start a check now (it says what's already happening otherwise).
pub fn manual_check(phase: Phase) -> bool {
    matches!(phase, Phase::Idle | Phase::UpToDate | Phase::Available | Phase::Failed)
}

/// What's going on that an update restart would cut off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Busy {
    /// A call is ringing on the phone.
    pub call: bool,
    /// Tugboat is moving files.
    pub transfer: bool,
    /// Voice typing is listening or typing.
    pub voice: bool,
}

/// Why applying has to wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Blocker {
    Call,
    Transfer,
    Voice,
}

/// The first thing that would be cut off, if any (a call matters most).
pub fn blocker(busy: Busy) -> Option<Blocker> {
    if busy.call {
        Some(Blocker::Call)
    } else if busy.transfer {
        Some(Blocker::Transfer)
    } else if busy.voice {
        Some(Blocker::Voice)
    } else {
        None
    }
}

/// Why Restart now didn't restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing downloaded and checked to apply.
    NotReady,
    /// The portable exe can't run an installer.
    Portable,
    /// The downloaded release isn't newer than this one.
    Downgrade,
    Busy(Blocker),
}

/// Whether Restart now may apply `version` over `current` right now.
pub fn can_apply(
    phase: Phase,
    portable: bool,
    current: &str,
    version: Option<&str>,
    busy: Busy,
) -> Result<(), Refusal> {
    if portable {
        return Err(Refusal::Portable);
    }
    let Some(version) = version.filter(|_| phase == Phase::Ready) else {
        return Err(Refusal::NotReady);
    };
    if !is_newer(current, version) {
        return Err(Refusal::Downgrade);
    }
    match blocker(busy) {
        Some(b) => Err(Refusal::Busy(b)),
        None => Ok(()),
    }
}

/// Whether quitting from the tray should apply the downloaded update (and not restart). Never when
/// Windows is signing out or shutting down: an installer then would be cut off half way.
pub fn apply_on_quit(phase: Phase, portable: bool, session_ending: bool) -> bool {
    phase == Phase::Ready && !portable && !session_ending
}

/// A short line from the release notes for the confirm: their first paragraph, without Markdown
/// markers, at most 160 characters.
pub fn headline(notes: Option<&str>) -> Option<String> {
    const MAX: usize = 160;
    let notes = notes?;
    let paragraph: Vec<&str> = notes
        .lines()
        .map(str::trim)
        .skip_while(|l| l.is_empty() || l.starts_with('#'))
        .take_while(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let text = paragraph
        .join(" ")
        .replace("**", "")
        .trim_start_matches(['-', '*', ' '])
        .trim()
        .to_string();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= MAX {
        return Some(text);
    }
    let cut: String = text.chars().take(MAX - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// Download progress in whole percent, when the size is known.
pub fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|t| *t > 0)?;
    Some((done.min(total) * 100 / total) as u8)
}

/// The screen to come back to after the restart, as the window names it ("messages",
/// "settings:about"): lowercase words, at most one ':'. The window checks it against its own
/// screens again when it reads it back.
pub fn valid_resume(screen: &str) -> bool {
    let mut parts = screen.split(':');
    let word = |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()));
    screen.len() <= 40
        && word(parts.next())
        && match parts.next() {
            None => true,
            some => word(some) && parts.next().is_none(),
        }
}

/// Where an update may be downloaded from: tug's own releases on GitHub. latest.json names the
/// installer's address; anything else (another repository, another host, plain http) is refused
/// before a byte is fetched, whatever the manifest says.
pub const DOWNLOAD_PREFIX: &str = "https://github.com/Davenee12/tug/releases/download/";

pub fn trusted_download(url: &str) -> bool {
    url.strip_prefix(DOWNLOAD_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && !rest.contains("..")
            && !rest.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    })
}

/// What to do when starting the installer failed. Before tug tidied up for it, nothing changed:
/// stay and say so. After (`on_before_exit` already took the tray icon away, hid the windows and
/// latched the one-time tidy-up), tug would be left running with no window and no tray icon, so
/// start it again instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterFailedInstall {
    Stay,
    Restart,
}

pub fn after_failed_install(tidied_for_exit: bool) -> AfterFailedInstall {
    if tidied_for_exit {
        AfterFailedInstall::Restart
    } else {
        AfterFailedInstall::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A public key in the shape `tauri signer generate` writes, made up for the test (it isn't
    /// anyone's key, and no private key exists for it).
    fn fake_pubkey(key_bytes: &[u8]) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        let text = format!(
            "untrusted comment: minisign public key: 0123456789ABCDEF\n{}\n",
            b64.encode(key_bytes)
        );
        b64.encode(text)
    }

    fn key_line(len: usize) -> Vec<u8> {
        let mut k = b"Ed".to_vec();
        k.resize(len, 7);
        k
    }

    #[test]
    fn only_a_real_minisign_public_key_turns_updates_on() {
        assert!(pubkey_configured(&fake_pubkey(&key_line(42))));
        assert!(pubkey_configured(&format!("  {}\n", fake_pubkey(&key_line(42)))));
        assert!(!pubkey_configured(""));
        assert!(!pubkey_configured("   "));
        assert!(!pubkey_configured("REPLACE_WITH_YOUR_PUBLIC_KEY"));
        assert!(!pubkey_configured(&fake_pubkey(&key_line(41))), "wrong length");
        let mut wrong_alg = key_line(42);
        wrong_alg[0] = b'X';
        assert!(!pubkey_configured(&fake_pubkey(&wrong_alg)), "not Ed25519");
        let b64 = base64::engine::general_purpose::STANDARD;
        assert!(!pubkey_configured(&b64.encode("just one line")));
        assert!(!pubkey_configured(&b64.encode("hello\nRWQ=\n")), "no untrusted comment");
    }

    #[test]
    fn without_a_key_or_in_a_debug_build_the_updater_does_nothing() {
        let key = fake_pubkey(&key_line(42));
        assert!(updates_enabled(false, &key));
        assert!(!updates_enabled(true, &key), "debug build");
        assert!(!updates_enabled(false, ""), "no key: never checks");
        assert!(!updates_enabled(true, ""));
        // And with nothing set up, nothing on the schedule or behind the button may start a check.
        assert!(!background_check(Phase::NotConfigured, true, true));
        assert!(!manual_check(Phase::NotConfigured));
    }

    #[test]
    fn newer_is_strictly_newer_and_downgrades_are_refused() {
        assert!(is_newer("0.5.18", "0.5.19"));
        assert!(is_newer("0.5.18", "0.6.0"));
        assert!(is_newer("0.5.18", "1.0.0"));
        assert!(is_newer("0.5.9", "0.5.10"), "numeric, not text, order");
        assert!(!is_newer("0.5.18", "0.5.18"), "same version");
        assert!(!is_newer("0.5.18", "0.5.17"), "downgrade");
        assert!(!is_newer("0.5.18", "0.4.99"), "downgrade");
        assert!(!is_newer("0.5.18", "v0.5.19"), "unreadable");
        assert!(!is_newer("0.5.18", ""), "unreadable");
        assert!(!is_newer("garbage", "0.5.19"), "unreadable current");
        assert!(is_newer("0.5.19-rc.1", "0.5.19"));
        assert!(
            !is_newer("0.5.19", "0.5.19-rc.1"),
            "a pre-release of the same version is older"
        );
    }

    #[test]
    fn the_first_check_waits_for_startup_then_every_six_hours() {
        let start = 1_000_000;
        let s = |d: Duration| d.as_millis() as i64;
        assert!(!check_due(start, start, None, false));
        assert!(!check_due(start + s(STARTUP_DELAY) - 1, start, None, false));
        assert!(check_due(start + s(STARTUP_DELAY), start, None, false));
        let last = start + s(STARTUP_DELAY);
        assert!(!check_due(last + s(CHECK_INTERVAL) - 1, start, Some(last), false));
        assert!(check_due(last + s(CHECK_INTERVAL), start, Some(last), false));
        assert_eq!(CHECK_INTERVAL, Duration::from_secs(6 * 3600));
    }

    #[test]
    fn a_failure_retries_sooner_than_the_next_regular_check() {
        let last = 5_000_000;
        let s = |d: Duration| d.as_millis() as i64;
        assert!(RETRY_AFTER_FAILURE < CHECK_INTERVAL);
        assert!(!check_due(last + s(RETRY_AFTER_FAILURE) - 1, 0, Some(last), true));
        assert!(check_due(last + s(RETRY_AFTER_FAILURE), 0, Some(last), true));
        assert!(!check_due(last + s(RETRY_AFTER_FAILURE), 0, Some(last), false));
    }

    #[test]
    fn a_clock_that_went_backwards_counts_as_due() {
        assert!(check_due(1_000, 0, Some(9_000_000), false));
        assert!(check_due(1_000, 9_000_000, None, false));
    }

    #[test]
    fn a_found_release_downloads_only_when_this_copy_can_install_and_auto_is_on() {
        assert_eq!(
            after_check(false, true, false),
            AfterCheck {
                phase: Phase::UpToDate,
                download: false
            }
        );
        assert_eq!(after_check(false, true, true).phase, Phase::UpToDate);
        assert_eq!(
            after_check(true, true, false),
            AfterCheck {
                phase: Phase::Downloading,
                download: true
            }
        );
        assert_eq!(
            after_check(true, false, false),
            AfterCheck {
                phase: Phase::Available,
                download: false
            }
        );
        // The portable exe never downloads: it can't install.
        assert_eq!(
            after_check(true, true, true),
            AfterCheck {
                phase: Phase::Available,
                download: false
            }
        );
        assert_eq!(
            after_check(true, false, true),
            AfterCheck {
                phase: Phase::Available,
                download: false
            }
        );
    }

    #[test]
    fn the_schedule_never_runs_unconfigured_off_or_over_work_in_progress() {
        for phase in [Phase::Idle, Phase::UpToDate, Phase::Available, Phase::Failed] {
            assert!(background_check(phase, true, true), "{phase:?}");
            assert!(!background_check(phase, false, true), "auto off: {phase:?}");
            assert!(!background_check(phase, true, false), "not due: {phase:?}");
        }
        for phase in [
            Phase::NotConfigured,
            Phase::Checking,
            Phase::Downloading,
            Phase::Ready,
            Phase::Applying,
        ] {
            assert!(!background_check(phase, true, true), "{phase:?}");
            assert!(!manual_check(phase), "{phase:?}");
        }
        assert!(manual_check(Phase::Failed));
        assert!(manual_check(Phase::UpToDate));
    }

    #[test]
    fn a_call_a_transfer_or_voice_typing_holds_the_update() {
        let none = Busy::default();
        assert_eq!(blocker(none), None);
        assert_eq!(blocker(Busy { call: true, ..none }), Some(Blocker::Call));
        assert_eq!(blocker(Busy { transfer: true, ..none }), Some(Blocker::Transfer));
        assert_eq!(blocker(Busy { voice: true, ..none }), Some(Blocker::Voice));
        assert_eq!(
            blocker(Busy {
                call: true,
                transfer: true,
                voice: true
            }),
            Some(Blocker::Call)
        );
        assert_eq!(
            blocker(Busy {
                transfer: true,
                voice: true,
                ..none
            }),
            Some(Blocker::Transfer)
        );
    }

    #[test]
    fn restart_applies_only_a_ready_newer_update_with_nothing_going_on() {
        let idle = Busy::default();
        assert_eq!(can_apply(Phase::Ready, false, "0.5.18", Some("0.5.19"), idle), Ok(()));
        assert_eq!(
            can_apply(
                Phase::Ready,
                false,
                "0.5.18",
                Some("0.5.19"),
                Busy { call: true, ..idle }
            ),
            Err(Refusal::Busy(Blocker::Call))
        );
        assert_eq!(
            can_apply(
                Phase::Ready,
                false,
                "0.5.18",
                Some("0.5.19"),
                Busy { voice: true, ..idle }
            ),
            Err(Refusal::Busy(Blocker::Voice))
        );
        assert_eq!(
            can_apply(Phase::Ready, false, "0.5.18", Some("0.5.18"), idle),
            Err(Refusal::Downgrade)
        );
        assert_eq!(
            can_apply(Phase::Ready, false, "0.5.18", Some("0.5.10"), idle),
            Err(Refusal::Downgrade)
        );
        assert_eq!(
            can_apply(Phase::Downloading, false, "0.5.18", Some("0.5.19"), idle),
            Err(Refusal::NotReady)
        );
        assert_eq!(
            can_apply(Phase::Available, false, "0.5.18", Some("0.5.19"), idle),
            Err(Refusal::NotReady)
        );
        assert_eq!(
            can_apply(Phase::Ready, false, "0.5.18", None, idle),
            Err(Refusal::NotReady)
        );
        assert_eq!(
            can_apply(Phase::Ready, true, "0.5.18", Some("0.5.19"), idle),
            Err(Refusal::Portable)
        );
    }

    #[test]
    fn quitting_applies_a_ready_update_but_never_while_windows_shuts_down() {
        assert!(apply_on_quit(Phase::Ready, false, false));
        assert!(!apply_on_quit(Phase::Ready, false, true));
        assert!(!apply_on_quit(Phase::Ready, true, false), "portable");
        assert!(!apply_on_quit(Phase::Downloading, false, false));
        assert!(!apply_on_quit(Phase::Available, false, false));
    }

    #[test]
    fn the_headline_is_the_notes_first_paragraph() {
        let notes =
            "Texts that send right away, a shared clipboard\nand smart replies.\n\n### New\n- **Smart replies:** …";
        assert_eq!(
            headline(Some(notes)).as_deref(),
            Some("Texts that send right away, a shared clipboard and smart replies.")
        );
        assert_eq!(
            headline(Some("## v0.5.19\n\n- **Faster** texts\n")).as_deref(),
            Some("Faster texts")
        );
        assert_eq!(headline(Some("   \n")), None);
        assert_eq!(headline(None), None);
        let long = "word ".repeat(80);
        let h = headline(Some(&long)).unwrap();
        assert_eq!(h.chars().count(), 160);
        assert!(h.ends_with('…'));
    }

    #[test]
    fn progress_is_a_percentage_when_the_size_is_known() {
        assert_eq!(percent(0, Some(200)), Some(0));
        assert_eq!(percent(100, Some(200)), Some(50));
        assert_eq!(percent(250, Some(200)), Some(100));
        assert_eq!(percent(10, None), None);
        assert_eq!(percent(10, Some(0)), None);
    }

    #[test]
    fn only_a_screen_name_is_kept_for_after_the_restart() {
        for ok in ["feed", "messages", "calls", "settings:about", "settings:iphone"] {
            assert!(valid_resume(ok), "{ok}");
        }
        for bad in [
            "",
            ":",
            "settings:",
            ":about",
            "a:b:c",
            "Settings",
            "feed/../x",
            "x y",
            &"a".repeat(41),
        ] {
            assert!(!valid_resume(bad), "{bad}");
        }
    }

    #[test]
    fn release_pages_carry_only_a_real_version() {
        assert_eq!(
            release_page("0.5.19").as_deref(),
            Some("https://github.com/Davenee12/tug/releases/tag/v0.5.19")
        );
        assert_eq!(release_page("0.5.19/../../evil"), None);
        assert_eq!(release_page(""), None);
        assert!(ENDPOINT.starts_with("https://github.com/Davenee12/tug/"));
    }

    #[test]
    fn an_installed_copy_has_the_uninstaller_beside_it() {
        assert!(!is_portable(true));
        assert!(is_portable(false));
    }

    #[test]
    fn a_failed_install_after_tidying_up_starts_tug_again() {
        assert_eq!(after_failed_install(false), AfterFailedInstall::Stay);
        assert_eq!(after_failed_install(true), AfterFailedInstall::Restart);
    }

    #[test]
    fn updates_download_only_from_tugs_own_releases() {
        assert!(trusted_download(
            "https://github.com/Davenee12/tug/releases/download/v0.5.19/tug-0.5.19-setup.exe"
        ));
        for bad in [
            "https://github.com/someone-else/tug/releases/download/v0.5.19/tug-0.5.19-setup.exe",
            "https://github.com/Davenee12/tug-fork/releases/download/v1/x.exe",
            "http://github.com/Davenee12/tug/releases/download/v0.5.19/tug-0.5.19-setup.exe",
            "https://github.com.evil.example/Davenee12/tug/releases/download/v1/x.exe",
            "https://objects.githubusercontent.com/x.exe",
            "https://github.com/Davenee12/tug/releases/download/../../../evil/x.exe",
            "https://github.com/Davenee12/tug/releases/download/",
            "https://github.com/Davenee12/tug/releases/download/v1/x y.exe",
        ] {
            assert!(!trusted_download(bad), "{bad}");
        }
        assert!(ENDPOINT.starts_with("https://github.com/Davenee12/tug/releases/"));
    }
}
