use super::decompress::{DecompressLimits, decompress_for_transcode};
use super::output::canonical_native_bytes;
use super::{
    DecoderRegistry, Error, ErrorKind, TAG_PIXEL_DATA, TranscodeLimits, TranscodeOptions,
    UID_ENCAPSULATED_UNCOMPRESSED, fragments_element, object_from, remove_elements,
    strings_element,
};
use crate::display::StoredSampleReader;
use crate::object::{Object, Tag, Vr};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{PixelErrorKind, extract_native_frames, frame_size};
use crate::transfer::{self, Syntax};
#[doc(hidden)]
pub const ERR_VERIFICATION_OPTIONS: &str = "dicom: invalid lossy verification options";
#[doc(hidden)]
pub const ERR_PIXEL_ERROR: &str = "dicom: lossy pixel error exceeds limit";
#[doc(hidden)]
pub const ERR_COMPARISON_UNSUPPORTED: &str =
    "dicom: lossy sample representations cannot be compared";
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LossyVerificationReport {
    pub maximum_error: f64,
    pub maximum_sample_error: i64,
    pub limit: f64,
    pub average_block_size: i64,
    pub frames: i64,
    pub blocks: i64,
}
fn options_error() -> Error {
    Error::new(ErrorKind::VerificationOptions, ERR_VERIFICATION_OPTIONS)
}
pub(crate) fn comparison_unsupported() -> Error {
    Error::new(ErrorKind::ComparisonUnsupported, ERR_COMPARISON_UNSUPPORTED)
}
#[doc(hidden)]
pub fn resolve_verification_options(mut o: TranscodeOptions) -> Result<TranscodeOptions, Error> {
    if o.average_block_size < 0 || o.max_pixel_error.is_none() && o.average_block_size != 0 {
        return Err(options_error());
    }
    let Some(limit) = o.max_pixel_error else {
        return Ok(o);
    };
    if !limit.is_finite() || limit < 0.0 || o.skip_verification {
        return Err(options_error());
    }
    if o.average_block_size == 0 {
        o.average_block_size = 1;
    }
    Ok(o)
}
pub(crate) fn add_lossy(
    aggregate: &mut Option<LossyVerificationReport>,
    frame: Option<LossyVerificationReport>,
) {
    let Some(frame) = frame else {
        return;
    };
    match aggregate {
        None => *aggregate = Some(frame),
        Some(current) => {
            current.maximum_error = crate::fmath::max(current.maximum_error, frame.maximum_error);
            current.maximum_sample_error =
                current.maximum_sample_error.max(frame.maximum_sample_error);
            current.frames = current.frames.saturating_add(frame.frames);
            current.blocks = current.blocks.saturating_add(frame.blocks);
        }
    }
}
pub(crate) fn check_codec_availability(
    registry: Option<DecoderRegistry>,
    uid: &str,
) -> Result<(), Error> {
    match registry {
        None => Err(super::decompress::codec_registry_nil(
            transfer::normalize_uid(uid),
        )),
        Some(registry) => registry
            .check(transfer::normalize_uid(uid))
            .map_err(Error::from),
    }
}
pub(crate) fn validate_target_verification_codec(
    source: &Syntax,
    target: &Syntax,
    o: &TranscodeOptions,
) -> Result<(), Error> {
    if o.skip_verification {
        return Ok(());
    }
    if !target.encapsulated
        || target.uid == UID_ENCAPSULATED_UNCOMPRESSED
        || source.uid == target.uid && !o.force_reencode
    {
        return Ok(());
    }
    let Some(registry) = &o.encoders else {
        return Ok(());
    };
    if !registry.has(&target.uid) {
        return Ok(());
    }
    check_codec_availability(o.decoder, &target.uid)
}
fn encoded_frame_proxy(header: &Object, encoded: &[u8]) -> Object {
    let mut elements = header.elements().to_vec();
    remove_elements(&mut elements, &[TAG_PIXEL_DATA, TAG_NUMBER_OF_FRAMES]);
    elements.push(strings_element(
        TAG_NUMBER_OF_FRAMES,
        Vr::IS,
        vec!["1".into()],
    ));
    elements.push(fragments_element(Vec::new(), vec![encoded.to_vec()]));
    object_from(elements, false)
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_transcoded_frame(
    source_header: &Object,
    output_header: &Object,
    encoded: &[u8],
    native: &[u8],
    target: &Syntax,
    o: &TranscodeOptions,
    limits: &TranscodeLimits,
    frame: usize,
    lossless: bool,
) -> Result<Option<LossyVerificationReport>, Error> {
    let proxy = encoded_frame_proxy(output_header, encoded);
    let mut limits = *limits;
    limits.max_input_bytes = limits.max_output_bytes;
    let Some(max_error) = o.max_pixel_error.filter(|_| !lossless) else {
        let decoded = canonical_native_bytes(&proxy, target, o.decoder, &limits)?;
        if lossless && decoded != native {
            return Err(Error::pixel(
                PixelErrorKind::IncompatiblePixelData,
                "lossless Pixel Data changed",
            ));
        }
        return Ok(None);
    };
    if target.encapsulated && target.uid != UID_ENCAPSULATED_UNCOMPRESSED && o.decoder.is_none() {
        return Err(super::decompress::codec_registry_nil(&target.uid));
    }
    let (decoded, _) = decompress_for_transcode(
        &proxy,
        target,
        o.decoder,
        DecompressLimits {
            max_frames: 1,
            max_pixels: limits.max_pixels as i64,
            max_input_bytes: limits.max_input_bytes,
            max_native_bytes: limits.max_output_bytes,
            max_expansion_ratio: limits.max_expansion_ratio,
        },
    )?;
    let output = extract_native_frames(&decoded)?;
    if output.data.len() != 1 {
        return Err(comparison_unsupported());
    }
    let source_metadata = crate::pixel::pixeldata::extract_metadata(source_header)?;
    let Some(decoded) = output.data.first() else {
        return Err(comparison_unsupported());
    };
    let mut measured = compare_lossy_frame(
        native,
        &source_metadata,
        decoded,
        &output.metadata,
        o.average_block_size,
    )?;
    measured.limit = max_error;
    if measured.maximum_error > measured.limit {
        return Err(lossy_error((frame as i64).saturating_add(1), &measured));
    }
    Ok(Some(measured))
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LossyVerificationError {
    pub frame: i64,
    pub measured: f64,
    pub limit: f64,
    pub average_block_size: i64,
}
impl std::fmt::Display for LossyVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lossy verification failed at frame {}: measured error {} above the limit {}",
            self.frame, self.measured, self.limit
        )
    }
}
impl std::error::Error for LossyVerificationError {}
pub(crate) fn lossy_error(frame: i64, m: &LossyVerificationReport) -> Error {
    Error::new(
        ErrorKind::PixelError,
        format!(
            "{ERR_PIXEL_ERROR}: frame={frame} measured={} limit={} stored-sample-units block-side={}",
            crate::numtext::format_float(m.maximum_error, 'g', -1),
            crate::numtext::format_float(m.limit, 'g', -1),
            m.average_block_size
        ),
    )
    .with_lossy(&LossyVerificationError {
        frame,
        measured: m.maximum_error,
        limit: m.limit,
        average_block_size: m.average_block_size,
    })
}
fn normalized_photometric(value: &str) -> String {
    crate::textfmt::to_upper(value)
        .trim_matches([' ', '\0'])
        .to_owned()
}
fn sample_reader(m: &Metadata) -> Result<StoredSampleReader, Error> {
    StoredSampleReader::new(
        i64::from(m.bits_allocated),
        i64::from(m.bits_stored),
        i64::from(m.high_bit),
        m.pixel_representation != 0,
        false,
    )
    .map_err(|_| {
        Error::new(
            ErrorKind::ComparisonUnsupported,
            format!("{ERR_COMPARISON_UNSUPPORTED}: sample format"),
        )
    })
}
fn lossy_sample_index(
    pixel: usize,
    component: usize,
    pixels: usize,
    components: usize,
    planar: u16,
) -> usize {
    if components > 1 && planar == 1 {
        component.saturating_mul(pixels).saturating_add(pixel)
    } else {
        pixel.saturating_mul(components).saturating_add(component)
    }
}
pub(crate) fn compare_lossy_frame(
    source: &[u8],
    a: &Metadata,
    decoded: &[u8],
    b: &Metadata,
    block: i64,
) -> Result<LossyVerificationReport, Error> {
    let pi = normalized_photometric(&a.photometric_interpretation);
    if a.rows == 0
        || a.columns == 0
        || a.rows != b.rows
        || a.columns != b.columns
        || a.samples_per_pixel != b.samples_per_pixel
        || a.bits_stored != b.bits_stored
        || a.pixel_representation != b.pixel_representation
        || pi != normalized_photometric(&b.photometric_interpretation)
        || block < 1
    {
        return Err(comparison_unsupported());
    }
    if !(a.samples_per_pixel == 1 && (pi == "MONOCHROME1" || pi == "MONOCHROME2")
        || a.samples_per_pixel == 3 && (pi == "RGB" || pi == "YBR_FULL"))
        || a.pixel_representation > 1
        || a.planar_configuration > 1
        || b.planar_configuration > 1
    {
        return Err(comparison_unsupported());
    }
    let ar = sample_reader(a)?;
    let br = sample_reader(b)?;
    if source.len() as i64 != frame_size(a) || decoded.len() as i64 != frame_size(b) {
        return Err(Error::pixel_bare(PixelErrorKind::PixelDataSizeMismatch));
    }
    let (rows, cols, components) = (
        usize::from(a.rows),
        usize::from(a.columns),
        usize::from(a.samples_per_pixel),
    );
    let block = block as usize;
    let area = (block.min(rows) as i64).saturating_mul(block.min(cols) as i64);
    let max_difference = 1i64
        .checked_shl(u32::from(a.bits_stored))
        .map_or(0, |v| v.saturating_sub(1));
    if max_difference <= 0 || area > i64::MAX.checked_div(max_difference).unwrap_or(0) {
        return Err(Error::new(
            ErrorKind::ComparisonUnsupported,
            format!("{ERR_COMPARISON_UNSUPPORTED}: block accumulator range"),
        ));
    }
    let mut report = LossyVerificationReport {
        average_block_size: block as i64,
        frames: 1,
        ..LossyVerificationReport::default()
    };
    let pixels = rows.saturating_mul(cols);
    let mut y = 0;
    while y < rows {
        let y_end = y.saturating_add(block.min(rows.saturating_sub(y)));
        let mut x = 0;
        while x < cols {
            let x_end = x.saturating_add(block.min(cols.saturating_sub(x)));
            let count =
                (y_end.saturating_sub(y) as i64).saturating_mul(x_end.saturating_sub(x) as i64);
            for c in 0..components {
                let mut sum = 0i64;
                for by in y..y_end {
                    for bx in x..x_end {
                        let pixel = by.saturating_mul(cols).saturating_add(bx);
                        let ai = lossy_sample_index(
                            pixel,
                            c,
                            pixels,
                            components,
                            a.planar_configuration,
                        )
                        .saturating_mul(ar.width());
                        let bi = lossy_sample_index(
                            pixel,
                            c,
                            pixels,
                            components,
                            b.planar_configuration,
                        )
                        .saturating_mul(br.width());
                        let (Some(sa), Some(sb)) = (source.get(ai..), decoded.get(bi..)) else {
                            return Err(Error::pixel_bare(PixelErrorKind::PixelDataSizeMismatch));
                        };
                        let difference = ar.value(sa).saturating_sub(br.value(sb));
                        sum = sum.saturating_add(difference);
                        report.maximum_sample_error =
                            report.maximum_sample_error.max(difference.saturating_abs());
                    }
                }
                report.maximum_error = crate::fmath::max(
                    report.maximum_error,
                    sum.saturating_abs() as f64 / count as f64,
                );
                report.blocks = report.blocks.saturating_add(1);
            }
            x = x_end;
        }
        y = y_end;
    }
    Ok(report)
}
pub(crate) fn validate_readback(
    dataset: &Object,
    syntax: &Syntax,
    limits: &TranscodeLimits,
) -> Result<(), Error> {
    let integer = dataset.get(TAG_PIXEL_DATA);
    if integer.is_none()
        || dataset.has(super::TAG_FLOAT_PIXEL_DATA)
        || dataset.has(super::TAG_DOUBLE_FLOAT_PIXEL_DATA)
    {
        return Err(Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED));
    }
    let Some(e) = integer else {
        return Err(Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED));
    };
    let pixel = super::pixel_view(e)?;
    let encapsulated = matches!(
        pixel,
        crate::pixel::pixeldata::PixelData::Encapsulated { .. }
    );
    if encapsulated != syntax.encapsulated {
        return Err(Error::pixel_bare(PixelErrorKind::IncompatiblePixelData));
    }
    let fragments = match &pixel {
        crate::pixel::pixeldata::PixelData::Encapsulated { fragments, .. } => fragments.len(),
        crate::pixel::pixeldata::PixelData::Native(_) => 0,
    };
    if fragments as i64 > limits.max_fragments
        || super::payload_bytes(&pixel) > limits.max_output_bytes
    {
        return Err(Error::new(
            ErrorKind::ResourceLimit,
            super::ERR_RESOURCE_LIMIT,
        ));
    }
    if syntax.encapsulated {
        crate::pixel::pixeldata::extract_metadata(dataset)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verification_options() {
        let o = TranscodeOptions {
            average_block_size: 2,
            ..TranscodeOptions::default()
        };
        assert_eq!(
            resolve_verification_options(o).unwrap_err().to_string(),
            ERR_VERIFICATION_OPTIONS
        );
        let o = TranscodeOptions {
            max_pixel_error: Some(1.5),
            ..TranscodeOptions::default()
        };
        assert_eq!(
            resolve_verification_options(o).map(|o| o.average_block_size),
            Ok(1)
        );
    }
    #[test]
    fn compare_blocks() {
        let m = Metadata {
            rows: 2,
            columns: 2,
            samples_per_pixel: 1,
            bits_allocated: 8,
            bits_stored: 8,
            high_bit: 7,
            number_of_frames: 1,
            photometric_interpretation: "MONOCHROME2".into(),
            ..Metadata::default()
        };
        let r = compare_lossy_frame(&[10, 10, 10, 10], &m, &[12, 8, 10, 10], &m, 2).unwrap();
        assert_eq!(
            (r.maximum_error, r.maximum_sample_error, r.blocks),
            (0.0, 2, 1)
        );
    }
}
