#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::algorithm::read_algorithm_identification;
use super::samples::read_sample_layout;
use super::{
    CodedEntry, Document, Error, Frame, Kind, LABEL_MAP_SEGMENTATION_STORAGE, MAX_READ_DIMENSION,
    MAX_READ_FRAMES, MAX_READ_PIXELS, ReadOptions, ReadReport, ReferencedImage,
    SEGMENTATION_STORAGE, SEGMENTATION_TYPE_FRACTIONAL, Segment, SourceReferenceIndex,
    TAG_CODE_MEANING, TAG_CODE_VALUE, TAG_CODING_SCHEME_DESIGNATOR, TAG_DERIVATION_IMAGE_SEQUENCE,
    TAG_DIMENSION_INDEX_POINTER, TAG_DIMENSION_INDEX_SEQUENCE, TAG_DIMENSION_INDEX_VALUES,
    TAG_FRAME_CONTENT_SEQUENCE, TAG_FUNCTIONAL_GROUP_POINTER, TAG_IMAGE_ORIENTATION_PATIENT,
    TAG_IMAGE_POSITION_PATIENT, TAG_IN_STACK_POSITION_NUMBER, TAG_PER_FRAME_FUNCTIONAL_GROUPS,
    TAG_PIXEL_MEASURES_SEQUENCE, TAG_PIXEL_SPACING, TAG_PLANE_ORIENTATION_SEQUENCE,
    TAG_PLANE_POSITION_SEQUENCE, TAG_RECOMMENDED_DISPLAY_CIELAB_VALUE,
    TAG_REFERENCED_INSTANCE_SEQUENCE, TAG_REFERENCED_SEGMENT_NUMBER,
    TAG_REFERENCED_SERIES_SEQUENCE, TAG_SEGMENT_ALGORITHM_NAME, TAG_SEGMENT_ALGORITHM_TYPE,
    TAG_SEGMENT_IDENTIFICATION_SEQUENCE, TAG_SEGMENT_LABEL, TAG_SEGMENT_NUMBER,
    TAG_SEGMENT_SEQUENCE, TAG_SEGMENTATION_TYPE, TAG_SEGMENTED_PROPERTY_CATEGORY_SEQ,
    TAG_SEGMENTED_PROPERTY_TYPE_SEQ, TAG_SEGMENTS_OVERLAP, TAG_SLICE_THICKNESS,
    TAG_SOURCE_IMAGE_SEQUENCE, TAG_SPACING_BETWEEN_SLICES, invalid, low_bit_mask, positive_finite,
    validate_representable_segmentation_type,
};
use crate::derivedio as dio;
use crate::fmath;
use crate::object::geometry::spacing_values;
use crate::object::{Object, Tag, Value, Vr};
use crate::render::grid::DICOM_GRID_COORDINATE_TOLERANCE;
use crate::render::{
    GeometryTolerances, SliceGeometry, Vec3, VolumeClassification, VolumeGeometry,
    build_volume_geometry,
};
use crate::roi::RasterMask;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
pub fn read(obj: &Object) -> Result<Document, Error> {
    read_with_options(obj, ReadOptions::default()).map(|(doc, _)| doc)
}
pub fn read_with_options(obj: &Object, opts: ReadOptions) -> Result<(Document, ReadReport), Error> {
    let doc = read_document_header(obj)?;
    read_document_pixels(obj, doc, opts, false)
}
#[doc(hidden)]
pub fn read_with_options_report(
    obj: &Object,
    opts: ReadOptions,
) -> (Result<Document, Error>, ReadReport) {
    match read_document_header(obj) {
        Ok(doc) => read_document_pixels_report(obj, doc, opts, false),
        Err(e) => (Err(e), ReadReport::default()),
    }
}
pub(crate) fn read_document_pixels(
    obj: &Object,
    doc: Document,
    opts: ReadOptions,
    continuous_binary: bool,
) -> Result<(Document, ReadReport), Error> {
    let (doc, report) = read_document_pixels_report(obj, doc, opts, continuous_binary);
    doc.map(|d| (d, report))
}
fn read_document_pixels_report(
    obj: &Object,
    mut doc: Document,
    opts: ReadOptions,
    continuous_binary: bool,
) -> (Result<Document, Error>, ReadReport) {
    if let Err(e) = validate_native_pixel_data(obj) {
        return (Err(e), ReadReport::default());
    }
    doc.geometry = match read_volume_geometry(obj, doc.rows, doc.columns) {
        Ok(g) => g,
        Err(e) => return (Err(e), ReadReport::default()),
    };
    let (frames, report) = read_frames_layout(obj, &doc, continuous_binary);
    doc.frames = frames;
    if report.truncated() && !opts.allow_truncated_pixel_data {
        let e = Error::wrap(
            Kind::TruncatedPixelData,
            format!(
                "{} bytes for {} frames of {} x {} need {} ({} incomplete frames)",
                report.pixel_data_bytes,
                doc.frames.len(),
                doc.rows,
                doc.columns,
                report.expected_pixel_data_bytes,
                report.incomplete_frames.len()
            ),
        );
        return (Err(e), report);
    }
    let checked = SourceReferenceIndex::new(&doc.referenced_images)
        .and_then(|index| index.resolve_frames(&doc.frames).map(|_| ()));
    match checked {
        Ok(()) => (Ok(doc), report),
        Err(e) => (Err(e), report),
    }
}
pub fn read_header(obj: &Object) -> Result<Document, Error> {
    let doc = read_document_header(obj)?;
    if doc.segments.is_empty() || doc.segments.len() > 65535 {
        return Err(invalid("invalid segment count"));
    }
    let mut seen = HashSet::new();
    for segment in &doc.segments {
        if segment.number < 1 || segment.number > 65535 || !seen.insert(segment.number) {
            return Err(invalid("invalid/duplicate segment number"));
        }
    }
    Ok(doc)
}
pub(crate) fn read_document_header(obj: &Object) -> Result<Document, Error> {
    let sop_class_uid = dio::clean_uid(obj, dio::TAG_SOP_CLASS_UID);
    if sop_class_uid != SEGMENTATION_STORAGE && sop_class_uid != LABEL_MAP_SEGMENTATION_STORAGE {
        return Err(Error::wrap(Kind::UnsupportedSopClass, sop_class_uid));
    }
    let segments = read_segments(obj)?;
    let mut doc = Document {
        sop_class_uid,
        sop_instance_uid: dio::clean_uid(obj, dio::TAG_SOP_INSTANCE_UID),
        study_instance_uid: dio::clean_uid(obj, dio::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: dio::clean_uid(obj, dio::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: dio::clean_uid(obj, dio::TAG_FRAME_OF_REFERENCE_UID),
        identification: dio::read_identification(obj),
        content_date: dio::clean_string(obj, dio::TAG_CONTENT_DATE),
        content_time: dio::clean_string(obj, dio::TAG_CONTENT_TIME),
        rows: dio::int(obj, dio::TAG_ROWS),
        columns: dio::int(obj, dio::TAG_COLUMNS),
        segmentation_type: dio::clean_string(obj, TAG_SEGMENTATION_TYPE),
        segments_overlap: dio::clean_string(obj, TAG_SEGMENTS_OVERLAP),
        segments,
        referenced_images: read_referenced_images(obj),
        ..Document::default()
    };
    validate_representable_segmentation_type(&doc.sop_class_uid, &doc.segmentation_type)?;
    read_sample_layout(obj, &mut doc)?;
    validate_read_dimensions(obj, &doc)?;
    Ok(doc)
}
fn per_frame_count(obj: &Object) -> usize {
    match obj.get(TAG_PER_FRAME_FUNCTIONAL_GROUPS).map(|e| &e.value) {
        Some(Value::Sequence(items)) => items.len(),
        _ => 0,
    }
}
fn validate_read_dimensions(obj: &Object, doc: &Document) -> Result<(), Error> {
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
    let mut frame_count = per_frame_count(obj) as i64;
    if frame_count == 0 {
        let value = match obj.get_int(dio::TAG_NUMBER_OF_FRAMES) {
            Ok(v) if v > 0 => v,
            _ => return Err(invalid("invalid Number Of Frames")),
        };
        if value > MAX_READ_FRAMES {
            return Err(Error::wrap(
                Kind::ResourceLimitExceeded,
                format!("Number Of Frames={value} exceeds {MAX_READ_FRAMES}"),
            ));
        }
        frame_count = value;
    }
    if frame_count > MAX_READ_FRAMES {
        return Err(Error::wrap(
            Kind::ResourceLimitExceeded,
            format!("{frame_count} per-frame items exceed {MAX_READ_FRAMES}"),
        ));
    }
    if doc.rows * doc.columns > MAX_READ_PIXELS / frame_count {
        return Err(Error::wrap(
            Kind::ResourceLimitExceeded,
            format!(
                "{} x {} x {frame_count} pixels exceed {MAX_READ_PIXELS}",
                doc.rows, doc.columns
            ),
        ));
    }
    Ok(())
}
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "<nil>",
        Value::Raw(_) => "core.RawValue",
        Value::Strings(_) => "core.StringValue",
        Value::Sequence(_) => "core.SequenceValue",
        Value::Fragments { .. } => "core.FragmentSequence",
        Value::BulkData(_) => "core.BulkDataValue",
    }
}
fn validate_native_pixel_data(obj: &Object) -> Result<(), Error> {
    let Some(elem) = obj.get(dio::TAG_PIXEL_DATA) else {
        return Err(invalid("Pixel Data is missing"));
    };
    match &elem.value {
        Value::Raw(_) => Ok(()),
        Value::Fragments { .. } => Err(Error::wrap(
            Kind::UnsupportedPixelData,
            "encapsulated Pixel Data",
        )),
        other => Err(invalid(format!(
            "Pixel Data has value type {}",
            value_type_name(other)
        ))),
    }
}
fn read_volume_geometry(obj: &Object, rows: i64, columns: i64) -> Result<VolumeGeometry, Error> {
    let Some((template, fallback_spacing)) = read_slice_geometry_template(obj, rows, columns)
    else {
        return Ok(VolumeGeometry::default());
    };
    let frame_items = dio::sequence(obj, TAG_PER_FRAME_FUNCTIONAL_GROUPS);
    let spatial = spatial_dimension_value_index(obj);
    let mut origins: HashMap<i64, Vec3> = HashMap::new();
    let (mut first_index, mut last_index) = (-1i64, -1i64);
    for (i, item) in frame_items.iter().enumerate() {
        let mut index = i as i64;
        if let Some(values) = read_dimension_index_values(Some(item))
            && let Some(decoded) = slice_index_from_dimension_values(&values, spatial)
        {
            index = decoded;
        }
        if index < 0 || index >= MAX_READ_FRAMES {
            return Err(Error::wrap(
                Kind::ResourceLimitExceeded,
                format!("slice index {index} exceeds {}", MAX_READ_FRAMES - 1),
            ));
        }
        let Some(origin) = read_frame_image_position(obj, i as i64) else {
            continue;
        };
        origins.insert(index, origin);
        if first_index < 0 || index < first_index {
            first_index = index;
        }
        if index > last_index {
            last_index = index;
        }
    }
    if origins.is_empty() {
        return Ok(VolumeGeometry {
            row_dir: template.row_dir,
            col_dir: template.col_dir,
            normal: template.normal,
            mean_spacing: fallback_spacing,
            regular: true,
            orientation_stable: true,
            classification: VolumeClassification::SingleSlice,
            ..VolumeGeometry::default()
        });
    }
    let mut step = template.normal.scale(fallback_spacing);
    if first_index != last_index {
        let candidate = origins[&last_index]
            .sub(origins[&first_index])
            .scale(1.0 / (last_index - first_index) as f64);
        if candidate.length() > 0.0 {
            step = candidate;
        }
    }
    let mut slices = Vec::new();
    let anchor = origins[&first_index];
    let stepped = step.length() > 0.0;
    for index in 0..=last_index {
        let mut geometry = template;
        match origins.get(&index) {
            Some(origin) => geometry.origin = *origin,
            None if stepped => {
                geometry.origin = anchor.add(step.scale((index - first_index) as f64));
            }
            None => continue,
        }
        slices.push(geometry);
    }
    let mut geometry = build_volume_geometry(&slices, GeometryTolerances::default_tolerances());
    if geometry.mean_spacing <= 0.0 && fallback_spacing > 0.0 {
        geometry.mean_spacing = fallback_spacing;
    }
    Ok(geometry)
}
fn read_slice_geometry_template(
    obj: &Object,
    rows: i64,
    columns: i64,
) -> Option<(SliceGeometry, f64)> {
    let pixel_measures = obj
        .functional_group_item(0, TAG_PIXEL_MEASURES_SEQUENCE)
        .0?;
    let orientation_item = obj
        .functional_group_item(0, TAG_PLANE_ORIENTATION_SEQUENCE)
        .0?;
    let (row_spacing, column_spacing, values) =
        pixel_measures.spacing_pair(TAG_PIXEL_SPACING).ok()?;
    let orientation = orientation_item
        .get_floats(TAG_IMAGE_ORIENTATION_PATIENT)
        .ok()?;
    if values == spacing_values::ABSENT || orientation.len() < 6 {
        return None;
    }
    let row_dir = Vec3::new(orientation[0], orientation[1], orientation[2]).normalize();
    let col_raw = Vec3::new(orientation[3], orientation[4], orientation[5]);
    let col_dir = col_raw.sub(row_dir.scale(col_raw.dot(row_dir))).normalize();
    let normal = row_dir.cross(col_dir).normalize();
    if row_dir == Vec3::ZERO || col_dir == Vec3::ZERO || normal == Vec3::ZERO {
        return None;
    }
    let mut fallback_spacing = positive_float(&pixel_measures, TAG_SPACING_BETWEEN_SLICES);
    if fallback_spacing == 0.0 {
        fallback_spacing = positive_float(&pixel_measures, TAG_SLICE_THICKNESS);
    }
    Some((
        SliceGeometry {
            origin: Vec3::ZERO,
            row_dir,
            col_dir,
            normal,
            row_spacing,
            col_spacing: column_spacing,
            rows,
            columns,
        },
        fallback_spacing,
    ))
}
fn positive_float(obj: &Object, tag: Tag) -> f64 {
    match obj.get_floats(tag) {
        Ok(values) if !values.is_empty() && positive_finite(values[0]) => values[0],
        _ => 0.0,
    }
}
fn read_segments(obj: &Object) -> Result<Vec<Segment>, Error> {
    let items = dio::sequence(obj, TAG_SEGMENT_SEQUENCE);
    let mut out = Vec::with_capacity(items.len());
    for item in &items {
        let algorithm = read_algorithm_identification(item)?;
        let mut segment = Segment {
            number: dio::int(item, TAG_SEGMENT_NUMBER),
            label: dio::clean_string(item, TAG_SEGMENT_LABEL),
            algorithm_type: dio::clean_string(item, TAG_SEGMENT_ALGORITHM_TYPE),
            algorithm_name: dio::clean_string(item, TAG_SEGMENT_ALGORITHM_NAME),
            algorithm_identification: algorithm,
            recommended_display_cielab: [0; 3],
            segmented_property_category: read_code_sequence(
                item,
                TAG_SEGMENTED_PROPERTY_CATEGORY_SEQ,
            ),
            segmented_property_type: read_code_sequence(item, TAG_SEGMENTED_PROPERTY_TYPE_SEQ),
        };
        if let Some(values) = dio::ints(Some(item), TAG_RECOMMENDED_DISPLAY_CIELAB_VALUE)
            && values.len() >= 3
            && values[..3].iter().all(|v| (0..=0xFFFF).contains(v))
        {
            segment.recommended_display_cielab =
                [values[0] as u16, values[1] as u16, values[2] as u16];
        }
        out.push(segment);
    }
    Ok(out)
}
fn read_code_sequence(obj: &Object, tag: Tag) -> CodedEntry {
    match obj.sequence_item_at(tag, 0) {
        None => CodedEntry::default(),
        Some(item) => CodedEntry {
            code_value: dio::clean_string(&item, TAG_CODE_VALUE),
            coding_scheme_designator: dio::clean_string(&item, TAG_CODING_SCHEME_DESIGNATOR),
            code_meaning: dio::clean_string(&item, TAG_CODE_MEANING),
        },
    }
}
fn read_referenced_images(obj: &Object) -> Vec<ReferencedImage> {
    let mut out = Vec::new();
    for series in dio::sequence(obj, TAG_REFERENCED_SERIES_SEQUENCE) {
        let series_uid = dio::clean_uid(&series, dio::TAG_SERIES_INSTANCE_UID);
        for ref_obj in dio::sequence(&series, TAG_REFERENCED_INSTANCE_SEQUENCE) {
            out.push(ReferencedImage {
                series_instance_uid: series_uid.clone(),
                sop_class_uid: dio::clean_uid(&ref_obj, dio::TAG_REF_SOP_CLASS_UID),
                sop_instance_uid: dio::clean_uid(&ref_obj, dio::TAG_REF_SOP_INSTANCE_UID),
                frames: ref_obj
                    .get_ints(dio::TAG_REF_FRAME_NUMBER)
                    .unwrap_or_default(),
            });
        }
    }
    out
}
fn native_pixels(obj: &Object) -> Cow<'_, [u8]> {
    let Some(elem) = obj.get(dio::TAG_PIXEL_DATA) else {
        return Cow::Borrowed(&[]);
    };
    let Some(data) = elem.raw() else {
        return Cow::Borrowed(&[]);
    };
    if obj.value_big_endian() && elem.vr() == Vr::OW {
        let mut swapped = data.to_vec();
        for pair in swapped.as_chunks_mut::<2>().0 {
            pair.swap(0, 1);
        }
        return Cow::Owned(swapped);
    }
    Cow::Borrowed(data)
}
fn read_frames_layout(
    obj: &Object,
    doc: &Document,
    continuous_binary: bool,
) -> (Vec<Frame>, ReadReport) {
    let items = dio::sequence(obj, TAG_PER_FRAME_FUNCTIONAL_GROUPS);
    let items: Vec<Option<Object>> = if items.is_empty() {
        let count = dio::int(obj, dio::TAG_NUMBER_OF_FRAMES).max(0);
        vec![None; count as usize]
    } else {
        items.into_iter().map(Some).collect()
    };
    let data = native_pixels(obj);
    let report = pixel_data_coverage(doc, items.len() as i64, data.len() as i64);
    let frames = if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        read_label_map_frames(&data, obj, doc, &items)
    } else if doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
        read_fractional_frames(&data, obj, doc, &items)
    } else {
        let bits = doc.rows * doc.columns;
        let stride = if continuous_binary {
            bits
        } else {
            binary_frame_bit_stride(bits, items.len() as i64, data.len() as i64)
        };
        read_binary_frames(&data, obj, doc, &items, stride)
    };
    (frames, report)
}
fn pixel_data_coverage(doc: &Document, frame_count: i64, data_bytes: i64) -> ReadReport {
    let pixels = doc.rows * doc.columns;
    let mut report = ReadReport {
        pixel_data_bytes: data_bytes,
        ..ReadReport::default()
    };
    if pixels <= 0 || frame_count <= 0 {
        return report;
    }
    let sample_bits = if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        if doc.label_map_bits_allocated == 8 {
            8
        } else {
            16
        }
    } else if doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
        8
    } else {
        1
    };
    let frame_bits = pixels * sample_bits;
    let stride = if sample_bits == 1 {
        binary_frame_bit_stride(pixels, frame_count, data_bytes)
    } else {
        frame_bits
    };
    report.expected_pixel_data_bytes = (frame_bits * frame_count + 7) / 8;
    let available = data_bytes * 8;
    for frame in 0..frame_count {
        if frame * stride + frame_bits > available {
            report.incomplete_frames.push(frame);
        }
    }
    report
}
fn read_binary_frames(
    data: &[u8],
    obj: &Object,
    doc: &Document,
    items: &[Option<Object>],
    frame_bit_stride: i64,
) -> Vec<Frame> {
    let spatial = spatial_dimension_value_index(obj);
    let mut frames = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let mut frame = read_frame_item(obj, item.as_ref(), i as i64, spatial, &doc.geometry);
        if frame.segment_number == 0 && doc.segments.len() == 1 {
            frame.segment_number = doc.segments[0].number;
        }
        let base_bit = i as i64 * frame_bit_stride;
        frame.mask = Some(read_packed_binary_mask(
            data,
            base_bit,
            doc.columns,
            doc.rows,
        ));
        frames.push(frame);
    }
    frames
}
pub(crate) fn read_packed_binary_mask(
    data: &[u8],
    base_bit: i64,
    columns: i64,
    rows: i64,
) -> RasterMask {
    let mut mask = RasterMask::new(columns, rows);
    if data.is_empty() || columns <= 0 || rows <= 0 {
        return mask;
    }
    let data_bits = data.len() as i64 * 8;
    for y in 0..rows {
        let row_start = base_bit + y * columns;
        if row_start >= data_bits {
            break;
        }
        let row_end = (row_start + columns).min(data_bits);
        let (mut active_start, mut active_end) = (-1i64, -1i64);
        let mut position = row_start;
        while position < row_end {
            let offset = position & 7;
            let count = (8 - offset).min(row_end - position);
            let value = (data[(position >> 3) as usize] >> offset) & low_bit_mask(count);
            let mut consumed = 0i64;
            while consumed < count {
                let zeros = i64::from((value >> consumed).trailing_zeros().min(8));
                if zeros >= count - consumed {
                    break;
                }
                let start = consumed + zeros;
                let ones = i64::from((!(value >> start)).trailing_zeros()).min(count - start);
                let run_start = position - row_start + start;
                let run_end = run_start + ones;
                if active_start < 0 {
                    active_start = run_start;
                    active_end = run_end;
                } else if run_start == active_end {
                    active_end = run_end;
                } else {
                    mask.set_run(y, active_start, active_end);
                    active_start = run_start;
                    active_end = run_end;
                }
                consumed = start + ones;
            }
            position += count;
        }
        if active_start >= 0 {
            mask.set_run(y, active_start, active_end);
        }
    }
    mask
}
fn binary_frame_bit_stride(bits_per_frame: i64, frame_count: i64, data_bytes: i64) -> i64 {
    let bytes_per_frame = (bits_per_frame + 7) / 8;
    let legacy_bytes = bytes_per_frame * frame_count;
    let continuous_bytes = (bits_per_frame * frame_count + 7) / 8;
    if bits_per_frame % 8 != 0 && data_bytes == legacy_bytes && legacy_bytes != continuous_bytes {
        return bytes_per_frame * 8;
    }
    bits_per_frame
}
fn read_label_map_frames(
    data: &[u8],
    obj: &Object,
    doc: &Document,
    items: &[Option<Object>],
) -> Vec<Frame> {
    let bytes_per_value = if doc.label_map_bits_allocated == 8 {
        1
    } else {
        2
    };
    let value_count = data.len() / bytes_per_value;
    let values_per_frame = (doc.rows * doc.columns) as usize;
    let spatial = spatial_dimension_value_index(obj);
    let mut frames = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let mut frame = read_frame_item(obj, item.as_ref(), i as i64, spatial, &doc.geometry);
        let start = i * values_per_frame;
        if start < value_count {
            let end = (start + values_per_frame).min(value_count);
            let mut labels = vec![0u16; values_per_frame];
            for index in start..end {
                labels[index - start] = if bytes_per_value == 1 {
                    u16::from(data[index])
                } else {
                    u16::from_le_bytes([data[2 * index], data[2 * index + 1]])
                };
            }
            frame.label_map = labels;
        }
        frames.push(frame);
    }
    frames
}
fn read_fractional_frames(
    data: &[u8],
    obj: &Object,
    doc: &Document,
    items: &[Option<Object>],
) -> Vec<Frame> {
    let samples_per_frame = (doc.rows * doc.columns) as usize;
    let spatial = spatial_dimension_value_index(obj);
    let mut frames = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let mut frame = read_frame_item(obj, item.as_ref(), i as i64, spatial, &doc.geometry);
        if frame.segment_number == 0 && doc.segments.len() == 1 {
            frame.segment_number = doc.segments[0].number;
        }
        let start = i * samples_per_frame;
        let end = (start + samples_per_frame).min(data.len());
        if start < end {
            let mut samples = vec![0u8; samples_per_frame];
            samples[..end - start].copy_from_slice(&data[start..end]);
            frame.fractional = samples;
        }
        frames.push(frame);
    }
    frames
}
fn read_frame_item(
    obj: &Object,
    item: Option<&Object>,
    frame_index: i64,
    spatial: i64,
    geometry: &VolumeGeometry,
) -> Frame {
    let mut frame = Frame {
        slice_index: frame_index,
        ..Frame::default()
    };
    if let Some(segment) = obj
        .functional_group_item(frame_index, TAG_SEGMENT_IDENTIFICATION_SEQUENCE)
        .0
    {
        frame.segment_number = dio::int(&segment, TAG_REFERENCED_SEGMENT_NUMBER);
    }
    if let Some(values) = read_dimension_index_values(item)
        && let Some(slice_index) = slice_index_from_dimension_values(&values, spatial)
    {
        frame.slice_index = slice_index;
    }
    let slices = geometry.slices();
    if frame.slice_index >= 0 && (frame.slice_index as usize) < slices.len() {
        frame.geometry = slices[frame.slice_index as usize];
    }
    if let Some(origin) = read_frame_image_position(obj, frame_index) {
        frame.geometry.origin = origin;
        if let Some(slice_index) = slice_index_from_plane_position(origin, geometry) {
            frame.slice_index = slice_index;
            frame.geometry = slices[slice_index as usize];
        }
    }
    let mut source: Option<Object> = item.cloned();
    if let Some(derivation) = obj
        .functional_group_item(frame_index, TAG_DERIVATION_IMAGE_SEQUENCE)
        .0
    {
        frame.derivation_description = derivation
            .get_string(Tag::new(0x0008, 0x2111))
            .unwrap_or_default();
        if let Some(first) = derivation.sequence_item_at(TAG_SOURCE_IMAGE_SEQUENCE, 0) {
            source = Some(first);
        }
    }
    if let Some(source) = &source {
        frame.referenced_sop_instance_uid = dio::clean_uid(source, dio::TAG_REF_SOP_INSTANCE_UID);
        frame.referenced_frame_number = dio::int(source, dio::TAG_REF_FRAME_NUMBER);
    }
    frame
}
fn spatial_dimension_value_index(obj: &Object) -> i64 {
    let items = dio::sequence(obj, TAG_DIMENSION_INDEX_SEQUENCE);
    for (i, item) in items.iter().enumerate() {
        let pointers = read_tag_values(item, TAG_DIMENSION_INDEX_POINTER);
        if pointers.contains(&TAG_IMAGE_POSITION_PATIENT)
            || pointers.contains(&TAG_IN_STACK_POSITION_NUMBER)
        {
            return i as i64;
        }
        if pointers.is_empty()
            && read_tag_values(item, TAG_FUNCTIONAL_GROUP_POINTER)
                .contains(&TAG_PLANE_POSITION_SEQUENCE)
        {
            return i as i64;
        }
    }
    -1
}
fn read_tag_values(obj: &Object, tag: Tag) -> Vec<Tag> {
    let Some(elem) = obj.get(tag) else {
        return Vec::new();
    };
    if elem.vr() != Vr::AT {
        return Vec::new();
    }
    match elem.raw() {
        Some(raw) if raw.len() % 4 == 0 => raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                Tag::new(
                    u16::from_le_bytes([c[0], c[1]]),
                    u16::from_le_bytes([c[2], c[3]]),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}
fn read_dimension_index_values(obj: Option<&Object>) -> Option<Vec<i64>> {
    let obj = obj?;
    let content = obj
        .sequence_item_at(TAG_FRAME_CONTENT_SEQUENCE, 0)
        .filter(|item| item.has(TAG_DIMENSION_INDEX_VALUES));
    let obj = content.as_ref().unwrap_or(obj);
    let elem = obj.get(TAG_DIMENSION_INDEX_VALUES)?;
    match elem.vr() {
        Vr::IS => obj.get_ints(TAG_DIMENSION_INDEX_VALUES).ok(),
        Vr::UL => {
            let raw = elem.raw().filter(|r| r.len() % 4 == 0)?;
            Some(
                raw.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| i64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                    .collect(),
            )
        }
        Vr::US => {
            let raw = elem.raw().filter(|r| r.len() % 2 == 0)?;
            Some(
                raw.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i64::from(u16::from_le_bytes([c[0], c[1]])))
                    .collect(),
            )
        }
        _ => None,
    }
}
fn slice_index_from_dimension_values(values: &[i64], spatial: i64) -> Option<i64> {
    if spatial < 0 || spatial as usize >= values.len() {
        return None;
    }
    let value = values[spatial as usize];
    (value > 0).then_some(value - 1)
}
fn read_frame_image_position(obj: &Object, frame_index: i64) -> Option<Vec3> {
    let position = obj
        .functional_group_item(frame_index, TAG_PLANE_POSITION_SEQUENCE)
        .0?;
    let values = position.get_floats(TAG_IMAGE_POSITION_PATIENT).ok()?;
    (values.len() >= 3).then(|| Vec3::new(values[0], values[1], values[2]))
}
fn slice_index_from_plane_position(origin: Vec3, geometry: &VolumeGeometry) -> Option<i64> {
    let slices = geometry.slices();
    if slices.is_empty() || geometry.normal.length() == 0.0 {
        return None;
    }
    let position = origin.dot(geometry.normal);
    let mut best_index: Option<usize> = None;
    let mut best_distance = f64::INFINITY;
    for (i, slice) in slices.iter().enumerate() {
        let distance = (slice.position_along(geometry.normal) - position).abs();
        if distance < best_distance {
            best_distance = distance;
            best_index = Some(i);
        }
    }
    let best_index = best_index?;
    let slice = &slices[best_index];
    let mut spacing = fmath::min(slice.row_spacing, slice.col_spacing);
    if geometry.mean_spacing > 0.0 {
        spacing = fmath::min(spacing, geometry.mean_spacing);
    }
    if !(spacing > 0.0) {
        spacing = 1.0;
    }
    if best_distance > DICOM_GRID_COORDINATE_TOLERANCE * spacing {
        return None;
    }
    Some(best_index as i64)
}
