//! A picture on the Windows clipboard (`CF_DIB`: a BITMAPINFOHEADER, maybe colour masks or a
//! colour table, then rows of pixels) turned into a PNG the phone's browser can show and save.
//! Pure and bounds-checked: the bytes come from whatever app put them on the clipboard.
//!
//! Handles what screenshots and apps actually put there: 24- and 32-bit pixels, uncompressed or
//! with colour masks, bottom-up or top-down. A 32-bit picture keeps its transparency when its
//! alpha channel is really used (some apps leave it all zero, which would make it invisible).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DibError {
    /// Shorter than its header says, or a header that makes no sense.
    Malformed,
    /// A kind of bitmap this doesn't read (palettes, compressed, 16-bit).
    Unsupported,
    /// More pixels than `max_pixels`.
    TooBig,
    Encode,
}

impl fmt::Display for DibError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DibError::Malformed => "malformed bitmap",
            DibError::Unsupported => "unsupported bitmap",
            DibError::TooBig => "bitmap too big",
            DibError::Encode => "PNG encoding failed",
        })
    }
}

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    u32_at(b, at).map(|v| v as i32)
}

/// One colour channel described by a bit mask (`0x00ff0000` for red in the usual layout).
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    max: u32,
}

impl Channel {
    fn new(mask: u32) -> Channel {
        if mask == 0 {
            return Channel {
                mask: 0,
                shift: 0,
                max: 0,
            };
        }
        let shift = mask.trailing_zeros();
        Channel {
            mask,
            shift,
            max: mask >> shift,
        }
    }

    /// The channel's value scaled to 0–255. In u64: a mask can be up to 32 bits wide, and
    /// `v * 255` overflows u32 for anything over 24 (a panic in debug, a wrong colour in release).
    fn get(self, px: u32) -> u8 {
        if self.max == 0 {
            return 0;
        }
        let v = u64::from((px & self.mask) >> self.shift);
        let max = u64::from(self.max);
        // At most 255: v never exceeds max.
        ((v * 255 + max / 2) / max) as u8
    }
}

/// A decoded picture: `width × height` pixels, RGB or RGBA, top row first.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    pub pixels: Vec<u8>,
}

/// Read a `CF_DIB`. Refuses anything over `max_pixels` before allocating for it.
pub fn decode(dib: &[u8], max_pixels: u64) -> Result<Decoded, DibError> {
    let header = u32_at(dib, 0).ok_or(DibError::Malformed)? as usize;
    // BITMAPINFOHEADER (40) and its V2–V5 successors; the old 12-byte core header isn't used for
    // the clipboard.
    if !matches!(header, 40 | 52 | 56 | 108 | 124) {
        return Err(DibError::Unsupported);
    }
    let width = i32_at(dib, 4).ok_or(DibError::Malformed)?;
    let height = i32_at(dib, 8).ok_or(DibError::Malformed)?;
    let bits = u16_at(dib, 14).ok_or(DibError::Malformed)?;
    let compression = u32_at(dib, 16).ok_or(DibError::Malformed)?;
    let colours_used = u32_at(dib, 32).ok_or(DibError::Malformed)? as usize;
    if width <= 0 || height == 0 || height == i32::MIN {
        return Err(DibError::Malformed);
    }
    let (w, h) = (width as u64, height.unsigned_abs() as u64);
    if w * h > max_pixels {
        return Err(DibError::TooBig);
    }
    let top_down = height < 0;
    if bits != 24 && bits != 32 {
        return Err(DibError::Unsupported);
    }
    // Colour masks: inside a V2+ header, or right after a plain 40-byte one.
    let (masks, masks_after_header) = match compression {
        BI_RGB => (None, 0),
        BI_BITFIELDS | BI_ALPHABITFIELDS if bits == 32 => {
            let at = if header >= 52 { 40 } else { header };
            let count = if compression == BI_ALPHABITFIELDS || header >= 56 {
                4
            } else {
                3
            };
            let m = |i: usize| u32_at(dib, at + i * 4).ok_or(DibError::Malformed);
            let alpha = if count == 4 { m(3)? } else { 0 };
            let after = if header == 40 { count * 4 } else { 0 };
            (Some([m(0)?, m(1)?, m(2)?, alpha]), after)
        }
        _ => return Err(DibError::Unsupported),
    };
    // A colour table can follow even for 24/32-bit pixels (as a hint for palette displays).
    if colours_used > 256 {
        return Err(DibError::Malformed);
    }
    let start = header + masks_after_header + colours_used * 4;
    let stride = ((w * bits as u64).div_ceil(32) * 4) as usize;
    let need = start as u64 + stride as u64 * h;
    if (dib.len() as u64) < need {
        return Err(DibError::Malformed);
    }
    let (w, h) = (w as usize, h as usize);
    let bytes_pp = bits as usize / 8;
    let [r, g, b, a] = masks
        .unwrap_or([0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0])
        .map(Channel::new);
    // In 32-bit BI_RGB the fourth byte is officially unused, but apps do put alpha there.
    let alpha_byte = bits == 32 && masks.is_none();
    let read_px = |row: &[u8], x: usize| -> [u8; 4] {
        let p = &row[x * bytes_pp..x * bytes_pp + bytes_pp];
        if bits == 24 {
            return [p[2], p[1], p[0], 255];
        }
        let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let alpha = if alpha_byte { p[3] } else { a.get(v) };
        [r.get(v), g.get(v), b.get(v), alpha]
    };
    // Is there real transparency? All-zero or all-opaque alpha means none.
    let has_alpha_channel = alpha_byte || a.mask != 0;
    let mut any_zero = false;
    let mut any_nonzero = false;
    let mut partly = false;
    if has_alpha_channel {
        for y in 0..h {
            let row = &dib[start + y * stride..start + (y + 1) * stride];
            for x in 0..w {
                match read_px(row, x)[3] {
                    0 => any_zero = true,
                    255 => any_nonzero = true,
                    _ => {
                        any_nonzero = true;
                        partly = true;
                    }
                }
            }
        }
    }
    let alpha = has_alpha_channel && any_nonzero && (any_zero || partly);
    let channels = if alpha { 4 } else { 3 };
    let mut pixels = Vec::with_capacity(w * h * channels);
    for out_y in 0..h {
        let y = if top_down { out_y } else { h - 1 - out_y };
        let row = &dib[start + y * stride..start + (y + 1) * stride];
        for x in 0..w {
            let px = read_px(row, x);
            pixels.extend_from_slice(&px[..channels]);
        }
    }
    Ok(Decoded {
        width: w as u32,
        height: h as u32,
        alpha,
        pixels,
    })
}

/// A `CF_DIB` as PNG bytes.
pub fn to_png(dib: &[u8], max_pixels: u64) -> Result<Vec<u8>, DibError> {
    let pic = decode(dib, max_pixels)?;
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, pic.width, pic.height);
        enc.set_color(if pic.alpha {
            png::ColorType::Rgba
        } else {
            png::ColorType::Rgb
        });
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(|_| DibError::Encode)?;
        writer.write_image_data(&pic.pixels).map_err(|_| DibError::Encode)?;
        writer.finish().map_err(|_| DibError::Encode)?;
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn header(size: u32, w: i32, h: i32, bits: u16, compression: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&size.to_le_bytes());
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(&compression.to_le_bytes());
        b.extend_from_slice(&[0u8; 20]); // size image, resolution, colours used/important
        b
    }

    /// A 24-bit DIB whose pixel (x, y) (top row first) is (x*40, y*40, 200): rows padded to 4 bytes.
    pub(crate) fn dib_24(w: i32, h: i32, top_down: bool) -> Vec<u8> {
        let mut b = header(40, w, if top_down { -h } else { h }, 24, BI_RGB);
        let stride = ((w as usize * 24).div_ceil(32)) * 4;
        for row in 0..h {
            let y = if top_down { row } else { h - 1 - row };
            let mut line = Vec::new();
            for x in 0..w {
                line.extend_from_slice(&[200, (y * 40) as u8, (x * 40) as u8]); // B, G, R
            }
            line.resize(stride, 0xee);
            b.extend_from_slice(&line);
        }
        b
    }

    #[test]
    fn reads_24_bit_bottom_up_and_top_down_alike() {
        for top_down in [false, true] {
            let pic = decode(&dib_24(3, 2, top_down), 100).unwrap();
            assert_eq!((pic.width, pic.height, pic.alpha), (3, 2, false));
            // Top-left, then bottom-right.
            assert_eq!(&pic.pixels[0..3], &[0, 0, 200]);
            assert_eq!(&pic.pixels[15..18], &[80, 40, 200]);
        }
    }

    #[test]
    fn reads_32_bit_with_masks_and_keeps_only_real_transparency() {
        // BI_BITFIELDS with the masks after a 40-byte header: BGRX.
        let mut b = header(40, 2, 1, 32, BI_BITFIELDS);
        for m in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff] {
            b.extend_from_slice(&m.to_le_bytes());
        }
        b.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        let pic = decode(&b, 100).unwrap();
        assert!(!pic.alpha);
        assert_eq!(pic.pixels, vec![3, 2, 1, 6, 5, 4]);

        // BI_RGB 32-bit with an all-zero fourth byte: opaque, not invisible.
        let mut b = header(40, 1, 1, 32, BI_RGB);
        b.extend_from_slice(&[10, 20, 30, 0]);
        let pic = decode(&b, 100).unwrap();
        assert!(!pic.alpha);
        assert_eq!(pic.pixels, vec![30, 20, 10]);

        // A real alpha channel (some clear, some solid) is kept.
        let mut b = header(40, 2, 1, 32, BI_RGB);
        b.extend_from_slice(&[10, 20, 30, 255, 0, 0, 0, 0]);
        let pic = decode(&b, 100).unwrap();
        assert!(pic.alpha);
        assert_eq!(pic.pixels, vec![30, 20, 10, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn reads_a_v5_header_with_its_own_masks() {
        let mut b = header(124, 1, 1, 32, BI_BITFIELDS);
        // Masks live inside the V5 header at offset 40: R, G, B, A.
        b.extend_from_slice(&0x00ff_0000u32.to_le_bytes());
        b.extend_from_slice(&0x0000_ff00u32.to_le_bytes());
        b.extend_from_slice(&0x0000_00ffu32.to_le_bytes());
        b.extend_from_slice(&0xff00_0000u32.to_le_bytes());
        b.resize(124, 0);
        b.extend_from_slice(&[7, 8, 9, 255]);
        let pic = decode(&b, 100).unwrap();
        assert!(!pic.alpha, "fully opaque needs no alpha channel");
        assert_eq!(pic.pixels, vec![9, 8, 7]);
    }

    #[test]
    fn refuses_short_odd_or_huge_bitmaps() {
        let good = dib_24(4, 4, false);
        assert_eq!(decode(&good[..good.len() - 1], 100).err(), Some(DibError::Malformed));
        assert_eq!(decode(&[], 100).err(), Some(DibError::Malformed));
        assert_eq!(decode(&good, 15).err(), Some(DibError::TooBig));
        assert_eq!(
            decode(&header(40, 1, 1, 8, BI_RGB), 100).err(),
            Some(DibError::Unsupported)
        );
        assert_eq!(decode(&header(40, 1, 1, 24, 4), 100).err(), Some(DibError::Unsupported));
        assert_eq!(decode(&header(12, 1, 1, 24, 0), 100).err(), Some(DibError::Unsupported));
        assert_eq!(decode(&header(40, 0, 1, 24, 0), 100).err(), Some(DibError::Malformed));
        assert_eq!(
            decode(&header(40, 1, i32::MIN, 24, 0), 100).err(),
            Some(DibError::Malformed)
        );
        // Huge dimensions are refused before anything is allocated for them.
        assert_eq!(
            decode(&header(40, i32::MAX, i32::MAX, 32, 0), 40_000_000).err(),
            Some(DibError::TooBig)
        );
    }

    #[test]
    fn wide_colour_masks_scale_without_overflowing() {
        // Masks a bitmap may legally declare: one channel 32 bits wide, another 25 bits wide (both
        // overflowed `v * 255` in u32), and blue in the top byte.
        let mut b = header(40, 3, 1, 32, BI_BITFIELDS);
        for m in [0xffff_ffffu32, 0x01ff_ffff, 0xff00_0000] {
            b.extend_from_slice(&m.to_le_bytes());
        }
        for px in [0xffff_ffffu32, 0, 0x8000_0000] {
            b.extend_from_slice(&px.to_le_bytes());
        }
        let pic = decode(&b, 100).unwrap();
        assert!(!pic.alpha);
        // Full scale, zero, and half scale (rounded) of each channel's own width.
        assert_eq!(pic.pixels, vec![255, 255, 255, 0, 0, 0, 128, 0, 128]);
    }

    #[test]
    fn encodes_a_png_that_decodes_back_to_the_same_pixels() {
        let png_bytes = to_png(&dib_24(3, 2, false), 100).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height, info.color_type), (3, 2, png::ColorType::Rgb));
        assert_eq!(&buf[..3], &[0, 0, 200]);
    }
}
