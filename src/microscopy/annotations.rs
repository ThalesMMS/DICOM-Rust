use super::{Error, Level, MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE, TAG_SOP_CLASS_UID, tag};
use crate::derivedio;
use crate::fmath::f64_to_i64;
use crate::object::{File, Object, Tag, Value, Vr};
use crate::textfmt::{quote, to_upper};
const MAX_ANNOTATION_POINTS: usize = 10_000_000;
const TAG_ANNOTATION_COORDINATE_TYPE: Tag = tag(0x006A, 0x0001);
const TAG_ANNOTATION_GROUP_SEQUENCE: Tag = tag(0x006A, 0x0002);
const TAG_ANNOTATION_GROUP_UID: Tag = tag(0x006A, 0x0003);
const TAG_ANNOTATION_GROUP_LABEL: Tag = tag(0x006A, 0x0005);
const TAG_ANNOTATION_GROUP_DESCRIPTION: Tag = tag(0x006A, 0x0006);
const TAG_ANNOTATION_GROUP_GENERATION_TYPE: Tag = tag(0x006A, 0x0007);
const TAG_NUMBER_OF_ANNOTATIONS: Tag = tag(0x006A, 0x000C);
const TAG_APPLIES_ALL_OPTICAL_PATHS: Tag = tag(0x006A, 0x000D);
const TAG_REFERENCED_OPTICAL_PATH: Tag = tag(0x006A, 0x000E);
const TAG_APPLIES_ALL_Z_PLANES: Tag = tag(0x006A, 0x000F);
const TAG_COMMON_Z_COORDINATE: Tag = tag(0x006A, 0x0010);
const TAG_ANNOTATION_INDEX_LIST: Tag = tag(0x006A, 0x0011);
const TAG_POINT_COORDINATES_DATA: Tag = tag(0x0066, 0x0016);
const TAG_DOUBLE_POINT_COORDINATES_DATA: Tag = tag(0x0066, 0x0022);
const TAG_LONG_PRIMITIVE_POINT_INDEX_LIST: Tag = tag(0x0066, 0x0040);
const TAG_MEASUREMENTS_SEQUENCE: Tag = tag(0x0066, 0x0121);
const TAG_FLOATING_POINT_VALUES: Tag = tag(0x0066, 0x0125);
const TAG_MEASUREMENT_VALUES_SEQUENCE: Tag = tag(0x0066, 0x0132);
const TAG_GRAPHIC_TYPE: Tag = tag(0x0070, 0x0023);
const TAG_CONCEPT_NAME_CODE_SEQUENCE: Tag = tag(0x0040, 0xA043);
const TAG_MEASUREMENT_UNITS_CODE_SEQUENCE: Tag = tag(0x0040, 0x08EA);
const TAG_CODE_VALUE: Tag = tag(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = tag(0x0008, 0x0102);
const TAG_CODE_MEANING: Tag = tag(0x0008, 0x0104);
const TAG_ANNOTATION_PROPERTY_CATEGORY_CODE: Tag = tag(0x006A, 0x0009);
const TAG_ANNOTATION_PROPERTY_TYPE_CODE: Tag = tag(0x006A, 0x000A);
const TAG_PIXEL_ORIGIN_INTERPRETATION: Tag = tag(0x0048, 0x0301);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodedConcept {
    pub code_value: String,
    pub coding_scheme_designator: String,
    pub code_meaning: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnnotationPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitive {
    pub index: i64,
    pub points: Vec<AnnotationPoint>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationMeasurement {
    pub concept: CodedConcept,
    pub units: CodedConcept,
    pub values: Vec<f64>,
    pub indices: Vec<i64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationGroup {
    pub uid: String,
    pub label: String,
    pub description: String,
    pub generation_type: String,
    pub coordinate_type: String,
    pub pixel_origin: String,
    pub graphic_type: String,
    pub category: CodedConcept,
    pub kind: CodedConcept,
    pub applies_all_optical_paths: bool,
    pub optical_paths: Vec<String>,
    pub applies_all_z_planes: bool,
    pub common_z_coordinates: Vec<f64>,
    pub primitives: Vec<Primitive>,
    pub measurements: Vec<AnnotationMeasurement>,
}
fn err<T>(text: impl Into<String>) -> Result<T, Error> {
    Err(Error::other(text))
}
pub fn read_annotations(file: &File) -> Result<Vec<AnnotationGroup>, Error> {
    let root = &file.dataset;
    let sop_class = derivedio::clean_uid(root, TAG_SOP_CLASS_UID);
    if sop_class != MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE {
        return err(format!(
            "dicom/microscopy: unsupported annotation SOP Class {}",
            quote(&sop_class)
        ));
    }
    let coordinate_type = to_upper(&derivedio::clean_string(
        root,
        TAG_ANNOTATION_COORDINATE_TYPE,
    ));
    if coordinate_type != "2D" && coordinate_type != "3D" {
        return err(format!(
            "dicom/microscopy: invalid annotation coordinate type {}",
            quote(&coordinate_type)
        ));
    }
    let pixel_origin = to_upper(&derivedio::clean_string(
        root,
        TAG_PIXEL_ORIGIN_INTERPRETATION,
    ));
    if coordinate_type == "2D" && pixel_origin != "FRAME" && pixel_origin != "VOLUME" {
        return err(format!(
            "dicom/microscopy: invalid pixel origin interpretation {}",
            quote(&pixel_origin)
        ));
    }
    let items = derivedio::sequence(root, TAG_ANNOTATION_GROUP_SEQUENCE);
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let group = read_annotation_group(item, &coordinate_type, &pixel_origin)
            .map_err(|e| e.prefixed(format!("dicom/microscopy: annotation group {}", index + 1)))?;
        out.push(group);
    }
    Ok(out)
}
fn yes(item: &Object, t: Tag) -> bool {
    derivedio::clean_string(item, t).eq_ignore_ascii_case("YES")
}
fn read_annotation_group(
    item: &Object,
    coordinate_type: &str,
    pixel_origin: &str,
) -> Result<AnnotationGroup, Error> {
    let number = derivedio::int(item, TAG_NUMBER_OF_ANNOTATIONS);
    if number <= 0 {
        return err(format!("invalid Number of Annotations {number}"));
    }
    let graphic_type = to_upper(&derivedio::clean_string(item, TAG_GRAPHIC_TYPE));
    let common_z = derivedio::floats(Some(item), TAG_COMMON_Z_COORDINATE).unwrap_or_default();
    let dimensions = if coordinate_type == "3D" && common_z.is_empty() {
        3
    } else {
        2
    };
    let coordinates = annotation_coordinates(item, dimensions)?;
    let primitives = annotation_primitives(item, &graphic_type, number as usize, &coordinates)?;
    let mut group = AnnotationGroup {
        uid: derivedio::clean_uid(item, TAG_ANNOTATION_GROUP_UID),
        label: derivedio::clean_string(item, TAG_ANNOTATION_GROUP_LABEL),
        description: derivedio::clean_string(item, TAG_ANNOTATION_GROUP_DESCRIPTION),
        generation_type: to_upper(&derivedio::clean_string(
            item,
            TAG_ANNOTATION_GROUP_GENERATION_TYPE,
        )),
        coordinate_type: coordinate_type.to_string(),
        pixel_origin: pixel_origin.to_string(),
        graphic_type,
        category: first_code(item, TAG_ANNOTATION_PROPERTY_CATEGORY_CODE),
        kind: first_code(item, TAG_ANNOTATION_PROPERTY_TYPE_CODE),
        applies_all_optical_paths: yes(item, TAG_APPLIES_ALL_OPTICAL_PATHS),
        optical_paths: strings_at(item, TAG_REFERENCED_OPTICAL_PATH),
        applies_all_z_planes: yes(item, TAG_APPLIES_ALL_Z_PLANES),
        common_z_coordinates: common_z,
        primitives,
        measurements: Vec::new(),
    };
    for measurement in derivedio::sequence(item, TAG_MEASUREMENTS_SEQUENCE) {
        group
            .measurements
            .push(read_annotation_measurement(&measurement, number)?);
    }
    Ok(group)
}
fn annotation_coordinates(item: &Object, dimensions: usize) -> Result<Vec<AnnotationPoint>, Error> {
    let Ok(values) = binary_floats(item, TAG_DOUBLE_POINT_COORDINATES_DATA)
        .or_else(|_| binary_floats(item, TAG_POINT_COORDINATES_DATA))
    else {
        return err("missing point coordinates");
    };
    if values.len() % dimensions != 0 || values.len() / dimensions > MAX_ANNOTATION_POINTS {
        return err(format!(
            "invalid coordinate count {} for {dimensions}D",
            values.len()
        ));
    }
    Ok(values
        .chunks_exact(dimensions)
        .map(|p| AnnotationPoint {
            x: p[0],
            y: p[1],
            z: if dimensions == 3 { p[2] } else { 0.0 },
        })
        .collect())
}
fn annotation_primitives(
    item: &Object,
    graphic_type: &str,
    number: usize,
    points: &[AnnotationPoint],
) -> Result<Vec<Primitive>, Error> {
    let primitive = |index: usize, points: &[AnnotationPoint]| Primitive {
        index: index as i64,
        points: points.to_vec(),
    };
    let starts = binary_uint32s(item, TAG_LONG_PRIMITIVE_POINT_INDEX_LIST).unwrap_or_default();
    if starts.is_empty() {
        return match graphic_type {
            "POINT" => {
                if points.len() != number {
                    return err(format!(
                        "{} POINT coordinates for {number} annotations",
                        points.len()
                    ));
                }
                Ok(points
                    .iter()
                    .enumerate()
                    .map(|(index, point)| primitive(index, std::slice::from_ref(point)))
                    .collect())
            }
            "RECTANGLE" | "ELLIPSE" => {
                if number.checked_mul(4) != Some(points.len()) {
                    return err(format!(
                        "{graphic_type} requires four points per annotation"
                    ));
                }
                Ok(points
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(index, four)| primitive(index, four))
                    .collect())
            }
            _ if number == 1 && !points.is_empty() => Ok(vec![primitive(0, points)]),
            _ => err(format!(
                "{graphic_type} annotations require Long Primitive Point Index List"
            )),
        };
    }
    if starts.len() != number {
        return err(format!(
            "{} primitive indices for {number} annotations",
            starts.len()
        ));
    }
    let mut out = Vec::with_capacity(number);
    for (index, &one_based) in starts.iter().enumerate() {
        if one_based == 0 {
            return err("primitive index is not one-based");
        }
        let start = (one_based - 1) as usize;
        let end = match starts.get(index + 1) {
            Some(next) => next.wrapping_sub(1) as usize,
            None => points.len(),
        };
        if end <= start || end > points.len() {
            return err(format!("invalid primitive point range [{start},{end})"));
        }
        out.push(primitive(index, &points[start..end]));
    }
    Ok(out)
}
fn read_annotation_measurement(
    item: &Object,
    annotation_count: i64,
) -> Result<AnnotationMeasurement, Error> {
    let mut out = AnnotationMeasurement {
        concept: first_code(item, TAG_CONCEPT_NAME_CODE_SEQUENCE),
        units: first_code(item, TAG_MEASUREMENT_UNITS_CODE_SEQUENCE),
        ..AnnotationMeasurement::default()
    };
    for values_item in derivedio::sequence(item, TAG_MEASUREMENT_VALUES_SEQUENCE) {
        let values = binary_floats(&values_item, TAG_FLOATING_POINT_VALUES)
            .map_err(|e| e.prefixed("measurement values"))?;
        let indices = binary_uint32s(&values_item, TAG_ANNOTATION_INDEX_LIST).unwrap_or_default();
        for &index in &indices {
            if index == 0 || i64::from(index) > annotation_count {
                return err(format!("measurement annotation index {index} out of range"));
            }
            out.indices.push(i64::from(index) - 1);
        }
        if !indices.is_empty() && indices.len() != values.len() {
            return err("measurement values and indices differ");
        }
        out.values.extend(values);
    }
    if out.indices.is_empty() && out.values.len() as i64 != annotation_count {
        return err(format!(
            "{} measurement values for {annotation_count} annotations",
            out.values.len()
        ));
    }
    Ok(out)
}
impl AnnotationGroup {
    pub fn slide_primitives(&self, level: &Level) -> Result<Vec<Primitive>, Error> {
        if self.coordinate_type == "3D" {
            return Ok(self.primitives.clone());
        }
        if self.pixel_origin != "VOLUME" {
            return err("dicom/microscopy: FRAME-relative annotations require a frame transform");
        }
        let z = self.common_z_coordinates.first().copied().unwrap_or(0.0);
        let nearest = |v: f64| f64_to_i64((v + 0.5).floor());
        let mut out = self.primitives.clone();
        for primitive in &mut out {
            for point in &mut primitive.points {
                let slide = level.slide_coordinate(nearest(point.x), nearest(point.y), z)?;
                *point = AnnotationPoint {
                    x: slide.x,
                    y: slide.y,
                    z: slide.z,
                };
            }
        }
        Ok(out)
    }
}
fn raw_value(item: &Object, t: Tag) -> Option<(&[u8], Vr)> {
    let element = item.get(t)?;
    match &element.value {
        Value::Raw(raw) => Some((raw, element.vr())),
        _ => None,
    }
}
fn binary_floats(item: &Object, t: Tag) -> Result<Vec<f64>, Error> {
    if item.get(t).is_none() {
        return err(format!("missing {t}"));
    }
    let Some((raw, vr)) = raw_value(item, t) else {
        return err(format!("{t} has no binary value"));
    };
    let big = item.value_big_endian();
    match vr {
        Vr::OF | Vr::FL => {
            if raw.len() % 4 != 0 {
                return err(format!("{t} has invalid OF length"));
            }
            Ok(raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| {
                    let b = [b[0], b[1], b[2], b[3]];
                    f64::from(if big {
                        f32::from_be_bytes(b)
                    } else {
                        f32::from_le_bytes(b)
                    })
                })
                .collect())
        }
        Vr::OD | Vr::FD => {
            if raw.len() % 8 != 0 {
                return err(format!("{t} has invalid OD length"));
            }
            Ok(raw
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| {
                    let b = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
                    if big {
                        f64::from_be_bytes(b)
                    } else {
                        f64::from_le_bytes(b)
                    }
                })
                .collect())
        }
        _ => err(format!("{t} has unsupported VR {vr}")),
    }
}
fn binary_uint32s(item: &Object, t: Tag) -> Option<Vec<u32>> {
    let (raw, _) = raw_value(item, t)?;
    if raw.len() % 4 != 0 {
        return None;
    }
    let big = item.value_big_endian();
    Some(
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|b| {
                let b = [b[0], b[1], b[2], b[3]];
                if big {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                }
            })
            .collect(),
    )
}
fn first_code(item: &Object, t: Tag) -> CodedConcept {
    match derivedio::sequence(item, t).first() {
        None => CodedConcept::default(),
        Some(code) => CodedConcept {
            code_value: derivedio::clean_string(code, TAG_CODE_VALUE),
            coding_scheme_designator: derivedio::clean_string(code, TAG_CODING_SCHEME_DESIGNATOR),
            code_meaning: derivedio::clean_string(code, TAG_CODE_MEANING),
        },
    }
}
fn strings_at(item: &Object, t: Tag) -> Vec<String> {
    item.lookup_strings(t)
        .unwrap_or_default()
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}
