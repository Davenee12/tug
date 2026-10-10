//! "Play iPhone audio on this PC": the decisions, as plain values (see `pc_audio` for the Windows
//! side). The PC becomes a Bluetooth speaker for the paired iPhone through Windows'
//! AudioPlaybackConnection; this module decides what each step means, which Windows device is the
//! iPhone, and when to turn on by itself. Every type that reaches the UI is mirrored in
//! `src/types/protocol.ts`; the words the UI shows for each state live in `src/lib/pcAudio.ts`.

use serde::Serialize;

/// Whether the iPhone's audio is playing on this PC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PcAudioState {
    #[default]
    Off,
    /// Finding the iPhone and asking it to send its audio here.
    Connecting,
    /// The connection is open: the iPhone can play through this PC's speakers.
    On,
}

/// Why it's off when the person wanted it on. Each one has its own calm sentence in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PcAudioProblem {
    /// Windows doesn't list the iPhone as something it can play audio from.
    NotFound,
    /// Windows said no (`DeniedBySystem`).
    Denied,
    /// The iPhone didn't answer in time (`RequestTimedOut`, or tug's own time limit).
    TimedOut,
    /// Anything else (`UnknownFailure`, or a Windows error).
    Failed,
    /// It was on, and the connection closed (the iPhone went away or chose another speaker).
    Dropped,
}

/// Everything the UI shows for this feature (`PcAudioStatus` in protocol.ts).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcAudioStatus {
    /// This version of Windows can do it at all (Windows 10 version 2004 and later).
    pub supported: bool,
    pub state: PcAudioState,
    pub problem: Option<PcAudioProblem>,
    /// "Turn on automatically when my iPhone connects", for the phone tug uses now.
    pub auto: bool,
}

/// What Windows answered to opening the connection (`AudioPlaybackConnectionOpenResultStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStatus {
    Success,
    RequestTimedOut,
    DeniedBySystem,
    UnknownFailure,
    /// A value newer than this build knows.
    Other,
}

impl OpenStatus {
    /// From the raw enum value Windows returns.
    pub fn from_raw(v: i32) -> Self {
        match v {
            0 => Self::Success,
            1 => Self::RequestTimedOut,
            2 => Self::DeniedBySystem,
            3 => Self::UnknownFailure,
            _ => Self::Other,
        }
    }

    /// The problem to show, or None when it opened.
    pub fn problem(self) -> Option<PcAudioProblem> {
        match self {
            Self::Success => None,
            Self::RequestTimedOut => Some(PcAudioProblem::TimedOut),
            Self::DeniedBySystem => Some(PcAudioProblem::Denied),
            Self::UnknownFailure | Self::Other => Some(PcAudioProblem::Failed),
        }
    }
}

/// Something that happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// "Play on this PC", "Try again", "Reconnect", or turning on automatically.
    TurnOn,
    /// "Stop", Forget, switching phones, or quitting.
    TurnOff,
    /// An attempt finished: None when it opened, else why not.
    Opened {
        attempt: u64,
        problem: Option<PcAudioProblem>,
    },
    /// Windows reported the connection from this attempt closed.
    Closed { attempt: u64 },
}

/// What the Windows side should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Start attempt `n`: find the iPhone, create the connection, start it and open it.
    Open(u64),
    /// Keep the connection this attempt opened, for as long as it stays on.
    Keep,
    /// Close the connection this attempt produced, if any: it failed, or it's no longer wanted.
    Discard,
    /// Close the live connection and let it go.
    Release,
}

/// The on/off state machine. Each attempt has a number, so a late answer from an attempt the
/// person already stopped (or a close from an old connection) can't turn things back on.
#[derive(Debug, Default)]
pub struct Machine {
    state: PcAudioState,
    problem: Option<PcAudioProblem>,
    attempt: u64,
}

impl Machine {
    pub fn state(&self) -> PcAudioState {
        self.state
    }

    pub fn problem(&self) -> Option<PcAudioProblem> {
        self.problem
    }

    pub fn step(&mut self, input: Input) -> Action {
        match (self.state, input) {
            (PcAudioState::Off, Input::TurnOn) => {
                self.attempt += 1;
                self.state = PcAudioState::Connecting;
                self.problem = None;
                Action::Open(self.attempt)
            }
            (_, Input::TurnOn) => Action::Nothing,
            (PcAudioState::Off, Input::TurnOff) => {
                self.problem = None;
                Action::Nothing
            }
            (PcAudioState::Connecting, Input::TurnOff) => {
                // Whatever the attempt in flight brings back is now stale.
                self.attempt += 1;
                self.state = PcAudioState::Off;
                self.problem = None;
                Action::Release
            }
            (PcAudioState::On, Input::TurnOff) => {
                self.state = PcAudioState::Off;
                self.problem = None;
                Action::Release
            }
            (PcAudioState::Connecting, Input::Opened { attempt, problem }) if attempt == self.attempt => {
                match problem {
                    None => {
                        self.state = PcAudioState::On;
                        Action::Keep
                    }
                    Some(p) => {
                        self.state = PcAudioState::Off;
                        self.problem = Some(p);
                        Action::Discard
                    }
                }
            }
            // A late answer from an attempt that was stopped or replaced.
            (_, Input::Opened { .. }) => Action::Discard,
            (PcAudioState::On, Input::Closed { attempt }) if attempt == self.attempt => {
                self.state = PcAudioState::Off;
                self.problem = Some(PcAudioProblem::Dropped);
                Action::Release
            }
            // Closed while still opening: the open itself reports how it went.
            (_, Input::Closed { .. }) => Action::Nothing,
        }
    }
}

/// Turn on by itself now? Only when the owner opted in for *this* phone, the phone has just
/// connected (not on every status update while connected), and it isn't already on or trying.
pub fn should_auto_start(
    auto_device: Option<&str>,
    device_id: Option<&str>,
    was_connected: bool,
    is_connected: bool,
    state: PcAudioState,
) -> bool {
    let opted_in = matches!((auto_device, device_id), (Some(a), Some(d)) if !a.is_empty() && a == d);
    opted_in && is_connected && !was_connected && state == PcAudioState::Off
}

/// One device Windows could play audio from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
}

/// What tug knows about its iPhone, to find it among the candidates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhoneIds {
    /// Bluetooth addresses as 12 lowercase hex digits, no separators.
    pub addresses: Vec<String>,
    pub names: Vec<String>,
}

/// The Bluetooth address at the end of a Windows Bluetooth device id
/// ("BluetoothLE#BluetoothLE<adapter>-c4:b3:01:aa:bb:cc", the same for Classic), as 12 lowercase
/// hex digits.
pub fn address_from_device_id(id: &str) -> Option<String> {
    let tail = id.rsplit('-').next()?;
    let hex: String = tail
        .chars()
        .filter(|c| *c != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    (hex.len() == 12 && hex.chars().all(|c| c.is_ascii_hexdigit()) && hex != "000000000000").then_some(hex)
}

/// Which candidate is the iPhone: an address match first (an id names the device it belongs to),
/// then a name match, but only an unambiguous one. Never a guess, even with one candidate: playing
/// a different phone's audio, or a speaker's, would be worse than saying it wasn't found.
pub fn pick(candidates: &[Candidate], phone: &PhoneIds) -> Option<usize> {
    let flat = |s: &str| s.to_ascii_lowercase().replace(':', "");
    if let Some(i) = candidates.iter().position(|c| {
        let id = flat(&c.id);
        phone.addresses.iter().any(|a| !a.is_empty() && id.contains(a.as_str()))
    }) {
        return Some(i);
    }
    let names: Vec<&str> = phone.names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect();
    let by_name: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| names.iter().any(|n| n.eq_ignore_ascii_case(c.name.trim())))
        .map(|(i, _)| i)
        .collect();
    (by_name.len() == 1).then(|| by_name[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(m: &mut Machine, problem: Option<PcAudioProblem>) -> Action {
        let attempt = m.attempt;
        m.step(Input::Opened { attempt, problem })
    }

    #[test]
    fn turning_on_opens_once_and_success_keeps_the_connection() {
        let mut m = Machine::default();
        assert_eq!(m.step(Input::TurnOn), Action::Open(1));
        assert_eq!(m.state(), PcAudioState::Connecting);
        assert_eq!(
            m.step(Input::TurnOn),
            Action::Nothing,
            "a second press doesn't start a rival"
        );
        assert_eq!(opened(&mut m, None), Action::Keep);
        assert_eq!(m.state(), PcAudioState::On);
        assert_eq!(m.step(Input::TurnOn), Action::Nothing);
    }

    #[test]
    fn a_failed_open_is_off_with_its_reason_and_trying_again_clears_it() {
        let mut m = Machine::default();
        m.step(Input::TurnOn);
        assert_eq!(opened(&mut m, Some(PcAudioProblem::TimedOut)), Action::Discard);
        assert_eq!(m.state(), PcAudioState::Off);
        assert_eq!(m.problem(), Some(PcAudioProblem::TimedOut));
        assert_eq!(m.step(Input::TurnOn), Action::Open(2));
        assert_eq!(m.problem(), None);
    }

    #[test]
    fn stop_releases_and_a_late_answer_from_the_stopped_attempt_is_discarded() {
        let mut m = Machine::default();
        m.step(Input::TurnOn);
        assert_eq!(m.step(Input::TurnOff), Action::Release);
        assert_eq!(m.state(), PcAudioState::Off);
        // The attempt the person stopped comes back successful: it must not turn audio on.
        assert_eq!(
            m.step(Input::Opened {
                attempt: 1,
                problem: None
            }),
            Action::Discard
        );
        assert_eq!(m.state(), PcAudioState::Off);
        // Nor can it, once a newer attempt is running.
        assert_eq!(m.step(Input::TurnOn), Action::Open(3));
        assert_eq!(
            m.step(Input::Opened {
                attempt: 1,
                problem: None
            }),
            Action::Discard
        );
        assert_eq!(m.state(), PcAudioState::Connecting);
    }

    #[test]
    fn stop_while_on_releases_and_clears_any_problem() {
        let mut m = Machine::default();
        m.step(Input::TurnOn);
        opened(&mut m, None);
        assert_eq!(m.step(Input::TurnOff), Action::Release);
        assert_eq!((m.state(), m.problem()), (PcAudioState::Off, None));
        // Stopping when already off just dismisses the last problem.
        m.step(Input::TurnOn);
        opened(&mut m, Some(PcAudioProblem::Denied));
        assert_eq!(m.step(Input::TurnOff), Action::Nothing);
        assert_eq!(m.problem(), None);
    }

    #[test]
    fn a_close_while_on_is_dropped_and_offers_reconnect() {
        let mut m = Machine::default();
        m.step(Input::TurnOn);
        opened(&mut m, None);
        assert_eq!(m.step(Input::Closed { attempt: 1 }), Action::Release);
        assert_eq!(
            (m.state(), m.problem()),
            (PcAudioState::Off, Some(PcAudioProblem::Dropped))
        );
        // Reconnect is just turning on again.
        assert_eq!(m.step(Input::TurnOn), Action::Open(2));
    }

    #[test]
    fn closes_from_old_connections_or_mid_open_change_nothing() {
        let mut m = Machine::default();
        m.step(Input::TurnOn);
        assert_eq!(
            m.step(Input::Closed { attempt: 1 }),
            Action::Nothing,
            "opening reports for itself"
        );
        assert_eq!(m.state(), PcAudioState::Connecting);
        opened(&mut m, None);
        m.step(Input::TurnOff);
        m.step(Input::TurnOn);
        opened(&mut m, None);
        assert_eq!(m.step(Input::Closed { attempt: 1 }), Action::Nothing);
        assert_eq!(m.state(), PcAudioState::On);
    }

    #[test]
    fn open_statuses_map_to_problems() {
        assert_eq!(OpenStatus::from_raw(0).problem(), None);
        assert_eq!(OpenStatus::from_raw(1).problem(), Some(PcAudioProblem::TimedOut));
        assert_eq!(OpenStatus::from_raw(2).problem(), Some(PcAudioProblem::Denied));
        assert_eq!(OpenStatus::from_raw(3).problem(), Some(PcAudioProblem::Failed));
        assert_eq!(OpenStatus::from_raw(42).problem(), Some(PcAudioProblem::Failed));
    }

    #[test]
    fn auto_start_only_for_the_opted_in_phone_on_connect() {
        use PcAudioState::*;
        assert!(should_auto_start(Some("a"), Some("a"), false, true, Off));
        assert!(!should_auto_start(None, Some("a"), false, true, Off), "off by default");
        assert!(
            !should_auto_start(Some("a"), Some("b"), false, true, Off),
            "another phone"
        );
        assert!(!should_auto_start(Some(""), Some(""), false, true, Off));
        assert!(!should_auto_start(Some("a"), None, false, true, Off));
        assert!(
            !should_auto_start(Some("a"), Some("a"), true, true, Off),
            "only on the connect itself"
        );
        assert!(!should_auto_start(Some("a"), Some("a"), false, false, Off));
        assert!(!should_auto_start(Some("a"), Some("a"), false, true, On));
        assert!(!should_auto_start(Some("a"), Some("a"), false, true, Connecting));
    }

    #[test]
    fn reads_the_address_from_windows_device_ids() {
        assert_eq!(
            address_from_device_id("BluetoothLE#BluetoothLE00:1a:7d:da:71:0a-C4:B3:01:AA:BB:CC").as_deref(),
            Some("c4b301aabbcc")
        );
        assert_eq!(
            address_from_device_id("Bluetooth#Bluetooth00:1a:7d:da:71:0a-c4:b3:01:aa:bb:cd").as_deref(),
            Some("c4b301aabbcd")
        );
        assert_eq!(address_from_device_id(""), None);
        assert_eq!(address_from_device_id("not-an-id"), None);
        assert_eq!(address_from_device_id("x-00:00:00:00:00:00"), None);
    }

    fn c(id: &str, name: &str) -> Candidate {
        Candidate {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn picks_by_address_first_then_an_unambiguous_name() {
        let list = [
            c(
                r"\\?\BTHENUM#{0000110a}_LOCALMFG&0002#7&1&0&AABBCCDDEEFF_C00000000#{x}",
                "Speaker",
            ),
            c(
                r"\\?\BTHENUM#{0000110a}_LOCALMFG&0002#7&1&0&C4B301AABBCC_C00000000#{x}",
                "Jordan's iPhone",
            ),
        ];
        let by_address = PhoneIds {
            addresses: vec!["c4b301aabbcc".into()],
            names: vec!["Speaker".into()],
        };
        assert_eq!(pick(&list, &by_address), Some(1), "the address wins over a name");
        let by_name = PhoneIds {
            addresses: vec!["000000000001".into()],
            names: vec![" jordan's iphone ".into()],
        };
        assert_eq!(pick(&list, &by_name), Some(1));
        // An id written with colons still matches.
        let colons = [c("Bluetooth#Bluetooth00:11:22:33:44:55-c4:b3:01:aa:bb:cc", "")];
        assert_eq!(pick(&colons, &by_address), Some(0));
    }

    #[test]
    fn never_guesses() {
        let one = [c("id-1", "Speaker")];
        let phone = PhoneIds {
            addresses: vec!["c4b301aabbcc".into()],
            names: vec!["Jordan's iPhone".into()],
        };
        assert_eq!(pick(&one, &phone), None, "a lone candidate that isn't the iPhone");
        let twins = [c("id-1", "iPhone"), c("id-2", "iPhone")];
        let generic = PhoneIds {
            addresses: vec![],
            names: vec!["iPhone".into()],
        };
        assert_eq!(pick(&twins, &generic), None, "two phones with the same name");
        let blank = PhoneIds {
            addresses: vec![String::new()],
            names: vec![String::new()],
        };
        assert_eq!(pick(&[c("id", "")], &blank), None);
        assert_eq!(pick(&[], &phone), None);
    }
}
