use super::super::Metadata;
use super::super::pixeldata::{
    self as pd, Frames, PixelData, PixelErrorKind, extract_metadata, extract_view, frame_size,
    normalized_photometric, total_size,
};
use super::{
    DecoderRegistry, ERR_PARTIAL_FILE, Error, ErrorKind, TAG_ENCAPSULATED_TOTAL_LENGTH,
    TAG_EXTENDED_OFFSET_TABLE, TAG_EXTENDED_OFFSET_TABLE_LENGTHS, TAG_PHOTOMETRIC, TAG_PIXEL_DATA,
    TAG_PLANAR_CONFIGURATION, TAG_SAMPLES_PER_PIXEL, TranscodeLimits,
    UID_ENCAPSULATED_UNCOMPRESSED, UID_EXPLICIT_LE, UID_IMPLICIT_LE, canonical_source,
    file_source_syntax, object_from, raw_element, remove_elements, replace_element,
    strings_element, swap_elements, uint16_element,
};
use crate::object::{File, Object, Tag, Value, Vr};
use crate::textfmt;
use crate::transfer::{self, Syntax};
mod recovery;
pub use recovery::*;
const TAG_LOSSY: Tag = Tag::new(0x0028, 0x2110);
const TAG_LOSSY_RATIO: Tag = Tag::new(0x0028, 0x2112);
const TAG_LOSSY_METHOD: Tag = Tag::new(0x0028, 0x2114);
const ERR_DECOMPRESS_LIMIT: &str = "dicom: decompression resource limit exceeded";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecompressLimits {
    pub max_frames: i64,
    pub max_pixels: i64,
    pub max_native_bytes: i64,
    pub max_input_bytes: i64,
    pub max_expansion_ratio: i64,
}
pub fn default_decompress_limits() -> DecompressLimits {
    DecompressLimits {
        max_frames: 100_000,
        max_pixels: 256 * 1024 * 1024,
        max_native_bytes: 512 << 20,
        max_input_bytes: 512 << 20,
        max_expansion_ratio: 1024,
    }
}
#[derive(Debug, Clone, Default)]
pub struct DecompressOptions {
    pub registry: Option<DecoderRegistry>,
    pub target_transfer_syntax: String,
    pub limits: DecompressLimits,
}
pub(crate) fn normalized(l: DecompressLimits) -> Result<DecompressLimits, Error> {
    if l.max_frames < 0
        || l.max_pixels < 0
        || l.max_native_bytes < 0
        || l.max_input_bytes < 0
        || l.max_expansion_ratio < 0
    {
        return Err(Error::new(
            ErrorKind::InvalidDecompressLimits,
            "dicom: invalid decompression limits",
        ));
    }
    let d = default_decompress_limits();
    let pick = |v: i64, d: i64| if v == 0 { d } else { v };
    Ok(DecompressLimits {
        max_frames: pick(l.max_frames, d.max_frames),
        max_pixels: pick(l.max_pixels, d.max_pixels),
        max_native_bytes: pick(l.max_native_bytes, d.max_native_bytes),
        max_input_bytes: pick(l.max_input_bytes, d.max_input_bytes),
        max_expansion_ratio: pick(l.max_expansion_ratio, d.max_expansion_ratio),
    })
}
pub(crate) fn transcode_decompress_limits(l: &TranscodeLimits) -> DecompressLimits {
    DecompressLimits {
        max_frames: l.max_frames,
        max_pixels: l.max_pixels as i64,
        max_input_bytes: l.max_input_bytes,
        max_native_bytes: l.max_output_bytes,
        max_expansion_ratio: l.max_expansion_ratio,
    }
}
pub(crate) fn limit_error(limit: &str, actual: i64, maximum: i64) -> Error {
    Error::new(
        ErrorKind::DecompressResourceLimit,
        format!("{ERR_DECOMPRESS_LIMIT}: {limit} actual={actual} maximum={maximum}"),
    )
}
pub(crate) fn codec_registry_nil(uid: &str) -> Error {
    let mut text = String::from("dicom: pixel data codec registry is nil");
    if !uid.is_empty() {
        text.push_str(&format!(": transfer syntax {}", textfmt::quote(uid)));
        if let Some(s) = transfer::default_registry().get(uid) {
            if !s.name.is_empty() {
                text.push_str(&format!(" ({})", s.name));
            }
            let hint = codec_registration_hint(&s);
            if !hint.is_empty() {
                text.push_str(&format!("; {hint}"));
            }
        }
    }
    Error::new(ErrorKind::CodecRegistryNil, text)
}
pub(crate) fn codec_registration_hint(s: &Syntax) -> &'static str {
    match s.uid.as_ref() {
        "1.2.840.10008.1.2.4.50" => {
            return "decode with a registry that has the JPEG Baseline codec (DecoderRegistry::Builtin, feature `jpeg`)";
        }
        "1.2.840.10008.1.2.4.51" => {
            return "decode with a registry that has the JPEG Extended Process 2/4 codec (DecoderRegistry::Builtin, feature `jpeg`)";
        }
        "1.2.840.10008.1.2.4.57" | "1.2.840.10008.1.2.4.70" => {
            return "decode with a registry that has the JPEG Lossless codec (DecoderRegistry::Builtin, feature `jpeg-lossless`)";
        }
        "1.2.840.10008.1.2.5" => {
            return "decode with a registry that has the RLE Lossless codec (DecoderRegistry::Builtin)";
        }
        "1.2.840.10008.1.2.8.1" => {
            return "decode with a registry that has the Deflated Image Frame Compression codec (DecoderRegistry::Builtin)";
        }
        "1.2.840.10008.1.2.4.80" => {
            return "decode with a registry that has the built-in JPEG-LS Lossless codec (DecoderRegistry::Builtin, feature `jpeg-ls`)";
        }
        "1.2.840.10008.1.2.4.81" => {
            return "decode with a registry that has the qualified unsigned JPEG-LS Near-Lossless decoder (DecoderRegistry::Builtin, feature `jpeg-ls`)";
        }
        _ => {}
    }
    if transfer::is_jpeg_2000_transfer_syntax(&s.uid) {
        return "JPEG 2000 / HTJ2K transfer syntax is recognized, but there is no default decoder adapter; metadata and encapsulated Pixel Data can be preserved, or decode with a registry that has the JPEG 2000 codecs (DecoderRegistry::Builtin)";
    }
    if transfer::is_jpeg_xl_transfer_syntax(&s.uid) {
        return "JPEG XL transfer syntax is recognized, but there is no default decoder adapter; metadata and encapsulated Pixel Data can be preserved, or decode frames with DecoderRegistry::BuiltinWithJpegXl or a Codec added with CodecRegistry::register_codec";
    }
    if transfer::is_video_transfer_syntax(&s.uid) {
        return "video transfer syntax is recognized but video frames are not decoded or played; preserve encoded Pixel Data or use an external media pipeline";
    }
    if matches!(
        s.uid.as_ref(),
        "1.2.840.10008.1.2.4.94"
            | "1.2.840.10008.1.2.4.95"
            | "1.2.840.10008.1.2.4.204"
            | "1.2.840.10008.1.2.4.205"
    ) {
        return "streaming/reference transfer syntax is recognized but external pixel streams are not fetched, inflated, or resolved; preserve metadata/reference values or use an external streaming pipeline";
    }
    if matches!(
        s.uid.as_ref(),
        "1.2.840.10008.1.2.7.1" | "1.2.840.10008.1.2.7.2" | "1.2.840.10008.1.2.7.3"
    ) {
        return "media transfer syntax is recognized but SMPTE ST 2110 video or audio streams are not decoded or played; use an external media pipeline";
    }
    if s.encapsulated {
        return "register a compatible Codec with CodecRegistry::register_codec";
    }
    ""
}
pub(crate) fn checked_product(values: &[i64]) -> Option<i64> {
    values
        .iter()
        .try_fold(1i64, |p, &v| if v <= 0 { None } else { p.checked_mul(v) })
}
fn bytes_per_sample(m: &Metadata) -> i64 {
    i64::from(m.bits_allocated.wrapping_add(7) / 8)
}
fn is_subsampled_422(photometric: &str) -> bool {
    matches!(
        normalized_photometric(photometric).as_str(),
        "YBR_FULL_422" | "YBR_PARTIAL_422"
    )
}
fn estimated_native_bytes(m: &Metadata, source: &Syntax) -> Option<i64> {
    if m.bits_allocated == 1 && !source.encapsulated {
        let n = pd::native_packed_byte_count(m);
        return if n > 0 { Some(n) } else { None };
    }
    let native = total_size(m);
    if native <= 0 {
        return None;
    }
    if !is_subsampled_422(&m.photometric_interpretation)
        || !matches!(
            transfer::normalize_uid(&source.uid),
            "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51"
        )
        || m.bits_allocated < 8
    {
        return Some(native);
    }
    let expanded = checked_product(&[
        i64::from(m.rows),
        i64::from(m.columns),
        i64::from(m.samples_per_pixel),
        bytes_per_sample(m),
        m.number_of_frames,
    ])?;
    Some(expanded.max(native))
}
pub(crate) fn check_admission(
    l: &DecompressLimits,
    m: &Metadata,
    source: &Syntax,
    input: i64,
) -> Result<(), Error> {
    if input < 0 {
        return Err(limit_error("MaxInputBytes", i64::MAX, l.max_input_bytes));
    }
    if input > l.max_input_bytes {
        return Err(limit_error("MaxInputBytes", input, l.max_input_bytes));
    }
    if m.rows == 0
        || m.columns == 0
        || m.samples_per_pixel == 0
        || m.bits_allocated == 0
        || m.number_of_frames <= 0
    {
        return Err(Error::pixel(
            PixelErrorKind::InvalidMetadata,
            format_args!(
                "rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
                m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
            ),
        ));
    }
    if m.number_of_frames > l.max_frames {
        return Err(limit_error("MaxFrames", m.number_of_frames, l.max_frames));
    }
    let Some(pixels) =
        checked_product(&[i64::from(m.rows), i64::from(m.columns), m.number_of_frames])
    else {
        return Err(limit_error("MaxPixels", i64::MAX, l.max_pixels));
    };
    if pixels > l.max_pixels {
        return Err(limit_error("MaxPixels", pixels, l.max_pixels));
    }
    let native = match estimated_native_bytes(m, source) {
        Some(n) if n > 0 => n,
        _ => return Err(limit_error("MaxNativeBytes", i64::MAX, l.max_native_bytes)),
    };
    if native > l.max_native_bytes {
        return Err(limit_error("MaxNativeBytes", native, l.max_native_bytes));
    }
    if source.encapsulated && source.uid != UID_ENCAPSULATED_UNCOMPRESSED {
        let ratio = if input <= 0 {
            i64::MAX
        } else {
            native
                .checked_div(input)
                .unwrap_or(i64::MAX)
                .saturating_add(i64::from(native.checked_rem(input).unwrap_or(0) != 0))
        };
        if ratio > l.max_expansion_ratio {
            return Err(limit_error(
                "MaxExpansionRatio",
                ratio,
                l.max_expansion_ratio,
            ));
        }
    }
    Ok(())
}
fn input_bytes(pixel: &PixelData<'_>) -> Option<i64> {
    match pixel {
        PixelData::Native(raw) => Some(raw.len() as i64),
        PixelData::Encapsulated {
            offset_table,
            fragments,
        } => {
            let mut total = offset_table.len() as i64;
            for f in fragments.iter() {
                total = total.checked_add(f.len() as i64)?;
            }
            Some(total)
        }
    }
}
pub fn decompress_data_set(
    dataset: &Object,
    source: &str,
    opts: &DecompressOptions,
) -> Result<(Object, String), Error> {
    let limits = normalized(opts.limits)?;
    let source = canonical_source(source)?;
    let target = target_syntax(&opts.target_transfer_syntax)?;
    let (output, _) = decompress(dataset, &source, opts.registry, &limits)?;
    Ok((output, target))
}
pub fn decompress_file(file: &File, opts: &DecompressOptions) -> Result<File, Error> {
    if file.partial_read.is_some() {
        return Err(Error::new(ErrorKind::PartialFile, ERR_PARTIAL_FILE));
    }
    let source = file_source_syntax(file)?;
    let (dataset, target) = decompress_data_set(&file.dataset, &source.uid, opts)?;
    let mut meta = file.meta.elements().to_vec();
    replace_element(
        &mut meta,
        strings_element(Tag::new(0x0002, 0x0010), Vr::UI, vec![target.clone()]),
    );
    Ok(File {
        preamble: file.preamble.clone(),
        meta: Object::from_elements(meta),
        dataset,
        transfer_syntax: crate::object::transfer::lookup(&target)
            .unwrap_or(crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN),
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
fn target_syntax(uid: &str) -> Result<String, Error> {
    let uid = transfer::normalize_uid(uid);
    if uid.is_empty() {
        return Ok(UID_EXPLICIT_LE.into());
    }
    let Some(target) = transfer::default_registry().get(uid) else {
        return Err(Error::new(
            ErrorKind::UnknownTransferSyntax,
            format!("dicom: unknown transfer syntax: {}", textfmt::quote(uid)),
        ));
    };
    if target.uid == UID_EXPLICIT_LE || target.uid == UID_IMPLICIT_LE {
        return Ok(target.uid.to_string());
    }
    Err(Error::new(
        ErrorKind::UnsupportedDecompression,
        format!(
            "dicom: unsupported still-image decompression: target transfer syntax {} is not a supported native little-endian syntax",
            textfmt::quote(&target.uid)
        ),
    ))
}
pub(crate) fn decompress_for_transcode(
    dataset: &Object,
    source: &Syntax,
    registry: Option<DecoderRegistry>,
    limits: DecompressLimits,
) -> Result<(Object, Vec<String>), Error> {
    let limits = normalized(limits)?;
    decompress(dataset, source, registry, &limits)
}
fn decompress(
    dataset: &Object,
    source: &Syntax,
    registry: Option<DecoderRegistry>,
    limits: &DecompressLimits,
) -> Result<(Object, Vec<String>), Error> {
    DecoderRegistry::refuse(registry, &source.uid)?;
    if !source.supported && !source.requires_codec() {
        return Err(Error::new(
            ErrorKind::UnsupportedDecompression,
            format!(
                "dicom: unsupported still-image decompression: source transfer syntax {} is not supported",
                textfmt::quote(&source.uid)
            ),
        ));
    }
    let pixel = extract_view(dataset)?;
    let lossy = detect_lossy_state_crate(dataset, &source.uid);
    let Some(input) = input_bytes(&pixel) else {
        return Err(limit_error(
            "MaxInputBytes",
            i64::MAX,
            limits.max_input_bytes,
        ));
    };
    let metadata = match extract_metadata(dataset) {
        Ok(m) => m,
        Err(e) => {
            if input > limits.max_input_bytes {
                return Err(limit_error("MaxInputBytes", input, limits.max_input_bytes));
            }
            return Err(e.into());
        }
    };
    check_admission(limits, &metadata, source, input)?;
    let encapsulated = matches!(pixel, PixelData::Encapsulated { .. });
    if source.encapsulated && !encapsulated {
        return Err(Error::pixel(
            PixelErrorKind::IncompatiblePixelData,
            format_args!(
                "transfer syntax {} expects encapsulated pixel data",
                textfmt::quote(&source.uid)
            ),
        ));
    }
    if !source.encapsulated && encapsulated {
        return Err(Error::pixel(
            PixelErrorKind::IncompatiblePixelData,
            format_args!(
                "transfer syntax {} expects native pixel data",
                textfmt::quote(&source.uid)
            ),
        ));
    }
    let frames =
        registry
            .unwrap_or(DecoderRegistry::Builtin)
            .decode_frames(&source.uid, &pixel, dataset)?;
    let diagnostics: Vec<String> = frames.diagnostics.iter().map(|d| d.to_string()).collect();
    let out_meta = output_metadata(&metadata, source, &frames);
    let frames = little_endian_frames(&out_meta, source, frames)?;
    let raw = native_frame_bytes(&out_meta, &frames)?;
    let raw_len = raw.len() as i64;
    let mut output = with_native_pixel_data(dataset, &out_meta, raw, source)?;
    stamp_lossy_state(&mut output, &lossy, raw_len, input);
    Ok((output, diagnostics))
}
pub(crate) fn native_frame_bytes(m: &Metadata, frames: &Frames) -> Result<Vec<u8>, Error> {
    if frames.rows != usize::from(m.rows) || frames.columns != usize::from(m.columns) {
        return Err(Error::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!(
                "decoded frame geometry={}x{} metadata={}x{}",
                frames.rows, frames.columns, m.rows, m.columns
            ),
        ));
    }
    if frames.data.len() as i64 != m.number_of_frames {
        return Err(Error::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!(
                "decoded frame count={} metadata NumberOfFrames={}",
                frames.data.len(),
                m.number_of_frames
            ),
        ));
    }
    let frame = frame_size(m);
    let mut total = total_size(m);
    let mut frame_bits = 0i64;
    if m.bits_allocated == 1 {
        frame_bits = pd::packed_frame_bits(m);
        total = pd::native_packed_byte_count(m);
    }
    if frame <= 0 || total <= 0 {
        return Err(Error::pixel(
            PixelErrorKind::InvalidMetadata,
            format_args!(
                "rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
                m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
            ),
        ));
    }
    let mut raw = vec![0u8; total as usize];
    for (i, data) in frames.data.iter().enumerate() {
        if data.len() as i64 != frame {
            return Err(Error::pixel(
                PixelErrorKind::PixelDataSizeMismatch,
                format_args!("decoded frame {i} has {} bytes, want {frame}", data.len()),
            ));
        }
        if frame_bits % 8 != 0 {
            pd::copy_packed_bits(
                &mut raw,
                (i as i64).saturating_mul(frame_bits),
                data,
                0,
                frame_bits,
            );
        } else {
            let start = i.saturating_mul(frame as usize);
            let Some(dst) = raw.get_mut(start..start.saturating_add(data.len())) else {
                return Err(Error::pixel(
                    PixelErrorKind::PixelDataSizeMismatch,
                    format_args!("decoded frame {i} lies past the frames' bytes"),
                ));
            };
            dst.copy_from_slice(data);
        }
    }
    Ok(raw)
}
pub(crate) fn with_native_pixel_data(
    dataset: &Object,
    m: &Metadata,
    raw: Vec<u8>,
    source: &Syntax,
) -> Result<Object, Error> {
    let mut elements = dataset.elements().to_vec();
    if source.big_endian {
        swap_elements(&mut elements, true)?;
    }
    if source.encapsulated {
        remove_elements(
            &mut elements,
            &[
                TAG_EXTENDED_OFFSET_TABLE,
                TAG_EXTENDED_OFFSET_TABLE_LENGTHS,
                TAG_ENCAPSULATED_TOTAL_LENGTH,
            ],
        );
    }
    let vr = if m.bits_allocated > 8 { Vr::OW } else { Vr::OB };
    replace_element(&mut elements, raw_element(TAG_PIXEL_DATA, vr, raw));
    replace_element(
        &mut elements,
        strings_element(
            TAG_PHOTOMETRIC,
            Vr::CS,
            vec![m.photometric_interpretation.clone()],
        ),
    );
    replace_element(
        &mut elements,
        uint16_element(TAG_SAMPLES_PER_PIXEL, m.samples_per_pixel),
    );
    if m.planar_configuration_present {
        replace_element(
            &mut elements,
            uint16_element(TAG_PLANAR_CONFIGURATION, m.planar_configuration),
        );
    }
    Ok(object_from(elements, false))
}
pub(crate) fn little_endian_frames(
    m: &Metadata,
    source: &Syntax,
    frames: Frames,
) -> Result<Frames, Error> {
    let width = bytes_per_sample(m) as usize;
    if !source.big_endian || width <= 1 {
        return Ok(frames);
    }
    let mut out = Frames {
        rows: frames.rows,
        columns: frames.columns,
        ..Frames::default()
    };
    for (i, f) in frames.data.iter().enumerate() {
        if f.len().checked_rem(width).unwrap_or(0) != 0 {
            return Err(Error::pixel(
                PixelErrorKind::PixelDataSizeMismatch,
                format_args!(
                    "decoded frame {i} has {} bytes, not a multiple of {width}-byte samples",
                    f.len()
                ),
            ));
        }
        out.data
            .push(crate::valueencode::reverse_fixed_width(f, width));
    }
    Ok(out)
}
pub(crate) fn output_metadata(m: &Metadata, source: &Syntax, frames: &Frames) -> Metadata {
    let mut m = m.clone();
    if m.samples_per_pixel != 3 {
        return m;
    }
    if !frames.photometric_interpretation.is_empty() {
        m.photometric_interpretation = frames.photometric_interpretation.clone();
        m.planar_configuration = 0;
        m.planar_configuration_present = true;
        return m;
    }
    match transfer::normalize_uid(&source.uid) {
        "1.2.840.10008.1.2.5" => {
            m.planar_configuration = 0;
            m.planar_configuration_present = true;
        }
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => {
            match normalized_photometric(&m.photometric_interpretation).as_str() {
                "YBR_FULL" => {
                    m.photometric_interpretation = "RGB".into();
                    m.planar_configuration = 0;
                    m.planar_configuration_present = true;
                }
                "YBR_FULL_422" | "YBR_PARTIAL_422" if expanded_rgb(&m, frames) => {
                    m.photometric_interpretation = "RGB".into();
                    m.planar_configuration = 0;
                    m.planar_configuration_present = true;
                }
                _ => {}
            }
        }
        _ => {}
    }
    m
}
fn expanded_rgb(m: &Metadata, frames: &Frames) -> bool {
    if m.bits_allocated < 8 || frames.data.is_empty() {
        return false;
    }
    let size = i64::from(m.rows)
        .saturating_mul(i64::from(m.columns))
        .saturating_mul(3)
        .saturating_mul(bytes_per_sample(m));
    frames.data.iter().all(|f| f.len() as i64 == size)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LossyState {
    pub lossy: bool,
    pub method: &'static str,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LossyEvidence {
    #[default]
    None,
    Attribute,
    TransferSyntax,
    Codestream,
}
impl LossyEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            LossyEvidence::None => "",
            LossyEvidence::Attribute => "attribute",
            LossyEvidence::TransferSyntax => "transfer-syntax",
            LossyEvidence::Codestream => "codestream",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LossyStateReport {
    pub lossy: bool,
    pub evidence: LossyEvidence,
    pub method: String,
}
pub fn detect_lossy_state(file: &File) -> LossyStateReport {
    detect_data_set_lossy_state(&file.dataset, file.transfer_syntax.uid)
}
pub fn detect_data_set_lossy_state(obj: &Object, uid: &str) -> LossyStateReport {
    let state = detect_lossy_state_crate(obj, uid);
    if !state.lossy {
        return LossyStateReport::default();
    }
    let evidence = if obj.get_string(TAG_LOSSY).is_some_and(|v| v.trim() == "01") {
        LossyEvidence::Attribute
    } else if lossy_only_method(transfer::normalize_uid(uid)).is_some() {
        LossyEvidence::TransferSyntax
    } else {
        LossyEvidence::Codestream
    };
    LossyStateReport {
        lossy: true,
        evidence,
        method: state.method.to_string(),
    }
}
pub(crate) fn detect_lossy_state_crate(obj: &Object, uid: &str) -> LossyState {
    if obj.get_string(TAG_LOSSY).is_some_and(|v| v.trim() == "01") {
        return LossyState {
            lossy: true,
            method: "",
        };
    }
    let uid = transfer::normalize_uid(uid);
    if let Some(method) = lossy_only_method(uid) {
        if matches!(uid, "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51")
            && let Some(false) = codestream_lossy(obj, uid)
        {
            return LossyState::default();
        }
        return LossyState {
            lossy: true,
            method,
        };
    }
    if let Some(true) = codestream_lossy(obj, uid) {
        return LossyState {
            lossy: true,
            method: codestream_method(uid),
        };
    }
    LossyState::default()
}
fn lossy_only_method(uid: &str) -> Option<&'static str> {
    match uid {
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" | "1.2.840.10008.1.2.4.111" => {
            Some("ISO_10918_1")
        }
        "1.2.840.10008.1.2.4.100"
        | "1.2.840.10008.1.2.4.100.1"
        | "1.2.840.10008.1.2.4.101"
        | "1.2.840.10008.1.2.4.101.1" => Some("ISO_13818_2"),
        "1.2.840.10008.1.2.4.107" | "1.2.840.10008.1.2.4.108" => Some("ISO_23008_2"),
        u if transfer::is_video_transfer_syntax(u) => Some("ISO_14496_10"),
        _ => None,
    }
}
fn codestream_method(uid: &str) -> &'static str {
    if transfer::is_jpeg_ls_transfer_syntax(uid) {
        "ISO_14495_1"
    } else if matches!(
        uid,
        "1.2.840.10008.1.2.4.201" | "1.2.840.10008.1.2.4.202" | "1.2.840.10008.1.2.4.203"
    ) {
        "ISO_15444_15"
    } else if transfer::is_jpeg_2000_transfer_syntax(uid) {
        "ISO_15444_1"
    } else if transfer::is_jpeg_xl_transfer_syntax(uid) {
        "ISO_18181_1"
    } else {
        "ISO_10918_1"
    }
}
fn codestream_lossy(obj: &Object, uid: &str) -> Option<bool> {
    let e = obj.get(TAG_PIXEL_DATA)?;
    let Value::Fragments { fragments, .. } = &e.value else {
        return None;
    };
    let first = fragments.first()?;
    let data = first.get(..first.len().min(1 << 20)).unwrap_or_default();
    match uid {
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.57"
        | "1.2.840.10008.1.2.4.70" => {
            let marker = jpeg_frame_marker(data)?;
            let lossless = marker == 0xc3 || marker == 0xc7;
            Some(!lossless && marker != 0xcb && marker != 0xcf)
        }
        u if transfer::is_jpeg_ls_transfer_syntax(u) => jpeg_ls_near_lossy(data),
        u if transfer::is_jpeg_2000_transfer_syntax(u) => {
            let data = jp2_codestream(data).unwrap_or(data);
            jpeg2000_irreversible(data)
        }
        _ => None,
    }
}
fn be16(data: &[u8], at: usize) -> Option<usize> {
    let &pair = data.get(at..)?.first_chunk::<2>()?;
    Some(usize::from(u16::from_be_bytes(pair)))
}
fn jpeg_frame_marker(data: &[u8]) -> Option<u8> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut i = 2usize;
    while i.saturating_add(4) <= data.len() {
        if data.get(i) != Some(&0xff) {
            return None;
        }
        let marker = *data.get(i.saturating_add(1))?;
        if marker == 0xff {
            i = i.saturating_add(1);
            continue;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            i = i.saturating_add(2);
            continue;
        }
        if marker == 0xda || marker == 0xd9 {
            return None;
        }
        let length = be16(data, i.saturating_add(2))?;
        if length < 2 || i.saturating_add(2).saturating_add(length) > data.len() {
            return None;
        }
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            if length < 8 {
                return None;
            }
            return Some(marker);
        }
        i = i.saturating_add(length.saturating_add(2));
    }
    None
}
fn jpeg_ls_near_lossy(data: &[u8]) -> Option<bool> {
    if data.len() < 4 || !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut known = false;
    let mut i = 2usize;
    while i.saturating_add(5) < data.len() {
        if data.get(i..i.saturating_add(2)) != Some(&[0xff, 0xda]) {
            i = i.saturating_add(1);
            continue;
        }
        let length = be16(data, i.saturating_add(2))?;
        let end = i.saturating_add(2).saturating_add(length);
        if length < 6 || end > data.len() {
            return None;
        }
        let components = usize::from(*data.get(i.saturating_add(4))?);
        let near = components
            .saturating_mul(2)
            .saturating_add(i)
            .saturating_add(5);
        if components < 1 || near >= end {
            return None;
        }
        if data.get(near) != Some(&0) {
            return Some(true);
        }
        known = true;
        i = i.saturating_add(length.saturating_add(1));
        i = i.saturating_add(1);
    }
    if known { Some(false) } else { None }
}
fn jpeg2000_irreversible(data: &[u8]) -> Option<bool> {
    if data.len() < 4 || !data.starts_with(&[0xff, 0x4f]) {
        return None;
    }
    let mut components = 0usize;
    let (mut lossy, mut known) = (false, false);
    let mut i = 2usize;
    while i.saturating_add(4) <= data.len() {
        if data.get(i) != Some(&0xff) {
            return None;
        }
        let marker = *data.get(i.saturating_add(1))?;
        if marker == 0x90 || marker == 0x93 || marker == 0xd9 {
            return if known { Some(lossy) } else { None };
        }
        let length = be16(data, i.saturating_add(2))?;
        let end = i.saturating_add(2).saturating_add(length);
        if length < 2 || end > data.len() {
            return None;
        }
        let segment = data.get(i.saturating_add(4)..end)?;
        let offset = if components >= 257 { 2usize } else { 1 };
        match marker {
            0x51 => {
                if segment.len() >= 36 {
                    components = be16(segment, 34)?;
                }
            }
            0x52 => {
                known = true;
                lossy = lossy || *segment.get(9)? == 0;
            }
            0x53 => {
                lossy = lossy || *segment.get(offset.saturating_add(5))? == 0;
            }
            0x5c => {
                known = true;
                lossy = lossy || segment.first()? & 0x1f != 0;
            }
            0x5d => {
                lossy = lossy || segment.get(offset)? & 0x1f != 0;
            }
            _ => {}
        }
        i = i.saturating_add(length.saturating_add(2));
    }
    None
}
const JP2_SIGNATURE: [u8; 12] = [
    0, 0, 0, 0x0c, b'j', b'P', b' ', b' ', 0x0d, 0x0a, 0x87, 0x0a,
];
pub(crate) fn jp2_codestream(data: &[u8]) -> Option<&[u8]> {
    if !data.starts_with(&JP2_SIGNATURE) {
        return None;
    }
    let mut offset = 0usize;
    while offset.saturating_add(8) <= data.len() {
        let rest = data.get(offset..)?;
        let &[l0, l1, l2, l3, k0, k1, k2, k3] = rest.first_chunk::<8>()?;
        let mut length = u64::from(u32::from_be_bytes([l0, l1, l2, l3]));
        let kind = [k0, k1, k2, k3];
        let mut header = 8u64;
        match length {
            0 => length = rest.len() as u64,
            1 => {
                let &b = rest.get(8..)?.first_chunk::<8>()?;
                length = u64::from_be_bytes(b);
                header = 16;
            }
            _ => {}
        }
        if length < header || length > rest.len() as u64 {
            return None;
        }
        if &kind == b"jp2c" {
            return rest.get(header as usize..length as usize);
        }
        offset = offset.saturating_add(length as usize);
    }
    None
}
pub(crate) fn stamp_lossy_state(obj: &mut Object, state: &LossyState, native: i64, input: i64) {
    if !state.lossy {
        return;
    }
    obj.put(strings_element(TAG_LOSSY, Vr::CS, vec!["01".into()]));
    if state.method.is_empty()
        || native <= 0
        || input <= 0
        || obj.has(TAG_LOSSY_METHOD)
        || obj.has(TAG_LOSSY_RATIO)
    {
        return;
    }
    let ratio = crate::numtext::format_float(native as f64 / input as f64, 'g', 8);
    obj.put(strings_element(TAG_LOSSY_RATIO, Vr::DS, vec![ratio]));
    obj.put(strings_element(
        TAG_LOSSY_METHOD,
        Vr::CS,
        vec![state.method.to_string()],
    ));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jpeg_ls_near_scan() {
        let data = [
            0xff, 0xd8, 0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x02, 0x00, 0x00,
        ];
        assert_eq!(jpeg_ls_near_lossy(&data), Some(true));
        assert_eq!(jpeg_ls_near_lossy(&[0xff, 0xd8, 0, 0]), None);
    }
    #[test]
    fn limits_text() {
        assert_eq!(
            limit_error("MaxFrames", 3, 2).to_string(),
            "dicom: decompression resource limit exceeded: MaxFrames actual=3 maximum=2"
        );
    }
}
