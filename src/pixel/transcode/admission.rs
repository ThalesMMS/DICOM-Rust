use super::decompress::{check_admission, transcode_decompress_limits};
use super::output::{check_icon_budget, icon_parts};
use super::{
    Error, ErrorKind, Mode, TranscodeOptions, UID_ENCAPSULATED_UNCOMPRESSED, build_plan,
    canonical_source, canonical_target, file_source_syntax, has_any_pixel_data, payload_bytes,
    pixel_view, resolve_limits, validate_metadata_bounds,
};
use crate::object::File;
use crate::pixel::frame_reader::{FrameReader, FrameReaderOptions};
use crate::pixel::pixeldata::{PixelErrorKind, extract_metadata};
use crate::transfer::{self, Syntax};
const TAG_META_TRANSFER_SYNTAX: crate::object::Tag = crate::object::Tag::new(0x0002, 0x0010);
pub fn is_limit_error(err: &Error) -> bool {
    err.is(ErrorKind::DecompressResourceLimit)
        || err.is(ErrorKind::ResourceLimit)
        || err.is(ErrorKind::EncapsulatedResourceLimit)
}
fn source_syntax(file: &File) -> Result<Syntax, Error> {
    let file_uid = transfer::normalize_uid(file.transfer_syntax.uid).to_string();
    let meta_uid = file
        .meta
        .get_string(TAG_META_TRANSFER_SYNTAX)
        .map(|u| transfer::normalize_uid(&u).to_string())
        .unwrap_or_default();
    if !file_uid.is_empty() && !meta_uid.is_empty() && file_uid != meta_uid {
        return Err(Error::pixel(
            PixelErrorKind::IncompatiblePixelData,
            "file transfer syntax does not match File Meta Information",
        ));
    }
    if !file_uid.is_empty() {
        return canonical_source(&file_uid);
    }
    if !meta_uid.is_empty() {
        return canonical_source(&meta_uid);
    }
    Err(Error::pixel(
        PixelErrorKind::IncompatiblePixelData,
        "source transfer syntax is empty",
    ))
}
pub(super) fn source_frame_reader_options(
    o: &TranscodeOptions,
    limits: &super::TranscodeLimits,
) -> FrameReaderOptions {
    let mut decode = transcode_decompress_limits(limits);
    decode.max_frames = 0;
    FrameReaderOptions {
        decoder: o.decoder,
        limits: decode,
        max_frames: limits.max_frames,
        max_fragments: limits.max_fragments,
    }
}
#[doc(hidden)]
pub fn check_decode_limits(
    file: &File,
    target: &str,
    options: &TranscodeOptions,
) -> Result<(), Error> {
    check_limits(file, target, options, false)
}
#[doc(hidden)]
pub fn check_decode_limits_deferred(
    file: &File,
    target: &str,
    options: &TranscodeOptions,
) -> Result<(), Error> {
    check_limits(file, target, options, true)
}
fn check_limits(
    file: &File,
    target: &str,
    options: &TranscodeOptions,
    deferred: bool,
) -> Result<(), Error> {
    let limits = resolve_limits(options.limits)?;
    let syntax = source_syntax(file)?;
    canonical_target(target)?;
    let target = canonical_target(target)?;
    let dataset = &file.dataset;
    if let Some((main, icons)) = icon_parts(dataset, &limits)? {
        check_icon_budget(&main, &icons, dataset.value_big_endian(), &limits)?;
    }
    if syntax.encapsulated {
        file_source_syntax(file)?;
    } else {
        let Ok(metadata) = extract_metadata(dataset) else {
            return Ok(());
        };
        return validate_metadata_bounds(&metadata, &limits);
    }
    let mode = build_plan(
        &syntax,
        &target,
        options.force_reencode,
        has_any_pixel_data(dataset),
    );
    if matches!(mode, Mode::MissingPixelData | Mode::EquivalentClone) {
        return Ok(());
    }
    let metadata = extract_metadata(dataset)?;
    let decode = transcode_decompress_limits(&limits);
    let reader = match FrameReader::new(file, source_frame_reader_options(options, &limits)) {
        Ok(r) => r,
        Err(_) => {
            validate_metadata_bounds(&metadata, &limits)?;
            if deferred {
                scanned_fragment_limit(file, limits.max_fragments)?;
            }
            let input = whole_pixel_input_bytes(file)?;
            return check_admission(&decode, &metadata, &syntax, input);
        }
    };
    check_admission(&decode, &metadata, &syntax, reader.input_bytes())?;
    if syntax.uid == UID_ENCAPSULATED_UNCOMPRESSED || reader.is_native() {
        return Ok(());
    }
    for i in 0..metadata.number_of_frames {
        if let Err(e) = reader.admitted_frame_input_bytes(i) {
            if is_limit_error(&e) {
                return Err(e);
            }
            return Ok(());
        }
    }
    Ok(())
}
fn scanned_fragment_limit(file: &File, max_fragments: i64) -> Result<(), Error> {
    let Some(pixel) = file.dataset.get(super::TAG_PIXEL_DATA) else {
        return Ok(());
    };
    if let crate::object::Value::Fragments { fragments, .. } = &pixel.value
        && fragments.len() as i64 > max_fragments
    {
        let limit = "dicom: encapsulated frame resource limit exceeded";
        return Err(Error::wrapping(
            ErrorKind::FrameLayout,
            format!(
                "dicom: frames cannot be located in Pixel Data: {limit}: fragment count exceeds {max_fragments}"
            ),
            Error::new(ErrorKind::EncapsulatedResourceLimit, limit),
        ));
    }
    Ok(())
}
fn whole_pixel_input_bytes(file: &File) -> Result<i64, Error> {
    let Some(pixel) = file.dataset.get(super::TAG_PIXEL_DATA) else {
        return Err(Error::pixel_bare(PixelErrorKind::PixelDataNotFound));
    };
    if matches!(pixel.value, crate::object::Value::Nil) {
        return Err(Error::new(
            ErrorKind::FrameLayout,
            "dicom: frames cannot be located in Pixel Data: deferred Pixel Data needs its seekable source",
        ));
    }
    Ok(payload_bytes(&pixel_view(pixel)?))
}
