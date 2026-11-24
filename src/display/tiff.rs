use super::object::{ObjectError, ObjectErrorKind};
use super::{Image, MAX_EXPORT_PIXELS};
const UNSUPPORTED_TIFF: &str = "dicom/display: unsupported TIFF image";
fn unsupported(detail: impl std::fmt::Display) -> ObjectError {
    ObjectError::new(
        ObjectErrorKind::UnsupportedTiff,
        format!("{UNSUPPORTED_TIFF}: {detail}"),
    )
}
pub fn encode_tiff(img: Option<&Image>) -> Result<Vec<u8>, ObjectError> {
    let img = img.ok_or_else(|| unsupported("nil image"))?;
    let (width, height) = img.size();
    let (w64, h64) = (width as i64, height as i64);
    if !(w64 > 0
        && h64 > 0
        && w64 <= MAX_EXPORT_PIXELS
        && h64 <= MAX_EXPORT_PIXELS
        && w64 <= MAX_EXPORT_PIXELS / h64)
    {
        return Err(unsupported(format_args!(
            "size exceeds {MAX_EXPORT_PIXELS} pixels"
        )));
    }
    let (bits, samples, photometric): (u32, u32, u32) = match img {
        Image::Gray(_) => (8, 1, 1),
        Image::Gray16(_) => (16, 1, 1),
        Image::Rgba(rgba) => {
            if rgba.pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 255) {
                return Err(unsupported("RGBA must be opaque"));
            }
            (8, 3, 2)
        }
        Image::Rgba64(_) => return Err(unsupported("need Gray, Gray16 or opaque RGBA")),
    };
    const DIRECTORY_SIZE: u32 = 2 + 10 * 12 + 4;
    let mut pixel_offset = 8 + DIRECTORY_SIZE;
    let mut bits_value = bits;
    if samples == 3 {
        bits_value = pixel_offset;
        pixel_offset += 6;
    }
    let mut out = vec![0u8; pixel_offset as usize];
    out[..8].copy_from_slice(&[b'I', b'I', 42, 0, 8, 0, 0, 0]);
    out[8..10].copy_from_slice(&10u16.to_le_bytes());
    let (w, h) = (width as u32, height as u32);
    let entries: [[u32; 4]; 10] = [
        [256, 4, 1, w],
        [257, 4, 1, h],
        [258, 3, samples, bits_value],
        [259, 3, 1, 1],
        [262, 3, 1, photometric],
        [273, 4, 1, pixel_offset],
        [277, 3, 1, samples],
        [278, 4, 1, h],
        [
            279,
            4,
            1,
            w.wrapping_mul(h)
                .wrapping_mul(samples)
                .wrapping_mul(bits / 8),
        ],
        [284, 3, 1, 1],
    ];
    for (i, e) in entries.iter().enumerate() {
        let o = 10 + i * 12;
        out[o..o + 2].copy_from_slice(&(e[0] as u16).to_le_bytes());
        out[o + 2..o + 4].copy_from_slice(&(e[1] as u16).to_le_bytes());
        out[o + 4..o + 8].copy_from_slice(&e[2].to_le_bytes());
        out[o + 8..o + 12].copy_from_slice(&e[3].to_le_bytes());
    }
    if samples == 3 {
        let base = (8 + DIRECTORY_SIZE) as usize;
        for i in 0..3 {
            out[base + 2 * i..base + 2 * i + 2].copy_from_slice(&8u16.to_le_bytes());
        }
    }
    match img {
        Image::Gray(g) => out.extend_from_slice(&g.pixels[..width * height]),
        Image::Gray16(g) => {
            for p in g.pixels[..2 * width * height].as_chunks::<2>().0 {
                out.extend_from_slice(&[p[1], p[0]]);
            }
        }
        Image::Rgba(rgba) => {
            for p in rgba.pixels[..4 * width * height].as_chunks::<4>().0 {
                out.extend_from_slice(&p[..3]);
            }
        }
        Image::Rgba64(_) => {}
    }
    Ok(out)
}
