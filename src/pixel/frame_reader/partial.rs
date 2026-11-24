use super::region::{FrameRegion, crop_frame, is_subsampled_422, subsampled_error};
use super::{FrameReader, Stored};
#[cfg(feature = "jpeg2000")]
use crate::codec::jpeg2000;
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::codec::rle;
use crate::pixel::Metadata;
use crate::pixel::codec_router::{Codec, DecodeContext};
#[cfg(any(feature = "jpeg-ls", feature = "jpeg2000"))]
use crate::pixel::pixeldata::to_codec_metadata;
use crate::pixel::pixeldata::{Frames, PixelErrorKind, frame_size};
use crate::pixel::transcode::decompress::{limit_error, output_metadata};
use crate::pixel::transcode::{DecoderRegistry, Error, ErrorKind};
use crate::transfer;
use std::sync::Arc;
pub const MAX_REDUCTION_LEVEL: i64 = 16;
pub const ERR_PARTIAL_DECODE_UNSUPPORTED: &str = "dicom: codec cannot serve this partial decode";
pub const ERR_INVALID_REDUCTION: &str = "dicom: invalid resolution reduction";
pub const ERR_INVALID_QUALITY_LAYERS: &str = "dicom: invalid quality layer count";
const ERR_INVALID_SAMPLE_FORMAT: &str = "dicom: invalid stored sample format";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartialDecodeRequest {
    pub frame: i64,
    pub region: FrameRegion,
    pub reduction_level: i64,
    pub quality_layers: i64,
    pub no_fallback: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartialDecodeCapabilities {
    pub region: bool,
    pub reduction_levels: i64,
    pub quality_layers: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialDecodePath {
    Codec,
    NativeRows,
    CodecRegion,
    FullFrame,
}
impl PartialDecodePath {
    pub fn as_str(self) -> &'static str {
        match self {
            PartialDecodePath::Codec => "codec",
            PartialDecodePath::NativeRows => "native-rows",
            PartialDecodePath::CodecRegion => "codec-region",
            PartialDecodePath::FullFrame => "full-frame",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialDecodeResult {
    pub data: Vec<u8>,
    pub metadata: Metadata,
    pub region: FrameRegion,
    pub request: PartialDecodeRequest,
    pub path: PartialDecodePath,
    pub fallback: bool,
    pub averaged_reduction: bool,
    pub quality_layers: i64,
}
fn floor_div(a: i64, b: i64) -> i64 {
    a.checked_div_euclid(b).unwrap_or(0)
}
fn ceil_div(a: i64, b: i64) -> i64 {
    floor_div(a.saturating_neg(), b).saturating_neg()
}
pub fn reduced_region(region: FrameRegion, level: i64) -> FrameRegion {
    if level <= 0 {
        return region;
    }
    let s = 1i64 << level.min(62);
    FrameRegion::new(
        floor_div(region.min_x, s),
        floor_div(region.min_y, s),
        ceil_div(region.max_x, s),
        ceil_div(region.max_y, s),
    )
}
fn intersect(a: FrameRegion, b: FrameRegion) -> FrameRegion {
    let r = FrameRegion {
        min_x: a.min_x.max(b.min_x),
        min_y: a.min_y.max(b.min_y),
        max_x: a.max_x.min(b.max_x),
        max_y: a.max_y.min(b.max_y),
    };
    if r.is_empty() {
        FrameRegion::default()
    } else {
        r
    }
}
fn within(r: &FrameRegion, bounds: &FrameRegion) -> bool {
    r.min_x >= bounds.min_x
        && r.max_x <= bounds.max_x
        && r.min_y >= bounds.min_y
        && r.max_y <= bounds.max_y
}
#[cfg_attr(not(all(feature = "jpeg-ls", feature = "jpeg2000")), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartCodec {
    Jpeg2000,
    Jpeg2000Part2,
    Rle,
    JpegLs,
    None,
}
pub(crate) fn part_codec(registry: DecoderRegistry, uid: &str) -> PartCodec {
    if let DecoderRegistry::Custom(r) = registry {
        if r.overrides(uid) {
            return PartCodec::None;
        }
        return part_codec(r.base(), uid);
    }
    let uid = transfer::normalize_uid(uid);
    if uid == rle::UID {
        return PartCodec::Rle;
    }
    #[cfg(feature = "jpeg-ls")]
    if uid == jpegls::UID || uid == jpegls::UID_NEAR_LOSSLESS {
        return PartCodec::JpegLs;
    }
    #[cfg(feature = "jpeg2000")]
    if registry != DecoderRegistry::BuiltinWithoutJpeg2000
        && jpeg2000::transfer_syntax_uids().contains(&uid)
    {
        if matches!(uid, "1.2.840.10008.1.2.4.92" | "1.2.840.10008.1.2.4.93") {
            return PartCodec::Jpeg2000Part2;
        }
        return PartCodec::Jpeg2000;
    }
    PartCodec::None
}
#[cfg(feature = "jpeg2000")]
fn j2k_rect(r: FrameRegion) -> jpeg2000::j2kcore::Rect {
    jpeg2000::j2kcore::Rect::new(r.min_x, r.min_y, r.max_x, r.max_y)
}
#[cfg(feature = "jpeg2000")]
fn j2k_error(e: jpeg2000::Error) -> Error {
    let kind = match e.kind() {
        jpeg2000::ErrorKind::InvalidReduction => ErrorKind::InvalidReduction,
        jpeg2000::ErrorKind::InvalidQualityLayers => ErrorKind::InvalidQualityLayers,
        jpeg2000::ErrorKind::InvalidRegion => ErrorKind::InvalidRegion,
        jpeg2000::ErrorKind::PartialDecodeUnsupported => ErrorKind::PartialDecodeUnsupported,
        jpeg2000::ErrorKind::InvalidMetadata => ErrorKind::Pixel(PixelErrorKind::InvalidMetadata),
        jpeg2000::ErrorKind::MissingMetadata => ErrorKind::Pixel(PixelErrorKind::MissingMetadata),
        jpeg2000::ErrorKind::PixelDataSizeMismatch => {
            ErrorKind::Pixel(PixelErrorKind::PixelDataSizeMismatch)
        }
        _ => ErrorKind::Other,
    };
    Error::new(kind, e.to_string())
}
#[cfg(feature = "jpeg-ls")]
fn jpegls_error(e: jpegls::Error) -> Error {
    let kind = match e.kind() {
        jpegls::ErrorKind::PixelDataSizeMismatch => {
            ErrorKind::Pixel(PixelErrorKind::PixelDataSizeMismatch)
        }
        jpegls::ErrorKind::IncompatiblePixelData => {
            ErrorKind::Pixel(PixelErrorKind::IncompatiblePixelData)
        }
        jpegls::ErrorKind::InvalidRegion => ErrorKind::InvalidRegion,
        _ => ErrorKind::Other,
    };
    Error::new(kind, e.to_string())
}
pub(crate) enum Part {
    Frames(Frames),
    Unsupported,
}
#[cfg(feature = "jpeg2000")]
fn frames_of(d: crate::codec::common::DecodedFrames) -> Frames {
    crate::pixel::pixeldata::from_decoded(d)
}
#[cfg(feature = "jpeg-ls")]
fn jpegls_codec(uid: &str) -> jpegls::Codec {
    if transfer::normalize_uid(uid) == jpegls::UID {
        jpegls::Codec::lossless()
    } else {
        jpegls::Codec::near_lossless()
    }
}
#[cfg(feature = "jpeg2000")]
pub(crate) fn builtin_j2k_part(
    codestream: &[u8],
    metadata: &Metadata,
    region: FrameRegion,
    level: i64,
    layers: i64,
) -> Result<Part, Error> {
    let cm = to_codec_metadata(metadata);
    let request = jpeg2000::PartialRequest {
        region: j2k_rect(region),
        reduction_level: level,
        quality_layers: layers,
    };
    match jpeg2000::decode_partial(codestream, &cm, request, jpeg2000::CoreOptions::default()) {
        Ok(d) => Ok(Part::Frames(frames_of(d))),
        Err(e) if e.kind() == jpeg2000::ErrorKind::PartialDecodeUnsupported => {
            Ok(Part::Unsupported)
        }
        Err(e) => Err(j2k_error(e)),
    }
}
#[cfg(not(feature = "jpeg2000"))]
pub(crate) fn builtin_j2k_part(
    _codestream: &[u8],
    _metadata: &Metadata,
    _region: FrameRegion,
    _level: i64,
    _layers: i64,
) -> Result<Part, Error> {
    Ok(Part::Unsupported)
}
#[cfg_attr(not(feature = "jpeg-ls"), allow(unused_variables))]
pub(crate) fn builtin_region(
    codec: PartCodec,
    uid: &str,
    codestream: &[u8],
    m: &Metadata,
    region: FrameRegion,
) -> Result<Part, Error> {
    match codec {
        PartCodec::Rle => {
            let r = rle::Rect::new(region.min_x, region.min_y, region.max_x, region.max_y);
            rle::decode_region(codestream, m, r, rle::Recovery::default())
                .map(|f| {
                    Part::Frames(Frames {
                        rows: f.rows,
                        columns: f.columns,
                        data: f.data,
                        photometric_interpretation: String::new(),
                        diagnostics: f.diagnostics,
                    })
                })
                .map_err(|e| {
                    let kind = match e.kind() {
                        rle::ErrorKind::PixelDataSizeMismatch => {
                            ErrorKind::Pixel(PixelErrorKind::PixelDataSizeMismatch)
                        }
                        rle::ErrorKind::IncompatiblePixelData => {
                            ErrorKind::Pixel(PixelErrorKind::IncompatiblePixelData)
                        }
                        rle::ErrorKind::InvalidRegion => ErrorKind::InvalidRegion,
                        _ => ErrorKind::Other,
                    };
                    Error::new(kind, e.to_string())
                })
        }
        #[cfg(feature = "jpeg-ls")]
        PartCodec::JpegLs => {
            let r = jpegls::Region {
                x0: region.min_x,
                y0: region.min_y,
                x1: region.max_x,
                y1: region.max_y,
            };
            match jpegls_codec(uid).decode_frame_region(
                codestream,
                &to_codec_metadata(m),
                r,
                jpegls::DecodeOptions::default(),
            ) {
                Ok(d) => Ok(Part::Frames(crate::pixel::pixeldata::from_decoded(d))),
                Err(e) if e.kind() == jpegls::ErrorKind::RegionUnsupported => Ok(Part::Unsupported),
                Err(e) => Err(jpegls_error(e)),
            }
        }
        PartCodec::Jpeg2000 => builtin_j2k_part(codestream, m, region, 0, 0),
        PartCodec::Jpeg2000Part2 | PartCodec::None => Ok(Part::Unsupported),
        #[cfg(not(feature = "jpeg-ls"))]
        PartCodec::JpegLs => Ok(Part::Unsupported),
    }
}
#[cfg_attr(
    not(all(feature = "jpeg-ls", feature = "jpeg2000")),
    allow(unused_variables)
)]
pub(crate) fn builtin_caps(
    codec: PartCodec,
    uid: &str,
    codestream: &[u8],
    m: &Metadata,
) -> Result<PartialDecodeCapabilities, Error> {
    match codec {
        #[cfg(feature = "jpeg2000")]
        PartCodec::Jpeg2000 => {
            let c = jpeg2000::partial_capabilities(codestream, &to_codec_metadata(m))
                .map_err(j2k_error)?;
            Ok(PartialDecodeCapabilities {
                region: c.region,
                reduction_levels: c.reduction_levels,
                quality_layers: c.quality_layers,
            })
        }
        #[cfg(feature = "jpeg-ls")]
        PartCodec::JpegLs => {
            match jpegls_codec(uid).check_region(codestream, &to_codec_metadata(m)) {
                Ok(()) => Ok(PartialDecodeCapabilities {
                    region: true,
                    ..PartialDecodeCapabilities::default()
                }),
                Err(e) if e.kind() == jpegls::ErrorKind::RegionUnsupported => {
                    Ok(PartialDecodeCapabilities::default())
                }
                Err(e) => Err(jpegls_error(e)),
            }
        }
        PartCodec::Rle => Ok(PartialDecodeCapabilities {
            region: true,
            ..PartialDecodeCapabilities::default()
        }),
        PartCodec::Jpeg2000Part2 | PartCodec::None => Ok(PartialDecodeCapabilities::default()),
        #[cfg(not(feature = "jpeg2000"))]
        PartCodec::Jpeg2000 => Ok(PartialDecodeCapabilities::default()),
        #[cfg(not(feature = "jpeg-ls"))]
        PartCodec::JpegLs => Ok(PartialDecodeCapabilities::default()),
    }
}
impl FrameReader<'_> {
    fn part_codec(&self) -> PartCodec {
        part_codec(self.registry(), &self.syntax.uid)
    }
    pub(super) fn custom_codec(&self) -> Option<Arc<dyn Codec>> {
        match self.registry() {
            DecoderRegistry::Custom(r) => r.get_codec(&self.syntax.uid),
            _ => None,
        }
    }
    pub(super) fn read_admitted_frame(&self, i: i64) -> Result<Vec<u8>, Error> {
        self.admitted_frame_input_bytes(i)?;
        self.raw_frame(i)
    }
    pub fn partial_decode_capabilities(&self, i: i64) -> Result<PartialDecodeCapabilities, Error> {
        self.check_index(i)?;
        if !self.syntax.encapsulated {
            return Ok(PartialDecodeCapabilities {
                region: !is_subsampled_422(&self.metadata.photometric_interpretation),
                ..PartialDecodeCapabilities::default()
            });
        }
        if !self.registry().decodes(&self.syntax.uid) {
            return Ok(PartialDecodeCapabilities::default());
        }
        if let Some(codec) = self.custom_codec() {
            if let Some(decoder) = codec.partial_decoder() {
                let codestream = self.read_admitted_frame(i)?;
                return decoder.partial_decode_capabilities(&codestream, &self.frame_metadata());
            }
            return Ok(PartialDecodeCapabilities {
                region: codec.region_decoder().is_some(),
                ..PartialDecodeCapabilities::default()
            });
        }
        match self.part_codec() {
            codec @ (PartCodec::Jpeg2000 | PartCodec::Jpeg2000Part2 | PartCodec::JpegLs) => {
                let codestream = self.read_admitted_frame(i)?;
                builtin_caps(codec, &self.syntax.uid, &codestream, &self.frame_metadata())
            }
            codec => builtin_caps(codec, &self.syntax.uid, &[], &self.frame_metadata()),
        }
    }
    pub fn decode_partial(
        &self,
        request: PartialDecodeRequest,
    ) -> Result<PartialDecodeResult, Error> {
        self.check_index(request.frame)?;
        let bounds = FrameRegion::new(
            0,
            0,
            i64::from(self.metadata.columns),
            i64::from(self.metadata.rows),
        );
        let request = validate_request(request, bounds)?;
        let level = request.reduction_level;
        let out = reduced_region(request.region, level);
        let source = intersect(
            FrameRegion {
                min_x: out.min_x << level,
                min_y: out.min_y << level,
                max_x: out.max_x << level,
                max_y: out.max_y << level,
            },
            bounds,
        );
        let beyond_region = level > 0 || request.quality_layers > 0;
        let whole = source == bounds && !beyond_region;
        let refuse = |why: &str| {
            Error::new(
                ErrorKind::PartialDecodeUnsupported,
                format!(
                    "{ERR_PARTIAL_DECODE_UNSUPPORTED}: frame {}, {} at level {level}, {} layers: {why}",
                    request.frame,
                    request.region.text(),
                    request.quality_layers
                ),
            )
        };
        let mut result = PartialDecodeResult {
            data: Vec::new(),
            metadata: Metadata::default(),
            region: out,
            request,
            path: PartialDecodePath::FullFrame,
            fallback: beyond_region,
            averaged_reduction: level > 0,
            quality_layers: 0,
        };
        let (data, metadata) = match &self.stored {
            Stored::Native(value) => {
                if beyond_region && request.no_fallback {
                    return Err(refuse(
                        "native Pixel Data has no lower resolutions or layers",
                    ));
                }
                if is_subsampled_422(&self.metadata.photometric_interpretation) {
                    return Err(subsampled_error());
                }
                result.path = PartialDecodePath::NativeRows;
                self.native_region(value, request.frame, source)?
            }
            Stored::Encapsulated(_) if self.custom_codec().is_some() => {
                let Some(codec) = self.custom_codec() else {
                    return Err(refuse("no codec"));
                };
                let cx = DecodeContext::default();
                let frame_metadata = self.frame_metadata();
                let mut codestream: Option<Vec<u8>> = None;
                if let Some(decoder) = codec.partial_decoder() {
                    let stream = self.read_admitted_frame(request.frame)?;
                    match decoder.decode_frame_partial(&cx, &stream, &frame_metadata, &request) {
                        Ok(frames) => {
                            let (data, metadata) = self.check_decoded_part(frames, out)?;
                            result.data = data;
                            result.metadata = metadata;
                            result.path = PartialDecodePath::Codec;
                            result.averaged_reduction = false;
                            result.quality_layers = request.quality_layers;
                            result.fallback = false;
                            return Ok(result);
                        }
                        Err(e)
                            if !e.is(ErrorKind::PartialDecodeUnsupported)
                                && !e.is(ErrorKind::RegionUnsupported) =>
                        {
                            return Err(e);
                        }
                        Err(_) => {}
                    }
                    codestream = Some(stream);
                }
                if beyond_region && request.no_fallback {
                    return Err(refuse("the codec cannot reduce or truncate layers itself"));
                }
                let mut decoded = None;
                if let Some(decoder) = codec.region_decoder()
                    && !whole
                {
                    let stream = match codestream {
                        Some(stream) => stream,
                        None => self.read_admitted_frame(request.frame)?,
                    };
                    match decoder.decode_frame_region(&cx, &stream, &frame_metadata, source) {
                        Ok(frames) => {
                            decoded = Some(self.check_decoded_part(frames, source)?);
                            result.path = PartialDecodePath::CodecRegion;
                        }
                        Err(e) if !e.is(ErrorKind::RegionUnsupported) => return Err(e),
                        Err(_) => {}
                    }
                }
                match decoded {
                    Some(part) => part,
                    None => {
                        if !whole && request.no_fallback {
                            return Err(refuse("the codec decodes whole frames only"));
                        }
                        let (frame, frame_metadata) = self.frame(request.frame)?;
                        result.path = PartialDecodePath::FullFrame;
                        result.fallback = !whole || beyond_region;
                        if whole {
                            (frame, frame_metadata)
                        } else {
                            crop_frame(&frame, frame_metadata, source)?
                        }
                    }
                }
            }
            Stored::Encapsulated(_) => {
                let codec = if self.registry().decodes(&self.syntax.uid) {
                    self.part_codec()
                } else {
                    PartCodec::None
                };
                let mut codestream: Option<Vec<u8>> = None;
                if matches!(
                    codec,
                    PartCodec::Jpeg2000 | PartCodec::Jpeg2000Part2 | PartCodec::JpegLs
                ) {
                    let stream = self.read_admitted_frame(request.frame)?;
                    let part = match codec {
                        PartCodec::Jpeg2000 => {
                            self.j2k_part(&stream, request.region, level, request.quality_layers)
                        }
                        PartCodec::JpegLs if level == 0 && request.quality_layers == 0 => {
                            self.region_part(codec, &stream, request.region)
                        }
                        _ => Ok(Part::Unsupported),
                    };
                    codestream = Some(stream);
                    if let Part::Frames(frames) = part? {
                        let (data, metadata) = self.check_decoded_part(frames, out)?;
                        result.data = data;
                        result.metadata = metadata;
                        result.path = PartialDecodePath::Codec;
                        result.averaged_reduction = false;
                        result.quality_layers = request.quality_layers;
                        result.fallback = false;
                        return Ok(result);
                    }
                }
                if beyond_region && request.no_fallback {
                    return Err(refuse("the codec cannot reduce or truncate layers itself"));
                }
                let mut decoded = None;
                if codec != PartCodec::None && !whole {
                    let stream = match codestream {
                        Some(stream) => stream,
                        None => self.read_admitted_frame(request.frame)?,
                    };
                    if let Part::Frames(frames) = self.region_part(codec, &stream, source)? {
                        decoded = Some(self.check_decoded_part(frames, source)?);
                        result.path = PartialDecodePath::CodecRegion;
                    }
                }
                match decoded {
                    Some(part) => part,
                    None => {
                        if !whole && request.no_fallback {
                            return Err(refuse("the codec decodes whole frames only"));
                        }
                        let (frame, frame_metadata) = self.frame(request.frame)?;
                        result.path = PartialDecodePath::FullFrame;
                        result.fallback = !whole || beyond_region;
                        if whole {
                            (frame, frame_metadata)
                        } else {
                            crop_frame(&frame, frame_metadata, source)?
                        }
                    }
                }
            }
        };
        let (data, metadata) = if level > 0 {
            average_cells(&data, metadata, level)?
        } else {
            (data, metadata)
        };
        if i64::from(metadata.columns) != out.dx() || i64::from(metadata.rows) != out.dy() {
            return Err(Error::pixel(
                PixelErrorKind::PixelDataSizeMismatch,
                format_args!(
                    "{}x{} output for {}",
                    metadata.columns,
                    metadata.rows,
                    out.text()
                ),
            ));
        }
        result.data = data;
        result.metadata = metadata;
        Ok(result)
    }
    fn j2k_part(
        &self,
        codestream: &[u8],
        region: FrameRegion,
        level: i64,
        layers: i64,
    ) -> Result<Part, Error> {
        builtin_j2k_part(codestream, &self.frame_metadata(), region, level, layers)
    }
    fn region_part(
        &self,
        codec: PartCodec,
        codestream: &[u8],
        region: FrameRegion,
    ) -> Result<Part, Error> {
        builtin_region(
            codec,
            &self.syntax.uid,
            codestream,
            &self.frame_metadata(),
            region,
        )
    }
    pub(super) fn check_decoded_part(
        &self,
        frames: Frames,
        rect: FrameRegion,
    ) -> Result<(Vec<u8>, Metadata), Error> {
        if frames.data.len() != 1
            || frames.rows as i64 != rect.dy()
            || frames.columns as i64 != rect.dx()
        {
            return Err(Error::pixel(
                PixelErrorKind::PixelDataSizeMismatch,
                format_args!(
                    "partial decoder returned {} frames of {}x{} for {}x{}",
                    frames.data.len(),
                    frames.columns,
                    frames.rows,
                    rect.dx(),
                    rect.dy()
                ),
            ));
        }
        let mut metadata = output_metadata(&self.frame_metadata(), &self.syntax, &frames);
        metadata.rows = rect.dy() as u16;
        metadata.columns = rect.dx() as u16;
        let bytes = frame_size(&metadata);
        let mut frames = frames;
        let data = frames.data.swap_remove(0);
        if data.len() as i64 != bytes {
            return Err(Error::pixel(
                PixelErrorKind::PixelDataSizeMismatch,
                format_args!("part of {} bytes, want {bytes}", data.len()),
            ));
        }
        let (limits, _) = self.prepare_decode()?;
        if bytes > limits.max_native_bytes {
            return Err(limit_error(
                "MaxNativeBytes",
                bytes,
                limits.max_native_bytes,
            ));
        }
        Ok((data, metadata))
    }
}
fn validate_request(
    mut request: PartialDecodeRequest,
    bounds: FrameRegion,
) -> Result<PartialDecodeRequest, Error> {
    if request.reduction_level < 0 || request.reduction_level > MAX_REDUCTION_LEVEL {
        return Err(Error::new(
            ErrorKind::InvalidReduction,
            format!(
                "{ERR_INVALID_REDUCTION}: level {}, want 0 to {MAX_REDUCTION_LEVEL}",
                request.reduction_level
            ),
        ));
    }
    if request.quality_layers < 0 {
        return Err(Error::new(
            ErrorKind::InvalidQualityLayers,
            format!("{ERR_INVALID_QUALITY_LAYERS}: {}", request.quality_layers),
        ));
    }
    if request.region == FrameRegion::default() {
        request.region = bounds;
    }
    if request.region.is_empty() || !within(&request.region, &bounds) {
        return Err(Error::new(
            ErrorKind::InvalidRegion,
            format!(
                "{}: {} in {}",
                super::ERR_INVALID_REGION,
                request.region.text(),
                bounds.text()
            ),
        ));
    }
    Ok(request)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn average_cells(
    data: &[u8],
    mut metadata: Metadata,
    level: i64,
) -> Result<(Vec<u8>, Metadata), Error> {
    if is_subsampled_422(&metadata.photometric_interpretation) {
        return Err(super::layout(
            "reducing subsampled 4:2:2 Pixel Data is unsupported",
        ));
    }
    if !(1..=MAX_REDUCTION_LEVEL).contains(&level) {
        return Err(super::layout(format_args!(
            "{ERR_INVALID_REDUCTION}: level {level}, want 1 to {MAX_REDUCTION_LEVEL}"
        )));
    }
    let (ba, bs, hb) = (
        metadata.bits_allocated,
        metadata.bits_stored,
        metadata.high_bit,
    );
    let format_error = |field: &str, why: &str| {
        Error::new(
            ErrorKind::Other,
            format!("{ERR_INVALID_SAMPLE_FORMAT}: {field}: {why}"),
        )
    };
    if ba != 8 && ba != 16 && ba != 32 {
        return Err(format_error(
            "BitsAllocated",
            "BitsAllocated must be 8, 16 or 32",
        ));
    }
    if bs < 1 || bs > ba {
        return Err(format_error(
            "BitsStored",
            "BitsStored must be between 1 and BitsAllocated",
        ));
    }
    if hb < bs - 1 || hb >= ba {
        return Err(format_error(
            "HighBit",
            "HighBit must be between BitsStored-1 and BitsAllocated-1",
        ));
    }
    let signed = metadata.pixel_representation != 0;
    let mask = (1u64 << bs) - 1;
    let sign_bit = if signed { 1u64 << (bs - 1) } else { 0 };
    let minimum: i64 = if signed { -(sign_bit as i64) } else { 0 };
    let size = frame_size(&metadata);
    if (data.len() as i64) < size {
        return Err(Error::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!("part of {} bytes, want {size}", data.len()),
        ));
    }
    let width = usize::from(ba / 8);
    let (columns, rows) = (usize::from(metadata.columns), usize::from(metadata.rows));
    let samples = usize::from(metadata.samples_per_pixel);
    let (planes, per_pixel) = if samples > 1 && metadata.planar_configuration == 1 {
        (samples, 1)
    } else {
        (1, samples)
    };
    let cell = 1usize << level;
    let (out_columns, out_rows) = (columns.div_ceil(cell), rows.div_ceil(cell));
    let mut out = vec![0u8; out_columns * out_rows * samples * width];
    let shift = u32::from(hb + 1 - bs);
    let offset = |at: usize| -> u64 {
        let raw = match width {
            1 => u64::from(data[at]),
            2 => u64::from(u16::from_le_bytes([data[at], data[at + 1]])),
            _ => u64::from(u32::from_le_bytes([
                data[at],
                data[at + 1],
                data[at + 2],
                data[at + 3],
            ])),
        };
        ((raw >> shift) & mask) ^ sign_bit
    };
    let line = out_columns * per_pixel;
    let mut sums = vec![0u128; line];
    let mut at = 0usize;
    for plane in 0..planes {
        for oy in 0..out_rows {
            sums.iter_mut().for_each(|s| *s = 0);
            let (y0, y1) = (oy * cell, ((oy + 1) * cell).min(rows));
            for y in y0..y1 {
                let mut row = ((plane * rows + y) * columns * per_pixel) * width;
                for x in 0..columns {
                    let base = (x >> level) * per_pixel;
                    for s in 0..per_pixel {
                        sums[base + s] += u128::from(offset(row));
                        row += width;
                    }
                }
            }
            for ox in 0..out_columns {
                let count = ((y1 - y0) * (((ox + 1) * cell).min(columns) - ox * cell)) as u128;
                for s in 0..per_pixel {
                    let mean = (sums[ox * per_pixel + s] + count / 2) / count;
                    let raw = (((mean as u64 as i64).wrapping_add(minimum) as u64) & mask) << shift;
                    out[at..at + width].copy_from_slice(&raw.to_le_bytes()[..width]);
                    at += width;
                }
            }
        }
    }
    metadata.rows = out_rows as u16;
    metadata.columns = out_columns as u16;
    Ok((out, metadata))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reduced_regions_round_out() {
        assert_eq!(
            reduced_region(FrameRegion::new(1, 2, 5, 7), 1),
            FrameRegion::new(0, 1, 3, 4)
        );
        assert_eq!(
            reduced_region(FrameRegion::new(0, 0, 3, 3), 0),
            FrameRegion::new(0, 0, 3, 3)
        );
    }
    #[test]
    fn averages_signed_cells_half_up() {
        let m = Metadata {
            rows: 2,
            columns: 3,
            samples_per_pixel: 1,
            bits_allocated: 8,
            bits_stored: 8,
            high_bit: 7,
            pixel_representation: 1,
            number_of_frames: 1,
            photometric_interpretation: "MONOCHROME2".into(),
            ..Metadata::default()
        };
        let data = [0xFF, 0, 5, 0xFE, 1, 6];
        let (out, m) = average_cells(&data, m, 1).unwrap_or_default();
        assert_eq!((m.rows, m.columns), (1, 2));
        assert_eq!(out, vec![0, 6]);
    }
}
