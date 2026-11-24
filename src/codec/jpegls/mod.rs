#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod bitstream;
mod decoder;
mod encoder;
mod framing;
mod loco;
pub use crate::codec::common::{DecodedFrames, Metadata};
use crate::textfmt;
use decoder::FrameHeader;
pub use encoder::{Encoder, EncoderCapabilities, EncoderOptions, InterleaveMode};
use std::fmt;
pub const UID: &str = "1.2.840.10008.1.2.4.80";
pub const UID_NEAR_LOSSLESS: &str = "1.2.840.10008.1.2.4.81";
const MAX_FRAME_BYTES: u64 = 512 << 20;
const MAX_FRAMES: usize = 100_000;
const MAX_WORKING_SET_BYTES: u64 = 512 << 20;
pub mod diagnostic {
    pub const NON_ZERO_FRAME_PAD: &str = "encapsulated-nonzero-frame-pad";
    pub const JAI_THRESHOLDS: &str = "jpeg-ls-jai-thresholds";
    pub const HP1: &str = "jpeg-ls-hp1-color-transform";
    pub const HP2: &str = "jpeg-ls-hp2-color-transform";
    pub const HP3: &str = "jpeg-ls-hp3-color-transform";
    pub const HP_BACKEND_PURE: &str = "jpeg-ls-hp-backend-pure-rust";
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidCodestream,
    UnexpectedEof,
    PixelDataSizeMismatch,
    IncompatiblePixelData,
    UnsupportedPhotometricInterpretation,
    UnsupportedEncoderMetadata,
    EncoderOutputInvalid,
    LossyDisallowed,
    InvalidRegion,
    RegionUnsupported,
    EncapsulatedLayout,
    ResourceLimit,
}
impl ErrorKind {
    fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidCodestream => "dicom: invalid JPEG-LS codestream",
            ErrorKind::UnexpectedEof => "unexpected EOF",
            ErrorKind::PixelDataSizeMismatch => "dicom: pixel data size does not match metadata",
            ErrorKind::IncompatiblePixelData => {
                "dicom: pixel data is incompatible with transfer syntax"
            }
            ErrorKind::UnsupportedPhotometricInterpretation => {
                "dicom: unsupported PhotometricInterpretation"
            }
            ErrorKind::UnsupportedEncoderMetadata => {
                "dicom: pixel metadata is unsupported by encoder"
            }
            ErrorKind::EncoderOutputInvalid => "dicom: pixel data encoder returned invalid output",
            ErrorKind::LossyDisallowed => "dicom: lossy transcode requires explicit opt-in",
            ErrorKind::InvalidRegion => "dicom: invalid frame region",
            ErrorKind::RegionUnsupported => "dicom: codec cannot decode a frame region",
            ErrorKind::EncapsulatedLayout => "dicom: invalid encapsulated frame layout",
            ErrorKind::ResourceLimit => "dicom: encapsulated frame resource limit exceeded",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}
impl Error {
    fn new(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            message: format!("{}: {detail}", kind.text()),
        }
    }
    fn bare(kind: ErrorKind) -> Error {
        Error {
            kind,
            message: kind.text().to_string(),
        }
    }
    pub(crate) fn invalid(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::InvalidCodestream, detail)
    }
    pub(crate) fn unexpected_eof() -> Error {
        Error::bare(ErrorKind::UnexpectedEof)
    }
    pub(crate) fn size(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::PixelDataSizeMismatch, detail)
    }
    pub(crate) fn incompatible(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::IncompatiblePixelData, detail)
    }
    pub(crate) fn encoder_metadata(field: &str) -> Error {
        Error::new(ErrorKind::UnsupportedEncoderMetadata, field)
    }
    pub(crate) fn wrap(self, prefix: impl fmt::Display) -> Error {
        Error {
            kind: self.kind,
            message: format!("{prefix}: {}", self.message),
        }
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeOptions {
    pub jai_thresholds: bool,
    pub hp_color_transforms: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Region {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}
impl Region {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Region {
        Region {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }
    fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    fn width(&self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }
    fn height(&self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodestreamInfo {
    pub rows: i64,
    pub columns: i64,
    pub components: i64,
    pub precision: i64,
    pub near: i64,
    pub interleave: i64,
    pub max_value: i64,
    pub hp_color_transform: i64,
}
pub fn inspect_codestream(
    data: &[u8],
    max_bytes: i64,
    options: DecodeOptions,
) -> Result<CodestreamInfo, Error> {
    if max_bytes < 0 {
        return Err(Error::bare(ErrorKind::InvalidCodestream));
    }
    let max_bytes = if max_bytes == 0 { 256 << 20 } else { max_bytes };
    if data.len() as i64 > max_bytes {
        return Err(Error::invalid("input byte limit"));
    }
    framing::validate_frame(data)?;
    let (h, _) = decoder::split_scans(data, options.hp_color_transforms)?;
    Ok(CodestreamInfo {
        rows: h.rows,
        columns: h.columns,
        components: h.components,
        precision: h.precision,
        near: h.near,
        interleave: h.ilv,
        max_value: h.preset.maxval,
        hp_color_transform: h.hp_transform,
    })
}
fn normalized_photometric(metadata: &Metadata) -> String {
    textfmt::to_upper(metadata.photometric_interpretation.trim())
}
fn trailing_pad(data: &[u8]) -> Option<u8> {
    match data {
        [.., 0xff, 0xd9, pad] => Some(*pad),
        _ => None,
    }
}
fn add_diagnostic(result: &mut DecodedFrames, code: &'static str) {
    if !result.diagnostics.contains(&code) {
        result.diagnostics.push(code);
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Codec {
    near_lossless: bool,
}
impl Codec {
    pub fn lossless() -> Codec {
        Codec {
            near_lossless: false,
        }
    }
    #[doc(hidden)]
    pub fn near_lossless() -> Codec {
        Codec {
            near_lossless: true,
        }
    }
    pub fn decode_frames(
        &self,
        codestreams: &[&[u8]],
        metadata: &Metadata,
        options: DecodeOptions,
    ) -> Result<DecodedFrames, Error> {
        self.validate_decode_metadata(metadata)?;
        let frames = metadata.number_of_frames as u64;
        if frames > MAX_FRAMES as u64 || codestreams.len() > MAX_FRAMES {
            return Err(Error::bare(ErrorKind::ResourceLimit));
        }
        if codestreams.is_empty() {
            return Err(Error::new(ErrorKind::EncapsulatedLayout, "no fragments"));
        }
        if frames != codestreams.len() as u64 {
            return Err(Error {
                kind: ErrorKind::PixelDataSizeMismatch,
                message: format!(
                    "{}\n{}: dicom: encapsulated frame count mismatch",
                    ErrorKind::PixelDataSizeMismatch.text(),
                    ErrorKind::EncapsulatedLayout.text()
                ),
            });
        }
        let mut total = 0u64;
        for c in codestreams {
            if c.is_empty() {
                return Err(Error::new(ErrorKind::EncapsulatedLayout, "empty fragment"));
            }
            total = total.saturating_add(c.len() as u64);
            if c.len() as u64 > MAX_FRAME_BYTES || total > MAX_FRAME_BYTES {
                return Err(Error::bare(ErrorKind::ResourceLimit));
            }
        }
        let mut result = DecodedFrames::default();
        let mut data = Vec::with_capacity(codestreams.len());
        for (index, codestream) in codestreams.iter().enumerate() {
            let decoded = self
                .decode_native_frame(codestream, metadata, options, &mut result)
                .map_err(|e| e.wrap(format_args!("decode JPEG-LS frame {index}")))?;
            data.push(decoded);
        }
        result.rows = metadata.rows as usize;
        result.columns = metadata.columns as usize;
        result.data = data;
        Ok(result)
    }
    pub fn decode_frame_region(
        &self,
        codestream: &[u8],
        metadata: &Metadata,
        region: Region,
        options: DecodeOptions,
    ) -> Result<DecodedFrames, Error> {
        if region.is_empty()
            || region.x0 < 0
            || region.y0 < 0
            || region.x1 > metadata.columns as i64
            || region.y1 > metadata.rows as i64
        {
            return Err(Error::bare(ErrorKind::InvalidRegion));
        }
        let (header, entropy, diagnostics) = self.prepare_region(codestream, metadata, options)?;
        let bytes_per_sample = (metadata.bits_allocated / 8) as usize;
        let mut output = vec![
            0u8;
            (region.width() as usize)
                .saturating_mul(region.height() as usize)
                .saturating_mul(bytes_per_sample)
        ];
        decoder::decode_native_scan(
            entropy,
            &header,
            &header.preset,
            &mut output,
            region,
            0,
            bytes_per_sample,
        )?;
        Ok(DecodedFrames {
            rows: region.height() as usize,
            columns: region.width() as usize,
            data: vec![output],
            photometric_interpretation: String::new(),
            diagnostics,
        })
    }
    pub(crate) fn check_region(&self, codestream: &[u8], metadata: &Metadata) -> Result<(), Error> {
        self.prepare_region(codestream, metadata, DecodeOptions::default())
            .map(|_| ())
    }
    fn prepare_region<'a>(
        &self,
        codestream: &'a [u8],
        metadata: &Metadata,
        options: DecodeOptions,
    ) -> Result<(FrameHeader, &'a [u8], Vec<&'static str>), Error> {
        let photometric = normalized_photometric(metadata);
        if metadata.samples_per_pixel != 1
            || photometric != "MONOCHROME1" && photometric != "MONOCHROME2"
        {
            return Err(Error::bare(ErrorKind::RegionUnsupported));
        }
        let mut single = metadata.clone();
        single.number_of_frames = 1;
        self.validate_decode_metadata(&single)?;
        if codestream.len() as u64 > MAX_FRAME_BYTES {
            return Err(Error::bare(ErrorKind::ResourceLimit));
        }
        let mut stream = codestream;
        let pad = trailing_pad(stream);
        if let Some(p) = pad {
            if p != 0 && !stream.len().is_multiple_of(2) {
                return Err(Error::invalid("trailing data after EOI"));
            }
            stream = stream.split_last().map_or(stream, |(_, s)| s);
        }
        let (mut header, scans) = decoder::split_scans(stream, false)?;
        decoder::validate_split_frame(&header, metadata, self.near_lossless)?;
        let mut diagnostics = Vec::new();
        if matches!(pad, Some(p) if p != 0) {
            diagnostics.push(diagnostic::NON_ZERO_FRAME_PAD);
        }
        if options.jai_thresholds && header.precision > 12 && !header.explicit_thresholds {
            header.preset = decoder::with_jai_thresholds(header.preset, header.precision)?;
            diagnostics.push(diagnostic::JAI_THRESHOLDS);
        }
        let entropy = scans.first().map(|s| s.entropy).unwrap_or_default();
        Ok((header, entropy, diagnostics))
    }
    pub(crate) fn check_decode_metadata(&self, metadata: &Metadata) -> Result<(), Error> {
        self.validate_decode_metadata(metadata)
    }
    fn validate_decode_metadata(&self, metadata: &Metadata) -> Result<(), Error> {
        encoder::validate_encoder_input(metadata)?;
        if self.near_lossless
            && metadata
                .photometric_interpretation
                .trim()
                .eq_ignore_ascii_case("PALETTE COLOR")
        {
            return Err(Error::new(
                ErrorKind::UnsupportedPhotometricInterpretation,
                "palette color requires the JPEG-LS Lossless transfer syntax",
            ));
        }
        validate_decode_request_allocation(metadata)
    }
    fn decode_native_frame(
        &self,
        stream: &[u8],
        metadata: &Metadata,
        options: DecodeOptions,
        result: &mut DecodedFrames,
    ) -> Result<Vec<u8>, Error> {
        let pad = trailing_pad(stream);
        let stream = if pad.is_some() {
            stream.split_last().map_or(stream, |(_, s)| s)
        } else {
            stream
        };
        let (header, scans) = decoder::split_scans(stream, options.hp_color_transforms)?;
        let mut header = header;
        if header.hp_transform != 0 {
            if self.near_lossless {
                return Err(Error::invalid(
                    "HP color transforms require the Lossless syntax",
                ));
            }
            decoder::validate_hp_metadata(&header, metadata)?;
        }
        let mut jai = false;
        if options.jai_thresholds && header.precision > 12 && !header.explicit_thresholds {
            header.preset = decoder::with_jai_thresholds(header.preset, header.precision)?;
            jai = true;
        }
        decoder::validate_split_frame(&header, metadata, self.near_lossless)?;
        let bytes_per_sample = (metadata.bits_allocated / 8) as usize;
        let frame_size = (metadata.rows as usize)
            .saturating_mul(metadata.columns as usize)
            .saturating_mul(metadata.samples_per_pixel as usize)
            .saturating_mul(bytes_per_sample);
        let mut native = vec![0u8; frame_size];
        let whole = Region::new(0, 0, header.columns, header.rows);
        for scan in &scans {
            let component = header
                .component_ids
                .iter()
                .position(|&id| Some(&id) == scan.component_ids.first())
                .unwrap_or(0);
            decoder::decode_native_scan(
                scan.entropy,
                &header,
                &header.preset,
                &mut native,
                whole,
                component,
                bytes_per_sample,
            )?;
        }
        if header.hp_transform != 0 {
            decoder::inverse_hp(&mut native, header.precision, header.hp_transform)?;
        }
        if matches!(pad, Some(p) if p != 0) {
            add_diagnostic(result, diagnostic::NON_ZERO_FRAME_PAD);
        }
        if jai {
            add_diagnostic(result, diagnostic::JAI_THRESHOLDS);
        }
        if header.hp_transform != 0 {
            let codes = [diagnostic::HP1, diagnostic::HP2, diagnostic::HP3];
            if let Some(&code) = codes.get((header.hp_transform as usize).wrapping_sub(1)) {
                add_diagnostic(result, code);
            }
            add_diagnostic(result, diagnostic::HP_BACKEND_PURE);
        }
        Ok(native)
    }
}
fn validate_decode_request_allocation(metadata: &Metadata) -> Result<(), Error> {
    let samples = (metadata.rows as u64)
        .saturating_mul(metadata.columns as u64)
        .saturating_mul(metadata.samples_per_pixel as u64);
    let frame_bytes = samples.saturating_mul((metadata.bits_allocated / 8) as u64);
    let frames = metadata.number_of_frames;
    if frame_bytes == 0 || frames <= 0 {
        return Err(Error::invalid("invalid decoded request size"));
    }
    let frames = frames as u64;
    let total = frame_bytes
        .checked_mul(frames)
        .ok_or_else(|| Error::invalid("decoded request size overflow"))?;
    let slices = frames
        .checked_mul(24)
        .ok_or_else(|| Error::invalid("decoded request overhead overflow"))?;
    let planes = samples.saturating_mul(8);
    let sum = total
        .checked_add(slices)
        .and_then(|s| s.checked_add(planes))
        .ok_or_else(|| Error::invalid("decoded request working-set overflow"))?;
    if sum > MAX_WORKING_SET_BYTES {
        return Err(Error::invalid("decoded request exceeds working-set limit"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn md(rows: u16, columns: u16, samples: u16, allocated: u16, stored: u16) -> Metadata {
        Metadata {
            rows,
            columns,
            samples_per_pixel: samples,
            bits_allocated: allocated,
            bits_stored: stored,
            high_bit: stored - 1,
            planar_configuration_present: samples == 3,
            number_of_frames: 1,
            photometric_interpretation: if samples == 3 { "RGB" } else { "MONOCHROME2" }.into(),
            ..Default::default()
        }
    }
    fn frame(m: &Metadata, seed: u32) -> Vec<u8> {
        let n = m.rows as usize * m.columns as usize * m.samples_per_pixel as usize;
        let mask = (1u32 << m.bits_stored) - 1;
        let mut out = Vec::new();
        for i in 0..n as u32 {
            let v = ((i / 7) * 3 + (i.wrapping_mul(seed) >> 29)) & mask;
            if m.bits_allocated == 8 {
                out.push(v as u8);
            } else {
                out.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
        out
    }
    #[test]
    fn round_trips_lossless_and_interleaved() {
        for (samples, allocated, stored) in
            [(1, 8, 8), (1, 16, 12), (1, 8, 2), (3, 8, 8), (3, 16, 16)]
        {
            let m = md(9, 11, samples, allocated, stored);
            let pixels = frame(&m, 2654435761);
            for ilv in 0..=2 {
                let encoder = Encoder::with_options(EncoderOptions {
                    interleave: ilv,
                    ..Default::default()
                })
                .unwrap();
                let stream = encoder.encode_frame(&pixels, &m).unwrap();
                let decoded = Codec::lossless()
                    .decode_frames(&[&stream], &m, DecodeOptions::default())
                    .unwrap();
                assert_eq!(
                    decoded.data[0], pixels,
                    "samples={samples} stored={stored} ilv={ilv}"
                );
            }
        }
    }
    #[test]
    fn near_lossless_stays_within_near() {
        let m = md(8, 13, 1, 16, 12);
        let pixels = frame(&m, 40503);
        let encoder = Encoder::with_options(EncoderOptions {
            near: 3,
            allow_lossy: true,
            interleave: 0,
        })
        .unwrap();
        assert_eq!(
            encoder.capabilities().transfer_syntax_uid,
            UID_NEAR_LOSSLESS
        );
        let stream = encoder.encode_frame(&pixels, &m).unwrap();
        let err = Codec::lossless()
            .decode_frames(&[&stream], &m, DecodeOptions::default())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "decode JPEG-LS frame 0: dicom: invalid JPEG-LS codestream: NEAR=3"
        );
        let decoded = Codec::near_lossless()
            .decode_frames(&[&stream], &m, DecodeOptions::default())
            .unwrap();
        for (a, b) in decoded.data[0].chunks(2).zip(pixels.chunks(2)) {
            let a = u16::from_le_bytes([a[0], a[1]]) as i32;
            let b = u16::from_le_bytes([b[0], b[1]]) as i32;
            assert!((a - b).abs() <= 3);
        }
    }
    #[test]
    fn region_matches_whole_frame() {
        let m = md(10, 12, 1, 8, 8);
        let pixels = frame(&m, 97);
        let stream = Encoder::new().encode_frame(&pixels, &m).unwrap();
        let region = Codec::lossless()
            .decode_frame_region(
                &stream,
                &m,
                Region::new(7, 6, 2, 3),
                DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!((region.rows, region.columns), (3, 5));
        for y in 0..3 {
            for x in 0..5 {
                assert_eq!(region.data[0][y * 5 + x], pixels[(y + 3) * 12 + x + 2]);
            }
        }
        let info = inspect_codestream(&stream, 0, DecodeOptions::default()).unwrap();
        assert_eq!((info.rows, info.columns, info.precision), (10, 12, 8));
    }
}
