use super::mappings::TAG_LUT_LABEL;
use super::{
    Code, Error, Kind, PARAMETRIC_MAP_STORAGE, Reference, TAG_CODE_MEANING, TAG_CODE_VALUE,
    TAG_CODING_SCHEME_DESIGNATOR, TAG_CONCEPT_CODE_SEQ, TAG_CONCEPT_NAME_CODE_SEQ,
    TAG_DIMENSION_INDEX_POINTER, TAG_DIMENSION_INDEX_SEQ, TAG_DIMENSION_INDEX_VALUES,
    TAG_DOUBLE_RWV_FIRST_MAPPED, TAG_DOUBLE_RWV_LAST_MAPPED, TAG_FLOAT_PIXEL_DATA,
    TAG_FRAME_CONTENT_SEQ, TAG_IMAGE_POSITION_PATIENT, TAG_MEASUREMENT_UNITS_CODE_SEQ,
    TAG_PER_FRAME_FUNCTIONAL_GROUPS, TAG_QUANTITY_DEFINITION_SEQ, TAG_REAL_WORLD_VALUE_MAPPING_SEQ,
    TAG_REFERENCED_SOP_CLASS_UID, TAG_REFERENCED_SOP_INSTANCE_UID, TAG_RWV_INTERCEPT,
    TAG_RWV_SLOPE, TAG_SHARED_FUNCTIONAL_GROUPS, checked_product, finite, finite_vec,
    positive_finite, trim_unicode_space,
};
use crate::dcmtime::ZonedTime;
use crate::derivation::{self, Derivation, DerivedCommon, Source};
use crate::derivedio as dio;
use crate::fmath;
use crate::object::inherit::inherit_modules;
use crate::object::{DataSet, Element, File, Tag, Vr};
use crate::render::SliceGeometry;
use std::collections::HashSet;
pub type Identification = dio::Identification;
pub const DEFAULT_CONTENT_LABEL: &str = "PARAMETRIC_MAP";
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub modality: String,
    pub series_description: String,
    pub rows: i64,
    pub columns: i64,
    pub frames: Vec<DocumentFrame>,
    pub quantity: Code,
    pub units: Code,
    pub derivation_code: Code,
    pub derivation_description: String,
    pub lut_label: String,
    pub derived_pixel_contrast: String,
    pub content_qualification: String,
    pub source_series_instance_uid: String,
    pub common: DerivedCommon,
    pub identification: Identification,
    pub content_date: String,
    pub content_time: String,
    pub now: Option<ZonedTime>,
}
#[derive(Debug, Clone, Default)]
pub struct DocumentFrame {
    pub geometry: SliceGeometry,
    pub values: Vec<f32>,
    pub sources: Vec<Reference>,
}
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_FRAME_TYPE: Tag = Tag::new(0x0008, 0x9007);
const TAG_PARAMETRIC_MAP_FRAME_TYPE: Tag = Tag::new(0x0040, 0x9092);
const TAG_CONTENT_QUALIFICATION: Tag = Tag::new(0x0018, 0x9004);
const TAG_LOSSY_IMAGE_COMPRESSION: Tag = Tag::new(0x0028, 0x2110);
const TAG_BURNED_IN_ANNOTATION: Tag = Tag::new(0x0028, 0x0301);
const TAG_RECOGNIZABLE_VISUAL: Tag = Tag::new(0x0028, 0x0302);
const TAG_PRESENTATION_LUT_SHAPE: Tag = Tag::new(0x2050, 0x0020);
const TAG_PIXEL_MEASURES_SEQ: Tag = Tag::new(0x0028, 0x9110);
const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
const TAG_SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
const TAG_SPACING_BETWEEN_SLICES: Tag = Tag::new(0x0018, 0x0088);
const TAG_PLANE_ORIENTATION_SEQ: Tag = Tag::new(0x0020, 0x9116);
const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
const TAG_PLANE_POSITION_SEQ: Tag = Tag::new(0x0020, 0x9113);
const TAG_FRAME_ANATOMY_SEQ: Tag = Tag::new(0x0020, 0x9071);
const TAG_FRAME_LATERALITY: Tag = Tag::new(0x0020, 0x9072);
const TAG_ANATOMIC_REGION_SEQ: Tag = Tag::new(0x0008, 0x2218);
const TAG_DIMENSION_ORGANIZATION_SEQ: Tag = Tag::new(0x0020, 0x9221);
const TAG_DIMENSION_ORGANIZATION_UID: Tag = Tag::new(0x0020, 0x9164);
const TAG_FUNCTIONAL_GROUP_POINTER: Tag = Tag::new(0x0020, 0x9167);
const TAG_DERIVATION_IMAGE_SEQ: Tag = Tag::new(0x0008, 0x9124);
const TAG_REFERENCED_SERIES_SEQ: Tag = Tag::new(0x0008, 0x1115);
const TAG_REFERENCED_INSTANCE_SEQ: Tag = Tag::new(0x0008, 0x114A);
const TAG_ACQUISITION_CONTEXT_SEQ: Tag = Tag::new(0x0040, 0x0555);
const TAG_LUT_EXPLANATION: Tag = Tag::new(0x0028, 0x3003);
const TAG_VALUE_TYPE: Tag = Tag::new(0x0040, 0xA040);
const TAG_SERIES_DESCRIPTION: Tag = Tag::new(0x0008, 0x103E);
fn or_default<'a>(value: &'a str, default: &'a str) -> &'a str {
    let v = trim_unicode_space(value);
    if v.is_empty() { default } else { v }
}
fn system_now() -> ZonedTime {
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
    doc.validate()?;
    let identification = doc
        .identification
        .elements(DEFAULT_CONTENT_LABEL)
        .map_err(|e| Error::wrap(Kind::InvalidObject, e))?;
    let mut content_date = doc.content_date.clone();
    let mut content_time = doc.content_time.clone();
    if content_date.is_empty() || content_time.is_empty() {
        let now = doc.now.unwrap_or_else(system_now);
        if content_date.is_empty() {
            let (y, m, d) = now.date();
            content_date = format!("{y:04}{m:02}{d:02}");
        }
        if content_time.is_empty() {
            content_time = format!("{:02}{:02}{:02}", now.hour(), now.minute(), now.second());
        }
    }
    let modality = or_default(&doc.modality, "MR");
    let contrast = or_default(&doc.derived_pixel_contrast, "QUANTITY");
    let qualification = or_default(&doc.content_qualification, "RESEARCH");
    let image_type: Vec<String> = ["DERIVED", "PRIMARY", "VOLUME", contrast]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let organization = dio::ui(
        TAG_DIMENSION_ORGANIZATION_UID,
        &dio::derived_uid(&[&doc.sop_instance_uid, "dimension-organization"]),
    );
    let first = doc.frames[0].geometry;
    let mut elements = vec![
        dio::ui(dio::TAG_SOP_CLASS_UID, PARAMETRIC_MAP_STORAGE),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &doc.sop_instance_uid),
        dio::ui(dio::TAG_STUDY_INSTANCE_UID, &doc.study_instance_uid),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, &doc.series_instance_uid),
        dio::ui(dio::TAG_FRAME_OF_REFERENCE_UID, &doc.frame_of_reference_uid),
        dio::cs(dio::TAG_MODALITY, modality),
        dio::strings(TAG_IMAGE_TYPE, Vr::CS, &image_type),
        dio::str_elem(dio::TAG_CONTENT_DATE, Vr::DA, &content_date),
        dio::str_elem(dio::TAG_CONTENT_TIME, Vr::TM, &content_time),
        dio::cs(TAG_CONTENT_QUALIFICATION, qualification),
        dio::cs(TAG_LOSSY_IMAGE_COMPRESSION, "00"),
        dio::cs(TAG_BURNED_IN_ANNOTATION, "NO"),
        dio::cs(TAG_RECOGNIZABLE_VISUAL, "NO"),
        dio::cs(TAG_PRESENTATION_LUT_SHAPE, "IDENTITY"),
        dio::us(dio::TAG_ROWS, &[doc.rows as u16]),
        dio::us(dio::TAG_COLUMNS, &[doc.columns as u16]),
        dio::is(dio::TAG_NUMBER_OF_FRAMES, &[doc.frames.len() as i64]),
        dio::us(dio::TAG_SAMPLES_PER_PIXEL, &[1]),
        dio::cs(dio::TAG_PHOTOMETRIC_INTERPRETATION, "MONOCHROME2"),
        dio::us(dio::TAG_BITS_ALLOCATED, &[32]),
        dio::seq(TAG_ACQUISITION_CONTEXT_SEQ, Vec::new()),
        dio::seq(
            TAG_DIMENSION_ORGANIZATION_SEQ,
            vec![dio::data_set(vec![organization.clone()])],
        ),
        dio::seq(
            TAG_DIMENSION_INDEX_SEQ,
            vec![dio::data_set(vec![
                pointer_element(TAG_DIMENSION_INDEX_POINTER, TAG_IMAGE_POSITION_PATIENT),
                pointer_element(TAG_FUNCTIONAL_GROUP_POINTER, TAG_PLANE_POSITION_SEQ),
                organization,
            ])],
        ),
        dio::seq(
            TAG_SHARED_FUNCTIONAL_GROUPS,
            vec![dio::data_set(vec![
                dio::seq(
                    TAG_PIXEL_MEASURES_SEQ,
                    vec![dio::data_set(pixel_measures(doc))],
                ),
                dio::seq(
                    TAG_PLANE_ORIENTATION_SEQ,
                    vec![dio::data_set(vec![dio::ds(
                        TAG_IMAGE_ORIENTATION_PATIENT,
                        &[
                            first.row_dir.x,
                            first.row_dir.y,
                            first.row_dir.z,
                            first.col_dir.x,
                            first.col_dir.y,
                            first.col_dir.z,
                        ],
                    )])],
                ),
                dio::seq(TAG_REAL_WORLD_VALUE_MAPPING_SEQ, vec![doc.mapping_item()]),
                dio::seq(
                    TAG_PARAMETRIC_MAP_FRAME_TYPE,
                    vec![dio::data_set(vec![dio::strings(
                        TAG_FRAME_TYPE,
                        Vr::CS,
                        &image_type,
                    )])],
                ),
                dio::seq(
                    TAG_FRAME_ANATOMY_SEQ,
                    vec![dio::data_set(vec![
                        dio::cs(TAG_FRAME_LATERALITY, "U"),
                        dio::seq(
                            TAG_ANATOMIC_REGION_SEQ,
                            vec![code_item(&Code::new("38266002", "SCT", "Entire body"))],
                        ),
                    ])],
                ),
            ])],
        ),
    ];
    let description = trim_unicode_space(&doc.series_description);
    if !description.is_empty() {
        elements.push(dio::lo(TAG_SERIES_DESCRIPTION, description));
    }
    elements.extend(identification);
    let mut per_frame = Vec::with_capacity(doc.frames.len());
    let mut pixels = Vec::with_capacity(doc.frames.len() * (doc.rows * doc.columns) as usize * 4);
    let mut references: Vec<&Reference> = Vec::new();
    let mut seen = HashSet::new();
    for (i, frame) in doc.frames.iter().enumerate() {
        let g = &frame.geometry;
        let mut items = vec![
            dio::seq(
                TAG_FRAME_CONTENT_SEQ,
                vec![dio::data_set(vec![dio::ul(
                    TAG_DIMENSION_INDEX_VALUES,
                    &[(i + 1) as u32],
                )])],
            ),
            dio::seq(
                TAG_PLANE_POSITION_SEQ,
                vec![dio::data_set(vec![dio::ds(
                    TAG_IMAGE_POSITION_PATIENT,
                    &[g.origin.x, g.origin.y, g.origin.z],
                )])],
            ),
        ];
        if !frame.sources.is_empty() {
            let code = doc.derivation_code();
            let mut d = Derivation {
                description: doc.derivation_description.clone(),
                codes: vec![derivation::Code::new(
                    &code.value,
                    &code.scheme,
                    &code.meaning,
                )],
                sources: Vec::new(),
            };
            for source in &frame.sources {
                d.sources.push(Source {
                    sop_class_uid: source.sop_class_uid.clone(),
                    sop_instance_uid: source.sop_instance_uid.clone(),
                    frames: source.frames.clone(),
                    purpose: Some(derivation::codes::SourceImageForProcessing()),
                });
                if seen.insert(source.sop_instance_uid.as_str()) {
                    references.push(source);
                }
            }
            let item = d
                .item()
                .map_err(|e| Error::wrap(Kind::InvalidObject, format!("frame {}: {e}", i + 1)))?;
            items.push(dio::seq(TAG_DERIVATION_IMAGE_SEQ, vec![item]));
        }
        per_frame.push(dio::data_set(items));
        for v in &frame.values {
            pixels.extend_from_slice(&v.to_bits().to_le_bytes());
        }
    }
    elements.push(dio::seq(TAG_PER_FRAME_FUNCTIONAL_GROUPS, per_frame));
    if !references.is_empty() {
        let instances = references
            .iter()
            .map(|r| {
                dio::data_set(vec![
                    dio::ui(TAG_REFERENCED_SOP_CLASS_UID, &r.sop_class_uid),
                    dio::ui(TAG_REFERENCED_SOP_INSTANCE_UID, &r.sop_instance_uid),
                ])
            })
            .collect();
        elements.push(dio::seq(
            TAG_REFERENCED_SERIES_SEQ,
            vec![dio::data_set(vec![
                dio::ui(
                    dio::TAG_SERIES_INSTANCE_UID,
                    &doc.source_series_instance_uid,
                ),
                dio::seq(TAG_REFERENCED_INSTANCE_SEQ, instances),
            ])],
        ));
    }
    elements.push(dio::raw(TAG_FLOAT_PIXEL_DATA, Vr::OF, pixels));
    let mut dataset = dio::object(elements);
    doc.common
        .apply(&mut dataset, inherit_modules::ALL)
        .map_err(Error::other)?;
    dio::file(PARAMETRIC_MAP_STORAGE, &doc.sop_instance_uid, Some(dataset)).map_err(Error::other)
}
impl Document {
    fn validate(&self) -> Result<(), Error> {
        for (name, uid) in [
            ("SOP Instance", &self.sop_instance_uid),
            ("Study Instance", &self.study_instance_uid),
            ("Series Instance", &self.series_instance_uid),
            ("Frame of Reference", &self.frame_of_reference_uid),
        ] {
            if trim_unicode_space(uid).is_empty() {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    format!("{name} UID is required"),
                ));
            }
        }
        let frames = self.frames.len() as i64;
        if self.rows <= 0
            || self.columns <= 0
            || self.rows > i64::from(u16::MAX)
            || self.columns > i64::from(u16::MAX)
            || frames == 0
        {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!(
                    "{}\u{d7}{} pixels in {frames} frames",
                    self.columns, self.rows
                ),
            ));
        }
        match checked_product(&[self.rows, self.columns, frames]) {
            Some(v) if v <= 64 * 1024 * 1024 => {}
            _ => {
                return Err(Error::wrap(
                    Kind::MemoryLimit,
                    format!("{}\u{d7}{}\u{d7}{frames} samples", self.columns, self.rows),
                ));
            }
        }
        if self.quantity.string().is_empty()
            || self.units.string().is_empty()
            || self.quantity.value.is_empty()
            || self.units.value.is_empty()
        {
            return Err(Error::wrap(
                Kind::MissingMapping,
                "the quantity and its units are required",
            ));
        }
        let first = self.frames[0].geometry;
        for (i, frame) in self.frames.iter().enumerate() {
            let n = i + 1;
            if frame.values.len() as i64 != self.rows * self.columns {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    format!(
                        "frame {n} has {} values, want {}",
                        frame.values.len(),
                        self.rows * self.columns
                    ),
                ));
            }
            let g = &frame.geometry;
            if !finite_vec(g.origin)
                || !finite_vec(g.row_dir)
                || !finite_vec(g.col_dir)
                || !positive_finite(g.row_spacing)
                || !positive_finite(g.col_spacing)
                || (g.row_dir.length() - 1.0).abs() > 1e-4
                || (g.col_dir.length() - 1.0).abs() > 1e-4
                || g.row_dir.dot(g.col_dir).abs() > 1e-4
            {
                return Err(Error::wrap(
                    Kind::GeometryMismatch,
                    format!("frame {n} has no valid patient geometry"),
                ));
            }
            if g.row_dir.sub(first.row_dir).length() > 1e-4
                || g.col_dir.sub(first.col_dir).length() > 1e-4
                || (g.row_spacing - first.row_spacing).abs() > 1e-6
                || (g.col_spacing - first.col_spacing).abs() > 1e-6
            {
                return Err(Error::wrap(
                    Kind::UnsupportedDimensions,
                    format!("frame {n} differs in orientation or spacing"),
                ));
            }
            for source in &frame.sources {
                if trim_unicode_space(&source.sop_class_uid).is_empty()
                    || trim_unicode_space(&source.sop_instance_uid).is_empty()
                {
                    return Err(Error::wrap(
                        Kind::InvalidObject,
                        format!("frame {n} has a source without SOP Class or Instance UID"),
                    ));
                }
                if trim_unicode_space(&self.source_series_instance_uid).is_empty() {
                    return Err(Error::wrap(
                        Kind::InvalidObject,
                        "sources need the source Series Instance UID",
                    ));
                }
            }
        }
        Ok(())
    }
    fn derivation_code(&self) -> &Code {
        if !self.derivation_code.value.is_empty() {
            &self.derivation_code
        } else {
            &self.quantity
        }
    }
    fn mapping_item(&self) -> DataSet {
        let mut label = trim_unicode_space(&self.lut_label).as_bytes();
        if label.is_empty() {
            label = self.quantity.meaning.as_bytes();
        }
        if label.len() > 16 {
            label = &label[..16];
        }
        let label = String::from_utf8_lossy(label);
        let (mut minimum, mut maximum) = (f64::INFINITY, f64::NEG_INFINITY);
        for frame in &self.frames {
            for &v in &frame.values {
                let f = f64::from(v);
                if finite(f) {
                    minimum = fmath::min(minimum, f);
                    maximum = fmath::max(maximum, f);
                }
            }
        }
        if minimum > maximum {
            minimum = 0.0;
            maximum = 0.0;
        }
        dio::data_set(vec![
            dio::fd(TAG_DOUBLE_RWV_FIRST_MAPPED, &[minimum]),
            dio::fd(TAG_DOUBLE_RWV_LAST_MAPPED, &[maximum]),
            dio::str_elem(TAG_LUT_EXPLANATION, Vr::LO, &self.quantity.meaning),
            dio::sh(TAG_LUT_LABEL, &label),
            dio::seq(TAG_MEASUREMENT_UNITS_CODE_SEQ, vec![code_item(&self.units)]),
            dio::seq(
                TAG_QUANTITY_DEFINITION_SEQ,
                vec![dio::data_set(vec![
                    dio::cs(TAG_VALUE_TYPE, "CODE"),
                    dio::seq(
                        TAG_CONCEPT_NAME_CODE_SEQ,
                        vec![code_item(&Code::new("246205007", "SCT", "Quantity"))],
                    ),
                    dio::seq(TAG_CONCEPT_CODE_SEQ, vec![code_item(&self.quantity)]),
                ])],
            ),
            dio::fd(TAG_RWV_INTERCEPT, &[0.0]),
            dio::fd(TAG_RWV_SLOPE, &[1.0]),
        ])
    }
}
fn pixel_measures(doc: &Document) -> Vec<Element> {
    let first = doc.frames[0].geometry;
    let mut thickness = 1.0;
    let mut spacing = 0.0;
    if doc.frames.len() > 1 {
        let normal = first.row_dir.cross(first.col_dir).normalize();
        let last = doc.frames[doc.frames.len() - 1].geometry;
        spacing = last.origin.sub(first.origin).dot(normal).abs() / (doc.frames.len() - 1) as f64;
        if spacing > 0.0 {
            thickness = spacing;
        }
    }
    let mut elements = vec![
        dio::ds(TAG_PIXEL_SPACING, &[first.row_spacing, first.col_spacing]),
        dio::ds(TAG_SLICE_THICKNESS, &[thickness]),
    ];
    if spacing > 0.0 {
        elements.push(dio::ds(TAG_SPACING_BETWEEN_SLICES, &[spacing]));
    }
    elements
}
fn code_item(code: &Code) -> DataSet {
    dio::data_set(vec![
        dio::sh(TAG_CODE_VALUE, &code.value),
        dio::sh(TAG_CODING_SCHEME_DESIGNATOR, &code.scheme),
        dio::lo(TAG_CODE_MEANING, &code.meaning),
    ])
}
fn pointer_element(tag: Tag, value: Tag) -> Element {
    let mut raw = Vec::with_capacity(4);
    raw.extend_from_slice(&value.group.to_le_bytes());
    raw.extend_from_slice(&value.element.to_le_bytes());
    dio::raw(tag, Vr::AT, raw)
}
