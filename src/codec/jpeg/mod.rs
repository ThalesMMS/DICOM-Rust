#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod encoder;
mod extended;
mod extended_encoder;
mod progressive12;
pub(crate) mod stdjpeg;
mod ycbcr;
pub use super::common::{ColorConversion, DecodedFrames, Metadata};
use crate::textfmt;
pub use encoder::{EncodedFrame, Encoder, EncoderCapabilities, EncoderOptions, Subsampling};
pub use extended_encoder::{ExtendedEncoder, ExtendedEncoderOptions};
use std::fmt;
use stdjpeg::image::Image;
pub const UID: &str = "1.2.840.10008.1.2.4.50";
pub const UID_EXTENDED: &str = "1.2.840.10008.1.2.4.51";
pub const UID_SPECTRAL_SELECTION: &str = "1.2.840.10008.1.2.4.53";
pub const UID_FULL_PROGRESSION: &str = "1.2.840.10008.1.2.4.55";
pub fn transfer_syntax_uids() -> Vec<&'static str> {
    vec![
        UID,
        UID_EXTENDED,
        UID_SPECTRAL_SELECTION,
        UID_FULL_PROGRESSION,
    ]
}
pub const DEFAULT_QUALITY: i64 = 90;
pub const MIN_QUALITY: i64 = 1;
pub const MAX_QUALITY: i64 = 100;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePixels<'a> {
    Gray(&'a [u8]),
    Rgb(&'a [u8]),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageEncodeError {
    BufferSize { expected: usize, got: usize },
    TooLarge,
}
impl fmt::Display for ImageEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageEncodeError::BufferSize { expected, got } => {
                write!(f, "jpeg: image buffer has {got} bytes, want {expected}")
            }
            ImageEncodeError::TooLarge => f.write_str("jpeg: image is too large to encode"),
        }
    }
}
impl std::error::Error for ImageEncodeError {}
pub fn encode_image(
    pixels: ImagePixels<'_>,
    width: usize,
    height: usize,
    quality: i64,
) -> Result<Vec<u8>, ImageEncodeError> {
    if width >= 1 << 16 || height >= 1 << 16 {
        return Err(ImageEncodeError::TooLarge);
    }
    let (source, data, per_pixel) = match pixels {
        ImagePixels::Gray(p) => (stdjpeg::Source::Gray(p), p, 1),
        ImagePixels::Rgb(p) => (stdjpeg::Source::Rgb(p), p, 3),
    };
    let expected = width.saturating_mul(height).saturating_mul(per_pixel);
    if data.len() != expected {
        return Err(ImageEncodeError::BufferSize {
            expected,
            got: data.len(),
        });
    }
    Ok(stdjpeg::encode(source, width, height, quality))
}
const EXTENDED_PRECISION: usize = 12;
const MAX_REQUEST_BYTES: u64 = 512 << 20;
pub mod diagnostic {
    pub const NON_ZERO_FRAME_PAD: &str = "encapsulated-nonzero-frame-pad";
    pub const BASELINE_UNDER_EXTENDED: &str = "jpeg-sof0-under-extended";
    pub const SCAN_PARAMETERS_IGNORED: &str = "jpeg-sequential-scan-parameters-ignored";
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidFragment,
    UnsupportedBitsAllocated,
    UnsupportedBitsStored,
    UnsupportedHighBit,
    UnsupportedSamplesPerPixel,
    ImageSizeMismatch,
    UnsupportedPhotometricInterpretation,
    UnsupportedPixelRepresentation,
    UnsupportedPlanarConfiguration,
    PixelDataSizeMismatch,
    UnsupportedEncoderMetadata,
    EncoderOutputInvalid,
    InvalidQuality,
    InvalidSubsampling,
    InvalidExtendedEncoderLimits,
    ExtendedEncoderResourceLimit,
    ProgressiveResourceLimit,
}
impl ErrorKind {
    fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidFragment => "dicom: invalid JPEG still-image fragment",
            ErrorKind::UnsupportedBitsAllocated => {
                "dicom: unsupported JPEG still-image BitsAllocated"
            }
            ErrorKind::UnsupportedBitsStored => "dicom: unsupported JPEG still-image BitsStored",
            ErrorKind::UnsupportedHighBit => "dicom: unsupported JPEG still-image HighBit",
            ErrorKind::UnsupportedSamplesPerPixel => {
                "dicom: unsupported JPEG still-image SamplesPerPixel"
            }
            ErrorKind::ImageSizeMismatch => "dicom: JPEG still-image size does not match metadata",
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
            ErrorKind::InvalidQuality => "dicom: JPEG quality must be between 1 and 100",
            ErrorKind::InvalidSubsampling => {
                "dicom: JPEG subsampling must be 4:2:0, 4:2:2 or 4:4:4"
            }
            ErrorKind::InvalidExtendedEncoderLimits => {
                "dicom: invalid JPEG Extended encoder limits"
            }
            ErrorKind::ExtendedEncoderResourceLimit => {
                "dicom: JPEG Extended encoder resource limit exceeded"
            }
            ErrorKind::ProgressiveResourceLimit => "dicom: JPEG progressive resource limit",
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
    fn fragment(detail: impl fmt::Display) -> Error {
        Error::new(ErrorKind::InvalidFragment, detail)
    }
    fn prefixed(self, prefix: impl fmt::Display) -> Error {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Syntax {
    #[default]
    Auto,
    Baseline,
    Extended,
    SpectralSelection,
    FullProgression,
}
impl Syntax {
    pub fn from_uid(uid: &str) -> Option<Syntax> {
        match uid {
            UID => Some(Syntax::Baseline),
            UID_EXTENDED => Some(Syntax::Extended),
            UID_SPECTRAL_SELECTION => Some(Syntax::SpectralSelection),
            UID_FULL_PROGRESSION => Some(Syntax::FullProgression),
            _ => None,
        }
    }
    fn progressive(self) -> bool {
        matches!(self, Syntax::SpectralSelection | Syntax::FullProgression)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeOptions {
    pub syntax: Syntax,
    pub color: ColorConversion,
    pub sequential_scan_quirks: bool,
}
fn add_diagnostic(frames: &mut DecodedFrames, code: &'static str) {
    if !frames.diagnostics.contains(&code) {
        frames.diagnostics.push(code);
    }
}
fn normalized(photometric: &str) -> String {
    textfmt::to_upper(photometric.trim())
}
fn bytes_per_sample(m: &Metadata) -> u16 {
    m.bits_allocated.wrapping_add(7) / 8
}
fn trailing_pad(data: &[u8]) -> Option<u8> {
    match data {
        [.., 0xff, 0xd9, pad] => Some(*pad),
        _ => None,
    }
}
fn be16_at(stream: &[u8], pos: usize) -> Option<usize> {
    let &[hi, lo] = stream.get(pos..pos.checked_add(2)?)? else {
        return None;
    };
    Some(usize::from(u16::from_be_bytes([hi, lo])))
}
fn supported_photometric(m: &Metadata) -> bool {
    let p = normalized(&m.photometric_interpretation);
    match m.samples_per_pixel {
        1 => p == "MONOCHROME1" || p == "MONOCHROME2",
        3 => p == "RGB" || p == "YBR_FULL" || p == "YBR_FULL_422",
        _ => false,
    }
}
pub fn decode_frames(
    codestreams: &[&[u8]],
    metadata: &Metadata,
    options: DecodeOptions,
) -> Result<DecodedFrames, Error> {
    use ErrorKind::*;
    let m = metadata;
    let mode = options.syntax;
    let policy = options.color;
    let progressive12 = mode.progressive() && m.bits_allocated == 16;
    let high_precision = m.bits_allocated == 16
        && (mode == Syntax::Auto || mode == Syntax::Extended || progressive12);
    let ba = format_args!("BitsAllocated={}", m.bits_allocated).to_string();
    if mode == Syntax::Baseline && m.bits_allocated != 8 {
        return Err(Error::new(UnsupportedBitsAllocated, ba));
    }
    if mode.progressive() && m.bits_allocated != 8 && !progressive12 {
        return Err(Error::new(UnsupportedBitsAllocated, ba));
    }
    if progressive12 && m.bits_stored != 12 {
        return Err(Error::new(
            UnsupportedBitsStored,
            "progressive12 requires BitsStored=12",
        ));
    }
    if m.bits_allocated != 8 && !high_precision {
        return Err(Error::new(UnsupportedBitsAllocated, ba));
    }
    if !high_precision && (m.bits_stored == 0 || m.bits_stored > 8) {
        return Err(Error::new(
            UnsupportedBitsStored,
            format_args!("BitsStored={}", m.bits_stored),
        ));
    }
    if high_precision && (m.bits_stored <= 8 || m.bits_stored as usize > EXTENDED_PRECISION) {
        return Err(Error::new(
            UnsupportedBitsAllocated,
            format_args!("BitsStored={}", m.bits_stored),
        ));
    }
    if m.high_bit != m.bits_stored.wrapping_sub(1) {
        return Err(Error::new(
            UnsupportedHighBit,
            format_args!("HighBit={}", m.high_bit),
        ));
    }
    if m.samples_per_pixel != 1 && m.samples_per_pixel != 3 {
        return Err(Error::new(
            UnsupportedSamplesPerPixel,
            format_args!("SamplesPerPixel={}", m.samples_per_pixel),
        ));
    }
    if m.pixel_representation > 1 || (m.pixel_representation == 1 && m.samples_per_pixel != 1) {
        return Err(Error::new(
            UnsupportedPixelRepresentation,
            format_args!("PixelRepresentation={}", m.pixel_representation),
        ));
    }
    if m.planar_configuration_present && m.planar_configuration != 0 {
        return Err(Error::new(
            UnsupportedPlanarConfiguration,
            format_args!("PlanarConfiguration={}", m.planar_configuration),
        ));
    }
    if !supported_photometric(m) {
        return Err(Error::new(
            UnsupportedPhotometricInterpretation,
            format_args!(
                "PhotometricInterpretation={} SamplesPerPixel={}",
                m.photometric_interpretation.trim(),
                m.samples_per_pixel
            ),
        ));
    }
    if progressive12 {
        if m.pixel_representation != 0 {
            return Err(Error::bare(UnsupportedPixelRepresentation));
        }
        if m.samples_per_pixel == 1 && m.planar_configuration_present
            || m.samples_per_pixel == 3 && !m.planar_configuration_present
        {
            return Err(Error::bare(UnsupportedPlanarConfiguration));
        }
        if m.samples_per_pixel == 3
            && (normalized(&m.photometric_interpretation) != "RGB"
                || policy == ColorConversion::Always)
        {
            return Err(Error::bare(UnsupportedPhotometricInterpretation));
        }
    }
    let layout = "dicom: invalid encapsulated frame layout";
    if m.number_of_frames <= 0 {
        return Err(Error::fragment(layout));
    }
    if m.number_of_frames > 100_000 || codestreams.len() > 100_000 {
        return Err(Error::fragment(
            "dicom: encapsulated frame resource limit exceeded",
        ));
    }
    if codestreams.is_empty() {
        return Err(Error::fragment(format_args!("{layout}: no fragments")));
    }
    let frames_wanted = m.number_of_frames as usize;
    if frames_wanted > codestreams.len() {
        return Err(Error::new(
            InvalidFragment,
            format_args!(
                "dicom: pixel data size does not match metadata\n{layout}: dicom: encapsulated frame count mismatch"
            ),
        ));
    }
    let mut input_bytes = 0u64;
    for c in codestreams {
        if c.is_empty() {
            return Err(Error::fragment(format_args!("{layout}: empty fragment")));
        }
        if c.len() as u64 > MAX_REQUEST_BYTES.saturating_sub(input_bytes) {
            return Err(Error::fragment(
                "dicom: encapsulated frame resource limit exceeded",
            ));
        }
        input_bytes = input_bytes.saturating_add(c.len() as u64);
    }
    if codestreams.len() != frames_wanted {
        return Err(Error::fragment(format_args!(
            "{layout}: dicom: encapsulated frame count mismatch"
        )));
    }
    let max_frame_bytes = codestreams
        .iter()
        .map(|c| c.len() as u64)
        .max()
        .unwrap_or(0);
    if high_precision && !progressive12 {
        extended::validate_request(m, codestreams).map_err(Error::fragment)?;
    }
    if progressive12 {
        progressive12::request_bytes(m, input_bytes, max_frame_bytes).map_err(Error::fragment)?;
    }
    let frame_bytes = (m.rows as i64)
        .saturating_mul(m.columns as i64)
        .saturating_mul(m.samples_per_pixel as i64)
        .saturating_mul(bytes_per_sample(m) as i64);
    let frames_count = m.number_of_frames as u64;
    if frame_bytes <= 0
        || frame_bytes as u64 > MAX_REQUEST_BYTES.checked_div(frames_count).unwrap_or(0)
    {
        return Err(Error::fragment("decoded request exceeds resource limit"));
    }
    let request_bytes = (frame_bytes as u64)
        .saturating_mul(frames_count)
        .saturating_add(input_bytes)
        .saturating_add(max_frame_bytes);
    let rgba_bytes = (m.rows as u64)
        .saturating_mul(m.columns as u64)
        .saturating_mul(4);
    if request_bytes.saturating_add(rgba_bytes) > MAX_REQUEST_BYTES {
        return Err(Error::fragment(
            "request working set exceeds resource limit",
        ));
    }
    let mut result = DecodedFrames::default();
    let mut frames = Vec::with_capacity(codestreams.len());
    let mut progressive_work = progressive12::WorkBudget::default();
    for (i, &fragment) in codestreams.iter().enumerate() {
        if let Some(pad) = trailing_pad(fragment)
            && pad != 0
        {
            add_diagnostic(&mut result, diagnostic::NON_ZERO_FRAME_PAD);
        }
        let frame_err = |e: String| Error::fragment(format_args!("frame {i}: {e}"));
        let baseline_candidate =
            !high_precision && (mode == Syntax::Baseline || mode == Syntax::Auto);
        let mut header = BaselineHeader::default();
        let sof = if progressive12 {
            0xc2
        } else if baseline_candidate {
            header = probe_baseline_header(fragment).map_err(frame_err)?;
            header.sof
        } else {
            jpeg_frame_sof_marker(fragment).map_err(frame_err)?
        };
        if mode == Syntax::Baseline && sof != 0xc0 {
            return Err(frame_err("JPEG Baseline requires SOF0".into()));
        }
        if mode == Syntax::Extended && sof != 0xc1 {
            if sof != 0xc0 || high_precision {
                return Err(frame_err(
                    "JPEG Extended requires SOF1 (or Baseline SOF0 at 8 bits)".into(),
                ));
            }
            add_diagnostic(&mut result, diagnostic::BASELINE_UNDER_EXTENDED);
        }
        if mode.progressive() && sof != 0xc2 {
            return Err(frame_err(
                "retired JPEG progressive syntaxes require SOF2".into(),
            ));
        }
        if progressive12 {
            let frame = progressive12::decode(
                fragment,
                m,
                mode == Syntax::SpectralSelection,
                &mut progressive_work,
            )
            .map_err(frame_err)?;
            frames.push(frame);
            if m.samples_per_pixel == 3 {
                result.photometric_interpretation = "RGB".to_string();
            }
            continue;
        }
        if high_precision {
            if sof != 0xc1 {
                return Err(Error::new(
                    UnsupportedBitsAllocated,
                    format_args!(
                        "BitsAllocated={} requires JPEG Extended SOF1",
                        m.bits_allocated
                    ),
                ));
            }
            let reserved = request_bytes.saturating_sub(frame_bytes as u64);
            let decoded = extended::decode(fragment, m, options.sequential_scan_quirks, reserved)
                .map_err(frame_err)?;
            if decoded.scan_parameters_ignored {
                add_diagnostic(&mut result, diagnostic::SCAN_PARAMETERS_IGNORED);
            }
            let mut frame = decoded.data;
            if m.samples_per_pixel == 3 {
                let convert = converts_ycbcr(policy, m, decoded.ycbcr);
                if convert {
                    ycbcr_to_rgb16(&mut frame, EXTENDED_PRECISION);
                }
                result.photometric_interpretation = output_photometric(policy, m, convert);
            }
            limit_stored_bits16(&mut frame, m);
            frames.push(frame);
            continue;
        }
        validate_frame(fragment).map_err(|e| Error::fragment(format_args!("{layout}: {e}")))?;
        let baseline = baseline_candidate && sof == 0xc0;
        let img = if baseline {
            decode_baseline_image(fragment, m, &header, i)?
        } else {
            let (w, h) = stdjpeg::decode_config(fragment).map_err(Error::fragment)?;
            if w != m.columns as usize || h != m.rows as usize {
                return Err(Error::bare(ImageSizeMismatch));
            }
            stdjpeg::decode(fragment).map_err(|e| frame_err(e.to_string()))?
        };
        let frame = if baseline {
            baseline_image_to_frame_bytes(&img, m, policy)
        } else {
            image_to_frame_bytes_policy(&img, m, policy)
        };
        let mut frame =
            frame.map_err(|e| e.prefixed(format_args!("decode JPEG still-image frame {i}")))?;
        if m.samples_per_pixel == 3 {
            let std_ycbcr = matches!(img, Image::YCbCr(_));
            result.photometric_interpretation =
                output_photometric(policy, m, converts_ycbcr(policy, m, std_ycbcr));
        }
        limit_stored_bits8(&mut frame, m);
        frames.push(frame);
    }
    result.rows = m.rows as usize;
    result.columns = m.columns as usize;
    result.data = frames;
    Ok(result)
}
#[derive(Debug, Clone, Copy, Default)]
struct BaselineHeader {
    sof: u8,
    width: usize,
    height: usize,
    admitted: bool,
}
fn is_sof(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}
fn probe_baseline_header(stream: &[u8]) -> Result<BaselineHeader, String> {
    if stream.len() < 4 || !stream.starts_with(&[0xff, 0xd8]) {
        return Err("missing JPEG SOI".into());
    }
    let mut pos = 2usize;
    while let Some(&first) = stream.get(pos) {
        if first != 0xff {
            return Err(format!("expected JPEG marker at byte {pos}"));
        }
        while stream.get(pos) == Some(&0xff) {
            pos = pos.saturating_add(1);
        }
        let Some(&marker) = stream.get(pos).filter(|&&b| b != 0) else {
            return Err("truncated or stuffed JPEG marker".into());
        };
        pos = pos.saturating_add(1);
        if is_sof(marker) {
            let mut header = BaselineHeader {
                sof: marker,
                ..Default::default()
            };
            if marker != 0xc0 {
                return Ok(header);
            }
            let Some(length) = be16_at(stream, pos) else {
                return Ok(header);
            };
            if length < 8 || length > stream.len().saturating_sub(pos) {
                return Ok(header);
            }
            let Some(sof) = stream.get(pos.saturating_add(2)..pos.saturating_add(length)) else {
                return Ok(header);
            };
            let &[precision, h0, h1, w0, w1, components, ..] = sof else {
                return Ok(header);
            };
            let components = usize::from(components);
            if precision != 8
                || (components != 1 && components != 3)
                || sof.len() != components.saturating_mul(3).saturating_add(6)
            {
                return Ok(header);
            }
            header.height = usize::from(u16::from_be_bytes([h0, h1]));
            header.width = usize::from(u16::from_be_bytes([w0, w1]));
            header.admitted = header.width > 0 && header.height > 0;
            return Ok(header);
        }
        match marker {
            0xd8..=0xda => return Err("JPEG frame has no SOF marker".into()),
            0xd0..=0xd7 => return Err("JPEG restart marker before SOF".into()),
            _ => {}
        }
        let Some(length) = be16_at(stream, pos) else {
            return Err("truncated JPEG segment length".into());
        };
        if length < 2 || length > stream.len().saturating_sub(pos) {
            return Err("invalid JPEG segment length".into());
        }
        pos = pos.saturating_add(length);
    }
    Err("JPEG frame has no SOF marker".into())
}
fn jpeg_frame_sof_marker(stream: &[u8]) -> Result<u8, String> {
    if stream.len() < 4 || !stream.starts_with(&[0xff, 0xd8]) {
        return Err("missing JPEG SOI".into());
    }
    let mut pos = 2usize;
    while let Some(&first) = stream.get(pos) {
        if first != 0xff {
            return Err(format!("expected JPEG marker at byte {pos}"));
        }
        while stream.get(pos) == Some(&0xff) {
            pos = pos.saturating_add(1);
        }
        let Some(&marker) = stream.get(pos).filter(|&&b| b != 0) else {
            return Err("truncated or stuffed JPEG marker".into());
        };
        pos = pos.saturating_add(1);
        if is_sof(marker) {
            return Ok(marker);
        }
        match marker {
            0xd8..=0xda => return Err("JPEG frame has no SOF marker".into()),
            0xd0..=0xd7 => return Err("JPEG restart marker before SOF".into()),
            _ => {}
        }
        let Some(length) = be16_at(stream, pos) else {
            return Err("truncated JPEG segment length".into());
        };
        if length < 2 || length > stream.len().saturating_sub(pos) {
            return Err("invalid JPEG segment length".into());
        }
        pos = pos.saturating_add(length);
    }
    Err("JPEG frame has no SOF marker".into())
}
fn validate_frame(data: &[u8]) -> Result<(), &'static str> {
    let mut s = MarkerScanner::default();
    for (i, &b) in data.iter().enumerate() {
        if s.feed(b)? {
            let rest = data.get(i.saturating_add(1)..).unwrap_or_default();
            if rest.len() > 1
                || matches!(rest, [pad] if !(data.len().is_multiple_of(2) || *pad == 0))
            {
                return Err("trailing bytes after EOI");
            }
            return Ok(());
        }
    }
    Err("missing EOI")
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ScanState {
    #[default]
    Soi,
    SoiCode,
    Marker,
    MarkerCode,
    LengthHigh,
    LengthLow,
    Payload,
    Entropy,
    EntropyCode,
    End,
}
#[derive(Debug, Default)]
struct MarkerScanner {
    state: ScanState,
    length: usize,
    remaining: usize,
    marker: u8,
    resume_entropy: bool,
}
impl MarkerScanner {
    fn feed(&mut self, b: u8) -> Result<bool, &'static str> {
        match self.state {
            ScanState::Soi => {
                if b != 0xff {
                    return Err("missing SOI");
                }
                self.state = ScanState::SoiCode;
            }
            ScanState::SoiCode => {
                if b != 0xd8 {
                    return Err("missing SOI");
                }
                self.state = ScanState::Marker;
            }
            ScanState::Marker => {
                if b != 0xff {
                    return Err("expected marker");
                }
                self.state = ScanState::MarkerCode;
            }
            ScanState::MarkerCode => {
                if b == 0xff {
                    return Ok(false);
                }
                return self.begin_marker(b, false);
            }
            ScanState::LengthHigh => {
                self.length = (b as usize) << 8;
                self.state = ScanState::LengthLow;
            }
            ScanState::LengthLow => {
                self.length |= b as usize;
                if self.length < 2 {
                    return Err("invalid marker length");
                }
                self.remaining = self.length.saturating_sub(2);
                self.state = ScanState::Payload;
                if self.remaining == 0 {
                    self.end_segment();
                }
            }
            ScanState::Payload => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining == 0 {
                    self.end_segment();
                }
            }
            ScanState::Entropy => {
                if b == 0xff {
                    self.state = ScanState::EntropyCode;
                }
            }
            ScanState::EntropyCode => {
                if b == 0xff {
                    return Ok(false);
                }
                if b == 0 {
                    self.state = ScanState::Entropy;
                    return Ok(false);
                }
                return self.begin_marker(b, true);
            }
            ScanState::End => return Err("data after EOI"),
        }
        Ok(false)
    }
    fn begin_marker(&mut self, b: u8, entropy: bool) -> Result<bool, &'static str> {
        if b == 0 || b == 0xd8 {
            return Err("invalid/nested marker");
        }
        if b == 0xd9 {
            self.state = ScanState::End;
            return Ok(true);
        }
        if b == 0x01 || (0xd0..=0xd7).contains(&b) {
            if !entropy {
                return Err("standalone marker outside scan");
            }
            self.state = ScanState::Entropy;
            return Ok(false);
        }
        self.marker = b;
        self.resume_entropy = entropy && b == 0xdc;
        self.state = ScanState::LengthHigh;
        Ok(false)
    }
    fn end_segment(&mut self) {
        self.state = if self.marker == 0xda || self.resume_entropy {
            ScanState::Entropy
        } else {
            ScanState::Marker
        };
    }
}
fn decode_baseline_image(
    stream: &[u8],
    m: &Metadata,
    header: &BaselineHeader,
    frame_index: usize,
) -> Result<Image, Error> {
    let mut checked_config = false;
    if !header.admitted || header.width != m.columns as usize || header.height != m.rows as usize {
        let (w, h) = stdjpeg::decode_config(stream).map_err(Error::fragment)?;
        if w != m.columns as usize || h != m.rows as usize {
            return Err(Error::bare(ErrorKind::ImageSizeMismatch));
        }
        checked_config = true;
    }
    match stdjpeg::decode(stream) {
        Ok(img) => Ok(img),
        Err(err) => {
            if !checked_config && let Err(config_err) = stdjpeg::decode_config(stream) {
                return Err(Error::fragment(config_err));
            }
            Err(Error::fragment(format_args!("frame {frame_index}: {err}")))
        }
    }
}
fn size_mismatch(img: &Image, m: &Metadata) -> Error {
    let (w, h) = img.bounds();
    Error::new(
        ErrorKind::ImageSizeMismatch,
        format_args!("got {w}x{h} want {}x{}", m.columns, m.rows),
    )
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn baseline_image_to_frame_bytes(
    img: &Image,
    m: &Metadata,
    policy: ColorConversion,
) -> Result<Vec<u8>, Error> {
    let gray = match img {
        Image::Gray(g) if m.samples_per_pixel == 1 => g,
        _ => return image_to_frame_bytes_policy(img, m, policy),
    };
    let (rows, columns) = (m.rows as usize, m.columns as usize);
    if gray.width != columns || gray.height != rows {
        return Err(size_mismatch(img, m));
    }
    if rows == 0
        || columns == 0
        || gray.stride < columns
        || gray.pix.len() < columns
        || rows.saturating_sub(1)
            > gray
                .pix
                .len()
                .saturating_sub(columns)
                .checked_div(gray.stride)
                .unwrap_or(0)
    {
        return Err(Error::fragment("decoded grayscale layout"));
    }
    let mut out = vec![0u8; rows * columns];
    for y in 0..rows {
        let source = y * gray.stride;
        out[y * columns..(y + 1) * columns].copy_from_slice(&gray.pix[source..source + columns]);
    }
    Ok(out)
}
fn image_to_frame_bytes(img: &Image, m: &Metadata) -> Result<Vec<u8>, Error> {
    let (w, h) = img.bounds();
    let (rows, columns) = (m.rows as usize, m.columns as usize);
    if w != columns || h != rows {
        return Err(size_mismatch(img, m));
    }
    match m.samples_per_pixel {
        1 => {
            let mut out = vec![0u8; rows.saturating_mul(columns)];
            for (y, line) in out.chunks_exact_mut(columns.max(1)).enumerate() {
                for (x, v) in line.iter_mut().enumerate() {
                    *v = img.gray_at(x, y);
                }
            }
            Ok(out)
        }
        3 => {
            if let Image::YCbCr(ycc) = img
                && let Some(out) = ycbcr::ycbcr_to_rgb_frame(ycc, rows, columns)
            {
                return Ok(out);
            }
            let mut out = vec![0u8; rows.saturating_mul(columns).saturating_mul(3)];
            let line = columns.saturating_mul(3).max(1);
            for (y, row) in out.chunks_exact_mut(line).enumerate() {
                for (x, px) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                    let (r, g, b) = img.rgb_at(x, y);
                    *px = [r, g, b];
                }
            }
            Ok(out)
        }
        n => Err(Error::new(
            ErrorKind::UnsupportedSamplesPerPixel,
            format_args!("SamplesPerPixel={n}"),
        )),
    }
}
fn converts_ycbcr(policy: ColorConversion, m: &Metadata, codestream_ycbcr: bool) -> bool {
    match policy {
        ColorConversion::Always => true,
        ColorConversion::Never => false,
        ColorConversion::Photometric => {
            normalized(&m.photometric_interpretation).starts_with("YBR")
        }
        ColorConversion::Guess => codestream_ycbcr,
    }
}
fn output_photometric(policy: ColorConversion, m: &Metadata, converted: bool) -> String {
    if converted || policy == ColorConversion::Guess {
        return "RGB".to_string();
    }
    let p = normalized(&m.photometric_interpretation);
    if p.starts_with("YBR") {
        return "YBR_FULL".to_string();
    }
    p
}
fn image_to_frame_bytes_policy(
    img: &Image,
    m: &Metadata,
    policy: ColorConversion,
) -> Result<Vec<u8>, Error> {
    if m.samples_per_pixel != 3 || policy == ColorConversion::Guess {
        return image_to_frame_bytes(img, m);
    }
    let std_ycbcr = matches!(img, Image::YCbCr(_));
    let convert = converts_ycbcr(policy, m, std_ycbcr);
    if convert == std_ycbcr {
        return image_to_frame_bytes(img, m);
    }
    if let Image::YCbCr(ycc) = img {
        let (rows, columns) = (m.rows as usize, m.columns as usize);
        if ycc.width != columns || ycc.height != rows {
            return Err(size_mismatch(img, m));
        }
        let mut out = vec![0u8; rows.saturating_mul(columns).saturating_mul(3)];
        let at = |plane: &[u8], i: usize| plane.get(i).copied().unwrap_or_default();
        let line = columns.saturating_mul(3).max(1);
        for (y, row) in out.chunks_exact_mut(line).enumerate() {
            for (x, px) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let (yi, ci) = (ycc.y_offset(x, y), ycc.c_offset(x, y));
                *px = [at(&ycc.y, yi), at(&ycc.cb, ci), at(&ycc.cr, ci)];
            }
        }
        return Ok(out);
    }
    let mut out = image_to_frame_bytes(img, m)?;
    for px in out.as_chunks_mut::<3>().0 {
        let [y, cb, cr] = *px;
        let (r, g, b) = stdjpeg::image::ycbcr_to_rgb(y, cb, cr);
        *px = [r, g, b];
    }
    Ok(out)
}
fn ycbcr_to_rgb16(frame: &mut [u8], precision: usize) {
    let precision = precision.clamp(1, 16) as u32;
    let maximum = (1i64 << precision).wrapping_sub(1) as f64;
    let half = (1i64 << precision.wrapping_sub(1)) as f64;
    let clamp =
        |v: f64| -> u16 { crate::fmath::max(0.0, crate::fmath::min(maximum, v)).round() as u16 };
    for px in frame.as_chunks_mut::<6>().0 {
        let [y0, y1, b0, b1, r0, r1] = *px;
        let y = u16::from_le_bytes([y0, y1]) as f64;
        let cb = u16::from_le_bytes([b0, b1]) as f64 - half;
        let cr = u16::from_le_bytes([r0, r1]) as f64 - half;
        let [r0, r1] = clamp(y + 1.402 * cr).to_le_bytes();
        let [g0, g1] = clamp(y - 0.344136 * cb - 0.714136 * cr).to_le_bytes();
        let [b0, b1] = clamp(y + 1.772 * cb).to_le_bytes();
        *px = [r0, r1, g0, g1, b0, b1];
    }
}
fn limit_stored_bits8(frame: &mut [u8], m: &Metadata) {
    let bits = m.bits_stored as usize;
    if bits == 0 || bits >= 8 {
        return;
    }
    let mask = ((1u32 << bits).wrapping_sub(1)) as u8;
    for v in frame.iter_mut() {
        if m.pixel_representation == 1 {
            *v &= mask;
        } else if *v > mask {
            *v = mask;
        }
    }
}
fn limit_stored_bits16(frame: &mut [u8], m: &Metadata) {
    let bits = m.bits_stored as usize;
    if bits == 0 || bits >= EXTENDED_PRECISION {
        return;
    }
    let mask = ((1u32 << bits).wrapping_sub(1)) as u16;
    for px in frame.as_chunks_mut::<2>().0 {
        let mut v = u16::from_le_bytes([px[0], px[1]]);
        if m.pixel_representation == 1 {
            v &= mask;
        } else if v > mask {
            v = mask;
        }
        px.copy_from_slice(&v.to_le_bytes());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encode_image_matches_the_frame_encoder() {
        let gray: Vec<u8> = (0..12 * 9).map(|i| (i * 7) as u8).collect();
        let data = encode_image(ImagePixels::Gray(&gray), 12, 9, 80).unwrap();
        assert_eq!(&data[..2], &[0xFF, 0xD8]);
        assert_eq!(
            data,
            stdjpeg::encode(stdjpeg::Source::Gray(&gray), 12, 9, 80)
        );
        let rgb = vec![200u8; 5 * 3 * 3];
        assert!(encode_image(ImagePixels::Rgb(&rgb), 5, 3, 0).is_ok());
        assert_eq!(
            encode_image(ImagePixels::Rgb(&rgb), 5, 4, 75),
            Err(ImageEncodeError::BufferSize {
                expected: 60,
                got: 45
            })
        );
        assert_eq!(
            encode_image(ImagePixels::Gray(&[]), 1 << 16, 0, 75),
            Err(ImageEncodeError::TooLarge)
        );
        assert!(encode_image(ImagePixels::Gray(&[]), 0, 0, 75).is_ok());
    }
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
    fn frame(m: &Metadata) -> Vec<u8> {
        let n = m.rows as usize * m.columns as usize * m.samples_per_pixel as usize;
        let mask = (1u32 << m.bits_stored) - 1;
        let mut out = Vec::new();
        let samples = m.samples_per_pixel as u32;
        let columns = m.columns as u32;
        for i in 0..n as u32 {
            let (x, y, c) = (
                (i / samples) % columns,
                (i / samples) / columns,
                i % samples,
            );
            let v = ((x * 5 + y * 3 + c * 20) * (mask + 1) / 256) & mask;
            if m.bits_allocated == 8 {
                out.push(v as u8);
            } else {
                out.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
        out
    }
    #[test]
    fn baseline_round_trip_is_close() {
        for (samples, sub, optimize) in [
            (1, Subsampling::S420, false),
            (3, Subsampling::S420, false),
            (3, Subsampling::S444, true),
        ] {
            let m = md(13, 21, samples, 8, 8);
            let input = frame(&m);
            let encoder = Encoder::with_options(EncoderOptions {
                quality: 100,
                subsampling: sub,
                optimize_huffman: optimize,
            })
            .unwrap();
            let encoded = encoder.encode_frame(&input, &m).unwrap();
            let decoded = decode_frames(&[&encoded.data], &m, DecodeOptions::default()).unwrap();
            assert_eq!(decoded.data[0].len(), input.len());
            let worst = decoded.data[0]
                .iter()
                .zip(&input)
                .map(|(a, b)| (*a as i32 - *b as i32).abs())
                .max()
                .unwrap();
            assert!(worst < 40, "samples={samples} worst={worst}");
        }
    }
    #[test]
    fn extended_round_trip_is_close() {
        let m = md(9, 17, 1, 16, 12);
        let input = frame(&m);
        let encoder = ExtendedEncoder::new(100).unwrap();
        let encoded = encoder.encode_frame(&input, &m).unwrap();
        let options = DecodeOptions {
            syntax: Syntax::Extended,
            ..Default::default()
        };
        let decoded = decode_frames(&[&encoded.data], &m, options).unwrap();
        for (a, b) in decoded.data[0].chunks(2).zip(input.chunks(2)) {
            let a = u16::from_le_bytes([a[0], a[1]]) as i32;
            let b = u16::from_le_bytes([b[0], b[1]]) as i32;
            assert!((a - b).abs() < 8);
        }
    }
    #[test]
    fn errors_keep_their_text() {
        let m = md(1, 1, 1, 8, 8);
        let err = decode_frames(&[b"\x00\x00\x00\x00"], &m, DecodeOptions::default()).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidFragment);
        assert_eq!(
            err.to_string(),
            "dicom: invalid JPEG still-image fragment: frame 0: missing JPEG SOI"
        );
    }
}
