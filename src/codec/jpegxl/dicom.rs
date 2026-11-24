use super::bitio::BitReader;
use super::codestream::{decode, read_codestream_header};
use super::jpegparse::parse_jpeg;
use super::{
    Error, ErrorKind, Image, LosslessOptions, LossyOptions, MAX_LOSSY_QUALITY, MIN_LOSSY_QUALITY,
    container, encode_lossless, encode_lossy, recompress_jpeg, reconstruct_jpeg,
};
use crate::codec::jpeg;
use crate::object::Object;
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{
    Frames, PixelData, PixelErrorKind, extract_metadata, frame_size, from_decoded,
    to_codec_metadata,
};
use crate::textfmt;
use std::fmt;
pub const LOSSY_METHOD: &str = "ISO_18181_1";
pub const BACKEND: &str = "dcm-rust-jpegxl";
#[doc(hidden)]
pub const UID_LOSSLESS: &str = "1.2.840.10008.1.2.4.110";
pub const UID_JPEG_RECOMPRESSION: &str = "1.2.840.10008.1.2.4.111";
#[doc(hidden)]
pub const UID_JPEG_XL: &str = "1.2.840.10008.1.2.4.112";
const UID_JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
const UID_JPEG_EXTENDED: &str = "1.2.840.10008.1.2.4.51";
pub fn uids() -> [&'static str; 3] {
    [UID_LOSSLESS, UID_JPEG_RECOMPRESSION, UID_JPEG_XL]
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DicomErrorKind {
    Codec(ErrorKind),
    FragmentLayout,
    Pixel(PixelErrorKind),
    UnsupportedPixelRepresentation,
    UnsupportedPlanarConfiguration,
    UnsupportedEncoderMetadata,
    Jpeg,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DicomError {
    pub kind: DicomErrorKind,
    text: String,
}
impl DicomError {
    fn new(kind: DicomErrorKind, text: impl Into<String>) -> DicomError {
        DicomError {
            kind,
            text: text.into(),
        }
    }
    fn pixel(kind: PixelErrorKind, base: &str, detail: impl fmt::Display) -> DicomError {
        DicomError::new(DicomErrorKind::Pixel(kind), format!("{base}: {detail}"))
    }
    fn codec(kind: ErrorKind, detail: impl Into<String>) -> DicomError {
        Error::new(kind, Some(detail.into())).into()
    }
    fn in_frame(self, i: usize) -> DicomError {
        DicomError {
            kind: self.kind,
            text: format!("frame {i}: {}", self.text),
        }
    }
    fn unsupported_metadata(field: &str) -> DicomError {
        DicomError::new(
            DicomErrorKind::UnsupportedEncoderMetadata,
            format!("dicom: pixel metadata is unsupported by encoder: {field}"),
        )
    }
}
impl From<Error> for DicomError {
    fn from(e: Error) -> DicomError {
        DicomError::new(DicomErrorKind::Codec(e.kind()), e.to_string())
    }
}
impl fmt::Display for DicomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for DicomError {}
const ERR_INVALID_METADATA: &str = "dicom: invalid pixel data metadata";
const ERR_INCOMPATIBLE: &str = "dicom: pixel data is incompatible with transfer syntax";
const ERR_SIZE_MISMATCH: &str = "dicom: pixel data size does not match metadata";
fn normalized_photometric(s: &str) -> String {
    textfmt::to_upper(s.trim())
}
pub fn decode_frames(
    pixel: &PixelData<'_>,
    obj: &Object,
    syntax_uid: Option<&str>,
) -> Result<Frames, DicomError> {
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = *pixel
    else {
        return Err(DicomError::pixel(
            PixelErrorKind::IncompatiblePixelData,
            ERR_INCOMPATIBLE,
            "JPEG XL requires encapsulated pixel data",
        ));
    };
    let m = extract_metadata(obj)
        .map_err(|e| DicomError::new(DicomErrorKind::Pixel(e.kind), e.to_string()))?;
    validate_decode_metadata(&m)?;
    const MAX_REQUEST_BYTES: i64 = 512 << 20;
    let size = frame_size(&m);
    if size <= 0
        || size
            > MAX_REQUEST_BYTES
                .checked_div(m.number_of_frames)
                .unwrap_or(0)
    {
        return Err(DicomError::codec(
            ErrorKind::Limit,
            "decoded request exceeds resource limit",
        ));
    }
    let payloads = frame_payloads(offset_table, fragments, m.number_of_frames)?;
    let mut result = Frames {
        rows: usize::from(m.rows),
        columns: usize::from(m.columns),
        data: vec![Vec::new(); payloads.len()],
        ..Frames::default()
    };
    let allow_xyb = syntax_uid.is_none_or(|uid| uid.is_empty() || uid == UID_JPEG_XL);
    let mut jpeg_frames: Option<Vec<Vec<u8>>> = None;
    let mixed = || {
        DicomError::codec(
            ErrorKind::Unsupported,
            "frames mix JPEG reconstruction and pixel codestreams",
        )
    };
    for (i, payload) in payloads.iter().enumerate() {
        if has_reconstruction_data(payload) {
            let jpg = reconstruct_jpeg(payload).map_err(|e| DicomError::from(e).in_frame(i))?;
            jpeg_frames.get_or_insert_with(Vec::new).push(jpg);
            continue;
        }
        if jpeg_frames.is_some() {
            return Err(mixed());
        }
        let (frame, photometric) =
            decode_pixel_frame(payload, &m, allow_xyb).map_err(|e| e.in_frame(i))?;
        if let Some(slot) = result.data.get_mut(i) {
            *slot = frame;
        }
        result.photometric_interpretation = photometric;
    }
    let Some(jpeg_frames) = jpeg_frames else {
        return Ok(result);
    };
    if jpeg_frames.len() != payloads.len() {
        return Err(mixed());
    }
    decode_reconstructed_jpeg(&jpeg_frames, &m)
}
fn validate_decode_metadata(m: &Metadata) -> Result<(), DicomError> {
    if m.rows == 0 || m.columns == 0 || m.number_of_frames <= 0 {
        return Err(DicomError::pixel(
            PixelErrorKind::InvalidMetadata,
            ERR_INVALID_METADATA,
            format_args!(
                "Rows={} Columns={} NumberOfFrames={}",
                m.rows, m.columns, m.number_of_frames
            ),
        ));
    }
    if m.bits_allocated != 8 && m.bits_allocated != 16 {
        return Err(DicomError::codec(
            ErrorKind::Unsupported,
            format!("BitsAllocated={}", m.bits_allocated),
        ));
    }
    if m.bits_stored == 0
        || m.bits_stored > m.bits_allocated
        || m.high_bit != m.bits_stored.wrapping_sub(1)
    {
        return Err(DicomError::pixel(
            PixelErrorKind::InvalidMetadata,
            ERR_INVALID_METADATA,
            format_args!("BitsStored={} HighBit={}", m.bits_stored, m.high_bit),
        ));
    }
    if m.samples_per_pixel != 1 && m.samples_per_pixel != 3 {
        return Err(DicomError::codec(
            ErrorKind::Unsupported,
            format!("SamplesPerPixel={}", m.samples_per_pixel),
        ));
    }
    if m.pixel_representation > 1 || m.pixel_representation == 1 && m.samples_per_pixel != 1 {
        return Err(DicomError::new(
            DicomErrorKind::UnsupportedPixelRepresentation,
            format!(
                "dicom: unsupported PixelRepresentation: PixelRepresentation={}",
                m.pixel_representation
            ),
        ));
    }
    if m.planar_configuration_present && m.planar_configuration != 0 {
        return Err(DicomError::new(
            DicomErrorKind::UnsupportedPlanarConfiguration,
            format!(
                "dicom: unsupported PlanarConfiguration: PlanarConfiguration={}",
                m.planar_configuration
            ),
        ));
    }
    Ok(())
}
fn decode_pixel_frame(
    payload: &[u8],
    m: &Metadata,
    allow_xyb: bool,
) -> Result<(Vec<u8>, String), DicomError> {
    check_frame_info(payload, m)?;
    let img = decode(payload, allow_xyb)?;
    if img.width != usize::from(m.columns)
        || img.height != usize::from(m.rows)
        || img.channels != usize::from(m.samples_per_pixel)
    {
        return Err(DicomError::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            ERR_SIZE_MISMATCH,
            format_args!(
                "codestream {}x{}x{}, metadata {}x{}x{}",
                img.width, img.height, img.channels, m.columns, m.rows, m.samples_per_pixel
            ),
        ));
    }
    if img.bits_per_sample > u32::from(m.bits_stored) {
        return Err(DicomError::codec(
            ErrorKind::Unsupported,
            format!(
                "codestream has {} bits per sample, BitsStored={}",
                img.bits_per_sample, m.bits_stored
            ),
        ));
    }
    let offset: i32 = if m.pixel_representation == 1 {
        1i32.checked_shl(u32::from(m.bits_stored).wrapping_sub(1))
            .unwrap_or(0)
    } else {
        0
    };
    let out = if m.bits_allocated == 8 {
        img.samples
            .iter()
            .map(|v| v.wrapping_sub(offset) as u8)
            .collect()
    } else {
        let mut out = Vec::with_capacity(img.samples.len().saturating_mul(2));
        for v in &img.samples {
            out.extend_from_slice(&(v.wrapping_sub(offset) as u16).to_le_bytes());
        }
        out
    };
    let photometric = if m.samples_per_pixel == 3
        && normalized_photometric(&m.photometric_interpretation) != "RGB"
    {
        "RGB".to_string()
    } else {
        String::new()
    };
    Ok((out, photometric))
}
fn check_frame_info(payload: &[u8], m: &Metadata) -> Result<(), DicomError> {
    let c = container::parse(payload)?;
    let hdr = read_codestream_header(&mut BitReader::new(&c.codestream))?;
    if hdr.meta.width != usize::from(m.columns) || hdr.meta.height != usize::from(m.rows) {
        return Err(DicomError::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            ERR_SIZE_MISMATCH,
            format_args!(
                "codestream {}x{}, metadata {}x{}",
                hdr.meta.width, hdr.meta.height, m.columns, m.rows
            ),
        ));
    }
    Ok(())
}
fn has_reconstruction_data(payload: &[u8]) -> bool {
    container::parse(payload).is_ok_and(|c| c.jbrd.is_some())
}
fn decode_reconstructed_jpeg(frames: &[Vec<u8>], m: &Metadata) -> Result<Frames, DicomError> {
    let refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
    let options = jpeg::DecodeOptions {
        syntax: jpeg::Syntax::Auto,
        ..Default::default()
    };
    jpeg::decode_frames(&refs, &to_codec_metadata(m), options)
        .map(from_decoded)
        .map_err(|e| DicomError::new(DicomErrorKind::Jpeg, e.to_string()))
}
pub(crate) fn frame_payloads(
    offset_table: &[u8],
    fragments: &[Vec<u8>],
    frames: i64,
) -> Result<Vec<Vec<u8>>, DicomError> {
    let layout = |detail: String| {
        DicomError::new(
            DicomErrorKind::FragmentLayout,
            format!("jpegxl: unsupported fragment layout: {detail}"),
        )
    };
    if fragments.is_empty() {
        return Err(layout("no fragments".into()));
    }
    if fragments.len() as i64 == frames {
        return Ok(fragments.to_vec());
    }
    if frames == 1 {
        return Ok(vec![fragments.concat()]);
    }
    if frames < 0 || offset_table.len() as i64 != frames.saturating_mul(4) {
        return Err(layout(format!(
            "{} fragments for {frames} frames without a Basic Offset Table",
            fragments.len()
        )));
    }
    let frames = frames as usize;
    let mut starts = std::collections::HashMap::with_capacity(fragments.len());
    let mut offset = 0u64;
    for (i, f) in fragments.iter().enumerate() {
        if offset > u64::from(u32::MAX) {
            return Err(layout("offsets exceed 32 bits".into()));
        }
        starts.insert(offset as u32, i);
        offset = offset
            .saturating_add(8)
            .saturating_add(f.len() as u64)
            .saturating_add((f.len() & 1) as u64);
    }
    let (entries, _) = offset_table.as_chunks::<4>();
    let mut first = Vec::with_capacity(frames.saturating_add(1));
    for (i, &entry) in entries.iter().enumerate() {
        let entry = u32::from_le_bytes(entry);
        match (starts.get(&entry), first.last()) {
            (Some(&idx), Some(&prev)) if idx > prev => first.push(idx),
            (Some(&0), None) => first.push(0),
            _ => return Err(layout(format!("Basic Offset Table entry {i}"))),
        }
    }
    first.push(fragments.len());
    Ok(first
        .windows(2)
        .map(|w| match *w {
            [a, b] => fragments.get(a..b).unwrap_or_default().concat(),
            _ => Vec::new(),
        })
        .collect())
}
#[doc(hidden)]
pub fn encode_lossless_frame(frame: &[u8], m: &Metadata) -> Result<Vec<u8>, DicomError> {
    let img = frame_image(frame, m)?;
    Ok(encode_lossless(
        &img,
        LosslessOptions {
            bits_per_sample: u32::from(m.bits_stored),
        },
    )?)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossyEncoder {
    quality: i64,
}
impl LossyEncoder {
    pub fn new(quality: i64) -> Result<LossyEncoder, DicomError> {
        if !(MIN_LOSSY_QUALITY..=MAX_LOSSY_QUALITY).contains(&quality) {
            return Err(DicomError::codec(
                ErrorKind::Unsupported,
                format!("quality {quality} outside 1..100"),
            ));
        }
        Ok(LossyEncoder { quality })
    }
    pub fn quality(&self) -> i64 {
        self.quality
    }
    pub fn encode_frame(&self, frame: &[u8], m: &Metadata) -> Result<Vec<u8>, DicomError> {
        if m.bits_allocated != 8 || m.bits_stored != 8 || m.pixel_representation != 0 {
            return Err(DicomError::unsupported_metadata("BitsStored"));
        }
        let img = frame_image(frame, m)?;
        Ok(encode_lossy(
            &img,
            LossyOptions {
                quality: self.quality,
            },
        )?)
    }
}
fn frame_image(frame: &[u8], m: &Metadata) -> Result<Image, DicomError> {
    let field = if m.rows == 0 {
        "Rows"
    } else if m.columns == 0 {
        "Columns"
    } else if m.bits_allocated != 8 && m.bits_allocated != 16 {
        "BitsAllocated"
    } else if m.bits_stored == 0 || m.bits_stored > m.bits_allocated {
        "BitsStored"
    } else if m.high_bit != m.bits_stored.wrapping_sub(1) {
        "HighBit"
    } else if m.pixel_representation > 1 || m.pixel_representation == 1 && m.samples_per_pixel != 1
    {
        "PixelRepresentation"
    } else if m.samples_per_pixel != 1 && m.samples_per_pixel != 3 {
        "SamplesPerPixel"
    } else if m.samples_per_pixel == 3
        && (!m.planar_configuration_present || m.planar_configuration != 0)
    {
        "PlanarConfiguration"
    } else {
        let photometric = normalized_photometric(&m.photometric_interpretation);
        if m.samples_per_pixel == 1 && photometric != "MONOCHROME1" && photometric != "MONOCHROME2"
            || m.samples_per_pixel == 3 && photometric != "RGB"
        {
            "PhotometricInterpretation"
        } else {
            ""
        }
    };
    if !field.is_empty() {
        return Err(DicomError::unsupported_metadata(field));
    }
    if frame.len() as i64 != frame_size(m) {
        return Err(DicomError::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            ERR_SIZE_MISMATCH,
            "native frame length",
        ));
    }
    let bits = u32::from(m.bits_stored);
    let mask = 1u32.checked_shl(bits).unwrap_or(0).wrapping_sub(1);
    let sign_bit = 1u32.checked_shl(bits.wrapping_sub(1)).unwrap_or(0);
    let raws: Vec<u32> = if m.bits_allocated == 8 {
        frame.iter().map(|&b| u32::from(b)).collect()
    } else {
        let (pairs, _) = frame.as_chunks::<2>();
        pairs
            .iter()
            .map(|&p| u32::from(u16::from_le_bytes(p)))
            .collect()
    };
    let samples = raws
        .into_iter()
        .map(|raw| {
            let mut raw = raw & mask;
            if m.pixel_representation == 1 {
                raw ^= sign_bit;
            }
            raw as i32
        })
        .collect();
    Ok(Image {
        width: usize::from(m.columns),
        height: usize::from(m.rows),
        channels: usize::from(m.samples_per_pixel),
        bits_per_sample: u32::from(m.bits_stored),
        samples,
    })
}
pub fn can_transcode_codestream(source_uid: &str, target_uid: &str) -> bool {
    let jpeg_uid = if source_uid == UID_JPEG_RECOMPRESSION {
        target_uid
    } else if target_uid != UID_JPEG_RECOMPRESSION {
        return false;
    } else {
        source_uid
    };
    jpeg_uid == UID_JPEG_BASELINE || jpeg_uid == UID_JPEG_EXTENDED
}
pub fn transcode_codestreams(
    source_uid: &str,
    target_uid: &str,
    pixel: &PixelData<'_>,
    obj: &Object,
) -> Result<Vec<Vec<u8>>, DicomError> {
    if !can_transcode_codestream(source_uid, target_uid) {
        return Err(DicomError::codec(
            ErrorKind::Unsupported,
            format!("{source_uid} to {target_uid}"),
        ));
    }
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = *pixel
    else {
        return Err(DicomError::pixel(
            PixelErrorKind::IncompatiblePixelData,
            ERR_INCOMPATIBLE,
            "encapsulated pixel data required",
        ));
    };
    let m = extract_metadata(obj)
        .map_err(|e| DicomError::new(DicomErrorKind::Pixel(e.kind), e.to_string()))?;
    if m.number_of_frames <= 0 {
        return Err(DicomError::pixel(
            PixelErrorKind::InvalidMetadata,
            ERR_INVALID_METADATA,
            format_args!("NumberOfFrames={}", m.number_of_frames),
        ));
    }
    let payloads = frame_payloads(offset_table, fragments, m.number_of_frames)?;
    payloads
        .iter()
        .enumerate()
        .map(|(i, payload)| {
            let out = if target_uid == UID_JPEG_RECOMPRESSION {
                recompress_jpeg(trim_jpeg_pad(payload)).map_err(DicomError::from)
            } else {
                reconstruct_for(payload, target_uid)
            };
            out.map_err(|e| e.in_frame(i))
        })
        .collect()
}
fn reconstruct_for(payload: &[u8], target_uid: &str) -> Result<Vec<u8>, DicomError> {
    let jpg = reconstruct_jpeg(payload)?;
    let p = parse_jpeg(&jpg)?;
    let want: u8 = if target_uid == UID_JPEG_EXTENDED {
        0xC1
    } else {
        0xC0
    };
    if p.sof_marker != want {
        return Err(DicomError::codec(
            ErrorKind::Unsupported,
            format!(
                "restored JPEG uses SOF{:X}, not the target's SOF{:X}",
                p.sof_marker & 0xF,
                want & 0xF
            ),
        ));
    }
    Ok(jpg)
}
fn trim_jpeg_pad(payload: &[u8]) -> &[u8] {
    match payload {
        [rest @ .., 0] if rest.ends_with(&[0xFF, 0xD9]) => rest,
        _ => payload,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn meta(rows: u16, columns: u16, spp: u16, ba: u16, bs: u16, pr: u16) -> Metadata {
        Metadata {
            rows,
            columns,
            samples_per_pixel: spp,
            bits_allocated: ba,
            bits_stored: bs,
            high_bit: bs - 1,
            pixel_representation: pr,
            planar_configuration: 0,
            planar_configuration_present: spp == 3,
            number_of_frames: 1,
            photometric_interpretation: if spp == 3 { "RGB" } else { "MONOCHROME2" }.into(),
        }
    }
    #[test]
    fn signed_samples_use_offset_binary() {
        let m = meta(1, 4, 1, 16, 12, 1);
        let values: [i16; 4] = [-2048, -1, 0, 2047];
        let frame: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let data = encode_lossless_frame(&frame, &m).unwrap_or_default();
        let img = super::super::decode(&data).unwrap_or_default();
        assert_eq!(img.bits_per_sample, 12);
        assert_eq!(img.samples, vec![0, 2047, 2048, 4095]);
    }
    #[test]
    fn fragment_layouts() {
        let frags = vec![vec![1u8, 2, 3], vec![4], vec![5, 6]];
        assert_eq!(
            frame_payloads(&[], &frags, 1).unwrap_or_default(),
            vec![vec![1, 2, 3, 4, 5, 6]]
        );
        let bot: Vec<u8> = [0u32, 22].iter().flat_map(|o| o.to_le_bytes()).collect();
        assert_eq!(
            frame_payloads(&bot, &frags, 2).unwrap_or_default(),
            vec![vec![1, 2, 3, 4], vec![5, 6]]
        );
        assert_eq!(
            frame_payloads(&[], &frags, 2).map_err(|e| e.to_string()),
            Err(
                "jpegxl: unsupported fragment layout: 3 fragments for 2 frames without a Basic Offset Table"
                    .into()
            )
        );
    }
    #[test]
    fn recompressor_directions() {
        assert!(can_transcode_codestream(
            UID_JPEG_BASELINE,
            UID_JPEG_RECOMPRESSION
        ));
        assert!(can_transcode_codestream(
            UID_JPEG_RECOMPRESSION,
            UID_JPEG_EXTENDED
        ));
        assert!(!can_transcode_codestream(UID_JPEG_BASELINE, UID_LOSSLESS));
        assert!(!can_transcode_codestream(
            UID_JPEG_RECOMPRESSION,
            "1.2.840.10008.1.2.4.90"
        ));
        assert_eq!(
            LossyEncoder::new(0).map_err(|e| e.to_string()),
            Err("jpegxl: unsupported feature: quality 0 outside 1..100".into())
        );
    }
}
