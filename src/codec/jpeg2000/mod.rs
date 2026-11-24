#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
pub mod encoder;
pub mod htj2k;
pub mod j2kcore;
pub mod j2kpart2;
pub use crate::codec::common::{DecodedFrames, Metadata};
use crate::pixel::encapsulated::{self, Format, Recovery};
use crate::textfmt;
use j2kcore::{Codestream, CoreError, DecodeOptions, Image, Limits, Rect};
use std::borrow::Cow;
use std::fmt;
#[doc(hidden)]
pub const UID_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
#[doc(hidden)]
pub const UID_LOSSY: &str = "1.2.840.10008.1.2.4.91";
#[doc(hidden)]
pub const UID_PART2_LOSSLESS: &str = "1.2.840.10008.1.2.4.92";
#[doc(hidden)]
pub const UID_PART2: &str = "1.2.840.10008.1.2.4.93";
#[doc(hidden)]
pub const UID_HTJ2K_LOSSLESS: &str = "1.2.840.10008.1.2.4.201";
#[doc(hidden)]
pub const UID_HTJ2K_LOSSLESS_RPCL: &str = "1.2.840.10008.1.2.4.202";
#[doc(hidden)]
pub const UID_HTJ2K: &str = "1.2.840.10008.1.2.4.203";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    UnsupportedMetadata,
    UnsupportedFragmentLayout,
    ImageSizeMismatch,
    MalformedCodestream,
    DecoderUnavailable,
    SignednessMismatch,
    UnsupportedCodestreamFeature,
    Jp2Wrapped,
    SampleOutOfRange,
    InvalidReduction,
    ResourceLimit,
    InvalidMetadata,
    MissingMetadata,
    CodecNotFound,
    UnsupportedPhotometricInterpretation,
    UnsupportedPlanarConfiguration,
    PixelDataSizeMismatch,
    InvalidRegion,
    InvalidQualityLayers,
    PartialDecodeUnsupported,
    UnsupportedEncoderMetadata,
    TranscodeLossyDisallowed,
    TranscodeResourceLimit,
    Frame,
    Other,
}
impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::UnsupportedMetadata => "jpeg2000adapter: unsupported JPEG 2000 metadata",
            ErrorKind::UnsupportedFragmentLayout => {
                "jpeg2000adapter: unsupported JPEG 2000 fragment layout"
            }
            ErrorKind::ImageSizeMismatch => {
                "jpeg2000adapter: decoded image size does not match metadata"
            }
            ErrorKind::MalformedCodestream => {
                "jpeg2000adapter: malformed JPEG 2000 / HTJ2K codestream"
            }
            ErrorKind::DecoderUnavailable => "jpeg2000adapter: decoder unavailable",
            ErrorKind::SignednessMismatch => {
                "jpeg2000adapter: codestream signedness contradicts PixelRepresentation"
            }
            ErrorKind::UnsupportedCodestreamFeature => {
                "jpeg2000adapter: unsupported JPEG 2000 codestream feature"
            }
            ErrorKind::Jp2Wrapped => {
                "jpeg2000adapter: JP2 container instead of a JPEG 2000 codestream"
            }
            ErrorKind::SampleOutOfRange => {
                "jpeg2000adapter: decoded sample outside the BitsStored range"
            }
            ErrorKind::InvalidReduction => {
                "jpeg2000adapter: reduction beyond the decomposition levels"
            }
            ErrorKind::ResourceLimit => "jpeg2000adapter: JPEG 2000 resource limit exceeded",
            ErrorKind::InvalidMetadata => "dicom: invalid pixel data metadata",
            ErrorKind::MissingMetadata => "dicom: missing required pixel data metadata",
            ErrorKind::CodecNotFound => "dicom: no pixel data codec registered for transfer syntax",
            ErrorKind::UnsupportedPhotometricInterpretation => {
                "dicom: unsupported PhotometricInterpretation"
            }
            ErrorKind::UnsupportedPlanarConfiguration => "dicom: unsupported PlanarConfiguration",
            ErrorKind::PixelDataSizeMismatch => "dicom: pixel data size does not match metadata",
            ErrorKind::InvalidRegion => "dicom: invalid frame region",
            ErrorKind::InvalidQualityLayers => "dicom: invalid quality layer count",
            ErrorKind::PartialDecodeUnsupported => "dicom: codec cannot serve this partial decode",
            ErrorKind::UnsupportedEncoderMetadata => {
                "dicom: pixel metadata is unsupported by encoder"
            }
            ErrorKind::TranscodeLossyDisallowed => {
                "dicom: lossy transcode requires explicit opt-in"
            }
            ErrorKind::TranscodeResourceLimit => "dicom: transcode resource limit exceeded",
            ErrorKind::Frame | ErrorKind::Other => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}
impl Error {
    fn new(kind: ErrorKind, message: String) -> Error {
        Error { kind, message }
    }
    fn bare(kind: ErrorKind) -> Error {
        Error::new(kind, kind.text().to_string())
    }
    fn wrap(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error::new(kind, format!("{}: {detail}", kind.text()))
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
pub enum Registration {
    #[default]
    Default,
    Core,
    CoreHtj2k,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignednessPolicy {
    #[default]
    Reject,
    ReinterpretUnsignedAsSigned,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoreLimits {
    pub max_input_bytes: i64,
    pub max_components: i64,
    pub max_tiles: i64,
    pub max_resolutions: i64,
    pub max_samples: i64,
    pub max_tile_work_bytes: i64,
    pub max_code_blocks_per_tile: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoreOptions {
    pub limits: CoreLimits,
    pub signedness_mismatch: SignednessPolicy,
    pub allow_jp2_wrapper: bool,
}
impl CoreLimits {
    fn core(&self) -> Limits {
        Limits {
            max_components: self.max_components,
            max_tiles: self.max_tiles,
            max_resolutions: self.max_resolutions,
            max_samples: self.max_samples,
            max_tile_work_bytes: self.max_tile_work_bytes,
            max_code_blocks_per_tile: self.max_code_blocks_per_tile,
        }
    }
}
pub fn transfer_syntax_uids() -> Vec<&'static str> {
    vec![
        UID_LOSSLESS,
        UID_LOSSY,
        UID_PART2_LOSSLESS,
        UID_PART2,
        UID_HTJ2K_LOSSLESS,
        UID_HTJ2K_LOSSLESS_RPCL,
        UID_HTJ2K,
    ]
}
enum Backend {
    Core {
        allow_palette: bool,
        options: CoreOptions,
    },
    Part2Adapter,
}
fn lookup(syntax: &str, registration: Registration, options: CoreOptions) -> Option<Backend> {
    let uid = syntax.trim_end_matches([' ', '\0']);
    match registration {
        Registration::Default => match uid {
            UID_LOSSLESS => Some(Backend::Core {
                allow_palette: true,
                options: CoreOptions::default(),
            }),
            UID_LOSSY | UID_HTJ2K_LOSSLESS | UID_HTJ2K_LOSSLESS_RPCL | UID_HTJ2K => {
                Some(Backend::Core {
                    allow_palette: false,
                    options: CoreOptions::default(),
                })
            }
            UID_PART2_LOSSLESS | UID_PART2 => Some(Backend::Part2Adapter),
            _ => None,
        },
        Registration::Core => match uid {
            UID_LOSSLESS => Some(Backend::Core {
                allow_palette: true,
                options,
            }),
            UID_LOSSY => Some(Backend::Core {
                allow_palette: false,
                options,
            }),
            _ => None,
        },
        Registration::CoreHtj2k => match uid {
            UID_HTJ2K_LOSSLESS | UID_HTJ2K_LOSSLESS_RPCL | UID_HTJ2K => Some(Backend::Core {
                allow_palette: false,
                options,
            }),
            _ => None,
        },
    }
}
fn validate_metadata(m: &Metadata, allow_palette: bool) -> Result<(), Error> {
    if m.rows == 0 || m.columns == 0 || m.number_of_frames <= 0 {
        return Err(Error::wrap(
            ErrorKind::InvalidMetadata,
            format_args!(
                "rows={} columns={} number_of_frames={}",
                m.rows, m.columns, m.number_of_frames
            ),
        ));
    }
    if m.bits_allocated != 8 && m.bits_allocated != 16 {
        return Err(Error::wrap(
            ErrorKind::UnsupportedMetadata,
            format_args!("BitsAllocated={}", m.bits_allocated),
        ));
    }
    if m.bits_stored == 0 || m.bits_stored > m.bits_allocated {
        return Err(Error::wrap(
            ErrorKind::InvalidMetadata,
            format_args!(
                "BitsStored={} BitsAllocated={}",
                m.bits_stored, m.bits_allocated
            ),
        ));
    }
    if m.high_bit != m.bits_stored.wrapping_sub(1) {
        return Err(Error::wrap(
            ErrorKind::UnsupportedMetadata,
            format_args!("HighBit={} BitsStored={}", m.high_bit, m.bits_stored),
        ));
    }
    if m.samples_per_pixel != 1 && m.samples_per_pixel != 3 {
        return Err(Error::wrap(
            ErrorKind::UnsupportedMetadata,
            format_args!("SamplesPerPixel={}", m.samples_per_pixel),
        ));
    }
    if m.pixel_representation != 0 && m.pixel_representation != 1 {
        return Err(Error::wrap(
            ErrorKind::UnsupportedMetadata,
            format_args!("PixelRepresentation={}", m.pixel_representation),
        ));
    }
    if m.planar_configuration_present && m.planar_configuration != 0 {
        return Err(Error::wrap(
            ErrorKind::UnsupportedPlanarConfiguration,
            format_args!("PlanarConfiguration={}", m.planar_configuration),
        ));
    }
    let trimmed = m.photometric_interpretation.trim();
    let photometric = textfmt::to_upper(trimmed);
    let palette = photometric == "PALETTE COLOR";
    let supported = match m.samples_per_pixel {
        1 => {
            photometric == "MONOCHROME1"
                || photometric == "MONOCHROME2"
                || (photometric == "PALETTE COLOR" && m.pixel_representation == 0)
        }
        3 => photometric == "RGB" || photometric == "YBR_RCT" || photometric == "YBR_ICT",
        _ => false,
    };
    if !supported || (palette && !allow_palette) {
        return Err(Error::wrap(
            ErrorKind::UnsupportedPhotometricInterpretation,
            format_args!(
                "PhotometricInterpretation={trimmed} SamplesPerPixel={}",
                m.samples_per_pixel
            ),
        ));
    }
    Ok(())
}
fn is_jp2(payload: &[u8]) -> bool {
    payload.len() >= 12 && payload.get(4..8) == Some(b"jP  ")
}
fn classify_core_error(err: CoreError) -> Error {
    match err {
        CoreError::Unsupported(feature) => {
            Error::wrap(ErrorKind::UnsupportedCodestreamFeature, feature)
        }
        e @ CoreError::Limit { .. } => Error::wrap(ErrorKind::ResourceLimit, e),
        e => Error::wrap(ErrorKind::MalformedCodestream, e),
    }
}
fn check_input(payload: &[u8], options: &CoreOptions) -> Result<(), Error> {
    let max_input = if options.limits.max_input_bytes <= 0 {
        1 << 30
    } else {
        options.limits.max_input_bytes
    };
    if payload.len() as i64 > max_input {
        return Err(Error::wrap(
            ErrorKind::ResourceLimit,
            format_args!("codestream of {} bytes, limit {max_input}", payload.len()),
        ));
    }
    if is_jp2(payload) && !options.allow_jp2_wrapper {
        return Err(Error::bare(ErrorKind::Jp2Wrapped));
    }
    Ok(())
}
fn check_core_geometry(stream: &Codestream, m: &Metadata) -> Result<(), Error> {
    let (width, height) = stream.output_size(0);
    if width != m.columns as i64 || height != m.rows as i64 {
        return Err(Error::wrap(
            ErrorKind::ImageSizeMismatch,
            format_args!(
                "JPEG 2000 size={width}x{height} Columns={} Rows={}",
                m.columns, m.rows
            ),
        ));
    }
    if stream.components.len() != m.samples_per_pixel as usize {
        return Err(Error::wrap(
            ErrorKind::ImageSizeMismatch,
            format_args!(
                "JPEG 2000 components={} SamplesPerPixel={}",
                stream.components.len(),
                m.samples_per_pixel
            ),
        ));
    }
    for (i, component) in stream.components.iter().enumerate() {
        if component.depth > m.bits_allocated as i64 {
            return Err(Error::wrap(
                ErrorKind::UnsupportedMetadata,
                format_args!(
                    "JPEG 2000 precision={} BitsAllocated={} component={i}",
                    component.depth, m.bits_allocated
                ),
            ));
        }
        if stream
            .components
            .first()
            .is_some_and(|f| component.signed != f.signed)
        {
            return Err(Error::wrap(
                ErrorKind::UnsupportedMetadata,
                "components differ in signedness",
            ));
        }
    }
    Ok(())
}
fn signedness(stream: &Codestream, m: &Metadata, options: &CoreOptions) -> Result<bool, Error> {
    let signed = stream.components.first().is_some_and(|c| c.signed);
    if signed && m.pixel_representation == 0 {
        return Err(Error::wrap(
            ErrorKind::SignednessMismatch,
            "signed codestream, PixelRepresentation=0",
        ));
    }
    if !signed && m.pixel_representation == 1 {
        if options.signedness_mismatch == SignednessPolicy::ReinterpretUnsignedAsSigned {
            return Ok(true);
        }
        return Err(Error::wrap(
            ErrorKind::SignednessMismatch,
            "unsigned codestream, PixelRepresentation=1",
        ));
    }
    Ok(false)
}
fn parse(payload: &[u8], m: &Metadata, options: &CoreOptions) -> Result<Codestream, Error> {
    check_input(payload, options)?;
    let stream = j2kcore::parse_codestream(payload).map_err(classify_core_error)?;
    check_core_geometry(&stream, m)?;
    Ok(stream)
}
fn pack_core_samples(image: &Image, m: &Metadata, reinterpret: bool) -> Result<Vec<u8>, Error> {
    let (rows, columns) = (m.rows as i64, m.columns as i64);
    let components = m.samples_per_pixel as i64;
    let bytes_per_sample = (m.bits_allocated / 8) as i64;
    if image.width != columns || image.height != rows || image.components.len() as i64 != components
    {
        return Err(Error::wrap(
            ErrorKind::ImageSizeMismatch,
            format_args!(
                "decoded {}x{}x{}",
                image.width,
                image.height,
                image.components.len()
            ),
        ));
    }
    let pixels = rows.saturating_mul(columns);
    let Some(size) = pixels
        .checked_mul(components)
        .and_then(|n| n.checked_mul(bytes_per_sample))
        .filter(|_| pixels > 0 && components > 0)
    else {
        return Err(Error::wrap(
            ErrorKind::ImageSizeMismatch,
            "output size overflows",
        ));
    };
    let mut out = vec![0u8; size as usize];
    let bits_stored = m.bits_stored as u32;
    let half = 1i32.wrapping_shl(bits_stored.wrapping_sub(1));
    let (mut low, mut high) = (0i32, (1i64 << bits_stored).wrapping_sub(1) as i32);
    if m.pixel_representation == 1 && !reinterpret {
        low = half.wrapping_neg();
        high = half.wrapping_sub(1);
    }
    let bps = bytes_per_sample as usize;
    let stride = (components as usize).saturating_mul(bps);
    for (c, plane) in image.components.iter().enumerate() {
        if plane.len() as i64 != pixels {
            return Err(Error::wrap(
                ErrorKind::ImageSizeMismatch,
                format_args!("component {c} has {} samples", plane.len()),
            ));
        }
        let at = c.saturating_mul(bps);
        for (pixel, &value) in out.chunks_exact_mut(stride).zip(plane) {
            let mut v = value;
            if v < low || v > high {
                return Err(Error::wrap(
                    ErrorKind::SampleOutOfRange,
                    format_args!("component {c}"),
                ));
            }
            if reinterpret && v >= half {
                v = v.wrapping_sub(1i32.wrapping_shl(bits_stored));
            }
            match pixel.get_mut(at..) {
                Some([b, ..]) if bps == 1 => *b = v as u8,
                Some([lo, hi, ..]) if bps == 2 => [*lo, *hi] = (v as u16).to_le_bytes(),
                _ => {}
            }
        }
    }
    Ok(out)
}
fn decode_core_frame(
    payload: &[u8],
    m: &Metadata,
    options: &CoreOptions,
) -> Result<Vec<u8>, Error> {
    let stream = parse(payload, m, options)?;
    let reinterpret = signedness(&stream, m, options)?;
    let image = j2kcore::decode(&stream, options.limits.core(), DecodeOptions::default())
        .map_err(classify_core_error)?;
    pack_core_samples(&image, m, reinterpret)
}
fn plan_frames<'a>(
    fragments: &'a [&'a [u8]],
    frames: i64,
    allow_jp2: bool,
) -> Result<Vec<Cow<'a, [u8]>>, Error> {
    if let Some(first) = fragments.first()
        && is_jp2(first)
    {
        if !allow_jp2 {
            return Err(Error::bare(ErrorKind::Jp2Wrapped));
        }
        if frames == 1 {
            return Ok(vec![Cow::Owned(fragments.concat())]);
        }
    }
    let plan = encapsulated::from_fragments(
        &[],
        fragments,
        crate::pixel::encapsulated::EotAttributes::default(),
        frames,
        Format::JPEG2000,
        encapsulated::Limits::default(),
        Recovery::default(),
    )
    .map_err(|e| Error::wrap(ErrorKind::UnsupportedFragmentLayout, e))?;
    let mut out = Vec::with_capacity(plan.len());
    for i in 0..plan.len() {
        let view = plan
            .frame(i)
            .map_err(|e| Error::wrap(ErrorKind::UnsupportedFragmentLayout, e))?;
        if let Err(e) = encapsulated::validate_frame(&view.data, Format::JPEG2000) {
            return Err(Error::wrap(
                ErrorKind::MalformedCodestream,
                format_args!("frame {i}: {e}"),
            ));
        }
        out.push(Cow::Owned(view.data.into_owned()));
    }
    Ok(out)
}
pub fn decode_frames(
    fragments: &[&[u8]],
    metadata: &Metadata,
    syntax: &str,
    registration: Registration,
    options: CoreOptions,
) -> Result<DecodedFrames, Error> {
    let backend =
        lookup(syntax, registration, options).ok_or(Error::bare(ErrorKind::CodecNotFound))?;
    let (allow_palette, options) = match backend {
        Backend::Core {
            allow_palette,
            options,
        } => (allow_palette, options),
        Backend::Part2Adapter => {
            validate_metadata(metadata, false)?;
            return Err(Error::wrap(
                ErrorKind::DecoderUnavailable,
                "the JPEG 2000 Part 2 adapter backend is not available",
            ));
        }
    };
    validate_metadata(metadata, allow_palette)?;
    let payloads = plan_frames(
        fragments,
        metadata.number_of_frames,
        options.allow_jp2_wrapper,
    )?;
    let mut data = Vec::with_capacity(payloads.len());
    for (i, payload) in payloads.iter().enumerate() {
        let frame = decode_core_frame(payload, metadata, &options).map_err(|e| {
            Error::new(ErrorKind::Frame, format!("decode JPEG 2000 frame {i}: {e}"))
        })?;
        data.push(frame);
    }
    Ok(DecodedFrames {
        rows: metadata.rows as usize,
        columns: metadata.columns as usize,
        data,
        ..DecodedFrames::default()
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartialRequest {
    pub region: Rect,
    pub reduction_level: i64,
    pub quality_layers: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PartialCapabilities {
    pub region: bool,
    pub reduction_levels: i64,
    pub quality_layers: i64,
}
pub const MAX_REDUCTION_LEVEL: i64 = 16;
fn codestream_scalability(stream: &Codestream) -> (i64, i64) {
    let mut levels = -1i64;
    let mut layers = 0i64;
    for tile in 0..stream.num_tiles() {
        for comp in 0..stream.components.len() as i64 {
            let style = stream.coding_for_tile(tile, comp);
            if levels < 0 || style.levels < levels {
                levels = style.levels;
            }
            layers = layers.max(style.layers);
        }
    }
    (levels.max(0), layers)
}
fn origin_on_reduced_grid(stream: &Codestream, reduce: i64) -> bool {
    let (ox, oy) = stream.output_origin(0);
    let s = 1i64 << reduce;
    ox.checked_rem(s) == Some(0) && oy.checked_rem(s) == Some(0)
}
fn floor_div(a: i64, b: i64) -> i64 {
    if a >= 0 {
        a.checked_div(b).unwrap_or(0)
    } else {
        a.saturating_neg()
            .saturating_add(b)
            .saturating_sub(1)
            .checked_div(b)
            .unwrap_or(0)
            .saturating_neg()
    }
}
pub fn reduced_region(region: Rect, level: i64) -> Rect {
    if level <= 0 {
        return region;
    }
    let s = 1i64 << level;
    Rect::new(
        floor_div(region.min_x, s),
        floor_div(region.min_y, s),
        floor_div(region.max_x.saturating_neg(), s).saturating_neg(),
        floor_div(region.max_y.saturating_neg(), s).saturating_neg(),
    )
}
#[doc(hidden)]
pub fn partial_capabilities(
    codestream: &[u8],
    metadata: &Metadata,
) -> Result<PartialCapabilities, Error> {
    let stream = parse(codestream, metadata, &CoreOptions::default())?;
    let (levels, layers) = codestream_scalability(&stream);
    let mut levels = levels.min(MAX_REDUCTION_LEVEL);
    while levels > 0 && !origin_on_reduced_grid(&stream, levels) {
        levels = levels.saturating_sub(1);
    }
    Ok(PartialCapabilities {
        region: true,
        reduction_levels: levels,
        quality_layers: layers,
    })
}
pub fn decode_partial(
    codestream: &[u8],
    metadata: &Metadata,
    request: PartialRequest,
    options: CoreOptions,
) -> Result<DecodedFrames, Error> {
    validate_metadata(metadata, false)?;
    let stream = parse(codestream, metadata, &options)?;
    let bounds = Rect::new(0, 0, metadata.columns as i64, metadata.rows as i64);
    let mut region = request.region;
    if region == Rect::default() {
        region = bounds;
    }
    if region.is_empty() || !region.is_in(&bounds) {
        return Err(Error::wrap(ErrorKind::InvalidRegion, region));
    }
    let (reduce, layers) = (request.reduction_level, request.quality_layers);
    let (levels, available) = codestream_scalability(&stream);
    if reduce < 0 || reduce > levels {
        return Err(Error::new(
            ErrorKind::InvalidReduction,
            format!(
                "dicom: invalid resolution reduction: {}: {reduce} levels asked, the codestream has {levels}",
                ErrorKind::InvalidReduction.text()
            ),
        ));
    }
    if layers < 0 || layers > available {
        return Err(Error::wrap(
            ErrorKind::InvalidQualityLayers,
            format_args!("{layers} asked, the codestream has {available}"),
        ));
    }
    if !origin_on_reduced_grid(&stream, reduce) {
        return Err(Error::wrap(
            ErrorKind::PartialDecodeUnsupported,
            format_args!("image origin off the grid reduced by 2^{reduce}"),
        ));
    }
    let reinterpret = signedness(&stream, metadata, &options)?;
    let out = reduced_region(region, reduce);
    let decoded = j2kcore::decode(
        &stream,
        options.limits.core(),
        DecodeOptions {
            region: out,
            reduce,
            layers,
            annex_j: None,
        },
    )
    .map_err(classify_core_error)?;
    let mut part = metadata.clone();
    part.rows = out.dy() as u16;
    part.columns = out.dx() as u16;
    let data = pack_core_samples(&decoded, &part, reinterpret)?;
    Ok(DecodedFrames {
        rows: out.dy() as usize,
        columns: out.dx() as usize,
        data: vec![data],
        ..DecodedFrames::default()
    })
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreImage {
    pub columns: i64,
    pub rows: i64,
    pub components: Vec<Vec<i32>>,
    pub depths: Vec<i64>,
    pub signed: Vec<bool>,
}
#[doc(hidden)]
pub fn decode_core_reduced(
    codestream: &[u8],
    reduce: i64,
    options: CoreOptions,
) -> Result<CoreImage, Error> {
    check_input(codestream, &options)?;
    let stream = j2kcore::parse_codestream(codestream).map_err(classify_core_error)?;
    if reduce < 0 {
        return Err(Error::wrap(ErrorKind::InvalidReduction, reduce));
    }
    for tile in 0..stream.num_tiles() {
        for comp in 0..stream.components.len() as i64 {
            let levels = stream.coding_for_tile(tile, comp).levels;
            if reduce > levels {
                return Err(Error::wrap(
                    ErrorKind::InvalidReduction,
                    format_args!(
                        "{reduce} levels asked, tile {tile} component {comp} has {levels}"
                    ),
                ));
            }
        }
    }
    let image = j2kcore::decode(
        &stream,
        options.limits.core(),
        DecodeOptions {
            reduce,
            ..DecodeOptions::default()
        },
    )
    .map_err(classify_core_error)?;
    Ok(CoreImage {
        columns: image.width,
        rows: image.height,
        components: image.components,
        depths: image.depths,
        signed: image.signed,
    })
}
#[doc(hidden)]
pub fn decode_part2(
    fragments: &[&[u8]],
    basic_offset_table: &[u8],
    extended_tables: bool,
    metadata: &Metadata,
    syntax: &str,
) -> Result<DecodedFrames, Error> {
    let uid = syntax.trim_end_matches([' ', '\0']);
    let lossless = match uid {
        UID_PART2_LOSSLESS => true,
        UID_PART2 => false,
        _ => return Err(Error::bare(ErrorKind::CodecNotFound)),
    };
    let m = metadata;
    if m.number_of_frames < 2
        || m.number_of_frames > j2kpart2::MAX_FRAMES
        || (m.rows as i64)
            .saturating_mul(m.columns as i64)
            .saturating_mul(m.number_of_frames)
            > j2kpart2::MAX_SAMPLES
    {
        return Err(Error::bare(ErrorKind::ResourceLimit));
    }
    if m.samples_per_pixel != 1
        || m.planar_configuration_present
        || (m.bits_allocated != 8 && m.bits_allocated != 16)
        || m.high_bit != m.bits_stored.wrapping_sub(1)
        || m.pixel_representation > 1
        || (m.photometric_interpretation != "MONOCHROME1"
            && m.photometric_interpretation != "MONOCHROME2")
        || fragments.len() != 1
        || !basic_offset_table.is_empty()
        || extended_tables
    {
        return Err(Error::bare(ErrorKind::UnsupportedMetadata));
    }
    let &[encoded] = fragments else {
        return Err(Error::bare(ErrorKind::UnsupportedMetadata));
    };
    if encoded.len() > 32 << 20 {
        return Err(Error::bare(ErrorKind::ResourceLimit));
    }
    let collection = j2kpart2::inspect(encoded)
        .map_err(|e| Error::wrap(ErrorKind::UnsupportedCodestreamFeature, e))?;
    let p = collection.profile;
    if p.frames != m.number_of_frames
        || collection.rows != m.rows as i64
        || collection.columns != m.columns as i64
        || p.bits != m.bits_stored as i64
        || p.signed != (m.pixel_representation == 1)
        || (lossless && !p.reversible)
        || (!p.reversible && m.bits_allocated != 8)
    {
        return Err(Error::bare(ErrorKind::UnsupportedMetadata));
    }
    let stream = j2kcore::parse_codestream(&collection.coded_components(encoded))
        .map_err(classify_core_error)?;
    let decoded = j2kcore::decode(
        &stream,
        Limits {
            max_components: j2kpart2::MAX_FRAMES,
            max_samples: j2kpart2::MAX_SAMPLES,
            max_tile_work_bytes: 128 << 20,
            ..Limits::default()
        },
        DecodeOptions {
            annex_j: Some(collection.profile),
            ..DecodeOptions::default()
        },
    )
    .map_err(classify_core_error)?;
    let frame_size = encoder::frame_size(m) as usize;
    let mut frames = Vec::with_capacity(decoded.components.len());
    for component in &decoded.components {
        let mut frame = vec![0u8; frame_size];
        if m.bits_allocated == 8 {
            for (d, &sample) in frame.iter_mut().zip(component) {
                *d = sample as u8;
            }
        } else {
            for (d, &sample) in frame.as_chunks_mut::<2>().0.iter_mut().zip(component) {
                *d = (sample as u16).to_le_bytes();
            }
        }
        frames.push(frame);
    }
    Ok(DecodedFrames {
        rows: m.rows as usize,
        columns: m.columns as usize,
        data: frames,
        ..DecodedFrames::default()
    })
}
