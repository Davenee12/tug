//! The iPhone's music in Windows' own media controls: the keyboard's play/pause, next and
//! previous keys, and the media flyout (volume/media keys, Quick Settings, lock screen).
//!
//! Windows calls these the System Media Transport Controls (SMTC). [`desired`] maps the AMS
//! Now Playing state to what SMTC should show, and [`command_for`] maps a pressed SMTC button
//! to the AMS command to send. Both are pure and unit-tested; the WinRT calls in `smtc` stay
//! thin.
//!
//! tug only claims an SMTC session while the iPhone reports a player with a track, and
//! closes it otherwise, so a stale iPhone tile never lingers and Windows' real media apps
//! keep the keys whenever the phone has nothing to offer.

use std::sync::Arc;

use crate::ams::{NowPlaying, PlaybackState, RemoteCommand};
use crate::ble::BleHandle;
use crate::state::Shared;

/// What the SMTC session should report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// No session: the iPhone has nothing to show, so Windows hides tug's tile.
    Closed,
    /// A player with a track but no playback state yet.
    Stopped,
    Paused,
    Playing,
}

/// An SMTC button tug handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Play,
    Pause,
    Next,
    Previous,
}

/// Track position for the flyout's progress bar, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeline {
    pub duration_ms: i64,
    pub position_ms: i64,
}

/// Everything tug sets on SMTC, derived from one Now Playing snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SmtcState {
    /// Whether tug holds an SMTC session at all (SMTC `IsEnabled`).
    pub enabled: bool,
    pub status: Status,
    pub play: bool,
    pub pause: bool,
    pub next: bool,
    pub previous: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Playback rate, so Windows can advance the position between updates.
    pub rate: f64,
    pub timeline: Option<Timeline>,
}

impl SmtcState {
    /// Nothing playing on the phone (or it's disconnected): no session.
    pub fn closed() -> Self {
        Self {
            enabled: false,
            status: Status::Closed,
            play: false,
            pause: false,
            next: false,
            previous: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            rate: 1.0,
            timeline: None,
        }
    }
}

/// Whether the iPhone has something for Windows to show and control. AMS names the player
/// whenever one is active, and a title means there's a track (what the Now Playing card
/// itself waits for). A disconnect resets Now Playing to its default, which closes the session.
fn active(np: &NowPlaying) -> bool {
    np.player.is_some() && np.title.is_some()
}

/// As the Now Playing card's buttons: before the phone has sent its command list, assume
/// the usual ones work rather than show a dead tile.
fn can(np: &NowPlaying, command: RemoteCommand) -> bool {
    np.available.is_empty() || np.lists(command)
}

/// The AMS command for a pressed SMTC button, or `None` if the player can't do it.
///
/// Windows turns the keyboard's play/pause key into Play or Pause from the status tug last
/// reported. An explicit Play/Pause is sent where the player accepts it, so a key press can
/// never flip playback the wrong way if Windows' idea of the state is a moment stale; the
/// toggle is the fallback for players that only list it.
pub fn command_for(button: Button, np: &NowPlaying) -> Option<RemoteCommand> {
    if !active(np) {
        return None;
    }
    let pick = |explicit: RemoteCommand| {
        if can(np, explicit) {
            Some(explicit)
        } else if np.lists(RemoteCommand::TogglePlayPause) {
            Some(RemoteCommand::TogglePlayPause)
        } else {
            None
        }
    };
    match button {
        Button::Play => pick(RemoteCommand::Play),
        Button::Pause => pick(RemoteCommand::Pause),
        Button::Next => can(np, RemoteCommand::NextTrack).then_some(RemoteCommand::NextTrack),
        Button::Previous => can(np, RemoteCommand::PreviousTrack).then_some(RemoteCommand::PreviousTrack),
    }
}

/// What SMTC should show for this Now Playing snapshot, `now_ms` being the current Unix time.
pub fn desired(np: &NowPlaying, now_ms: i64) -> SmtcState {
    if !active(np) {
        return SmtcState::closed();
    }
    let playing = matches!(
        np.state,
        PlaybackState::Playing | PlaybackState::Rewinding | PlaybackState::FastForwarding
    );
    let status = match np.state {
        PlaybackState::Unknown => Status::Stopped,
        PlaybackState::Paused => Status::Paused,
        _ => Status::Playing,
    };
    // AMS reports 0 while paused; Windows only needs the rate to advance a playing track.
    let rate = np.rate.filter(|r| playing && r.is_finite() && *r != 0.0).unwrap_or(1.0);
    let shown = shown_track(np.title.as_deref().unwrap_or(""), np.artist.as_deref().unwrap_or(""));
    SmtcState {
        enabled: true,
        status,
        play: command_for(Button::Play, np).is_some(),
        pause: command_for(Button::Pause, np).is_some(),
        next: command_for(Button::Next, np).is_some(),
        previous: command_for(Button::Previous, np).is_some(),
        title: shown.0,
        artist: shown.1,
        album: np.album.clone().unwrap_or_default(),
        rate,
        timeline: timeline(np, playing, rate, now_ms),
    }
}

/// Title and artist for the flyout. On Spotify Connect the iPhone sends the title as
/// "Song • Artist" and the artist as "Listening on <device>"; the flyout shows the song and the
/// artist, never the device hint as an artist (as the Now Playing card, `src/lib/playback.ts`).
fn shown_track(title: &str, artist: &str) -> (String, String) {
    let hint = artist.trim().to_lowercase();
    if hint.starts_with("listening on ") && hint.len() > "listening on ".len() {
        return match title.rsplit_once(" • ") {
            Some((song, by)) if !song.trim().is_empty() => (song.trim().to_string(), by.trim().to_string()),
            _ => (title.to_string(), String::new()),
        };
    }
    (title.to_string(), artist.to_string())
}

/// The position now: the phone's last report, advanced while playing (as the card does).
fn timeline(np: &NowPlaying, playing: bool, rate: f64, now_ms: i64) -> Option<Timeline> {
    let duration = np.duration.filter(|d| d.is_finite() && *d > 0.0)?;
    let elapsed = np.elapsed.filter(|e| e.is_finite())?;
    let drift = match (playing, np.elapsed_at) {
        (true, Some(at)) => (now_ms - at).max(0) as f64 / 1000.0 * rate,
        _ => 0.0,
    };
    let position = (elapsed + drift).clamp(0.0, duration);
    Some(Timeline {
        duration_ms: (duration * 1000.0).round() as i64,
        position_ms: (position * 1000.0).round() as i64,
    })
}

/// Register with SMTC and keep it in step with Now Playing. Never a reason not to start:
/// if Windows refuses, media keys are logged as unavailable and everything else carries on.
pub fn start(shared: Arc<Shared>, ble: BleHandle) {
    #[cfg(windows)]
    smtc::start(shared, ble);
    #[cfg(not(windows))]
    let _ = (shared, ble);
}

#[cfg(windows)]
mod smtc {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use windows::core::{Result, HSTRING};
    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::Playback::MediaPlayer;
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs, SystemMediaTransportControlsTimelineProperties,
    };

    use super::{command_for, desired, Button, SmtcState, Status};
    use crate::ble::{BleHandle, Command};
    use crate::state::Shared;

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Own the SMTC session on its own thread, so a slow call into Windows' media service
    /// never holds up the Bluetooth actor. Changes are coalesced: after a burst of AMS
    /// updates (a new track sends several), only the latest state is applied.
    pub fn start(shared: Arc<Shared>, ble: BleHandle) {
        let (tx, rx) = mpsc::channel::<()>();
        shared.set_now_playing_hook(Box::new(move || {
            let _ = tx.send(());
        }));
        let spawned = std::thread::Builder::new()
            .name("tug-media-keys".into())
            .spawn(move || {
                let session = match Session::new(shared.clone(), ble) {
                    Ok(s) => s,
                    Err(e) => return log::warn!("Windows media keys unavailable: {e}"),
                };
                log::info!("Windows media keys ready");
                let mut last = None;
                session.sync(&shared, &mut last);
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    session.sync(&shared, &mut last);
                }
            });
        if let Err(e) = spawned {
            log::warn!("Windows media keys unavailable: {e}");
        }
    }

    /// A desktop (non-UWP) app has no CoreWindow for `GetForCurrentView`, so tug borrows the
    /// SMTC of a `MediaPlayer` it never plays anything through, with the player's automatic
    /// command handling off so tug alone decides what the session shows.
    struct Session {
        _player: MediaPlayer,
        smtc: SystemMediaTransportControls,
    }

    impl Session {
        fn new(shared: Arc<Shared>, ble: BleHandle) -> Result<Self> {
            let player = MediaPlayer::new()?;
            player.CommandManager()?.SetIsEnabled(false)?;
            let smtc = player.SystemMediaTransportControls()?;
            apply(&smtc, &SmtcState::closed())?;
            smtc.ButtonPressed(&TypedEventHandler::<
                SystemMediaTransportControls,
                SystemMediaTransportControlsButtonPressedEventArgs,
            >::new(move |_, args| {
                let Some(args) = args.as_ref() else { return Ok(()) };
                let button = match args.Button()? {
                    SystemMediaTransportControlsButton::Play => Button::Play,
                    SystemMediaTransportControlsButton::Pause => Button::Pause,
                    SystemMediaTransportControlsButton::Next => Button::Next,
                    SystemMediaTransportControlsButton::Previous => Button::Previous,
                    other => {
                        log::debug!("media key {} ignored", other.0);
                        return Ok(());
                    }
                };
                match command_for(button, &shared.now_playing()) {
                    Some(command) => {
                        log::info!("media key {button:?}: sending {}", command.as_str());
                        // The actor logs the outcome of every AMS command; nothing waits here.
                        let (reply, _) = tokio::sync::oneshot::channel();
                        ble.send(Command::Media {
                            command,
                            requested_at: std::time::Instant::now(),
                            report_repeat: false,
                            reply,
                        });
                    }
                    None => log::info!("media key {button:?} ignored: the iPhone's player doesn't offer it"),
                }
                Ok(())
            }))?;
            Ok(Self { _player: player, smtc })
        }

        /// Bring SMTC in line with the current Now Playing, skipping a no-op update.
        fn sync(&self, shared: &Shared, last: &mut Option<SmtcState>) {
            let want = desired(&shared.now_playing(), now_ms());
            if last.as_ref() == Some(&want) {
                return;
            }
            match apply(&self.smtc, &want) {
                Ok(()) => {
                    if last.as_ref().map(|l| l.enabled) != Some(want.enabled) {
                        log::info!(
                            "Windows media controls {}",
                            if want.enabled { "showing the iPhone" } else { "closed" }
                        );
                    }
                    *last = Some(want);
                }
                Err(e) => {
                    log::warn!("Windows media controls update failed: {e}");
                    *last = None;
                }
            }
        }
    }

    fn span(ms: i64) -> TimeSpan {
        // TimeSpan counts 100 ns ticks.
        TimeSpan {
            Duration: ms.saturating_mul(10_000),
        }
    }

    fn apply(smtc: &SystemMediaTransportControls, s: &SmtcState) -> Result<()> {
        let display = smtc.DisplayUpdater()?;
        if !s.enabled {
            display.ClearAll()?;
            display.Update()?;
            smtc.UpdateTimelineProperties(&SystemMediaTransportControlsTimelineProperties::new()?)?;
            smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
            smtc.SetIsPlayEnabled(false)?;
            smtc.SetIsPauseEnabled(false)?;
            smtc.SetIsNextEnabled(false)?;
            smtc.SetIsPreviousEnabled(false)?;
            return smtc.SetIsEnabled(false);
        }
        smtc.SetIsEnabled(true)?;
        smtc.SetIsPlayEnabled(s.play)?;
        smtc.SetIsPauseEnabled(s.pause)?;
        smtc.SetIsNextEnabled(s.next)?;
        smtc.SetIsPreviousEnabled(s.previous)?;
        smtc.SetIsStopEnabled(false)?;
        smtc.SetPlaybackStatus(match s.status {
            Status::Closed => MediaPlaybackStatus::Closed,
            Status::Stopped => MediaPlaybackStatus::Stopped,
            Status::Paused => MediaPlaybackStatus::Paused,
            Status::Playing => MediaPlaybackStatus::Playing,
        })?;
        smtc.SetPlaybackRate(s.rate)?;
        display.SetType(MediaPlaybackType::Music)?;
        let music = display.MusicProperties()?;
        music.SetTitle(&HSTRING::from(s.title.as_str()))?;
        music.SetArtist(&HSTRING::from(s.artist.as_str()))?;
        music.SetAlbumTitle(&HSTRING::from(s.album.as_str()))?;
        display.Update()?;
        // No seek: AMS has no command for it, so the seek range stays empty.
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        if let Some(t) = s.timeline {
            timeline.SetStartTime(span(0))?;
            timeline.SetEndTime(span(t.duration_ms))?;
            timeline.SetPosition(span(t.position_ms))?;
            timeline.SetMinSeekTime(span(t.position_ms))?;
            timeline.SetMaxSeekTime(span(t.position_ms))?;
        }
        smtc.UpdateTimelineProperties(&timeline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spotify-like: a player, a track, the usual five commands.
    fn spotify() -> NowPlaying {
        let mut np = NowPlaying {
            player: Some("Spotify".into()),
            state: PlaybackState::Playing,
            rate: Some(1.0),
            elapsed: Some(42.0),
            elapsed_at: Some(1_000_000),
            title: Some("Teardrop".into()),
            artist: Some("Massive Attack".into()),
            album: Some("Mezzanine".into()),
            duration: Some(330.0),
            ..Default::default()
        };
        np.apply_available_commands(&[0, 1, 2, 3, 4, 5, 6]);
        np
    }

    #[test]
    fn nothing_playing_closes_the_session() {
        assert_eq!(desired(&NowPlaying::default(), 0), SmtcState::closed());
        // A player with no track (an app open on nothing) doesn't claim the keys either.
        let np = NowPlaying {
            player: Some("Music".into()),
            ..Default::default()
        };
        assert!(!desired(&np, 0).enabled);
        // A disconnect resets Now Playing to the default, which closes it again.
        let mut np = spotify();
        assert!(desired(&np, 0).enabled);
        np = NowPlaying::default();
        assert_eq!(desired(&np, 0).status, Status::Closed);
    }

    #[test]
    fn closed_session_offers_no_buttons() {
        let s = SmtcState::closed();
        assert!(!s.enabled && !s.play && !s.pause && !s.next && !s.previous);
        assert_eq!(s.timeline, None);
        assert_eq!(command_for(Button::Play, &NowPlaying::default()), None);
        assert_eq!(command_for(Button::Next, &NowPlaying::default()), None);
    }

    #[test]
    fn shows_the_track_and_state() {
        let s = desired(&spotify(), 1_000_000);
        assert!(s.enabled);
        assert_eq!(s.status, Status::Playing);
        assert_eq!(
            (s.title.as_str(), s.artist.as_str(), s.album.as_str()),
            ("Teardrop", "Massive Attack", "Mezzanine")
        );
        assert!(s.play && s.pause && s.next && s.previous);
        assert_eq!(s.rate, 1.0);

        let mut np = spotify();
        np.state = PlaybackState::Paused;
        np.rate = Some(0.0);
        let s = desired(&np, 1_000_000);
        assert_eq!(s.status, Status::Paused);
        assert_eq!(s.rate, 1.0, "AMS's paused rate of 0 isn't passed on");

        np.state = PlaybackState::Unknown;
        assert_eq!(desired(&np, 0).status, Status::Stopped);
        np.state = PlaybackState::FastForwarding;
        np.rate = Some(2.0);
        let s = desired(&np, 0);
        assert_eq!((s.status, s.rate), (Status::Playing, 2.0));
    }

    #[test]
    fn spotify_connect_hint_is_never_the_artist() {
        let mut np = spotify();
        np.title = Some("Sweet Music • Voice, Trini Baby".into());
        np.artist = Some("Listening on Kitchen Echo Dot".into());
        let s = desired(&np, 0);
        assert_eq!(
            (s.title.as_str(), s.artist.as_str()),
            ("Sweet Music", "Voice, Trini Baby")
        );
        // No bullet in the title: the song stays whole and the artist is blank.
        np.title = Some("Sweet Music".into());
        let s = desired(&np, 0);
        assert_eq!((s.title.as_str(), s.artist.as_str()), ("Sweet Music", ""));
        // A normal track with a bullet in its title is left alone.
        let mut np = spotify();
        np.title = Some("A • B".into());
        assert_eq!(desired(&np, 0).title, "A • B");
    }

    #[test]
    fn missing_artist_and_album_are_blank() {
        let mut np = spotify();
        np.artist = None;
        np.album = None;
        let s = desired(&np, 0);
        assert_eq!((s.artist.as_str(), s.album.as_str()), ("", ""));
    }

    #[test]
    fn buttons_follow_the_players_command_list() {
        // Only the toggle: Play and Pause both map to it.
        let mut np = spotify();
        np.apply_available_commands(&[2]);
        let s = desired(&np, 0);
        assert!(s.play && s.pause);
        assert!(!s.next && !s.previous);
        assert_eq!(command_for(Button::Play, &np), Some(RemoteCommand::TogglePlayPause));
        assert_eq!(command_for(Button::Pause, &np), Some(RemoteCommand::TogglePlayPause));
        assert_eq!(command_for(Button::Next, &np), None);

        // Next and previous only.
        np.apply_available_commands(&[3, 4]);
        let s = desired(&np, 0);
        assert!(!s.play && !s.pause && s.next && s.previous);
        assert_eq!(command_for(Button::Previous, &np), Some(RemoteCommand::PreviousTrack));
        assert_eq!(command_for(Button::Pause, &np), None);
    }

    #[test]
    fn explicit_play_and_pause_win_over_the_toggle() {
        let np = spotify();
        assert_eq!(command_for(Button::Play, &np), Some(RemoteCommand::Play));
        assert_eq!(command_for(Button::Pause, &np), Some(RemoteCommand::Pause));
        assert_eq!(command_for(Button::Next, &np), Some(RemoteCommand::NextTrack));
        assert_eq!(command_for(Button::Previous, &np), Some(RemoteCommand::PreviousTrack));
    }

    #[test]
    fn before_the_command_list_arrives_every_button_works() {
        let mut np = spotify();
        np.available.clear();
        let s = desired(&np, 0);
        assert!(s.play && s.pause && s.next && s.previous);
        assert_eq!(command_for(Button::Play, &np), Some(RemoteCommand::Play));
    }

    #[test]
    fn button_states_always_match_what_a_press_would_do() {
        for commands in [&[][..], &[0], &[1], &[2], &[3], &[4], &[0, 1, 2, 3, 4], &[5, 6, 7]] {
            let mut np = spotify();
            np.apply_available_commands(commands);
            let s = desired(&np, 0);
            assert_eq!(s.play, command_for(Button::Play, &np).is_some(), "{commands:?}");
            assert_eq!(s.pause, command_for(Button::Pause, &np).is_some(), "{commands:?}");
            assert_eq!(s.next, command_for(Button::Next, &np).is_some(), "{commands:?}");
            assert_eq!(s.previous, command_for(Button::Previous, &np).is_some(), "{commands:?}");
        }
    }

    #[test]
    fn timeline_advances_while_playing_and_holds_while_paused() {
        let np = spotify(); // 42 s reported at t = 1 000 000 ms
        let t = desired(&np, 1_010_000).timeline.unwrap();
        assert_eq!(
            t,
            Timeline {
                duration_ms: 330_000,
                position_ms: 52_000
            }
        );

        let mut paused = np.clone();
        paused.state = PlaybackState::Paused;
        assert_eq!(desired(&paused, 1_010_000).timeline.unwrap().position_ms, 42_000);

        // Never past the end, never before the start (a clock that went backwards).
        assert_eq!(desired(&np, 9_000_000).timeline.unwrap().position_ms, 330_000);
        assert_eq!(desired(&np, 0).timeline.unwrap().position_ms, 42_000);
    }

    #[test]
    fn timeline_needs_a_duration_and_a_position() {
        let mut np = spotify();
        np.duration = None;
        assert_eq!(desired(&np, 0).timeline, None);
        np.duration = Some(0.0);
        assert_eq!(desired(&np, 0).timeline, None);
        np = spotify();
        np.elapsed = None;
        assert_eq!(desired(&np, 0).timeline, None);
    }
}
