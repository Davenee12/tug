//! A small JPEG preview of a stored photo, made with Windows Imaging Component, because the web
//! view can't show an iPhone's HEIC photos. WIC decodes JPEG, PNG and GIF on every Windows. HEIC
//! needs two free Microsoft Store extensions: "HEIF Image Extensions" (the container: without it
//! WIC finds no decoder for the file) and "HEVC Video Extensions from Device Manufacturer" (the
//! pictures inside: without it the decoder opens the file but can't decode a frame). Which one is
//! missing decides what the hint asks for. A failure only counts as a missing extension when the
//! file really is HEIF (`ftypheic`, `ftypheix` or `ftypmif1`); anything else is just a photo
//! Windows couldn't read.
//!
//! Thin on purpose: decode, turn upright (the photo's orientation tag), scale to fit, encode as
//! JPEG, all in memory. Runs on a thread of its own with a time limit (a codec can hang), never on
//! the async runtime.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use windows::core::{w, HRESULT, HSTRING};
use windows::Win32::Foundation::{GENERIC_READ, WINCODEC_ERR_COMPONENTNOTFOUND, WINCODEC_ERR_UNKNOWNIMAGEFORMAT};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, IWICBitmapSource,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant,
    WICBitmapPaletteTypeCustom, WICBitmapTransformFlipHorizontal, WICBitmapTransformFlipVertical,
    WICBitmapTransformOptions, WICBitmapTransformRotate0, WICBitmapTransformRotate180, WICBitmapTransformRotate270,
    WICBitmapTransformRotate90, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STREAM_SEEK_END,
    STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::VT_UI2;

/// Why there's no preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbError {
    /// A HEIC photo, and Windows has no decoder for it: HEIF Image Extensions isn't installed.
    NeedsHeif,
    /// A HEIC photo the HEIF decoder opened but couldn't decode: HEVC Video Extensions isn't
    /// installed.
    NeedsHevc,
    /// Anything else: a damaged file, or Windows didn't answer in time.
    Failed,
}

/// Media Foundation's "no suitable decoder" (`MF_E_TOPO_CODEC_NOT_FOUND`), what the HEIF decoder
/// reports when it can't find an HEVC decoder.
const MF_E_TOPO_CODEC_NOT_FOUND: HRESULT = HRESULT(0xC00D_5212_u32 as i32);

/// How far decoding got when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Finding a decoder for the file.
    Open,
    /// Decoding a frame, once a decoder was found.
    Decode,
}

/// Whether a file starts like a HEIF image: an ISO box `ftyp` with the brand iPhones write.
pub fn is_heif_file(head: &[u8]) -> bool {
    head.len() >= 12 && &head[4..8] == b"ftyp" && matches!(&head[8..12], b"heic" | b"heix" | b"mif1")
}

fn heif_on_disk(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 12];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && is_heif_file(&head)
}

/// Which extension a failure means is missing, if any.
fn classify(step: Step, code: HRESULT, heif: bool) -> ThumbError {
    if !heif {
        return ThumbError::Failed;
    }
    match step {
        Step::Open if code == WINCODEC_ERR_COMPONENTNOTFOUND || code == WINCODEC_ERR_UNKNOWNIMAGEFORMAT => {
            ThumbError::NeedsHeif
        }
        Step::Decode if code == WINCODEC_ERR_COMPONENTNOTFOUND || code == MF_E_TOPO_CODEC_NOT_FOUND => {
            ThumbError::NeedsHevc
        }
        _ => ThumbError::Failed,
    }
}

/// The longest side of a preview, in pixels.
pub const MAX_SIDE: u32 = 720;
const TIME_LIMIT: Duration = Duration::from_secs(20);

/// A JPEG no larger than `max_side` on its longest side.
pub fn jpeg_thumbnail(path: &Path, max_side: u32) -> Result<Vec<u8>, ThumbError> {
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("tug-thumb".into())
        .spawn(move || {
            // SAFETY: COM is initialised on this thread for the call and released only if it was.
            let result = unsafe {
                let init = CoInitializeEx(None, COINIT_MULTITHREADED);
                let r = encode(&path, max_side);
                if init.is_ok() {
                    CoUninitialize();
                }
                r
            };
            let _ = tx.send(result);
        })
        .map_err(|_| ThumbError::Failed)?;
    rx.recv_timeout(TIME_LIMIT).unwrap_or(Err(ThumbError::Failed))
}

/// SAFETY: COM must be initialised on the calling thread.
unsafe fn encode(path: &Path, max_side: u32) -> Result<Vec<u8>, ThumbError> {
    let heif = heif_on_disk(path);
    let opening = |e: windows::core::Error| classify(Step::Open, e.code(), heif);
    let decoding = |e: windows::core::Error| classify(Step::Decode, e.code(), heif);
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|_| ThumbError::Failed)?;
    let decoder = factory
        .CreateDecoderFromFilename(
            // tug's own folder (see files::open for why not from the OsStr).
            &HSTRING::from(&*path.to_string_lossy()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
        .map_err(opening)?;
    let frame = decoder.GetFrame(0).map_err(decoding)?;
    let transform = frame
        .GetMetadataQueryReader()
        .ok()
        .and_then(|reader| {
            let mut value = PROPVARIANT::default();
            let read = reader.GetMetadataByName(w!("System.Photo.Orientation"), &mut value);
            let inner = &value.Anonymous.Anonymous;
            let orientation = (read.is_ok() && inner.vt == VT_UI2).then(|| inner.Anonymous.uiVal);
            let _ = PropVariantClear(&mut value);
            orientation
        })
        .map(orientation_transform)
        .unwrap_or(WICBitmapTransformRotate0);

    let (mut w, mut h) = (0u32, 0u32);
    frame.GetSize(&mut w, &mut h).map_err(decoding)?;
    if w == 0 || h == 0 {
        return Err(ThumbError::Failed);
    }
    let scale = (max_side as f64 / w.max(h) as f64).min(1.0);
    let (sw, sh) = (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    );
    let scaler = factory.CreateBitmapScaler().map_err(|_| ThumbError::Failed)?;
    scaler
        .Initialize(&frame, sw, sh, WICBitmapInterpolationModeFant)
        .map_err(decoding)?;
    let mut source: IWICBitmapSource = scaler.into();
    if transform != WICBitmapTransformRotate0 {
        let rotator = factory.CreateBitmapFlipRotator().map_err(|_| ThumbError::Failed)?;
        rotator.Initialize(&source, transform).map_err(|_| ThumbError::Failed)?;
        source = rotator.into();
    }
    let converter = factory.CreateFormatConverter().map_err(|_| ThumbError::Failed)?;
    converter
        .Initialize(
            &source,
            &GUID_WICPixelFormat24bppBGR,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .map_err(decoding)?;
    let (mut ow, mut oh) = (0u32, 0u32);
    converter.GetSize(&mut ow, &mut oh).map_err(|_| ThumbError::Failed)?;

    let stream = CreateStreamOnHGlobal(Default::default(), true).map_err(|_| ThumbError::Failed)?;
    let encoder = factory
        .CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())
        .map_err(|_| ThumbError::Failed)?;
    encoder
        .Initialize(&stream, WICBitmapEncoderNoCache)
        .map_err(|_| ThumbError::Failed)?;
    let mut out_frame = None;
    let mut options = None;
    encoder
        .CreateNewFrame(&mut out_frame, &mut options)
        .map_err(|_| ThumbError::Failed)?;
    let out_frame = out_frame.ok_or(ThumbError::Failed)?;
    out_frame.Initialize(options.as_ref()).map_err(|_| ThumbError::Failed)?;
    out_frame.SetSize(ow, oh).map_err(|_| ThumbError::Failed)?;
    let mut format = GUID_WICPixelFormat24bppBGR;
    out_frame.SetPixelFormat(&mut format).map_err(|_| ThumbError::Failed)?;
    // The pixels are decoded here, so a missing HEVC decoder can show up here too.
    out_frame.WriteSource(&converter, std::ptr::null()).map_err(decoding)?;
    out_frame.Commit().map_err(|_| ThumbError::Failed)?;
    encoder.Commit().map_err(|_| ThumbError::Failed)?;

    let mut len = 0u64;
    stream
        .Seek(0, STREAM_SEEK_END, Some(&mut len))
        .map_err(|_| ThumbError::Failed)?;
    stream.Seek(0, STREAM_SEEK_SET, None).map_err(|_| ThumbError::Failed)?;
    let mut bytes = vec![0u8; usize::try_from(len).map_err(|_| ThumbError::Failed)?];
    let mut read = 0u32;
    stream
        .Read(bytes.as_mut_ptr().cast(), bytes.len() as u32, Some(&mut read))
        .ok()
        .map_err(|_| ThumbError::Failed)?;
    bytes.truncate(read as usize);
    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err(ThumbError::Failed);
    }
    Ok(bytes)
}

/// EXIF orientation (1–8) to the turn that makes the picture upright. The mirrored ones (2, 4, 5,
/// 7, which a phone camera doesn't write) get the flip, and for 5 and 7 their nearest turn.
fn orientation_transform(o: u16) -> WICBitmapTransformOptions {
    match o {
        2 => WICBitmapTransformFlipHorizontal,
        3 => WICBitmapTransformRotate180,
        4 => WICBitmapTransformFlipVertical,
        5 | 6 => WICBitmapTransformRotate90,
        7 | 8 => WICBitmapTransformRotate270,
        _ => WICBitmapTransformRotate0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_a_png_as_a_small_jpeg() {
        let dir = std::env::temp_dir().join(format!("tug-wic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.png");
        let (w, h) = (1600u32, 900u32);
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut enc = png::Encoder::new(file, w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            let pixels: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
            writer.write_image_data(&pixels).unwrap();
        }
        let jpeg = jpeg_thumbnail(&path, MAX_SIDE).unwrap();
        assert!(jpeg.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert!(jpeg.len() < 400_000);
        // Not an image at all.
        std::fs::write(dir.join("not.heic"), b"definitely not a photo").unwrap();
        assert!(jpeg_thumbnail(&dir.join("not.heic"), MAX_SIDE).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_real_heif_file_can_be_missing_an_extension() {
        let heic = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic";
        assert!(is_heif_file(heic));
        assert!(is_heif_file(b"\0\0\0\x1cftypmif1\0\0\0\0"));
        assert!(is_heif_file(b"\0\0\0\x1cftypheix\0\0\0\0"));
        assert!(!is_heif_file(b"\xFF\xD8\xFF\xE0\0\x10JFIF\0\x01"), "a JPEG");
        assert!(!is_heif_file(b"\0\0\0\x18ftypqt  \0\0\0\0"), "a video");
        assert!(!is_heif_file(b"ftyp"));

        // No decoder for a HEIF file: HEIF Image Extensions is missing.
        assert_eq!(
            classify(Step::Open, WINCODEC_ERR_COMPONENTNOTFOUND, true),
            ThumbError::NeedsHeif
        );
        assert_eq!(
            classify(Step::Open, WINCODEC_ERR_UNKNOWNIMAGEFORMAT, true),
            ThumbError::NeedsHeif
        );
        // The decoder opened it but couldn't decode: HEVC Video Extensions is missing.
        assert_eq!(
            classify(Step::Decode, WINCODEC_ERR_COMPONENTNOTFOUND, true),
            ThumbError::NeedsHevc
        );
        assert_eq!(
            classify(Step::Decode, MF_E_TOPO_CODEC_NOT_FOUND, true),
            ThumbError::NeedsHevc
        );
        // A file that isn't HEIF ("unknown format" is just a damaged or odd file), or any other
        // error, is no missing extension.
        assert_eq!(
            classify(Step::Open, WINCODEC_ERR_UNKNOWNIMAGEFORMAT, false),
            ThumbError::Failed
        );
        assert_eq!(
            classify(Step::Decode, MF_E_TOPO_CODEC_NOT_FOUND, false),
            ThumbError::Failed
        );
        assert_eq!(
            classify(Step::Open, HRESULT(0x8007_0005_u32 as i32), true),
            ThumbError::Failed
        );
    }

    #[test]
    fn a_heic_name_on_a_file_that_isnt_heif_is_just_unreadable() {
        let dir = std::env::temp_dir().join(format!("tug-wic-heif-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("not.heic"), b"definitely not a photo").unwrap();
        assert_eq!(jpeg_thumbnail(&dir.join("not.heic"), MAX_SIDE), Err(ThumbError::Failed));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn orientation_tags_turn_the_picture_upright() {
        assert_eq!(orientation_transform(1), WICBitmapTransformRotate0);
        assert_eq!(orientation_transform(6), WICBitmapTransformRotate90);
        assert_eq!(orientation_transform(8), WICBitmapTransformRotate270);
        assert_eq!(orientation_transform(3), WICBitmapTransformRotate180);
        assert_eq!(orientation_transform(99), WICBitmapTransformRotate0);
    }
}
