use super::Metadata;
use super::pixeldata::{PixelData, extract_metadata, extract_view, frame_size};
use crate::object::File;
use std::fmt;
pub const MISMATCH_MESSAGE: &str = "the image data does not match its description in the file";
pub const SUPPORTED_SAMPLES: [u16; 3] = [1, 3, 4];
pub(crate) const SUPPORTED_BITS_ALLOCATED: [u16; 4] = [1, 8, 16, 32];
pub const MAX_JPEG_SEGMENTS: usize = 512;
const MAX_JP2_BOXES: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardLimits {
    pub max_side: i64,
    pub max_samples: i64,
    pub max_frames: i64,
    pub max_frame_bytes: i64,
    pub max_series_bytes: i64,
}
impl Default for GuardLimits {
    fn default() -> Self {
        GuardLimits::DEFAULT
    }
}
impl GuardLimits {
    pub const DEFAULT: GuardLimits = GuardLimits {
        max_side: 16_384,
        max_samples: 67_108_864,
        max_frames: 10_000,
        max_frame_bytes: 512 << 20,
        max_series_bytes: 2 << 30,
    };
    pub fn clamped(&self) -> GuardLimits {
        let d = GuardLimits::DEFAULT;
        let clamp = |v: i64, d: i64| v.clamp(d / 10, d.saturating_mul(4));
        GuardLimits {
            max_side: clamp(self.max_side, d.max_side),
            max_samples: clamp(self.max_samples, d.max_samples),
            max_frames: clamp(self.max_frames, d.max_frames),
            max_frame_bytes: clamp(self.max_frame_bytes, d.max_frame_bytes),
            max_series_bytes: clamp(self.max_series_bytes, d.max_series_bytes),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Larger {
    Stream,
    Attributes,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    NoRows,
    NoColumns,
    TooManyRows { rows: i64, limit: i64 },
    TooManyColumns { columns: i64, limit: i64 },
    TooManySamples { samples: i64, limit: i64 },
    TooManyFrames { frames: i64, limit: i64 },
    UnsupportedSamples(u16),
    UnsupportedBitsAllocated(u16),
    BitsStored { stored: u16, allocated: u16 },
    NativeLength { actual: i64, expected: i64 },
    FrameTooLarge { frame: i64, bytes: i64, limit: i64 },
    SeriesTooLarge { bytes: i64, limit: i64 },
    Header { frame: i64, details: String },
    Mismatch { larger: Larger, details: String },
}
impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GuardError::NoRows => f.write_str("the image has no rows (Rows is 0)"),
            GuardError::NoColumns => f.write_str("the image has no columns (Columns is 0)"),
            GuardError::TooManyRows { rows, limit } => {
                write!(f, "the image is {rows} pixels high; the limit is {limit}")
            }
            GuardError::TooManyColumns { columns, limit } => {
                write!(
                    f,
                    "the image is {columns} pixels wide; the limit is {limit}"
                )
            }
            GuardError::TooManySamples { samples, limit } => write!(
                f,
                "one frame has {samples} samples (Rows x Columns x Samples per Pixel); the limit is {limit}"
            ),
            GuardError::TooManyFrames { frames, limit } => {
                write!(f, "the image has {frames} frames; the limit is {limit}")
            }
            GuardError::UnsupportedSamples(s) => write!(
                f,
                "Samples per Pixel {s} is not supported (1, 3, or 4 for ARGB)"
            ),
            GuardError::UnsupportedBitsAllocated(b) => {
                write!(f, "Bits Allocated {b} is not supported (1, 8, 16 or 32)")
            }
            GuardError::BitsStored { stored, allocated } => write!(
                f,
                "Bits Stored {stored} must be between 1 and Bits Allocated ({allocated})"
            ),
            GuardError::NativeLength { actual, expected } => write!(
                f,
                "the pixel data has {actual} bytes; the image attributes describe {expected}"
            ),
            GuardError::FrameTooLarge {
                frame,
                bytes,
                limit,
            } => write!(
                f,
                "compressed frame {} has {bytes} bytes; the limit is {limit}",
                frame.saturating_add(1)
            ),
            GuardError::SeriesTooLarge { bytes, limit } => write!(
                f,
                "the images would need {bytes} bytes once decoded; the limit is {limit}"
            ),
            GuardError::Header { frame, .. } => write!(
                f,
                "the compressed stream of frame {} has no readable image header",
                frame.saturating_add(1)
            ),
            GuardError::Mismatch { larger, .. } => write!(
                f,
                "{MISMATCH_MESSAGE} ({} is larger)",
                match larger {
                    Larger::Stream => "the compressed stream",
                    Larger::Attributes => "the description",
                }
            ),
        }
    }
}
impl std::error::Error for GuardError {}
impl GuardError {
    pub fn details(&self) -> String {
        match self {
            GuardError::Header { details, .. } | GuardError::Mismatch { details, .. } => {
                format!("{self}: {details}")
            }
            _ => self.to_string(),
        }
    }
}
pub fn check_attributes(m: &Metadata, limits: &GuardLimits) -> Result<(), GuardError> {
    let (rows, columns) = (i64::from(m.rows), i64::from(m.columns));
    if rows == 0 {
        return Err(GuardError::NoRows);
    }
    if columns == 0 {
        return Err(GuardError::NoColumns);
    }
    if rows > limits.max_side {
        return Err(GuardError::TooManyRows {
            rows,
            limit: limits.max_side,
        });
    }
    if columns > limits.max_side {
        return Err(GuardError::TooManyColumns {
            columns,
            limit: limits.max_side,
        });
    }
    if !SUPPORTED_SAMPLES.contains(&m.samples_per_pixel) {
        return Err(GuardError::UnsupportedSamples(m.samples_per_pixel));
    }
    let samples = rows
        .saturating_mul(columns)
        .saturating_mul(i64::from(m.samples_per_pixel));
    if samples > limits.max_samples {
        return Err(GuardError::TooManySamples {
            samples,
            limit: limits.max_samples,
        });
    }
    if m.number_of_frames > limits.max_frames {
        return Err(GuardError::TooManyFrames {
            frames: m.number_of_frames,
            limit: limits.max_frames,
        });
    }
    if !SUPPORTED_BITS_ALLOCATED.contains(&m.bits_allocated) {
        return Err(GuardError::UnsupportedBitsAllocated(m.bits_allocated));
    }
    if m.bits_stored == 0 || m.bits_stored > m.bits_allocated {
        return Err(GuardError::BitsStored {
            stored: m.bits_stored,
            allocated: m.bits_allocated,
        });
    }
    Ok(())
}
pub fn estimated_decoded_bytes(m: &Metadata) -> i64 {
    frame_size(m)
        .checked_mul(m.number_of_frames.max(1))
        .unwrap_or(i64::MAX)
}
pub fn check_series_bytes(bytes: i64, limits: &GuardLimits) -> Result<(), GuardError> {
    if bytes > limits.max_series_bytes {
        return Err(GuardError::SeriesTooLarge {
            bytes,
            limit: limits.max_series_bytes,
        });
    }
    Ok(())
}
#[doc(hidden)]
pub fn check_native_length(
    m: &Metadata,
    actual: i64,
    allow_trailing: bool,
) -> Result<(), GuardError> {
    let frames = m.number_of_frames.max(1);
    let expected = if m.bits_allocated < 8 {
        let bits = i64::from(m.rows)
            .saturating_mul(i64::from(m.columns))
            .saturating_mul(i64::from(m.samples_per_pixel))
            .saturating_mul(frames)
            .saturating_mul(i64::from(m.bits_allocated));
        (bits / 8).saturating_add(i64::from(bits % 8 != 0))
    } else {
        frame_size(m).saturating_mul(frames)
    };
    let diff = actual.saturating_sub(expected);
    if diff < -1 || (diff > 1 && !allow_trailing) {
        return Err(GuardError::NativeLength { actual, expected });
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamHeader {
    pub width: i64,
    pub height: i64,
    pub components: i64,
    pub precision: i64,
    pub marker: u8,
}
fn be16(data: &[u8], at: usize) -> Option<i64> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(i64::from(u16::from_be_bytes(b.try_into().ok()?)))
}
fn be32(data: &[u8], at: usize) -> Option<i64> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(i64::from(u32::from_be_bytes(b.try_into().ok()?)))
}
#[doc(hidden)]
pub fn jpeg_frame_header(data: &[u8]) -> Result<StreamHeader, String> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return Err("no JPEG SOI marker".into());
    }
    let mut pos = 2usize;
    for _ in 0..MAX_JPEG_SEGMENTS {
        if data.get(pos) != Some(&0xFF) {
            return Err(format!("no marker at byte {pos}"));
        }
        while data.get(pos.saturating_add(1)) == Some(&0xFF) {
            pos = pos.saturating_add(1);
        }
        let Some(&marker) = data.get(pos.saturating_add(1)) else {
            return Err("stream ends inside a marker".into());
        };
        pos = pos.saturating_add(2);
        match marker {
            0xD9 => return Err("EOI before a frame header".into()),
            0xDA => return Err("SOS before a frame header".into()),
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        let Some(length) = be16(data, pos) else {
            return Err("stream ends inside a segment length".into());
        };
        if length < 2 {
            return Err(format!("segment length {length} at byte {pos}"));
        }
        let is_sof = matches!(marker, 0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC)
            || marker == 0xF7;
        if is_sof {
            let (Some(&precision), Some(height), Some(width), Some(&components)) = (
                data.get(pos.saturating_add(2)),
                be16(data, pos.saturating_add(3)),
                be16(data, pos.saturating_add(5)),
                data.get(pos.saturating_add(7)),
            ) else {
                return Err("stream ends inside the frame header".into());
            };
            return Ok(StreamHeader {
                width,
                height,
                components: i64::from(components),
                precision: i64::from(precision),
                marker,
            });
        }
        pos = pos
            .checked_add(length as usize)
            .ok_or_else(|| "segment length overflows".to_string())?;
    }
    Err(format!(
        "no frame header within {MAX_JPEG_SEGMENTS} marker segments"
    ))
}
const JP2_SIGNATURE: [u8; 12] = [0, 0, 0, 12, b'j', b'P', b' ', b' ', 0x0D, 0x0A, 0x87, 0x0A];
#[doc(hidden)]
pub fn jpeg2000_siz(data: &[u8]) -> Result<StreamHeader, String> {
    let codestream = if data.starts_with(&JP2_SIGNATURE) {
        jp2_codestream(data)?
    } else {
        data
    };
    if codestream.get(..4) != Some(&[0xFF, 0x4F, 0xFF, 0x51]) {
        return Err("no SOC and SIZ markers".into());
    }
    let field = |at: usize| be32(codestream, at).ok_or("stream ends inside SIZ");
    let x = field(8)?;
    let y = field(12)?;
    let x0 = field(16)?;
    let y0 = field(20)?;
    let components = be16(codestream, 40).ok_or("stream ends inside SIZ")?;
    if x0 > x || y0 > y || components == 0 {
        return Err(format!(
            "SIZ image area {x0}..{x} x {y0}..{y}, {components} components"
        ));
    }
    let mut precision = 0;
    for c in 0..components as usize {
        let &ssiz = codestream
            .get(c.saturating_mul(3).saturating_add(42))
            .ok_or("stream ends inside SIZ components")?;
        precision = precision.max(i64::from(ssiz & 0x7F).saturating_add(1));
    }
    Ok(StreamHeader {
        width: x.saturating_sub(x0),
        height: y.saturating_sub(y0),
        components,
        precision,
        marker: 0,
    })
}
fn jp2_codestream(data: &[u8]) -> Result<&[u8], String> {
    let mut pos = 0usize;
    for _ in 0..MAX_JP2_BOXES {
        let length = be32(data, pos).ok_or("stream ends inside a JP2 box")?;
        let kind = data
            .get(pos.saturating_add(4)..pos.saturating_add(8))
            .ok_or("stream ends inside a JP2 box")?;
        let (header, length) = match length {
            0 => (8usize, data.len().saturating_sub(pos) as i64),
            1 => {
                let hi = be32(data, pos.saturating_add(8)).ok_or("stream ends inside a JP2 box")?;
                let lo =
                    be32(data, pos.saturating_add(12)).ok_or("stream ends inside a JP2 box")?;
                (16, hi.checked_shl(32).unwrap_or(i64::MAX) | lo)
            }
            n => (8, n),
        };
        if length < header as i64 {
            return Err(format!("JP2 box length {length} at byte {pos}"));
        }
        if kind == b"jp2c" {
            let end = pos.saturating_add(length as usize).min(data.len());
            return data
                .get(pos.saturating_add(header)..end)
                .ok_or_else(|| format!("JP2 box length {length} at byte {pos}"));
        }
        pos = pos
            .checked_add(length as usize)
            .filter(|&p| p < data.len())
            .ok_or("no contiguous codestream box")?;
    }
    Err(format!("no codestream within {MAX_JP2_BOXES} JP2 boxes"))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Jpeg,
    JpegLs,
    Jpeg2000,
}
pub fn family(uid: &str) -> Option<Family> {
    let uid = uid.trim_end_matches(['\0', ' ']);
    let tail = uid.strip_prefix("1.2.840.10008.1.2.4.")?;
    match tail.parse::<u32>().ok()? {
        50..=57 | 70 => Some(Family::Jpeg),
        80 | 81 => Some(Family::JpegLs),
        90..=93 | 201..=203 => Some(Family::Jpeg2000),
        _ => None,
    }
}
pub fn check_stream(
    family: Family,
    data: &[u8],
    m: &Metadata,
    frame: i64,
) -> Result<(), GuardError> {
    let header = match family {
        Family::Jpeg | Family::JpegLs => jpeg_frame_header(data),
        Family::Jpeg2000 => jpeg2000_siz(data),
    }
    .map_err(|details| GuardError::Header { frame, details })?;
    let (rows, columns, samples) = (
        i64::from(m.rows),
        i64::from(m.columns),
        i64::from(m.samples_per_pixel),
    );
    let describe = || {
        format!(
            "frame {}: stream {}x{} with {} component(s) at {} bits (marker {:#04x}); attributes {}x{} with {} sample(s), Bits Stored {}, Bits Allocated {}",
            frame.saturating_add(1),
            header.width,
            header.height,
            header.components,
            header.precision,
            header.marker,
            columns,
            rows,
            samples,
            m.bits_stored,
            m.bits_allocated
        )
    };
    let mismatch = |larger| GuardError::Mismatch {
        larger,
        details: describe(),
    };
    for (stream, described) in [
        (header.width, columns),
        (header.height, rows),
        (header.components, samples),
    ] {
        if stream != described {
            return Err(mismatch(if stream > described {
                Larger::Stream
            } else {
                Larger::Attributes
            }));
        }
    }
    let p = header.precision;
    let (stored, allocated) = (i64::from(m.bits_stored), i64::from(m.bits_allocated));
    let lossless = matches!(header.marker, 0xC3 | 0xC7 | 0xCB | 0xCF);
    let range = match family {
        Family::Jpeg if lossless => 2..=16,
        Family::Jpeg => 8..=12,
        Family::JpegLs => 2..=16,
        Family::Jpeg2000 => 1..=38,
    };
    if !range.contains(&p) || (family == Family::Jpeg && !lossless && p != 8 && p != 12) {
        return Err(GuardError::Header {
            frame,
            details: format!(
                "sample precision {p} is outside the codec's range; {}",
                describe()
            ),
        });
    }
    if p > allocated {
        return Err(mismatch(Larger::Stream));
    }
    if family != Family::Jpeg2000 && p < stored {
        return Err(mismatch(Larger::Attributes));
    }
    Ok(())
}
fn frame_streams<'a>(
    offset_table: &[u8],
    fragments: &'a [Vec<u8>],
    frames: i64,
    family: Option<Family>,
) -> Option<Vec<(&'a [u8], i64)>> {
    let frames = usize::try_from(frames.max(1)).ok()?;
    let mut starts: Vec<usize> = Vec::new();
    if offset_table.len() >= 4 && offset_table.len().is_multiple_of(4) {
        let offsets: Vec<u64> = offset_table
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u64::from(u32::from_le_bytes(*c)))
            .collect();
        let mut pos = 0u64;
        let mut at = 0usize;
        for (i, f) in fragments.iter().enumerate() {
            if offsets.get(at) == Some(&pos) {
                starts.push(i);
                at = at.saturating_add(1);
            }
            pos = pos.saturating_add(f.len() as u64).saturating_add(8);
        }
        if starts.len() != offsets.len() {
            starts.clear();
        }
    }
    if starts.is_empty() {
        if fragments.len() == frames || frames == 1 {
            starts = if frames == 1 {
                vec![0]
            } else {
                (0..frames).collect()
            };
        } else {
            let opens = |f: &[u8]| match family {
                Some(Family::Jpeg | Family::JpegLs) => f.starts_with(&[0xFF, 0xD8]),
                Some(Family::Jpeg2000) => {
                    f.starts_with(&[0xFF, 0x4F]) || f.starts_with(&JP2_SIGNATURE)
                }
                None => false,
            };
            starts = fragments
                .iter()
                .enumerate()
                .filter(|(_, f)| opens(f))
                .map(|(i, _)| i)
                .collect();
        }
    }
    if starts.len() != frames || fragments.is_empty() || starts.first() != Some(&0) {
        return None;
    }
    let mut out = Vec::with_capacity(frames);
    for (k, &start) in starts.iter().enumerate() {
        let end = starts
            .get(k.saturating_add(1))
            .copied()
            .unwrap_or(fragments.len());
        let frame = fragments.get(start..end)?;
        let bytes: i64 = frame
            .iter()
            .map(|f| f.len() as i64)
            .fold(0, i64::saturating_add);
        out.push((frame.first()?.as_slice(), bytes));
    }
    Some(out)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuardOptions {
    pub limits: GuardLimits,
    pub allow_trailing: bool,
    pub frame: Option<i64>,
}
pub fn guard_file(file: &File, options: &GuardOptions) -> Result<(), GuardError> {
    let limits = options.limits.clamped();
    let Ok(m) = extract_metadata(&file.dataset) else {
        return Ok(());
    };
    check_attributes(&m, &limits)?;
    check_series_bytes(estimated_decoded_bytes(&m), &limits)?;
    let Ok(pixels) = extract_view(&file.dataset) else {
        return Ok(());
    };
    match pixels {
        PixelData::Native(data) => {
            if !file.transfer_syntax.encapsulated {
                check_native_length(&m, data.len() as i64, options.allow_trailing)?;
            }
            Ok(())
        }
        PixelData::Encapsulated {
            offset_table,
            fragments,
        } => {
            let family = family(file.transfer_syntax.uid);
            let Some(frames) = frame_streams(offset_table, fragments, m.number_of_frames, family)
            else {
                let total: i64 = fragments.iter().map(|f| f.len() as i64).sum();
                let limit = limits
                    .max_frame_bytes
                    .saturating_mul(m.number_of_frames.max(1));
                if total > limit {
                    return Err(GuardError::FrameTooLarge {
                        frame: 0,
                        bytes: total,
                        limit,
                    });
                }
                return Ok(());
            };
            for (i, (head, bytes)) in frames.into_iter().enumerate() {
                let i = i as i64;
                if options.frame.is_some_and(|f| f != i) {
                    continue;
                }
                if bytes > limits.max_frame_bytes {
                    return Err(GuardError::FrameTooLarge {
                        frame: i,
                        bytes,
                        limit: limits.max_frame_bytes,
                    });
                }
                if let Some(family) = family {
                    check_stream(family, head, &m, i)?;
                }
            }
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests;
