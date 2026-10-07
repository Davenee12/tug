//! One-time codes, for the bridge's `get_latest_code` and `tug code`: a port of `findCode` in
//! `src/lib/codes.ts`. Both are tested against the same cases (`src/lib/codes.cases.json`), so
//! the Feed's Copy code and the `tug` command always agree on what a code is.
//!
//! Anchored on wording ("code", "verification", "OTP", …) so phone numbers, prices, times and
//! order numbers aren't mistaken for codes.

use std::sync::LazyLock;

use fancy_regex::Regex;

/// Words that mean "this message carries a code", in English and the iPhone's other major
/// languages, as whole words (Unicode-aware edges). Mirrors `CUE_WORDS` in `src/lib/codes.ts`.
const CUE_WORDS: &[&str] = &[
    // en
    "code",
    "codes",
    "passcode",
    "pass code",
    "verification",
    "verify",
    "otp",
    "one[- ]time",
    "2fa",
    "two[- ]factor",
    "security",
    "login",
    "log in",
    "sign[- ]in",
    "pin",
    r"authenticat\p{L}*",
    r"confirm\p{L}*",
    // es, pt
    "código",
    "codigo",
    "verificación",
    "verificação",
    "clave",
    "senha", // fr
    "vérification",
    "vérifier", // de
    "bestätigungscode",
    "sicherheitscode",
    "anmeldecode",
    "verifizierungscode",
    "aktivierungscode",
    "freischaltcode",
    // it
    "codice",
    "verifica", // nl
    "verificatiecode",
    "beveiligingscode",
    "inlogcode",
    "bevestigingscode", // sv, no, da, pl, tr, ru
    "kod",
    "koden",
    "kode",
    "kodu",
    "engångskod",
    "doğrulama",
    "код",
    "кода",
];

static CUE: LazyLock<Regex> = LazyLock::new(|| {
    let w = r"[\p{L}\p{N}_]";
    Regex::new(&format!("(?i)(?<!{w})(?:{})(?!{w})", CUE_WORDS.join("|"))).expect("CUE compiles")
});

/// Scripts without spaces between words: matched anywhere (ja, zh, ko).
static CUE_ANYWHERE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"コード|验证码|驗證碼|校验码|认证码|인증\s?번호|인증\s?코드").expect("compiles"));

/// A short-code sender (3–6 digits, as banks and services text from).
static SHORT_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{3,6}$").expect("compiles"));

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
    find_code_from(text, None)
}

/// [`find_code`], also accepting a text from a short-code `sender` ("72975") that holds exactly
/// one 4–8 digit number, whatever language it's in.
pub fn find_code_from(text: &str, sender: Option<&str>) -> Option<FoundCode> {
    if text.is_empty() {
        return None;
    }
    let cued = CUE.is_match(text).unwrap_or(false) || CUE_ANYWHERE.is_match(text).unwrap_or(false);
    let short_code = sender.is_some_and(|s| {
        // iOS wraps numbers in notification titles in invisible direction marks.
        let s: String = crate::text::strip_invisible(s)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        SHORT_CODE.is_match(&s).unwrap_or(false)
    });
    if !cued && !short_code {
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
    // Several numbers and nothing to tell them apart: don't guess. Without a cue word, only a text
    // with exactly one number counts.
    if !cued && found.len() != 1 {
        return None;
    }
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
            let found = find_code_from(text, c.get("sender").and_then(|s| s.as_str()));
            assert_eq!(found.as_ref().map(|f| f.code.as_str()), c["code"].as_str(), "{text}");
            if let Some(shown) = c.get("shown").and_then(|s| s.as_str()) {
                assert_eq!(found.unwrap().shown, shown, "{text}");
            }
        }
    }
}
