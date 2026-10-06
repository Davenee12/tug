//! Apple Media Service (AMS) wire protocol: now-playing info and remote control.
//!
//! Spec: https://developer.apple.com/library/archive/documentation/CoreBluetooth/Reference/AppleMediaService_Reference/
//!
//! Pure encode/decode only, like `ancs`.

use std::fmt;

use serde::Serialize;

pub const SERVICE: u128 = 0x89D3502B_0F36_433A_8EF4_C502AD55F8DC;
pub const REMOTE_COMMAND: u128 = 0x9B3C81D8_57B1_4A8A_B8DF_0E56F7CA51C2;
pub const ENTITY_UPDATE: u128 = 0x2F7CABCE_808D_411F_9A0C_BB92BA96C102;
pub const ENTITY_ATTRIBUTE: u128 = 0xC6B2F38C_23AB_46D8_A6AB_A3A870BBD5D7;

const ENTITY_PLAYER: u8 = 0;
const ENTITY_QUEUE: u8 = 1;
const ENTITY_TRACK: u8 = 2;

/// EntityUpdateFlagTruncated: the value was cut to fit the notification; the full
/// value is read through Entity Attribute.
const FLAG_TRUNCATED: u8 = 1 << 0;

const QUEUE_INDEX: u8 = 0;
const QUEUE_COUNT: u8 = 1;
const QUEUE_SHUFFLE_MODE: u8 = 2;
const QUEUE_REPEAT_MODE: u8 = 3;

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
    /// Off → repeat all → repeat one → off, as the phone's own button.
    AdvanceRepeatMode,
    /// Jump forward in the track (iOS uses a 15 s step).
    SkipForward,
    /// Jump back in the track (iOS uses a 15 s step).
    SkipBackward,
    /// Thumbs up the current track (Apple Music loves/favourites it).
    LikeTrack,
    /// Thumbs down the current track.
    DislikeTrack,
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
            Self::AdvanceRepeatMode => 7,
            Self::SkipForward => 9,
            Self::SkipBackward => 10,
            Self::LikeTrack => 11,
            Self::DislikeTrack => 12,
        }
    }

    /// The command's name, as the UI and the logs spell it.
    pub fn as_str(self) -> &'static str {
        Self::name(self.id()).unwrap_or("unknown")
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
            7 => "advanceRepeatMode",
            9 => "skipForward",
            10 => "skipBackward",
            11 => "likeTrack",
            12 => "dislikeTrack",
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
            "advanceRepeatMode" => Self::AdvanceRepeatMode,
            "skipForward" => Self::SkipForward,
            "skipBackward" => Self::SkipBackward,
            "likeTrack" => Self::LikeTrack,
            "dislikeTrack" => Self::DislikeTrack,
            _ => return None,
        })
    }
}

/// Every RemoteCommandID the spec defines, named for the log, including the ones tug
/// doesn't send, so the log shows exactly what the player offers.
pub fn command_label(id: u8) -> String {
    let name = match id {
        // The commands tug doesn't send are named here; the rest come from RemoteCommand::name.
        8 => "advanceShuffleMode",
        13 => "bookmarkTrack",
        _ => match RemoteCommand::name(id) {
            Some(n) => n,
            None => return format!("unknown({id})"),
        },
    };
    name.to_string()
}

/// A Remote Command notification (the player's supported commands) for the log.
pub fn describe_commands(b: &[u8]) -> String {
    let names: Vec<_> = b.iter().map(|&id| command_label(id)).collect();
    format!("[{}]", names.join(", "))
}

/// AMS-specific ATT error codes (spec "Error Codes"); `None` for generic ATT ones.
pub fn error_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0xA0 => "Invalid State (AMS not set up: Entity Update not subscribed)",
        0xA1 => "Invalid Command (improperly formatted)",
        0xA2 => "Absent Attribute (the attribute is empty)",
        _ => return None,
    })
}

/// `player`, `queue`, `track`, for the log.
pub fn entity_name(entity: u8) -> String {
    match entity {
        ENTITY_PLAYER => "player".into(),
        ENTITY_QUEUE => "queue".into(),
        ENTITY_TRACK => "track".into(),
        e => format!("entity{e}"),
    }
}

/// `queue/repeatMode` and the like, for the log.
pub fn attribute_name(entity: u8, attribute: u8) -> String {
    let attr = match (entity, attribute) {
        (ENTITY_PLAYER, PLAYER_NAME) => "name",
        (ENTITY_PLAYER, PLAYER_PLAYBACK_INFO) => "playbackInfo",
        (ENTITY_PLAYER, PLAYER_VOLUME) => "volume",
        (ENTITY_QUEUE, QUEUE_INDEX) => "index",
        (ENTITY_QUEUE, QUEUE_COUNT) => "count",
        (ENTITY_QUEUE, QUEUE_SHUFFLE_MODE) => "shuffleMode",
        (ENTITY_QUEUE, QUEUE_REPEAT_MODE) => "repeatMode",
        (ENTITY_TRACK, TRACK_ARTIST) => "artist",
        (ENTITY_TRACK, TRACK_ALBUM) => "album",
        (ENTITY_TRACK, TRACK_TITLE) => "title",
        (ENTITY_TRACK, TRACK_DURATION) => "duration",
        _ => return format!("{}/attr{attribute}", entity_name(entity)),
    };
    format!("{}/{attr}", entity_name(entity))
}

/// One Entity Update notification: `[EntityID][AttributeID][EntityUpdateFlags][value…]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityUpdate<'a> {
    pub entity: u8,
    pub attribute: u8,
    /// The value was cut to fit the notification; the full value can be read
    /// through Entity Attribute.
    pub truncated: bool,
    pub value: &'a [u8],
}

impl<'a> EntityUpdate<'a> {
    pub fn parse(b: &'a [u8]) -> Option<Self> {
        match b {
            [entity, attribute, flags, value @ ..] => Some(Self {
                entity: *entity,
                attribute: *attribute,
                truncated: flags & FLAG_TRUNCATED != 0,
                value,
            }),
            _ => None,
        }
    }
}

impl fmt::Display for EntityUpdate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} = {:?}",
            attribute_name(self.entity, self.attribute),
            String::from_utf8_lossy(self.value)
        )?;
        if self.truncated {
            f.write_str(" (truncated)")?;
        }
        Ok(())
    }
}

/// Entity Update writes that register for player, queue and track changes.
/// Each must be written separately: one entity per write.
pub fn registrations() -> [Vec<u8>; 3] {
    [
        vec![ENTITY_PLAYER, PLAYER_NAME, PLAYER_PLAYBACK_INFO, PLAYER_VOLUME],
        vec![ENTITY_QUEUE, QUEUE_REPEAT_MODE],
        vec![ENTITY_TRACK, TRACK_ARTIST, TRACK_ALBUM, TRACK_TITLE, TRACK_DURATION],
    ]
}

/// Whether media can work without this registration. The queue only feeds the loop
/// button, so a phone or player that refuses it shouldn't take the whole Now Playing
/// card down; player and track are the card itself.
pub fn registration_is_optional(registration: &[u8]) -> bool {
    registration.first() == Some(&ENTITY_QUEUE)
}

/// Every (entity, attribute) tug shows. Entity Update only reports *changes*, so on
/// connect each one is read through Entity Attribute (write the pair, then read)
/// to pick up whatever is already playing.
pub fn current_value_queries() -> [[u8; 2]; 8] {
    [
        [ENTITY_PLAYER, PLAYER_NAME],
        [ENTITY_PLAYER, PLAYER_PLAYBACK_INFO],
        [ENTITY_PLAYER, PLAYER_VOLUME],
        [ENTITY_QUEUE, QUEUE_REPEAT_MODE],
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepeatMode {
    Off,
    One,
    All,
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
    /// The player's repeat mode, when it reports one.
    pub repeat: Option<RepeatMode>,
    /// Remote commands the current player accepts.
    pub available: Vec<&'static str>,
}

impl NowPlaying {
    /// Apply one Entity Update notification: `[entity][attr][flags][utf8 value]`.
    /// Returns true if anything changed.
    pub fn apply_entity_update(&mut self, b: &[u8], now_ms: i64) -> bool {
        match EntityUpdate::parse(b) {
            Some(u) => self.apply_attribute(u.entity, u.attribute, u.value, now_ms),
            None => false,
        }
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
            (ENTITY_QUEUE, QUEUE_REPEAT_MODE) => {
                self.repeat = match value.trim() {
                    "0" => Some(RepeatMode::Off),
                    "1" => Some(RepeatMode::One),
                    "2" => Some(RepeatMode::All),
                    _ => None,
                }
            }
            (ENTITY_TRACK, TRACK_ARTIST) => self.artist = text(),
            (ENTITY_TRACK, TRACK_ALBUM) => self.album = text(),
            (ENTITY_TRACK, TRACK_TITLE) => self.title = text(),
            (ENTITY_TRACK, TRACK_DURATION) => self.duration = value.parse().ok(),
            _ => {}
        }
        *self != before
    }

    /// Whether the current player listed `command` among its supported commands.
    pub fn lists(&self, command: RemoteCommand) -> bool {
        self.available.contains(&command.as_str())
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
    fn repeat_mode_follows_the_queue() {
        let mut np = NowPlaying::default();
        assert!(np.apply_entity_update(&update(1, 3, "2"), 0));
        assert_eq!(np.repeat, Some(RepeatMode::All));
        assert!(np.apply_entity_update(&update(1, 3, "1"), 0));
        assert_eq!(np.repeat, Some(RepeatMode::One));
        assert!(np.apply_entity_update(&update(1, 3, "0"), 0));
        assert_eq!(np.repeat, Some(RepeatMode::Off));
        // Other queue attributes (index, count, shuffle) aren't shown.
        assert!(!np.apply_entity_update(&update(1, 0, "4"), 0));
        assert_eq!(
            RemoteCommand::parse("advanceRepeatMode").map(RemoteCommand::id),
            Some(7)
        );
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
        // 0–7 and 9–12 are commands tug sends; 8 (shuffle) and 13 (bookmark) are not.
        for id in [0, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12] {
            let name = RemoteCommand::name(id).unwrap();
            let command = RemoteCommand::parse(name).unwrap();
            assert_eq!(command.id(), id);
            assert_eq!(command.as_str(), name);
        }
        assert_eq!(RemoteCommand::name(8), None, "tug doesn't send shuffle");
        assert_eq!(RemoteCommand::name(13), None, "tug doesn't send bookmark");
    }

    #[test]
    fn exposes_skip_and_like_when_the_player_lists_them() {
        let mut np = NowPlaying::default();
        // Apple Music-like: skip and like/dislike offered (shuffle 8 is ignored, repeat 7 too).
        np.apply_available_commands(&[0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12]);
        assert!(np.lists(RemoteCommand::SkipForward));
        assert!(np.lists(RemoteCommand::SkipBackward));
        assert!(np.lists(RemoteCommand::LikeTrack));
        assert!(np.lists(RemoteCommand::DislikeTrack));
        // Shuffle (8) isn't a command tug sends, so it never reaches `available`.
        assert!(!np.available.contains(&"advanceShuffleMode"));
        // Spotify-like: skip but no like.
        np.apply_available_commands(&[0, 1, 2, 3, 4, 5, 6, 9, 10]);
        assert!(np.lists(RemoteCommand::SkipForward));
        assert!(!np.lists(RemoteCommand::LikeTrack));
    }

    #[test]
    fn parses_entity_update_header_and_truncation_flag() {
        let u = EntityUpdate::parse(&[1, 3, 0, b'2']).unwrap();
        assert_eq!((u.entity, u.attribute, u.truncated, u.value), (1, 3, false, &b"2"[..]));
        assert_eq!(u.to_string(), r#"queue/repeatMode = "2""#);

        let u = EntityUpdate::parse(&[2, 2, 1, b'L', b'o']).unwrap();
        assert!(u.truncated);
        assert_eq!(u.to_string(), r#"track/title = "Lo" (truncated)"#);
        // Other flag bits are reserved and don't mean truncated.
        assert!(!EntityUpdate::parse(&[2, 2, 0b10]).unwrap().truncated);

        // An empty value is still a valid update (it clears the attribute).
        let u = EntityUpdate::parse(&[2, 0, 0]).unwrap();
        assert_eq!(u.value, b"");
        assert_eq!(EntityUpdate::parse(&[2, 0]), None);
        assert_eq!(EntityUpdate::parse(&[]), None);
    }

    #[test]
    fn names_attributes_for_the_log() {
        assert_eq!(attribute_name(0, 1), "player/playbackInfo");
        assert_eq!(attribute_name(1, 2), "queue/shuffleMode");
        assert_eq!(attribute_name(1, 3), "queue/repeatMode");
        assert_eq!(attribute_name(2, 3), "track/duration");
        assert_eq!(attribute_name(1, 9), "queue/attr9");
        assert_eq!(attribute_name(7, 0), "entity7/attr0");
    }

    #[test]
    fn describes_every_spec_command_for_the_log() {
        assert_eq!(
            describe_commands(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 200]),
            "[play, pause, togglePlayPause, nextTrack, previousTrack, volumeUp, volumeDown, \
             advanceRepeatMode, advanceShuffleMode, skipForward, skipBackward, likeTrack, \
             dislikeTrack, bookmarkTrack, unknown(200)]"
        );
        assert_eq!(describe_commands(&[]), "[]");
    }

    #[test]
    fn names_ams_error_codes() {
        assert!(error_name(0xA0).unwrap().starts_with("Invalid State"));
        assert!(error_name(0xA1).unwrap().starts_with("Invalid Command"));
        assert!(error_name(0xA2).unwrap().starts_with("Absent Attribute"));
        assert_eq!(error_name(0x0E), None);
    }

    #[test]
    fn queue_registration_is_separate_and_optional() {
        let regs = registrations();
        // One entity per write, as the spec requires.
        assert_eq!(regs.iter().map(|r| r[0]).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(regs[1], vec![1, 3]);
        assert!(registration_is_optional(&regs[1]));
        assert!(!registration_is_optional(&regs[0]));
        assert!(!registration_is_optional(&regs[2]));
    }

    #[test]
    fn repeat_support_comes_from_the_command_list_not_the_queue_value() {
        // Spotify-like: reports a repeat mode but doesn't list AdvanceRepeatMode.
        let mut np = NowPlaying::default();
        np.apply_attribute(1, 3, b"0", 0);
        np.apply_available_commands(&[0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(np.repeat, Some(RepeatMode::Off));
        assert!(!np.lists(RemoteCommand::AdvanceRepeatMode));
        // Apple Music-like: lists it.
        np.apply_available_commands(&[0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(np.lists(RemoteCommand::AdvanceRepeatMode));
        assert!(np.lists(RemoteCommand::NextTrack));
    }

    #[test]
    fn unknown_repeat_value_is_none() {
        let mut np = NowPlaying::default();
        np.apply_entity_update(&update(1, 3, "2"), 0);
        assert!(np.apply_entity_update(&update(1, 3, ""), 0));
        assert_eq!(np.repeat, None);
        np.apply_entity_update(&update(1, 3, "1 "), 0);
        assert_eq!(np.repeat, Some(RepeatMode::One));
    }
}
