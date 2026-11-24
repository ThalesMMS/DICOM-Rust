use super::super::Metadata;
use super::super::encapsulated::{self, Format, Limits, Recovery, TableAttribute, TableValue};
use super::super::pixeldata::{
    PixelData, PixelErrorKind, extract_metadata, extract_native_frames, extract_view, frame_size,
};
use super::decompress::{DecodeRecovery, DecompressLimits, decompress_for_transcode};
use super::{
    DecoderRegistry, Error, ErrorKind, PlanarOutput, TAG_ENCAPSULATED_TOTAL_LENGTH,
    TAG_EXTENDED_OFFSET_TABLE, TAG_EXTENDED_OFFSET_TABLE_LENGTHS, TAG_PIXEL_DATA,
    TAG_PLANAR_CONFIGURATION, TAG_SOP_CLASS_UID, TAG_SOP_INSTANCE_UID, TranscodeLimits,
    TranscodeOptions, TranscodeReport, UID_ENCAPSULATED_UNCOMPRESSED, UID_EXPLICIT_LE, UidPolicy,
    canonical_source, canonical_target, encode, fragments_element, has_nested_pixel_data, measure,
    object_from, raw_element, replace_element, resolve_limits, strings_element,
    transcode_top_recovering, uint16_element,
};
use crate::derivation::{self, Derivation};
use crate::display;
use crate::object::frame_writer;
use crate::object::{DataSet, Element, Object, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::transfer::Syntax;
const TAG_ICON_IMAGE_SEQUENCE: Tag = Tag::new(0x0088, 0x0200);
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const MAX_ITEM: i64 = u32::MAX as i64 - 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconAction {
    PreservedNative,
    Decompressed,
    Compressed,
    PreservedEncapsulated,
    Absent,
}
impl IconAction {
    pub fn as_str(self) -> &'static str {
        match self {
            IconAction::PreservedNative => "preserved-native",
            IconAction::Decompressed => "decompressed",
            IconAction::Compressed => "compressed",
            IconAction::PreservedEncapsulated => "preserved-encapsulated",
            IconAction::Absent => "absent",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct IconReport {
    pub path: String,
    pub action: IconAction,
    pub metadata_incomplete: bool,
    pub native_bytes: i64,
    pub encoded_bytes: i64,
    pub decode_diagnostics: Vec<String>,
    pub encode_compatibility_profiles: Vec<String>,
}
fn path_string(path: &[(Tag, usize)]) -> String {
    path.iter()
        .map(|(t, i)| format!("{t}[{i}]"))
        .collect::<Vec<_>>()
        .join("/")
}
pub(super) fn explicit_encapsulation(o: &TranscodeOptions) -> bool {
    o.max_fragment_bytes != 0 || o.offset_table != frame_writer::offset_table::AUTO
}
pub(super) fn jpeg2000_part2(uid: &str) -> bool {
    uid == "1.2.840.10008.1.2.4.92" || uid == "1.2.840.10008.1.2.4.93"
}
pub(super) fn validate_output_policy(o: &TranscodeOptions) -> Result<(), Error> {
    encode::validate_uid_root(&o.uid_root)?;
    if !o.uid_root.is_empty() && o.uid_policy == UidPolicy::Preserve {
        return Err(Error::unsupported(
            "UID root conflicts with preserved SOP identity",
        ));
    }
    let f = o.max_fragment_bytes;
    if !(0..=MAX_ITEM).contains(&f) || f != 0 && (f < 2 || f % 2 != 0) {
        return Err(Error::unsupported(
            "fragment cap must be zero or an even Item length",
        ));
    }
    if o.offset_table > frame_writer::offset_table::EMPTY {
        return Err(Error::unsupported("unknown output policy"));
    }
    if o.secondary_capture && o.uid_policy == UidPolicy::Preserve {
        return Err(Error::unsupported("Secondary Capture requires a new UID"));
    }
    Ok(())
}
pub(crate) fn validate_output_options(
    dataset: &Object,
    target: &str,
    o: &TranscodeOptions,
) -> Result<(), Error> {
    validate_output_policy(o)?;
    let target = canonical_target(target)?;
    if !target.encapsulated && explicit_encapsulation(o) {
        return Err(Error::unsupported(
            "encapsulation options require an encapsulated target",
        ));
    }
    if target.encapsulated && o.planar_output != PlanarOutput::Default {
        return Err(Error::unsupported(
            "planar output policy requires a native target",
        ));
    }
    if jpeg2000_part2(&target.uid) && (explicit_encapsulation(o) || o.compress_icons) {
        return Err(Error::unsupported(
            "Annex J output requires one collection with empty offset tables and native icons",
        ));
    }
    if o.secondary_capture {
        return validate_secondary_capture(dataset);
    }
    Ok(())
}
fn sc_error(detail: &str) -> Error {
    Error::unsupported(detail)
}
fn validate_secondary_capture(obj: &Object) -> Result<(), Error> {
    let m = extract_metadata(obj)?;
    let class = obj.get_string(TAG_SOP_CLASS_UID).unwrap_or_default();
    let entry = crate::dictionary::uid::lookup_storage_sop_class(&class);
    let classic = entry.is_some_and(|e| {
        e.category == crate::dictionary::uid::StorageCategory::Image
            && !e.keyword.contains("Enhanced")
            && !e.keyword.contains("MultiFrame")
    });
    if !classic
        || obj.has(Tag::new(0x5200, 0x9229))
        || obj.has(Tag::new(0x5200, 0x9230))
        || m.number_of_frames != 1
    {
        return Err(sc_error(
            "Secondary Capture requires a classic single-frame Image IOD",
        ));
    }
    let p = m.photometric_interpretation.as_str();
    if m.bits_allocated != 8 && m.bits_allocated != 16
        || m.samples_per_pixel == 1
            && p != "MONOCHROME1"
            && p != "MONOCHROME2"
            && p != "PALETTE COLOR"
        || m.samples_per_pixel == 3
            && (m.bits_allocated != 8
                || m.pixel_representation != 0
                || p != "RGB" && p != "YBR_FULL" && p != "YBR_FULL_422")
        || m.samples_per_pixel != 1 && m.samples_per_pixel != 3
    {
        return Err(sc_error("Secondary Capture pixel representation"));
    }
    for tag in [
        TAG_SOP_CLASS_UID,
        TAG_SOP_INSTANCE_UID,
        Tag::new(0x0020, 0x000d),
        Tag::new(0x0020, 0x000e),
    ] {
        let value = obj.get_string(tag).unwrap_or_default();
        let value = crate::transfer::normalize_uid(&value);
        if !crate::object::is_valid_uid(value.as_bytes()) {
            return Err(sc_error(
                "Secondary Capture requires valid source, study and series UIDs",
            ));
        }
    }
    if p == "PALETTE COLOR" {
        let palette = match display::palette_from_object(obj, display::LutOptions::default()) {
            Ok(Some(palette)) => palette,
            _ => return Err(sc_error("Secondary Capture requires a valid palette")),
        };
        let (Some(red), Some(green), Some(blue)) = (&palette.red, &palette.green, &palette.blue)
        else {
            return Err(sc_error("Secondary Capture requires a valid palette"));
        };
        for c in [red, green, blue] {
            if c.entries.len() != red.entries.len()
                || c.first_mapped != red.first_mapped
                || c.bits_per_entry != red.bits_per_entry
                || c.bits_per_entry != 8 && c.bits_per_entry != 16
            {
                return Err(sc_error("Secondary Capture palette descriptors disagree"));
            }
        }
    }
    Ok(())
}
pub(super) fn output_planar(
    obj: &Object,
    m: &Metadata,
    policy: PlanarOutput,
) -> Result<u16, Error> {
    if policy == PlanarOutput::Default || m.samples_per_pixel == 1 {
        return Ok(m.planar_configuration);
    }
    let p = m.photometric_interpretation.as_str();
    if m.samples_per_pixel != 3
        || m.bits_allocated != 8 && m.bits_allocated != 16
        || m.planar_configuration > 1
        || p != "RGB" && p != "YBR_FULL"
    {
        return Err(Error::unsupported(
            "planar output requires full-resolution RGB or YBR_FULL samples",
        ));
    }
    match policy {
        PlanarOutput::Planar => return Ok(1),
        PlanarOutput::Interleaved => return Ok(0),
        _ => {}
    }
    let class = obj.get_string(TAG_SOP_CLASS_UID).unwrap_or_default();
    let class = crate::transfer::normalize_uid(&class);
    if class == "1.2.840.10008.5.1.1.30"
        || p == "YBR_FULL"
            && (class == "1.2.840.10008.5.1.4.1.1.6.1" || class == "1.2.840.10008.5.1.4.1.1.3.1")
    {
        return Ok(1);
    }
    Ok(0)
}
pub(super) fn reorder_frame(frame: &[u8], m: &Metadata, target: u16) -> Result<Vec<u8>, Error> {
    if target > 1 || m.planar_configuration > 1 || m.bits_allocated != 8 && m.bits_allocated != 16 {
        return Err(Error::new(
            ErrorKind::Other,
            "dicom: native pixel data rewrite unsupported: planar sample layout",
        ));
    }
    if frame.len() as i64 != frame_size(m) {
        return Err(Error::pixel_bare(PixelErrorKind::PixelDataSizeMismatch));
    }
    if m.samples_per_pixel == 1 || m.planar_configuration == target {
        return Ok(frame.to_vec());
    }
    let samples = usize::from(m.samples_per_pixel);
    let pixels = usize::from(m.rows).saturating_mul(usize::from(m.columns));
    let width = usize::from(m.bits_allocated) / 8;
    let mut out = vec![0u8; frame.len()];
    for i in 0..pixels {
        for c in 0..samples {
            let interleaved = samples
                .saturating_mul(i)
                .saturating_add(c)
                .saturating_mul(width);
            let planar = c
                .saturating_mul(pixels)
                .saturating_add(i)
                .saturating_mul(width);
            let (to, from) = if target == 1 {
                (planar, interleaved)
            } else {
                (interleaved, planar)
            };
            let (Some(dst), Some(src)) = (
                out.get_mut(to..to.saturating_add(width)),
                frame.get(from..from.saturating_add(width)),
            ) else {
                return Err(Error::pixel_bare(PixelErrorKind::PixelDataSizeMismatch));
            };
            dst.copy_from_slice(src);
        }
    }
    Ok(out)
}
pub(super) fn put_planar(obj: &mut Object, target: u16) {
    let raw = if obj.value_big_endian() {
        target.to_be_bytes()
    } else {
        target.to_le_bytes()
    };
    obj.put(raw_element(TAG_PLANAR_CONFIGURATION, Vr::US, raw.to_vec()));
}
pub(super) fn apply_native_output_planar(
    obj: &mut Object,
    policy: PlanarOutput,
) -> Result<(), Error> {
    if policy == PlanarOutput::Default {
        return Ok(());
    }
    let m = extract_metadata(obj)?;
    let target = output_planar(obj, &m, policy)?;
    if m.samples_per_pixel == 1 {
        return Ok(());
    }
    if target != m.planar_configuration {
        let frames = extract_native_frames(obj)?;
        let mut data = Vec::new();
        for f in &frames.data {
            data.extend(reorder_frame(f, &m, target)?);
        }
        let vr = obj.get(TAG_PIXEL_DATA).map(Element::vr).unwrap_or(Vr::OB);
        obj.put(raw_element(TAG_PIXEL_DATA, vr, data));
    }
    put_planar(obj, target);
    Ok(())
}
fn frame_format(uid: &str) -> Option<Format> {
    match uid {
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.57"
        | "1.2.840.10008.1.2.4.70" => Some(Format::JPEG),
        "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => Some(Format::JPEGLS),
        "1.2.840.10008.1.2.4.90"
        | "1.2.840.10008.1.2.4.91"
        | "1.2.840.10008.1.2.4.201"
        | "1.2.840.10008.1.2.4.202"
        | "1.2.840.10008.1.2.4.203" => Some(Format::JPEG2000),
        _ => None,
    }
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
fn raw_frames(obj: &Object, uid: &str, limits: &TranscodeLimits) -> Result<Vec<Vec<u8>>, Error> {
    let m = extract_metadata(obj)?;
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = extract_view(obj)?
    else {
        return Err(Error::pixel_bare(PixelErrorKind::IncompatiblePixelData));
    };
    let layout = |detail: String| {
        Error::new(
            ErrorKind::Other,
            format!("dicom: frames cannot be located in Pixel Data: {detail}"),
        )
    };
    if let Some(format) = frame_format(uid) {
        let refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
        let plan = encapsulated::from_fragments(
            offset_table,
            &refs,
            crate::pixel::encapsulated::EotAttributes {
                extended: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE),
                lengths: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
            },
            m.number_of_frames,
            format,
            Limits {
                max_frames: limits.max_frames,
                max_fragments: limits.max_fragments,
                ..Limits::default()
            },
            Recovery::default(),
        )
        .map_err(|e| layout(e.to_string()))?;
        return (0..plan.len())
            .map(|i| {
                plan.frame(i)
                    .map(|v| v.data.into_owned())
                    .map_err(|e| layout(e.to_string()))
            })
            .collect();
    }
    if fragments.len() as i64 != m.number_of_frames {
        return Err(layout(format!(
            "{} fragments for {} frames",
            fragments.len(),
            m.number_of_frames
        )));
    }
    Ok(fragments.to_vec())
}
pub(crate) fn apply_materialized_output_options(
    source: &Object,
    result: &mut Object,
    target: &str,
    o: &TranscodeOptions,
    report: &mut TranscodeReport,
) -> Result<(), Error> {
    if o.planar_output == PlanarOutput::Default
        && !explicit_encapsulation(o)
        && !o.secondary_capture
        && o.uid_policy == UidPolicy::Default
    {
        return Ok(());
    }
    let limits = resolve_limits(o.limits)?;
    apply_native_output_planar(result, o.planar_output)?;
    if o.planar_output != PlanarOutput::Default {
        report.pixel_data_preserved = false;
    }
    if explicit_encapsulation(o) {
        let target = canonical_target(target)?;
        let frames = raw_frames(result, &target.uid, &limits)?;
        let lengths: Vec<u64> = frames.iter().map(|f| f.len() as u64).collect();
        let layout = frame_writer::plan(
            &lengths,
            o.max_fragment_bytes as u64,
            o.offset_table,
            limits.max_fragments,
            frame_writer::single_fragment(&target.uid),
        )
        .map_err(|e| {
            if e == "fragment count exceeds limit" {
                Error::limit("fragments")
            } else {
                Error::unsupported(format!("output fragment layout: {e}"))
            }
        })?;
        let mut fragments = Vec::new();
        for frame in &frames {
            let mut rest: &[u8] = frame;
            while !rest.is_empty() {
                let n =
                    frame_writer::fragment_length(rest.len() as u64, o.max_fragment_bytes as u64)
                        as usize;
                let (fragment, after) = rest.split_at(n.clamp(1, rest.len()));
                fragments.push(fragment.to_vec());
                rest = after;
            }
        }
        result.remove(TAG_EXTENDED_OFFSET_TABLE);
        result.remove(TAG_EXTENDED_OFFSET_TABLE_LENGTHS);
        result.remove(TAG_ENCAPSULATED_TOTAL_LENGTH);
        result.put(fragments_element(layout.basic, fragments));
        if let Some((extended, lengths)) = layout.extended {
            result.put(raw_element(TAG_EXTENDED_OFFSET_TABLE, Vr::OV, extended));
            result.put(raw_element(
                TAG_EXTENDED_OFFSET_TABLE_LENGTHS,
                Vr::OV,
                lengths,
            ));
        }
    }
    apply_identity(source, result, report, o)?;
    measure(result, &limits)?;
    Ok(())
}
pub(super) fn apply_identity(
    source: &Object,
    result: &mut Object,
    report: &mut TranscodeReport,
    o: &TranscodeOptions,
) -> Result<(), Error> {
    if o.uid_policy == UidPolicy::Preserve && report.lossy {
        return Err(Error::unsupported(
            "a new lossy generation requires a new UID",
        ));
    }
    if !o.secondary_capture && o.uid_policy != UidPolicy::Always {
        return Ok(());
    }
    report.identity_changed = true;
    let mut step = Derivation {
        description: "New SOP Instance created by transfer syntax conversion".into(),
        codes: Vec::new(),
        sources: Vec::new(),
    };
    if let Ok(p) = derivation::source_of(
        Some(source),
        Some(derivation::codes::SourceImageForProcessing()),
    ) {
        step.sources.push(p);
    }
    if o.secondary_capture {
        validate_secondary_capture(result)?;
        step.description = "Converted to single-frame Secondary Capture".into();
        result.put(strings_element(
            TAG_SOP_CLASS_UID,
            Vr::UI,
            vec!["1.2.840.10008.5.1.4.1.1.7".into()],
        ));
        result.put(strings_element(
            Tag::new(0x0008, 0x0064),
            Vr::CS,
            vec!["WSD".into()],
        ));
        result.put(strings_element(
            TAG_IMAGE_TYPE,
            Vr::CS,
            vec!["DERIVED".into(), "SECONDARY".into()],
        ));
        result.remove(TAG_NUMBER_OF_FRAMES);
        result.remove(Tag::new(0x0028, 0x0009));
        for (tag, vr) in [
            (Tag::new(0x0010, 0x0010), Vr::PN),
            (Tag::new(0x0010, 0x0020), Vr::LO),
            (Tag::new(0x0010, 0x0030), Vr::DA),
            (Tag::new(0x0010, 0x0040), Vr::CS),
            (Tag::new(0x0008, 0x0020), Vr::DA),
            (Tag::new(0x0008, 0x0030), Vr::TM),
            (Tag::new(0x0008, 0x0090), Vr::PN),
            (Tag::new(0x0020, 0x0010), Vr::SH),
            (Tag::new(0x0008, 0x0050), Vr::SH),
            (Tag::new(0x0020, 0x0011), Vr::IS),
            (Tag::new(0x0020, 0x0013), Vr::IS),
            (Tag::new(0x0020, 0x0020), Vr::CS),
            (Tag::new(0x0008, 0x0023), Vr::DA),
            (Tag::new(0x0008, 0x0033), Vr::TM),
            (Tag::new(0x0008, 0x002a), Vr::DT),
        ] {
            if !result.has(tag) {
                result.put(strings_element(tag, vr, Vec::new()));
            }
        }
    }
    derivation::apply_to(Some(result), &step)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    if !report.lossy {
        let uid = encode::new_uid(&o.uid_root).map_err(|_| {
            Error::stage(
                "uid",
                Error::new(ErrorKind::Transaction, super::ERR_TRANSACTION),
            )
        })?;
        result.put(strings_element(TAG_SOP_INSTANCE_UID, Vr::UI, vec![uid]));
    }
    Ok(())
}
pub(crate) fn move_embedded_overlays(
    decoded: &Object,
    extract: bool,
) -> Result<(Object, i64), Error> {
    let Some(native) = decoded.get(TAG_PIXEL_DATA).and_then(Element::raw) else {
        return Ok((decoded.clone(), 0));
    };
    let base = "dicom: transcode would drop overlays embedded in pixel data";
    let (overlays, err) = display::embedded_overlays(decoded, native, false);
    if let Some(err) = err {
        return Err(Error::new(
            ErrorKind::EmbeddedOverlay,
            format!("{base}: {err}"),
        ));
    }
    if overlays.is_empty() {
        return Ok((decoded.clone(), 0));
    }
    if !extract {
        return Err(Error::new(
            ErrorKind::EmbeddedOverlay,
            format!(
                "{base}: {} overlay group(s); set TranscodeOptions.ExtractEmbeddedOverlays",
                overlays.len()
            ),
        ));
    }
    let m = extract_metadata(decoded)?;
    let width = usize::from(m.bits_allocated) / 8;
    let mut pixels = native.to_vec();
    let mut clear: u16 = 0;
    for o in &overlays {
        clear |= 1u16.wrapping_shl(o.bit_position as u32);
    }
    let mut offset = 0usize;
    while width > 0 && offset.saturating_add(width) <= pixels.len() {
        if width == 2 {
            if let Some(pair) = pixels
                .get_mut(offset..)
                .and_then(|p| p.first_chunk_mut::<2>())
            {
                let v = u16::from_le_bytes(*pair) & !clear;
                *pair = v.to_le_bytes();
            }
        } else if let Some(b) = pixels.get_mut(offset) {
            *b &= !(clear as u8);
        }
        offset = offset.saturating_add(width);
    }
    let vr = if m.bits_allocated > 8 { Vr::OW } else { Vr::OB };
    let mut elements = decoded.elements().to_vec();
    replace_element(&mut elements, raw_element(TAG_PIXEL_DATA, vr, pixels));
    for o in &overlays {
        let mut data = o.data.clone();
        if data.len() % 2 != 0 {
            data.push(0);
        }
        let g = o.group;
        replace_element(
            &mut elements,
            uint16_element(Tag::new(g, 0x0010), o.rows as u16),
        );
        replace_element(
            &mut elements,
            uint16_element(Tag::new(g, 0x0011), o.columns as u16),
        );
        replace_element(&mut elements, uint16_element(Tag::new(g, 0x0100), 1));
        replace_element(&mut elements, uint16_element(Tag::new(g, 0x0102), 0));
        replace_element(
            &mut elements,
            strings_element(
                Tag::new(g, 0x0015),
                Vr::IS,
                vec![o.frame_count().to_string()],
            ),
        );
        replace_element(
            &mut elements,
            uint16_element(Tag::new(g, 0x0051), o.frame_origin as u16),
        );
        replace_element(
            &mut elements,
            raw_element(Tag::new(g, 0x3000), Vr::OW, data),
        );
    }
    Ok((object_from(elements, false), overlays.len() as i64))
}
pub(super) struct IconSequence {
    path: Vec<(Tag, usize)>,
    items: Vec<DataSet>,
}
pub(super) fn icon_parts(
    dataset: &Object,
    limits: &TranscodeLimits,
) -> Result<Option<(Object, Vec<IconSequence>)>, Error> {
    if !has_nested_pixel_data(dataset, limits)? {
        return Ok(None);
    }
    let mut sequences = Vec::new();
    let stripped = strip_icons(dataset.elements(), &[], &mut sequences);
    if sequences.is_empty() {
        return Ok(None);
    }
    Ok(Some((
        object_from(stripped, dataset.value_big_endian()),
        sequences,
    )))
}
fn validate_unique(
    elements: &[Element],
    depth: i64,
    limits: &TranscodeLimits,
    nodes: &mut i64,
) -> Result<(), Error> {
    if depth > limits.max_depth {
        return Err(Error::limit("depth"));
    }
    if elements.len() as i64 > limits.max_elements.saturating_sub(*nodes) {
        return Err(Error::limit("elements"));
    }
    *nodes = (*nodes).saturating_add(elements.len() as i64);
    let mut seen = std::collections::HashSet::new();
    for e in elements {
        if !seen.insert(e.tag()) {
            return Err(Error::unsupported("duplicate data element"));
        }
        if let Value::Strings(values) = &e.value {
            if values.len() as i64 > limits.max_elements.saturating_sub(*nodes) {
                return Err(Error::limit("elements"));
            }
            *nodes = (*nodes).saturating_add(values.len() as i64);
        }
        let Value::Sequence(items) = &e.value else {
            continue;
        };
        if items.len() as i64 > limits.max_elements.saturating_sub(*nodes) {
            return Err(Error::limit("elements"));
        }
        *nodes = (*nodes).saturating_add(items.len() as i64);
        for item in items {
            validate_unique(&item.elements, depth.saturating_add(1), limits, nodes)?;
        }
    }
    Ok(())
}
pub(super) fn strip_icons(
    elements: &[Element],
    path: &[(Tag, usize)],
    out: &mut Vec<IconSequence>,
) -> Vec<Element> {
    let mut result = elements.to_vec();
    for e in result.iter_mut() {
        let Value::Sequence(items) = &e.value else {
            continue;
        };
        if e.tag() == TAG_ICON_IMAGE_SEQUENCE {
            out.push(IconSequence {
                path: path.to_vec(),
                items: items.clone(),
            });
            e.value = Value::Sequence(Vec::new());
            e.header.length = UNDEFINED_LENGTH;
            e.header.length_set = true;
            continue;
        }
        let tag = e.tag();
        let stripped: Vec<DataSet> = items
            .iter()
            .enumerate()
            .map(|(j, item)| {
                let mut next = path.to_vec();
                next.push((tag, j));
                DataSet {
                    elements: strip_icons(&item.elements, &next, out),
                    item_offset: None,
                }
            })
            .collect();
        e.value = Value::Sequence(stripped);
    }
    result
}
fn restore_icons(
    elements: &mut [Element],
    path: &[(Tag, usize)],
    sequences: &[IconSequence],
) -> Result<(), Error> {
    for e in elements.iter_mut() {
        let tag = e.tag();
        let Value::Sequence(items) = &mut e.value else {
            continue;
        };
        if tag == TAG_ICON_IMAGE_SEQUENCE {
            let key = path_string(path);
            let Some(seq) = sequences.iter().find(|s| path_string(&s.path) == key) else {
                return Err(Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED));
            };
            *items = seq.items.clone();
            continue;
        }
        for (j, item) in items.iter_mut().enumerate() {
            let mut next = path.to_vec();
            next.push((tag, j));
            restore_icons(&mut item.elements, &next, sequences)?;
        }
    }
    Ok(())
}
pub(super) fn check_icon_budget(
    main: &Object,
    sequences: &[IconSequence],
    big_endian: bool,
    limits: &TranscodeLimits,
) -> Result<(), Error> {
    let mut pixels: u64 = 0;
    let mut frames: i64 = 0;
    let mut account = |m: &Metadata| -> Result<(), Error> {
        if m.number_of_frames <= 0 || m.number_of_frames > limits.max_frames.saturating_sub(frames)
        {
            return Err(Error::limit("icon_pixels_or_frames"));
        }
        let area = u64::from(m.rows).saturating_mul(u64::from(m.columns));
        let room = limits
            .max_pixels
            .saturating_sub(pixels)
            .checked_div(area)
            .unwrap_or(u64::MAX);
        if area > 0 && m.number_of_frames as u64 > room {
            return Err(Error::limit("icon_pixels_or_frames"));
        }
        pixels = pixels.saturating_add(area.saturating_mul(m.number_of_frames as u64));
        frames = frames.saturating_add(m.number_of_frames);
        Ok(())
    };
    if let Ok(m) = extract_metadata(main) {
        account(&m)?;
    }
    for seq in sequences {
        for item in &seq.items {
            let icon = object_from(item.elements.clone(), big_endian);
            let Ok(m) = extract_metadata(&icon) else {
                continue;
            };
            if m.number_of_frames != 1 {
                return Err(Error::stage(
                    "icon",
                    Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED),
                ));
            }
            account(&m)?;
        }
    }
    Ok(())
}
pub(crate) fn transcode_with_icons_recovering(
    dataset: &Object,
    source: &str,
    target: &str,
    options: &TranscodeOptions,
    recovery: &DecodeRecovery,
) -> Result<(Object, TranscodeReport), Error> {
    let limits = resolve_limits(options.limits)?;
    if !has_nested_pixel_data(dataset, &limits)? {
        return transcode_top_recovering(dataset, source, target, options, recovery);
    }
    let mut nodes = 0;
    validate_unique(dataset.elements(), 1, &limits, &mut nodes)?;
    let mut il = limits;
    il.max_output_bytes = limits.max_input_bytes;
    measure(dataset, &il)?;
    let source_syntax = canonical_source(source)?;
    let target_syntax = canonical_target(target)?;
    let mut sequences = Vec::new();
    let stripped = strip_icons(dataset.elements(), &[], &mut sequences);
    if sequences.is_empty() {
        return transcode_top_recovering(dataset, source, target, options, recovery);
    }
    let main = object_from(stripped, dataset.value_big_endian());
    check_icon_budget(&main, &sequences, dataset.value_big_endian(), &limits)?;
    let mut reports = Vec::new();
    let mut icon_bytes: i64 = 0;
    for seq in sequences.iter_mut() {
        let mut items = Vec::with_capacity(seq.items.len());
        for (i, item) in seq.items.iter().enumerate() {
            let icon = object_from(item.elements.clone(), dataset.value_big_endian());
            let mut path = seq.path.clone();
            path.push((TAG_ICON_IMAGE_SEQUENCE, i));
            let mut icon_options = options.clone();
            icon_options.limits = limits;
            icon_options.rewrite = None;
            if icon_bytes >= limits.max_output_bytes {
                return Err(Error::limit("output_bytes"));
            }
            icon_options.limits.max_output_bytes = icon_options
                .limits
                .max_output_bytes
                .saturating_sub(icon_bytes);
            let (converted, mut report) = transcode_icon(
                &icon,
                &source_syntax,
                &target_syntax,
                &icon_options,
                recovery,
            )
            .map_err(|e| Error::stage("icon", e))?;
            let usage = measure(&converted, &limits)?;
            if usage.bytes > limits.max_output_bytes.saturating_sub(icon_bytes) {
                return Err(Error::limit("output_bytes"));
            }
            icon_bytes = icon_bytes.saturating_add(usage.bytes);
            report.path = path_string(&path);
            reports.push(report);
            items.push(DataSet {
                elements: converted.elements().to_vec(),
                item_offset: None,
            });
        }
        seq.items = items;
    }
    if icon_bytes >= limits.max_output_bytes {
        return Err(Error::limit("output_bytes"));
    }
    let mut main_options = options.clone();
    main_options.limits = limits;
    main_options.limits.max_output_bytes = main_options
        .limits
        .max_output_bytes
        .saturating_sub(icon_bytes);
    let (output, mut report) =
        transcode_top_recovering(&main, source, target, &main_options, recovery)?;
    check_icon_budget(&output, &sequences, target_syntax.big_endian, &limits)?;
    let mut elements = output.elements().to_vec();
    restore_icons(&mut elements, &[], &sequences)?;
    let output = object_from(elements, target_syntax.big_endian);
    measure(&output, &limits)?;
    report.icons = reports;
    Ok((output, report))
}
fn explicit_le() -> Syntax {
    crate::transfer::default_registry()
        .get(UID_EXPLICIT_LE)
        .unwrap_or_default()
}
fn transcode_icon(
    icon: &Object,
    source: &Syntax,
    target: &Syntax,
    options: &TranscodeOptions,
    recovery: &DecodeRecovery,
) -> Result<(Object, IconReport), Error> {
    let report = |action| IconReport {
        path: String::new(),
        action,
        metadata_incomplete: false,
        native_bytes: 0,
        encoded_bytes: 0,
        decode_diagnostics: Vec::new(),
        encode_compatibility_profiles: Vec::new(),
    };
    let pixel = match extract_view(icon) {
        Ok(p) => p,
        Err(e) if e.kind == PixelErrorKind::PixelDataNotFound => {
            let s = if source.encapsulated {
                explicit_le()
            } else {
                source.clone()
            };
            let t = if target.encapsulated {
                explicit_le()
            } else {
                target.clone()
            };
            let (clone, _) = transcode_top_recovering(icon, &s.uid, &t.uid, options, recovery)?;
            return Ok((clone, report(IconAction::Absent)));
        }
        Err(e) => return Err(e.into()),
    };
    let pixel_encapsulated = matches!(pixel, PixelData::Encapsulated { .. });
    let icon_source = if !pixel_encapsulated && source.encapsulated {
        explicit_le()
    } else {
        source.clone()
    };
    let icon_target = if target.encapsulated && !options.compress_icons {
        explicit_le()
    } else {
        target.clone()
    };
    let mut metadata = extract_metadata(icon);
    let mut diagnostics: Vec<String> = Vec::new();
    let defaulted;
    let mut icon = icon;
    if metadata.is_err()
        && recovery.default_image_pixel_module
        && let Some((repaired, applied)) =
            super::decompress::with_legacy_image_recovery(icon, &icon_source, recovery)
    {
        defaulted = repaired;
        icon = &defaulted;
        diagnostics = applied;
        metadata = extract_metadata(icon);
    }
    if let Ok(m) = &metadata
        && m.number_of_frames != 1
    {
        return Err(Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED));
    }
    if !pixel_encapsulated && metadata.is_ok() {
        extract_native_frames(icon)?;
    }
    if icon_target.encapsulated && options.compress_icons {
        let m = extract_metadata(icon)?;
        if icon_target.uid != UID_ENCAPSULATED_UNCOMPRESSED
            && (icon_source.uid != icon_target.uid
                || options.force_reencode
                || recovery.requires_legacy_pixel_repair())
        {
            let resolved = encode::resolve(&icon_target, &m, options)?;
            if !resolved.caps.lossless {
                return Err(Error::new(
                    ErrorKind::LossyDisallowed,
                    format!(
                        "{}: icon encoding requires a lossless codec",
                        super::ERR_LOSSY_DISALLOWED
                    ),
                ));
            }
        }
    }
    let (mut converted, r) =
        transcode_top_recovering(icon, &icon_source.uid, &icon_target.uid, options, recovery)?;
    if !r.pixel_data_preserved {
        let limits = resolve_limits(options.limits)?;
        let repaired;
        let mut icon = icon;
        let mut icon_source = icon_source.clone();
        if recovery.requires_legacy_pixel_repair() {
            let (normalized, _) = super::decompress::decompress_for_transcode_recovering(
                icon,
                &icon_source,
                options.decoder,
                super::decompress::transcode_decompress_limits(&limits),
                recovery,
            )?;
            repaired = normalized;
            icon = &repaired;
            icon_source = explicit_le();
        }
        verify_lossless(
            icon,
            &icon_source,
            &converted,
            &icon_target,
            options.decoder,
            &resolve_limits(options.limits)?,
        )?;
    }
    let action = if icon_target.encapsulated && r.pixel_data_preserved {
        IconAction::PreservedEncapsulated
    } else if icon_target.encapsulated {
        IconAction::Compressed
    } else if pixel_encapsulated {
        IconAction::Decompressed
    } else {
        IconAction::PreservedNative
    };
    converted.set_value_big_endian(icon_target.big_endian);
    Ok((
        converted,
        IconReport {
            path: String::new(),
            action,
            metadata_incomplete: metadata.is_err(),
            native_bytes: r.native_bytes,
            encoded_bytes: r.encoded_bytes,
            decode_diagnostics: {
                diagnostics.extend(r.decode_diagnostics);
                diagnostics
            },
            encode_compatibility_profiles: r.encode_compatibility_profiles,
        },
    ))
}
fn equal_planar_layout(a: &Object, b: &Object) -> bool {
    match (extract_metadata(a), extract_metadata(b)) {
        (Err(_), Err(_)) => true,
        (Ok(x), Ok(y)) => x.planar_configuration == y.planar_configuration,
        _ => false,
    }
}
pub(super) fn verify_lossless(
    source: &Object,
    source_syntax: &Syntax,
    output: &Object,
    output_syntax: &Syntax,
    registry: Option<DecoderRegistry>,
    limits: &TranscodeLimits,
) -> Result<(), Error> {
    let changed = |detail: &str| Error::pixel(PixelErrorKind::IncompatiblePixelData, detail);
    let (src, out) = (source.get(TAG_PIXEL_DATA), output.get(TAG_PIXEL_DATA));
    if src.is_some() != out.is_some() {
        return Err(changed("Pixel Data presence changed"));
    }
    let (Some(src), Some(out)) = (src, out) else {
        return Ok(());
    };
    if let Some(sr) = src.raw()
        && !source_syntax.encapsulated
        && source_syntax.big_endian == output_syntax.big_endian
        && equal_planar_layout(source, output)
        && let Some(or) = out.raw()
        && !output_syntax.encapsulated
    {
        if sr != or {
            return Err(changed("lossless Pixel Data changed"));
        }
        return Ok(());
    }
    let a = canonical_native_bytes(source, source_syntax, registry, limits)?;
    let mut ol = *limits;
    ol.max_input_bytes = limits.max_output_bytes;
    let b = canonical_native_bytes(output, output_syntax, registry, &ol)?;
    if a != b {
        return Err(changed("lossless Pixel Data changed"));
    }
    Ok(())
}
pub(super) fn canonical_native_bytes(
    obj: &Object,
    syntax: &Syntax,
    registry: Option<DecoderRegistry>,
    limits: &TranscodeLimits,
) -> Result<Vec<u8>, Error> {
    if syntax.encapsulated && syntax.uid != UID_ENCAPSULATED_UNCOMPRESSED && registry.is_none() {
        return Err(super::decompress::codec_registry_nil(&syntax.uid));
    }
    let (mut native, _) = decompress_for_transcode(
        obj,
        syntax,
        registry,
        DecompressLimits {
            max_frames: limits.max_frames,
            max_pixels: limits.max_pixels as i64,
            max_input_bytes: limits.max_input_bytes,
            max_native_bytes: limits.max_output_bytes,
            max_expansion_ratio: limits.max_expansion_ratio,
        },
    )?;
    if let Err(err) = apply_native_output_planar(&mut native, PlanarOutput::Interleaved) {
        let keep = match extract_metadata(&native) {
            Err(_) => false,
            Ok(m) => {
                !(m.samples_per_pixel == 3
                    && (m.photometric_interpretation == "RGB"
                        || m.photometric_interpretation == "YBR_FULL"))
            }
        };
        if !keep {
            return Err(err);
        }
    }
    let Some(e) = native.get(TAG_PIXEL_DATA) else {
        return Err(Error::pixel_bare(PixelErrorKind::PixelDataNotFound));
    };
    e.raw()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error::pixel_bare(PixelErrorKind::IncompatiblePixelData))
}
