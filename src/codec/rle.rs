#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use std::fmt;
pub const UID: &str = "1.2.840.10008.1.2.5";
pub const DIAGNOSTIC_SEGMENT_OVERRUN: &str = "rle-segment-overrun";
pub const DIAGNOSTIC_REVERSED_BYTE_SEGMENTS: &str = "rle-reversed-byte-segments";
const HEADER_LENGTH: usize = 64;
const MAX_FRAGMENT_LENGTH: u64 = (1 << 32) - 2;
const MAX_PACKBITS_EXPANSION: u64 = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidHeader,
    InvalidSegmentCount,
    InvalidSegmentOffset,
    SegmentDecodeFailed,
    UnsupportedBitsAllocated,
    UnsupportedSamplesPerPixel,
    IncompatiblePixelData,
    PixelDataSizeMismatch,
    UnsupportedPhotometricInterpretation,
    UnsupportedPixelRepresentation,
    UnsupportedPlanarConfiguration,
    InvalidRegion,
    BufferTooSmall,
    UnsupportedEncoderMetadata,
    EncoderOutputInvalid,
}
impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidHeader => "dicom: invalid RLE header",
            ErrorKind::InvalidSegmentCount => "dicom: invalid RLE segment count",
            ErrorKind::InvalidSegmentOffset => "dicom: invalid RLE segment offset",
            ErrorKind::SegmentDecodeFailed => "dicom: RLE segment decode failed",
            ErrorKind::UnsupportedBitsAllocated => "dicom: unsupported RLE BitsAllocated",
            ErrorKind::UnsupportedSamplesPerPixel => "dicom: unsupported RLE SamplesPerPixel",
            ErrorKind::IncompatiblePixelData => {
                "dicom: pixel data is incompatible with transfer syntax"
            }
            ErrorKind::PixelDataSizeMismatch => "dicom: pixel data size does not match metadata",
            ErrorKind::UnsupportedPhotometricInterpretation => {
                "dicom: unsupported PhotometricInterpretation"
            }
            ErrorKind::UnsupportedPixelRepresentation => "dicom: unsupported PixelRepresentation",
            ErrorKind::UnsupportedPlanarConfiguration => "dicom: unsupported PlanarConfiguration",
            ErrorKind::InvalidRegion => "dicom: invalid frame region",
            ErrorKind::BufferTooSmall => "dicom: destination buffer too small",
            ErrorKind::UnsupportedEncoderMetadata => {
                "dicom: pixel metadata is unsupported by encoder"
            }
            ErrorKind::EncoderOutputInvalid => "dicom: pixel data encoder returned invalid output",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}
impl Error {
    fn bare(kind: ErrorKind) -> Error {
        Error {
            kind,
            message: kind.text().to_string(),
        }
    }
    fn new(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            message: format!("{}: {detail}", kind.text()),
        }
    }
    fn with_segment(mut self, segment: usize) -> Error {
        self.message = format!("{}: segment {segment}", self.message);
        self
    }
    fn with_frame(mut self, frame: usize) -> Error {
        self.message = format!("decode RLE frame {frame}: {}", self.message);
        self
    }
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub rows: u16,
    pub columns: u16,
    pub samples_per_pixel: u16,
    pub bits_allocated: u16,
    pub bits_stored: u16,
    pub high_bit: u16,
    pub pixel_representation: u16,
    pub planar_configuration: u16,
    pub planar_configuration_present: bool,
    pub number_of_frames: i64,
    pub photometric_interpretation: String,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recovery {
    pub segment_overrun: bool,
    pub reversed_byte_segments: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frames {
    pub rows: usize,
    pub columns: usize,
    pub data: Vec<Vec<u8>>,
    pub diagnostics: Vec<&'static str>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}
impl Rect {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    pub fn dx(&self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn dy(&self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    fn within(&self, columns: i64, rows: i64) -> bool {
        self.is_empty() || (0 <= self.x0 && self.x1 <= columns && 0 <= self.y0 && self.y1 <= rows)
    }
}
impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})-({},{})", self.x0, self.y0, self.x1, self.y1)
    }
}
fn trim_upper(s: &str) -> String {
    crate::textfmt::to_upper(s.trim())
}
fn supported_bits_allocated(bits_allocated: i64, samples_per_pixel: i64) -> bool {
    bits_allocated >= 8
        && bits_allocated % 8 == 0
        && samples_per_pixel.saturating_mul(bits_allocated) / 8 <= 15
}
fn supported_photometric(m: &Metadata) -> bool {
    let photometric = trim_upper(&m.photometric_interpretation);
    match m.samples_per_pixel {
        1 => matches!(
            photometric.as_str(),
            "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR"
        ),
        3 => matches!(photometric.as_str(), "RGB" | "YBR_FULL"),
        _ => false,
    }
}
pub(crate) fn validate_decode_metadata(m: &Metadata) -> Result<(), Error> {
    if m.samples_per_pixel == 0 || m.samples_per_pixel > 3 {
        return Err(Error::new(
            ErrorKind::UnsupportedSamplesPerPixel,
            format_args!("SamplesPerPixel={}", m.samples_per_pixel),
        ));
    }
    if !supported_bits_allocated(m.bits_allocated.into(), m.samples_per_pixel.into()) {
        return Err(Error::new(
            ErrorKind::UnsupportedBitsAllocated,
            format_args!(
                "BitsAllocated={} SamplesPerPixel={}",
                m.bits_allocated, m.samples_per_pixel
            ),
        ));
    }
    if m.bits_stored == 0
        || m.bits_stored > m.bits_allocated
        || m.high_bit != m.bits_stored.wrapping_sub(1)
    {
        return Err(Error::new(
            ErrorKind::IncompatiblePixelData,
            "invalid stored-bit metadata",
        ));
    }
    if m.pixel_representation != 0 && m.pixel_representation != 1 {
        return Err(Error::new(
            ErrorKind::UnsupportedPixelRepresentation,
            format_args!("PixelRepresentation={}", m.pixel_representation),
        ));
    }
    if !supported_photometric(m) {
        return Err(Error::new(
            ErrorKind::UnsupportedPhotometricInterpretation,
            format_args!(
                "PhotometricInterpretation={} SamplesPerPixel={}",
                m.photometric_interpretation.trim(),
                m.samples_per_pixel
            ),
        ));
    }
    let photometric = trim_upper(&m.photometric_interpretation);
    if m.samples_per_pixel == 3 && m.planar_configuration_present && m.planar_configuration > 1 {
        return Err(Error::new(
            ErrorKind::UnsupportedPlanarConfiguration,
            "invalid PlanarConfiguration",
        ));
    }
    if m.pixel_representation == 1 && photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
        return Err(Error::new(
            ErrorKind::UnsupportedPixelRepresentation,
            format_args!("PixelRepresentation={}", m.pixel_representation),
        ));
    }
    Ok(())
}
fn le32(b: &[u8], at: usize) -> u32 {
    b.get(at..)
        .and_then(<[u8]>::first_chunk::<4>)
        .map_or(0, |&q| u32::from_le_bytes(q))
}
fn header_offsets(fragment: &[u8]) -> impl Iterator<Item = u32> + '_ {
    let table = fragment.get(4..HEADER_LENGTH).unwrap_or_default();
    table
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&q| u32::from_le_bytes(q))
}
fn concat(fragments: &[&[u8]]) -> Vec<u8> {
    fragments.concat()
}
fn split_at(fragments: &[&[u8]], starts: &[usize]) -> Vec<Vec<u8>> {
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts
                .get(i.saturating_add(1))
                .copied()
                .unwrap_or(fragments.len());
            concat(fragments.get(start..end).unwrap_or_default())
        })
        .collect()
}
fn offset_table_starts(table: &[u8], fragments: &[&[u8]], frames: usize) -> Option<Vec<usize>> {
    if Some(table.len()) != frames.checked_mul(4) {
        return None;
    }
    let mut positions = std::collections::HashMap::with_capacity(fragments.len());
    let mut position = 0u64;
    for (i, fragment) in fragments.iter().enumerate() {
        positions.insert(position, i);
        position = position
            .saturating_add(8)
            .saturating_add(fragment.len() as u64);
    }
    let mut starts: Vec<usize> = Vec::with_capacity(frames);
    for &entry in table.as_chunks::<4>().0 {
        let index = *positions.get(&u64::from(u32::from_le_bytes(entry)))?;
        let increasing = match starts.last() {
            Some(&previous) => index > previous,
            None => index == 0,
        };
        if !increasing {
            return None;
        }
        starts.push(index);
    }
    Some(starts)
}
fn looks_like_header(fragment: &[u8], expected: usize) -> bool {
    if fragment.len() < HEADER_LENGTH || le32(fragment, 0) as usize != expected {
        return false;
    }
    let mut previous = 0u32;
    for (i, offset) in header_offsets(fragment).enumerate() {
        if (i == 0 && offset != 64)
            || (i < expected && offset < previous)
            || (i >= expected && offset != 0)
        {
            return false;
        }
        previous = offset;
    }
    true
}
fn group_frames(
    fragments: &[&[u8]],
    offset_table: &[u8],
    number_of_frames: i64,
    expected_segments: usize,
) -> Result<Vec<Vec<u8>>, Error> {
    if fragments.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            "no RLE frame fragments",
        ));
    }
    let frames = if number_of_frames <= 0 {
        1
    } else {
        number_of_frames
    };
    if frames == fragments.len() as i64 {
        return Ok(fragments.iter().map(|f| f.to_vec()).collect());
    }
    if frames > fragments.len() as i64 {
        return Err(Error::new(
            ErrorKind::PixelDataSizeMismatch,
            format_args!("NumberOfFrames={frames} fragments={}", fragments.len()),
        ));
    }
    let frames = frames as usize;
    if frames == 1 {
        return Ok(vec![concat(fragments)]);
    }
    if let Some(starts) = offset_table_starts(offset_table, fragments, frames) {
        return Ok(split_at(fragments, &starts));
    }
    let starts: Vec<usize> = fragments
        .iter()
        .enumerate()
        .filter(|&(i, fragment)| i == 0 || looks_like_header(fragment, expected_segments))
        .map(|(i, _)| i)
        .collect();
    if starts.len() != frames {
        return Err(Error::new(
            ErrorKind::PixelDataSizeMismatch,
            format_args!(
                "NumberOfFrames={frames} fragments={}, no Basic Offset Table and {} RLE headers",
                fragments.len(),
                starts.len()
            ),
        ));
    }
    Ok(split_at(fragments, &starts))
}
fn parse_header(fragment: &[u8]) -> Result<Vec<u32>, Error> {
    if fragment.len() < HEADER_LENGTH {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            format_args!("got {} bytes", fragment.len()),
        ));
    }
    if fragment.len() as u64 > u64::from(u32::MAX) {
        return Err(Error::new(
            ErrorKind::InvalidHeader,
            format_args!("fragment too large: {} bytes", fragment.len()),
        ));
    }
    let count = le32(fragment, 0);
    if count == 0 || count > 15 {
        return Err(Error::new(
            ErrorKind::InvalidSegmentCount,
            format_args!("got {count}"),
        ));
    }
    let count = count as usize;
    let length = fragment.len() as u32;
    let mut offsets = Vec::with_capacity(count.saturating_add(1));
    for (i, offset) in header_offsets(fragment).take(count).enumerate() {
        if offset < 64 || offset > length {
            return Err(Error::new(
                ErrorKind::InvalidSegmentOffset,
                format_args!("segment {i} offset={offset} fragment_length={length}"),
            ));
        }
        if let Some(&previous) = offsets.last()
            && offset < previous
        {
            return Err(Error::new(
                ErrorKind::InvalidSegmentOffset,
                format_args!("segment {i} offset={offset} previous_offset={previous}"),
            ));
        }
        offsets.push(offset);
    }
    offsets.push(length);
    Ok(offsets)
}
fn can_fill(n: usize, size: usize) -> bool {
    (n as u64).saturating_mul(MAX_PACKBITS_EXPANSION) >= size as u64
}
fn decode_failed(detail: &str) -> Error {
    Error::new(ErrorKind::SegmentDecodeFailed, detail)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn unpack_bits(
    segment: &[u8],
    dst: &mut [u8],
    first: usize,
    step: usize,
    size: usize,
    tolerant: bool,
) -> Result<bool, Error> {
    if first >= step
        || dst
            .len()
            .checked_div(step)
            .is_none_or(|planes| planes < size)
    {
        return Err(decode_failed("output buffer smaller than the plane"));
    }
    let mut recovered = false;
    let mut written = 0usize;
    let mut i = 0usize;
    while i < segment.len() && written < size {
        let h = segment[i] as i8 as i32;
        i += 1;
        if h >= 0 {
            let mut n = h as usize + 1;
            if i + n > segment.len() {
                if !tolerant {
                    return Err(decode_failed("truncated literal run"));
                }
                n = segment.len() - i;
                recovered = true;
            }
            if n > size - written {
                if !tolerant {
                    return Err(decode_failed("literal run exceeds expected size"));
                }
                n = size - written;
                recovered = true;
            }
            let mut d = first + written * step;
            for &value in &segment[i..i + n] {
                dst[d] = value;
                d += step;
            }
            written += n;
            i += n;
        } else if h >= -127 {
            if i >= segment.len() {
                if !tolerant {
                    return Err(decode_failed("truncated repeat run"));
                }
                recovered = true;
                continue;
            }
            let mut n = (1 - h) as usize;
            let value = segment[i];
            i += 1;
            if n > size - written {
                if !tolerant {
                    return Err(decode_failed("repeat run exceeds expected size"));
                }
                n = size - written;
                recovered = true;
            }
            let mut d = first + written * step;
            for _ in 0..n {
                dst[d] = value;
                d += step;
            }
            written += n;
        }
    }
    if written != size {
        if !tolerant {
            return Err(Error::new(
                ErrorKind::SegmentDecodeFailed,
                format_args!("decoded {written} bytes, expected {size}"),
            ));
        }
        recovered = true;
    }
    Ok(recovered)
}
fn decode_packbits_range(
    segment: &[u8],
    skip: usize,
    stop: usize,
    tolerant: bool,
    out: &mut Vec<u8>,
) -> Result<bool, Error> {
    out.clear();
    let mut position = 0usize;
    let mut recovered = false;
    let mut rest = segment;
    while position < stop
        && let Some((&header, tail)) = rest.split_first()
    {
        rest = tail;
        let h = header as i8;
        if h >= 0 {
            let mut n = usize::from(header).saturating_add(1);
            if n > rest.len() {
                if !tolerant {
                    return Err(decode_failed("truncated literal run"));
                }
                n = rest.len();
                recovered = true;
            }
            let (literal, tail) = rest.split_at_checked(n).unwrap_or((rest, &[]));
            let (from, to) = (position.max(skip), position.saturating_add(n).min(stop));
            if from < to {
                let kept = from.saturating_sub(position)..to.saturating_sub(position);
                out.extend_from_slice(literal.get(kept).unwrap_or_default());
            }
            position = position.saturating_add(n);
            rest = tail;
        } else if h >= -127 {
            let Some((&value, tail)) = rest.split_first() else {
                if !tolerant {
                    return Err(decode_failed("truncated repeat run"));
                }
                recovered = true;
                rest = &[];
                continue;
            };
            rest = tail;
            let n = usize::from(header.wrapping_neg()).saturating_add(1);
            let (from, to) = (position.max(skip), position.saturating_add(n).min(stop));
            if from < to {
                out.resize(out.len().saturating_add(to.saturating_sub(from)), value);
            }
            position = position.saturating_add(n);
        }
    }
    if position < stop {
        if !tolerant {
            return Err(Error::new(
                ErrorKind::SegmentDecodeFailed,
                format_args!("decoded {position} bytes, need {stop}"),
            ));
        }
        let wanted = stop.saturating_sub(skip);
        if out.len() < wanted {
            out.resize(wanted, 0);
        }
        recovered = true;
    }
    Ok(recovered)
}
fn byte_offset(segment: usize, bytes_per_sample: usize, reversed: bool) -> usize {
    let byte = segment.checked_rem(bytes_per_sample).unwrap_or(0);
    if reversed {
        byte
    } else {
        bytes_per_sample.saturating_sub(1).saturating_sub(byte)
    }
}
#[doc(hidden)]
pub fn decode_frame(
    fragment: &[u8],
    rows: i64,
    columns: i64,
    samples_per_pixel: i64,
    bits_allocated: i64,
    recovery: Recovery,
) -> Result<(Vec<u8>, bool), Error> {
    let tolerant = recovery.segment_overrun;
    if !supported_bits_allocated(bits_allocated, samples_per_pixel) {
        return Err(Error::new(
            ErrorKind::UnsupportedBitsAllocated,
            format_args!("BitsAllocated={bits_allocated}"),
        ));
    }
    if rows <= 0 || columns <= 0 || samples_per_pixel <= 0 || samples_per_pixel > 3 {
        return Err(Error::new(
            ErrorKind::InvalidSegmentCount,
            format_args!("rows={rows} columns={columns} samples_per_pixel={samples_per_pixel}"),
        ));
    }
    let bytes_per_sample = (bits_allocated / 8) as usize;
    let samples = samples_per_pixel as usize;
    let expected_segments = samples.saturating_mul(bytes_per_sample);
    let offsets = parse_header(fragment)?;
    let segments = offsets.len().saturating_sub(1);
    if segments != expected_segments {
        return Err(Error::new(
            ErrorKind::InvalidSegmentCount,
            format_args!("got {segments} want {expected_segments}"),
        ));
    }
    let pixels = rows.checked_mul(columns);
    let frame_size = pixels.and_then(|p| p.checked_mul(expected_segments as i64));
    let (Some(pixels), Some(frame_size)) = (pixels, frame_size) else {
        return Err(Error::new(
            ErrorKind::InvalidSegmentCount,
            format_args!(
                "rows={rows} columns={columns} samples_per_pixel={samples_per_pixel} bits_allocated={bits_allocated}"
            ),
        ));
    };
    let pixels = pixels as usize;
    for (segment, range) in offsets.windows(2).enumerate() {
        let &[start, end] = range else { continue };
        let (start, end) = (start as usize, end as usize);
        if end < start {
            return Err(Error::new(
                ErrorKind::InvalidSegmentOffset,
                format_args!("segment {segment} offset range {start}..{end}"),
            ));
        }
        let bytes = end.saturating_sub(start);
        if !tolerant && !can_fill(bytes, pixels) {
            return Err(Error::new(
                ErrorKind::SegmentDecodeFailed,
                format_args!("segment {segment}: {bytes} bytes cannot decode to {pixels}"),
            ));
        }
    }
    let mut out = vec![0u8; frame_size as usize];
    let step = expected_segments;
    let mut recovered = false;
    for (segment, data) in segment_data(fragment, &offsets) {
        let sample = segment.checked_div(bytes_per_sample).unwrap_or(0);
        let offset = byte_offset(segment, bytes_per_sample, recovery.reversed_byte_segments);
        recovered |= unpack_bits(
            data,
            &mut out,
            sample
                .saturating_mul(bytes_per_sample)
                .saturating_add(offset),
            step,
            pixels,
            tolerant,
        )
        .map_err(|e| e.with_segment(segment))?;
    }
    Ok((out, recovered))
}
fn segment_data<'a>(
    fragment: &'a [u8],
    offsets: &'a [u32],
) -> impl Iterator<Item = (usize, &'a [u8])> + 'a {
    offsets.windows(2).enumerate().map(|(segment, range)| {
        let data = match *range {
            [start, end] => fragment.get(start as usize..end as usize),
            _ => None,
        };
        (segment, data.unwrap_or_default())
    })
}
pub fn decode(
    fragments: &[&[u8]],
    offset_table: &[u8],
    m: &Metadata,
    recovery: Recovery,
) -> Result<Frames, Error> {
    validate_decode_metadata(m)?;
    let expected_segments =
        usize::from(m.samples_per_pixel).saturating_mul(usize::from(m.bits_allocated)) / 8;
    let frames = group_frames(
        fragments,
        offset_table,
        m.number_of_frames,
        expected_segments,
    )?;
    let mut overrun = false;
    let mut data = Vec::with_capacity(frames.len());
    for (i, fragment) in frames.iter().enumerate() {
        let (frame, recovered) = decode_frame(
            fragment,
            m.rows.into(),
            m.columns.into(),
            m.samples_per_pixel.into(),
            m.bits_allocated.into(),
            recovery,
        )
        .map_err(|e| e.with_frame(i))?;
        overrun |= recovered;
        data.push(frame);
    }
    let mut diagnostics = Vec::new();
    if overrun {
        diagnostics.push(DIAGNOSTIC_SEGMENT_OVERRUN);
    }
    if recovery.reversed_byte_segments && m.bits_allocated > 8 {
        diagnostics.push(DIAGNOSTIC_REVERSED_BYTE_SEGMENTS);
    }
    Ok(Frames {
        rows: m.rows.into(),
        columns: m.columns.into(),
        data,
        diagnostics,
    })
}
fn region_offsets(
    codestream: &[u8],
    segments: usize,
    stop: usize,
    tolerant: bool,
    bare: bool,
) -> Result<Vec<u32>, Error> {
    let offsets = parse_header(codestream)?;
    let got = offsets.len().saturating_sub(1);
    if got != segments {
        return Err(if bare {
            Error::bare(ErrorKind::InvalidSegmentCount)
        } else {
            Error::new(
                ErrorKind::InvalidSegmentCount,
                format_args!("got {got} want {segments}"),
            )
        });
    }
    for (segment, range) in offsets.windows(2).enumerate() {
        let &[start, end] = range else { continue };
        let (start, end) = (start as usize, end as usize);
        if end < start || end > codestream.len() {
            return Err(if bare {
                Error::bare(ErrorKind::InvalidSegmentOffset)
            } else {
                Error::new(
                    ErrorKind::InvalidSegmentOffset,
                    format_args!("segment {segment} offset range {start}..{end}"),
                )
            });
        }
        let bytes = end.saturating_sub(start);
        if !tolerant && !can_fill(bytes, stop) {
            return Err(if bare {
                Error::bare(ErrorKind::SegmentDecodeFailed)
            } else {
                Error::new(
                    ErrorKind::SegmentDecodeFailed,
                    format_args!("segment {segment}: {bytes} bytes cannot decode to {stop}"),
                )
            });
        }
    }
    Ok(offsets)
}
#[doc(hidden)]
pub fn decode_region(
    codestream: &[u8],
    m: &Metadata,
    region: Rect,
    recovery: Recovery,
) -> Result<Frames, Error> {
    validate_decode_metadata(m)?;
    let (rows, columns) = (i64::from(m.rows), i64::from(m.columns));
    if region.is_empty() || !region.within(columns, rows) {
        return Err(Error::new(
            ErrorKind::InvalidRegion,
            format_args!("{region} in {columns}x{rows}"),
        ));
    }
    let tolerant = recovery.segment_overrun;
    let bytes_per_sample = usize::from(m.bits_allocated) / 8;
    let segments = usize::from(m.samples_per_pixel).saturating_mul(bytes_per_sample);
    let cols = columns as usize;
    let (x0, x1) = (region.x0 as usize, region.x1 as usize);
    let (width, height) = (region.dx() as usize, region.dy() as usize);
    let (skip, stop) = (
        (region.y0 as usize).saturating_mul(cols),
        (region.y1 as usize).saturating_mul(cols),
    );
    let offsets = region_offsets(codestream, segments, stop, tolerant, false)?;
    let step = segments;
    let row_bytes = width.saturating_mul(step);
    let mut out = vec![0u8; row_bytes.saturating_mul(height)];
    let mut band = Vec::with_capacity(stop.saturating_sub(skip));
    let mut recovered = false;
    for (segment, data) in segment_data(codestream, &offsets) {
        recovered |= decode_packbits_range(data, skip, stop, tolerant, &mut band)
            .map_err(|e| e.with_segment(segment))?;
        let sample = segment.checked_div(bytes_per_sample).unwrap_or(0);
        let offset = byte_offset(segment, bytes_per_sample, recovery.reversed_byte_segments);
        let first = sample
            .saturating_mul(bytes_per_sample)
            .saturating_add(offset);
        for (row_out, band_row) in out
            .chunks_exact_mut(row_bytes.max(1))
            .zip(band.chunks_exact(cols.max(1)))
        {
            let columns = band_row.get(x0..x1).unwrap_or_default();
            for (slot, &value) in row_out
                .get_mut(first..)
                .unwrap_or_default()
                .iter_mut()
                .step_by(step.max(1))
                .zip(columns)
            {
                *slot = value;
            }
        }
    }
    let mut diagnostics = Vec::new();
    if recovered {
        diagnostics.push(DIAGNOSTIC_SEGMENT_OVERRUN);
    }
    if recovery.reversed_byte_segments && bytes_per_sample > 1 {
        diagnostics.push(DIAGNOSTIC_REVERSED_BYTE_SEGMENTS);
    }
    Ok(Frames {
        rows: height,
        columns: width,
        data: vec![out],
        diagnostics,
    })
}
pub fn buffer_metadata(m: &Metadata, region: Rect, whole_frame: bool) -> Result<Metadata, Error> {
    let (rows, columns) = (i64::from(m.rows), i64::from(m.columns));
    if whole_frame && region != Rect::new(0, 0, columns, rows) {
        return Err(Error::bare(ErrorKind::InvalidRegion));
    }
    validate_decode_metadata(m)?;
    if region.is_empty() || !region.within(columns, rows) {
        return Err(Error::bare(ErrorKind::InvalidRegion));
    }
    let mut out = m.clone();
    out.rows = region.dy() as u16;
    out.columns = region.dx() as u16;
    out.number_of_frames = 1;
    if out.samples_per_pixel == 3 {
        out.planar_configuration = 0;
        out.planar_configuration_present = true;
    }
    Ok(out)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntoError {
    pub written: usize,
    pub error: Error,
}
impl std::fmt::Display for IntoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for IntoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.error)
    }
}
pub fn decode_frame_into(
    codestream: &[u8],
    m: &Metadata,
    region: Rect,
    whole_frame: bool,
    dst: &mut [u8],
    recovery: Recovery,
) -> Result<(usize, Vec<&'static str>), IntoError> {
    let fail = |written: usize, error: Error| IntoError { written, error };
    let out_metadata = buffer_metadata(m, region, whole_frame).map_err(|e| fail(0, e))?;
    let bytes_per_sample = usize::from(m.bits_allocated.wrapping_add(7) / 8);
    let samples = usize::from(m.samples_per_pixel);
    let required = usize::from(out_metadata.rows)
        .saturating_mul(usize::from(out_metadata.columns))
        .saturating_mul(usize::from(out_metadata.samples_per_pixel))
        .saturating_mul(usize::from(out_metadata.bits_allocated.wrapping_add(7) / 8));
    let have = dst.len();
    let Some(dst) = dst.get_mut(..required) else {
        return Err(fail(
            0,
            Error::new(
                ErrorKind::BufferTooSmall,
                format_args!("need {required} bytes, have {have}"),
            ),
        ));
    };
    let segments = samples.saturating_mul(bytes_per_sample);
    let tolerant = recovery.segment_overrun;
    let reversed = recovery.reversed_byte_segments;
    let cols = usize::from(m.columns);
    let (skip, stop) = (
        (region.y0 as usize).saturating_mul(cols),
        (region.y1 as usize).saturating_mul(cols),
    );
    let offsets =
        region_offsets(codestream, segments, stop, tolerant, true).map_err(|e| fail(0, e))?;
    let step = segments;
    let mut recovered = false;
    if whole_frame {
        dst.fill(0);
        let plane = usize::from(m.rows).saturating_mul(cols);
        for (segment, data) in segment_data(codestream, &offsets) {
            let first = segment
                .checked_div(bytes_per_sample)
                .unwrap_or(0)
                .saturating_mul(bytes_per_sample)
                .saturating_add(byte_offset(segment, bytes_per_sample, reversed));
            recovered |= unpack_bits(data, dst, first, step, plane, tolerant)
                .map_err(|e| fail(required, e))?;
        }
    } else {
        let (x0, x1) = (region.x0 as usize, region.x1 as usize);
        let row_bytes = (region.dx() as usize).saturating_mul(step);
        let mut band = Vec::with_capacity(stop.saturating_sub(skip));
        let mut highwater = 0usize;
        for (segment, data) in segment_data(codestream, &offsets) {
            recovered |= decode_packbits_range(data, skip, stop, tolerant, &mut band)
                .map_err(|e| fail(highwater, e.with_segment(segment)))?;
            let sample = segment.checked_div(bytes_per_sample).unwrap_or(0);
            let offset = byte_offset(segment, bytes_per_sample, reversed);
            let first = sample
                .saturating_mul(bytes_per_sample)
                .saturating_add(offset);
            for (y, (row_out, band_row)) in dst
                .chunks_exact_mut(row_bytes.max(1))
                .zip(band.chunks_exact(cols.max(1)))
                .enumerate()
            {
                let columns = band_row.get(x0..x1).unwrap_or_default();
                let count = row_out
                    .get_mut(first..)
                    .unwrap_or_default()
                    .iter_mut()
                    .step_by(step.max(1))
                    .zip(columns)
                    .map(|(slot, &value)| *slot = value)
                    .count();
                if let Some(last) = count.checked_sub(1) {
                    let end = y
                        .saturating_mul(row_bytes)
                        .saturating_add(first)
                        .saturating_add(last.saturating_mul(step))
                        .saturating_add(1);
                    highwater = highwater.max(end);
                }
            }
        }
    }
    let mut diagnostics = Vec::new();
    if recovered {
        diagnostics.push(DIAGNOSTIC_SEGMENT_OVERRUN);
    }
    if reversed && bytes_per_sample > 1 {
        diagnostics.push(DIAGNOSTIC_REVERSED_BYTE_SEGMENTS);
    }
    Ok((required, diagnostics))
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderCapabilities {
    pub transfer_syntax_uid: &'static str,
    pub bits_allocated: Vec<u16>,
    pub pixel_representations: Vec<u16>,
    pub samples_per_pixel: Vec<u16>,
    pub photometric_interpretations: Vec<&'static str>,
    pub lossless: bool,
    pub supports_multi_frame: bool,
    pub backend: &'static str,
}
pub fn capabilities() -> EncoderCapabilities {
    EncoderCapabilities {
        transfer_syntax_uid: UID,
        bits_allocated: (1..=15u16).map(|n| n.saturating_mul(8)).collect(),
        pixel_representations: vec![0, 1],
        samples_per_pixel: vec![1, 3],
        photometric_interpretations: vec![
            "MONOCHROME1",
            "MONOCHROME2",
            "PALETTE COLOR",
            "RGB",
            "YBR_FULL",
        ],
        lossless: true,
        supports_multi_frame: true,
        backend: "pure-rust",
    }
}
fn unsupported(field: &str) -> Error {
    Error::new(ErrorKind::UnsupportedEncoderMetadata, field)
}
struct EncoderLayout {
    bytes_per_sample: usize,
    segment_count: usize,
    frame_length: u64,
    max_encoded_length: u64,
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn validate_encoder_input(m: &Metadata) -> Result<EncoderLayout, Error> {
    if m.rows == 0 {
        return Err(unsupported("Rows"));
    }
    if m.columns == 0 {
        return Err(unsupported("Columns"));
    }
    if m.samples_per_pixel != 1 && m.samples_per_pixel != 3 {
        return Err(unsupported("SamplesPerPixel"));
    }
    if !supported_bits_allocated(m.bits_allocated.into(), m.samples_per_pixel.into()) {
        return Err(unsupported("BitsAllocated"));
    }
    if m.bits_stored == 0 || m.bits_stored > m.bits_allocated {
        return Err(unsupported("BitsStored"));
    }
    if m.high_bit != m.bits_stored - 1 {
        return Err(unsupported("HighBit"));
    }
    if m.pixel_representation > 1 {
        return Err(unsupported("PixelRepresentation"));
    }
    if m.number_of_frames < 1 {
        return Err(unsupported("NumberOfFrames"));
    }
    let photometric = trim_upper(&m.photometric_interpretation);
    if m.samples_per_pixel == 1 {
        if !matches!(
            photometric.as_str(),
            "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR"
        ) {
            return Err(unsupported("PhotometricInterpretation"));
        }
        if photometric == "PALETTE COLOR" && m.pixel_representation != 0 {
            return Err(unsupported("PixelRepresentation"));
        }
    } else {
        if photometric != "RGB" && photometric != "YBR_FULL" {
            return Err(unsupported("PhotometricInterpretation"));
        }
        if m.pixel_representation != 0 {
            return Err(unsupported("PixelRepresentation"));
        }
        if !m.planar_configuration_present || m.planar_configuration != 0 {
            return Err(unsupported("PlanarConfiguration"));
        }
    }
    let bytes_per_sample = usize::from(m.bits_allocated / 8);
    let segment_count = usize::from(m.samples_per_pixel) * bytes_per_sample;
    let (rows, columns) = (u64::from(m.rows), u64::from(m.columns));
    let frame_length = rows * columns * segment_count as u64;
    let worst_row = columns + columns.div_ceil(128);
    let mut worst_segment = rows * worst_row;
    if worst_segment % 2 != 0 {
        worst_segment += 1;
    }
    let max_encoded_length = HEADER_LENGTH as u64 + segment_count as u64 * worst_segment;
    if max_encoded_length > MAX_FRAGMENT_LENGTH || max_encoded_length > isize::MAX as u64 {
        return Err(Error::new(
            ErrorKind::EncoderOutputInvalid,
            "encoded frame length",
        ));
    }
    Ok(EncoderLayout {
        bytes_per_sample,
        segment_count,
        frame_length,
        max_encoded_length,
    })
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn append_literals(dst: &mut Vec<u8>, mut literals: &[u8]) {
    while !literals.is_empty() {
        let length = literals.len().min(128);
        dst.push((length - 1) as u8);
        dst.extend_from_slice(&literals[..length]);
        literals = &literals[length..];
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn append_packbits_row(dst: &mut Vec<u8>, row: &[u8]) {
    let mut literal_start = 0;
    let mut i = 0;
    while i < row.len() {
        let mut run_end = i + 1;
        while run_end < row.len() && row[run_end] == row[i] {
            run_end += 1;
        }
        let mut run = run_end - i;
        if run < 3 {
            i = run_end;
            continue;
        }
        append_literals(dst, &row[literal_start..i]);
        while run > 128 {
            dst.extend_from_slice(&[0x81, row[i]]);
            i += 128;
            run -= 128;
        }
        if run >= 3 {
            dst.extend_from_slice(&[(1 - run as i32) as i8 as u8, row[i]]);
            i += run;
            literal_start = i;
            continue;
        }
        literal_start = i;
        i += run;
    }
    append_literals(dst, &row[literal_start..]);
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn encode_frame(frame: &[u8], m: &Metadata) -> Result<Vec<u8>, Error> {
    let layout = validate_encoder_input(m)?;
    if frame.len() as u64 != layout.frame_length {
        return Err(Error::new(
            ErrorKind::PixelDataSizeMismatch,
            "native frame length",
        ));
    }
    let bytes_per_sample = layout.bytes_per_sample;
    let stride = layout.segment_count;
    let (rows, columns) = (usize::from(m.rows), usize::from(m.columns));
    let mut encoded = Vec::with_capacity(layout.max_encoded_length as usize);
    encoded.resize(HEADER_LENGTH, 0);
    encoded[0..4].copy_from_slice(&(layout.segment_count as u32).to_le_bytes());
    let mut row_bytes = vec![0u8; columns];
    for sample in 0..usize::from(m.samples_per_pixel) {
        for plane in (0..bytes_per_sample).rev() {
            let segment = sample * bytes_per_sample + bytes_per_sample - 1 - plane;
            let offset = (encoded.len() as u32).to_le_bytes();
            encoded[4 + segment * 4..8 + segment * 4].copy_from_slice(&offset);
            for y in 0..rows {
                if stride == 1 {
                    append_packbits_row(&mut encoded, &frame[y * columns..(y + 1) * columns]);
                } else {
                    let start = y * columns * stride + sample * bytes_per_sample + plane;
                    for (x, byte) in row_bytes.iter_mut().enumerate() {
                        *byte = frame[start + x * stride];
                    }
                    append_packbits_row(&mut encoded, &row_bytes);
                }
            }
            if encoded.len() % 2 != 0 {
                encoded.push(0);
            }
        }
    }
    Ok(encoded)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn mono8(rows: u16, columns: u16) -> Metadata {
        Metadata {
            rows,
            columns,
            samples_per_pixel: 1,
            bits_allocated: 8,
            bits_stored: 8,
            high_bit: 7,
            number_of_frames: 1,
            photometric_interpretation: "MONOCHROME2".into(),
            ..Metadata::default()
        }
    }
    #[test]
    fn packbits_run_boundaries() {
        for (n, want) in [
            (2usize, vec![0x01, 7, 7, 0]),
            (3, vec![0xfe, 7]),
            (128, vec![0x81, 7]),
            (129, vec![0x81, 7, 0x00, 7]),
            (130, vec![0x81, 7, 0x01, 7, 7, 0]),
        ] {
            let encoded = encode_frame(&vec![7; n], &mono8(1, n as u16)).unwrap();
            assert_eq!(encoded[64..], want[..], "run of {n}");
        }
    }
    #[test]
    fn round_trip_rgb16() {
        let mut m = mono8(3, 5);
        m.samples_per_pixel = 3;
        m.bits_allocated = 16;
        m.bits_stored = 16;
        m.high_bit = 15;
        m.planar_configuration_present = true;
        m.photometric_interpretation = "RGB".into();
        let pixels: Vec<u8> = (0..90u32).map(|i| (i * 37 % 7) as u8).collect();
        let encoded = encode_frame(&pixels, &m).unwrap();
        let frames = decode(&[&encoded], &[], &m, Recovery::default()).unwrap();
        assert_eq!(frames.data, vec![pixels.clone()]);
        let region =
            decode_region(&encoded, &m, Rect::new(1, 1, 4, 3), Recovery::default()).unwrap();
        let mut dst = vec![0xA5; 36];
        let (n, diagnostics) = decode_frame_into(
            &encoded,
            &m,
            Rect::new(1, 1, 4, 3),
            false,
            &mut dst,
            Recovery::default(),
        )
        .unwrap();
        assert_eq!((n, diagnostics.len()), (36, 0));
        assert_eq!(region.data[0], dst);
    }
    #[test]
    fn malformed_headers_keep_go_texts() {
        let mut fragment = vec![0u8; 64];
        fragment[0] = 16;
        let err = decode(&[&fragment], &[], &mono8(2, 2), Recovery::default()).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidSegmentCount);
        assert_eq!(
            err.to_string(),
            "decode RLE frame 0: dicom: invalid RLE segment count: got 16"
        );
    }
    #[test]
    fn overrun_recovery_is_opt_in() {
        let mut fragment = vec![0u8; 64];
        fragment[0] = 1;
        fragment[4] = 64;
        fragment.extend_from_slice(&[3, 1, 2, 3]);
        let m = mono8(2, 2);
        assert!(decode(&[&fragment], &[], &m, Recovery::default()).is_err());
        let recovery = Recovery {
            segment_overrun: true,
            ..Recovery::default()
        };
        let frames = decode(&[&fragment], &[], &m, recovery).unwrap();
        assert_eq!(frames.data, vec![vec![1, 2, 3, 0]]);
        assert_eq!(frames.diagnostics, vec![DIAGNOSTIC_SEGMENT_OVERRUN]);
    }
}
