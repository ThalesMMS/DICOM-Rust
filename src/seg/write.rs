use super::algorithm::{algorithm_identification_sequence, code_sequence};
use super::samples::segments_overlap_value;
use super::{
    DEFAULT_CONTENT_LABEL, Document, Error, FRACTIONAL_TYPE_OCCUPANCY, FRACTIONAL_TYPE_PROBABILITY,
    Frame, Kind, LABEL_MAP_SEGMENTATION_STORAGE, MAX_READ_DIMENSION, MAX_READ_FRAMES,
    ReferencedImage, SEGMENTATION_STORAGE, SEGMENTATION_TYPE_FRACTIONAL, Segment,
    SourceReferenceIndex, TAG_DERIVATION_IMAGE_SEQUENCE, TAG_DIMENSION_INDEX_POINTER,
    TAG_DIMENSION_INDEX_SEQUENCE, TAG_DIMENSION_INDEX_VALUES, TAG_DIMENSION_ORGANIZATION_SEQUENCE,
    TAG_DIMENSION_ORGANIZATION_UID, TAG_FRAME_CONTENT_SEQUENCE, TAG_FUNCTIONAL_GROUP_POINTER,
    TAG_IMAGE_ORIENTATION_PATIENT, TAG_IMAGE_POSITION_PATIENT, TAG_IMAGE_TYPE,
    TAG_IN_STACK_POSITION_NUMBER, TAG_LOSSY_IMAGE_COMPRESSION, TAG_MAXIMUM_FRACTIONAL_VALUE,
    TAG_PER_FRAME_FUNCTIONAL_GROUPS, TAG_PIXEL_MEASURES_SEQUENCE, TAG_PIXEL_SPACING,
    TAG_PLANE_ORIENTATION_SEQUENCE, TAG_PLANE_POSITION_SEQUENCE,
    TAG_RECOMMENDED_DISPLAY_CIELAB_VALUE, TAG_REFERENCED_INSTANCE_SEQUENCE,
    TAG_REFERENCED_SEGMENT_NUMBER, TAG_REFERENCED_SERIES_SEQUENCE, TAG_SEGMENT_ALGORITHM_NAME,
    TAG_SEGMENT_ALGORITHM_TYPE, TAG_SEGMENT_IDENTIFICATION_SEQUENCE, TAG_SEGMENT_LABEL,
    TAG_SEGMENT_NUMBER, TAG_SEGMENT_SEQUENCE, TAG_SEGMENTATION_FRACTIONAL_TYPE,
    TAG_SEGMENTATION_TYPE, TAG_SEGMENTED_PROPERTY_CATEGORY_SEQ, TAG_SEGMENTED_PROPERTY_TYPE_SEQ,
    TAG_SEGMENTS_OVERLAP, TAG_SHARED_FUNCTIONAL_GROUPS, TAG_SLICE_THICKNESS,
    TAG_SPACING_BETWEEN_SLICES, TAG_STACK_ID, geometry_for_frame, invalid, normalize_segment,
    segmentation_type, set_packed_bit_range, trim_space, valid_slice_geometry,
    validate_representable_segmentation_type,
};
use crate::dcmtime::ZonedTime;
use crate::derivation::{self, Derivation, Source};
use crate::derivedio as dio;
use crate::object::inherit::inherit_modules;
use crate::object::{DataSet, Element, File, Tag, Vr};
use crate::render::{SliceGeometry, VolumeGeometry};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
fn local_now() -> ZonedTime {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    ZonedTime::from_fields(
        i64::from(now.year()),
        i64::from(now.month()),
        i64::from(now.day()),
        i64::from(now.hour()),
        i64::from(now.minute()),
        i64::from(now.second()),
        0,
        i64::from(now.offset().local_minus_utc()),
    )
}
pub fn write(doc: &Document) -> Result<File, Error> {
    write_document(doc, true, "")
}
pub(crate) fn write_document(doc: &Document, pixels: bool, overlap: &str) -> Result<File, Error> {
    let sop_class_uid = if doc.sop_class_uid.is_empty() {
        SEGMENTATION_STORAGE
    } else {
        doc.sop_class_uid.as_str()
    };
    if sop_class_uid != SEGMENTATION_STORAGE && sop_class_uid != LABEL_MAP_SEGMENTATION_STORAGE {
        return Err(Error::wrap(Kind::UnsupportedSopClass, sop_class_uid));
    }
    validate_representable_segmentation_type(sop_class_uid, &doc.segmentation_type)?;
    let source_references = SourceReferenceIndex::new(&doc.referenced_images)?;
    let resolved = source_references.resolve_frames(&doc.frames)?;
    if pixels {
        validate_frame_payloads(doc, sop_class_uid)?;
    }
    validate_write_dimensions(doc)?;
    validate_unsigned_short_fields(doc)?;
    let identification = doc
        .identification
        .elements(DEFAULT_CONTENT_LABEL)
        .map_err(invalid)?;
    let mut content_date = doc.content_date.clone();
    let mut content_time = doc.content_time.clone();
    if content_date.is_empty() || content_time.is_empty() {
        let now = doc.now.unwrap_or_else(local_now);
        if content_date.is_empty() {
            let (y, m, d) = now.date();
            content_date = format!("{y:04}{m:02}{d:02}");
        }
        if content_time.is_empty() {
            content_time = format!("{:02}{:02}{:02}", now.hour(), now.minute(), now.second());
        }
    }
    let label_map = sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE;
    let mut elements = vec![
        dio::ui(dio::TAG_SOP_CLASS_UID, sop_class_uid),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &doc.sop_instance_uid),
        dio::cs(dio::TAG_MODALITY, "SEG"),
        dio::strings(
            TAG_IMAGE_TYPE,
            Vr::CS,
            &["DERIVED".to_string(), "PRIMARY".to_string()],
        ),
        dio::cs(TAG_LOSSY_IMAGE_COMPRESSION, "00"),
        dio::str_elem(dio::TAG_CONTENT_DATE, Vr::DA, &content_date),
        dio::str_elem(dio::TAG_CONTENT_TIME, Vr::TM, &content_time),
        dimension_organization_sequence(doc),
        dio::ui(dio::TAG_STUDY_INSTANCE_UID, &doc.study_instance_uid),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, &doc.series_instance_uid),
        dio::ui(dio::TAG_FRAME_OF_REFERENCE_UID, &doc.frame_of_reference_uid),
        dio::us(dio::TAG_ROWS, &[doc.rows as u16]),
        dio::us(dio::TAG_COLUMNS, &[doc.columns as u16]),
        dio::is(dio::TAG_NUMBER_OF_FRAMES, &[doc.frames.len() as i64]),
        dio::us(dio::TAG_SAMPLES_PER_PIXEL, &[1]),
        dio::cs(dio::TAG_PHOTOMETRIC_INTERPRETATION, "MONOCHROME2"),
        dio::cs(TAG_SEGMENTATION_TYPE, segmentation_type(doc)),
        segment_sequence(&doc.segments, label_map),
        referenced_series_sequence(&doc.referenced_images),
        dimension_index_sequence(doc, sop_class_uid),
    ];
    elements.extend(identification);
    elements.push(
        shared_functional_groups_sequence(doc)
            .unwrap_or_else(|| dio::seq(TAG_SHARED_FUNCTIONAL_GROUPS, vec![DataSet::default()])),
    );
    elements.push(per_frame_sequence(doc, sop_class_uid, &resolved)?);
    let overlap = if overlap.is_empty() {
        segments_overlap_value(doc, sop_class_uid)?
    } else {
        overlap
    };
    elements.push(dio::cs(TAG_SEGMENTS_OVERLAP, overlap));
    let layout = |bits: u16| {
        vec![
            dio::us(dio::TAG_BITS_ALLOCATED, &[bits]),
            dio::us(dio::TAG_BITS_STORED, &[bits]),
            dio::us(dio::TAG_HIGH_BIT, &[bits - 1]),
            dio::us(dio::TAG_PIXEL_REPRESENTATION, &[0]),
        ]
    };
    if label_map {
        let bits_allocated = label_map_bits_allocated(doc)?;
        elements.extend(layout(bits_allocated as u16));
        if pixels {
            let vr = if bits_allocated == 8 { Vr::OB } else { Vr::OW };
            elements.push(dio::raw(
                dio::TAG_PIXEL_DATA,
                vr,
                label_map_pixel_data(doc, bits_allocated),
            ));
        }
    } else if segmentation_type(doc) == SEGMENTATION_TYPE_FRACTIONAL {
        elements.push(dio::cs(
            TAG_SEGMENTATION_FRACTIONAL_TYPE,
            &doc.fractional_type,
        ));
        elements.push(dio::us(
            TAG_MAXIMUM_FRACTIONAL_VALUE,
            &[doc.max_fractional_value as u16],
        ));
        elements.extend(layout(8));
        if pixels {
            elements.push(dio::raw(
                dio::TAG_PIXEL_DATA,
                Vr::OB,
                fractional_pixel_data(doc),
            ));
        }
    } else {
        elements.extend(layout(1));
        if pixels {
            elements.push(dio::raw(
                dio::TAG_PIXEL_DATA,
                Vr::OB,
                binary_pixel_data(doc),
            ));
        }
    }
    let mut dataset = dio::object(elements);
    doc.common
        .apply(&mut dataset, inherit_modules::ALL)
        .map_err(Error::other)?;
    dio::file(sop_class_uid, &doc.sop_instance_uid, Some(dataset)).map_err(Error::other)
}
pub(crate) fn validate_write_dimensions(doc: &Document) -> Result<(), Error> {
    if doc.rows <= 0 || doc.columns <= 0 {
        return Err(invalid(format!(
            "Rows={} Columns={}",
            doc.rows, doc.columns
        )));
    }
    if doc.rows > MAX_READ_DIMENSION || doc.columns > MAX_READ_DIMENSION {
        return Err(Error::wrap(
            Kind::ResourceLimitExceeded,
            format!(
                "Rows={} Columns={} exceed {MAX_READ_DIMENSION}",
                doc.rows, doc.columns
            ),
        ));
    }
    if doc.frames.is_empty() {
        return Err(invalid("invalid Number Of Frames"));
    }
    if doc.frames.len() as i64 > MAX_READ_FRAMES {
        return Err(Error::wrap(
            Kind::ResourceLimitExceeded,
            format!(
                "Number Of Frames={} exceeds {MAX_READ_FRAMES}",
                doc.frames.len()
            ),
        ));
    }
    Ok(())
}
fn segment_sequence(segments: &[Segment], label_map: bool) -> Element {
    let mut items = Vec::with_capacity(segments.len());
    for (i, original) in segments.iter().enumerate() {
        let number = original.number;
        let has_label = !trim_space(&original.label).is_empty();
        let mut segment = normalize_segment(original, i as i64 + 1);
        if label_map {
            segment.number = number;
            if !has_label && number == 0 {
                segment.label = "Segment 0".to_string();
            }
        }
        let mut elements = vec![
            dio::us(TAG_SEGMENT_NUMBER, &[segment.number as u16]),
            dio::lo(TAG_SEGMENT_LABEL, &segment.label),
            dio::cs(TAG_SEGMENT_ALGORITHM_TYPE, &segment.algorithm_type),
            code_sequence(
                TAG_SEGMENTED_PROPERTY_CATEGORY_SEQ,
                &segment.segmented_property_category,
            ),
            code_sequence(
                TAG_SEGMENTED_PROPERTY_TYPE_SEQ,
                &segment.segmented_property_type,
            ),
        ];
        if !segment.algorithm_name.is_empty() {
            elements.push(dio::lo(TAG_SEGMENT_ALGORITHM_NAME, &segment.algorithm_name));
        }
        if !segment.algorithm_identification.is_zero() {
            elements.push(algorithm_identification_sequence(
                &segment.algorithm_identification,
            ));
        }
        if segment.recommended_display_cielab != [0, 0, 0] {
            elements.push(dio::us(
                TAG_RECOMMENDED_DISPLAY_CIELAB_VALUE,
                &segment.recommended_display_cielab,
            ));
        }
        items.push(dio::data_set(elements));
    }
    dio::seq(TAG_SEGMENT_SEQUENCE, items)
}
fn ref_data_set(reference: &ReferencedImage) -> DataSet {
    let mut elements = vec![
        dio::ui(dio::TAG_REF_SOP_CLASS_UID, &reference.sop_class_uid),
        dio::ui(dio::TAG_REF_SOP_INSTANCE_UID, &reference.sop_instance_uid),
    ];
    if !reference.frames.is_empty() {
        elements.push(dio::is(dio::TAG_REF_FRAME_NUMBER, &reference.frames));
    }
    dio::data_set(elements)
}
fn referenced_series_sequence(refs: &[ReferencedImage]) -> Element {
    let mut groups: Vec<(&str, Vec<DataSet>)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for reference in refs {
        let uid = reference.series_instance_uid.as_str();
        match index.get(uid) {
            Some(&i) => groups[i].1.push(ref_data_set(reference)),
            None => {
                index.insert(uid, groups.len());
                groups.push((uid, vec![ref_data_set(reference)]));
            }
        }
    }
    let items = groups
        .into_iter()
        .map(|(uid, images)| {
            dio::data_set(vec![
                dio::ui(dio::TAG_SERIES_INSTANCE_UID, uid),
                dio::seq(TAG_REFERENCED_INSTANCE_SEQUENCE, images),
            ])
        })
        .collect();
    dio::seq(TAG_REFERENCED_SERIES_SEQUENCE, items)
}
pub(crate) fn validate_frame_payloads(doc: &Document, sop_class_uid: &str) -> Result<(), Error> {
    let seg_type = segmentation_type(doc);
    let fractional =
        sop_class_uid == SEGMENTATION_STORAGE && seg_type == SEGMENTATION_TYPE_FRACTIONAL;
    if fractional {
        if doc.fractional_type != FRACTIONAL_TYPE_PROBABILITY
            && doc.fractional_type != FRACTIONAL_TYPE_OCCUPANCY
        {
            return Err(invalid(format!(
                "Segmentation Fractional Type {}",
                quote(&doc.fractional_type)
            )));
        }
        if doc.max_fractional_value < 1 || doc.max_fractional_value > 255 {
            return Err(invalid(format!(
                "Maximum Fractional Value {} is outside 1-255",
                doc.max_fractional_value
            )));
        }
    }
    let pixels = doc.rows.wrapping_mul(doc.columns);
    let label_map = sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE;
    let described: HashSet<i64> = if label_map {
        doc.segments.iter().map(|s| s.number).collect()
    } else {
        HashSet::new()
    };
    let payload = |detail: String| Error::wrap(Kind::UnsupportedFramePayload, detail);
    for (i, frame) in doc.frames.iter().enumerate() {
        let mask_set = frame.mask.as_ref().is_some_and(|m| !m.is_empty());
        if let Some(mask) = &frame.mask
            && (mask.rows != doc.rows || mask.columns != doc.columns)
        {
            return Err(invalid(format!(
                "frame {i} mask dimensions {}x{} do not match {}x{}",
                mask.columns, mask.rows, doc.columns, doc.rows
            )));
        }
        if !frame.fractional.is_empty() && !fractional {
            return Err(payload(format!(
                "frame {i} has fractional samples in a {seg_type} segmentation"
            )));
        }
        if fractional {
            if frame.fractional.is_empty() && mask_set {
                return Err(payload(format!(
                    "fractional frame {i} contains only a binary mask"
                )));
            }
            if !frame.fractional.is_empty() && frame.fractional.len() as i64 != pixels {
                return Err(invalid(format!(
                    "fractional frame {i} has {} samples, want {pixels}",
                    frame.fractional.len()
                )));
            }
            if let Some(sample) = frame
                .fractional
                .iter()
                .find(|&&s| i64::from(s) > doc.max_fractional_value)
            {
                return Err(invalid(format!(
                    "fractional frame {i} sample {sample} exceeds Maximum Fractional Value {}",
                    doc.max_fractional_value
                )));
            }
            continue;
        }
        if label_map {
            if frame.label_map.is_empty() && mask_set {
                return Err(payload(format!(
                    "label-map frame {i} contains only a binary mask"
                )));
            }
            if frame.label_map.len() as i64 > pixels {
                return Err(invalid(format!(
                    "label-map frame {i} has {} values, want {pixels}",
                    frame.label_map.len()
                )));
            }
            if (frame.label_map.len() as i64) < pixels && !described.contains(&0) {
                return Err(invalid(format!(
                    "label-map frame {i} pads with 0, which no segment describes"
                )));
            }
            if let Some(value) = frame
                .label_map
                .iter()
                .find(|&&v| !described.contains(&i64::from(v)))
            {
                return Err(invalid(format!(
                    "label-map frame {i} uses value {value}, which no segment describes"
                )));
            }
        }
        if sop_class_uid == SEGMENTATION_STORAGE
            && frame.mask.is_none()
            && frame.label_map.iter().any(|&v| v != 0)
        {
            return Err(payload(format!(
                "binary frame {i} contains only label-map values"
            )));
        }
    }
    Ok(())
}
pub(crate) fn validate_unsigned_short_fields(doc: &Document) -> Result<(), Error> {
    if doc.rows <= 0 || doc.rows > 0xFFFF || doc.columns <= 0 || doc.columns > 0xFFFF {
        return Err(invalid(format!(
            "dimensions {}x{} are outside US",
            doc.columns, doc.rows
        )));
    }
    let label_map = doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE;
    let mut seen: HashSet<i64> = HashSet::new();
    for (i, segment) in doc.segments.iter().enumerate() {
        let mut normalized = normalize_segment(segment, i as i64 + 1);
        let number = if label_map {
            segment.number
        } else {
            normalized.number
        };
        if number < 0 || (number == 0 && !label_map) || number > 0xFFFF {
            return Err(invalid(format!(
                "segment {i} number {number} is outside US"
            )));
        }
        if seen.contains(&number) {
            return Err(invalid(format!("segment number {number} is not unique")));
        }
        normalized.number = number;
        normalized.validate_algorithm()?;
        seen.insert(number);
    }
    if label_map {
        return Ok(());
    }
    for (i, frame) in doc.frames.iter().enumerate() {
        if frame.segment_number <= 0 || frame.segment_number > 0xFFFF {
            return Err(invalid(format!(
                "frame {i} segment number {} is outside US",
                frame.segment_number
            )));
        }
    }
    Ok(())
}
fn shared_functional_groups_sequence(doc: &Document) -> Option<Element> {
    let geom = shared_geometry(doc)?;
    let mut measures = vec![
        dio::ds(TAG_PIXEL_SPACING, &[geom.row_spacing, geom.col_spacing]),
        dio::ds(TAG_SLICE_THICKNESS, &[geometry_thickness(&doc.geometry)]),
    ];
    if doc.geometry.mean_spacing > 0.0 {
        measures.push(dio::ds(
            TAG_SPACING_BETWEEN_SLICES,
            &[doc.geometry.mean_spacing],
        ));
    }
    Some(dio::seq(
        TAG_SHARED_FUNCTIONAL_GROUPS,
        vec![dio::data_set(vec![
            dio::seq(TAG_PIXEL_MEASURES_SEQUENCE, vec![dio::data_set(measures)]),
            dio::seq(
                TAG_PLANE_ORIENTATION_SEQUENCE,
                vec![dio::data_set(vec![dio::ds(
                    TAG_IMAGE_ORIENTATION_PATIENT,
                    &[
                        geom.row_dir.x,
                        geom.row_dir.y,
                        geom.row_dir.z,
                        geom.col_dir.x,
                        geom.col_dir.y,
                        geom.col_dir.z,
                    ],
                )])],
            ),
        ])],
    ))
}
fn shared_geometry(doc: &Document) -> Option<SliceGeometry> {
    doc.frames
        .iter()
        .map(|f| f.geometry)
        .find(valid_slice_geometry)
        .or_else(|| doc.geometry.slices().first().copied())
}
fn geometry_thickness(geometry: &VolumeGeometry) -> f64 {
    if geometry.mean_spacing > 0.0 {
        geometry.mean_spacing
    } else {
        1.0
    }
}
fn dimension_organization_uid(doc: &Document) -> String {
    dio::derived_uid(&[&doc.sop_instance_uid, "dimension-organization"])
}
fn dimension_organization_sequence(doc: &Document) -> Element {
    dio::seq(
        TAG_DIMENSION_ORGANIZATION_SEQUENCE,
        vec![dio::data_set(vec![dio::ui(
            TAG_DIMENSION_ORGANIZATION_UID,
            &dimension_organization_uid(doc),
        )])],
    )
}
fn dimension_index_sequence(doc: &Document, sop_class_uid: &str) -> Element {
    let organization = dio::ui(
        TAG_DIMENSION_ORGANIZATION_UID,
        &dimension_organization_uid(doc),
    );
    let item = |pointer: Tag, group: Tag| {
        dio::data_set(vec![
            tag_pointer_element(TAG_DIMENSION_INDEX_POINTER, pointer),
            tag_pointer_element(TAG_FUNCTIONAL_GROUP_POINTER, group),
            organization.clone(),
        ])
    };
    let mut items = Vec::new();
    if sop_class_uid != LABEL_MAP_SEGMENTATION_STORAGE {
        items.push(item(
            TAG_REFERENCED_SEGMENT_NUMBER,
            TAG_SEGMENT_IDENTIFICATION_SEQUENCE,
        ));
    }
    if frames_have_positions(doc) {
        items.push(item(
            TAG_IMAGE_POSITION_PATIENT,
            TAG_PLANE_POSITION_SEQUENCE,
        ));
    } else {
        items.push(item(
            TAG_IN_STACK_POSITION_NUMBER,
            TAG_FRAME_CONTENT_SEQUENCE,
        ));
    }
    dio::seq(TAG_DIMENSION_INDEX_SEQUENCE, items)
}
fn frames_have_positions(doc: &Document) -> bool {
    !doc.frames.is_empty()
        && doc
            .frames
            .iter()
            .all(|f| geometry_for_frame(&doc.geometry, f).is_some())
}
fn tag_pointer_element(tag: Tag, value: Tag) -> Element {
    let mut raw = Vec::with_capacity(4);
    raw.extend_from_slice(&value.group.to_le_bytes());
    raw.extend_from_slice(&value.element.to_le_bytes());
    dio::raw(tag, Vr::AT, raw)
}
fn per_frame_sequence(
    doc: &Document,
    sop_class_uid: &str,
    source_references: &[ReferencedImage],
) -> Result<Element, Error> {
    let mut items = Vec::with_capacity(doc.frames.len());
    let positioned = frames_have_positions(doc);
    let segmented = sop_class_uid != LABEL_MAP_SEGMENTATION_STORAGE;
    for (frame_index, frame) in doc.frames.iter().enumerate() {
        let mut elements = Vec::new();
        if segmented {
            elements.push(dio::seq(
                TAG_SEGMENT_IDENTIFICATION_SEQUENCE,
                vec![dio::data_set(vec![dio::us(
                    TAG_REFERENCED_SEGMENT_NUMBER,
                    &[frame.segment_number as u16],
                )])],
            ));
        }
        elements.push(frame_content_sequence(frame, segmented, !positioned));
        if let Some(geom) = geometry_for_frame(&doc.geometry, frame) {
            elements.push(dio::seq(
                TAG_PLANE_POSITION_SEQUENCE,
                vec![dio::data_set(vec![dio::ds(
                    TAG_IMAGE_POSITION_PATIENT,
                    &[geom.origin.x, geom.origin.y, geom.origin.z],
                )])],
            ));
        }
        if !frame.referenced_sop_instance_uid.is_empty() {
            let derivation_image =
                derivation_image_sequence(&source_references[frame_index], frame).map_err(|e| {
                    Error::wrap(
                        Kind::MissingReference,
                        format!("frame {}: {e}", frame_index + 1),
                    )
                })?;
            elements.push(derivation_image);
        }
        items.push(dio::data_set(elements));
    }
    Ok(dio::seq(TAG_PER_FRAME_FUNCTIONAL_GROUPS, items))
}
fn derivation_image_sequence(
    reference: &ReferencedImage,
    frame: &Frame,
) -> Result<Element, derivation::Error> {
    let source = Source {
        sop_class_uid: reference.sop_class_uid.clone(),
        sop_instance_uid: frame.referenced_sop_instance_uid.clone(),
        frames: if frame.referenced_frame_number > 0 {
            vec![frame.referenced_frame_number]
        } else {
            Vec::new()
        },
        purpose: Some(derivation::codes::SourceImageForProcessing()),
    };
    let item = Derivation {
        description: frame.derivation_description.clone(),
        codes: vec![derivation::codes::Segmentation()],
        sources: vec![source],
    }
    .item()?;
    Ok(dio::seq(TAG_DERIVATION_IMAGE_SEQUENCE, vec![item]))
}
fn frame_content_sequence(frame: &Frame, segment_dimension: bool, stack: bool) -> Element {
    let position = frame.slice_index.wrapping_add(1).max(1) as u32;
    let mut raw = Vec::with_capacity(8);
    if segment_dimension {
        raw.extend_from_slice(&(frame.segment_number as u32).to_le_bytes());
    }
    raw.extend_from_slice(&position.to_le_bytes());
    let mut elements = Vec::new();
    if stack {
        elements.push(dio::sh(TAG_STACK_ID, "1"));
        elements.push(dio::raw(
            TAG_IN_STACK_POSITION_NUMBER,
            Vr::UL,
            position.to_le_bytes().to_vec(),
        ));
    }
    elements.push(dio::raw(TAG_DIMENSION_INDEX_VALUES, Vr::UL, raw));
    dio::seq(TAG_FRAME_CONTENT_SEQUENCE, vec![dio::data_set(elements)])
}
fn binary_pixel_data(doc: &Document) -> Vec<u8> {
    let bits_per_frame = doc.rows * doc.columns;
    if bits_per_frame <= 0 {
        return Vec::new();
    }
    let total_bits = bits_per_frame * doc.frames.len() as i64;
    let mut out = vec![0u8; ((total_bits + 7) / 8) as usize];
    for (frame_index, frame) in doc.frames.iter().enumerate() {
        let Some(mask) = &frame.mask else {
            continue;
        };
        let base_bit = frame_index as i64 * bits_per_frame;
        mask.for_each_run(|y, run| {
            let row_bit = base_bit + y * doc.columns;
            set_packed_bit_range(&mut out, row_bit + run.start, row_bit + run.end);
        });
    }
    out
}
fn label_map_pixel_data(doc: &Document, bits_allocated: i64) -> Vec<u8> {
    let values_per_frame = (doc.rows * doc.columns) as usize;
    let bytes_per_value = (bits_allocated / 8) as usize;
    let mut out = vec![0u8; values_per_frame * doc.frames.len() * bytes_per_value];
    for (frame_index, frame) in doc.frames.iter().enumerate() {
        let base = frame_index * values_per_frame * bytes_per_value;
        for (i, &value) in frame.label_map.iter().enumerate() {
            if bytes_per_value == 1 {
                out[base + i] = value as u8;
            } else {
                out[base + 2 * i..base + 2 * i + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}
pub(crate) fn label_map_bits_allocated(doc: &Document) -> Result<i64, Error> {
    let mut largest = 0;
    for segment in &doc.segments {
        largest = largest.max(segment.number);
    }
    for frame in &doc.frames {
        for &value in &frame.label_map {
            largest = largest.max(i64::from(value));
        }
    }
    match doc.label_map_bits_allocated {
        0 => Ok(if largest <= 0xFF { 8 } else { 16 }),
        8 => {
            if largest > 0xFF {
                return Err(invalid(format!(
                    "label {largest} does not fit 8-bit label-map samples"
                )));
            }
            Ok(8)
        }
        16 => Ok(16),
        other => Err(invalid(format!(
            "label-map Bits Allocated {other} is not 8 or 16"
        ))),
    }
}
fn fractional_pixel_data(doc: &Document) -> Vec<u8> {
    let samples_per_frame = (doc.rows * doc.columns) as usize;
    let mut out = vec![0u8; samples_per_frame * doc.frames.len()];
    for (frame_index, frame) in doc.frames.iter().enumerate() {
        let start = frame_index * samples_per_frame;
        let n = frame.fractional.len().min(out.len() - start);
        out[start..start + n].copy_from_slice(&frame.fractional[..n]);
    }
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}
