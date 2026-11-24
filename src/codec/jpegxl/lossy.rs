use super::jpegparse::parse_jpeg;
use super::recompress::encode_recompressed;
use super::{Error, ErrorKind, Image, MAX_IMAGE_PIXELS};
use crate::codec::jpeg::stdjpeg;
pub const DEFAULT_LOSSY_QUALITY: i64 = 90;
pub const MIN_LOSSY_QUALITY: i64 = 1;
pub const MAX_LOSSY_QUALITY: i64 = 100;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LossyOptions {
    pub quality: i64,
}
fn err(kind: ErrorKind, detail: String) -> Error {
    Error::new(kind, Some(detail))
}
pub fn encode_lossy(img: &Image, opts: LossyOptions) -> Result<Vec<u8>, Error> {
    let quality = if opts.quality == 0 {
        DEFAULT_LOSSY_QUALITY
    } else {
        opts.quality
    };
    if !(MIN_LOSSY_QUALITY..=MAX_LOSSY_QUALITY).contains(&quality) {
        return Err(err(
            ErrorKind::Unsupported,
            format!("quality {quality} outside 1..100"),
        ));
    }
    validate_image(img)?;
    if img.bits_per_sample != 8 {
        return Err(err(
            ErrorKind::Unsupported,
            "lossy encoding requires 8-bit samples".into(),
        ));
    }
    if img.width >= 1 << 16 || img.height >= 1 << 16 {
        return Err(Error::new(
            ErrorKind::Other,
            Some("jpeg: image is too large to encode".into()),
        ));
    }
    let pix: Vec<u8> = img.samples.iter().map(|&v| v as u8).collect();
    let source = if img.channels == 1 {
        stdjpeg::Source::Gray(&pix)
    } else {
        stdjpeg::Source::Rgb(&pix)
    };
    let jpg = stdjpeg::encode(source, img.width, img.height, quality);
    let p = parse_jpeg(&jpg)?;
    encode_recompressed(&p.data, false)
}
pub(crate) fn validate_image(img: &Image) -> Result<(), Error> {
    if img.width == 0 || img.height == 0 {
        return Err(err(ErrorKind::Unsupported, "empty image".into()));
    }
    if img.width > 1 << 30
        || img.height > 1 << 30
        || (img.width as u64).saturating_mul(img.height as u64) > MAX_IMAGE_PIXELS
    {
        return Err(err(ErrorKind::Limit, "image too large".into()));
    }
    if img.channels != 1 && img.channels != 3 {
        return Err(err(
            ErrorKind::Unsupported,
            format!("{} channels", img.channels),
        ));
    }
    if !(1..=16).contains(&img.bits_per_sample) {
        return Err(err(
            ErrorKind::Unsupported,
            format!("{} bits per sample", img.bits_per_sample),
        ));
    }
    if img.samples.len()
        != img
            .width
            .saturating_mul(img.height)
            .saturating_mul(img.channels)
    {
        return Err(err(
            ErrorKind::Unsupported,
            format!(
                "{} samples for {}x{}x{}",
                img.samples.len(),
                img.width,
                img.height,
                img.channels
            ),
        ));
    }
    let max_val = 1i32
        .checked_shl(img.bits_per_sample)
        .unwrap_or(0)
        .wrapping_sub(1);
    if let Some(&v) = img.samples.iter().find(|&&v| v < 0 || v > max_val) {
        return Err(err(
            ErrorKind::Unsupported,
            format!("sample {v} outside 0..{max_val}"),
        ));
    }
    Ok(())
}
