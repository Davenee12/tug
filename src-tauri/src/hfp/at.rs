//! HFP AT commands from the hands-free side: build the few tug sends, parse what the phone
//! answers, and walk the Service Level Connection setup (HFP 1.7 §4.2) one reply at a time.

use thiserror::Error;

/// Hands-free features tug offers in AT+BRSF: none. No codec negotiation (so no AT+BAC),
/// no three-way calling (so no AT+CHLD=?), no HF indicators (so no AT+BIND). Just enough
/// to be allowed to dial.
pub const HF_FEATURES: u32 = 0;

pub fn brsf(features: u32) -> String {
    format!("AT+BRSF={features}\r")
}

pub const CIND_TEST: &str = "AT+CIND=?\r";
pub const CIND_READ: &str = "AT+CIND?\r";
/// Indicator events on (mode 3, ind 1), so the phone reports how the call is going.
pub const CMER: &str = "AT+CMER=3,0,0,1\r";

/// Longest dial string accepted; real numbers are far shorter.
const MAX_DIGITS: usize = 32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NumberError {
    #[error("there's no number to call")]
    Empty,
    #[error("the number is too long")]
    TooLong,
    #[error("“{0}” can't be dialed")]
    BadChar(char),
}

/// `ATD<number>;` for a voice call. Formatting (spaces, dashes, brackets, dots) is dropped;
/// a `+` may only lead; anything else (letters, an email address) is refused, never sent.
pub fn dial(number: &str) -> Result<String, NumberError> {
    let mut digits = String::new();
    for c in number.trim().chars() {
        match c {
            '0'..='9' | '*' | '#' => digits.push(c),
            '+' if digits.is_empty() => digits.push(c),
            ' ' | '-' | '(' | ')' | '.' => {}
            other => return Err(NumberError::BadChar(other)),
        }
    }
    if digits.trim_start_matches('+').is_empty() {
        return Err(NumberError::Empty);
    }
    if digits.len() > MAX_DIGITS {
        return Err(NumberError::TooLong);
    }
    Ok(format!("ATD{digits};\r"))
}

/// One line from the phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Ok,
    Error,
    CmeError(u16),
    /// `+BRSF: n`: the phone's (Audio Gateway's) features.
    Brsf(u32),
    /// `+CIND: ("service",(0,1)),("call",(0,1)),…`: indicator names, in index order.
    CindNames(Vec<String>),
    /// `+CIND: 1,0,0,…`: their current values.
    CindValues(Vec<u8>),
    /// `+CIEV: i,v`: indicator `i` (1-based) changed to `v`.
    Ciev {
        index: usize,
        value: u8,
    },
    /// RING, +CLIP, +BSIR and anything else tug doesn't act on.
    Other(String),
}

pub fn parse_reply(line: &str) -> Reply {
    let line = line.trim();
    if line == "OK" {
        return Reply::Ok;
    }
    if line == "ERROR" {
        return Reply::Error;
    }
    if let Some(code) = line.strip_prefix("+CME ERROR:") {
        return Reply::CmeError(code.trim().parse().unwrap_or(0));
    }
    if let Some(rest) = line.strip_prefix("+BRSF:") {
        if let Ok(f) = rest.trim().parse() {
            return Reply::Brsf(f);
        }
    }
    if let Some(rest) = line.strip_prefix("+CIND:") {
        let rest = rest.trim();
        if rest.starts_with('(') {
            return Reply::CindNames(quoted(rest));
        }
        let values: Option<Vec<u8>> = rest.split(',').map(|v| v.trim().parse().ok()).collect();
        if let Some(values) = values {
            return Reply::CindValues(values);
        }
    }
    if let Some(rest) = line.strip_prefix("+CIEV:") {
        if let Some((i, v)) = rest.split_once(',') {
            if let (Ok(index), Ok(value)) = (i.trim().parse(), v.trim().parse()) {
                return Reply::Ciev { index, value };
            }
        }
    }
    Reply::Other(line.to_string())
}

/// Every `"…"` in order.
fn quoted(s: &str) -> Vec<String> {
    s.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

/// Complete lines from what has arrived so far; a partial last line stays in `buf` for later.
/// The phone frames replies as `\r\n<text>\r\n`, so blank lines are dropped.
pub fn take_lines(buf: &mut String) -> Vec<String> {
    let Some(end) = buf.rfind(['\r', '\n']) else {
        return Vec::new();
    };
    let rest = buf.split_off(end + 1);
    let lines = buf
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    *buf = rest;
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Brsf,
    CindTest,
    CindRead,
    Cmer,
    Ready,
}

impl Step {
    fn command(self) -> &'static str {
        match self {
            Step::Brsf => "AT+BRSF",
            Step::CindTest => "AT+CIND=?",
            Step::CindRead => "AT+CIND?",
            Step::Cmer | Step::Ready => "AT+CMER",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SlcError {
    #[error("the iPhone refused the hands-free setup at {0}")]
    Refused(&'static str),
}

/// What to do after a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Send(String),
    Wait,
    Ready,
}

/// Where a call tug dialed has got to, from the phone's `+CIEV` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallProgress {
    Dialing,
    Ringing,
    Active,
}

/// The Service Level Connection setup, driven by replies. With tug offering no features,
/// it's always the four steps; the optional ones (AT+BAC, AT+CHLD=?, AT+BIND) only apply
/// when both sides offer the feature.
#[derive(Debug)]
pub struct Slc {
    step: Step,
    pub ag_features: Option<u32>,
    pub indicators: Vec<String>,
}

impl Slc {
    /// The state machine and the first command to send.
    pub fn start() -> (Self, String) {
        let slc = Self {
            step: Step::Brsf,
            ag_features: None,
            indicators: Vec::new(),
        };
        (slc, brsf(HF_FEATURES))
    }

    pub fn is_ready(&self) -> bool {
        self.step == Step::Ready
    }

    pub fn on_reply(&mut self, reply: &Reply) -> Result<Next, SlcError> {
        let next = match (self.step, reply) {
            (Step::Ready, _) => return Ok(Next::Ready),
            (step, Reply::Error | Reply::CmeError(_)) => return Err(SlcError::Refused(step.command())),
            (Step::Brsf, Reply::Brsf(f)) => {
                self.ag_features = Some(*f);
                Next::Wait
            }
            (Step::Brsf, Reply::Ok) => {
                self.step = Step::CindTest;
                Next::Send(CIND_TEST.into())
            }
            (Step::CindTest, Reply::CindNames(names)) => {
                self.indicators = names.clone();
                Next::Wait
            }
            (Step::CindTest, Reply::Ok) => {
                self.step = Step::CindRead;
                Next::Send(CIND_READ.into())
            }
            (Step::CindRead, Reply::Ok) => {
                self.step = Step::Cmer;
                Next::Send(CMER.into())
            }
            (Step::Cmer, Reply::Ok) => {
                self.step = Step::Ready;
                Next::Ready
            }
            // Values, RING, +CIEV and the like while setting up: nothing to answer.
            _ => Next::Wait,
        };
        Ok(next)
    }

    /// 1-based index of the named indicator ("call", "callsetup"), as `+CIEV` counts.
    fn indicator(&self, name: &str) -> Option<usize> {
        self.indicators.iter().position(|n| n == name).map(|i| i + 1)
    }

    /// What a `+CIEV` says about an outgoing call, if anything.
    pub fn progress(&self, reply: &Reply) -> Option<CallProgress> {
        let Reply::Ciev { index, value } = *reply else {
            return None;
        };
        if Some(index) == self.indicator("callsetup") {
            match value {
                2 => Some(CallProgress::Dialing),
                3 => Some(CallProgress::Ringing),
                _ => None,
            }
        } else if Some(index) == self.indicator("call") && value == 1 {
            Some(CallProgress::Active)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dial_strings_keep_only_what_can_be_dialed() {
        assert_eq!(dial("+1 (302) 555-0142").unwrap(), "ATD+13025550142;\r");
        assert_eq!(dial("302.555.0142").unwrap(), "ATD3025550142;\r");
        assert_eq!(dial("*67 72975#").unwrap(), "ATD*6772975#;\r");
        assert_eq!(dial("  "), Err(NumberError::Empty));
        assert_eq!(dial("+"), Err(NumberError::Empty));
        assert_eq!(dial("tay@icloud.com"), Err(NumberError::BadChar('t')));
        assert_eq!(dial("555;ATH"), Err(NumberError::BadChar(';')), "no command injection");
        assert_eq!(dial("1+2"), Err(NumberError::BadChar('+')));
        assert_eq!(dial(&"1".repeat(40)), Err(NumberError::TooLong));
    }

    #[test]
    fn parses_the_replies_tug_cares_about() {
        assert_eq!(parse_reply("OK"), Reply::Ok);
        assert_eq!(parse_reply(" ERROR "), Reply::Error);
        assert_eq!(parse_reply("+CME ERROR: 30"), Reply::CmeError(30));
        assert_eq!(parse_reply("+BRSF: 3943"), Reply::Brsf(3943));
        assert_eq!(
            parse_reply(r#"+CIND: ("service",(0,1)),("call",(0,1)),("callsetup",(0-3)),("battchg",(0-5))"#),
            Reply::CindNames(vec![
                "service".into(),
                "call".into(),
                "callsetup".into(),
                "battchg".into()
            ])
        );
        assert_eq!(parse_reply("+CIND: 1,0,0,5"), Reply::CindValues(vec![1, 0, 0, 5]));
        assert_eq!(parse_reply("+CIEV: 3,2"), Reply::Ciev { index: 3, value: 2 });
        assert_eq!(parse_reply("RING"), Reply::Other("RING".into()));
        assert_eq!(parse_reply("+CIEV: x"), Reply::Other("+CIEV: x".into()));
    }

    #[test]
    fn lines_are_split_across_reads() {
        let mut buf = String::from("\r\n+BRSF: 3943\r\n\r\nO");
        assert_eq!(take_lines(&mut buf), vec!["+BRSF: 3943"]);
        assert_eq!(buf, "O");
        buf.push_str("K\r\n");
        assert_eq!(take_lines(&mut buf), vec!["OK"]);
        assert!(buf.is_empty());
        assert!(take_lines(&mut buf).is_empty());
    }

    /// Feed a scripted phone through the setup; returns what tug sent.
    fn run(slc: &mut Slc, phone: &[&str]) -> Result<Vec<String>, SlcError> {
        let mut sent = Vec::new();
        for line in phone {
            match slc.on_reply(&parse_reply(line))? {
                Next::Send(cmd) => sent.push(cmd),
                Next::Wait | Next::Ready => {}
            }
        }
        Ok(sent)
    }

    #[test]
    fn walks_the_service_level_connection_setup() {
        let (mut slc, first) = Slc::start();
        assert_eq!(first, "AT+BRSF=0\r");
        let sent = run(
            &mut slc,
            &[
                "+BRSF: 3943",
                "OK",
                r#"+CIND: ("service",(0,1)),("call",(0,1)),("callsetup",(0-3))"#,
                "OK",
                "+CIND: 1,0,0",
                "OK",
                "OK",
            ],
        )
        .unwrap();
        assert_eq!(sent, vec![CIND_TEST, CIND_READ, CMER]);
        assert!(slc.is_ready());
        assert_eq!(slc.ag_features, Some(3943));
        assert_eq!(
            slc.progress(&Reply::Ciev { index: 3, value: 2 }),
            Some(CallProgress::Dialing)
        );
        assert_eq!(
            slc.progress(&Reply::Ciev { index: 3, value: 3 }),
            Some(CallProgress::Ringing)
        );
        assert_eq!(
            slc.progress(&Reply::Ciev { index: 2, value: 1 }),
            Some(CallProgress::Active)
        );
        assert_eq!(
            slc.progress(&Reply::Ciev { index: 1, value: 1 }),
            None,
            "service, not the call"
        );
    }

    #[test]
    fn unsolicited_lines_during_setup_are_ignored() {
        let (mut slc, _) = Slc::start();
        let sent = run(&mut slc, &["RING", "+BRSF: 1", "+CIEV: 1,1", "OK"]).unwrap();
        assert_eq!(sent, vec![CIND_TEST]);
        assert!(!slc.is_ready());
    }

    #[test]
    fn a_refusal_names_the_step() {
        let (mut slc, _) = Slc::start();
        assert_eq!(run(&mut slc, &["ERROR"]), Err(SlcError::Refused("AT+BRSF")));
        let (mut slc, _) = Slc::start();
        assert_eq!(
            run(&mut slc, &["+BRSF: 1", "OK", "+CME ERROR: 3"]),
            Err(SlcError::Refused("AT+CIND=?"))
        );
    }
}
