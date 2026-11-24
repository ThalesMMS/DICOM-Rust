use super::segment::{self as seg, CodedEntry, ReferencedImage};
use super::{
    Algorithm, Document, Edge, Error, Kind, MAX_READ_GEOMETRY_BYTES, MAX_READ_INDICES,
    MAX_READ_POINTS, MAX_READ_PRIMITIVES, MAX_READ_TRIANGLES, Primitives, Processing,
    SURFACE_SEGMENTATION_STORAGE, Segment, Surface, SurfaceReference, YES, invalid, limit,
    validate_vector_accuracy,
};
use crate::derivedio as dio;
use crate::mesh::Mesh;
use crate::object::{Object, Tag, Value, Vr};
use std::collections::HashSet;
use tags::*;
pub(crate) mod tags {
    use crate::object::Tag;
    pub(crate) const REFERENCED_SERIES_SEQUENCE: Tag = Tag::new(0x0008, 0x1115);
    pub const REFERENCED_INSTANCE_SEQUENCE: Tag = Tag::new(0x0008, 0x114A);
    pub const CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
    pub const CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
    pub const CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
    pub(crate) const SEGMENT_SEQUENCE: Tag = Tag::new(0x0062, 0x0002);
    pub const SEGMENTED_PROPERTY_CATEGORY: Tag = Tag::new(0x0062, 0x0003);
    pub const SEGMENT_NUMBER: Tag = Tag::new(0x0062, 0x0004);
    pub const SEGMENT_LABEL: Tag = Tag::new(0x0062, 0x0005);
    pub const SEGMENT_ALGORITHM_TYPE: Tag = Tag::new(0x0062, 0x0008);
    pub const SEGMENT_ALGORITHM_NAME: Tag = Tag::new(0x0062, 0x0009);
    pub const RECOMMENDED_GRAYSCALE: Tag = Tag::new(0x0062, 0x000C);
    pub const RECOMMENDED_CIELAB: Tag = Tag::new(0x0062, 0x000D);
    pub const SEGMENTED_PROPERTY_TYPE: Tag = Tag::new(0x0062, 0x000F);
    pub const NUMBER_OF_SURFACES: Tag = Tag::new(0x0066, 0x0001);
    pub(crate) const SURFACE_SEQUENCE: Tag = Tag::new(0x0066, 0x0002);
    pub const SURFACE_NUMBER: Tag = Tag::new(0x0066, 0x0003);
    pub const SURFACE_COMMENTS: Tag = Tag::new(0x0066, 0x0004);
    pub const SURFACE_PROCESSING: Tag = Tag::new(0x0066, 0x0009);
    pub const SURFACE_PROCESSING_RATIO: Tag = Tag::new(0x0066, 0x000A);
    pub const SURFACE_PROCESSING_DESCR: Tag = Tag::new(0x0066, 0x000B);
    pub const PRESENTATION_OPACITY: Tag = Tag::new(0x0066, 0x000C);
    pub const PRESENTATION_TYPE: Tag = Tag::new(0x0066, 0x000D);
    pub const FINITE_VOLUME: Tag = Tag::new(0x0066, 0x000E);
    pub const MANIFOLD: Tag = Tag::new(0x0066, 0x0010);
    pub const SURFACE_POINTS_SEQUENCE: Tag = Tag::new(0x0066, 0x0011);
    pub const SURFACE_NORMALS_SEQUENCE: Tag = Tag::new(0x0066, 0x0012);
    pub const MESH_PRIMITIVES_SEQUENCE: Tag = Tag::new(0x0066, 0x0013);
    pub const NUMBER_OF_SURFACE_POINTS: Tag = Tag::new(0x0066, 0x0015);
    pub const POINT_COORDINATES_DATA: Tag = Tag::new(0x0066, 0x0016);
    pub const POINTS_BOUNDING_BOX: Tag = Tag::new(0x0066, 0x001A);
    pub const NUMBER_OF_VECTORS: Tag = Tag::new(0x0066, 0x001E);
    pub const VECTOR_DIMENSIONALITY: Tag = Tag::new(0x0066, 0x001F);
    pub const VECTOR_ACCURACY: Tag = Tag::new(0x0066, 0x0020);
    pub const VECTOR_COORDINATE_DATA: Tag = Tag::new(0x0066, 0x0021);
    pub const TRIANGLE_POINT_INDEX_LIST: Tag = Tag::new(0x0066, 0x0023);
    pub const EDGE_POINT_INDEX_LIST: Tag = Tag::new(0x0066, 0x0024);
    pub const VERTEX_POINT_INDEX_LIST: Tag = Tag::new(0x0066, 0x0025);
    pub const TRIANGLE_STRIP_SEQUENCE: Tag = Tag::new(0x0066, 0x0026);
    pub const TRIANGLE_FAN_SEQUENCE: Tag = Tag::new(0x0066, 0x0027);
    pub const LINE_SEQUENCE: Tag = Tag::new(0x0066, 0x0028);
    pub const PRIMITIVE_POINT_INDEX_LIST: Tag = Tag::new(0x0066, 0x0029);
    pub const SURFACE_COUNT: Tag = Tag::new(0x0066, 0x002A);
    pub const REFERENCED_SURFACE_SEQUENCE: Tag = Tag::new(0x0066, 0x002B);
    pub const REFERENCED_SURFACE_NUMBER: Tag = Tag::new(0x0066, 0x002C);
    pub const GENERATION_ALGORITHM_SEQ: Tag = Tag::new(0x0066, 0x002D);
    pub const SOURCE_INSTANCE_SEQUENCE: Tag = Tag::new(0x0066, 0x002E);
    pub const ALGORITHM_FAMILY_CODE_SEQ: Tag = Tag::new(0x0066, 0x002F);
    pub const ALGORITHM_NAME_CODE_SEQ: Tag = Tag::new(0x0066, 0x0030);
    pub const ALGORITHM_VERSION: Tag = Tag::new(0x0066, 0x0031);
    pub const ALGORITHM_PARAMETERS: Tag = Tag::new(0x0066, 0x0032);
    pub const FACET_SEQUENCE: Tag = Tag::new(0x0066, 0x0034);
    pub const PROCESSING_ALGORITHM_SEQ: Tag = Tag::new(0x0066, 0x0035);
    pub const ALGORITHM_NAME: Tag = Tag::new(0x0066, 0x0036);
    pub const LONG_PRIMITIVE_POINT_INDEX: Tag = Tag::new(0x0066, 0x0040);
    pub const LONG_TRIANGLE_POINT_INDEX: Tag = Tag::new(0x0066, 0x0041);
    pub const LONG_EDGE_POINT_INDEX: Tag = Tag::new(0x0066, 0x0042);
    pub const LONG_VERTEX_POINT_INDEX: Tag = Tag::new(0x0066, 0x0043);
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadLimits {
    pub max_points: i64,
    pub max_triangles: i64,
    pub max_primitives: i64,
    pub max_indices: i64,
    pub max_bytes: i64,
}
impl ReadLimits {
    fn defaults(mut self) -> Result<ReadLimits, Error> {
        for (field, default) in [
            (&mut self.max_points, MAX_READ_POINTS),
            (&mut self.max_triangles, MAX_READ_TRIANGLES),
            (&mut self.max_primitives, MAX_READ_PRIMITIVES),
            (&mut self.max_indices, MAX_READ_INDICES),
            (&mut self.max_bytes, MAX_READ_GEOMETRY_BYTES),
        ] {
            if *field < 0 {
                return Err(invalid("negative geometry resource limit"));
            }
            if *field == 0 || *field > default {
                *field = default;
            }
        }
        Ok(self)
    }
}
fn geometry_value_bytes(item: &Object, tag: Tag) -> Result<i64, Error> {
    let Some(e) = item.get(tag) else {
        return Ok(0);
    };
    match &e.value {
        Value::Nil => Err(invalid(format!("{tag} has no value"))),
        Value::Raw(raw) => Ok(raw.len() as i64),
        Value::Strings(values) => {
            let mut total: usize = values.iter().map(String::len).sum();
            total += values.len().saturating_sub(1);
            Ok((total + total % 2) as i64)
        }
        _ => Err(invalid(format!("{tag} has no bounded inline value"))),
    }
}
fn select_index_tag(item: &Object, long: Tag, retired: Tag) -> Result<Tag, Error> {
    let long_bytes = geometry_value_bytes(item, long)?;
    let retired_bytes = geometry_value_bytes(item, retired)?;
    if long_bytes != 0 && retired_bytes != 0 {
        return Err(invalid(format!(
            "both {long} and {retired} contain point indices"
        )));
    }
    if retired_bytes != 0 || !item.has(long) {
        return Ok(retired);
    }
    Ok(long)
}
fn vector_accuracy_bytes(item: &Object) -> Result<i64, Error> {
    let Some(element) = item.get(VECTOR_ACCURACY) else {
        return Ok(0);
    };
    if element.vr() != Vr::FL {
        return Err(invalid("Vector Accuracy requires FL"));
    }
    let bytes =
        geometry_value_bytes(item, VECTOR_ACCURACY).map_err(|e| e.prefixed("Vector Accuracy"))?;
    if !matches!(bytes, 0 | 4 | 12) {
        return Err(invalid(format!(
            "Vector Accuracy has {bytes} bytes; expected one shared value or three XYZ values"
        )));
    }
    Ok(bytes)
}
#[derive(Default)]
struct Totals {
    points: i64,
    triangles: i64,
    primitives: i64,
    indices: i64,
    bytes: i64,
}
impl Totals {
    fn check(&self, l: &ReadLimits) -> Result<(), Error> {
        if self.points > l.max_points
            || self.triangles > l.max_triangles
            || self.primitives > l.max_primitives
            || self.indices > l.max_indices
            || self.bytes > l.max_bytes
        {
            return Err(limit(format!(
                "geometry points={} triangles={} primitives={} indices={} bytes={}",
                self.points, self.triangles, self.primitives, self.indices, self.bytes
            )));
        }
        Ok(())
    }
    fn index_count(
        &mut self,
        l: &ReadLimits,
        item: &Object,
        long: Tag,
        retired: Tag,
    ) -> Result<i64, Error> {
        let tag = select_index_tag(item, long, retired)?;
        let width = if tag == retired { 2 } else { 4 };
        let n = geometry_value_bytes(item, tag)?;
        if n % width != 0 {
            return Err(invalid(format!("{tag} has incomplete indices")));
        }
        self.indices += n / width;
        self.bytes += n + 4 * (n / width);
        self.check(l)?;
        Ok(n / width)
    }
}
fn preflight_geometry(obj: &Object, limits: ReadLimits) -> Result<(), Error> {
    let l = limits.defaults()?;
    let mut t = Totals::default();
    for surface in &dio::sequence(obj, SURFACE_SEQUENCE) {
        for item in &dio::sequence(surface, SURFACE_POINTS_SEQUENCE) {
            let n = geometry_value_bytes(item, POINT_COORDINATES_DATA)?;
            let declared = match item.get_int(NUMBER_OF_SURFACE_POINTS) {
                Ok(v) if v >= 0 => v,
                _ => return Err(invalid("invalid Number of Surface Points")),
            };
            if declared > l.max_points - t.points {
                return Err(limit(format!("more than {} points", l.max_points)));
            }
            t.points += declared;
            t.bytes += n + 12 * declared;
            t.check(&l)?;
        }
        if let Some(element) = surface.get(SURFACE_NORMALS_SEQUENCE) {
            match &element.value {
                Value::Sequence(items) if element.vr() == Vr::SQ && items.len() <= 1 => {}
                _ => {
                    return Err(invalid(
                        "Surface Points Normals Sequence must be SQ with zero or one item",
                    ));
                }
            }
        }
        for item in &dio::sequence(surface, SURFACE_NORMALS_SEQUENCE) {
            let n = geometry_value_bytes(item, VECTOR_COORDINATE_DATA)?;
            let accuracy_bytes = vector_accuracy_bytes(item)?;
            t.bytes += 2 * (n + accuracy_bytes);
            t.check(&l)?;
        }
        for item in &dio::sequence(surface, MESH_PRIMITIVES_SEQUENCE) {
            let v = t.index_count(&l, item, LONG_VERTEX_POINT_INDEX, VERTEX_POINT_INDEX_LIST)?;
            let e = t.index_count(&l, item, LONG_EDGE_POINT_INDEX, EDGE_POINT_INDEX_LIST)?;
            let tri = t.index_count(
                &l,
                item,
                LONG_TRIANGLE_POINT_INDEX,
                TRIANGLE_POINT_INDEX_LIST,
            )?;
            t.primitives += v + e / 2 + tri / 3;
            t.triangles += tri / 3;
            t.bytes += 4 * e + 4 * tri;
            t.check(&l)?;
            for tag in [
                LINE_SEQUENCE,
                TRIANGLE_STRIP_SEQUENCE,
                TRIANGLE_FAN_SEQUENCE,
                FACET_SEQUENCE,
            ] {
                for primitive in &dio::sequence(item, tag) {
                    let n = t.index_count(
                        &l,
                        primitive,
                        LONG_PRIMITIVE_POINT_INDEX,
                        PRIMITIVE_POINT_INDEX_LIST,
                    )?;
                    if tag == LINE_SEQUENCE {
                        t.primitives += 1;
                        t.bytes += 24;
                    } else {
                        let extra = (n - 2).max(0);
                        t.triangles += extra;
                        t.primitives += extra;
                        t.bytes += 12 * extra;
                    }
                    t.check(&l)?;
                }
            }
        }
    }
    Ok(())
}
pub fn read(obj: Option<&Object>) -> Result<Document, Error> {
    read_with_limits(obj, ReadLimits::default())
}
pub fn read_with_limits(obj: Option<&Object>, limits: ReadLimits) -> Result<Document, Error> {
    let Some(obj) = obj else {
        return Err(invalid("data set is nil"));
    };
    let sop_class = dio::clean_uid(obj, dio::TAG_SOP_CLASS_UID);
    if sop_class != SURFACE_SEGMENTATION_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, sop_class));
    }
    preflight_geometry(obj, limits)?;
    let mut doc = Document {
        sop_instance_uid: dio::clean_uid(obj, dio::TAG_SOP_INSTANCE_UID),
        study_instance_uid: dio::clean_uid(obj, dio::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: dio::clean_uid(obj, dio::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: dio::clean_uid(obj, dio::TAG_FRAME_OF_REFERENCE_UID),
        identification: dio::read_identification(obj),
        content_date: dio::clean_string(obj, dio::TAG_CONTENT_DATE),
        content_time: dio::clean_string(obj, dio::TAG_CONTENT_TIME),
        ..Document::default()
    };
    let items = dio::sequence(obj, SURFACE_SEQUENCE);
    if items.is_empty() {
        return Err(invalid("no Surface Sequence"));
    }
    if obj.get_int(NUMBER_OF_SURFACES).ok() != Some(items.len() as i64) {
        return Err(invalid(format!(
            "Number of Surfaces does not match the {} Surface Sequence items",
            items.len()
        )));
    }
    let mut numbers = HashSet::new();
    let (mut points, mut triangles) = (0i64, 0i64);
    for (i, item) in items.iter().enumerate() {
        let surface = read_surface(item, &mut points, &mut triangles)
            .map_err(|e| e.prefixed(format!("surface item {}", i + 1)))?;
        if surface.number < 1 || !numbers.insert(surface.number) {
            return Err(invalid(format!(
                "Surface Number {} is not unique and positive",
                surface.number
            )));
        }
        doc.surfaces.push(surface);
    }
    for item in &dio::sequence(obj, SEGMENT_SEQUENCE) {
        let segment = read_segment(item)?;
        for reference in &segment.surfaces {
            if !numbers.contains(&reference.number) {
                return Err(invalid(format!(
                    "segment {} references missing surface {}",
                    segment.segment.number, reference.number
                )));
            }
        }
        doc.segments.push(segment);
    }
    if doc.segments.is_empty() {
        return Err(invalid("no Segment Sequence"));
    }
    doc.referenced_images = read_referenced_series(obj);
    Ok(doc)
}
fn read_code(obj: &Object, tag: Tag) -> CodedEntry {
    match dio::sequence(obj, tag).first() {
        None => CodedEntry::default(),
        Some(item) => CodedEntry {
            code_value: dio::clean_string(item, CODE_VALUE),
            coding_scheme_designator: dio::clean_string(item, CODING_SCHEME_DESIGNATOR),
            code_meaning: dio::clean_string(item, CODE_MEANING),
        },
    }
}
fn read_algorithm(obj: &Object, tag: Tag) -> Algorithm {
    match dio::sequence(obj, tag).first() {
        None => Algorithm::default(),
        Some(item) => Algorithm {
            family: read_code(item, ALGORITHM_FAMILY_CODE_SEQ),
            name_code: read_code(item, ALGORITHM_NAME_CODE_SEQ),
            name: dio::clean_string(item, ALGORITHM_NAME),
            version: dio::clean_string(item, ALGORITHM_VERSION),
            parameters: dio::clean_string(item, ALGORITHM_PARAMETERS),
        },
    }
}
fn read_instance_reference(item: &Object) -> ReferencedImage {
    ReferencedImage {
        series_instance_uid: String::new(),
        sop_class_uid: dio::clean_uid(item, dio::TAG_REF_SOP_CLASS_UID),
        sop_instance_uid: dio::clean_uid(item, dio::TAG_REF_SOP_INSTANCE_UID),
        frames: dio::ints(Some(item), dio::TAG_REF_FRAME_NUMBER).unwrap_or_default(),
    }
}
fn cielab(item: &Object) -> Result<Option<[u16; 3]>, i64> {
    let lab = dio::ints(Some(item), RECOMMENDED_CIELAB).unwrap_or_default();
    if lab.len() < 3 {
        return Ok(None);
    }
    let mut out = [0u16; 3];
    for k in 0..3 {
        out[k] = u16::try_from(lab[k]).map_err(|_| lab[k])?;
    }
    Ok(Some(out))
}
fn read_segment(item: &Object) -> Result<Segment, Error> {
    let mut segment = Segment {
        segment: seg::Segment {
            number: dio::int(item, SEGMENT_NUMBER),
            label: dio::clean_string(item, SEGMENT_LABEL),
            algorithm_type: dio::clean_string(item, SEGMENT_ALGORITHM_TYPE),
            algorithm_name: dio::clean_string(item, SEGMENT_ALGORITHM_NAME),
            segmented_property_category: read_code(item, SEGMENTED_PROPERTY_CATEGORY),
            segmented_property_type: read_code(item, SEGMENTED_PROPERTY_TYPE),
            ..seg::Segment::default()
        },
        surfaces: Vec::new(),
    };
    let number = segment.segment.number;
    match cielab(item) {
        Ok(Some(lab)) => segment.segment.recommended_display_cielab = lab,
        Ok(None) => {}
        Err(value) => {
            return Err(invalid(format!("segment {number} CIELab value {value}")));
        }
    }
    if number < 1 {
        return Err(invalid(format!("Segment Number {number}")));
    }
    let refs = dio::sequence(item, REFERENCED_SURFACE_SEQUENCE);
    if item.get_int(SURFACE_COUNT).ok() != Some(refs.len() as i64) {
        return Err(invalid(format!(
            "segment {number} Surface Count does not match its {} referenced surfaces",
            refs.len()
        )));
    }
    for ref_item in &refs {
        segment.surfaces.push(SurfaceReference {
            number: dio::int(ref_item, REFERENCED_SURFACE_NUMBER),
            algorithm: read_algorithm(ref_item, GENERATION_ALGORITHM_SEQ),
            source_instances: dio::sequence(ref_item, SOURCE_INSTANCE_SEQUENCE)
                .iter()
                .map(read_instance_reference)
                .collect(),
        });
    }
    Ok(segment)
}
fn read_referenced_series(obj: &Object) -> Vec<ReferencedImage> {
    let mut out = Vec::new();
    for series in &dio::sequence(obj, REFERENCED_SERIES_SEQUENCE) {
        let uid = dio::clean_uid(series, dio::TAG_SERIES_INSTANCE_UID);
        for item in &dio::sequence(series, REFERENCED_INSTANCE_SEQUENCE) {
            let mut reference = read_instance_reference(item);
            reference.series_instance_uid = uid.clone();
            out.push(reference);
        }
    }
    out
}
fn float32s(item: &Object, tag: Tag) -> Result<Vec<f32>, String> {
    let Some(element) = item.get(tag) else {
        return Err(format!("{tag} is missing"));
    };
    let raw = match element.raw() {
        Some(raw) if raw.len() % 4 == 0 => raw,
        _ => return Err(format!("{tag} is not a list of 32-bit floats")),
    };
    let big = item.value_big_endian();
    let mut out = Vec::with_capacity(raw.len() / 4);
    for c in raw.as_chunks::<4>().0 {
        let bytes = [c[0], c[1], c[2], c[3]];
        out.push(f32::from_bits(if big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }));
    }
    if out.iter().any(|v| !v.is_finite()) {
        return Err(format!("{tag} holds a value that is not finite"));
    }
    Ok(out)
}
fn triples(values: &[f32], count: usize) -> Vec<[f32; 3]> {
    (0..count)
        .map(|i| [values[3 * i], values[3 * i + 1], values[3 * i + 2]])
        .collect()
}
fn read_surface(item: &Object, points: &mut i64, triangles: &mut i64) -> Result<Surface, Error> {
    let mut s = Surface {
        number: dio::int(item, SURFACE_NUMBER),
        comments: dio::clean_string(item, SURFACE_COMMENTS),
        presentation_type: dio::clean_string(item, PRESENTATION_TYPE),
        finite_volume: dio::clean_string(item, FINITE_VOLUME),
        manifold: dio::clean_string(item, MANIFOLD),
        opacity_set: true,
        ..Surface::default()
    };
    let n = s.number;
    if let Ok(gray) = u16::try_from(dio::int(item, RECOMMENDED_GRAYSCALE)) {
        s.recommended_display_grayscale = gray;
    }
    match cielab(item) {
        Ok(Some(lab)) => s.recommended_display_cielab = lab,
        Ok(None) => {}
        Err(value) => return Err(invalid(format!("CIELab value {value}"))),
    }
    match item.get_float(PRESENTATION_OPACITY) {
        Ok(opacity) => s.opacity = opacity,
        Err(_) => {
            s.opacity = 1.0;
            s.opacity_set = false;
        }
    }
    if dio::clean_string(item, SURFACE_PROCESSING) == YES {
        s.processing = Some(Processing {
            ratio: item.get_float(SURFACE_PROCESSING_RATIO).unwrap_or(0.0),
            description: dio::clean_string(item, SURFACE_PROCESSING_DESCR),
            algorithm: read_algorithm(item, PROCESSING_ALGORITHM_SEQ),
        });
    }
    let point_items = dio::sequence(item, SURFACE_POINTS_SEQUENCE);
    if point_items.len() != 1 {
        return Err(invalid(format!(
            "surface {n} has {} Surface Points Sequence items",
            point_items.len()
        )));
    }
    let coordinates = float32s(&point_items[0], POINT_COORDINATES_DATA)
        .map_err(|e| invalid(format!("surface {n} points: {e}")))?;
    let count = match point_items[0].get_int(NUMBER_OF_SURFACE_POINTS) {
        Ok(count) if count >= 0 && count.checked_mul(3) == Some(coordinates.len() as i64) => count,
        _ => {
            return Err(invalid(format!(
                "surface {n} Number of Surface Points does not match {} coordinates",
                coordinates.len()
            )));
        }
    };
    *points += count;
    if *points > MAX_READ_POINTS {
        return Err(limit(format!("more than {MAX_READ_POINTS} points")));
    }
    let mut m = Mesh {
        vertices: triples(&coordinates, count as usize),
        normals: None,
        triangles: Vec::new(),
    };
    if let Some(normal_item) = dio::sequence(item, SURFACE_NORMALS_SEQUENCE).first() {
        let vectors = float32s(normal_item, VECTOR_COORDINATE_DATA)
            .map_err(|e| invalid(format!("surface {n} normals: {e}")))?;
        let vector_count = dio::int(normal_item, NUMBER_OF_VECTORS);
        let dimensions = dio::int(normal_item, VECTOR_DIMENSIONALITY);
        if dimensions != 3
            || vector_count != count
            || Some(vectors.len() as i64) != vector_count.checked_mul(3)
        {
            return Err(invalid(format!(
                "surface {n} has {vector_count} normals of dimension {dimensions} for {count} points"
            )));
        }
        if normal_item.has(VECTOR_ACCURACY) {
            let accuracy = float32s(normal_item, VECTOR_ACCURACY)
                .map_err(|e| invalid(format!("surface {n} Vector Accuracy: {e}")))?;
            validate_vector_accuracy(&accuracy).map_err(|e| e.prefixed(format!("surface {n}")))?;
            s.vector_accuracy = accuracy;
        }
        m.normals = Some(triples(&vectors, vector_count as usize));
    }
    let primitives = dio::sequence(item, MESH_PRIMITIVES_SEQUENCE);
    if primitives.len() != 1 {
        return Err(invalid(format!(
            "surface {n} has {} Surface Mesh Primitives Sequence items",
            primitives.len()
        )));
    }
    let (tris, preserved) = read_primitives(&primitives[0], count as u32, triangles)
        .map_err(|e| e.prefixed(format!("surface {n}")))?;
    m.triangles = tris;
    s.primitives = preserved;
    s.retired_index_lists = only_retired_index_lists(&primitives[0]);
    if let Err(e) = m.validate() {
        return Err(invalid(format!("surface {n}: {e}")));
    }
    let empty = m.vertices.is_empty() || m.triangles.is_empty() && s.primitives.count() == 0;
    s.mesh = Some(m);
    if empty {
        return Err(invalid(format!(
            "surface {n} has no points or mesh primitives"
        )));
    }
    Ok(s)
}
struct TriangleSink<'a> {
    out: Vec<[u32; 3]>,
    total: &'a mut i64,
}
impl TriangleSink<'_> {
    fn add(&mut self, a: u32, b: u32, c: u32) -> Result<(), Error> {
        if a == b || b == c || a == c {
            return Ok(());
        }
        *self.total += 1;
        if *self.total > MAX_READ_TRIANGLES {
            return Err(limit(format!("more than {MAX_READ_TRIANGLES} triangles")));
        }
        self.out.push([a, b, c]);
        Ok(())
    }
    fn fan(&mut self, v: &[u32]) -> Result<(), Error> {
        for i in 2..v.len() {
            self.add(v[0], v[i - 1], v[i])?;
        }
        Ok(())
    }
    fn strip(&mut self, v: &[u32]) -> Result<(), Error> {
        for i in 2..v.len() {
            let (mut a, mut b) = (v[i - 2], v[i - 1]);
            if i % 2 == 1 {
                std::mem::swap(&mut a, &mut b);
            }
            self.add(a, b, v[i])?;
        }
        Ok(())
    }
}
fn read_primitives(
    item: &Object,
    points: u32,
    total: &mut i64,
) -> Result<(Vec<[u32; 3]>, Primitives), Error> {
    let list = index_list(
        item,
        LONG_TRIANGLE_POINT_INDEX,
        TRIANGLE_POINT_INDEX_LIST,
        points,
    )?;
    if list.len() % 3 != 0 {
        return Err(invalid(format!(
            "triangle index list of {} values",
            list.len()
        )));
    }
    if (list.len() / 3) as i64 > MAX_READ_TRIANGLES - *total {
        return Err(limit(format!("more than {MAX_READ_TRIANGLES} triangles")));
    }
    let mut sink = TriangleSink {
        out: Vec::with_capacity(list.len() / 3),
        total,
    };
    for t in list.as_chunks::<3>().0 {
        sink.add(t[0], t[1], t[2])?;
    }
    for p in &dio::sequence(item, TRIANGLE_STRIP_SEQUENCE) {
        let v = primitive_lists_one(p, points)?;
        sink.strip(&v)?;
    }
    for tag in [TRIANGLE_FAN_SEQUENCE, FACET_SEQUENCE] {
        for p in &dio::sequence(item, tag) {
            let v = primitive_lists_one(p, points)?;
            sink.fan(&v)?;
        }
    }
    let mut preserved = Primitives {
        vertices: index_list(
            item,
            LONG_VERTEX_POINT_INDEX,
            VERTEX_POINT_INDEX_LIST,
            points,
        )?,
        ..Primitives::default()
    };
    let edges = index_list(item, LONG_EDGE_POINT_INDEX, EDGE_POINT_INDEX_LIST, points)?;
    if edges.len() % 2 != 0 {
        return Err(invalid(format!(
            "edge index list has incomplete pair ({} indices)",
            edges.len()
        )));
    }
    preserved.edges = edges
        .as_chunks::<2>()
        .0
        .iter()
        .map(|e| -> Edge { [e[0], e[1]] })
        .collect();
    for line in &dio::sequence(item, LINE_SEQUENCE) {
        preserved.lines.push(index_list(
            line,
            LONG_PRIMITIVE_POINT_INDEX,
            PRIMITIVE_POINT_INDEX_LIST,
            points,
        )?);
    }
    preserved.validate(points)?;
    Ok((sink.out, preserved))
}
fn primitive_lists_one(p: &Object, points: u32) -> Result<Vec<u32>, Error> {
    let indices = index_list(
        p,
        LONG_PRIMITIVE_POINT_INDEX,
        PRIMITIVE_POINT_INDEX_LIST,
        points,
    )?;
    if indices.len() < 3 {
        return Err(invalid(format!("primitive of {} points", indices.len())));
    }
    Ok(indices)
}
fn only_retired_index_lists(item: &Object) -> bool {
    let mut retired = false;
    for (long, old) in [
        (LONG_VERTEX_POINT_INDEX, VERTEX_POINT_INDEX_LIST),
        (LONG_EDGE_POINT_INDEX, EDGE_POINT_INDEX_LIST),
        (LONG_TRIANGLE_POINT_INDEX, TRIANGLE_POINT_INDEX_LIST),
    ] {
        if item.has(long) {
            return false;
        }
        retired = retired || item.has(old);
    }
    for tag in [
        LINE_SEQUENCE,
        TRIANGLE_STRIP_SEQUENCE,
        TRIANGLE_FAN_SEQUENCE,
        FACET_SEQUENCE,
    ] {
        for primitive in &dio::sequence(item, tag) {
            if primitive.has(LONG_PRIMITIVE_POINT_INDEX) {
                return false;
            }
            retired = retired || primitive.has(PRIMITIVE_POINT_INDEX_LIST);
        }
    }
    retired
}
fn index_list(item: &Object, long: Tag, retired: Tag, points: u32) -> Result<Vec<u32>, Error> {
    let tag = select_index_tag(item, long, retired)?;
    let big = item.value_big_endian();
    let mut out: Vec<u32> = Vec::new();
    if let Some(element) = item.get(tag) {
        let width = if tag == long { 4 } else { 2 };
        let raw = match element.raw() {
            Some(raw) if raw.len() % width == 0 => raw,
            _ if tag == long => {
                return Err(invalid(format!("{long} is not a list of 32-bit indices")));
            }
            _ => {
                return Err(invalid(format!(
                    "{retired} is not a list of 16-bit indices"
                )));
            }
        };
        out = raw
            .chunks_exact(width)
            .map(|c| match (width, big) {
                (4, true) => u32::from_be_bytes([c[0], c[1], c[2], c[3]]),
                (4, false) => u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                (_, true) => u32::from(u16::from_be_bytes([c[0], c[1]])),
                (_, false) => u32::from(u16::from_le_bytes([c[0], c[1]])),
            })
            .collect();
    }
    for v in &mut out {
        if *v < 1 || *v > points {
            return Err(invalid(format!("point index {v} outside 1..{points}")));
        }
        *v -= 1;
    }
    Ok(out)
}
