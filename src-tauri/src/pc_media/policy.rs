//! Which of Windows' media sessions tug pauses for a ringing call, and whether one it paused is
//! still as tug left it when the call is over. Pure: `mod.rs` reads the sessions and carries it out.

/// What a session is doing, as far as this cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    /// Stopped, closed, opening or changing: nothing tug paused, and nothing to play again.
    Other,
}

/// One media session as Windows lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    /// The app behind it (`Spotify.exe`, `Chrome`, an AppUserModelID).
    pub app_id: String,
    pub playback: Playback,
    pub title: String,
    pub artist: String,
    /// Where it is in the track, when the app reports a timeline.
    pub position_ms: Option<i64>,
}

/// After pausing, an app can still report itself playing for a moment while it catches up; only
/// playing after this long means someone pressed play.
pub const SETTLE_MS: i64 = 2_000;
/// How far an unwatched paused session's position may drift (apps round and report late) before it
/// counts as played and paused again.
pub const POSITION_SLACK_MS: i64 = 3_000;

/// tug's own session: the iPhone's Now Playing it shows in Windows' media controls
/// (`media_keys.rs`). Pausing it would send Pause to the phone, and iOS already pauses the phone's
/// music for the call. Windows names it by the AppUserModelID tug claims (`quick_actions::init`),
/// or by the exe when that didn't take.
fn is_tug(app_id: &str, own_app_id: Option<&str>) -> bool {
    let id = app_id.trim();
    if own_app_id.is_some_and(|own| !own.is_empty() && id.eq_ignore_ascii_case(own)) {
        return true;
    }
    let file = id.rsplit(['\\', '/']).next().unwrap_or(id);
    file.eq_ignore_ascii_case("tug.exe")
}

/// The iPhone's own audio, if Windows lists it as a session while "Play iPhone audio on this PC"
/// is on. Not seen on hardware: this assumes such a session's id names Bluetooth. Its buttons
/// would control the phone's player, which iOS pauses for the call itself, so it's left alone
/// (and so is `pc_audio`).
fn is_phone_audio(app_id: &str) -> bool {
    app_id.to_ascii_lowercase().contains("bluetooth")
}

/// Pause this one: it's playing, and it's the PC's own (not tug's, not the phone's).
pub fn should_pause(s: &SessionView, own_app_id: Option<&str>) -> bool {
    s.playback == Playback::Playing && !is_tug(&s.app_id, own_app_id) && !is_phone_audio(&s.app_id)
}

/// A session reported `playback` `since_pause_ms` after tug paused it: does that mean the person
/// played it themselves? Then it's theirs, and tug won't play it again.
pub fn played_by_person(playback: Playback, since_pause_ms: i64) -> bool {
    playback == Playback::Playing && since_pause_ms >= SETTLE_MS
}

/// Play this session again after the call: only if it's still there, still paused, on the same
/// track, and nobody played it in between. Anything else means the person changed it (or closed
/// it), and that's theirs to keep. `played_since` is what watching the session saw (`None` when
/// Windows wouldn't let tug watch it); without a watch, a position that moved on stands in for it.
pub fn should_resume(paused: &SessionView, played_since: Option<bool>, now: Option<&SessionView>) -> bool {
    let Some(now) = now else { return false };
    if played_since == Some(true) || now.playback != Playback::Paused {
        return false;
    }
    if now.app_id != paused.app_id || now.title != paused.title || now.artist != paused.artist {
        return false;
    }
    match (played_since, paused.position_ms, now.position_ms) {
        (None, Some(a), Some(b)) => (a - b).abs() <= POSITION_SLACK_MS,
        // Watched: an app catching up on where it paused isn't a change. Or no timeline to compare.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(app_id: &str, playback: Playback) -> SessionView {
        SessionView {
            app_id: app_id.into(),
            playback,
            title: "Harbour Lights".into(),
            artist: "The Example Band".into(),
            position_ms: Some(61_000),
        }
    }

    const OWN: Option<&str> = Some("dev.davejames.tug");

    #[test]
    fn pauses_what_plays_on_the_pc() {
        assert!(should_pause(&session("Spotify.exe", Playback::Playing), OWN));
        assert!(should_pause(&session("Chrome", Playback::Playing), OWN));
        assert!(should_pause(
            &session(
                "Microsoft.ZuneVideo_8wekyb3d8bbwe!Microsoft.ZuneVideo",
                Playback::Playing
            ),
            OWN
        ));
        // Already paused or stopped: nothing to do, and nothing to play again later.
        assert!(!should_pause(&session("Spotify.exe", Playback::Paused), OWN));
        assert!(!should_pause(&session("Spotify.exe", Playback::Other), OWN));
    }

    #[test]
    fn leaves_tugs_own_session_alone() {
        assert!(!should_pause(&session("dev.davejames.tug", Playback::Playing), OWN));
        assert!(!should_pause(&session("DEV.DAVEJAMES.TUG", Playback::Playing), OWN));
        // A dev build's id, or the exe when the id didn't take.
        assert!(!should_pause(
            &session("dev.davejames.tug.dev", Playback::Playing),
            Some("dev.davejames.tug.dev")
        ));
        assert!(!should_pause(&session("tug.exe", Playback::Playing), None));
        assert!(!should_pause(
            &session(r"C:\Program Files\tug\tug.exe", Playback::Playing),
            OWN
        ));
        // Only tug itself, not every name ending in it.
        assert!(should_pause(&session("mytug.exe", Playback::Playing), OWN));
        assert!(should_pause(&session("Spotify.exe", Playback::Playing), Some("")));
    }

    #[test]
    fn leaves_the_phones_own_audio_alone() {
        assert!(!should_pause(
            &session("Windows.Bluetooth.Audio", Playback::Playing),
            OWN
        ));
    }

    #[test]
    fn playing_right_after_the_pause_is_the_app_catching_up() {
        assert!(!played_by_person(Playback::Playing, 0));
        assert!(!played_by_person(Playback::Playing, SETTLE_MS - 1));
        assert!(played_by_person(Playback::Playing, SETTLE_MS));
        assert!(!played_by_person(Playback::Paused, 60_000));
    }

    #[test]
    fn plays_again_only_what_is_still_as_tug_left_it() {
        let paused = session("Spotify.exe", Playback::Paused);
        assert!(should_resume(&paused, Some(false), Some(&paused)));
        assert!(should_resume(&paused, None, Some(&paused)));
        // Unwatched, a little drift in the reported position is fine.
        let drift = SessionView {
            position_ms: Some(62_500),
            ..paused.clone()
        };
        assert!(should_resume(&paused, None, Some(&drift)));
        // Watched and never played: the app settling on where it paused isn't a change.
        let settled = SessionView {
            position_ms: Some(75_000),
            ..paused.clone()
        };
        assert!(should_resume(&paused, Some(false), Some(&settled)));
        // No timeline on one side: the other checks decide.
        let untimed = SessionView {
            position_ms: None,
            ..paused.clone()
        };
        assert!(should_resume(&paused, None, Some(&untimed)));
    }

    #[test]
    fn never_plays_what_the_person_changed() {
        let paused = session("Spotify.exe", Playback::Paused);
        // Gone (the app closed).
        assert!(!should_resume(&paused, Some(false), None));
        // Played in between, even if paused again since.
        assert!(!should_resume(&paused, Some(true), Some(&paused)));
        // Playing now, or stopped.
        let playing = session("Spotify.exe", Playback::Playing);
        assert!(!should_resume(&paused, Some(false), Some(&playing)));
        assert!(!should_resume(
            &paused,
            None,
            Some(&session("Spotify.exe", Playback::Other))
        ));
        // Another track.
        let other = SessionView {
            title: "Another Song".into(),
            ..paused.clone()
        };
        assert!(!should_resume(&paused, Some(false), Some(&other)));
        let other_artist = SessionView {
            artist: "Someone Else".into(),
            ..paused.clone()
        };
        assert!(!should_resume(&paused, None, Some(&other_artist)));
        // Unwatched and moved on in the track (played and paused again, or skipped ahead).
        let moved = SessionView {
            position_ms: Some(90_000),
            ..paused.clone()
        };
        assert!(!should_resume(&paused, None, Some(&moved)));
    }
}
