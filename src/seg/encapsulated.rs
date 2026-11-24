#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::read::{read_document_header, read_document_pixels};
use super::{
    Document, Error, Kind, LABEL_MAP_SEGMENTATION_STORAGE, ReadOptions, SEGMENTATION_STORAGE,
    SEGMENTATION_TYPE_BINARY, SEGMENTATION_TYPE_FRACTIONAL, SEGMENTATION_TYPE_LABEL_MAP,
    TAG_PER_FRAME_FUNCTIONAL_GROUPS,
};
use crate::derivedio as dio;
use crate::object::{Object, Tag, Value, Vr};
use crate::pixel::Metadata;
use crate::pixel::encapsulated::{self as encap, TableAttribute, TableValue};
use crate::pixel::pixeldata::{self as pd, PixelData};
use crate::pixel::transcode::{
    DecoderRegistry, DecompressLimits, DecompressOptions, ErrorKind as TranscodeKind,
    decompress_data_set, default_decompress_limits,
};
use crate::render::parallel::Cancellation;
use crate::transfer;
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const ERR_DECOMPRESS_LIMIT: &str = "dicom: decompression resource limit exceeded";
const ERR_INVALID_DECOMPRESS_LIMITS: &str = "dicom: invalid decompression limits";
const ERR_INCOMPATIBLE_PIXEL_DATA: &str = "dicom: pixel data is incompatible with transfer syntax";
const ERR_CODEC_REGISTRY_NIL: &str = "dicom: pixel data codec registry is nil";
const ERR_FRAME_COUNT: &str = "dicom: encapsulated frame count mismatch";
#[derive(Debug, Clone, Default)]
pub struct PixelDataReadOptions {
    pub transfer_syntax: String,
    pub registry: Option<DecoderRegistry>,
    pub limits: DecompressLimits,
}
fn context_error(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx.and_then(|c| c.err()) {
        Some(e) => Err(Error::other(e)),
        None => Ok(()),
    }
}
pub fn read_context(
    ctx: Option<&dyn Cancellation>,
    obj: &Object,
    opts: &PixelDataReadOptions,
) -> Result<Document, Error> {
    context_error(ctx)?;
    let limits = seg_read_limits(opts.limits)?;
    let source = match transfer::default_registry().get(&opts.transfer_syntax) {
        Some(s) if !s.uid.is_empty() => s,
        _ => {
            return Err(Error::wrap(
                Kind::UnsupportedPixelData,
                "explicit known transfer syntax required",
            ));
        }
    };
    let doc = read_document_header(obj)?;
    let metadata = strict_seg_metadata(obj, &doc, &limits, source.encapsulated)?;
    let pixel = pd::extract_view(obj).map_err(|e| Error::wrap(Kind::InvalidObject, e))?;
    let encapsulated = matches!(pixel, PixelData::Encapsulated { .. });
    if source.encapsulated != encapsulated {
        return Err(Error::wrap(
            Kind::UnsupportedPixelData,
            ERR_INCOMPATIBLE_PIXEL_DATA,
        ));
    }
    let decoded;
    let mut source_obj = obj;
    match pixel {
        PixelData::Native(raw) => {
            match source.uid.as_ref() {
                "1.2.840.10008.1.2"
                | "1.2.840.10008.1.2.1"
                | "1.2.840.10008.1.2.2"
                | "1.2.840.10008.1.2.1.99" => {}
                _ => return Err(Error::sentinel(Kind::UnsupportedPixelData)),
            }
            let len = raw.len() as i64;
            if len > limits.max_input_bytes {
                return Err(seg_read_limit("MaxInputBytes", len, limits.max_input_bytes));
            }
            let mut expected = pd::total_size(&metadata);
            if metadata.bits_allocated == 1 {
                expected = (i64::from(metadata.rows)
                    * i64::from(metadata.columns)
                    * metadata.number_of_frames
                    + 7)
                    / 8;
            }
            if len < expected {
                return Err(Error::sentinel(Kind::TruncatedPixelData));
            }
            if len > expected + (expected & 1) {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    "native Pixel Data exceeds declared frames",
                ));
            }
        }
        PixelData::Encapsulated {
            offset_table,
            fragments,
        } => {
            match source.uid.as_ref() {
                "1.2.840.10008.1.2.8.1" => {
                    if doc.segmentation_type != SEGMENTATION_TYPE_BINARY
                        || doc.sop_class_uid != SEGMENTATION_STORAGE
                    {
                        return Err(Error::wrap(
                            Kind::UnsupportedPixelData,
                            "Deflated Image Frame profile requires BINARY",
                        ));
                    }
                }
                "1.2.840.10008.1.2.5" => {
                    if doc.segmentation_type != SEGMENTATION_TYPE_FRACTIONAL
                        && doc.segmentation_type != SEGMENTATION_TYPE_LABEL_MAP
                    {
                        return Err(Error::wrap(
                            Kind::UnsupportedPixelData,
                            "RLE profile requires FRACTIONAL or LABELMAP",
                        ));
                    }
                }
                _ => {
                    return Err(Error::wrap(
                        Kind::UnsupportedPixelData,
                        "transfer syntax outside the lossless SEG profile",
                    ));
                }
            }
            validate_seg_fragments(ctx, obj, offset_table, fragments, &metadata, &limits)?;
            let Some(registry) = opts.registry else {
                return Err(Error::wrap(
                    Kind::UnsupportedPixelData,
                    ERR_CODEC_REGISTRY_NIL,
                ));
            };
            let out = decompress_data_set(
                obj,
                &source.uid,
                &DecompressOptions {
                    registry: Some(registry),
                    target_transfer_syntax: String::new(),
                    limits,
                },
            );
            match out {
                Ok((o, _)) => decoded = o,
                Err(e) => {
                    if e.is(TranscodeKind::DecompressResourceLimit) {
                        return Err(Error::wrap(Kind::ResourceLimitExceeded, e));
                    }
                    return Err(Error::other(e));
                }
            }
            source_obj = &decoded;
        }
    }
    context_error(ctx)?;
    let (doc, _) = read_document_pixels(source_obj, doc, ReadOptions::default(), true)?;
    context_error(ctx)?;
    validate_read_samples(ctx, &doc)?;
    Ok(doc)
}
fn seg_read_limits(l: DecompressLimits) -> Result<DecompressLimits, Error> {
    if l.max_frames < 0
        || l.max_pixels < 0
        || l.max_native_bytes < 0
        || l.max_input_bytes < 0
        || l.max_expansion_ratio < 0
    {
        return Err(Error::other(ERR_INVALID_DECOMPRESS_LIMITS));
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
fn seg_read_limit(name: &str, value: i64, maximum: i64) -> Error {
    Error::wrap(
        Kind::ResourceLimitExceeded,
        format!("{ERR_DECOMPRESS_LIMIT}: {name} actual={value} maximum={maximum}"),
    )
}
fn strict_seg_metadata(
    obj: &Object,
    doc: &Document,
    l: &DecompressLimits,
    frame_local_bits: bool,
) -> Result<Metadata, Error> {
    let frames = match obj.get_int(dio::TAG_NUMBER_OF_FRAMES) {
        Ok(f) if f > 0 => f,
        _ => {
            return Err(Error::wrap(Kind::InvalidObject, "invalid Number of Frames"));
        }
    };
    if frames > l.max_frames {
        return Err(seg_read_limit("MaxFrames", frames, l.max_frames));
    }
    if dio::sequence(obj, TAG_PER_FRAME_FUNCTIONAL_GROUPS).len() as i64 != frames {
        return Err(Error::wrap(
            Kind::InvalidObject,
            "Number of Frames differs from Functional Groups",
        ));
    }
    let pixels = doc.rows * doc.columns;
    if pixels > l.max_pixels / frames {
        return Err(seg_read_limit("MaxPixels", pixels * frames, l.max_pixels));
    }
    let mut bits = 1i64;
    if doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
        bits = 8;
    }
    if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        bits = doc.label_map_bits_allocated;
    }
    for (tag, want) in [
        (dio::TAG_BITS_ALLOCATED, bits),
        (dio::TAG_BITS_STORED, bits),
        (dio::TAG_HIGH_BIT, bits - 1),
        (dio::TAG_PIXEL_REPRESENTATION, 0),
        (dio::TAG_SAMPLES_PER_PIXEL, 1),
    ] {
        match obj.get_int(tag) {
            Ok(got) if got == want => {}
            _ => {
                return Err(Error::wrap(
                    Kind::UnsupportedPixelData,
                    "inconsistent Image Pixel Module",
                ));
            }
        }
    }
    let photometric = dio::clean_string(obj, TAG_PHOTOMETRIC);
    if photometric != "MONOCHROME2"
        && !(doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE && photometric == "PALETTE COLOR")
    {
        return Err(Error::wrap(
            Kind::UnsupportedPixelData,
            "incompatible Photometric Interpretation",
        ));
    }
    let mut native = (pixels * bits + 7) / 8 * frames;
    if bits == 1 && !frame_local_bits {
        native = (pixels * frames + 7) / 8;
    }
    if native > l.max_native_bytes {
        return Err(seg_read_limit("MaxNativeBytes", native, l.max_native_bytes));
    }
    pd::extract_metadata(obj).map_err(Error::other)
}
fn table_attribute(obj: &Object, tag: Tag) -> Option<TableAttribute<'_>> {
    let e = obj.get(tag)?;
    Some(TableAttribute {
        vr_ov: e.vr() == Vr::OV,
        value: match &e.value {
            Value::Raw(r) => TableValue::Raw(r),
            _ => TableValue::Unsupported("core.Value"),
        },
    })
}
fn validate_seg_fragments(
    ctx: Option<&dyn Cancellation>,
    obj: &Object,
    offset_table: &[u8],
    fragments: &[Vec<u8>],
    m: &Metadata,
    l: &DecompressLimits,
) -> Result<(), Error> {
    let frames = m.number_of_frames;
    if fragments.len() as i64 != frames {
        return Err(Error::wrap(Kind::InvalidEncapsulation, ERR_FRAME_COUNT));
    }
    let mut input = offset_table.len() as i64;
    if input > l.max_input_bytes {
        return Err(seg_read_limit("MaxInputBytes", input, l.max_input_bytes));
    }
    if !offset_table.is_empty() && offset_table.len() as i64 != 4 * frames {
        return Err(Error::wrap(
            Kind::InvalidEncapsulation,
            "invalid Basic Offset Table count",
        ));
    }
    let tables = encap::read_tables(
        offset_table,
        table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE),
        table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
        frames,
    )
    .map_err(|e| Error::wrap(Kind::InvalidEncapsulation, e))?;
    if tables.extended_present != tables.lengths_present
        || tables.extended_present && !tables.basic.is_empty()
    {
        return Err(Error::wrap(
            Kind::InvalidEncapsulation,
            "inconsistent Extended Offset Table",
        ));
    }
    let mut position = 0u64;
    for (i, fragment) in fragments.iter().enumerate() {
        context_error(ctx)?;
        let size = fragment.len() as u64;
        if size < 2 || size & 1 != 0 || size > u64::from(u32::MAX) - 1 {
            return Err(Error::wrap(
                Kind::InvalidEncapsulation,
                "invalid frame Item length",
            ));
        }
        if size as i64 > l.max_input_bytes - input {
            return Err(seg_read_limit(
                "MaxInputBytes",
                input + size as i64,
                l.max_input_bytes,
            ));
        }
        input += size as i64;
        if !tables.basic.is_empty() {
            let at = &tables.basic[4 * i..4 * i + 4];
            if u64::from(u32::from_le_bytes([at[0], at[1], at[2], at[3]])) != position {
                return Err(Error::wrap(
                    Kind::InvalidEncapsulation,
                    "Basic Offset Table does not match frame Items",
                ));
            }
        }
        if tables.extended_present {
            let length = tables.lengths[i];
            if tables.extended[i] != position
                || length == 0
                || length > size
                || size - length > 1
                || size != length && (length & 1 == 0 || fragment[fragment.len() - 1] != 0)
            {
                return Err(Error::wrap(
                    Kind::InvalidEncapsulation,
                    "Extended Offset Table does not match frame Items",
                ));
            }
        }
        position += 8 + size;
    }
    let native = pd::total_size(m);
    let mut ratio = native / input;
    if native % input != 0 {
        ratio += 1;
    }
    if ratio > l.max_expansion_ratio {
        return Err(seg_read_limit(
            "MaxExpansionRatio",
            ratio,
            l.max_expansion_ratio,
        ));
    }
    Ok(())
}
fn validate_read_samples(ctx: Option<&dyn Cancellation>, doc: &Document) -> Result<(), Error> {
    let described: std::collections::HashSet<i64> = doc.segments.iter().map(|s| s.number).collect();
    for frame in &doc.frames {
        context_error(ctx)?;
        for &sample in &frame.fractional {
            if i64::from(sample) > doc.max_fractional_value {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    "fractional sample exceeds declared maximum",
                ));
            }
        }
        for &label in &frame.label_map {
            if !described.contains(&i64::from(label)) {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    "label has no described segment",
                ));
            }
        }
    }
    context_error(ctx)
}
