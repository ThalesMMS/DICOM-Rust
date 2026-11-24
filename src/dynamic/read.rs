use super::{AcquisitionTimestamp, Duration, FrameMetadata, Instant, Timeline, build};
use crate::dcmtime;
use crate::derivedio as dio;
use crate::numtext::format_float;
use crate::object::{File, Object, Tag, Value, Vr};
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
const TAG_FRAME_CONTENT_SEQUENCE: Tag = Tag::new(0x0020, 0x9111);
const TAG_DIMENSION_INDEX_SEQUENCE: Tag = Tag::new(0x0020, 0x9222);
const TAG_DIMENSION_INDEX_POINTER: Tag = Tag::new(0x0020, 0x9165);
const TAG_DIMENSION_INDEX_VALUES: Tag = Tag::new(0x0020, 0x9157);
const TAG_TEMPORAL_POSITION_INDEX: Tag = Tag::new(0x0020, 0x9128);
const TAG_TEMPORAL_POSITION_IDENTIFIER: Tag = Tag::new(0x0020, 0x0100);
const TAG_NUMBER_OF_TEMPORAL_POSITIONS: Tag = Tag::new(0x0020, 0x0105);
const TAG_STACK_ID: Tag = Tag::new(0x0020, 0x9056);
const TAG_IN_STACK_POSITION_NUMBER: Tag = Tag::new(0x0020, 0x9057);
const TAG_FRAME_ACQUISITION_DATE_TIME: Tag = Tag::new(0x0018, 0x9074);
const TAG_ACQUISITION_DATE_TIME: Tag = Tag::new(0x0008, 0x002A);
const TAG_ACQUISITION_DATE: Tag = Tag::new(0x0008, 0x0022);
const TAG_ACQUISITION_TIME: Tag = Tag::new(0x0008, 0x0032);
const TAG_ACQUISITION_ZONE: Tag = Tag::new(0x0008, 0x0201);
const TAG_FRAME_REFERENCE_TIME: Tag = Tag::new(0x0054, 0x1300);
const TAG_TRIGGER_TIME: Tag = Tag::new(0x0018, 0x1060);
const TAG_NOMINAL_CARDIAC_TRIGGER_DELAY_TIME: Tag = Tag::new(0x0020, 0x9153);
const TAG_NOMINAL_CARDIAC_PHASE_PERCENT: Tag = Tag::new(0x0020, 0x9241);
const TAG_ACTUAL_FRAME_DURATION: Tag = Tag::new(0x0018, 0x1242);
const TAG_FRAME_TIME: Tag = Tag::new(0x0018, 0x1063);
const TAG_FRAME_TIME_VECTOR: Tag = Tag::new(0x0018, 0x1065);
const MILLISECOND: f64 = 1_000_000.0;
const GO_ZERO_TIME: i128 = -62_135_596_800 * 1_000_000_000;
pub fn read(file: &File) -> Result<Timeline, super::Error> {
    read_timeline(file, true).map_err(super::Error::new)
}
pub fn read_frame_metadata(file: &File) -> Result<Vec<FrameMetadata>, super::Error> {
    read_timeline(file, false)
        .map(|timeline| timeline.frames)
        .map_err(super::Error::new)
}
fn finite(value: f64) -> bool {
    !value.is_nan() && !value.is_infinite()
}
fn value_text(value: f64) -> String {
    format_float(value, 'g', -1)
}
fn read_timeline(file: &File, validate_declared_positions: bool) -> Result<Timeline, String> {
    let obj = &file.dataset;
    let mut frame_count = dio::int(obj, TAG_NUMBER_OF_FRAMES);
    if frame_count <= 0 {
        frame_count = 1;
    }
    if frame_count > 1_000_000 {
        return Err(format!(
            "dicom/dynamic: unreasonable frame count {frame_count}"
        ));
    }
    let per_frame = dio::sequence(obj, TAG_PER_FRAME_FUNCTIONAL_GROUPS);
    if !per_frame.is_empty() && per_frame.len() as i64 != frame_count {
        return Err(format!(
            "dicom/dynamic: {} per-frame groups for {} frames",
            per_frame.len(),
            frame_count
        ));
    }
    let shared = first_sequence_item(obj, TAG_SHARED_FUNCTIONAL_GROUPS);
    let pointers = dimension_pointers(obj);
    let vector = numeric_values(Some(obj), TAG_FRAME_TIME_VECTOR);
    if !vector.is_empty() {
        if vector.len() as i64 != frame_count {
            return Err(format!(
                "dicom/dynamic: FrameTimeVector has {} values for {} frames",
                vector.len(),
                frame_count
            ));
        }
        for (index, &increment) in vector.iter().enumerate() {
            if !finite(increment) || increment < 0.0 {
                return Err(format!(
                    "dicom/dynamic: invalid FrameTimeVector increment {} at frame {}",
                    value_text(increment),
                    index
                ));
            }
        }
        if vector[0] != 0.0 {
            return Err(format!(
                "dicom/dynamic: first FrameTimeVector increment is {}, want 0",
                value_text(vector[0])
            ));
        }
    }
    let frame_time = direct_milliseconds(obj, TAG_FRAME_TIME);
    let legacy_position = direct_int(Some(obj), TAG_TEMPORAL_POSITION_IDENTIFIER);
    let number_of_positions = direct_int(Some(obj), TAG_NUMBER_OF_TEMPORAL_POSITIONS);
    let mut frames = Vec::with_capacity(frame_count as usize);
    let mut vector_offset: Duration = 0;
    for index in 0..frame_count {
        let containers: [Option<&Object>; 2] = [per_frame.get(index as usize), shared.as_ref()];
        let mut frame = FrameMetadata {
            frame_index: index,
            number_of_temporal_positions: number_of_positions.unwrap_or(0),
            has_number_of_temporal_positions: number_of_positions.is_some(),
            ..Default::default()
        };
        if let Some(v) = first_frame_int(&containers, obj, TAG_TEMPORAL_POSITION_INDEX) {
            (frame.temporal_position, frame.has_temporal_position) = (v, true);
        }
        frame.stack_id = first_frame_string(&containers, obj, TAG_STACK_ID).unwrap_or_default();
        if let Some(v) = first_frame_int(&containers, obj, TAG_IN_STACK_POSITION_NUMBER) {
            (frame.in_stack_position, frame.has_in_stack_position) = (v, true);
        }
        if let Some(content) =
            first_recursive_sequence_item(&containers, TAG_FRAME_CONTENT_SEQUENCE)
        {
            apply_dimension_values(
                &mut frame,
                &pointers,
                &dio::ints(Some(&content), TAG_DIMENSION_INDEX_VALUES).unwrap_or_default(),
            );
        }
        if !frame.has_temporal_position
            && let Some(position) = legacy_position
        {
            (frame.temporal_position, frame.has_temporal_position) = (position, true);
        }
        if let Some(ms) = first_frame_float(&containers, obj, TAG_FRAME_REFERENCE_TIME) {
            (frame.offset, frame.has_offset) = milliseconds_duration(ms);
        }
        if let Some(ms) = first_frame_float(&containers, obj, TAG_TRIGGER_TIME) {
            (frame.trigger, frame.has_trigger) = milliseconds_duration(ms);
        } else if let Some(ms) =
            first_frame_float(&containers, obj, TAG_NOMINAL_CARDIAC_TRIGGER_DELAY_TIME)
        {
            (frame.trigger, frame.has_trigger) = milliseconds_duration(ms);
        }
        if let Some(percent) =
            first_frame_float(&containers, obj, TAG_NOMINAL_CARDIAC_PHASE_PERCENT)
            && finite(percent)
        {
            (frame.phase, frame.has_phase) = (percent, true);
        }
        if let Some(ms) = first_frame_float(&containers, obj, TAG_ACTUAL_FRAME_DURATION) {
            (frame.duration, frame.has_duration) = milliseconds_duration(ms);
            (frame.actual_duration, frame.has_actual_duration) =
                (frame.duration, frame.has_duration);
        } else if !vector.is_empty() {
            let mut duration_index = index as usize + 1;
            if duration_index >= vector.len() {
                duration_index = index as usize;
            }
            if duration_index < vector.len() {
                (frame.duration, frame.has_duration) =
                    milliseconds_duration(vector[duration_index]);
            }
        } else if let Some(ms) = frame_time {
            (frame.duration, frame.has_duration) = milliseconds_duration(ms);
        }
        if !vector.is_empty() {
            if index > 0 && (index as usize) < vector.len() {
                let (increment, ok) = milliseconds_duration(vector[index as usize]);
                if ok {
                    match add_duration(vector_offset, increment) {
                        Some(sum) => vector_offset = sum,
                        None => {
                            return Err(format!(
                                "dicom/dynamic: FrameTimeVector overflows at frame {index}"
                            ));
                        }
                    }
                }
            }
            if !frame.has_offset {
                (frame.offset, frame.has_offset) = (vector_offset, true);
            }
        } else if let Some(ms) = frame_time
            && !frame.has_offset
        {
            let (step, _) = milliseconds_duration(ms);
            match multiply_duration(step, index) {
                Some(offset) => (frame.offset, frame.has_offset) = (offset, true),
                None => {
                    return Err(format!(
                        "dicom/dynamic: FrameTime overflows at frame {index}"
                    ));
                }
            }
        }
        if let Some(value) = first_frame_string(&containers, obj, TAG_FRAME_ACQUISITION_DATE_TIME) {
            (frame.acquisition_time, frame.has_acquisition_time) = parse_date_time(&value);
        } else if let Some(value) = first_frame_string(&containers, obj, TAG_ACQUISITION_DATE_TIME)
        {
            (frame.acquisition_time, frame.has_acquisition_time) = parse_date_time(&value);
        }
        frame.acquisition_stamp = read_acquisition_timestamp(obj, &containers, frame_count);
        let geometry = obj.frame_geometry_at(index);
        if let (Some(position), Some(orientation)) = (
            geometry.image_position_patient.as_deref(),
            geometry.image_orientation_patient.as_deref(),
        ) && position.len() == 3
            && orientation.len() == 6
        {
            let row = vector3(&orientation[..3]);
            let column = vector3(&orientation[3..6]);
            let normal = cross(row, column);
            let origin = vector3(position);
            if length(normal) > 0.0 && origin.iter().all(|v| finite(*v)) {
                frame.spatial_position = dot(origin, normalize(normal));
                frame.has_spatial_position = finite(frame.spatial_position);
            }
        }
        frames.push(frame);
    }
    let timeline = build(&frames);
    if validate_declared_positions
        && let Some(declared) = number_of_positions
        && declared > 1
        && timeline.points().len() <= 1
    {
        return Err(format!(
            "dicom/dynamic: NumberOfTemporalPositions={declared} but temporal frames cannot be separated"
        ));
    }
    Ok(timeline)
}
fn dimension_pointers(obj: &Object) -> Vec<Tag> {
    dio::sequence(obj, TAG_DIMENSION_INDEX_SEQUENCE)
        .iter()
        .map(|item| {
            let Some(element) = item.get(TAG_DIMENSION_INDEX_POINTER) else {
                return Tag::new(0, 0);
            };
            let Value::Raw(raw) = &element.value else {
                return Tag::new(0, 0);
            };
            if raw.len() < 4 {
                return Tag::new(0, 0);
            }
            let read = |b: &[u8]| {
                if item.value_big_endian() {
                    u16::from_be_bytes([b[0], b[1]])
                } else {
                    u16::from_le_bytes([b[0], b[1]])
                }
            };
            Tag::new(read(&raw[..2]), read(&raw[2..4]))
        })
        .collect()
}
fn apply_dimension_values(frame: &mut FrameMetadata, pointers: &[Tag], values: &[i64]) {
    if pointers.len() != values.len() {
        return;
    }
    for (index, pointer) in pointers.iter().enumerate() {
        let value = values[index];
        if *pointer == TAG_TEMPORAL_POSITION_INDEX {
            if !frame.has_temporal_position {
                (frame.temporal_position, frame.has_temporal_position) = (value, true);
            }
        } else if *pointer == TAG_IN_STACK_POSITION_NUMBER && !frame.has_in_stack_position {
            (frame.in_stack_position, frame.has_in_stack_position) = (value, true);
        }
    }
}
fn first_recursive_sequence_item(containers: &[Option<&Object>], tag: Tag) -> Option<Object> {
    containers
        .iter()
        .find_map(|container| recursive_sequence_item(*container, tag, 0))
}
fn recursive_sequence_item(obj: Option<&Object>, tag: Tag, depth: i64) -> Option<Object> {
    let obj = obj?;
    if depth > 4 {
        return None;
    }
    if let Some(first) = dio::sequence(obj, tag).into_iter().next() {
        return Some(first);
    }
    for element in obj.elements() {
        if element.vr() != Vr::SQ {
            continue;
        }
        let Some(items) = obj.get_sequence(element.tag()) else {
            continue;
        };
        for item in &items {
            if let Some(found) = recursive_sequence_item(Some(item), tag, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}
fn first_sequence_item(obj: &Object, tag: Tag) -> Option<Object> {
    dio::sequence(obj, tag).into_iter().next()
}
fn first_frame_int(containers: &[Option<&Object>], root: &Object, tag: Tag) -> Option<i64> {
    for container in containers {
        if let Some(target) = recursive_object_with_tag(*container, tag, 0)
            && let Some(first) = dio::ints(Some(&target), tag).and_then(|v| v.first().copied())
        {
            return Some(first);
        }
    }
    direct_int(Some(root), tag)
}
fn direct_int(obj: Option<&Object>, tag: Tag) -> Option<i64> {
    dio::ints(obj, tag).and_then(|v| v.first().copied())
}
fn first_frame_float(containers: &[Option<&Object>], root: &Object, tag: Tag) -> Option<f64> {
    for container in containers {
        if let Some(target) = recursive_object_with_tag(*container, tag, 0) {
            let values = numeric_values(Some(&target), tag);
            if let Some(&first) = values.first()
                && finite(first)
            {
                return Some(first);
            }
        }
    }
    let values = numeric_values(Some(root), tag);
    match values.first() {
        Some(&first) if finite(first) => Some(first),
        _ => None,
    }
}
fn direct_milliseconds(obj: &Object, tag: Tag) -> Option<f64> {
    let values = numeric_values(Some(obj), tag);
    match values.first() {
        Some(&first) if finite(first) && first >= 0.0 => Some(first),
        _ => None,
    }
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
fn first_frame_string(containers: &[Option<&Object>], root: &Object, tag: Tag) -> Option<String> {
    for container in containers {
        if let Some(target) = recursive_object_with_tag(*container, tag, 0) {
            let value = trim_unicode_space(&dio::clean_string(&target, tag)).to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    let value = trim_unicode_space(&dio::clean_string(root, tag)).to_string();
    (!value.is_empty()).then_some(value)
}
fn recursive_object_with_tag(obj: Option<&Object>, tag: Tag, depth: i64) -> Option<Object> {
    let obj = obj?;
    if depth > 4 {
        return None;
    }
    if obj.get(tag).is_some() {
        return Some(obj.clone());
    }
    for element in obj.elements() {
        if element.vr() != Vr::SQ {
            continue;
        }
        let Some(items) = obj.get_sequence(element.tag()) else {
            continue;
        };
        for item in &items {
            if let Some(found) = recursive_object_with_tag(Some(item), tag, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}
fn numeric_values(obj: Option<&Object>, tag: Tag) -> Vec<f64> {
    if let Some(values) = dio::floats(obj, tag)
        && !values.is_empty()
    {
        return values;
    }
    dio::ints(obj, tag)
        .unwrap_or_default()
        .into_iter()
        .map(|v| v as f64)
        .collect()
}
fn read_acquisition_timestamp(
    root: &Object,
    groups: &[Option<&Object>],
    frames: i64,
) -> AcquisitionTimestamp {
    let mut raw = String::new();
    for group in groups.iter().flatten() {
        if !group.has(TAG_FRAME_CONTENT_SEQUENCE) {
            continue;
        }
        let content = match group.get_sequence(TAG_FRAME_CONTENT_SEQUENCE) {
            Some(items) if items.len() == 1 => items,
            _ => return AcquisitionTimestamp::default(),
        };
        if !content[0].has(TAG_FRAME_ACQUISITION_DATE_TIME) {
            continue;
        }
        if !raw.is_empty() {
            return AcquisitionTimestamp::default();
        }
        raw = single_acquisition_value(&content[0], TAG_FRAME_ACQUISITION_DATE_TIME);
        if raw.is_empty() {
            return AcquisitionTimestamp::default();
        }
    }
    if raw.is_empty() {
        if frames != 1 {
            return AcquisitionTimestamp::default();
        }
        if root.has(TAG_ACQUISITION_DATE_TIME) {
            raw = single_acquisition_value(root, TAG_ACQUISITION_DATE_TIME);
        } else {
            let date = single_acquisition_value(root, TAG_ACQUISITION_DATE);
            let clock = single_acquisition_value(root, TAG_ACQUISITION_TIME);
            if date.len() != 8 || clock.len() < 6 {
                return AcquisitionTimestamp::default();
            }
            raw = date + &clock;
        }
    }
    let mut parsed = match dcmtime::parse_datetime(&raw) {
        Ok(parsed) if parsed.precision <= dcmtime::PrecisionLevel::SECONDS => parsed,
        _ => return AcquisitionTimestamp::default(),
    };
    if parsed.no_offset && root.has(TAG_ACQUISITION_ZONE) {
        let zone = single_acquisition_value(root, TAG_ACQUISITION_ZONE);
        if zone.len() != 5 {
            return AcquisitionTimestamp::default();
        }
        parsed = match dcmtime::parse_datetime(&(raw + &zone)) {
            Ok(parsed) if !parsed.no_offset => parsed,
            _ => return AcquisitionTimestamp::default(),
        };
    }
    AcquisitionTimestamp {
        time: Some(parsed.time),
        utc_offset_known: !parsed.no_offset,
    }
}
fn single_acquisition_value(object: &Object, tag: Tag) -> String {
    match object.get_strings(tag) {
        Some(values) if values.len() == 1 => trim_unicode_space(&values[0]).to_string(),
        _ => String::new(),
    }
}
fn parse_date_time(value: &str) -> (Instant, bool) {
    match dcmtime::parse_datetime(trim_unicode_space(value)) {
        Ok(parsed) => (Instant(GO_ZERO_TIME + parsed.time.nanos_since_zero()), true),
        Err(_) => (Instant::default(), false),
    }
}
fn milliseconds_duration(value: f64) -> (Duration, bool) {
    if !finite(value) || value < 0.0 || value > i64::MAX as f64 / MILLISECOND {
        return (0, false);
    }
    (
        crate::fmath::f64_to_i64((value * MILLISECOND).round()),
        true,
    )
}
fn add_duration(a: Duration, b: Duration) -> Option<Duration> {
    if b > 0 && a > i64::MAX - b {
        return None;
    }
    Some(a.wrapping_add(b))
}
fn multiply_duration(value: Duration, factor: i64) -> Option<Duration> {
    if value < 0 || factor < 0 {
        return None;
    }
    if value == 0 || factor == 0 {
        return Some(0);
    }
    if value > i64::MAX / factor {
        return None;
    }
    Some(value * factor)
}
fn vector3(values: &[f64]) -> [f64; 3] {
    if values.len() < 3 {
        return [0.0; 3];
    }
    [values[0], values[1], values[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}
fn normalize(v: [f64; 3]) -> [f64; 3] {
    let l = length(v);
    if l == 0.0 || !finite(l) {
        return [0.0; 3];
    }
    [v[0] / l, v[1] / l, v[2] / l]
}
