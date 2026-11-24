use super::Metadata;
use super::encapsulated::{self, Format, Limits, Recovery, TableAttribute, TableValue};
#[cfg(feature = "jpeg")]
use crate::codec::jpeg;
#[cfg(feature = "jpeg2000")]
use crate::codec::jpeg2000;
#[cfg(feature = "jpeg-lossless")]
use crate::codec::jpeglossless;
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::codec::{common, rle};
use crate::limits::Limits as ResourceLimits;
use crate::object::transfer::{self, Syntax};
use crate::object::{Element, Object, Tag, Value, Vr};
use crate::textfmt;
use std::fmt;
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_PIXEL_DATA_PROVIDER_URL: Tag = Tag::new(0x0028, 0x7FE0);
pub(super) const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const ENCAPSULATED_UNCOMPRESSED: &str = "1.2.840.10008.1.2.1.98";
const DEFLATED_IMAGE_FRAME: &str = "1.2.840.10008.1.2.8.1";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelErrorKind {
    PixelDataNotFound,
    MissingMetadata,
    InvalidMetadata,
    CodecNotFound,
    CodecDecodeFailed,
    IncompatiblePixelData,
    EncapsulatedPixelData,
    PixelDataSizeMismatch,
    JpipRetrievalRequired,
    MediaPayloadNotRenderable,
    UnsupportedValueType,
    Display,
    Frame,
    NoCodecBackend,
    Other,
    ResourceLimit,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelError {
    pub kind: PixelErrorKind,
    text: String,
    cause: Option<PixelErrorKind>,
}
impl PixelError {
    pub(super) fn new(kind: PixelErrorKind, text: impl Into<String>) -> PixelError {
        PixelError {
            kind,
            text: text.into(),
            cause: None,
        }
    }
    fn base(kind: PixelErrorKind) -> &'static str {
        match kind {
            PixelErrorKind::PixelDataNotFound => "dicom: pixel data element not found",
            PixelErrorKind::MissingMetadata => "dicom: missing required pixel data metadata",
            PixelErrorKind::InvalidMetadata => "dicom: invalid pixel data metadata",
            PixelErrorKind::CodecNotFound => {
                "dicom: no pixel data codec registered for transfer syntax"
            }
            PixelErrorKind::CodecDecodeFailed => "dicom: pixel data codec decode failed",
            PixelErrorKind::IncompatiblePixelData => {
                "dicom: pixel data is incompatible with transfer syntax"
            }
            PixelErrorKind::EncapsulatedPixelData => {
                "dicom: native frame extraction does not support encapsulated pixel data"
            }
            PixelErrorKind::PixelDataSizeMismatch => {
                "dicom: pixel data size does not match metadata"
            }
            PixelErrorKind::JpipRetrievalRequired => {
                "dicom: JPIP pixel data retrieval pipeline required"
            }
            PixelErrorKind::MediaPayloadNotRenderable => {
                "dicom: media payload is not renderable as still-image frames"
            }
            PixelErrorKind::UnsupportedValueType => {
                "dicom: pixel data is not a supported value type"
            }
            PixelErrorKind::Display
            | PixelErrorKind::Frame
            | PixelErrorKind::NoCodecBackend
            | PixelErrorKind::ResourceLimit
            | PixelErrorKind::Other => "",
        }
    }
    fn bare(kind: PixelErrorKind) -> PixelError {
        PixelError::new(kind, PixelError::base(kind))
    }
    fn detail(kind: PixelErrorKind, detail: impl fmt::Display) -> PixelError {
        PixelError::new(kind, format!("{}: {detail}", PixelError::base(kind)))
    }
}
impl PixelError {
    pub fn custom(kind: PixelErrorKind, text: impl Into<String>) -> PixelError {
        PixelError::new(kind, text)
    }
    pub fn is(&self, kind: PixelErrorKind) -> bool {
        self.kind == kind || self.cause == Some(kind)
    }
    pub fn cause(&self) -> Option<PixelErrorKind> {
        self.cause
    }
}
impl fmt::Display for PixelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for PixelError {}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelData<'a> {
    Native(&'a [u8]),
    Encapsulated {
        offset_table: &'a [u8],
        fragments: &'a [Vec<u8>],
    },
}
impl PixelData<'_> {
    fn encapsulated(&self) -> bool {
        matches!(self, PixelData::Encapsulated { .. })
    }
    fn fragments(&self) -> &[Vec<u8>] {
        match self {
            PixelData::Encapsulated { fragments, .. } => fragments,
            PixelData::Native(_) => &[],
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frames {
    pub rows: usize,
    pub columns: usize,
    pub data: Vec<Vec<u8>>,
    pub photometric_interpretation: String,
    pub diagnostics: Vec<&'static str>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeFrames {
    pub metadata: Metadata,
    pub data: Vec<Vec<u8>>,
}
pub fn extract_view(obj: &Object) -> Result<PixelData<'_>, PixelError> {
    let elem = obj
        .get(TAG_PIXEL_DATA)
        .ok_or_else(|| PixelError::bare(PixelErrorKind::PixelDataNotFound))?;
    pixel_data_view(elem)
}
fn pixel_data_view(elem: &Element) -> Result<PixelData<'_>, PixelError> {
    match &elem.value {
        Value::Raw(raw) => Ok(PixelData::Native(raw)),
        Value::Fragments {
            offset_table,
            fragments,
        } => Ok(PixelData::Encapsulated {
            offset_table,
            fragments,
        }),
        _ => Err(PixelError::bare(PixelErrorKind::UnsupportedValueType)),
    }
}
fn get_uint16(obj: &Object, tag: Tag) -> Option<u16> {
    let elem = obj.get(tag)?;
    if elem.vr() != Vr::US {
        return None;
    }
    let pair: [u8; 2] = elem.raw()?.try_into().ok()?;
    Some(if obj.value_big_endian() {
        u16::from_be_bytes(pair)
    } else {
        u16::from_le_bytes(pair)
    })
}
fn missing(name: &str, tag: Tag) -> PixelError {
    PixelError::detail(
        PixelErrorKind::MissingMetadata,
        format_args!("{name} ({tag})"),
    )
}
fn number_of_frames(obj: &Object) -> Result<i64, PixelError> {
    let value = obj.get_string(TAG_NUMBER_OF_FRAMES).unwrap_or_default();
    let v = value.trim();
    if v.is_empty() {
        return Ok(1);
    }
    match crate::numtext::parse_int(v) {
        Ok(n) if n > 0 && n <= i64::from(i32::MAX) => Ok(n),
        _ => Err(PixelError::detail(
            PixelErrorKind::InvalidMetadata,
            "invalid NumberOfFrames",
        )),
    }
}
pub fn extract_metadata(obj: &Object) -> Result<Metadata, PixelError> {
    let rows = get_uint16(obj, TAG_ROWS).ok_or_else(|| missing("Rows", TAG_ROWS))?;
    let columns = get_uint16(obj, TAG_COLUMNS).ok_or_else(|| missing("Columns", TAG_COLUMNS))?;
    let samples_per_pixel = get_uint16(obj, TAG_SAMPLES_PER_PIXEL)
        .ok_or_else(|| missing("SamplesPerPixel", TAG_SAMPLES_PER_PIXEL))?;
    let bits_allocated = get_uint16(obj, TAG_BITS_ALLOCATED)
        .ok_or_else(|| missing("BitsAllocated", TAG_BITS_ALLOCATED))?;
    let bits_stored =
        get_uint16(obj, TAG_BITS_STORED).ok_or_else(|| missing("BitsStored", TAG_BITS_STORED))?;
    let high_bit = get_uint16(obj, TAG_HIGH_BIT).ok_or_else(|| missing("HighBit", TAG_HIGH_BIT))?;
    let pixel_representation = get_uint16(obj, TAG_PIXEL_REPRESENTATION)
        .ok_or_else(|| missing("PixelRepresentation", TAG_PIXEL_REPRESENTATION))?;
    let planar = get_uint16(obj, TAG_PLANAR_CONFIGURATION);
    let photometric = obj.get_string(TAG_PHOTOMETRIC);
    let trimmed = photometric
        .as_deref()
        .unwrap_or("")
        .trim_matches([' ', '\0'])
        .to_owned();
    if photometric.is_none() || trimmed.is_empty() {
        return Err(missing("PhotometricInterpretation", TAG_PHOTOMETRIC));
    }
    let number_of_frames = number_of_frames(obj)?;
    Ok(Metadata {
        rows,
        columns,
        samples_per_pixel,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        planar_configuration: planar.unwrap_or(0),
        planar_configuration_present: planar.is_some(),
        number_of_frames,
        photometric_interpretation: trimmed,
    })
}
pub(super) fn normalized_photometric(value: &str) -> String {
    textfmt::to_upper(value)
        .trim_matches([' ', '\0'])
        .to_owned()
}
fn bytes_per_sample(m: &Metadata) -> i64 {
    i64::from(m.bits_allocated.wrapping_add(7) / 8)
}
#[allow(clippy::arithmetic_side_effects)]
pub fn frame_size(m: &Metadata) -> i64 {
    let photometric = normalized_photometric(&m.photometric_interpretation);
    if (photometric == "YBR_FULL_422" || photometric == "YBR_PARTIAL_422") && m.bits_allocated >= 8
    {
        return i64::from(m.rows) * i64::from(m.columns) * 2 * bytes_per_sample(m);
    }
    let samples = i64::from(m.rows) * i64::from(m.columns) * i64::from(m.samples_per_pixel);
    if m.bits_allocated < 8 {
        return (samples * i64::from(m.bits_allocated) + 7) / 8;
    }
    samples * bytes_per_sample(m)
}
pub fn total_size(m: &Metadata) -> i64 {
    let frame = frame_size(m);
    let frames = m.number_of_frames;
    if frame <= 0 || frames <= 0 {
        return 0;
    }
    frame.checked_mul(frames).unwrap_or(0)
}
#[allow(clippy::arithmetic_side_effects)]
pub(super) fn packed_frame_bits(m: &Metadata) -> i64 {
    i64::from(m.rows) * i64::from(m.columns) * i64::from(m.samples_per_pixel)
}
fn checked_product(values: &[i64]) -> Option<i64> {
    values
        .iter()
        .try_fold(1i64, |p, &v| if v <= 0 { None } else { p.checked_mul(v) })
}
pub(crate) fn native_packed_byte_count(m: &Metadata) -> i64 {
    let bits = checked_product(&[packed_frame_bits(m), m.number_of_frames]).unwrap_or(0);
    (bits / 8).saturating_add(i64::from(bits % 8 != 0))
}
pub(super) fn copy_packed_bits(
    dst: &mut [u8],
    mut dst_bit: i64,
    src: &[u8],
    mut src_bit: i64,
    mut count: i64,
) {
    if dst_bit % 8 == 0 && src_bit % 8 == 0 && count >= 8 {
        let whole = count / 8;
        let (d, s, n) = (
            (dst_bit / 8) as usize,
            (src_bit / 8) as usize,
            whole as usize,
        );
        if let (Some(to), Some(from)) = (
            dst.get_mut(d..d.saturating_add(n)),
            src.get(s..s.saturating_add(n)),
        ) {
            to.copy_from_slice(from);
        }
        let bits = whole.saturating_mul(8);
        dst_bit = dst_bit.saturating_add(bits);
        src_bit = src_bit.saturating_add(bits);
        count = count.saturating_sub(bits);
    }
    let byte = |i: i64| src.get((i / 8) as usize).copied().unwrap_or(0);
    while count > 0 {
        let (dst_shift, src_shift) = (dst_bit % 8, src_bit % 8);
        let width = 8i64.saturating_sub(dst_shift).min(count);
        let mut value = byte(src_bit) >> src_shift;
        if src_shift.saturating_add(width) > 8 {
            value |= byte(src_bit.saturating_add(8)) << 8i64.saturating_sub(src_shift);
        }
        let mask = ((1u16 << width).wrapping_sub(1) as u8) << dst_shift;
        if let Some(d) = dst.get_mut((dst_bit / 8) as usize) {
            *d = *d & !mask | (value << dst_shift) & mask;
        }
        dst_bit = dst_bit.saturating_add(width);
        src_bit = src_bit.saturating_add(width);
        count = count.saturating_sub(width);
    }
}
fn invalid_layout(m: &Metadata) -> PixelError {
    PixelError::detail(
        PixelErrorKind::InvalidMetadata,
        format_args!(
            "rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
            m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
        ),
    )
}
pub fn extract_native_frames(obj: &Object) -> Result<NativeFrames, PixelError> {
    let pixel = extract_view(obj)?;
    let (metadata, data) = split_native_frames(&pixel, obj, obj.value_big_endian())?;
    Ok(NativeFrames { metadata, data })
}
fn split_native_frames(
    pixel: &PixelData<'_>,
    obj: &Object,
    big_endian: bool,
) -> Result<(Metadata, Vec<Vec<u8>>), PixelError> {
    let metadata = extract_metadata(obj)?;
    let frames = split_native_frame_range(pixel, obj, big_endian, &metadata)?;
    Ok((metadata, frames))
}
pub(super) fn split_native_frame_range(
    pixel: &PixelData<'_>,
    obj: &Object,
    big_endian: bool,
    m: &Metadata,
) -> Result<Vec<Vec<u8>>, PixelError> {
    let PixelData::Native(raw) = *pixel else {
        return Err(PixelError::bare(PixelErrorKind::EncapsulatedPixelData));
    };
    let frame = frame_size(m);
    let buffers = total_size(m);
    let total = if m.bits_allocated == 1 {
        native_packed_byte_count(m)
    } else {
        buffers
    };
    if frame <= 0 || total <= 0 || buffers <= 0 {
        return Err(invalid_layout(m));
    }
    let actual = raw.len() as i64;
    let excess = actual.saturating_sub(total);
    if excess < 0 || (excess >= frame && !(total % 2 == 1 && excess == 1)) {
        return Err(PixelError::detail(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!(
                "expected {total} bytes for {} frame(s), got {}",
                m.number_of_frames,
                raw.len()
            ),
        ));
    }
    let swapped;
    let mut raw: &[u8] = raw;
    let byte_lanes = big_endian
        && obj.get(TAG_PIXEL_DATA).is_some_and(|e| e.vr() == Vr::OW)
        && m.bits_allocated <= 8;
    if byte_lanes {
        if !raw.len().is_multiple_of(2) {
            return Err(PixelError::detail(
                PixelErrorKind::PixelDataSizeMismatch,
                "Big Endian OW Pixel Data has an odd byte length",
            ));
        }
        let n = total.saturating_add(total % 2) as usize;
        swapped = raw
            .get(..n)
            .unwrap_or(raw)
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[1], p[0]])
            .collect::<Vec<u8>>();
        raw = &swapped;
    }
    let count = m.number_of_frames as usize;
    let frame = frame as usize;
    if m.bits_allocated == 1 && m.number_of_frames > 1 {
        let frame_bits = packed_frame_bits(m);
        if frame_bits % 8 != 0 {
            return Ok((0..count)
                .map(|i| {
                    let mut out = vec![0u8; frame];
                    let at = (i as i64).saturating_mul(frame_bits);
                    copy_packed_bits(&mut out, 0, raw, at, frame_bits);
                    out
                })
                .collect());
        }
    }
    Ok(raw
        .chunks_exact(frame)
        .take(count)
        .map(<[u8]>::to_vec)
        .collect())
}
fn split_encapsulated_uncompressed(
    pixel: &PixelData<'_>,
    obj: &Object,
) -> Result<(Metadata, Vec<Vec<u8>>), PixelError> {
    let m = extract_metadata(obj)?;
    let PixelData::Encapsulated { fragments, .. } = *pixel else {
        return Err(PixelError::detail(
            PixelErrorKind::IncompatiblePixelData,
            format_args!(
                "transfer syntax {} expects encapsulated pixel data",
                textfmt::quote(ENCAPSULATED_UNCOMPRESSED)
            ),
        ));
    };
    let frame = frame_size(&m);
    let total = total_size(&m);
    if frame <= 0 || total <= 0 {
        return Err(invalid_layout(&m));
    }
    let count = m.number_of_frames as usize;
    if fragments.is_empty() {
        return Err(PixelError::detail(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!(
                "expected {total} bytes for {} frame(s), got 0 bytes across 0 fragments",
                m.number_of_frames
            ),
        ));
    }
    let frame_len = frame as usize;
    if fragments.len() == count
        && fragments.iter().all(|f| {
            f.len() == frame_len
                || !frame_len.is_multiple_of(2) && f.len() == frame_len.saturating_add(1)
        })
    {
        let frames = fragments
            .iter()
            .map(|f| f.get(..frame_len).unwrap_or(f).to_vec())
            .collect();
        return Ok((m, frames));
    }
    let bytes: i64 = fragments
        .iter()
        .map(|f| f.len() as i64)
        .fold(0, i64::saturating_add);
    if !(bytes == total || total % 2 == 1 && bytes == total.saturating_add(1)) {
        return Err(PixelError::detail(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!(
                "expected {total} bytes for {} frame(s), got {bytes} bytes across {} fragments",
                m.number_of_frames,
                fragments.len()
            ),
        ));
    }
    let assembled: Vec<u8> = fragments.concat();
    let frames = assembled
        .chunks_exact(frame_len)
        .take(count)
        .map(<[u8]>::to_vec)
        .collect();
    Ok((m, frames))
}
fn is_jpip(uid: &str) -> bool {
    matches!(
        uid,
        "1.2.840.10008.1.2.4.94"
            | "1.2.840.10008.1.2.4.95"
            | "1.2.840.10008.1.2.4.204"
            | "1.2.840.10008.1.2.4.205"
    )
}
pub fn is_video_transfer_syntax(uid: &str) -> bool {
    matches!(
        transfer::normalize_uid(uid),
        "1.2.840.10008.1.2.4.100"
            | "1.2.840.10008.1.2.4.100.1"
            | "1.2.840.10008.1.2.4.101"
            | "1.2.840.10008.1.2.4.101.1"
            | "1.2.840.10008.1.2.4.102"
            | "1.2.840.10008.1.2.4.102.1"
            | "1.2.840.10008.1.2.4.103"
            | "1.2.840.10008.1.2.4.103.1"
            | "1.2.840.10008.1.2.4.104"
            | "1.2.840.10008.1.2.4.104.1"
            | "1.2.840.10008.1.2.4.105"
            | "1.2.840.10008.1.2.4.105.1"
            | "1.2.840.10008.1.2.4.106"
            | "1.2.840.10008.1.2.4.106.1"
            | "1.2.840.10008.1.2.4.107"
            | "1.2.840.10008.1.2.4.108"
    )
}
pub fn is_jpip_referenced_transfer_syntax(uid: &str) -> bool {
    is_jpip(transfer::normalize_uid(uid))
}
fn is_jpeg2000(uid: &str) -> bool {
    matches!(
        uid,
        "1.2.840.10008.1.2.4.90"
            | "1.2.840.10008.1.2.4.91"
            | "1.2.840.10008.1.2.4.92"
            | "1.2.840.10008.1.2.4.93"
            | "1.2.840.10008.1.2.4.201"
            | "1.2.840.10008.1.2.4.202"
            | "1.2.840.10008.1.2.4.203"
    )
}
fn is_jpeg_xl(uid: &str) -> bool {
    matches!(
        uid,
        "1.2.840.10008.1.2.4.110" | "1.2.840.10008.1.2.4.111" | "1.2.840.10008.1.2.4.112"
    )
}
fn syntax_suffix(uid: &str, name: Option<&str>) -> String {
    let mut s = format!(": transfer syntax {}", textfmt::quote(uid));
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        s.push_str(&format!(" ({name})"));
    }
    s
}
fn jpip_error(uid: &str, obj: &Object) -> PixelError {
    let url = obj
        .get_string(TAG_PIXEL_DATA_PROVIDER_URL)
        .map(|u| u.trim().to_owned())
        .unwrap_or_default();
    if url.is_empty() {
        return missing("PixelDataProviderURL", TAG_PIXEL_DATA_PROVIDER_URL);
    }
    if let Err(e) = number_of_frames(obj) {
        return e;
    }
    let mut text =
        String::from("dicom: JPIP referenced pixel stream requires external streaming retrieval");
    text.push_str(&syntax_suffix(uid, transfer::lookup(uid).map(|s| s.name)));
    text.push_str("; Pixel Data Provider URL present (redacted)");
    PixelError::new(PixelErrorKind::JpipRetrievalRequired, text)
}
pub(super) fn media_error(uid: &str, pixel: Option<&PixelData<'_>>) -> PixelError {
    let mut text =
        String::from("dicom: video media payload is not renderable as still-image frames");
    text.push_str(&syntax_suffix(uid, transfer::lookup(uid).map(|s| s.name)));
    let fragments = pixel.map_or(0, |p| p.fragments().len());
    if fragments > 0 {
        text.push_str(&format!("; fragments: {fragments}"));
    }
    text.push_str("; preserve encoded Pixel Data or use an external media pipeline");
    PixelError::new(PixelErrorKind::MediaPayloadNotRenderable, text)
}
pub fn registered_codec_uids() -> Vec<&'static str> {
    let mut uids = vec![rle::UID, DEFLATED_IMAGE_FRAME];
    #[cfg(feature = "jpeg")]
    uids.extend(jpeg::transfer_syntax_uids());
    #[cfg(feature = "jpeg-lossless")]
    uids.extend([jpeglossless::UID_PROCESS14, jpeglossless::UID_PROCESS14_SV1]);
    #[cfg(feature = "jpeg-ls")]
    uids.extend([jpegls::UID, jpegls::UID_NEAR_LOSSLESS]);
    #[cfg(feature = "jpeg2000")]
    uids.extend([
        jpeg2000::UID_LOSSLESS,
        jpeg2000::UID_LOSSY,
        jpeg2000::UID_PART2_LOSSLESS,
        jpeg2000::UID_PART2,
        jpeg2000::UID_HTJ2K_LOSSLESS,
        jpeg2000::UID_HTJ2K_LOSSLESS_RPCL,
        jpeg2000::UID_HTJ2K,
    ]);
    uids.sort_unstable();
    uids
}
fn codec_registration_hint(syntax: &Syntax) -> String {
    let uid = syntax.uid;
    if is_jpeg_xl(uid) {
        return "JPEG XL transfer syntax is recognized, but there is no default decoder adapter; metadata and encapsulated Pixel Data can be preserved, or decode frames with DecoderRegistry::BuiltinWithJpegXl or a Codec added with CodecRegistry::register_codec".into();
    }
    if is_video_transfer_syntax(uid) {
        return "video transfer syntax is recognized but video frames are not decoded or played; preserve encoded Pixel Data or use an external media pipeline".into();
    }
    if is_jpip(uid) {
        return "streaming/reference transfer syntax is recognized but external pixel streams are not fetched, inflated, or resolved; preserve metadata/reference values or use an external streaming pipeline".into();
    }
    if matches!(
        uid,
        "1.2.840.10008.1.2.7.1" | "1.2.840.10008.1.2.7.2" | "1.2.840.10008.1.2.7.3"
    ) {
        return "media transfer syntax is recognized but SMPTE ST 2110 video or audio streams are not decoded or played; use an external media pipeline".into();
    }
    if syntax.encapsulated {
        return "register a compatible Codec with CodecRegistry::register_codec".into();
    }
    String::new()
}
pub fn check_codec_availability(uid: &str) -> Result<(), PixelError> {
    let normalized = transfer::normalize_uid(uid);
    if registered_codec_uids().contains(&normalized) {
        return Ok(());
    }
    Err(codec_not_found(normalized))
}
pub fn builtin_codec_uids() -> Vec<&'static str> {
    #[cfg(not(feature = "jpeg2000"))]
    let adapter: [&str; 0] = [];
    #[cfg(feature = "jpeg2000")]
    let adapter = [
        jpeg2000::UID_LOSSLESS,
        jpeg2000::UID_LOSSY,
        jpeg2000::UID_PART2_LOSSLESS,
        jpeg2000::UID_PART2,
        jpeg2000::UID_HTJ2K_LOSSLESS,
        jpeg2000::UID_HTJ2K_LOSSLESS_RPCL,
        jpeg2000::UID_HTJ2K,
    ];
    registered_codec_uids()
        .into_iter()
        .filter(|uid| !adapter.contains(uid))
        .collect()
}
pub fn check_builtin_codec_availability(uid: &str) -> Result<(), PixelError> {
    let normalized = transfer::normalize_uid(uid);
    let registered = builtin_codec_uids();
    if registered.contains(&normalized) {
        return Ok(());
    }
    Err(codec_not_found_in(normalized, &registered))
}
fn codec_not_found(uid: &str) -> PixelError {
    codec_not_found_in(uid, &registered_codec_uids())
}
fn codec_not_found_in(uid: &str, registered: &[&'static str]) -> PixelError {
    let mut text = String::from(PixelError::base(PixelErrorKind::CodecNotFound));
    let syntax = transfer::lookup(uid);
    if !uid.is_empty() {
        text.push_str(&syntax_suffix(uid, syntax.map(|s| s.name)));
    }
    if let Some(syntax) = &syntax {
        let hint = if is_jpeg2000(syntax.uid) {
            "JPEG 2000 / HTJ2K transfer syntax is recognized, but there is no default decoder adapter; metadata and encapsulated Pixel Data can be preserved, or decode with a registry that has the JPEG 2000 codecs (DecoderRegistry::Builtin)".to_string()
        } else {
            codec_registration_hint(syntax)
        };
        if !hint.is_empty() {
            text.push_str(&format!("; {hint}"));
        }
    }
    text.push_str(&format!("; registered codecs: {}", registered.join(", ")));
    PixelError::new(PixelErrorKind::CodecNotFound, text)
}
fn codec_decode_error(uid: &str) -> PixelError {
    let mut text = String::from(PixelError::base(PixelErrorKind::CodecDecodeFailed));
    if !uid.is_empty() {
        text.push_str(&format!(" for transfer syntax {}", textfmt::quote(uid)));
        if let Some(name) = transfer::lookup(uid)
            .map(|s| s.name)
            .filter(|n| !n.is_empty())
        {
            text.push_str(&format!(" ({name})"));
        }
    }
    PixelError::new(PixelErrorKind::CodecDecodeFailed, text)
}
pub fn decode_frames(uid: &str, pixel: &PixelData<'_>, obj: &Object) -> Result<Frames, PixelError> {
    decode_frames_with_recovery(uid, pixel, obj, &CodecRecovery::default())
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodecRecovery {
    pub ignore_inconsistent_basic_offset_table: bool,
    pub jpeg_lossless_encoder_quirks: bool,
    pub jpeg_lossless_predictor6_overflow: bool,
    pub rle_segment_overrun: bool,
    pub rle_reversed_byte_segments: bool,
    pub jpeg_sequential_scan_quirks: bool,
    pub jpegls_jai_thresholds: bool,
    pub jpegls_hp_color_transforms: bool,
    pub jpeg_color: common::ColorConversion,
}
pub const DIAGNOSTIC_BASIC_OFFSET_TABLE_IGNORED: &str = "encapsulated-basic-offset-table-ignored";
pub(crate) fn decode_frames_with_recovery(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    recovery: &CodecRecovery,
) -> Result<Frames, PixelError> {
    decode_frames_in(uid, pixel, obj, recovery, false, &ResourceLimits::DEFAULT)
}
pub(crate) fn decode_frames_with_recovery_and_jpegxl(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    recovery: &CodecRecovery,
) -> Result<Frames, PixelError> {
    decode_frames_in(uid, pixel, obj, recovery, true, &ResourceLimits::DEFAULT)
}
#[doc(hidden)]
pub fn decode_frames_with_limits(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    recovery: &CodecRecovery,
    limits: &ResourceLimits,
) -> Result<Frames, PixelError> {
    check_image_limits(obj, limits)?;
    decode_frames_in(uid, pixel, obj, recovery, true, limits)
}
pub(crate) fn check_image_limits(obj: &Object, limits: &ResourceLimits) -> Result<(), PixelError> {
    let Ok(m) = extract_metadata(obj) else {
        return Ok(());
    };
    #[allow(clippy::arithmetic_side_effects)]
    let frame_bytes = u128::from(m.rows)
        * u128::from(m.columns)
        * u128::from(m.samples_per_pixel)
        * u128::from(m.bits_allocated.div_ceil(8).max(1));
    limits
        .check_image(
            u64::from(m.rows),
            u64::from(m.columns),
            u64::try_from(m.number_of_frames).unwrap_or(0),
            u64::try_from(frame_bytes).unwrap_or(u64::MAX),
        )
        .map_err(|e| PixelError::new(PixelErrorKind::ResourceLimit, e.to_string()))
}
#[doc(hidden)]
pub fn registered_codec_uids_with_jpegxl() -> Vec<&'static str> {
    let mut uids = registered_codec_uids();
    #[cfg(feature = "jpeg-xl")]
    uids.extend(crate::codec::jpegxl::dicom::uids());
    uids.sort_unstable();
    uids
}
pub(crate) fn check_codec_availability_with_jpegxl(uid: &str) -> Result<(), PixelError> {
    let normalized = transfer::normalize_uid(uid);
    let registered = registered_codec_uids_with_jpegxl();
    if registered.contains(&normalized) {
        return Ok(());
    }
    Err(codec_not_found_in(normalized, &registered))
}
pub(crate) fn decode_frames_with_jpegxl(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
) -> Result<Frames, PixelError> {
    decode_frames_in(
        uid,
        pixel,
        obj,
        &CodecRecovery::default(),
        true,
        &ResourceLimits::DEFAULT,
    )
}
#[cfg_attr(not(feature = "jpeg-xl"), allow(unused_variables))]
pub(crate) fn decode_with_builtin_codec(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    recovery: &CodecRecovery,
    jpegxl: bool,
) -> Result<Frames, PixelError> {
    let uid = transfer::normalize_uid(uid);
    #[cfg(feature = "jpeg-xl")]
    if jpegxl && is_jpeg_xl(uid) {
        return crate::codec::jpegxl::dicom::decode_frames(pixel, obj, Some(uid))
            .map_err(|e| PixelError::new(PixelErrorKind::Other, e.to_string()));
    }
    decode_registered(uid, pixel, obj, recovery).map_err(|e| match e {
        Some(text) => PixelError::new(PixelErrorKind::Other, text),
        None => codec_decode_error(uid),
    })
}
pub(crate) fn decode_frames_with_codec(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    decode: &mut dyn FnMut() -> Result<Frames, PixelError>,
) -> Result<Frames, PixelError> {
    let uid = transfer::normalize_uid(uid);
    if let Some(handled) = decode_frames_prechecked(uid, pixel, obj) {
        return handled;
    }
    decode().map_err(|e| {
        let text = e.to_string();
        if text.ends_with("context canceled") || text.ends_with("context deadline exceeded") {
            return e;
        }
        let mut err = codec_decode_error(uid);
        if e.kind != PixelErrorKind::Other {
            err.cause = Some(e.kind);
        } else {
            err.cause = e.cause;
        }
        err
    })
}
fn decode_frames_prechecked(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
) -> Option<Result<Frames, PixelError>> {
    if is_jpip(uid) {
        return Some(Err(jpip_error(uid, obj)));
    }
    if is_video_transfer_syntax(uid) {
        return Some(Err(media_error(uid, Some(pixel))));
    }
    let syntax = transfer::lookup(uid);
    if let Some(syntax) = &syntax {
        if syntax.encapsulated && !pixel.encapsulated() {
            return Some(Err(PixelError::detail(
                PixelErrorKind::IncompatiblePixelData,
                format_args!(
                    "transfer syntax {} expects encapsulated pixel data",
                    textfmt::quote(syntax.uid)
                ),
            )));
        }
        if !syntax.encapsulated && pixel.encapsulated() {
            return Some(Err(PixelError::detail(
                PixelErrorKind::IncompatiblePixelData,
                format_args!(
                    "transfer syntax {} expects native pixel data",
                    textfmt::quote(syntax.uid)
                ),
            )));
        }
    }
    if uid == ENCAPSULATED_UNCOMPRESSED {
        return Some(
            split_encapsulated_uncompressed(pixel, obj).map(|(m, data)| Frames {
                rows: usize::from(m.rows),
                columns: usize::from(m.columns),
                data,
                ..Frames::default()
            }),
        );
    }
    if !pixel.encapsulated() {
        let big_endian = match &syntax {
            Some(s) => s.big_endian,
            None => obj.value_big_endian(),
        };
        let (m, mut data) = match split_native_frames(pixel, obj, big_endian) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        if m.bits_allocated == 1 && data.len() == 1 {
            let remainder = packed_frame_bits(&m) % 8;
            if remainder != 0
                && let Some(last) = data.first_mut().and_then(|f| f.last_mut())
            {
                *last &= ((1u16 << remainder).wrapping_sub(1)) as u8;
            }
        }
        return Some(Ok(Frames {
            rows: usize::from(m.rows),
            columns: usize::from(m.columns),
            data,
            ..Frames::default()
        }));
    }
    None
}
fn decode_frames_in(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    recovery: &CodecRecovery,
    jpegxl: bool,
    limits: &ResourceLimits,
) -> Result<Frames, PixelError> {
    let uid = transfer::normalize_uid(uid);
    if is_jpip(uid) {
        return Err(jpip_error(uid, obj));
    }
    if is_video_transfer_syntax(uid) {
        return Err(media_error(uid, Some(pixel)));
    }
    let syntax = transfer::lookup(uid);
    if let Some(syntax) = &syntax {
        if syntax.encapsulated && !pixel.encapsulated() {
            return Err(PixelError::detail(
                PixelErrorKind::IncompatiblePixelData,
                format_args!(
                    "transfer syntax {} expects encapsulated pixel data",
                    textfmt::quote(syntax.uid)
                ),
            ));
        }
        if !syntax.encapsulated && pixel.encapsulated() {
            return Err(PixelError::detail(
                PixelErrorKind::IncompatiblePixelData,
                format_args!(
                    "transfer syntax {} expects native pixel data",
                    textfmt::quote(syntax.uid)
                ),
            ));
        }
    }
    check_image_limits(obj, limits)?;
    if uid == ENCAPSULATED_UNCOMPRESSED {
        let (m, data) = split_encapsulated_uncompressed(pixel, obj)?;
        return Ok(Frames {
            rows: usize::from(m.rows),
            columns: usize::from(m.columns),
            data,
            ..Frames::default()
        });
    }
    if !pixel.encapsulated() {
        let big_endian = match &syntax {
            Some(s) => s.big_endian,
            None => obj.value_big_endian(),
        };
        let (m, mut data) = split_native_frames(pixel, obj, big_endian)?;
        if m.bits_allocated == 1 && data.len() == 1 {
            let remainder = packed_frame_bits(&m) % 8;
            if remainder != 0
                && let Some(last) = data.first_mut().and_then(|f| f.last_mut())
            {
                *last &= ((1u16 << remainder).wrapping_sub(1)) as u8;
            }
        }
        return Ok(Frames {
            rows: usize::from(m.rows),
            columns: usize::from(m.columns),
            data,
            ..Frames::default()
        });
    }
    if jpegxl {
        #[cfg(feature = "jpeg-xl")]
        if is_jpeg_xl(uid) {
            return crate::codec::jpegxl::dicom::decode_frames(pixel, obj, Some(uid)).map_err(
                |e| {
                    let mut err = codec_decode_error(uid);
                    if let crate::codec::jpegxl::dicom::DicomErrorKind::Pixel(kind) = e.kind {
                        err.cause = Some(kind);
                    }
                    err
                },
            );
        }
        check_codec_availability_with_jpegxl(uid)?;
    } else if !registered_codec_uids().contains(&uid) {
        return Err(codec_not_found(uid));
    }
    decode_registered(uid, pixel, obj, recovery).map_err(|_| codec_decode_error(uid))
}
#[cfg_attr(
    not(any(
        feature = "jpeg",
        feature = "jpeg-lossless",
        feature = "jpeg-ls",
        feature = "jpeg2000"
    )),
    allow(dead_code)
)]
pub(crate) fn to_codec_metadata(m: &Metadata) -> common::Metadata {
    common::Metadata {
        rows: m.rows,
        columns: m.columns,
        samples_per_pixel: m.samples_per_pixel,
        bits_allocated: m.bits_allocated,
        bits_stored: m.bits_stored,
        high_bit: m.high_bit,
        pixel_representation: m.pixel_representation,
        planar_configuration: m.planar_configuration,
        planar_configuration_present: m.planar_configuration_present,
        number_of_frames: m.number_of_frames,
        photometric_interpretation: m.photometric_interpretation.clone(),
    }
}
#[cfg_attr(
    not(any(
        feature = "jpeg",
        feature = "jpeg-lossless",
        feature = "jpeg-ls",
        feature = "jpeg2000"
    )),
    allow(dead_code)
)]
pub(crate) fn from_decoded(d: common::DecodedFrames) -> Frames {
    Frames {
        rows: d.rows,
        columns: d.columns,
        data: d.data,
        photometric_interpretation: d.photometric_interpretation,
        diagnostics: d.diagnostics,
    }
}
fn table_attribute(obj: &Object, tag: Tag) -> Option<TableAttribute<'_>> {
    let elem = obj.get(tag)?;
    let value = match &elem.value {
        Value::Raw(raw) => TableValue::Raw(raw),
        _ => TableValue::Unsupported("core.Value"),
    };
    Some(TableAttribute {
        vr_ov: elem.vr() == Vr::OV,
        value,
    })
}
#[cfg_attr(
    not(any(
        feature = "jpeg",
        feature = "jpeg-lossless",
        feature = "jpeg-ls",
        feature = "jpeg2000"
    )),
    allow(dead_code)
)]
fn plan_codestreams(
    pixel: &PixelData<'_>,
    obj: &Object,
    frames: i64,
    format: Format,
    recovery: &CodecRecovery,
) -> Result<(Vec<Vec<u8>>, bool), ()> {
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = *pixel
    else {
        return Err(());
    };
    let refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
    let plan = encapsulated::from_fragments(
        offset_table,
        &refs,
        crate::pixel::encapsulated::EotAttributes {
            extended: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE),
            lengths: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
        },
        frames,
        format,
        Limits::default(),
        Recovery {
            ignore_inconsistent_basic_offset_table: recovery.ignore_inconsistent_basic_offset_table,
        },
    )
    .map_err(|_| ())?;
    let ignored = plan.recovery().basic_offset_table_ignored;
    let data: Result<Vec<Vec<u8>>, ()> = (0..plan.len())
        .map(|i| plan.frame(i).map(|v| v.data.into_owned()).map_err(|_| ()))
        .collect();
    Ok((data?, ignored))
}
#[cfg(any(feature = "jpeg", feature = "jpeg-lossless", feature = "jpeg-ls"))]
fn from_decoded_planned(d: common::DecodedFrames, ignored: bool) -> Frames {
    let mut f = from_decoded(d);
    if ignored
        && !f
            .diagnostics
            .contains(&DIAGNOSTIC_BASIC_OFFSET_TABLE_IGNORED)
    {
        f.diagnostics
            .insert(0, DIAGNOSTIC_BASIC_OFFSET_TABLE_IGNORED);
    }
    f
}
fn decode_registered(
    uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
    recovery: &CodecRecovery,
) -> Result<Frames, Option<String>> {
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = *pixel
    else {
        return Err(None);
    };
    let m = extract_metadata(obj).map_err(|_| None)?;
    let refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
    if uid == rle::UID {
        let f = rle::decode(
            &refs,
            offset_table,
            &m,
            rle::Recovery {
                segment_overrun: recovery.rle_segment_overrun,
                reversed_byte_segments: recovery.rle_reversed_byte_segments,
            },
        )
        .map_err(|e| Some(e.to_string()))?;
        return Ok(Frames {
            rows: f.rows,
            columns: f.columns,
            data: f.data,
            photometric_interpretation: String::new(),
            diagnostics: f.diagnostics,
        });
    }
    if uid == DEFLATED_IMAGE_FRAME {
        return decode_deflated(fragments, offset_table, &m).map_err(|()| None);
    }
    #[cfg(any(
        feature = "jpeg",
        feature = "jpeg-lossless",
        feature = "jpeg-ls",
        feature = "jpeg2000"
    ))]
    let cm = to_codec_metadata(&m);
    #[cfg(feature = "jpeg2000")]
    if is_jpeg2000(uid) {
        let jp2 = refs
            .first()
            .is_some_and(|f| f.len() >= 12 && f.get(4..8) == Some(b"jP  ".as_slice()));
        let planned;
        let mut payloads = refs.clone();
        if !jp2 {
            (planned, _) =
                plan_codestreams(pixel, obj, m.number_of_frames, Format::JPEG2000, recovery)
                    .map_err(|()| None)?;
            payloads = planned.iter().map(Vec::as_slice).collect();
        }
        return jpeg2000::decode_frames(
            &payloads,
            &cm,
            uid,
            jpeg2000::Registration::Default,
            jpeg2000::CoreOptions::default(),
        )
        .map(from_decoded)
        .map_err(|e| Some(e.to_string()));
    }
    #[cfg(feature = "jpeg")]
    if let Some(syntax) = jpeg::Syntax::from_uid(uid) {
        let (codestreams, ignored) =
            plan_codestreams(pixel, obj, m.number_of_frames, Format::JPEG, recovery)
                .map_err(|()| None)?;
        let refs: Vec<&[u8]> = codestreams.iter().map(Vec::as_slice).collect();
        let mut options = jpeg_options(syntax);
        options.sequential_scan_quirks = recovery.jpeg_sequential_scan_quirks;
        options.color = recovery.jpeg_color;
        return jpeg::decode_frames(&refs, &cm, options)
            .map(|d| from_decoded_planned(d, ignored))
            .map_err(|e| Some(e.to_string()));
    }
    #[cfg(feature = "jpeg-lossless")]
    if uid == jpeglossless::UID_PROCESS14 || uid == jpeglossless::UID_PROCESS14_SV1 {
        let (codestreams, ignored) =
            plan_codestreams(pixel, obj, m.number_of_frames, Format::JPEG, recovery)
                .map_err(|()| None)?;
        let refs: Vec<&[u8]> = codestreams.iter().map(Vec::as_slice).collect();
        let options = jpeglossless::DecodeOptions {
            selection_value: u8::from(uid == jpeglossless::UID_PROCESS14_SV1),
            encoder_quirks: recovery.jpeg_lossless_encoder_quirks,
            predictor6_overflow: recovery.jpeg_lossless_predictor6_overflow,
        };
        return jpeglossless::decode_frames(&refs, &cm, options)
            .map(|d| from_decoded_planned(d, ignored))
            .map_err(|e| Some(e.to_string()));
    }
    #[cfg(feature = "jpeg-ls")]
    if uid == jpegls::UID || uid == jpegls::UID_NEAR_LOSSLESS {
        let codec = if uid == jpegls::UID {
            jpegls::Codec::lossless()
        } else {
            jpegls::Codec::near_lossless()
        };
        let (codestreams, ignored) =
            plan_codestreams(pixel, obj, m.number_of_frames, Format::JPEGLS, recovery)
                .map_err(|()| None)?;
        let refs: Vec<&[u8]> = codestreams.iter().map(Vec::as_slice).collect();
        let options = jpegls::DecodeOptions {
            jai_thresholds: recovery.jpegls_jai_thresholds,
            hp_color_transforms: recovery.jpegls_hp_color_transforms,
        };
        return codec
            .decode_frames(&refs, &cm, options)
            .map(|d| from_decoded_planned(d, ignored))
            .map_err(|e| Some(e.to_string()));
    }
    Err(None)
}
#[cfg(feature = "jpeg")]
fn jpeg_options(syntax: jpeg::Syntax) -> jpeg::DecodeOptions {
    jpeg::DecodeOptions {
        syntax,
        ..Default::default()
    }
}
fn decode_deflated(fragments: &[Vec<u8>], offset_table: &[u8], m: &Metadata) -> Result<Frames, ()> {
    if m.rows == 0 || m.columns == 0 || m.samples_per_pixel == 0 {
        return Err(());
    }
    let ba = m.bits_allocated;
    if !(ba == 1 || (8..=64).contains(&ba) && ba.is_multiple_of(8)) {
        return Err(());
    }
    if !matches!(m.samples_per_pixel, 1 | 3 | 4) || m.number_of_frames < 1 {
        return Err(());
    }
    let size = frame_size(m);
    if size <= 0 {
        return Err(());
    }
    let size = size as usize;
    let streams = group_deflated(fragments, offset_table, m.number_of_frames as usize)?;
    let mut out = Frames {
        rows: usize::from(m.rows),
        columns: usize::from(m.columns),
        ..Frames::default()
    };
    for stream in &streams {
        if stream.is_empty() {
            return Err(());
        }
        let (data, err, consumed) = crate::object::inflate::inflate_raw(stream);
        if data.len() > size || data.len() < size || !err.is(crate::object::Kind::Eof) {
            return Err(());
        }
        match stream.len().saturating_sub(consumed) {
            0 => {}
            1 => {
                if stream.last().is_some_and(|&pad| pad != 0)
                    && !out.diagnostics.contains(&"encapsulated-nonzero-frame-pad")
                {
                    out.diagnostics.push("encapsulated-nonzero-frame-pad");
                }
            }
            _ => return Err(()),
        }
        out.data.push(data);
    }
    Ok(out)
}
fn group_deflated(
    fragments: &[Vec<u8>],
    offset_table: &[u8],
    frames: usize,
) -> Result<Vec<Vec<u8>>, ()> {
    if fragments.is_empty() || frames > fragments.len() {
        return Err(());
    }
    if frames == fragments.len() {
        return Ok(fragments.to_vec());
    }
    if frames == 1 {
        return Ok(vec![fragments.concat()]);
    }
    if Some(offset_table.len()) != frames.checked_mul(4) {
        return Err(());
    }
    let mut index = std::collections::HashMap::new();
    let mut position = 0u64;
    for (i, f) in fragments.iter().enumerate() {
        index.insert(position, i);
        position = position.saturating_add(f.len() as u64).saturating_add(8);
    }
    let mut starts: Vec<usize> = Vec::with_capacity(frames);
    for entry in offset_table.as_chunks::<4>().0 {
        let offset = u32::from_le_bytes(*entry);
        let at = *index.get(&u64::from(offset)).ok_or(())?;
        let ordered = match starts.last() {
            None => at == 0,
            Some(&previous) => at > previous,
        };
        if !ordered {
            return Err(());
        }
        starts.push(at);
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = starts
                .get(i.saturating_add(1))
                .copied()
                .unwrap_or(fragments.len());
            fragments.get(start..end).map(<[Vec<u8>]>::concat).ok_or(())
        })
        .collect()
}
