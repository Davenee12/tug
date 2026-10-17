//! Native SMS bodies (MAP `CHARSET:native`): the message content is the SMS PDU written in hex,
//! one PDU per line for a long text sent in parts (3GPP TS 23.040). tug asks for UTF-8, so this
//! only matters if a phone ignores that; then the text is decoded here instead of being lost.

/// The text of one or more hex PDUs (one per line), or None if they don't parse.
pub fn decode_hex_pdus(content: &str) -> Option<String> {
    let mut out = String::new();
    let mut any = false;
    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let bytes = hex(line)?;
        // With or without the service-centre address in front (phones differ).
        out.push_str(&decode_pdu(&bytes, true).or_else(|| decode_pdu(&bytes, false))?);
        any = true;
    }
    any.then_some(out)
}

/// Whether `content` is hex digits only (allowing line breaks): a native PDU, not text.
pub fn looks_like_hex(content: &str) -> bool {
    let mut digits = 0;
    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !line.len().is_multiple_of(2) || !line.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
        digits += line.len();
    }
    digits >= 2
}

fn hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

enum Alphabet {
    Gsm7,
    Octets,
    Ucs2,
}

/// SMS-DELIVER (from the network) or SMS-SUBMIT (to it): the user data as text.
fn decode_pdu(b: &[u8], with_smsc: bool) -> Option<String> {
    let mut i = 0;
    if with_smsc {
        i += 1 + *b.first()? as usize;
    }
    let first = *b.get(i)?;
    i += 1;
    let udhi = first & 0x40 != 0;
    match first & 0x03 {
        // SMS-DELIVER: originating address, PID, DCS, timestamp.
        0x00 => {
            i += address_len(b, i)?;
            let dcs = *b.get(i + 1)?;
            i += 2 + 7;
            user_data(b.get(i..)?, dcs, udhi)
        }
        // SMS-SUBMIT: reference, destination address, PID, DCS, validity period.
        0x01 => {
            i += 1;
            i += address_len(b, i)?;
            let dcs = *b.get(i + 1)?;
            i += 2;
            i += match (first >> 3) & 0x03 {
                0 => 0,
                2 => 1,
                _ => 7,
            };
            user_data(b.get(i..)?, dcs, udhi)
        }
        _ => None,
    }
}

/// Bytes taken by an address field: length (in digits), type, then the digits two per byte.
fn address_len(b: &[u8], at: usize) -> Option<usize> {
    let digits = *b.get(at)? as usize;
    Some(2 + digits.div_ceil(2))
}

fn alphabet(dcs: u8) -> Alphabet {
    let bits = match dcs & 0xF0 {
        0xF0 => {
            return if dcs & 0x04 != 0 {
                Alphabet::Octets
            } else {
                Alphabet::Gsm7
            };
        }
        0xC0 | 0xD0 => return Alphabet::Gsm7,
        0xE0 => return Alphabet::Ucs2,
        _ => (dcs >> 2) & 0x03,
    };
    match bits {
        1 => Alphabet::Octets,
        2 => Alphabet::Ucs2,
        _ => Alphabet::Gsm7,
    }
}

/// UDL then UD. A user-data header (UDHI, e.g. the parts of a long text) is skipped. The user
/// data must end the PDU exactly: that's what tells a PDU with a service-centre address in front
/// from one without.
fn user_data(b: &[u8], dcs: u8, udhi: bool) -> Option<String> {
    let udl = *b.first()? as usize;
    let ud = &b[1..];
    match alphabet(dcs) {
        Alphabet::Gsm7 => {
            let needed = (udl * 7).div_ceil(8);
            if ud.len() != needed {
                return None;
            }
            let septets = unpack_septets(ud, udl);
            let skip = if udhi {
                let header = 1 + *ud.first()? as usize;
                (header * 8).div_ceil(7)
            } else {
                0
            };
            Some(gsm7_to_string(septets.get(skip..)?))
        }
        Alphabet::Octets | Alphabet::Ucs2 => {
            if ud.len() != udl {
                return None;
            }
            let data = ud;
            let data = if udhi {
                data.get(1 + *data.first()? as usize..)?
            } else {
                data
            };
            if matches!(alphabet(dcs), Alphabet::Ucs2) {
                let units: Vec<u16> = data.as_chunks::<2>().0.iter().map(|&p| u16::from_be_bytes(p)).collect();
                Some(String::from_utf16_lossy(&units))
            } else {
                Some(data.iter().map(|&c| super::charset::cp1252(c)).collect())
            }
        }
    }
}

fn unpack_septets(packed: &[u8], count: usize) -> Vec<u8> {
    (0..count)
        .map(|n| {
            let bit = n * 7;
            let (byte, shift) = (bit / 8, bit % 8);
            let lo = packed.get(byte).copied().unwrap_or(0) as u16;
            let hi = packed.get(byte + 1).copied().unwrap_or(0) as u16;
            (((hi << 8 | lo) >> shift) & 0x7F) as u8
        })
        .collect()
}

/// GSM 03.38 default alphabet, with its extension table behind the 0x1B escape.
const GSM7: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å', 'Δ', '_', 'Φ', 'Γ', 'Λ', 'Ω',
    'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', '\u{1B}', 'Æ', 'æ', 'ß', 'É', ' ', '!', '"', '#', '¤', '%', '&', '\'', '(', ')', '*', '+',
    ',', '-', '.', '/', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', '¡', 'A', 'B',
    'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y',
    'Z', 'Ä', 'Ö', 'Ñ', 'Ü', '§', '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à',
];

fn gsm7_extension(c: u8) -> Option<char> {
    Some(match c {
        0x0A => '\u{0C}',
        0x14 => '^',
        0x28 => '{',
        0x29 => '}',
        0x2F => '\\',
        0x3C => '[',
        0x3D => '~',
        0x3E => ']',
        0x40 => '|',
        0x65 => '€',
        _ => return None,
    })
}

fn gsm7_to_string(septets: &[u8]) -> String {
    let mut out = String::with_capacity(septets.len());
    let mut iter = septets.iter();
    while let Some(&c) = iter.next() {
        if c == 0x1B {
            if let Some(&next) = iter.next() {
                out.push(gsm7_extension(next).unwrap_or(GSM7[next as usize & 0x7F]));
            }
            continue;
        }
        out.push(GSM7[c as usize & 0x7F]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_gsm7_deliver_pdu_with_service_centre() {
        // The classic example from 3GPP-style tutorials: "hellohello" from +27381000015.
        let pdu = "07917283010010F5040BC87238880900F10000993092516195800AE8329BFD4697D9EC37";
        assert_eq!(decode_hex_pdus(pdu).as_deref(), Some("hellohello"));
        assert!(looks_like_hex(pdu));
    }

    #[test]
    fn decodes_without_the_service_centre_too() {
        let pdu = "040BC87238880900F10000993092516195800AE8329BFD4697D9EC37";
        assert_eq!(decode_hex_pdus(pdu).as_deref(), Some("hellohello"));
    }

    #[test]
    fn decodes_ucs2_with_emoji() {
        // Empty service-centre address, SMS-DELIVER, sender "1234", DCS 0x08 (UCS-2), "hi 😀".
        let text: Vec<u8> = "hi 😀".encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let mut pdu = vec![0x00, 0x04, 0x04, 0x81, 0x21, 0x43, 0x00, 0x08];
        pdu.extend([0x62, 0x01, 0x01, 0x12, 0x00, 0x00, 0x00]);
        pdu.push(text.len() as u8);
        pdu.extend(&text);
        let hex: String = pdu.iter().map(|b| format!("{b:02X}")).collect();
        assert_eq!(decode_hex_pdus(&hex).as_deref(), Some("hi 😀"));
    }

    #[test]
    fn skips_the_header_of_a_multipart_text_and_joins_the_parts() {
        // Two parts of one long text, UDHI set, 6-byte concatenation header, GSM 7-bit.
        let part = |text: &str, n: u8| {
            let header = [0x05, 0x00, 0x03, 0x2A, 0x02, n];
            // Header is 6 bytes = 48 bits; pad to 49 (7 septets), then the text.
            let mut septets: Vec<u8> = vec![0; 7];
            septets.extend(text.bytes()); // ASCII letters map 1:1 in GSM 7-bit
            let mut packed = vec![0u8; (septets.len() * 7).div_ceil(8)];
            for (k, s) in septets.iter().enumerate() {
                let bit = k * 7;
                packed[bit / 8] |= s << (bit % 8);
                if bit % 8 > 1 {
                    packed[bit / 8 + 1] |= s >> (8 - bit % 8);
                }
            }
            packed[..6].copy_from_slice(&header);
            let mut pdu = vec![0x00, 0x44, 0x04, 0x81, 0x21, 0x43, 0x00, 0x00];
            pdu.extend([0x62, 0x01, 0x01, 0x12, 0x00, 0x00, 0x00]);
            pdu.push(septets.len() as u8);
            pdu.extend(packed);
            pdu.iter().map(|b| format!("{b:02X}")).collect::<String>()
        };
        let both = format!("{}\r\n{}", part("Hello ", 1), part("there", 2));
        assert_eq!(decode_hex_pdus(&both).as_deref(), Some("Hello there"));
    }

    #[test]
    fn gsm7_extension_characters() {
        assert_eq!(gsm7_to_string(&[0x1B, 0x65, 0x35]), "€5");
        assert_eq!(gsm7_to_string(&[0x00, 0x01, 0x02]), "@£$");
    }

    #[test]
    fn text_is_not_mistaken_for_hex() {
        assert!(!looks_like_hex("cafe babe"));
        assert!(!looks_like_hex("abc"));
        assert!(!looks_like_hex(""));
        assert!(decode_hex_pdus("ZZ").is_none());
        assert!(decode_hex_pdus("00").is_none(), "too short to be a PDU");
    }
}
