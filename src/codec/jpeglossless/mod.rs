#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod decoder;
mod encoder;
mod huffman;
pub use super::common::{DecodedFrames, Metadata};
use crate::textfmt;
pub use encoder::Encoder;
use std::fmt;
pub const UID_PROCESS14: &str = "1.2.840.10008.1.2.4.57";
pub const UID_PROCESS14_SV1: &str = "1.2.840.10008.1.2.4.70";
const MAX_REQUEST_BYTES: u64 = 512 << 20;
pub mod diagnostic {
    pub const NON_ZERO_FRAME_PAD: &str = "encapsulated-nonzero-frame-pad";
    pub const SE_IGNORED: &str = "jpeg-lossless-se-ignored";
    pub const FILL_BITS: &str = "jpeg-lossless-invalid-fill-bits";
    pub const CATEGORY16_EXTRA_BITS: &str = "jpeg-lossless-category16-extra-bits";
    pub const PREDICTOR6_OVERFLOW: &str = "jpeg-lossless-predictor6-overflow";
    pub const YBR_FULL_422_FULL_RESOLUTION: &str = "jpeg-lossless-ybr-full-422-full-resolution";
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidStream,
    UnsupportedScan,
    ImageSizeMismatch,
    UnsupportedBitsAllocated,
    UnsupportedPhotometricInterpretation,
    UnsupportedPixelRepresentation,
    UnsupportedPlanarConfiguration,
    PixelDataSizeMismatch,
    UnsupportedEncoderMetadata,
    EncoderOutputInvalid,
}
impl ErrorKind {
    fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidStream => "dicom: invalid JPEG Lossless stream",
            ErrorKind::UnsupportedScan => "dicom: unsupported JPEG Lossless scan",
            ErrorKind::ImageSizeMismatch => {
                "dicom: JPEG Lossless image size does not match metadata"
            }
            ErrorKind::UnsupportedBitsAllocated => "dicom: unsupported JPEG Lossless BitsAllocated",
            ErrorKind::UnsupportedPhotometricInterpretation => {
                "dicom: unsupported PhotometricInterpretation"
            }
            ErrorKind::UnsupportedPixelRepresentation => "dicom: unsupported PixelRepresentation",
            ErrorKind::UnsupportedPlanarConfiguration => "dicom: unsupported PlanarConfiguration",
            ErrorKind::PixelDataSizeMismatch => "dicom: pixel data size does not match metadata",
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
    fn new(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            message: format!("{}: {detail}", kind.text()),
        }
    }
    fn invalid(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::InvalidStream, detail)
    }
    fn unsupported(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::UnsupportedScan, detail)
    }
    fn size(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::ImageSizeMismatch, detail)
    }
    fn encoder_metadata(field: &str) -> Error {
        Error::new(ErrorKind::UnsupportedEncoderMetadata, field)
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
    pub selection_value: u8,
    pub encoder_quirks: bool,
    pub predictor6_overflow: bool,
}
fn normalized_photometric(metadata: &Metadata) -> String {
    textfmt::to_upper(metadata.photometric_interpretation.trim())
}
pub fn decode_frames(
    codestreams: &[&[u8]],
    metadata: &Metadata,
    options: DecodeOptions,
) -> Result<DecodedFrames, Error> {
    validate_metadata(metadata)?;
    if metadata.number_of_frames <= 0 {
        return Err(Error::invalid("dicom: invalid encapsulated frame layout"));
    }
    if codestreams.is_empty() {
        return Err(Error::invalid(
            "dicom: invalid encapsulated frame layout: no fragments",
        ));
    }
    if (codestreams.len() as u64) < metadata.number_of_frames as u64 {
        return Err(Error::invalid(
            "dicom: pixel data size does not match metadata\n\
             dicom: invalid encapsulated frame layout: dicom: encapsulated frame count mismatch",
        ));
    }
    if codestreams.iter().any(|c| c.is_empty()) {
        return Err(Error::invalid(
            "dicom: invalid encapsulated frame layout: empty fragment",
        ));
    }
    let max_frame = codestreams
        .iter()
        .map(|c| c.len() as u64)
        .max()
        .unwrap_or(0);
    validate_request(
        metadata,
        codestreams.iter().map(|c| c.len() as u64),
        max_frame,
    )?;
    let rows = metadata.rows as usize;
    let columns = metadata.columns as usize;
    let base = decoder::Quirks {
        predictor6_overflow: options.predictor6_overflow,
        ..Default::default()
    };
    let mut result = DecodedFrames::default();
    let mut frames = Vec::with_capacity(codestreams.len());
    for (index, data) in codestreams.iter().enumerate() {
        if let Some(pad) = trailing_pad(data)
            && pad != 0
        {
            result.add_diagnostic(diagnostic::NON_ZERO_FRAME_PAD);
        }
        let (image, quirks) = decoder::decode_frame_with_recovery(
            data,
            options.selection_value,
            metadata,
            base,
            options.encoder_quirks,
        );
        for code in quirks.diagnostics() {
            result.add_diagnostic(code);
        }
        let image = image.map_err(|e| Error::invalid(format_args!("frame {index}: {e}")))?;
        if image.width != columns || image.height != rows {
            return Err(Error::size(format_args!(
                "got {}x{} want {}x{}",
                image.width, image.height, columns, rows
            )));
        }
        if image.precision != metadata.bits_stored as usize {
            return Err(Error::size(format_args!(
                "precision={} BitsStored={}",
                image.precision, metadata.bits_stored
            )));
        }
        if image.planes.len() != metadata.samples_per_pixel as usize {
            return Err(Error::size(format_args!(
                "components={} SamplesPerPixel={}",
                image.planes.len(),
                metadata.samples_per_pixel
            )));
        }
        frames.push(image.into_interleaved_bytes(metadata.bits_allocated));
    }
    if normalized_photometric(metadata) == "YBR_FULL_422" {
        result.photometric_interpretation = "YBR_FULL".to_string();
        result.add_diagnostic(diagnostic::YBR_FULL_422_FULL_RESOLUTION);
    }
    result.rows = rows;
    result.columns = columns;
    result.data = frames;
    Ok(result)
}
fn trailing_pad(data: &[u8]) -> Option<u8> {
    match data {
        [.., 0xff, 0xd9, pad] => Some(*pad),
        _ => None,
    }
}
#[allow(clippy::arithmetic_side_effects)]
fn validate_metadata(metadata: &Metadata) -> Result<(), Error> {
    if metadata.bits_allocated != 8 && metadata.bits_allocated != 16 {
        return Err(Error::new(
            ErrorKind::UnsupportedBitsAllocated,
            format_args!("BitsAllocated={}", metadata.bits_allocated),
        ));
    }
    if metadata.bits_stored == 0
        || metadata.bits_stored > metadata.bits_allocated
        || metadata.high_bit != metadata.bits_stored - 1
    {
        return Err(Error::new(
            ErrorKind::UnsupportedBitsAllocated,
            "invalid BitsStored/HighBit",
        ));
    }
    let photometric = normalized_photometric(metadata);
    let unsupported_photometric = || {
        Error::new(
            ErrorKind::UnsupportedPhotometricInterpretation,
            format_args!(
                "PhotometricInterpretation={}",
                metadata.photometric_interpretation.trim()
            ),
        )
    };
    match metadata.samples_per_pixel {
        1 => {
            if photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
                return Err(unsupported_photometric());
            }
        }
        3 => {
            if photometric != "RGB" && photometric != "YBR_FULL" && photometric != "YBR_FULL_422" {
                return Err(unsupported_photometric());
            }
            if metadata.pixel_representation != 0 {
                return Err(Error::new(
                    ErrorKind::UnsupportedPixelRepresentation,
                    format_args!(
                        "color PixelRepresentation={}",
                        metadata.pixel_representation
                    ),
                ));
            }
            if !metadata.planar_configuration_present || metadata.planar_configuration != 0 {
                return Err(Error::new(
                    ErrorKind::UnsupportedPlanarConfiguration,
                    format_args!("PlanarConfiguration={}", metadata.planar_configuration),
                ));
            }
        }
        n => {
            return Err(Error::unsupported(format_args!("SamplesPerPixel={n}")));
        }
    }
    Ok(())
}
#[allow(clippy::arithmetic_side_effects)]
fn validate_request(
    metadata: &Metadata,
    fragments: impl Iterator<Item = u64>,
    joined_bytes: u64,
) -> Result<(), Error> {
    let frames = metadata.number_of_frames as u64;
    let pixels = metadata.rows as u64 * metadata.columns as u64;
    let samples = pixels * metadata.samples_per_pixel as u64;
    let bytes_per_sample = (metadata.bits_allocated / 8) as u64;
    if metadata.number_of_frames <= 0
        || samples == 0
        || bytes_per_sample == 0
        || samples > u64::MAX / bytes_per_sample
    {
        return Err(Error::invalid("decoded request size overflow"));
    }
    let frame_output = samples * bytes_per_sample;
    if frame_output > u64::MAX / frames {
        return Err(Error::invalid("decoded request size overflow"));
    }
    let total = frame_output * frames;
    let word_bytes = usize::BITS as u64 / 8;
    if frames > u64::MAX / (6 * word_bytes) {
        return Err(Error::invalid("decoded request overhead overflow"));
    }
    let total = total
        .checked_add(frames * 6 * word_bytes)
        .ok_or_else(|| Error::invalid("decoded request overhead overflow"))?;
    let total = total
        .checked_add(joined_bytes)
        .ok_or_else(|| Error::invalid("joined frame size overflow"))?;
    if samples > u64::MAX / 4 {
        return Err(Error::invalid("decoded working-set overflow"));
    }
    let mut total = total
        .checked_add(samples * 4)
        .ok_or_else(|| Error::invalid("decoded working-set overflow"))?;
    for fragment in fragments {
        match total.checked_add(fragment) {
            Some(t) if t <= MAX_REQUEST_BYTES => total = t,
            _ => return Err(Error::invalid("decoded request exceeds resource limit")),
        }
    }
    if total > MAX_REQUEST_BYTES {
        return Err(Error::invalid("decoded request exceeds resource limit"));
    }
    Ok(())
}
struct DecodedImage {
    width: usize,
    height: usize,
    precision: usize,
    planes: Vec<Vec<i32>>,
    point_transform: Vec<u32>,
    packed_mono: Option<Vec<u8>>,
}
impl DecodedImage {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn into_interleaved_bytes(self, bits_allocated: u16) -> Vec<u8> {
        if let Some(packed) = self.packed_mono {
            return packed;
        }
        let pixels = self.width * self.height;
        let components = self.planes.len();
        let bytes_per_sample = (bits_allocated / 8) as usize;
        let mut out = vec![0u8; pixels * components * bytes_per_sample];
        for (component, plane) in self.planes.iter().enumerate() {
            let shift = self.point_transform[component];
            if bytes_per_sample == 1 {
                for (o, &sample) in out[component..].iter_mut().step_by(components).zip(plane) {
                    *o = (sample << shift) as u8;
                }
            } else {
                for (o, &sample) in out[component * 2..].chunks_mut(components * 2).zip(plane) {
                    o[..2].copy_from_slice(&((sample << shift) as u16).to_le_bytes());
                }
            }
        }
        out
    }
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
    #[test]
    fn round_trips_every_predictor() {
        for (samples, allocated, stored) in [(1, 8, 8), (1, 16, 12), (3, 8, 8), (3, 16, 16)] {
            let m = md(7, 9, samples, allocated, stored);
            let mask = (1u32 << stored) - 1;
            let n = 7 * 9 * samples as usize;
            let mut frame = Vec::new();
            for i in 0..n as u32 {
                let v = (i.wrapping_mul(2654435761) >> 7) & mask;
                if allocated == 8 {
                    frame.push(v as u8);
                } else {
                    frame.extend_from_slice(&(v as u16).to_le_bytes());
                }
            }
            for sv in 0..=7 {
                let encoder = if sv == 0 {
                    Encoder::sv1()
                } else {
                    Encoder::process14(sv).unwrap()
                };
                let stream = encoder.encode_frame(&frame, &m).unwrap();
                let options = DecodeOptions {
                    selection_value: (sv <= 1) as u8,
                    ..Default::default()
                };
                let decoded = decode_frames(&[&stream], &m, options).unwrap();
                assert_eq!(
                    decoded.data[0], frame,
                    "samples={samples} stored={stored} sv={sv}"
                );
            }
        }
    }
    #[test]
    fn errors_keep_their_text() {
        let m = md(1, 1, 1, 8, 8);
        let err = decode_frames(&[b"\x00\x00\x00\x00"], &m, DecodeOptions::default()).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidStream);
        assert_eq!(
            err.to_string(),
            "dicom: invalid JPEG Lossless stream: frame 0: dicom: invalid JPEG Lossless stream: missing SOI"
        );
        assert_eq!(
            Encoder::process14(8).unwrap_err().to_string(),
            "dicom: pixel metadata is unsupported by encoder: SelectionValue"
        );
    }
}
