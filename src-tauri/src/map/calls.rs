//! PBAP call history (`telecom/cch.vcf`, or `ich`/`och`/`mch`): who called, which way, when.
//! Each call is a vCard whose `X-IRMC-CALL-DATETIME` carries the direction as a parameter
//! (`;MISSED` in 2.1, `;TYPE=MISSED` in 3.0) and the time, phone-local unless it ends in `Z`.

use serde::Serialize;

use super::address::normalize;
use super::vcard::{property, structured_name, unfold};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallDirection {
    Incoming,
    Outgoing,
    Missed,
}

impl CallDirection {
    fn from_param(p: &str) -> Option<Self> {
        let p = p.strip_prefix("TYPE=").unwrap_or(p);
        p.split(',').find_map(|t| match t.trim() {
            "RECEIVED" => Some(Self::Incoming),
            "DIALED" => Some(Self::Outgoing),
            "MISSED" => Some(Self::Missed),
            _ => None,
        })
    }
}

/// One call from the phone's recents. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallRecord {
    pub direction: CallDirection,
    /// The name the phone showed, if the caller is a contact.
    pub name: Option<String>,
    /// Normalised like message addresses; `None` for a withheld number.
    pub number: Option<String>,
    /// ISO time: phone-local without a zone, or UTC with `Z` when the phone says so.
    pub at: Option<String>,
}

/// Parse a call-history object. `default` is the direction for a list that is one kind of
/// call (`mch` is all missed); a combined list's cards each say which they are, and a card
/// that says nothing in a combined list is skipped rather than guessed.
pub fn parse(raw: &str, default: Option<CallDirection>) -> Vec<CallRecord> {
    let mut out = Vec::new();
    let mut card: Option<Card> = None;
    for line in unfold(raw) {
        let Some((key, params, value)) = property(&line) else {
            continue;
        };
        let value = value.trim();
        match key.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VCARD") => card = Some(Card::default()),
            "END" if value.eq_ignore_ascii_case("VCARD") => {
                if let Some(record) = card.take().and_then(|c| c.finish(default)) {
                    out.push(record);
                }
            }
            _ => {
                let Some(c) = card.as_mut() else { continue };
                match key.as_str() {
                    "FN" if !value.is_empty() => c.fn_name = Some(value.to_string()),
                    "N" => c.n_name = structured_name(value),
                    // The first number is the one that called; a contact's others don't matter here.
                    "TEL" if !value.is_empty() && c.number.is_none() => c.number = Some(value.to_string()),
                    "X-IRMC-CALL-DATETIME" => {
                        c.direction = params.iter().find_map(|p| CallDirection::from_param(p));
                        c.at = call_time(value);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

#[derive(Default)]
struct Card {
    fn_name: Option<String>,
    n_name: Option<String>,
    number: Option<String>,
    direction: Option<CallDirection>,
    at: Option<String>,
}

impl Card {
    fn finish(self, default: Option<CallDirection>) -> Option<CallRecord> {
        let direction = self.direction.or(default)?;
        let number = self.number.map(|n| normalize(&n)).filter(|n| !n.is_empty());
        // Unknown callers come back named after their number (or nothing): that's not a name.
        let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
        let name = self.fn_name.or(self.n_name).filter(|name| {
            let d = digits(name);
            let is_number = !d.is_empty() && number.as_deref().is_some_and(|n| digits(n).ends_with(&d));
            !is_number && !name.trim().is_empty()
        });
        Some(CallRecord {
            direction,
            name,
            number,
            at: self.at,
        })
    }
}

/// `20250320T100000` → `2025-03-20T10:00:00`; a trailing `Z` or `±hhmm` is kept as a zone.
fn call_time(s: &str) -> Option<String> {
    let iso = crate::ancs::ancs_date_to_iso(s.get(..15)?)?;
    match &s.as_bytes()[15..] {
        [] => Some(iso),
        [b'Z'] => Some(format!("{iso}Z")),
        [sign @ (b'+' | b'-'), rest @ ..] if rest.len() == 4 && rest.iter().all(u8::is_ascii_digit) => {
            Some(format!("{iso}{}{}:{}", *sign as char, &s[16..18], &s[18..20]))
        }
        _ => Some(iso),
    }
}

/// Several lists (`ich`, `och`, `mch`) as one, newest first, at most `max`. Calls without a
/// time can't be placed, so they go last.
pub fn merge(lists: Vec<Vec<CallRecord>>, max: usize) -> Vec<CallRecord> {
    let mut all: Vec<CallRecord> = lists.into_iter().flatten().collect();
    // Stable, so each list's own (newest-first) order holds among equal or missing times.
    all.sort_by(|a, b| match (&a.at, &b.at) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    all.truncate(max);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_combined_list_in_either_vcard_version() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Zoe\r\nN:;Zoe;;;\r\nTEL;TYPE=CELL:+1 (302) 555-0142\r\n\
                   X-IRMC-CALL-DATETIME;TYPE=MISSED:20261005T093012\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:2.1\r\nN:Smith;Chris\r\nTEL:2145550199\r\n\
                   X-IRMC-CALL-DATETIME;DIALED:20261004T181500Z\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:3.0\r\nFN:\r\nN:;;;;\r\nTEL:+44 20 7946 0958\r\n\
                   X-IRMC-CALL-DATETIME;TYPE=RECEIVED:20261003T080000+0100\r\nEND:VCARD\r\n";
        assert_eq!(
            parse(raw, None),
            vec![
                CallRecord {
                    direction: CallDirection::Missed,
                    name: Some("Zoe".into()),
                    number: Some("+13025550142".into()),
                    at: Some("2026-10-05T09:30:12".into()),
                },
                CallRecord {
                    direction: CallDirection::Outgoing,
                    name: Some("Chris Smith".into()),
                    number: Some("+12145550199".into()),
                    at: Some("2026-10-04T18:15:00Z".into()),
                },
                CallRecord {
                    direction: CallDirection::Incoming,
                    name: None,
                    number: Some("+442079460958".into()),
                    at: Some("2026-10-03T08:00:00+01:00".into()),
                },
            ]
        );
    }

    #[test]
    fn a_name_that_is_just_the_number_is_no_name() {
        let raw = "BEGIN:VCARD\nVERSION:3.0\nFN:(302) 555-0100\nTEL:+13025550100\n\
                   X-IRMC-CALL-DATETIME;TYPE=RECEIVED:20261005T100000\nEND:VCARD\n";
        let got = parse(raw, None);
        assert_eq!(got[0].name, None);
        assert_eq!(got[0].number.as_deref(), Some("+13025550100"));
    }

    #[test]
    fn withheld_numbers_and_single_kind_lists() {
        // `mch.vcf`: every card is a missed call even when its parameter is missing.
        let raw = "BEGIN:VCARD\nVERSION:3.0\nFN:\nTEL:\nX-IRMC-CALL-DATETIME:20261005T070000\nEND:VCARD\n";
        assert_eq!(
            parse(raw, Some(CallDirection::Missed)),
            vec![CallRecord {
                direction: CallDirection::Missed,
                name: None,
                number: None,
                at: Some("2026-10-05T07:00:00".into()),
            }]
        );
        assert!(
            parse(raw, None).is_empty(),
            "a combined-list card with no direction is skipped"
        );
    }

    #[test]
    fn bad_times_are_dropped_not_fatal() {
        let raw = "BEGIN:VCARD\nTEL:72975\nX-IRMC-CALL-DATETIME;MISSED:yesterday\nEND:VCARD\n";
        let got = parse(raw, None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].at, None);
        assert_eq!(got[0].number.as_deref(), Some("72975"));
    }

    #[test]
    fn merge_orders_newest_first_and_caps() {
        let call = |direction, at: Option<&str>| CallRecord {
            direction,
            name: None,
            number: Some("+13025550142".into()),
            at: at.map(str::to_string),
        };
        let incoming = vec![call(CallDirection::Incoming, Some("2026-10-05T09:00:00"))];
        let outgoing = vec![
            call(CallDirection::Outgoing, Some("2026-10-05T10:00:00")),
            call(CallDirection::Outgoing, None),
        ];
        let missed = vec![call(CallDirection::Missed, Some("2026-10-04T23:00:00"))];
        let got = merge(vec![incoming, outgoing, missed], 3);
        let order: Vec<_> = got.iter().map(|c| (c.direction, c.at.clone())).collect();
        assert_eq!(
            order,
            vec![
                (CallDirection::Outgoing, Some("2026-10-05T10:00:00".into())),
                (CallDirection::Incoming, Some("2026-10-05T09:00:00".into())),
                (CallDirection::Missed, Some("2026-10-04T23:00:00".into())),
            ],
            "the untimed call falls off the end of the cap"
        );
    }
}
