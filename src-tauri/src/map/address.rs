//! Normalising message addresses so one person is one conversation, however the
//! phone formats their number.

use std::sync::OnceLock;

/// Regions that share the North American Numbering Plan (country code +1), as ISO 3166-1
/// alpha-2 codes. Only on a PC set to one of these is a bare 10-digit number taken as `+1…`.
/// Mirrors `NANP_REGIONS` in `src/lib/address.ts`.
const NANP_REGIONS: &[&str] = &[
    "US", "CA", "PR", "VI", "GU", "AS", "MP", "UM", "AG", "AI", "BB", "BM", "BS", "DM", "DO", "GD", "JM", "KN", "KY",
    "LC", "MS", "SX", "TC", "TT", "VC", "VG",
];

/// Whether `region` (an ISO 3166-1 alpha-2 code, any case) dials with +1.
pub fn is_nanp_region(region: &str) -> bool {
    NANP_REGIONS.iter().any(|r| r.eq_ignore_ascii_case(region.trim()))
}

/// The PC's home region (Settings › Time & language › Region), e.g. `"US"` or `"AU"`; None when
/// Windows doesn't say (or reports the "World" region). Read once: it only decides how to read
/// numbers typed without a country code.
pub fn pc_region() -> Option<&'static str> {
    // Unit tests run as if on a US PC, so their results don't depend on the machine's settings.
    if cfg!(test) {
        return Some("US");
    }
    static REGION: OnceLock<Option<String>> = OnceLock::new();
    REGION.get_or_init(read_region).as_deref()
}

#[cfg(windows)]
fn read_region() -> Option<String> {
    let mut buf = [0u16; 16];
    // SAFETY: the buffer is writable and the wrapper passes its length alongside it.
    let n = unsafe { windows::Win32::Globalization::GetUserDefaultGeoName(&mut buf) };
    // n counts the terminating NUL; 0 means failure.
    let len = usize::try_from(n).ok()?.checked_sub(1)?;
    let name = String::from_utf16_lossy(buf.get(..len)?);
    // Only two-letter country codes; "001" (World) and other numeric regions say nothing.
    (name.len() == 2 && name.chars().all(|c| c.is_ascii_alphabetic())).then(|| name.to_ascii_uppercase())
}

#[cfg(not(windows))]
fn read_region() -> Option<String> {
    None
}

/// Normalise for this PC's region (see [`normalize_in`]).
pub fn normalize(raw: &str) -> String {
    normalize_in(raw, pc_region())
}

/// `+1 (302) 555-0173`, `13025550173` → `+13025550173`, and on a PC in a +1 region a bare
/// `3025550173` too. Emails are lower-cased. Anything else keeps its digits (and a leading `+`)
/// as typed, and the iPhone resolves it as it would a number typed there: a local `07700 900123`
/// or `0491 570 006` is never turned into a US number.
///
/// Mirrors `normalizeAddress` in `src/lib/address.ts`; change both together.
pub fn normalize_in(raw: &str, region: Option<&str>) -> String {
    let raw = raw.trim();
    if raw.contains('@') {
        return raw.to_lowercase();
    }
    let plus = raw.starts_with('+');
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let nanp = region.is_some_and(is_nanp_region);
    match (plus, digits.len()) {
        (_, 0) => raw.to_string(),
        (true, _) => format!("+{digits}"),
        // A leading 0 is a trunk prefix (UK, AU, FR, IN, ...): never a North American number.
        (false, _) if digits.starts_with('0') => digits,
        // North American numbers without a country code, only where that's what people dial.
        (false, 10) if nanp => format!("+1{digits}"),
        (false, 11) if nanp && digits.starts_with('1') => format!("+{digits}"),
        (false, _) => digits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const US: Option<&str> = Some("US");

    #[test]
    fn normalises_nanp_variants_to_one_key() {
        for region in [US, Some("CA"), Some("pr")] {
            for raw in [
                "+1 (302) 555-0173",
                "13025550173",
                "3025550173",
                "302.555.0173",
                "+13025550173",
            ] {
                assert_eq!(normalize_in(raw, region), "+13025550173", "{raw} in {region:?}");
            }
        }
    }

    #[test]
    fn local_numbers_elsewhere_are_kept_as_typed() {
        // Fictional ranges: UK mobile (Ofcom 07700 900xxx), AU mobile (ACMA 0491 570 xxx),
        // FR mobile (ARCEP 06 39 98 xx xx); IN has no fiction range, so a placeholder.
        assert_eq!(normalize_in("07700 900123", Some("GB")), "07700900123");
        assert_eq!(normalize_in("0491 570 006", Some("AU")), "0491570006");
        assert_eq!(normalize_in("06 39 98 12 34", Some("FR")), "0639981234");
        assert_eq!(normalize_in("98765 43210", Some("IN")), "9876543210");
        // A leading 0 is never North American, even on a US PC or with no region known.
        assert_eq!(normalize_in("0491 570 006", US), "0491570006");
        assert_eq!(normalize_in("07700 900123", None), "07700900123");
        // A bare 10-digit number off a +1 PC (or with no region) stays as typed.
        assert_eq!(normalize_in("3025550173", None), "3025550173");
        assert_eq!(normalize_in("3025550173", Some("GB")), "3025550173");
        assert_eq!(normalize_in("13025550173", Some("IN")), "13025550173");
    }

    #[test]
    fn numbers_with_plus_keep_their_country_code() {
        for region in [US, Some("GB"), Some("AU"), None] {
            assert_eq!(normalize_in("+44 7700 900123", region), "+447700900123");
            assert_eq!(normalize_in("+61 491 570 006", region), "+61491570006");
            assert_eq!(normalize_in("+33 6 39 98 12 34", region), "+33639981234");
            assert_eq!(normalize_in("+91 98765 43210", region), "+919876543210");
            assert_eq!(normalize_in("+1 302 555 0173", region), "+13025550173");
        }
    }

    #[test]
    fn keeps_international_short_codes_and_emails() {
        assert_eq!(normalize_in("+44 20 7946 0958", US), "+442079460958");
        assert_eq!(normalize_in("72975", US), "72975");
        assert_eq!(normalize_in(" Zoe@Example.com ", US), "zoe@example.com");
        assert_eq!(normalize_in("Unknown", US), "Unknown");
    }

    #[test]
    fn nanp_regions() {
        assert!(is_nanp_region("US") && is_nanp_region("ca") && is_nanp_region("JM"));
        assert!(!is_nanp_region("GB") && !is_nanp_region("AU") && !is_nanp_region("001"));
    }
}
