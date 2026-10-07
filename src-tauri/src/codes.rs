//! One-time codes, for the bridge's `get_latest_code` and `tug code`: a port of `findCode` in
//! `src/lib/codes.ts`. Both are tested against the same cases (`src/lib/codes.cases.json`), so
//! the Feed's Copy code and the `tug` command always agree on what a code is.
//!
//! Anchored on wording ("code", "verification", "OTP", …) so phone numbers, prices, times and
//! order numbers aren't mistaken for codes.

use std::sync::LazyLock;

use fancy_regex::Regex;

/// Words that mean "this message carries a code".
static CUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(code|codes|passcode|pass code|verification|verify|otp|one[- ]time|2fa|two[- ]factor|security|login|log in|sign[- ]in|pin|authenticat[A-Za-z0-9_]*|confirm[A-Za-z0-9_]*|código|codigo)\b",
    )
    .expect("CUE compiles")
});

/// 4–8 digits, optionally split once by a dash or space (482-913), or a provider prefix like
/// G-482913. Not part of a longer number, a price, a time or a phone number.
static CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?<![A-Za-z0-9_$£€#.:/-])(?:[A-Z]{1,3}-)?([0-9]{3,4}[- ][0-9]{3,4}|[0-9]{4,8})(?![A-Za-z0-9_/]|[.:,-][0-9])",
    )
    .expect("CANDIDATE compiles")
});

/// Phone-number shapes to rule out ("302-555-0173", "(302) 555-0173", "+1 302…").
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\+?[0-9]{1,2}[\s.-]?)?\(?[0-9]{3}\)?[\s.-][0-9]{3}[\s.-][0-9]{4}").expect("PHONE compiles")
});

static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(19|20)[0-9][0-9]$").expect("YEAR compiles"));
static CODE_OR_PIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)code|pin").expect("compiles"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundCode {
    /// What to paste: digits only.
    pub code: String,
    /// As written in the text.
    pub shown: String,
}

/// The one code in `text`, or `None` (no cue word, no candidate, or several different numbers).
pub fn find_code(text: &str) -> Option<FoundCode> {
    if text.is_empty() || !CUE.is_match(text).unwrap_or(false) {
        return None;
    }
    let phones: Vec<(usize, usize)> = PHONE.find_iter(text).flatten().map(|m| (m.start(), m.end())).collect();
    let in_phone = |i: usize| phones.iter().any(|&(a, b)| i >= a && i < b);
    let mut found: Vec<FoundCode> = Vec::new();
    for caps in CANDIDATE.captures_iter(text).flatten() {
        let (Some(whole), Some(num)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        if in_phone(whole.start()) {
            continue;
        }
        let digits: String = num.as_str().chars().filter(char::is_ascii_digit).collect();
        if digits.len() < 4 || digits.len() > 8 {
            continue;
        }
        // A bare year in prose ("since 2019") isn't a code.
        if digits.len() == 4 && YEAR.is_match(&digits).unwrap_or(false) && !CODE_OR_PIN.is_match(text).unwrap_or(false)
        {
            continue;
        }
        found.push(FoundCode {
            code: digits,
            shown: whole.as_str().to_string(),
        });
    }
    // Several numbers and nothing to tell them apart: don't guess.
    let first = found.first()?;
    found.iter().all(|f| f.code == first.code).then(|| first.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_the_shared_cases() {
        let raw = include_str!("../../src/lib/codes.cases.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let cases = v["cases"].as_array().unwrap();
        assert!(cases.len() >= 20);
        for c in cases {
            let text = c["text"].as_str().unwrap();
            let found = find_code(text);
            assert_eq!(found.as_ref().map(|f| f.code.as_str()), c["code"].as_str(), "{text}");
            if let Some(shown) = c.get("shown").and_then(|s| s.as_str()) {
                assert_eq!(found.unwrap().shown, shown, "{text}");
            }
        }
    }
}
