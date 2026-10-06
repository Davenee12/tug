//! The QR code the Drop panel shows, as one SVG path the UI draws (no image, no v-html).

use qrcode::{Color, EcLevel, QrCode};
use serde::Serialize;

/// Mirrored in `src/types/protocol.ts` (`DropQr`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Qr {
    /// Modules per side (the UI adds the quiet zone around it).
    pub size: u32,
    /// Dark modules as an SVG path in module units: one `M x y h n v1 h-n z` run per row segment.
    pub path: String,
}

pub fn encode(text: &str) -> Option<Qr> {
    // Medium error correction: a phone camera reads it easily off a screen.
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    let size = code.width();
    let colors = code.to_colors();
    let mut path = String::new();
    for y in 0..size {
        let mut x = 0;
        while x < size {
            if colors[y * size + x] == Color::Dark {
                let start = x;
                while x < size && colors[y * size + x] == Color::Dark {
                    x += 1;
                }
                let n = x - start;
                path.push_str(&format!("M{start} {y}h{n}v1h-{n}z"));
            } else {
                x += 1;
            }
        }
    }
    Some(Qr {
        size: size as u32,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_drop_url() {
        let qr = encode("http://192.168.1.20:53211/#AAECAwQFBgcICQoLDA0ODw").unwrap();
        // 49 bytes at level M is a version 4 code: 33 modules per side.
        assert_eq!(qr.size, 33);
        // Starts with the top-left finder pattern's first row: 7 dark modules.
        assert!(qr.path.starts_with("M0 0h7v1h-7z"));
        assert!(qr.path.len() > 200);
    }
}
