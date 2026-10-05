//! Apple Media Service (AMS) wire protocol: now-playing info and remote control.
//!
//! Spec: https://developer.apple.com/library/archive/documentation/CoreBluetooth/Reference/AppleMediaService_Reference/
//!
//! Pure encode/decode only, like `ancs`.

use serde::Serialize;

pub const SERVICE: u128 = 0x89D3502B_0F36_433A_8EF4_C502AD55F8DC;
pub const REMOTE_COMMAND: u128 = 0x9B3C81D8_57B1_4A8A_B8DF_0E56F7CA51C2;
pub const ENTITY_UPDATE: u128 = 0x2F7CABCE_808D_411F_9A0C_BB92BA96C102;
pub const ENTITY_ATTRIBUTE: u128 = 0xC6B2F38C_23AB_46D8_A6AB_A3A870BBD5D7;

const ENTITY_PLAYER: u8 = 0;
const ENTITY_TRACK: u8 = 2;

const PLAYER_NAME: u8 = 0;
const PLAYER_PLAYBACK_INFO: u8 = 1;
const PLAYER_VOLUME: u8 = 2;

const TRACK_ARTIST: u8 = 0;
const TRACK_ALBUM: u8 = 1;
const TRACK_TITLE: u8 = 2;
const TRACK_DURATION: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCommand {
    Play,
    Pause,
    TogglePlayPause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
}

impl RemoteCommand {
    pub fn id(self) -> u8 {
        match self {
            Self::Play => 0,
            Self::Pause => 1,
            Self::TogglePlayPause => 2,
            Self::NextTrack => 3,
            Self::PreviousTrack => 4,
            Self::VolumeUp => 5,
            Self::VolumeDown => 6,
        }
    }

    fn name(id: u8) -> Option<&'static str> {
        Some(match id {
            0 => "play",
            1 => "pause",
            2 => "togglePlayPause",
            3 => "nextTrack",
            4 => "previousTrack",
            5 => "volumeUp",
            6 => "volumeDown",
            _ => return None,
        })
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "play" => Self::Play,
            "pause" => Self::Pause,
            "togglePlayPause" => Self::TogglePlayPause,
            "nextTrack" => Self::NextTrack,
            "previousTrack" => Self::PreviousTrack,
            "volumeUp" => Self::VolumeUp,
            "volumeDown" => Self::VolumeDown,
            _ => return None,
        })
    }
}

/// Entity Update writes that register for player and track changes.
/// Each must be written separately.
pub fn registrations() -> [Vec<u8>; 2] {
    [
        vec![ENTITY_PLAYER, PLAYER_NAME, PLAYER_PLAYBACK_INFO, PLAYER_VOLUME],
        vec![ENTITY_TRACK, TRACK_ARTIST, TRACK_ALBUM, TRACK_TITLE, TRACK_DURATION],
    ]
}

/// Every (entity, attribute) tug shows. Entity Update only reports *changes*, so on
/// connect each one is read through Entity Attribute (write the pair, then read)
/// to pick up whatever is already playing.
pub fn current_value_queries() -> [[u8; 2]; 7] {
    [
        [ENTITY_PLAYER, PLAYER_NAME],
        [ENTITY_PLAYER, PLAYER_PLAYBACK_INFO],
        [ENTITY_PLAYER, PLAYER_VOLUME],
        [ENTITY_TRACK, TRACK_TITLE],
        [ENTITY_TRACK, TRACK_ARTIST],
        [ENTITY_TRACK, TRACK_ALBUM],
        [ENTITY_TRACK, TRACK_DURATION],
    ]
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    #[default]
    Unknown,
    Paused,
    Playing,
    Rewinding,
    FastForwarding,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub player: Option<String>,
    pub state: PlaybackState,
    pub rate: Option<f64>,
    pub elapsed: Option<f64>,
    /// Unix ms when the phone reported `elapsed`. Progress is computed from this,
    /// not from when *any* attribute last changed — otherwise a volume change
    /// would make the progress bar jump back to the last reported position.
    pub elapsed_at: Option<i64>,
    pub volume: Option<f64>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<f64>,
    /// Remote commands the current player accepts.
    pub available: Vec<&'static str>,
}

impl NowPlaying {
    /// Apply one Entity Update notification: `[entity][attr][flags][utf8 value]`.
    /// Returns true if anything changed.
    pub fn apply_entity_update(&mut self, b: &[u8], now_ms: i64) -> bool {
        if b.len() < 3 {
            return false;
        }
        self.apply_attribute(b[0], b[1], &b[3..], now_ms)
    }

    /// Apply one attribute value, from an Entity Update or an Entity Attribute read.
    pub fn apply_attribute(&mut self, entity: u8, attribute: u8, raw: &[u8], now_ms: i64) -> bool {
        let value = String::from_utf8_lossy(raw).into_owned();
        let text = || Some(value.clone()).filter(|s| !s.is_empty());
        let before = self.clone();
        match (entity, attribute) {
            (ENTITY_PLAYER, PLAYER_NAME) => self.player = text(),
            (ENTITY_PLAYER, PLAYER_PLAYBACK_INFO) => {
                // "state,rate,elapsed", e.g. "1,1.0,42.317"
                let mut parts = value.split(',');
                self.state = match parts.next() {
                    Some("0") => PlaybackState::Paused,
                    Some("1") => PlaybackState::Playing,
                    Some("2") => PlaybackState::Rewinding,
                    Some("3") => PlaybackState::FastForwarding,
                    _ => PlaybackState::Unknown,
                };
                self.rate = parts.next().and_then(|s| s.parse().ok());
                self.elapsed = parts.next().and_then(|s| s.parse().ok());
                self.elapsed_at = self.elapsed.map(|_| now_ms);
            }
            (ENTITY_PLAYER, PLAYER_VOLUME) => self.volume = value.parse().ok(),
            (ENTITY_TRACK, TRACK_ARTIST) => self.artist = text(),
            (ENTITY_TRACK, TRACK_ALBUM) => self.album = text(),
            (ENTITY_TRACK, TRACK_TITLE) => self.title = text(),
            (ENTITY_TRACK, TRACK_DURATION) => self.duration = value.parse().ok(),
            _ => {}
        }
        *self != before
    }

    /// Apply a Remote Command notification: a list of supported command ids.
    pub fn apply_available_commands(&mut self, b: &[u8]) -> bool {
        let available: Vec<_> = b.iter().filter_map(|&id| RemoteCommand::name(id)).collect();
        let changed = available != self.available;
        self.available = available;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(entity: u8, attr: u8, v: &str) -> Vec<u8> {
        let mut out = vec![entity, attr, 0];
        out.extend_from_slice(v.as_bytes());
        out
    }

    #[test]
    fn applies_player_and_track_updates() {
        let mut np = NowPlaying::default();
        assert!(np.apply_entity_update(&update(0, 0, "Spotify"), 0));
        assert!(np.apply_entity_update(&update(0, 1, "1,1.0,42.5"), 0));
        assert!(np.apply_entity_update(&update(0, 2, "0.75"), 0));
        assert!(np.apply_entity_update(&update(2, 2, "Teardrop"), 0));
        assert!(np.apply_entity_update(&update(2, 0, "Massive Attack"), 0));
        assert!(np.apply_entity_update(&update(2, 3, "330.1"), 0));
        assert_eq!(np.player.as_deref(), Some("Spotify"));
        assert_eq!(np.state, PlaybackState::Playing);
        assert_eq!(np.elapsed, Some(42.5));
        assert_eq!(np.volume, Some(0.75));
        assert_eq!(np.title.as_deref(), Some("Teardrop"));
        assert_eq!(np.duration, Some(330.1));
        assert!(
            !np.apply_entity_update(&update(2, 2, "Teardrop"), 0),
            "same value is not a change"
        );
    }

    #[test]
    fn applies_attribute_reads() {
        let mut np = NowPlaying::default();
        for [e, a] in current_value_queries() {
            let value: &[u8] = match (e, a) {
                (0, 1) => b"1,1.0,12.5",
                (0, 2) => b"0.5",
                (2, 3) => b"200",
                _ => b"x",
            };
            np.apply_attribute(e, a, value, 0);
        }
        assert_eq!(np.state, PlaybackState::Playing);
        assert_eq!(np.volume, Some(0.5));
        assert_eq!(np.duration, Some(200.0));
        assert_eq!(np.title.as_deref(), Some("x"));
    }

    #[test]
    fn volume_change_does_not_move_the_playback_position() {
        let mut np = NowPlaying::default();
        np.apply_entity_update(&update(0, 1, "1,1.0,42.5"), 1_000);
        assert_eq!((np.elapsed, np.elapsed_at), (Some(42.5), Some(1_000)));
        // Pressing volume 30 s later reports only the volume.
        assert!(np.apply_entity_update(&update(0, 2, "0.9"), 31_000));
        assert_eq!(
            (np.elapsed, np.elapsed_at),
            (Some(42.5), Some(1_000)),
            "position anchor unchanged"
        );
        // A new playback report re-anchors.
        np.apply_entity_update(&update(0, 1, "1,1.0,72.6"), 31_500);
        assert_eq!((np.elapsed, np.elapsed_at), (Some(72.6), Some(31_500)));
    }

    #[test]
    fn empty_value_clears_text() {
        let mut np = NowPlaying {
            title: Some("x".into()),
            ..Default::default()
        };
        np.apply_entity_update(&update(2, 2, ""), 0);
        assert_eq!(np.title, None);
    }

    #[test]
    fn parses_available_commands() {
        let mut np = NowPlaying::default();
        assert!(np.apply_available_commands(&[0, 1, 2, 3, 4, 200]));
        assert_eq!(
            np.available,
            vec!["play", "pause", "togglePlayPause", "nextTrack", "previousTrack"]
        );
    }

    #[test]
    fn command_names_round_trip() {
        for id in 0..7 {
            let name = RemoteCommand::name(id).unwrap();
            assert_eq!(RemoteCommand::parse(name).unwrap().id(), id);
        }
    }
}
