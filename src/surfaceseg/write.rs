use super::read::tags::*;
use super::segment::{CodedEntry, ReferencedImage};
use super::{
    Algorithm, DEFAULT_CONTENT_LABEL, Document, Error, Kind, NO, SURFACE_SEGMENTATION_STORAGE,
    Segment, Surface, YES,
};
use crate::derivedio as dio;
use crate::mesh::Mesh;
use crate::object::element_setters::fl_element;
use crate::object::inherit::inherit_modules;
use crate::object::{DataSet, Element, File, Tag, Vr};
use std::collections::HashMap;
fn now_date_time() -> (String, String) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let now = crate::dcmtime::ZonedTime::from_fields(1970, 1, 1, 0, 0, seconds, 0, 0);
    let (year, month, day) = now.date();
    (
        format!("{year:04}{month:02}{day:02}"),
        format!("{:02}{:02}{:02}", now.hour(), now.minute(), now.second()),
    )
}
pub fn write(doc: Option<&Document>) -> Result<File, Error> {
    let Some(doc) = doc else {
        return Err(super::invalid("document is nil"));
    };
    let surfaces = doc.resolved()?;
    let identification = doc
        .identification
        .elements(DEFAULT_CONTENT_LABEL)
        .map_err(|e| Error::new(Kind::InvalidObject, e))?;
    let (mut content_date, mut content_time) = (doc.content_date.clone(), doc.content_time.clone());
    if content_date.is_empty() || content_time.is_empty() {
        let (date, time) = now_date_time();
        if content_date.is_empty() {
            content_date = date;
        }
        if content_time.is_empty() {
            content_time = time;
        }
    }
    let mut elements = vec![
        dio::ui(dio::TAG_SOP_CLASS_UID, SURFACE_SEGMENTATION_STORAGE),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &doc.sop_instance_uid),
        dio::cs(dio::TAG_MODALITY, "SEG"),
        dio::str_elem(dio::TAG_CONTENT_DATE, Vr::DA, &content_date),
        dio::str_elem(dio::TAG_CONTENT_TIME, Vr::TM, &content_time),
        dio::ui(dio::TAG_STUDY_INSTANCE_UID, &doc.study_instance_uid),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, &doc.series_instance_uid),
        dio::ui(dio::TAG_FRAME_OF_REFERENCE_UID, &doc.frame_of_reference_uid),
        segment_sequence(&doc.segments),
        dio::ul(NUMBER_OF_SURFACES, &[surfaces.len() as u32]),
        surface_sequence(&surfaces),
    ];
    elements.extend(identification);
    if !doc.referenced_images.is_empty() {
        elements.push(referenced_series_sequence(&doc.referenced_images));
    }
    let mut dataset = dio::object(elements);
    doc.common
        .apply(&mut dataset, inherit_modules::ALL)
        .map_err(|e| Error::other(e.to_string()))?;
    dio::file(
        SURFACE_SEGMENTATION_STORAGE,
        &doc.sop_instance_uid,
        Some(dataset),
    )
    .map_err(|e| Error::other(e.to_string()))
}
fn code_sequence(tag: Tag, code: &CodedEntry) -> Element {
    dio::seq(
        tag,
        vec![dio::data_set(vec![
            dio::sh(CODE_VALUE, &code.code_value),
            dio::sh(CODING_SCHEME_DESIGNATOR, &code.coding_scheme_designator),
            dio::lo(CODE_MEANING, &code.code_meaning),
        ])],
    )
}
fn algorithm_item(a: &Algorithm) -> DataSet {
    let mut elements = vec![
        code_sequence(ALGORITHM_FAMILY_CODE_SEQ, &a.family),
        dio::lo(ALGORITHM_NAME, &a.name),
        dio::lo(ALGORITHM_VERSION, &a.version),
    ];
    if a.name_code != CodedEntry::default() {
        elements.push(code_sequence(ALGORITHM_NAME_CODE_SEQ, &a.name_code));
    }
    if !a.parameters.is_empty() {
        elements.push(dio::str_elem(ALGORITHM_PARAMETERS, Vr::LT, &a.parameters));
    }
    dio::data_set(elements)
}
fn instance_reference(reference: &ReferencedImage) -> DataSet {
    let mut elements = vec![
        dio::ui(dio::TAG_REF_SOP_CLASS_UID, &reference.sop_class_uid),
        dio::ui(dio::TAG_REF_SOP_INSTANCE_UID, &reference.sop_instance_uid),
    ];
    if !reference.frames.is_empty() {
        elements.push(dio::is(dio::TAG_REF_FRAME_NUMBER, &reference.frames));
    }
    dio::data_set(elements)
}
fn segment_sequence(segments: &[Segment]) -> Element {
    let items = segments
        .iter()
        .map(|segment| {
            let s = &segment.segment;
            let mut elements = vec![
                dio::us(SEGMENT_NUMBER, &[s.number as u16]),
                dio::lo(SEGMENT_LABEL, &s.label),
                dio::cs(SEGMENT_ALGORITHM_TYPE, &s.algorithm_type),
                code_sequence(SEGMENTED_PROPERTY_CATEGORY, &s.segmented_property_category),
                code_sequence(SEGMENTED_PROPERTY_TYPE, &s.segmented_property_type),
                dio::ul(SURFACE_COUNT, &[segment.surfaces.len() as u32]),
            ];
            if !s.algorithm_name.is_empty() {
                elements.push(dio::lo(SEGMENT_ALGORITHM_NAME, &s.algorithm_name));
            }
            if s.recommended_display_cielab != [0; 3] {
                elements.push(dio::us(RECOMMENDED_CIELAB, &s.recommended_display_cielab));
            }
            let refs = segment
                .surfaces
                .iter()
                .map(|reference| {
                    dio::data_set(vec![
                        dio::ul(REFERENCED_SURFACE_NUMBER, &[reference.number as u32]),
                        dio::seq(
                            GENERATION_ALGORITHM_SEQ,
                            vec![algorithm_item(&reference.algorithm)],
                        ),
                        dio::seq(
                            SOURCE_INSTANCE_SEQUENCE,
                            reference
                                .source_instances
                                .iter()
                                .map(instance_reference)
                                .collect(),
                        ),
                    ])
                })
                .collect();
            elements.push(dio::seq(REFERENCED_SURFACE_SEQUENCE, refs));
            dio::data_set(elements)
        })
        .collect();
    dio::seq(SEGMENT_SEQUENCE, items)
}
fn referenced_series_sequence(refs: &[ReferencedImage]) -> Element {
    let mut order: Vec<&str> = Vec::new();
    let mut by_series: HashMap<&str, Vec<DataSet>> = HashMap::new();
    for reference in refs {
        let series = reference.series_instance_uid.as_str();
        if !by_series.contains_key(series) {
            order.push(series);
        }
        by_series
            .entry(series)
            .or_default()
            .push(instance_reference(reference));
    }
    let items = order
        .into_iter()
        .map(|series| {
            dio::data_set(vec![
                dio::ui(dio::TAG_SERIES_INSTANCE_UID, series),
                dio::seq(
                    REFERENCED_INSTANCE_SEQUENCE,
                    by_series.remove(series).unwrap_or_default(),
                ),
            ])
        })
        .collect();
    dio::seq(REFERENCED_SERIES_SEQUENCE, items)
}
fn surface_sequence(surfaces: &[Surface]) -> Element {
    let empty = Mesh::default();
    let items = surfaces
        .iter()
        .map(|s| {
            let m = s.mesh.as_ref().unwrap_or(&empty);
            let mut elements = vec![
                dio::ul(SURFACE_NUMBER, &[s.number as u32]),
                dio::us(RECOMMENDED_GRAYSCALE, &[s.recommended_display_grayscale]),
                dio::us(RECOMMENDED_CIELAB, &s.recommended_display_cielab),
                dio::fl(PRESENTATION_OPACITY, &[s.opacity]),
                dio::cs(PRESENTATION_TYPE, &s.presentation_type),
                dio::cs(FINITE_VOLUME, &s.finite_volume),
                dio::cs(MANIFOLD, &s.manifold),
                dio::seq(SURFACE_POINTS_SEQUENCE, vec![points_item(&m.vertices)]),
                normals_sequence(m, &s.vector_accuracy),
                primitive_sequence(s, m),
            ];
            if !s.comments.is_empty() {
                elements.push(dio::str_elem(SURFACE_COMMENTS, Vr::LT, &s.comments));
            }
            match &s.processing {
                Some(p) => {
                    elements.push(dio::cs(SURFACE_PROCESSING, YES));
                    elements.push(dio::fl(SURFACE_PROCESSING_RATIO, &[p.ratio]));
                    elements.push(dio::seq(
                        PROCESSING_ALGORITHM_SEQ,
                        vec![algorithm_item(&p.algorithm)],
                    ));
                    if !p.description.is_empty() {
                        elements.push(dio::lo(SURFACE_PROCESSING_DESCR, &p.description));
                    }
                }
                None => elements.push(dio::cs(SURFACE_PROCESSING, NO)),
            }
            dio::data_set(elements)
        })
        .collect();
    dio::seq(SURFACE_SEQUENCE, items)
}
fn min_nan(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}
fn max_nan(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}
fn float_bytes(values: &[[f32; 3]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect()
}
fn points_item(vertices: &[[f32; 3]]) -> DataSet {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for v in vertices {
        for k in 0..3 {
            lo[k] = min_nan(lo[k], v[k]);
            hi[k] = max_nan(hi[k], v[k]);
        }
    }
    let bounds: Vec<f64> = lo.iter().chain(&hi).map(|v| f64::from(*v)).collect();
    dio::data_set(vec![
        dio::ul(NUMBER_OF_SURFACE_POINTS, &[vertices.len() as u32]),
        dio::raw(POINT_COORDINATES_DATA, Vr::OF, float_bytes(vertices)),
        dio::fl(POINTS_BOUNDING_BOX, &bounds),
    ])
}
fn normals_sequence(m: &Mesh, accuracy: &[f32]) -> Element {
    let normals = m.normals.as_deref().unwrap_or_default();
    if normals.is_empty() {
        return dio::seq(SURFACE_NORMALS_SEQUENCE, Vec::new());
    }
    let mut elements = vec![
        dio::ul(NUMBER_OF_VECTORS, &[normals.len() as u32]),
        dio::us(VECTOR_DIMENSIONALITY, &[3]),
        dio::raw(VECTOR_COORDINATE_DATA, Vr::OF, float_bytes(normals)),
    ];
    if !accuracy.is_empty() {
        elements.push(fl_element(VECTOR_ACCURACY, false, accuracy));
    }
    dio::seq(SURFACE_NORMALS_SEQUENCE, vec![dio::data_set(elements)])
}
fn primitive_sequence(s: &Surface, m: &Mesh) -> Element {
    let indices = |long: Tag, retired: Tag, v: &[u32]| -> Element {
        if s.retired_index_lists {
            let data = v
                .iter()
                .flat_map(|value| (value.wrapping_add(1) as u16).to_le_bytes())
                .collect();
            return dio::raw(retired, Vr::OW, data);
        }
        let data = v
            .iter()
            .flat_map(|value| value.wrapping_add(1).to_le_bytes())
            .collect();
        dio::raw(long, Vr::OL, data)
    };
    let edges: Vec<u32> = s.primitives.edges.iter().flatten().copied().collect();
    let triangles: Vec<u32> = m.triangles.iter().flatten().copied().collect();
    let lines = s
        .primitives
        .lines
        .iter()
        .map(|line| {
            dio::data_set(vec![indices(
                LONG_PRIMITIVE_POINT_INDEX,
                PRIMITIVE_POINT_INDEX_LIST,
                line,
            )])
        })
        .collect();
    dio::seq(
        MESH_PRIMITIVES_SEQUENCE,
        vec![dio::data_set(vec![
            indices(
                LONG_VERTEX_POINT_INDEX,
                VERTEX_POINT_INDEX_LIST,
                &s.primitives.vertices,
            ),
            indices(LONG_EDGE_POINT_INDEX, EDGE_POINT_INDEX_LIST, &edges),
            indices(
                LONG_TRIANGLE_POINT_INDEX,
                TRIANGLE_POINT_INDEX_LIST,
                &triangles,
            ),
            dio::seq(TRIANGLE_STRIP_SEQUENCE, Vec::new()),
            dio::seq(TRIANGLE_FAN_SEQUENCE, Vec::new()),
            dio::seq(LINE_SEQUENCE, lines),
            dio::seq(FACET_SEQUENCE, Vec::new()),
        ])],
    )
}
