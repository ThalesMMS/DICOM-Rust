use super::super::super::Metadata;
use super::super::super::encapsulated;
use super::super::super::pixeldata::{
    CodecRecovery, Frames, PixelData, PixelErrorKind, extract_metadata, extract_view, frame_size,
    normalized_photometric, total_size,
};
use super::super::{
    DecoderRegistry, ERR_PARTIAL_FILE, Error, ErrorKind, TAG_BITS_ALLOCATED,
    TAG_EXTENDED_OFFSET_TABLE, TAG_EXTENDED_OFFSET_TABLE_LENGTHS, TAG_PHOTOMETRIC, TAG_PIXEL_DATA,
    TAG_PLANAR_CONFIGURATION, TAG_SAMPLES_PER_PIXEL, UID_ENCAPSULATED_UNCOMPRESSED,
    UID_EXPLICIT_LE, canonical_source, fragments_element, object_from, raw_element,
    replace_element, strings_element,
};
use super::{
    DecompressLimits, DecompressOptions, bytes_per_sample, check_admission,
    detect_lossy_state_crate, input_bytes, is_subsampled_422, jp2_codestream, limit_error,
    little_endian_frames, native_frame_bytes, normalized, output_metadata, stamp_lossy_state,
    target_syntax, with_native_pixel_data,
};
use crate::display::{self, LutOptions, PaletteColorLut};
use crate::object::parser::IncompletePixelData;
use crate::object::{Element, File, Object, Tag, Value, Vr};
use crate::textfmt;
use crate::transfer::{self, Syntax};
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_IMAGE_DIMENSIONS: Tag = Tag::new(0x0028, 0x0005);
const TAG_PLANES: Tag = Tag::new(0x0028, 0x0012);
const TAG_RECOGNITION_CODE: Tag = Tag::new(0x0008, 0x0010);
const TAG_SOP_CLASS: Tag = Tag::new(0x0008, 0x0016);
const ERR_UNSUPPORTED_DECOMPRESSION: &str = "dicom: unsupported still-image decompression";
pub const DIAGNOSTIC_BIT_COUNT_MASKS: &str = "bit-count-masks-repaired";
pub const DIAGNOSTIC_DEFINED_LENGTH_FRAGMENTS: &str = "encapsulated-defined-length";
#[doc(hidden)]
pub const DIAGNOSTIC_NATIVE_UNDER_ENCAPSULATED: &str =
    "native-pixel-data-under-encapsulated-syntax";
pub const DIAGNOSTIC_JP2_CONTAINER_UNWRAPPED: &str = "jp2-container-unwrapped";
pub const DIAGNOSTIC_RECONCILED_WITH_CODESTREAM: &str = "metadata-reconciled-with-codestream";
pub const DIAGNOSTIC_DEFAULT_SAMPLES_PER_PIXEL: &str = "default-samples-per-pixel";
#[doc(hidden)]
pub const DIAGNOSTIC_DEFAULT_PHOTOMETRIC: &str = "default-photometric-interpretation";
pub const DIAGNOSTIC_NUMBER_OF_FRAMES_FROM_PLANES: &str = "number-of-frames-from-planes";
pub const DIAGNOSTIC_UNPACKED_12_BIT: &str = "unpacked-12-bit";
pub const DIAGNOSTIC_LIBIDO_DIMENSIONS: &str =
    crate::object::legacy_upgrade::LEGACY_DIAGNOSTIC_LIBIDO_DIMENSIONS;
pub const DIAGNOSTIC_LIBIDO_RGB24: &str =
    crate::object::legacy_upgrade::LEGACY_DIAGNOSTIC_LIBIDO_RGB24;
pub const DIAGNOSTIC_INCOMPLETE_PIXEL_DATA: &str = "incomplete-pixel-data-complete-prefix";
pub const DIAGNOSTIC_RLE_REVERSED_BYTE_SEGMENTS: &str = "rle-reversed-byte-segments";
pub const ERR_NO_COMPLETE_FRAMES: &str = "dicom: incomplete Pixel Data has no complete frames";
pub const DIAGNOSTIC_PHOTOMETRIC_FROM_SAMPLES: &str = "photometric-from-samples";
pub const DIAGNOSTIC_INVALID_PHOTOMETRIC_REPLACED: &str = "invalid-photometric-replaced";
pub const DIAGNOSTIC_SAMPLES_FROM_PHOTOMETRIC: &str = "samples-per-pixel-from-photometric";
pub const DIAGNOSTIC_IGNORED_PLANAR_CONFIGURATION: &str = "ignored-planar-configuration";
pub const DIAGNOSTIC_DEFAULT_PLANAR_CONFIGURATION: &str = "default-planar-configuration";
pub const DIAGNOSTIC_NATIVE_PLANAR_OVERRIDE: &str = "native-planar-layout-overridden";
pub const DIAGNOSTIC_NATIVE_WHOLE_IMAGE_PLANAR: &str = "native-whole-image-planar-reordered";
pub const DIAGNOSTIC_NATIVE_OW_BYTE_SWAP: &str = "native-ow-byte-swap";
pub const DIAGNOSTIC_WRONG_PALETTE_ATTRIBUTE_TAGS: &str = "wrong-palette-attribute-tags";
pub const DIAGNOSTIC_RETIRED_ARGB_TO_RGB: &str = "retired-argb-to-rgb";
pub const DIAGNOSTIC_RETIRED_CMYK_TO_RGB: &str = "retired-cmyk-additive-to-rgb";
pub const DIAGNOSTIC_RETIRED_HSV_TO_RGB: &str = "retired-hsv-full-range-to-rgb";
pub type NativePlanarRecovery = u8;
pub const NATIVE_PLANAR_DECLARED: NativePlanarRecovery = 0;
pub const NATIVE_PLANAR_INTERLEAVED: NativePlanarRecovery = 1;
pub const NATIVE_PLANAR_BY_FRAME: NativePlanarRecovery = 2;
pub const NATIVE_PLANAR_WHOLE_IMAGE: NativePlanarRecovery = 3;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeRecovery {
    pub native_planar_layout: NativePlanarRecovery,
    pub native_ow_byte_swap: bool,
    pub rle_reversed_byte_segments: bool,
    pub wrong_palette_attribute_tags: bool,
    pub convert_retired_color_models: bool,
    pub incomplete_data: bool,
    pub ignore_inconsistent_basic_offset_table: bool,
    pub default_image_pixel_module: bool,
    pub repair_photometric_interpretation: bool,
    pub unpack_12_bit_packed: bool,
    pub jpeg_lossless_encoder_quirks: bool,
    pub jpeg_lossless_predictor6_overflow: bool,
    pub jpeg_sequential_scan_quirks: bool,
    pub jpegls_jai_thresholds: bool,
    pub jpegls_hp_color_transforms: bool,
    pub rle_segment_overrun: bool,
    pub reconcile_with_codestream: bool,
    pub unwrap_jp2_container: bool,
    pub native_pixel_data_under_encapsulated: bool,
    pub defined_length_fragments: bool,
    pub repair_bit_count_masks: bool,
}
impl DecodeRecovery {
    pub(crate) fn codec_recovery(&self) -> CodecRecovery {
        CodecRecovery {
            ignore_inconsistent_basic_offset_table: self.ignore_inconsistent_basic_offset_table,
            jpeg_lossless_encoder_quirks: self.jpeg_lossless_encoder_quirks,
            jpeg_lossless_predictor6_overflow: self.jpeg_lossless_predictor6_overflow,
            rle_segment_overrun: self.rle_segment_overrun,
            rle_reversed_byte_segments: self.rle_reversed_byte_segments,
            jpeg_sequential_scan_quirks: self.jpeg_sequential_scan_quirks,
            jpegls_jai_thresholds: self.jpegls_jai_thresholds,
            jpegls_hp_color_transforms: self.jpegls_hp_color_transforms,
            jpeg_color: Default::default(),
        }
    }
    pub(crate) fn requires_legacy_pixel_repair(&self) -> bool {
        self.native_planar_layout != NATIVE_PLANAR_DECLARED
            || self.native_ow_byte_swap
            || self.rle_reversed_byte_segments
            || self.wrong_palette_attribute_tags
            || self.convert_retired_color_models
    }
}
pub fn detectable_decode_recovery() -> DecodeRecovery {
    DecodeRecovery {
        ignore_inconsistent_basic_offset_table: true,
        default_image_pixel_module: true,
        unpack_12_bit_packed: true,
        jpeg_lossless_encoder_quirks: true,
        jpeg_sequential_scan_quirks: true,
        rle_segment_overrun: true,
        reconcile_with_codestream: true,
        unwrap_jp2_container: true,
        native_pixel_data_under_encapsulated: true,
        defined_length_fragments: true,
        repair_bit_count_masks: true,
        ..DecodeRecovery::default()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataReconciliation {
    pub attribute: &'static str,
    pub tag: Tag,
    pub declared: String,
    pub used: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecompressReport {
    pub incomplete: Option<IncompleteFrameRecovery>,
    pub diagnostics: Vec<String>,
    pub reconciliations: Vec<MetadataReconciliation>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncompleteFrameRecovery {
    pub declared_bytes: i64,
    pub available_bytes: i64,
    pub kind: String,
    pub value_offset: i64,
    pub declared_frames: i64,
    pub retained_frames: i64,
    pub discarded_fragments: i64,
    pub discarded_bytes: i64,
}
pub type JpegColorConversion = crate::codec::common::ColorConversion;
fn invalid(detail: impl std::fmt::Display) -> Error {
    Error::pixel(PixelErrorKind::InvalidMetadata, detail)
}
fn unsupported(detail: &str) -> Error {
    Error::new(
        ErrorKind::UnsupportedDecompression,
        format!("{ERR_UNSUPPORTED_DECOMPRESSION}: {detail}"),
    )
}
fn get_u16(obj: &Object, tag: Tag) -> Option<u16> {
    let elem = obj.get(tag)?;
    if elem.vr() != Vr::US {
        return None;
    }
    let raw = elem.raw()?;
    let &[b0, b1] = raw else {
        return None;
    };
    let pair = [b0, b1];
    Some(if obj.value_big_endian() {
        u16::from_be_bytes(pair)
    } else {
        u16::from_le_bytes(pair)
    })
}
fn u16_element(tag: Tag, value: u16, big_endian: bool) -> Element {
    let raw = if big_endian {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    raw_element(tag, Vr::US, raw.to_vec())
}
fn photometric_element(value: &str) -> Element {
    strings_element(TAG_PHOTOMETRIC, Vr::CS, vec![value.to_string()])
}
fn number_of_frames(obj: &Object) -> Result<i64, Error> {
    let value = obj.get_string(TAG_NUMBER_OF_FRAMES).unwrap_or_default();
    let v = value.trim();
    if v.is_empty() {
        return Ok(1);
    }
    match crate::numtext::parse_int(v) {
        Ok(n) if n > 0 && n <= i64::from(i32::MAX) => Ok(n),
        _ => Err(invalid("invalid NumberOfFrames")),
    }
}
fn native_raw<'a>(pixel: &PixelData<'a>) -> Option<&'a [u8]> {
    match *pixel {
        PixelData::Native(raw) => Some(raw),
        PixelData::Encapsulated { .. } => None,
    }
}
fn native_size_matches(total: i64, size: i64) -> bool {
    total > 0 && (size == total || (total % 2 == 1 && size == total.saturating_add(1)))
}
fn mask_count(v: u16) -> Option<u16> {
    if v <= 16 || (v.wrapping_add(1).count_ones() != 1 && v != 0xffff) {
        return None;
    }
    Some(v.count_ones() as u16)
}
fn high_bit_mask(v: u16) -> Option<u16> {
    if v <= 15 || !v.is_multiple_of(2) {
        return None;
    }
    mask_count(v.saturating_add(1)).map(|n| n.saturating_sub(1))
}
fn with_bit_count_mask_repair(
    dataset: &Object,
    source: &Syntax,
) -> Option<(Object, Vec<MetadataReconciliation>)> {
    let allocated = get_u16(dataset, TAG_BITS_ALLOCATED)?;
    let stored = get_u16(dataset, TAG_BITS_STORED)?;
    let high = get_u16(dataset, TAG_HIGH_BIT)?;
    let mut reconciled = Vec::new();
    let mut replace = |name: &'static str, tag: Tag, declared: u16, used: u16| {
        reconciled.push(MetadataReconciliation {
            attribute: name,
            tag,
            declared: declared.to_string(),
            used: used.to_string(),
        });
    };
    let (mut new_allocated, mut new_stored, mut new_high) = (allocated, stored, high);
    if let Some(n) = mask_count(allocated) {
        new_allocated = n;
        replace("BitsAllocated", TAG_BITS_ALLOCATED, allocated, n);
    }
    if let Some(n) = mask_count(stored) {
        new_stored = n;
        replace("BitsStored", TAG_BITS_STORED, stored, n);
    }
    if let Some(n) = high_bit_mask(high) {
        new_high = n;
        replace("HighBit", TAG_HIGH_BIT, high, n);
    }
    if reconciled.is_empty()
        || (new_allocated != 8 && new_allocated != 16)
        || new_stored == 0
        || new_stored > new_allocated
        || new_high != new_stored.wrapping_sub(1)
    {
        return None;
    }
    let order = dataset.value_big_endian();
    let mut elements = dataset.elements().to_vec();
    replace_element(
        &mut elements,
        u16_element(TAG_BITS_ALLOCATED, new_allocated, order),
    );
    replace_element(
        &mut elements,
        u16_element(TAG_BITS_STORED, new_stored, order),
    );
    replace_element(&mut elements, u16_element(TAG_HIGH_BIT, new_high, order));
    let out = object_from(elements, order);
    {
        let pixel = extract_view(&out).ok()?;
        if let Some(raw) = native_raw(&pixel)
            && !source.encapsulated
        {
            let metadata = extract_metadata(&out).ok()?;
            if !native_size_matches(total_size(&metadata), raw.len() as i64) {
                return None;
            }
        }
    }
    Some((out, reconciled))
}
const MAX_DEFINED_LENGTH_FRAGMENTS: usize = 100_000;
pub(crate) fn defined_length_fragments(dataset: &Object, source: &Syntax) -> Option<Object> {
    if !source.encapsulated {
        return None;
    }
    let (offset_table, fragments) = {
        let pixel = extract_view(dataset).ok()?;
        split_defined_length_items(native_raw(&pixel)?, MAX_DEFINED_LENGTH_FRAGMENTS)?
    };
    let mut elements = dataset.elements().to_vec();
    replace_element(&mut elements, fragments_element(offset_table, fragments));
    Some(object_from(elements, dataset.value_big_endian()))
}
fn split_defined_length_items(value: &[u8], max: usize) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
    let mut items: Vec<&[u8]> = Vec::new();
    let mut pos = 0usize;
    while let Some(rest) = value.get(pos..).filter(|r| !r.is_empty()) {
        let (&[g0, g1, e0, e1, l0, l1, l2, l3], body) = rest.split_first_chunk::<8>()?;
        let group = u16::from_le_bytes([g0, g1]);
        let element = u16::from_le_bytes([e0, e1]);
        let length = u32::from_le_bytes([l0, l1, l2, l3]);
        if group == 0xFFFE && element == 0xE0DD && length == 0 && body.is_empty() {
            pos = pos.saturating_add(8);
        } else if group == 0xFFFE
            && element == 0xE000
            && length != 0xFFFF_FFFF
            && let Some(item) = body.get(..length as usize)
        {
            if items.len() > max {
                return None;
            }
            items.push(item);
            pos = pos.saturating_add(item.len()).saturating_add(8);
        } else {
            return None;
        }
    }
    let [first, rest @ ..] = items.as_slice() else {
        return None;
    };
    if rest.is_empty() {
        return None;
    }
    Some((first.to_vec(), rest.iter().map(|f| f.to_vec()).collect()))
}
pub(crate) fn native_under_encapsulated(dataset: &Object, source: &Syntax) -> bool {
    if !source.encapsulated || transfer::normalize_uid(&source.uid) == UID_ENCAPSULATED_UNCOMPRESSED
    {
        return false;
    }
    let Ok(pixel) = extract_view(dataset) else {
        return false;
    };
    let Some(raw) = native_raw(&pixel) else {
        return false;
    };
    let Ok(metadata) = extract_metadata(dataset) else {
        return false;
    };
    native_size_matches(total_size(&metadata), raw.len() as i64)
}
const JP2_SIGNATURE: [u8; 12] = [
    0, 0, 0, 0x0c, b'j', b'P', b' ', b' ', 0x0d, 0x0a, 0x87, 0x0a,
];
fn unwrap_jp2(dataset: &Object, source: &Syntax) -> Option<Object> {
    if !transfer::is_jpeg_2000_transfer_syntax(&source.uid) {
        return None;
    }
    let mut codestreams: Vec<Vec<u8>> = Vec::new();
    {
        let pixel = extract_view(dataset).ok()?;
        let PixelData::Encapsulated { fragments, .. } = pixel else {
            return None;
        };
        if !fragments
            .first()
            .is_some_and(|f| f.starts_with(&JP2_SIGNATURE))
        {
            return None;
        }
        let frames = number_of_frames(dataset).ok()?;
        if frames == 1 {
            let joined = fragments.concat();
            codestreams.push(jp2_codestream(&joined)?.to_vec());
        } else if frames == fragments.len() as i64 {
            for fragment in fragments {
                codestreams.push(jp2_codestream(fragment)?.to_vec());
            }
        } else {
            return None;
        }
    }
    for codestream in &mut codestreams {
        if codestream.len() % 2 != 0 {
            codestream.push(0);
        }
    }
    let mut elements = dataset.elements().to_vec();
    replace_element(&mut elements, fragments_element(Vec::new(), codestreams));
    Some(object_from(elements, dataset.value_big_endian()))
}
fn single_sample_photometric(value: &str) -> bool {
    matches!(
        value.trim(),
        "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR"
    )
}
fn is_libido_image(dataset: &Object) -> bool {
    if !dataset
        .get_uid(TAG_SOP_CLASS)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return false;
    }
    let code = dataset.get_string(TAG_RECOGNITION_CODE).unwrap_or_default();
    code == "ACRNEMA_LIBIDO_1.0" || code == "ACRNEMA_LIBIDO_1.1"
}
fn is_libido_rgb24(dataset: &Object, rows: u16, columns: u16, frames: i64, bytes: i64) -> bool {
    for (tag, want) in [
        (TAG_BITS_ALLOCATED, 24u16),
        (TAG_BITS_STORED, 24),
        (TAG_HIGH_BIT, 23),
        (TAG_PIXEL_REPRESENTATION, 0),
    ] {
        if get_u16(dataset, tag) != Some(want) {
            return false;
        }
    }
    if dataset.has(TAG_SAMPLES_PER_PIXEL) && get_u16(dataset, TAG_SAMPLES_PER_PIXEL) != Some(1) {
        return false;
    }
    if !dataset
        .get_string(TAG_PHOTOMETRIC)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return false;
    }
    if dataset.has(TAG_PLANAR_CONFIGURATION)
        && get_u16(dataset, TAG_PLANAR_CONFIGURATION) != Some(0)
    {
        return false;
    }
    let frame_bytes = i64::from(rows)
        .saturating_mul(i64::from(columns))
        .saturating_mul(3);
    frame_bytes > 0 && frames > 0 && frame_bytes.checked_mul(frames) == Some(bytes)
}
fn fits_one_sample(n: i64, rows: u16, columns: u16, bits: u16, frames: i64) -> bool {
    let size = |samples: u16| {
        total_size(&Metadata {
            rows,
            columns,
            samples_per_pixel: samples,
            bits_allocated: bits,
            number_of_frames: frames,
            ..Metadata::default()
        })
    };
    let (one, three) = (size(1), size(3));
    one > 0 && n >= one && (three <= 0 || n < three)
}
fn unpack_12_bit(packed: &[u8], big_endian: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity((packed.len() / 3).saturating_mul(4));
    let bytes = |v: u16| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    for &[b0, b1, b2] in packed.as_chunks::<3>().0 {
        let (b0, b1, b2) = (u16::from(b0), u16::from(b1), u16::from(b2));
        out.extend_from_slice(&bytes(b0 | ((b1 & 0x0f) << 8)));
        out.extend_from_slice(&bytes((b1 >> 4) | (b2 << 4)));
    }
    out
}
pub(crate) fn with_legacy_image_recovery(
    dataset: &Object,
    source: &Syntax,
    recovery: &DecodeRecovery,
) -> Option<(Object, Vec<String>)> {
    if !recovery.default_image_pixel_module && !recovery.unpack_12_bit_packed {
        return None;
    }
    let pixel = extract_view(dataset).ok()?;
    let pixel_raw = native_raw(&pixel);
    let native = pixel_raw.is_some() && !source.encapsulated;
    let order = dataset.value_big_endian();
    let mut elements = dataset.elements().to_vec();
    let mut applied: Vec<String> = Vec::new();
    let mut frames = number_of_frames(dataset).ok()?;
    if recovery.default_image_pixel_module {
        let has_frames = dataset.get_string(TAG_NUMBER_OF_FRAMES).is_some();
        let dims = get_u16(dataset, TAG_IMAGE_DIMENSIONS).unwrap_or(0);
        if let Some(planes) = get_u16(dataset, TAG_PLANES)
            && !has_frames
            && dims == 3
            && planes > 1
        {
            frames = i64::from(planes);
            replace_element(
                &mut elements,
                strings_element(TAG_NUMBER_OF_FRAMES, Vr::IS, vec![frames.to_string()]),
            );
            applied.push(DIAGNOSTIC_NUMBER_OF_FRAMES_FROM_PLANES.into());
        }
    }
    let mut rows = get_u16(dataset, TAG_ROWS)?;
    let mut columns = get_u16(dataset, TAG_COLUMNS)?;
    let mut bits = get_u16(dataset, TAG_BITS_ALLOCATED)?;
    let raw_len = pixel_raw.map_or(0, <[u8]>::len) as i64;
    let mut libido_rgb = false;
    if recovery.default_image_pixel_module
        && native
        && rows > 0
        && columns > 0
        && is_libido_image(dataset)
    {
        let mut repaired = false;
        if rows != columns {
            std::mem::swap(&mut rows, &mut columns);
            replace_element(&mut elements, u16_element(TAG_ROWS, rows, order));
            replace_element(&mut elements, u16_element(TAG_COLUMNS, columns, order));
            applied.push(DIAGNOSTIC_LIBIDO_DIMENSIONS.into());
            repaired = true;
        }
        if is_libido_rgb24(dataset, rows, columns, frames, raw_len) {
            bits = 8;
            for replacement in [
                u16_element(TAG_SAMPLES_PER_PIXEL, 3, order),
                photometric_element("RGB"),
                u16_element(TAG_PLANAR_CONFIGURATION, 0, order),
                u16_element(TAG_BITS_ALLOCATED, 8, order),
                u16_element(TAG_BITS_STORED, 8, order),
                u16_element(TAG_HIGH_BIT, 7, order),
            ] {
                replace_element(&mut elements, replacement);
            }
            applied.push(DIAGNOSTIC_LIBIDO_RGB24.into());
            libido_rgb = true;
            repaired = true;
        }
        if repaired {
            elements.retain(|e| e.tag() != TAG_RECOGNITION_CODE);
        }
    }
    let samples_declared = get_u16(dataset, TAG_SAMPLES_PER_PIXEL);
    let has_samples = samples_declared.is_some();
    let samples = samples_declared.unwrap_or(1);
    let mut unpacked_len = raw_len;
    if recovery.unpack_12_bit_packed
        && native
        && bits == 12
        && let Some(raw) = pixel_raw
    {
        let count = i64::from(rows)
            .saturating_mul(i64::from(columns))
            .saturating_mul(frames)
            .saturating_mul(i64::from(samples));
        if count > 0 && count % 2 == 0 && raw.len() as i64 == (count / 2).saturating_mul(3) {
            let unpacked = unpack_12_bit(raw, order);
            unpacked_len = unpacked.len() as i64;
            bits = 16;
            replace_element(&mut elements, raw_element(TAG_PIXEL_DATA, Vr::OW, unpacked));
            replace_element(&mut elements, u16_element(TAG_BITS_ALLOCATED, 16, order));
            applied.push(DIAGNOSTIC_UNPACKED_12_BIT.into());
        }
    }
    if recovery.default_image_pixel_module {
        if libido_rgb {
            return Some((object_from(elements, order), applied));
        }
        let photometric =
            normalized_photometric(&dataset.get_string(TAG_PHOTOMETRIC).unwrap_or_default());
        let missing_photometric = photometric.is_empty();
        let fits = !native || fits_one_sample(unpacked_len, rows, columns, bits, frames);
        if !has_samples && (missing_photometric || single_sample_photometric(&photometric)) && fits
        {
            replace_element(&mut elements, u16_element(TAG_SAMPLES_PER_PIXEL, 1, order));
            applied.push(DIAGNOSTIC_DEFAULT_SAMPLES_PER_PIXEL.into());
            if missing_photometric {
                replace_element(&mut elements, photometric_element("MONOCHROME2"));
                applied.push(DIAGNOSTIC_DEFAULT_PHOTOMETRIC.into());
            }
        } else if has_samples && samples == 1 && missing_photometric && fits {
            replace_element(&mut elements, photometric_element("MONOCHROME2"));
            applied.push(DIAGNOSTIC_DEFAULT_PHOTOMETRIC.into());
        }
    }
    if applied.is_empty() {
        return None;
    }
    Some((object_from(elements, order), applied))
}
fn photometric_samples(name: &str) -> u16 {
    match name {
        "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR" => 1,
        "RGB" | "HSV" | "YBR_FULL" | "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420"
        | "YBR_ICT" | "YBR_RCT" | "XYB" => 3,
        "ARGB" | "CMYK" => 4,
        _ => 0,
    }
}
fn with_photometric_recovery(
    dataset: &Object,
    source: &Syntax,
    limits: &DecompressLimits,
) -> Result<Option<(Object, Vec<String>)>, Error> {
    let pixel = extract_view(dataset)?;
    let raw = match native_raw(&pixel) {
        Some(raw) if !source.encapsulated => raw,
        _ => {
            return Err(unsupported(
                "photometric recovery requires native Pixel Data",
            ));
        }
    };
    let raw_len = raw.len() as i64;
    if raw_len > limits.max_input_bytes {
        return Err(limit_error(
            "MaxInputBytes",
            raw_len,
            limits.max_input_bytes,
        ));
    }
    let fail = |field: &str| {
        invalid(format_args!(
            "photometric recovery cannot establish {field}"
        ))
    };
    let rows = get_u16(dataset, TAG_ROWS);
    let cols = get_u16(dataset, TAG_COLUMNS);
    let bits = get_u16(dataset, TAG_BITS_ALLOCATED);
    let frames = number_of_frames(dataset)?;
    let (Some(rows), Some(cols), Some(bits)) = (rows, cols, bits) else {
        return Err(fail("native dimensions/bit depth"));
    };
    if rows == 0 || cols == 0 || (bits != 1 && bits != 8 && bits != 16) {
        return Err(fail("native dimensions/bit depth"));
    }
    let samples_declared = get_u16(dataset, TAG_SAMPLES_PER_PIXEL);
    if dataset.has(TAG_SAMPLES_PER_PIXEL) && samples_declared.is_none() {
        return Err(fail("SamplesPerPixel"));
    }
    let has_samples = samples_declared.is_some();
    let samples = samples_declared.unwrap_or(0);
    if let Some(elem) = dataset.get(TAG_PHOTOMETRIC) {
        if elem.vr() != Vr::CS {
            return Err(fail("PhotometricInterpretation VR"));
        }
        let bounded = match &elem.value {
            Value::Strings(values) => {
                values.len() <= 1
                    && values
                        .first()
                        .is_none_or(|v| v.len() <= 18 && !v.contains('\\'))
            }
            _ => elem
                .raw()
                .is_some_and(|raw| raw.len() <= 18 && !raw.contains(&b'\\')),
        };
        if !bounded {
            return Err(fail("PhotometricInterpretation VM/length"));
        }
    }
    let name = dataset.get_string(TAG_PHOTOMETRIC);
    if dataset.has(TAG_PHOTOMETRIC) && name.is_none() {
        return Err(fail("PhotometricInterpretation"));
    }
    let original = normalized_photometric(&name.unwrap_or_default());
    let mut name = original.clone();
    let mut wanted = photometric_samples(&name);
    let inferred = wanted == 0;
    let order = dataset.value_big_endian();
    let mut changes: Vec<Element> = Vec::new();
    let mut diagnostics: Vec<String> = Vec::new();
    if inferred {
        name = match samples {
            1 => "MONOCHROME2",
            3 => "RGB",
            4 => "ARGB",
            _ => return Err(fail("photometric from declared sample count")),
        }
        .to_string();
        wanted = samples;
        changes.push(photometric_element(&name));
        diagnostics.push(
            if original.is_empty() {
                DIAGNOSTIC_PHOTOMETRIC_FROM_SAMPLES
            } else {
                DIAGNOSTIC_INVALID_PHOTOMETRIC_REPLACED
            }
            .into(),
        );
    }
    if matches!(
        name.as_str(),
        "YBR_PARTIAL_420" | "YBR_ICT" | "YBR_RCT" | "XYB"
    ) {
        return Err(fail("native photometric"));
    }
    if wanted > 1 && (get_u16(dataset, TAG_PIXEL_REPRESENTATION) != Some(0) || bits == 1) {
        return Err(fail("unsigned 8/16-bit color samples"));
    }
    if !has_samples || samples != wanted {
        changes.push(u16_element(TAG_SAMPLES_PER_PIXEL, wanted, order));
        diagnostics.push(DIAGNOSTIC_SAMPLES_FROM_PHOTOMETRIC.into());
    }
    let planar_declared = get_u16(dataset, TAG_PLANAR_CONFIGURATION);
    let has_planar = planar_declared.is_some();
    let planar = planar_declared.unwrap_or(0);
    if (dataset.has(TAG_PLANAR_CONFIGURATION) && !has_planar) || (has_planar && planar > 1) {
        return Err(fail("PlanarConfiguration"));
    }
    if has_planar && planar == 1 && (wanted == 1 || (inferred && wanted == 4)) {
        changes.push(u16_element(TAG_PLANAR_CONFIGURATION, 0, order));
        diagnostics.push(DIAGNOSTIC_IGNORED_PLANAR_CONFIGURATION.into());
    } else if !has_planar && wanted > 1 {
        changes.push(u16_element(TAG_PLANAR_CONFIGURATION, 0, order));
        diagnostics.push(DIAGNOSTIC_DEFAULT_PLANAR_CONFIGURATION.into());
    }
    if changes.is_empty() {
        return Ok(None);
    }
    if is_subsampled_422(&name) && (cols % 2 != 0 || planar != 0) {
        return Err(fail("native 4:2:2 layout"));
    }
    let size = total_size(&Metadata {
        rows,
        columns: cols,
        samples_per_pixel: wanted,
        bits_allocated: bits,
        number_of_frames: frames,
        photometric_interpretation: name,
        ..Metadata::default()
    });
    if size <= 0
        || (raw_len != size
            && !(size % 2 == 1
                && raw_len == size.saturating_add(1)
                && raw.last().copied() == Some(0)))
    {
        return Err(fail("exact native byte count"));
    }
    if size > limits.max_native_bytes {
        return Err(limit_error("MaxNativeBytes", size, limits.max_native_bytes));
    }
    let mut elements = dataset.elements().to_vec();
    for change in changes {
        replace_element(&mut elements, change);
    }
    Ok(Some((object_from(elements, order), diagnostics)))
}
#[derive(Default)]
struct CodestreamHeader {
    rows: i64,
    columns: i64,
    components: i64,
    precision: i64,
    has_sign: bool,
    signed: bool,
    color_photometric: &'static str,
}
fn be16(data: &[u8], at: usize) -> i64 {
    data.get(at..)
        .and_then(|d| d.first_chunk::<2>())
        .map_or(0, |&b| i64::from(u16::from_be_bytes(b)))
}
fn be32(data: &[u8], at: usize) -> i64 {
    data.get(at..)
        .and_then(|d| d.first_chunk::<4>())
        .map_or(0, |&b| i64::from(u32::from_be_bytes(b)))
}
fn parse_jpeg_frame_header(data: &[u8]) -> Option<CodestreamHeader> {
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
        let length = be16(data, i.saturating_add(2)) as usize;
        if length < 2 || i.saturating_add(2).saturating_add(length) > data.len() {
            return None;
        }
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            if length < 8 {
                return None;
            }
            let segment = data.get(i.saturating_add(4)..)?;
            let lossless = marker == 0xc3 || marker == 0xc7;
            return Some(CodestreamHeader {
                precision: i64::from(*segment.first()?),
                rows: be16(segment, 1),
                columns: be16(segment, 3),
                components: i64::from(*segment.get(5)?),
                color_photometric: if lossless { "RGB" } else { "YBR_FULL_422" },
                ..CodestreamHeader::default()
            });
        }
        i = i.saturating_add(length.saturating_add(2));
    }
    None
}
fn parse_jpeg2000_siz(data: &[u8]) -> Option<CodestreamHeader> {
    const FIXED: usize = 2 + 2 + 2 + 2 + 8 * 4 + 2;
    if data.len() < FIXED || !data.starts_with(&[0xff, 0x4f, 0xff, 0x51]) {
        return None;
    }
    let width = be32(data, 8).saturating_sub(be32(data, 16));
    let height = be32(data, 12).saturating_sub(be32(data, 20));
    let components = be16(data, 40) as usize;
    let entries = data.get(FIXED..).unwrap_or_default().as_chunks::<3>().0;
    if width <= 0
        || height <= 0
        || width > 0xffff
        || height > 0xffff
        || components < 1
        || entries.len() < components
    {
        return None;
    }
    let mut header = CodestreamHeader {
        rows: height,
        columns: width,
        components: components as i64,
        has_sign: true,
        color_photometric: "RGB",
        ..CodestreamHeader::default()
    };
    for (c, &[ssiz, xr, yr]) in entries.iter().take(components).enumerate() {
        let (precision, signed) = (i64::from(ssiz & 0x7f).saturating_add(1), ssiz & 0x80 != 0);
        if xr != 1
            || yr != 1
            || (c > 0 && (precision != header.precision || signed != header.signed))
        {
            return None;
        }
        header.precision = precision;
        header.signed = signed;
    }
    Some(header)
}
fn codestream_header_of(fragment: &[u8], source: &Syntax) -> Option<CodestreamHeader> {
    match transfer::normalize_uid(&source.uid) {
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.57"
        | "1.2.840.10008.1.2.4.70" => parse_jpeg_frame_header(fragment),
        uid if transfer::is_jpeg_2000_transfer_syntax(uid) => parse_jpeg2000_siz(fragment),
        _ => None,
    }
}
fn photometric_fits_components(photometric: &str, components: i64) -> bool {
    match photometric {
        "MONOCHROME1" | "MONOCHROME2" => components == 1,
        "RGB" | "YBR_FULL" | "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420" | "YBR_ICT"
        | "YBR_RCT" => components == 3,
        _ => false,
    }
}
fn reconcile_with_codestream(
    dataset: &Object,
    source: &Syntax,
) -> Option<(Object, Vec<MetadataReconciliation>)> {
    let header = {
        let pixel = extract_view(dataset).ok()?;
        let PixelData::Encapsulated { fragments, .. } = pixel else {
            return None;
        };
        codestream_header_of(fragments.first()?, source)?
    };
    if header.precision < 2
        || header.precision > 16
        || (header.components != 1 && header.components != 3)
    {
        return None;
    }
    let order = dataset.value_big_endian();
    let mut elements = dataset.elements().to_vec();
    let mut recs: Vec<MetadataReconciliation> = Vec::new();
    let set_us = |elements: &mut Vec<Element>,
                  recs: &mut Vec<MetadataReconciliation>,
                  name: &'static str,
                  tag: Tag,
                  want: i64| {
        let got = get_u16(dataset, tag);
        if got.map(i64::from) == Some(want) {
            return;
        }
        replace_element(elements, u16_element(tag, want as u16, order));
        recs.push(MetadataReconciliation {
            attribute: name,
            tag,
            declared: got.map(|g| g.to_string()).unwrap_or_default(),
            used: want.to_string(),
        });
    };
    if header.rows > 0 {
        set_us(&mut elements, &mut recs, "Rows", TAG_ROWS, header.rows);
    }
    set_us(
        &mut elements,
        &mut recs,
        "Columns",
        TAG_COLUMNS,
        header.columns,
    );
    set_us(
        &mut elements,
        &mut recs,
        "SamplesPerPixel",
        TAG_SAMPLES_PER_PIXEL,
        header.components,
    );
    let photometric =
        normalized_photometric(&dataset.get_string(TAG_PHOTOMETRIC).unwrap_or_default());
    if !photometric_fits_components(&photometric, header.components) {
        let want = if header.components == 3 {
            header.color_photometric
        } else {
            "MONOCHROME2"
        };
        replace_element(&mut elements, photometric_element(want));
        recs.push(MetadataReconciliation {
            attribute: "PhotometricInterpretation",
            tag: TAG_PHOTOMETRIC,
            declared: photometric,
            used: want.to_string(),
        });
    }
    if header.components == 3 {
        set_us(
            &mut elements,
            &mut recs,
            "PlanarConfiguration",
            TAG_PLANAR_CONFIGURATION,
            0,
        );
    }
    let allocated = if header.precision > 8 { 16 } else { 8 };
    let declared = get_u16(dataset, TAG_BITS_ALLOCATED);
    if !matches!(declared, Some(got) if (got == 8 || got == 16) && i64::from(got) >= header.precision)
    {
        set_us(
            &mut elements,
            &mut recs,
            "BitsAllocated",
            TAG_BITS_ALLOCATED,
            allocated,
        );
    }
    set_us(
        &mut elements,
        &mut recs,
        "BitsStored",
        TAG_BITS_STORED,
        header.precision,
    );
    set_us(
        &mut elements,
        &mut recs,
        "HighBit",
        TAG_HIGH_BIT,
        header.precision.saturating_sub(1),
    );
    if header.has_sign {
        set_us(
            &mut elements,
            &mut recs,
            "PixelRepresentation",
            TAG_PIXEL_REPRESENTATION,
            i64::from(header.signed),
        );
    }
    if recs.is_empty() {
        return None;
    }
    Some((object_from(elements, order), recs))
}
const PALETTE_DESCRIPTOR_TAGS: [Tag; 3] = [
    Tag::new(0x0028, 0x1101),
    Tag::new(0x0028, 0x1102),
    Tag::new(0x0028, 0x1103),
];
const PALETTE_DATA_TAGS: [Tag; 3] = [
    Tag::new(0x0028, 0x1201),
    Tag::new(0x0028, 0x1202),
    Tag::new(0x0028, 0x1203),
];
const SEGMENTED_PALETTE_TAGS: [Tag; 3] = [
    Tag::new(0x0028, 0x1221),
    Tag::new(0x0028, 0x1222),
    Tag::new(0x0028, 0x1223),
];
const RETIRED_COLOR_FREE_TAGS: [Tag; 7] = [
    Tag::new(0x0028, 0x0106),
    Tag::new(0x0028, 0x0107),
    Tag::new(0x0028, 0x0120),
    Tag::new(0x0028, 0x0121),
    Tag::new(0x0028, 0x1199),
    Tag::new(0x0028, 0x2000),
    Tag::new(0x0028, 0x2002),
];
fn legacy_layout_error() -> Error {
    invalid("unsupported legacy pixel repair layout")
}
fn bounded_legacy_palette(dataset: &Object) -> Result<(), Error> {
    for (tags, limit) in [
        (PALETTE_DESCRIPTOR_TAGS, 6usize),
        (PALETTE_DATA_TAGS, 2 * 65536),
        (SEGMENTED_PALETTE_TAGS, 8 * (65536 + 2)),
    ] {
        for tag in tags {
            if let Some(elem) = dataset.get(tag)
                && !matches!(elem.raw(), Some(raw) if raw.len() <= limit)
            {
                return Err(unsupported("legacy palette exceeds bounded profile"));
            }
        }
    }
    Ok(())
}
fn palette_of(dataset: &Object) -> Result<Option<PaletteColorLut>, Error> {
    display::palette_from_object(dataset, LutOptions::default())
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
}
fn repair_palette_tags(out: &mut Object, m: &Metadata) -> Result<bool, Error> {
    let mut wrong = Vec::with_capacity(6);
    let mut canonical = Vec::with_capacity(6);
    for c in 1u16..=3 {
        wrong.push(Tag::new(0x0028, c.saturating_add(0x1110)));
        wrong.push(Tag::new(0x0028, c.saturating_add(0x1210)));
        canonical.push(Tag::new(0x0028, c.saturating_add(0x1100)));
        canonical.push(Tag::new(0x0028, c.saturating_add(0x1200)));
    }
    let count = wrong.iter().filter(|t| out.has(**t)).count();
    if count == 0 {
        return Ok(false);
    }
    let photometric = normalized_photometric(&m.photometric_interpretation);
    if count != 6 || (photometric != "PALETTE COLOR" && photometric != "ARGB") {
        return Err(invalid("incomplete legacy palette tag set"));
    }
    for (c, tag) in canonical.iter().enumerate() {
        if out.has(*tag) || out.has(Tag::new(0x0028, ((c / 2) as u16).saturating_add(0x1221))) {
            return Err(invalid("conflicting palette tag sets"));
        }
    }
    for (tag, &right) in wrong.iter().zip(&canonical) {
        let Some(mut elem) = out.get(*tag).cloned() else {
            continue;
        };
        elem.header.tag = right;
        out.remove(*tag);
        out.put(elem);
    }
    bounded_legacy_palette(out)?;
    match display::palette_from_object(out, LutOptions::default()) {
        Ok(Some(_)) => Ok(true),
        _ => Err(invalid("malformed legacy palette")),
    }
}
fn with_legacy_pixel_repairs(
    dataset: &Object,
    source: &Syntax,
    m: &Metadata,
    r: &DecodeRecovery,
) -> Result<Option<(Object, Metadata, Vec<String>)>, Error> {
    if !r.requires_legacy_pixel_repair() {
        return Ok(None);
    }
    if r.native_planar_layout > NATIVE_PLANAR_WHOLE_IMAGE {
        return Err(legacy_layout_error());
    }
    if (r.native_planar_layout != NATIVE_PLANAR_DECLARED || r.native_ow_byte_swap)
        && source.encapsulated
    {
        return Err(legacy_layout_error());
    }
    let mut m = m.clone();
    let mut out = object_from(dataset.elements().to_vec(), dataset.value_big_endian());
    let mut diagnostics: Vec<String> = Vec::new();
    if r.wrong_palette_attribute_tags && repair_palette_tags(&mut out, &m)? {
        diagnostics.push(DIAGNOSTIC_WRONG_PALETTE_ATTRIBUTE_TAGS.into());
    }
    if r.native_ow_byte_swap || r.native_planar_layout != NATIVE_PLANAR_DECLARED {
        let size = total_size(&m);
        let (vr, mut raw) = match out.get(TAG_PIXEL_DATA) {
            Some(pixel) => match pixel.raw() {
                Some(raw) => (pixel.vr(), raw.to_vec()),
                None => return Err(legacy_layout_error()),
            },
            None => return Err(legacy_layout_error()),
        };
        let len = raw.len() as i64;
        if size <= 0
            || size > len
            || len > size.saturating_add(size % 2)
            || (m.bits_allocated != 8 && m.bits_allocated != 16)
        {
            return Err(legacy_layout_error());
        }
        if r.native_ow_byte_swap {
            if vr != Vr::OW || !raw.len().is_multiple_of(2) {
                return Err(legacy_layout_error());
            }
            for pair in raw.as_chunks_mut::<2>().0 {
                pair.swap(0, 1);
            }
            diagnostics.push(DIAGNOSTIC_NATIVE_OW_BYTE_SWAP.into());
        }
        if r.native_planar_layout != NATIVE_PLANAR_DECLARED {
            let photometric = normalized_photometric(&m.photometric_interpretation);
            if m.pixel_representation != 0
                || m.samples_per_pixel < 3
                || m.samples_per_pixel > 4
                || m.samples_per_pixel != photometric_samples(&photometric)
                || m.bits_stored == 0
                || m.bits_stored > m.bits_allocated
                || m.high_bit != m.bits_stored.wrapping_sub(1)
            {
                return Err(legacy_layout_error());
            }
            if !matches!(
                photometric.as_str(),
                "RGB" | "YBR_FULL" | "ARGB" | "CMYK" | "HSV"
            ) {
                return Err(legacy_layout_error());
            }
            let planar = u16::from(r.native_planar_layout != NATIVE_PLANAR_INTERLEAVED);
            if r.native_planar_layout == NATIVE_PLANAR_WHOLE_IMAGE {
                let mut reordered = vec![0u8; raw.len()];
                let plane = usize::from(m.rows)
                    .saturating_mul(usize::from(m.columns))
                    .saturating_mul(bytes_per_sample(&m) as usize);
                let frames = m.number_of_frames as usize;
                let samples = usize::from(m.samples_per_pixel);
                for f in 0..frames {
                    for c in 0..samples {
                        let from = c
                            .saturating_mul(frames)
                            .saturating_add(f)
                            .saturating_mul(plane);
                        let to = f
                            .saturating_mul(samples)
                            .saturating_add(c)
                            .saturating_mul(plane);
                        let (Some(dst), Some(src)) = (
                            reordered.get_mut(to..to.saturating_add(plane)),
                            raw.get(from..from.saturating_add(plane)),
                        ) else {
                            return Err(legacy_layout_error());
                        };
                        dst.copy_from_slice(src);
                    }
                }
                raw = reordered;
                diagnostics.push(DIAGNOSTIC_NATIVE_WHOLE_IMAGE_PLANAR.into());
            } else if !m.planar_configuration_present || m.planar_configuration != planar {
                diagnostics.push(DIAGNOSTIC_NATIVE_PLANAR_OVERRIDE.into());
            }
            m.planar_configuration = planar;
            m.planar_configuration_present = true;
            let order = out.value_big_endian();
            out.put(u16_element(TAG_PLANAR_CONFIGURATION, planar, order));
        }
        out.put(raw_element(TAG_PIXEL_DATA, vr, raw));
    }
    Ok(Some((out, m, diagnostics)))
}
fn is_retired_color(name: &str) -> bool {
    matches!(
        normalized_photometric(name).as_str(),
        "ARGB" | "CMYK" | "HSV"
    )
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn retired_color_frames(
    dataset: &Object,
    source: &Syntax,
    frames: &Frames,
    m: &Metadata,
) -> Result<Option<(Frames, Metadata, &'static str)>, Error> {
    let name = normalized_photometric(&m.photometric_interpretation);
    if !is_retired_color(&name) {
        return Ok(None);
    }
    let fail = || unsupported("unsupported retired color normalization layout");
    let (channels, code) = match name.as_str() {
        "HSV" => (3usize, DIAGNOSTIC_RETIRED_HSV_TO_RGB),
        "CMYK" => (4, DIAGNOSTIC_RETIRED_CMYK_TO_RGB),
        _ => (4, DIAGNOSTIC_RETIRED_ARGB_TO_RGB),
    };
    if source.encapsulated
        || m.pixel_representation != 0
        || usize::from(m.samples_per_pixel) != channels
        || (m.bits_allocated != 8 && m.bits_allocated != 16)
        || m.bits_stored != m.bits_allocated
        || m.high_bit != m.bits_stored - 1
        || !m.planar_configuration_present
        || m.planar_configuration > 1
    {
        return Err(fail());
    }
    let mut palette = None;
    if name == "ARGB" {
        bounded_legacy_palette(dataset)?;
        palette = palette_of(dataset)?;
    }
    let tables = palette
        .as_ref()
        .and_then(|p| Some([p.red.as_ref()?, p.green.as_ref()?, p.blue.as_ref()?]));
    let pixels = usize::from(m.rows) * usize::from(m.columns);
    let width = bytes_per_sample(m) as usize;
    let maximum = f64::from(1u32.wrapping_shl(u32::from(m.bits_stored)).wrapping_sub(1));
    let mut out = Frames {
        rows: frames.rows,
        columns: frames.columns,
        data: Vec::with_capacity(frames.data.len()),
        diagnostics: frames.diagnostics.clone(),
        photometric_interpretation: "RGB".into(),
    };
    for raw in &frames.data {
        if raw.len() != pixels * channels * width {
            return Err(fail());
        }
        let mut rgb = vec![0u8; pixels * 3 * width];
        let sample = |i: usize, c: usize| -> f64 {
            let pos = if m.planar_configuration == 1 {
                c * pixels + i
            } else {
                i * channels + c
            };
            if width == 1 {
                f64::from(raw[pos])
            } else {
                f64::from(u16::from_le_bytes([raw[pos * 2], raw[pos * 2 + 1]]))
            }
        };
        for i in 0..pixels {
            let mut values = [0f64; 3];
            match name.as_str() {
                "ARGB" => {
                    let alpha = sample(i, 0) as i64;
                    if alpha > 0 {
                        let Some(tables) = tables else {
                            return Err(Error::new(
                                ErrorKind::Other,
                                display::ColorError::MissingPalette.to_string(),
                            ));
                        };
                        for (c, lut) in tables.iter().enumerate() {
                            let top =
                                (1u32 << (lut.bits_per_entry.clamp(0, 31) as u32)).wrapping_sub(1);
                            let Some(value16) = u32::from(lut.lookup(alpha))
                                .wrapping_mul(65535)
                                .wrapping_add(top / 2)
                                .checked_div(top)
                            else {
                                return Err(fail());
                            };
                            values[c] = if width == 1 {
                                f64::from(value16 >> 8)
                            } else {
                                f64::from(value16)
                            };
                        }
                    } else {
                        for (c, v) in values.iter_mut().enumerate() {
                            *v = sample(i, c + 1);
                        }
                    }
                }
                "CMYK" => {
                    for (c, v) in values.iter_mut().enumerate() {
                        *v = (maximum - sample(i, c) - sample(i, 3)).max(0.0);
                    }
                }
                _ => {
                    let h = sample(i, 0) * 6.0 / (maximum + 1.0);
                    let s = sample(i, 1) / maximum;
                    let v = sample(i, 2);
                    let chroma = v * s;
                    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
                    values = match (h as i64) % 6 {
                        0 => [chroma, x, 0.0],
                        1 => [x, chroma, 0.0],
                        2 => [0.0, chroma, x],
                        3 => [0.0, x, chroma],
                        4 => [x, 0.0, chroma],
                        _ => [chroma, 0.0, x],
                    };
                    for value in &mut values {
                        *value += v - chroma;
                    }
                }
            }
            for (c, value) in values.iter().enumerate() {
                let value = value.floor();
                if width == 1 {
                    rgb[3 * i + c] = crate::fmath::f64_to_u8(value);
                } else {
                    let at = (3 * i + c) * 2;
                    rgb[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
                }
            }
        }
        out.data.push(rgb);
    }
    let mut m = m.clone();
    m.samples_per_pixel = 3;
    m.photometric_interpretation = "RGB".into();
    m.planar_configuration = 0;
    m.planar_configuration_present = true;
    Ok(Some((out, m, code)))
}
fn remove_retired_color_attributes(out: &mut Object) {
    for tag in PALETTE_DESCRIPTOR_TAGS
        .into_iter()
        .chain(PALETTE_DATA_TAGS)
        .chain(SEGMENTED_PALETTE_TAGS)
        .chain(RETIRED_COLOR_FREE_TAGS)
    {
        out.remove(tag);
    }
}
fn incompatible_incomplete_recovery(r: &DecodeRecovery) -> bool {
    r.requires_legacy_pixel_repair()
        || r.ignore_inconsistent_basic_offset_table
        || r.default_image_pixel_module
        || r.repair_photometric_interpretation
        || r.unpack_12_bit_packed
        || r.reconcile_with_codestream
        || r.unwrap_jp2_container
        || r.native_pixel_data_under_encapsulated
        || r.defined_length_fragments
        || r.repair_bit_count_masks
}
fn incompatible(detail: &str) -> Error {
    Error::pixel(PixelErrorKind::IncompatiblePixelData, detail)
}
fn incomplete_materialized_metadata(elements: &[Element], depth: usize) -> Result<(), Error> {
    if depth > 64 {
        return Err(incompatible(
            "metadata nesting exceeds incomplete-data profile",
        ));
    }
    for element in elements {
        match &element.value {
            Value::Nil => {
                return Err(incompatible(
                    "materialize deferred metadata before incomplete-data normalization",
                ));
            }
            Value::Sequence(items) => {
                for item in items {
                    incomplete_materialized_metadata(&item.elements, depth.saturating_add(1))?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn codec_table_attribute(obj: &Object, tag: Tag) -> Option<encapsulated::TableAttribute<'_>> {
    let elem = obj.get(tag)?;
    let value = match &elem.value {
        Value::Raw(raw) => encapsulated::TableValue::Raw(raw),
        _ => encapsulated::TableValue::Unsupported("core.Value"),
    };
    Some(encapsulated::TableAttribute {
        vr_ov: elem.vr() == Vr::OV,
        value,
    })
}
fn no_complete_frames(detail: Option<&encapsulated::Error>) -> Error {
    let text = match detail {
        Some(e) => format!("{ERR_NO_COMPLETE_FRAMES}\n{e}"),
        None => ERR_NO_COMPLETE_FRAMES.to_string(),
    };
    Error::new(ErrorKind::Other, text)
}
fn normalize_incomplete_pixel_data(
    dataset: &Object,
    source: &Syntax,
    metadata: &Metadata,
    info: &IncompletePixelData,
    limits: &DecompressLimits,
) -> Result<(Object, IncompleteFrameRecovery), Error> {
    incomplete_materialized_metadata(dataset.elements(), 0)?;
    let pixel = extract_view(dataset)?;
    if input_bytes(&pixel) != Some(info.available_bytes) {
        return Err(incompatible("inspected Pixel Data was changed"));
    }
    let mut report = IncompleteFrameRecovery {
        kind: info.kind.to_string(),
        value_offset: info.value_offset,
        declared_bytes: info.declared_bytes,
        available_bytes: info.available_bytes,
        declared_frames: metadata.number_of_frames,
        discarded_bytes: info.discarded_bytes,
        discarded_fragments: info.discarded_fragments,
        retained_frames: 0,
    };
    let Some(elem) = dataset.get(TAG_PIXEL_DATA) else {
        return Err(Error::pixel_bare(PixelErrorKind::PixelDataNotFound));
    };
    let mut elem = elem.clone();
    let value = if !source.encapsulated {
        let raw = match pixel {
            PixelData::Native(raw)
                if info.kind == "native-value-eof"
                    && elem.header.length_set
                    && i64::from(elem.header.length) == info.declared_bytes
                    && metadata.bits_allocated >= 8
                    && metadata.bits_allocated.is_multiple_of(8) =>
            {
                raw
            }
            _ => return Err(incompatible("unsupported incomplete native layout")),
        };
        let (size, total) = (frame_size(metadata), total_size(metadata));
        if size <= 0
            || total <= 0
            || total == i64::MAX
            || info.declared_bytes != total.saturating_add(total % 2)
            || raw.len() as i64 > total
        {
            return Err(Error::pixel_bare(PixelErrorKind::IncompatiblePixelData));
        }
        let retained = (raw.len() as i64).checked_div(size).unwrap_or(0);
        if retained < 1 {
            return Err(no_complete_frames(None));
        }
        report.retained_frames = retained;
        let keep = retained.saturating_mul(size) as usize;
        report.discarded_bytes = report
            .discarded_bytes
            .saturating_add((raw.len() as i64).saturating_sub(keep as i64));
        Value::Raw(raw.get(..keep).unwrap_or_default().to_vec())
    } else {
        let PixelData::Encapsulated {
            offset_table,
            fragments,
        } = pixel
        else {
            return Err(Error::pixel_bare(PixelErrorKind::IncompatiblePixelData));
        };
        if info.declared_bytes != -1 || fragments.len() as i64 != info.retained_fragments {
            return Err(Error::pixel_bare(PixelErrorKind::IncompatiblePixelData));
        }
        let format = match source.uid.as_ref() {
            "1.2.840.10008.1.2.4.50"
            | "1.2.840.10008.1.2.4.51"
            | "1.2.840.10008.1.2.4.57"
            | "1.2.840.10008.1.2.4.70" => encapsulated::Format::JPEG,
            "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => encapsulated::Format::JPEGLS,
            _ => return Err(unsupported("incomplete framing profile")),
        };
        let encap = |e: encapsulated::Error| Error::new(ErrorKind::Other, e.to_string());
        let tables = encapsulated::read_tables(
            offset_table,
            codec_table_attribute(dataset, TAG_EXTENDED_OFFSET_TABLE),
            codec_table_attribute(dataset, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
            metadata.number_of_frames,
        )
        .map_err(encap)?;
        let refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
        let prefix = match encapsulated::complete_prefix(
            Box::new(encapsulated::Memory::new(&refs)),
            &tables,
            metadata.number_of_frames,
            format,
            encapsulated::Limits {
                max_frames: limits.max_frames,
                max_bytes: limits.max_input_bytes as u64,
                max_frame_bytes: limits.max_input_bytes as u64,
                ..encapsulated::Limits::default()
            },
        ) {
            Ok((_, prefix)) => prefix,
            Err(e) if e.is(encapsulated::ErrorKind::FrameCount) => {
                return Err(no_complete_frames(Some(&e)));
            }
            Err(e) => return Err(encap(e)),
        };
        report.retained_frames = prefix.retained_frames as i64;
        report.discarded_fragments = report
            .discarded_fragments
            .saturating_add(prefix.discarded_fragments as i64);
        report.discarded_bytes = report
            .discarded_bytes
            .saturating_add(prefix.discarded_bytes as i64);
        let mut table = offset_table.to_vec();
        if !table.is_empty() {
            table.truncate(prefix.retained_frames.saturating_mul(4));
        }
        Value::Fragments {
            offset_table: table,
            fragments: fragments
                .get(..prefix.retained_fragments)
                .unwrap_or(fragments)
                .to_vec(),
        }
    };
    if report.retained_frames != metadata.number_of_frames {
        for tag in [
            Tag::new(0x5200, 0x9230),
            Tag::new(0x0020, 0x9161),
            Tag::new(0x0020, 0x9162),
            Tag::new(0x0020, 0x9163),
            Tag::new(0x0020, 0x9221),
            Tag::new(0x0020, 0x9222),
            Tag::new(0x0028, 0x0009),
            Tag::new(0x0018, 0x1065),
        ] {
            if dataset.has(tag) {
                return Err(incompatible("unsupported frame-dependent metadata"));
            }
        }
        for e in dataset.elements() {
            let t = e.tag();
            if (0x6000..=0x60ff).contains(&t.group)
                && t.group % 2 == 0
                && (t.element == 0x0015 || t.element == 0x0051)
            {
                return Err(Error::pixel_bare(PixelErrorKind::IncompatiblePixelData));
            }
        }
    }
    let mut out = object_from(dataset.elements().to_vec(), dataset.value_big_endian());
    if report.retained_frames != metadata.number_of_frames {
        out.put(strings_element(
            TAG_NUMBER_OF_FRAMES,
            Vr::IS,
            vec![report.retained_frames.to_string()],
        ));
    }
    if let Value::Raw(raw) = &value {
        elem.header.length = raw.len() as u32;
        elem.header.length_set = true;
    }
    elem.value = value;
    out.put(elem);
    Ok((out, report))
}
#[doc(hidden)]
pub fn decompress_data_set_with_recovery(
    dataset: &Object,
    source: &str,
    opts: &DecompressOptions,
    recovery: &DecodeRecovery,
    color: JpegColorConversion,
    report: Option<&mut DecompressReport>,
) -> Result<(Object, String), Error> {
    decompress_data_set_incomplete(dataset, None, source, opts, recovery, color, report)
}
fn decompress_data_set_incomplete(
    dataset: &Object,
    incomplete: Option<&IncompletePixelData>,
    source: &str,
    opts: &DecompressOptions,
    recovery: &DecodeRecovery,
    color: JpegColorConversion,
    report: Option<&mut DecompressReport>,
) -> Result<(Object, String), Error> {
    if incomplete.is_some()
        && (!recovery.incomplete_data || incompatible_incomplete_recovery(recovery))
    {
        return Err(Error::new(ErrorKind::PartialFile, ERR_PARTIAL_FILE));
    }
    let limits = normalized(opts.limits)?;
    let source = canonical_source(source)?;
    if !source.supported && !source.requires_codec() {
        return Err(unsupported(&format!(
            "source transfer syntax {} is not supported",
            textfmt::quote(&source.uid)
        )));
    }
    let target = target_syntax(&opts.target_transfer_syntax)?;
    let (output, found) = decompress_recovering(
        dataset,
        incomplete,
        &source,
        &limits,
        recovery,
        color,
        opts.registry,
    )?;
    if let Some(report) = report {
        *report = found;
    }
    Ok((output, target))
}
pub(crate) fn decompress_for_transcode_recovering(
    dataset: &Object,
    source: &Syntax,
    registry: Option<DecoderRegistry>,
    limits: DecompressLimits,
    recovery: &DecodeRecovery,
) -> Result<(Object, Vec<String>), Error> {
    let limits = normalized(limits)?;
    if !source.supported && !source.requires_codec() {
        return Err(unsupported(&format!(
            "source transfer syntax {} is not supported",
            textfmt::quote(&source.uid)
        )));
    }
    let (output, found) = decompress_recovering(
        dataset,
        None,
        source,
        &limits,
        recovery,
        JpegColorConversion::default(),
        registry,
    )?;
    Ok((output, found.diagnostics))
}
fn recovering_source_syntax(file: &File) -> Result<Syntax, Error> {
    let file_uid = transfer::normalize_uid(file.transfer_syntax.uid).to_string();
    let meta_uid = file
        .meta
        .get_string(Tag::new(0x0002, 0x0010))
        .map(|u| transfer::normalize_uid(&u).to_string())
        .unwrap_or_default();
    if !file_uid.is_empty() && !meta_uid.is_empty() && file_uid != meta_uid {
        return Err(incompatible(
            "file transfer syntax does not match File Meta Information",
        ));
    }
    if !file_uid.is_empty() {
        return canonical_source(&file_uid);
    }
    if !meta_uid.is_empty() {
        return canonical_source(&meta_uid);
    }
    Err(incompatible("source transfer syntax is empty"))
}
#[doc(hidden)]
pub fn decompress_file_with_recovery(
    file: &File,
    opts: &DecompressOptions,
    recovery: &DecodeRecovery,
    color: JpegColorConversion,
    report: Option<&mut DecompressReport>,
) -> Result<File, Error> {
    if let Some(partial) = &file.partial_read {
        let accepted = recovery.incomplete_data
            && file.incomplete_pixel_data.as_ref().is_some_and(|info| {
                partial.tag == TAG_PIXEL_DATA
                    && partial.offset == info.failure_offset
                    && partial
                        .err
                        .is(crate::object::error::Kind::IncompletePixelData)
            });
        if !accepted {
            return Err(Error::new(ErrorKind::PartialFile, ERR_PARTIAL_FILE));
        }
    }
    let source = recovering_source_syntax(file)?;
    let (dataset, target) = decompress_data_set_incomplete(
        &file.dataset,
        file.incomplete_pixel_data.as_ref(),
        &source.uid,
        opts,
        recovery,
        color,
        report,
    )?;
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
pub(crate) fn decompress_recovering(
    dataset: &Object,
    incomplete: Option<&IncompletePixelData>,
    source: &Syntax,
    limits: &DecompressLimits,
    r: &DecodeRecovery,
    color: JpegColorConversion,
    registry: Option<DecoderRegistry>,
) -> Result<(Object, DecompressReport), Error> {
    let mut defaulted: Vec<String> = Vec::new();
    let mut reconciled: Vec<MetadataReconciliation> = Vec::new();
    let mut source = source.clone();
    let mut owned: Option<Object> = None;
    macro_rules! current {
        () => {
            owned.as_ref().unwrap_or(dataset)
        };
    }
    if r.repair_bit_count_masks
        && let Some((out, masks)) = with_bit_count_mask_repair(current!(), &source)
    {
        defaulted.push(DIAGNOSTIC_BIT_COUNT_MASKS.into());
        reconciled.extend(masks);
        owned = Some(out);
    }
    if r.defined_length_fragments
        && let Some(out) = defined_length_fragments(current!(), &source)
    {
        defaulted.push(DIAGNOSTIC_DEFINED_LENGTH_FRAGMENTS.into());
        owned = Some(out);
    }
    if let Some((out, legacy)) = with_legacy_image_recovery(current!(), &source, r) {
        defaulted.extend(legacy);
        owned = Some(out);
    }
    if r.repair_photometric_interpretation
        && let Some((out, repairs)) = with_photometric_recovery(current!(), &source, limits)?
    {
        defaulted.extend(repairs);
        owned = Some(out);
    }
    if r.unwrap_jp2_container
        && let Some(out) = unwrap_jp2(current!(), &source)
    {
        defaulted.push(DIAGNOSTIC_JP2_CONTAINER_UNWRAPPED.into());
        owned = Some(out);
    }
    if r.native_pixel_data_under_encapsulated && native_under_encapsulated(current!(), &source) {
        source = canonical_source(UID_EXPLICIT_LE)?;
        defaulted.push(DIAGNOSTIC_NATIVE_UNDER_ENCAPSULATED.into());
    }
    if r.reconcile_with_codestream
        && let Some((out, recs)) = reconcile_with_codestream(current!(), &source)
    {
        reconciled.extend(recs);
        defaulted.push(DIAGNOSTIC_RECONCILED_WITH_CODESTREAM.into());
        owned = Some(out);
    }
    let (lossy, input, metadata) = {
        let ds = current!();
        let pixel = extract_view(ds)?;
        let lossy = detect_lossy_state_crate(ds, &source.uid);
        let Some(input) = input_bytes(&pixel) else {
            return Err(limit_error(
                "MaxInputBytes",
                i64::MAX,
                limits.max_input_bytes,
            ));
        };
        let metadata = match extract_metadata(ds) {
            Ok(m) => m,
            Err(e) => {
                if input > limits.max_input_bytes {
                    return Err(limit_error("MaxInputBytes", input, limits.max_input_bytes));
                }
                return Err(e.into());
            }
        };
        check_admission(limits, &metadata, &source, input)?;
        (lossy, input, metadata)
    };
    let mut metadata = metadata;
    if let Some((out, m, repairs)) = with_legacy_pixel_repairs(current!(), &source, &metadata, r)? {
        defaulted.extend(repairs);
        metadata = m;
        owned = Some(out);
    }
    let mut incomplete_report = None;
    if let Some(info) = incomplete {
        let (out, found) =
            normalize_incomplete_pixel_data(current!(), &source, &metadata, info, limits)?;
        owned = Some(out);
        metadata = extract_metadata(current!())?;
        incomplete_report = Some(found);
        defaulted.push(DIAGNOSTIC_INCOMPLETE_PIXEL_DATA.into());
    }
    let ds = current!();
    let pixel = extract_view(ds)?;
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
    let mut codec = r.codec_recovery();
    codec.jpeg_color = color;
    let frames = registry
        .unwrap_or(DecoderRegistry::Builtin)
        .decode_frames_with_recovery(&source.uid, &pixel, ds, &codec)?;
    let diagnostics: Vec<String> = frames.diagnostics.iter().map(|d| d.to_string()).collect();
    let mut out_meta = output_metadata(&metadata, &source, &frames);
    let mut frames = little_endian_frames(&out_meta, &source, frames)?;
    if r.convert_retired_color_models
        && let Some((converted, m, code)) = retired_color_frames(ds, &source, &frames, &out_meta)?
    {
        frames = converted;
        out_meta = m;
        defaulted.push(code.into());
    }
    let raw = native_frame_bytes(&out_meta, &frames)?;
    let raw_len = raw.len() as i64;
    let mut output = with_native_pixel_data(ds, &out_meta, raw, &source)?;
    if metadata.photometric_interpretation != out_meta.photometric_interpretation
        && r.convert_retired_color_models
        && is_retired_color(&metadata.photometric_interpretation)
    {
        remove_retired_color_attributes(&mut output);
    }
    stamp_lossy_state(&mut output, &lossy, raw_len, input);
    defaulted.extend(diagnostics);
    Ok((
        output,
        DecompressReport {
            incomplete: incomplete_report,
            diagnostics: defaulted,
            reconciliations: reconciled,
        },
    ))
}
