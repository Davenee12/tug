//! Times on the bridge: everything travels as UTC ISO 8601 (`2026-10-07T09:30:00Z`), and tools
//! take a `since` that people and AI tools actually write ("2h", "30 minutes", "1d", an ISO
//! time or date). Pure, no clock of its own: callers pass `now_ms`.

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Unix milliseconds as `2026-10-07T09:30:00Z` (seconds precision).
pub fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn num(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `2026-10-07`, `2026-10-07T09:30`, `2026-10-07T09:30:00Z`, `2026-10-07 09:30:00.123+02:00`.
/// A date or time with no offset is taken as UTC.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = match s.find(['T', 't', ' ']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let mut parts = date.split('-');
    let (y, m, d) = (num(parts.next()?)?, num(parts.next()?)?, num(parts.next()?)?);
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1970..=9999).contains(&y) {
        return None;
    }
    let mut ms = days_from_civil(y, m, d) * DAY;
    if rest.is_empty() {
        return Some(ms);
    }
    // Split off the offset: Z, +hh:mm or -hh:mm.
    let (clock, offset_ms) = if let Some(c) = rest.strip_suffix(['Z', 'z']) {
        (c, 0)
    } else if let Some(i) = rest.rfind(['+', '-']) {
        let (c, off) = rest.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let mut hm = off[1..].split(':');
        let oh = num(hm.next()?)?;
        let om = hm.next().map(num).unwrap_or(Some(0))?;
        (c, sign * (oh * HOUR + om * MINUTE))
    } else {
        (rest, 0)
    };
    let mut hms = clock.split(':');
    let h = num(hms.next()?)?;
    let mi = num(hms.next()?)?;
    let (sec, frac) = match hms.next() {
        Some(sec) => match sec.split_once('.') {
            Some((s, f)) => (num(s)?, f),
            None => (num(sec)?, ""),
        },
        None => (0, ""),
    };
    if hms.next().is_some() || h > 23 || mi > 59 || sec > 60 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frac_ms = frac.get(..frac.len().min(3)).map_or(0, |f| {
        let v = num(f).unwrap_or(0);
        v * 10_i64.pow(3 - f.len() as u32)
    });
    ms += h * HOUR + mi * MINUTE + sec * 1000 + frac_ms;
    Some(ms - offset_ms)
}

/// A relative span: `45s`, `30m`, `30 min`, `2h`, `2 hours`, `1d`, `1 week`.
pub fn parse_span(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = s.split_at(split);
    let n = num(n)?;
    let unit = unit.trim().trim_end_matches(" ago").trim();
    let per = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => 1000,
        "m" | "min" | "mins" | "minute" | "minutes" => MINUTE,
        "h" | "hr" | "hrs" | "hour" | "hours" => HOUR,
        "d" | "day" | "days" => DAY,
        "w" | "wk" | "week" | "weeks" => 7 * DAY,
        _ => return None,
    };
    n.checked_mul(per)
}

/// The start of a `since` window, as Unix ms: a span back from now ("2h") or an ISO time/date.
/// Never in the future, never before 1970.
pub fn parse_since(s: &str, now_ms: i64) -> Result<i64, String> {
    let at = match parse_span(s) {
        Some(span) => now_ms.saturating_sub(span),
        None => parse_iso(s).ok_or_else(|| {
            format!(
                "`since` should be like \"30m\", \"2h\", \"1d\" or an ISO time such as 2026-10-07T09:00:00Z, not {s:?}"
            )
        })?,
    };
    if at > now_ms {
        return Err("`since` is in the future".into());
    }
    Ok(at.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_365_400_000; // 2026-10-07T09:30:00Z

    #[test]
    fn formats_utc() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(NOW), "2026-10-07T09:30:00Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn parses_iso_round_trip() {
        assert_eq!(parse_iso("2026-10-07T09:30:00Z"), Some(NOW));
        assert_eq!(parse_iso("2026-10-07T09:30"), Some(NOW));
        assert_eq!(parse_iso("2026-10-07 11:30:00+02:00"), Some(NOW));
        assert_eq!(parse_iso("2026-10-07T04:30:00.000-05:00"), Some(NOW));
        assert_eq!(parse_iso("2026-10-07T09:30:00.5Z"), Some(NOW + 500));
        assert_eq!(parse_iso("2026-10-07"), Some(NOW - 9 * HOUR - 30 * MINUTE));
        for d in [0, NOW, 4_102_444_800_000] {
            assert_eq!(parse_iso(&iso_utc(d)), Some(d));
        }
    }

    #[test]
    fn rejects_bad_iso() {
        for s in [
            "",
            "yesterday",
            "2026-13-01",
            "2026-10-07T25:00",
            "2026-10",
            "10/07/2026",
            "2026-10-07Tab:cd",
        ] {
            assert_eq!(parse_iso(s), None, "{s}");
        }
    }

    #[test]
    fn parses_spans() {
        assert_eq!(parse_span("30m"), Some(30 * MINUTE));
        assert_eq!(parse_span("30 min"), Some(30 * MINUTE));
        assert_eq!(parse_span("2h"), Some(2 * HOUR));
        assert_eq!(parse_span("2 hours ago"), Some(2 * HOUR));
        assert_eq!(parse_span("1D"), Some(DAY));
        assert_eq!(parse_span("1 week"), Some(7 * DAY));
        assert_eq!(parse_span("45s"), Some(45_000));
        assert_eq!(parse_span("h"), None);
        assert_eq!(parse_span("5 fortnights"), None);
        assert_eq!(parse_span("99999999999999999999d"), None);
    }

    #[test]
    fn since_is_bounded() {
        assert_eq!(parse_since("1h", NOW), Ok(NOW - HOUR));
        assert_eq!(parse_since("2026-10-07T09:00:00Z", NOW), Ok(NOW - 30 * MINUTE));
        assert!(parse_since("2027-01-01", NOW).is_err());
        assert!(parse_since("whenever", NOW).is_err());
        assert_eq!(parse_since("100000w", NOW), Ok(0));
    }
}
