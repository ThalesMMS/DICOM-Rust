use super::Metadata;
use super::pixeldata::{extract_metadata, extract_native_frames, frame_size};
use super::transcode::{
    self, DecoderRegistry, DecompressLimits, DecompressOptions, EncoderRegistry, TranscodeLimits,
    TranscodeOptions, canonical_source, clone_detached, equivalent_clone_limits,
    require_lossless_encoder, resolve_limits,
};
use crate::object::{Element, ElementHeader, Object, Tag, Value};
use std::fmt;
const TAG_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0008);
const TAG_DOUBLE_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0009);
const UID_EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const UID_ENCAPSULATED_UNCOMPRESSED: &str = "1.2.840.10008.1.2.1.98";
pub const ERR_INVALID_REDACTION: &str = "dicom: invalid pixel redaction request";
pub const ERR_REDACTION_UNSUPPORTED: &str =
    "dicom: pixel redaction is unsupported for this pixel data";
pub const ERR_REDACTION_NOT_LOSSLESS: &str =
    "dicom: redacted pixel data cannot be re-encoded losslessly";
const ERR_VERIFICATION: &str = "re-encoded pixel data does not decode to the redacted frames";
pub const REDACTION_ALL_FRAMES: i64 = -1;
pub const DEFAULT_MAX_REDACTION_REGIONS: i64 = 10_000;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedactionRegion {
    pub frame: i64,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedactionOutput(pub u8);
impl RedactionOutput {
    pub const LOSSLESS_OR_NATIVE: RedactionOutput = RedactionOutput(0);
    pub const LOSSLESS_ONLY: RedactionOutput = RedactionOutput(1);
    pub const NATIVE: RedactionOutput = RedactionOutput(2);
}
#[derive(Clone, Debug, Default)]
pub struct RedactionOptions {
    pub decoder: Option<DecoderRegistry>,
    pub encoders: Option<EncoderRegistry>,
    pub fill: i64,
    pub output: RedactionOutput,
    pub limits: TranscodeLimits,
    pub max_regions: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedactionCoverage {
    pub frames: i64,
    pub frames_covered: i64,
    pub regions: i64,
}
impl RedactionCoverage {
    pub fn all_frames_covered(&self) -> bool {
        self.frames > 0 && self.frames_covered == self.frames
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RedactionReport {
    pub coverage: RedactionCoverage,
    pub pixels_redacted: i64,
    pub source_transfer_syntax_uid: String,
    pub output_transfer_syntax_uid: String,
    pub reencoded: bool,
    pub decoded_to_native: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedactErrorKind {
    Invalid,
    Unsupported,
    NotLossless,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedactError {
    pub kind: RedactErrorKind,
    pub text: String,
}
impl fmt::Display for RedactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for RedactError {}
fn invalid(detail: impl fmt::Display) -> RedactError {
    RedactError {
        kind: RedactErrorKind::Invalid,
        text: format!("{ERR_INVALID_REDACTION}: {detail}"),
    }
}
fn unsupported(detail: impl fmt::Display) -> RedactError {
    RedactError {
        kind: RedactErrorKind::Unsupported,
        text: format!("{ERR_REDACTION_UNSUPPORTED}: {detail}"),
    }
}
fn other(err: impl fmt::Display) -> RedactError {
    RedactError {
        kind: RedactErrorKind::Other,
        text: err.to_string(),
    }
}
pub fn validate_redaction_regions(
    metadata: &Metadata,
    regions: &[RedactionRegion],
    max_regions: i64,
) -> Result<RedactionCoverage, RedactError> {
    let max_regions = if max_regions <= 0 {
        DEFAULT_MAX_REDACTION_REGIONS
    } else {
        max_regions
    };
    if regions.is_empty() {
        return Err(invalid("no regions"));
    }
    if regions.len() as i64 > max_regions {
        return Err(invalid(format!(
            "{} regions exceed the limit of {max_regions}",
            regions.len()
        )));
    }
    let frames = metadata.number_of_frames.max(1);
    let (rows, columns) = (i64::from(metadata.rows), i64::from(metadata.columns));
    if rows <= 0 || columns <= 0 {
        return Err(invalid("image has no rows or columns"));
    }
    let mut covered = vec![false; frames as usize];
    let mut coverage = RedactionCoverage {
        frames,
        ..Default::default()
    };
    for (index, r) in regions.iter().enumerate() {
        if r.width <= 0
            || r.height <= 0
            || r.x < 0
            || r.y < 0
            || r.x >= columns
            || r.y >= rows
            || r.width > columns.saturating_sub(r.x)
            || r.height > rows.saturating_sub(r.y)
        {
            return Err(invalid(format!(
                "region {} is outside the {columns}x{rows} image",
                index.saturating_add(1)
            )));
        }
        if r.frame == REDACTION_ALL_FRAMES {
            covered.iter_mut().for_each(|c| *c = true);
            coverage.regions = coverage.regions.saturating_add(frames);
        } else if let Some(c) = usize::try_from(r.frame)
            .ok()
            .and_then(|f| covered.get_mut(f))
        {
            *c = true;
            coverage.regions = coverage.regions.saturating_add(1);
        } else {
            return Err(invalid(format!(
                "region {} names a frame outside {frames} frame(s)",
                index.saturating_add(1)
            )));
        }
    }
    coverage.frames_covered = covered.iter().filter(|c| **c).count() as i64;
    Ok(coverage)
}
pub fn redact_data_set(
    dataset: &Object,
    source: &str,
    regions: &[RedactionRegion],
    options: &RedactionOptions,
) -> Result<(Object, String, RedactionReport), RedactError> {
    if options.output.0 > RedactionOutput::NATIVE.0 {
        return Err(invalid("unknown output policy"));
    }
    if dataset.has(TAG_FLOAT_PIXEL_DATA) || dataset.has(TAG_DOUBLE_FLOAT_PIXEL_DATA) {
        return Err(unsupported("float pixel data"));
    }
    let Some(element) = dataset.get(Tag::PIXEL_DATA) else {
        return Err(unsupported("missing Pixel Data"));
    };
    if matches!(element.value, Value::Nil) {
        return Err(unsupported("Pixel Data is not loaded"));
    }
    let limits = resolve_limits(options.limits).map_err(other)?;
    let source = canonical_source(source).map_err(other)?;
    if source.media_payload {
        return Err(unsupported("media payload transfer syntax"));
    }
    let metadata = extract_metadata(dataset).map_err(other)?;
    let coverage = validate_redaction_regions(&metadata, regions, options.max_regions)?;
    let native_in_place = !source.encapsulated && source.is_little_endian();
    let mut native = if native_in_place {
        clone_detached(dataset, &equivalent_clone_limits(limits)).map_err(other)?
    } else {
        let opts = TranscodeOptions {
            decoder: options.decoder,
            limits: options.limits,
            ..Default::default()
        };
        transcode::transcode_data_set(dataset, &source.uid, UID_EXPLICIT_LE, &opts)
            .map_err(other)?
            .0
    };
    let (redacted, pixels) = redact_native_object(&mut native, regions, options.fill)?;
    let mut report = RedactionReport {
        coverage,
        pixels_redacted: pixels,
        source_transfer_syntax_uid: source.uid.to_string(),
        ..Default::default()
    };
    if native_in_place {
        report.output_transfer_syntax_uid = source.uid.to_string();
        return Ok((native, source.uid.to_string(), report));
    }
    if source.encapsulated && options.output != RedactionOutput::NATIVE {
        match reencode_redacted(&native, &source.uid, &redacted, options, &limits) {
            Ok(encoded) => {
                report.reencoded = true;
                report.output_transfer_syntax_uid = source.uid.to_string();
                return Ok((encoded, source.uid.to_string(), report));
            }
            Err((verification, err)) => {
                if verification || options.output == RedactionOutput::LOSSLESS_ONLY {
                    return Err(RedactError {
                        kind: RedactErrorKind::NotLossless,
                        text: format!("{ERR_REDACTION_NOT_LOSSLESS}: {err}"),
                    });
                }
            }
        }
    }
    report.decoded_to_native = true;
    report.output_transfer_syntax_uid = UID_EXPLICIT_LE.to_string();
    Ok((native, UID_EXPLICIT_LE.to_string(), report))
}
fn reencode_redacted(
    native: &Object,
    source: &str,
    redacted: &[Vec<u8>],
    options: &RedactionOptions,
    limits: &TranscodeLimits,
) -> Result<Object, (bool, String)> {
    if source != UID_ENCAPSULATED_UNCOMPRESSED {
        require_lossless_encoder(options.encoders.as_ref(), source)
            .map_err(|e| (false, e.to_string()))?;
    }
    let opts = TranscodeOptions {
        decoder: options.decoder,
        encoders: options.encoders.clone(),
        limits: options.limits,
        ..Default::default()
    };
    let (encoded, _) = transcode::transcode_data_set(native, UID_EXPLICIT_LE, source, &opts)
        .map_err(|e| (false, e.to_string()))?;
    let decompress = DecompressOptions {
        registry: options.decoder,
        limits: DecompressLimits {
            max_frames: limits.max_frames,
            max_pixels: limits.max_pixels as i64,
            max_input_bytes: limits.max_output_bytes,
            max_native_bytes: limits.max_output_bytes,
            max_expansion_ratio: limits.max_expansion_ratio,
        },
        ..Default::default()
    };
    let (decoded, _) = transcode::decompress_data_set(&encoded, source, &decompress)
        .map_err(|e| (true, format!("{ERR_VERIFICATION}: {e}")))?;
    let frames = extract_native_frames(&decoded).map_err(|_| (true, ERR_VERIFICATION.into()))?;
    if frames.data.len() != redacted.len() || frames.data.iter().zip(redacted).any(|(a, b)| a != b)
    {
        return Err((true, ERR_VERIFICATION.into()));
    }
    Ok(encoded)
}
fn redact_native_object(
    obj: &mut Object,
    regions: &[RedactionRegion],
    fill: i64,
) -> Result<(Vec<Vec<u8>>, i64), RedactError> {
    let Some(element) = obj.get(Tag::PIXEL_DATA) else {
        return Err(unsupported("missing Pixel Data"));
    };
    let vr = element.vr();
    let native = extract_native_frames(obj).map_err(unsupported)?;
    let m = &native.metadata;
    if !matches!(m.bits_allocated, 8 | 16 | 32) {
        return Err(unsupported(format!("BitsAllocated={}", m.bits_allocated)));
    }
    let photometric = super::pixeldata::normalized_photometric(&m.photometric_interpretation);
    if matches!(
        photometric.as_str(),
        "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420"
    ) {
        return Err(unsupported("subsampled native color"));
    }
    if m.planar_configuration_present && m.planar_configuration > 1 {
        return Err(unsupported(format!(
            "PlanarConfiguration={}",
            m.planar_configuration
        )));
    }
    let sample = fill_sample(m, fill)?;
    let (rows, columns) = (usize::from(m.rows), usize::from(m.columns));
    let samples = usize::from(m.samples_per_pixel);
    let width = sample.len();
    let planar = samples > 1 && m.planar_configuration_present && m.planar_configuration == 1;
    let size = frame_size(m);
    let mut frames = native.data.clone();
    for (index, frame) in frames.iter().enumerate() {
        if (frame.len() as i64) < size {
            return Err(unsupported(format!(
                "truncated frame {}",
                index.saturating_add(1)
            )));
        }
    }
    let mut pixels = 0i64;
    let mut fill_rect = |frame: &mut [u8], r: &RedactionRegion| {
        let (x0, y0) = (r.x as usize, r.y as usize);
        let (x1, y1) = (
            r.x.saturating_add(r.width) as usize,
            r.y.saturating_add(r.height) as usize,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let pixel = y.saturating_mul(columns).saturating_add(x);
                for s in 0..samples {
                    let index = if planar {
                        s.saturating_mul(rows)
                            .saturating_mul(columns)
                            .saturating_add(pixel)
                    } else {
                        pixel.saturating_mul(samples).saturating_add(s)
                    };
                    let offset = index.saturating_mul(width);
                    if let Some(t) = frame.get_mut(offset..offset.saturating_add(width)) {
                        t.copy_from_slice(&sample);
                    }
                }
            }
        }
        pixels = pixels.saturating_add(r.width.saturating_mul(r.height));
    };
    let count = frames.len() as i64;
    for r in regions {
        if r.frame == REDACTION_ALL_FRAMES {
            for frame in frames.iter_mut() {
                fill_rect(frame, r);
            }
            continue;
        }
        let Some(frame) = usize::try_from(r.frame)
            .ok()
            .and_then(|f| frames.get_mut(f))
        else {
            return Err(invalid(format!("frame outside {count} frame(s)")));
        };
        fill_rect(frame, r);
    }
    let mut joined: Vec<u8> = frames.concat();
    if !joined.len().is_multiple_of(2) {
        joined.push(0);
    }
    obj.put(Element {
        header: ElementHeader {
            tag: Tag::PIXEL_DATA,
            vr,
            length: joined.len() as u32,
            length_set: true,
        },
        value: Value::Raw(joined),
    });
    Ok((frames, pixels))
}
fn fill_sample(m: &Metadata, fill: i64) -> Result<Vec<u8>, RedactError> {
    let bits_stored = i64::from(m.bits_stored);
    if bits_stored <= 0 || bits_stored > i64::from(m.bits_allocated) {
        return Err(unsupported(format!("BitsStored={bits_stored}")));
    }
    let (low, high) = if m.pixel_representation == 1 {
        let half = 1i64 << bits_stored.saturating_sub(1);
        (half.saturating_neg(), half.saturating_sub(1))
    } else {
        (0, (1i64 << bits_stored).saturating_sub(1))
    };
    if fill < low || fill > high {
        return Err(invalid(format!(
            "fill value outside the {bits_stored}-bit stored range"
        )));
    }
    let width = usize::from(m.bits_allocated).div_ceil(8);
    let value = fill as u64;
    Ok(value.to_le_bytes().into_iter().take(width).collect())
}
