use super::affine::{SPATIAL_REGISTRATION_STORAGE, TRANSFORM_RIGID, validate_matrix};
use super::read::{
    TAG_DEFORMABLE_REGISTRATION_GRID_SEQUENCE, TAG_DEFORMABLE_REGISTRATION_SEQUENCE,
    TAG_FRAME_TRANSFORMATION_MATRIX, TAG_FRAME_TRANSFORMATION_MATRIX_TYPE, TAG_GRID_DIMENSIONS,
    TAG_GRID_RESOLUTION, TAG_IMAGE_ORIENTATION_PATIENT, TAG_IMAGE_POSITION_PATIENT,
    TAG_MATRIX_REGISTRATION_SEQUENCE, TAG_MATRIX_SEQUENCE, TAG_POST_DEFORMATION_MATRIX_SEQUENCE,
    TAG_PRE_DEFORMATION_MATRIX_SEQUENCE, TAG_REFERENCED_IMAGE_SEQUENCE,
    TAG_REFERENCED_SOP_INSTANCE_UID, TAG_REGISTRATION_SEQUENCE, TAG_SOURCE_FRAME_OF_REFERENCE_UID,
    TAG_VECTOR_GRID_DATA,
};
use super::{
    DEFORMABLE_SPATIAL_REGISTRATION_STORAGE, Error, Grid, IDENTITY, Kind, Matrix,
    checked_product64, valid_basis,
};
use crate::dcmtime::ZonedTime;
use crate::derivation::DerivedCommon;
use crate::derivedio as dio;
use crate::dicomjson::trim_unicode_space;
use crate::object::inherit::{inherit_modules, new_uid};
use crate::object::is_valid_uid;
use crate::object::{DataSet, Element, File, Object as DataObject, Tag, Vr};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const TAG_REFERENCED_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
const TAG_REFERENCED_SERIES_SEQUENCE: Tag = Tag::new(0x0008, 0x1115);
const TAG_REFERENCED_INSTANCE_SEQUENCE: Tag = Tag::new(0x0008, 0x114A);
const TAG_CONTENT_CREATORS_NAME: Tag = Tag::new(0x0070, 0x0084);
const TAG_REGISTRATION_TYPE_CODE_SEQUENCE: Tag = Tag::new(0x0070, 0x030D);
const TAG_FRAME_TRANSFORMATION_COMMENT: Tag = Tag::new(0x3006, 0x00C8);
const TAG_STUDIES_CONTAINING_OTHER_REF_SEQ: Tag = Tag::new(0x0008, 0x1200);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageReference {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Debug, Default)]
pub struct MatrixRegistration {
    pub source_frame_of_reference_uid: String,
    pub matrix: Matrix,
    pub comment: String,
    pub referenced_images: Vec<ImageReference>,
}
#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    pub common: DerivedCommon,
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: i64,
    pub series_description: String,
    pub instance_number: i64,
    pub content_label: String,
    pub content_description: String,
    pub content_creator_name: String,
    pub registered_frame_of_reference_uid: String,
    pub registrations: Vec<MatrixRegistration>,
    pub content_time: Option<ZonedTime>,
}
fn write_err(detail: impl AsRef<str>) -> Error {
    Error::new(Kind::Write, detail)
}
fn valid_uid(uid: &str) -> bool {
    is_valid_uid(uid.as_bytes())
}
fn uid_or_new(value: &str) -> Result<String, Error> {
    let value = trim_unicode_space(value);
    if value.is_empty() {
        return Ok(new_uid());
    }
    if !valid_uid(value) {
        return Err(write_err(format!("UID {}", quote(value))));
    }
    Ok(value.to_string())
}
fn content_label(label: &str) -> Result<String, Error> {
    let mut label = trim_unicode_space(label);
    if label.is_empty() {
        label = "REGISTRATION";
    }
    if label.len() > 16
        || label
            .chars()
            .any(|r| !(r.is_ascii_uppercase() || r.is_ascii_digit() || r == ' ' || r == '_'))
    {
        return Err(write_err(format!(
            "Content Label {} is not a CS value",
            quote(label)
        )));
    }
    Ok(label.to_string())
}
fn system_now() -> ZonedTime {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    ZonedTime::from_fields(1970, 1, 1, 0, 0, now, 0, 0)
}
struct Header<'a> {
    common: &'a DerivedCommon,
    sop_class_uid: &'static str,
    sop_instance_uid: &'a str,
    series_instance_uid: &'a str,
    series_number: i64,
    series_description: &'a str,
    instance_number: i64,
    content_description: &'a str,
    content_creator_name: &'a str,
    registered: &'a str,
    content_time: Option<ZonedTime>,
    sequence: Tag,
}
fn build_file(
    h: Header<'_>,
    label: String,
    items: Vec<DataSet>,
    references: &[&[ImageReference]],
) -> Result<File, Error> {
    let sop_instance_uid = uid_or_new(h.sop_instance_uid)?;
    let series_instance_uid = uid_or_new(h.series_instance_uid)?;
    let now = h.content_time.unwrap_or_else(system_now);
    let (year, month, day) = now.date();
    let series_number = if h.series_number > 0 {
        dio::is(dio::TAG_SERIES_NUMBER, &[h.series_number])
    } else {
        dio::is(dio::TAG_SERIES_NUMBER, &[])
    };
    let mut dataset = dio::object(vec![
        dio::cs(TAG_SPECIFIC_CHARACTER_SET, "ISO_IR 192"),
        dio::ui(dio::TAG_SOP_CLASS_UID, h.sop_class_uid),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &sop_instance_uid),
        dio::cs(dio::TAG_MODALITY, "REG"),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, &series_instance_uid),
        series_number,
        dio::is(dio::TAG_INSTANCE_NUMBER, &[h.instance_number]),
        dio::ui(dio::TAG_FRAME_OF_REFERENCE_UID, h.registered),
        dio::str_elem(
            dio::TAG_CONTENT_DATE,
            Vr::DA,
            &format!("{year:04}{month:02}{day:02}"),
        ),
        dio::str_elem(
            dio::TAG_CONTENT_TIME,
            Vr::TM,
            &format!("{:02}{:02}{:02}", now.hour(), now.minute(), now.second()),
        ),
        dio::cs(dio::TAG_CONTENT_LABEL, &label),
        dio::lo(dio::TAG_CONTENT_DESCRIPTION, h.content_description),
        dio::str_elem(TAG_CONTENT_CREATORS_NAME, Vr::PN, h.content_creator_name),
        dio::seq(h.sequence, items),
    ]);
    let description = trim_unicode_space(h.series_description);
    if !description.is_empty() {
        dataset.put(dio::lo(dio::TAG_SERIES_DESCR, description));
    }
    h.common
        .apply(&mut dataset, inherit_modules::ALL)
        .map_err(|e| Error::plain(e.to_string()))?;
    put_common_instance_references(&mut dataset, references);
    dio::file(h.sop_class_uid, &sop_instance_uid, Some(dataset))
        .map_err(|e| Error::plain(e.to_string()))
}
fn registered_frame(explicit: &str, common: &DerivedCommon) -> Result<String, Error> {
    let mut registered = trim_unicode_space(explicit).to_string();
    if registered.is_empty()
        && let Some(source) = &common.source
    {
        registered = dio::clean_uid(source, dio::TAG_FRAME_OF_REFERENCE_UID);
    }
    if !valid_uid(&registered) {
        return Err(write_err(format!(
            "registered Frame of Reference UID {}",
            quote(&registered)
        )));
    }
    Ok(registered)
}
fn complete_reference(reference: &ImageReference) -> bool {
    valid_uid(&reference.sop_class_uid)
        && valid_uid(&reference.sop_instance_uid)
        && valid_uid(&reference.series_instance_uid)
}
pub fn write(opts: &WriteOptions) -> Result<File, Error> {
    if opts.registrations.is_empty() {
        return Err(write_err("no registration"));
    }
    let registered = registered_frame(&opts.registered_frame_of_reference_uid, &opts.common)?;
    for (i, r) in opts.registrations.iter().enumerate() {
        let n = i + 1;
        if !valid_uid(&r.source_frame_of_reference_uid) {
            return Err(write_err(format!(
                "registration {n}: source Frame of Reference UID {}",
                quote(&r.source_frame_of_reference_uid)
            )));
        }
        if r.source_frame_of_reference_uid == registered {
            return Err(write_err(format!(
                "registration {n} maps the registered Frame of Reference onto itself"
            )));
        }
        validate_matrix(&r.matrix.values, &r.matrix.r#type)
            .map_err(|e| e.wrap(Kind::Write, format!("registration {n}")))?;
        if !r.referenced_images.iter().all(complete_reference) {
            return Err(write_err(format!(
                "registration {n}: incomplete image reference"
            )));
        }
    }
    let label = content_label(&opts.content_label)?;
    let mut items = vec![registration_item(
        &registered,
        &Matrix {
            r#type: TRANSFORM_RIGID.to_string(),
            values: IDENTITY,
        },
        "Registered Frame of Reference",
        &[],
    )];
    let mut references = Vec::new();
    for r in &opts.registrations {
        items.push(registration_item(
            &r.source_frame_of_reference_uid,
            &r.matrix,
            &r.comment,
            &r.referenced_images,
        ));
        references.push(r.referenced_images.as_slice());
    }
    build_file(
        Header {
            common: &opts.common,
            sop_class_uid: SPATIAL_REGISTRATION_STORAGE,
            sop_instance_uid: &opts.sop_instance_uid,
            series_instance_uid: &opts.series_instance_uid,
            series_number: opts.series_number,
            series_description: &opts.series_description,
            instance_number: if opts.instance_number == 0 {
                1
            } else {
                opts.instance_number
            },
            content_description: &opts.content_description,
            content_creator_name: &opts.content_creator_name,
            registered: &registered,
            content_time: opts.content_time,
            sequence: TAG_REGISTRATION_SEQUENCE,
        },
        label,
        items,
        &references,
    )
}
fn matrix_item(matrix: &Matrix) -> DataSet {
    dio::data_set(vec![
        dio::ds(TAG_FRAME_TRANSFORMATION_MATRIX, &matrix.values.0),
        dio::cs(TAG_FRAME_TRANSFORMATION_MATRIX_TYPE, &matrix.r#type),
    ])
}
fn image_items(refs: &[ImageReference]) -> Vec<DataSet> {
    refs.iter()
        .map(|reference| {
            dio::data_set(vec![
                dio::ui(TAG_REFERENCED_SOP_CLASS_UID, &reference.sop_class_uid),
                dio::ui(TAG_REFERENCED_SOP_INSTANCE_UID, &reference.sop_instance_uid),
            ])
        })
        .collect()
}
fn registration_item(
    frame: &str,
    matrix: &Matrix,
    comment: &str,
    refs: &[ImageReference],
) -> DataSet {
    let mut registration = vec![
        dio::seq(TAG_REGISTRATION_TYPE_CODE_SEQUENCE, Vec::new()),
        dio::seq(TAG_MATRIX_SEQUENCE, vec![matrix_item(matrix)]),
    ];
    let comment = trim_unicode_space(comment);
    if !comment.is_empty() {
        registration.push(dio::lo(TAG_FRAME_TRANSFORMATION_COMMENT, comment));
    }
    let mut elements = vec![
        dio::ui(dio::TAG_FRAME_OF_REFERENCE_UID, frame),
        dio::seq(
            TAG_MATRIX_REGISTRATION_SEQUENCE,
            vec![dio::data_set(registration)],
        ),
    ];
    if !refs.is_empty() {
        elements.push(dio::seq(TAG_REFERENCED_IMAGE_SEQUENCE, image_items(refs)));
    }
    dio::data_set(elements)
}
fn put_common_instance_references(dataset: &mut DataObject, registrations: &[&[ImageReference]]) {
    let study = dio::clean_uid(dataset, dio::TAG_STUDY_INSTANCE_UID);
    let mut order: Vec<(String, String)> = Vec::new();
    let mut instances: HashMap<(String, String), Vec<DataSet>> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for refs in registrations {
        for reference in refs.iter() {
            if !seen.insert(reference.sop_instance_uid.as_str()) {
                continue;
            }
            let study_uid = if reference.study_instance_uid.is_empty() {
                study.clone()
            } else {
                reference.study_instance_uid.clone()
            };
            let key = (study_uid, reference.series_instance_uid.clone());
            if !instances.contains_key(&key) {
                order.push(key.clone());
            }
            instances.entry(key).or_default().push(dio::data_set(vec![
                dio::ui(TAG_REFERENCED_SOP_CLASS_UID, &reference.sop_class_uid),
                dio::ui(TAG_REFERENCED_SOP_INSTANCE_UID, &reference.sop_instance_uid),
            ]));
        }
    }
    let mut local = Vec::new();
    let mut other: HashMap<String, Vec<DataSet>> = HashMap::new();
    let mut other_order: Vec<String> = Vec::new();
    for key in order {
        let item = dio::data_set(vec![
            dio::ui(dio::TAG_SERIES_INSTANCE_UID, &key.1),
            dio::seq(
                TAG_REFERENCED_INSTANCE_SEQUENCE,
                instances.remove(&key).unwrap_or_default(),
            ),
        ]);
        if key.0 == study {
            local.push(item);
            continue;
        }
        if !other.contains_key(&key.0) {
            other_order.push(key.0.clone());
        }
        other.entry(key.0).or_default().push(item);
    }
    if !local.is_empty() {
        dataset.put(dio::seq(TAG_REFERENCED_SERIES_SEQUENCE, local));
    }
    if !other_order.is_empty() {
        let studies = other_order
            .into_iter()
            .map(|uid| {
                let series = other.remove(&uid).unwrap_or_default();
                dio::data_set(vec![
                    dio::ui(dio::TAG_STUDY_INSTANCE_UID, &uid),
                    dio::seq(TAG_REFERENCED_SERIES_SEQUENCE, series),
                ])
            })
            .collect();
        dataset.put(dio::seq(TAG_STUDIES_CONTAINING_OTHER_REF_SEQ, studies));
    }
}
#[derive(Clone, Debug, Default)]
pub struct DeformableRegistration {
    pub source_frame_of_reference_uid: String,
    pub pre: Matrix,
    pub post: Matrix,
    pub grid: Option<Grid>,
    pub comment: String,
    pub referenced_images: Vec<ImageReference>,
}
#[derive(Clone, Debug, Default)]
pub struct DeformableWriteOptions {
    pub common: DerivedCommon,
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: i64,
    pub series_description: String,
    pub instance_number: i64,
    pub content_label: String,
    pub content_description: String,
    pub content_creator_name: String,
    pub registered_frame_of_reference_uid: String,
    pub registrations: Vec<DeformableRegistration>,
    pub content_time: Option<ZonedTime>,
}
pub fn write_deformable(opts: &DeformableWriteOptions) -> Result<File, Error> {
    if opts.registrations.is_empty() {
        return Err(write_err("no registration"));
    }
    let registered = registered_frame(&opts.registered_frame_of_reference_uid, &opts.common)?;
    let mut items = Vec::new();
    let mut references = Vec::new();
    for (i, r) in opts.registrations.iter().enumerate() {
        let n = i + 1;
        if !valid_uid(&r.source_frame_of_reference_uid)
            || r.source_frame_of_reference_uid == registered
        {
            return Err(write_err(format!(
                "deformable registration {n}: source Frame of Reference UID {}",
                quote(&r.source_frame_of_reference_uid)
            )));
        }
        if r.grid.is_none() && r.pre.r#type.is_empty() && r.post.r#type.is_empty() {
            return Err(write_err(format!(
                "deformable registration {n} has neither a grid nor a matrix"
            )));
        }
        let item = deformable_item(r)
            .map_err(|e| e.wrap(Kind::Write, format!("deformable registration {n}")))?;
        items.push(item);
        references.push(r.referenced_images.as_slice());
    }
    let label = content_label(&opts.content_label)?;
    build_file(
        Header {
            common: &opts.common,
            sop_class_uid: DEFORMABLE_SPATIAL_REGISTRATION_STORAGE,
            sop_instance_uid: &opts.sop_instance_uid,
            series_instance_uid: &opts.series_instance_uid,
            series_number: opts.series_number,
            series_description: &opts.series_description,
            instance_number: opts.instance_number.max(1),
            content_description: &opts.content_description,
            content_creator_name: &opts.content_creator_name,
            registered: &registered,
            content_time: opts.content_time,
            sequence: TAG_DEFORMABLE_REGISTRATION_SEQUENCE,
        },
        label,
        items,
        &references,
    )
}
fn deformable_item(r: &DeformableRegistration) -> Result<DataSet, Error> {
    let mut elements: Vec<Element> = vec![
        dio::ui(
            TAG_SOURCE_FRAME_OF_REFERENCE_UID,
            &r.source_frame_of_reference_uid,
        ),
        dio::seq(TAG_REGISTRATION_TYPE_CODE_SEQUENCE, Vec::new()),
    ];
    for (tag, matrix) in [
        (TAG_PRE_DEFORMATION_MATRIX_SEQUENCE, &r.pre),
        (TAG_POST_DEFORMATION_MATRIX_SEQUENCE, &r.post),
    ] {
        if matrix.r#type.is_empty() {
            continue;
        }
        validate_matrix(&matrix.values, &matrix.r#type)?;
        elements.push(dio::seq(tag, vec![matrix_item(matrix)]));
    }
    if let Some(grid) = &r.grid {
        elements.push(dio::seq(
            TAG_DEFORMABLE_REGISTRATION_GRID_SEQUENCE,
            vec![grid_item(grid)?],
        ));
    }
    let comment = trim_unicode_space(&r.comment);
    if !comment.is_empty() {
        elements.push(dio::lo(TAG_FRAME_TRANSFORMATION_COMMENT, comment));
    }
    if !r.referenced_images.is_empty() {
        if !r.referenced_images.iter().all(complete_reference) {
            return Err(Error::plain("incomplete image reference"));
        }
        elements.push(dio::seq(
            TAG_REFERENCED_IMAGE_SEQUENCE,
            image_items(&r.referenced_images),
        ));
    }
    Ok(dio::data_set(elements))
}
fn grid_item(g: &Grid) -> Result<DataSet, Error> {
    let count = checked_product64(&g.dimensions.map(u64::from));
    if count != Some(g.vectors.len() as u64)
        || !valid_basis(g.row_direction, g.column_direction, g.normal_direction)
        || g.row_direction
            .cross(g.column_direction)
            .dot(g.normal_direction)
            < 0.0
    {
        return Err(Error::plain("invalid deformation grid"));
    }
    let mut data = Vec::with_capacity(12 * g.vectors.len());
    for v in g.vectors.iter() {
        for component in v {
            data.extend_from_slice(&component.to_bits().to_le_bytes());
        }
    }
    Ok(dio::data_set(vec![
        dio::ds(
            TAG_IMAGE_POSITION_PATIENT,
            &[g.origin.x, g.origin.y, g.origin.z],
        ),
        dio::ds(
            TAG_IMAGE_ORIENTATION_PATIENT,
            &[
                g.row_direction.x,
                g.row_direction.y,
                g.row_direction.z,
                g.column_direction.x,
                g.column_direction.y,
                g.column_direction.z,
            ],
        ),
        dio::ul(TAG_GRID_DIMENSIONS, &g.dimensions),
        dio::fd(
            TAG_GRID_RESOLUTION,
            &[g.spacing.x, g.spacing.y, g.spacing.z],
        ),
        dio::raw(TAG_VECTOR_GRID_DATA, Vr::OF, data),
    ]))
}
