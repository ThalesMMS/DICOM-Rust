#![forbid(unsafe_code)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
mod meshes;
mod read;
pub mod segment;
mod write;
use crate::derivation::DerivedCommon;
use crate::derivedio::Identification;
use crate::dicomjson::trim_unicode_space;
use crate::mesh::Mesh;
use crate::object::is_valid_uid;
use crate::textfmt::{float_v, quote};
pub use meshes::{FromMeshesOptions, MeshSegment, from_meshes, surface_nets_algorithm};
pub use read::{ReadLimits, read, read_with_limits};
use segment::{self as seg, CodedEntry, ReferencedImage};
use std::collections::HashSet;
pub use write::write;
pub const SURFACE_SEGMENTATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.5";
pub const DEFAULT_CONTENT_LABEL: &str = "SURFACE_SEG";
pub const PRESENTATION_SURFACE: &str = "SURFACE";
pub const PRESENTATION_WIREFRAME: &str = "WIREFRAME";
pub const PRESENTATION_POINTS: &str = "POINTS";
pub const YES: &str = "YES";
pub const NO: &str = "NO";
pub const UNKNOWN: &str = "UNKNOWN";
pub const MAX_READ_POINTS: i64 = 1 << 26;
pub const MAX_READ_TRIANGLES: i64 = 1 << 27;
pub const MAX_READ_PRIMITIVES: i64 = 1 << 27;
pub const MAX_READ_INDICES: i64 = 1 << 29;
pub const MAX_READ_GEOMETRY_BYTES: i64 = 256 << 20;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    InvalidObject,
    UnsupportedSopClass,
    ResourceLimitExceeded,
    NonTrianglePrimitives,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidObject => "dicom/surfaceseg: invalid object",
            Kind::UnsupportedSopClass => "dicom/surfaceseg: unsupported SOP class",
            Kind::ResourceLimitExceeded => "dicom/surfaceseg: resource limit exceeded",
            Kind::NonTrianglePrimitives => {
                "dicom/surfaceseg: triangle-mesh export cannot preserve non-triangle primitives"
            }
            Kind::Other => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            text: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
    pub(crate) fn other(text: impl Into<String>) -> Error {
        Error {
            kind: Kind::Other,
            text: text.into(),
        }
    }
    pub(crate) fn prefixed(mut self, prefix: impl AsRef<str>) -> Error {
        self.text = format!("{}: {}", prefix.as_ref(), self.text);
        self
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) fn invalid(detail: impl AsRef<str>) -> Error {
    Error::new(Kind::InvalidObject, detail)
}
pub(crate) fn limit(detail: impl AsRef<str>) -> Error {
    Error::new(Kind::ResourceLimitExceeded, detail)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Algorithm {
    pub family: CodedEntry,
    pub name_code: CodedEntry,
    pub name: String,
    pub version: String,
    pub parameters: String,
}
fn complete_code(c: &CodedEntry) -> bool {
    !trim_unicode_space(&c.code_value).is_empty()
        && !trim_unicode_space(&c.coding_scheme_designator).is_empty()
        && !trim_unicode_space(&c.code_meaning).is_empty()
}
impl Algorithm {
    fn validate(&self) -> Result<(), &'static str> {
        if !complete_code(&self.family) {
            return Err("Algorithm Family Code Sequence is incomplete");
        }
        if self.name_code != CodedEntry::default() && !complete_code(&self.name_code) {
            return Err("Algorithm Name Code Sequence is incomplete");
        }
        if trim_unicode_space(&self.name).is_empty() || trim_unicode_space(&self.version).is_empty()
        {
            return Err("Algorithm Name and Algorithm Version are required");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceReference {
    pub number: i64,
    pub algorithm: Algorithm,
    pub source_instances: Vec<ReferencedImage>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Segment {
    pub segment: seg::Segment,
    pub surfaces: Vec<SurfaceReference>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Processing {
    pub ratio: f64,
    pub description: String,
    pub algorithm: Algorithm,
}
pub type Edge = [u32; 2];
pub type Polyline = Vec<u32>;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Primitives {
    pub vertices: Vec<u32>,
    pub edges: Vec<Edge>,
    pub lines: Vec<Polyline>,
}
impl Primitives {
    pub fn count(&self) -> usize {
        self.vertices.len() + self.edges.len() + self.lines.len()
    }
    pub(crate) fn validate(&self, points: u32) -> Result<(), Error> {
        let check = |v: u32| {
            if v >= points {
                return Err(invalid(format!(
                    "primitive point index {v} outside 0..{}",
                    points.wrapping_sub(1)
                )));
            }
            Ok(())
        };
        for &v in &self.vertices {
            check(v)?;
        }
        for e in &self.edges {
            check(e[0])?;
            check(e[1])?;
        }
        for line in &self.lines {
            if line.len() < 2 {
                return Err(invalid(format!(
                    "line has {} points; at least two required",
                    line.len()
                )));
            }
            for &v in line {
                check(v)?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Surface {
    pub number: i64,
    pub comments: String,
    pub mesh: Option<Mesh>,
    pub vector_accuracy: Vec<f32>,
    pub primitives: Primitives,
    pub retired_index_lists: bool,
    pub recommended_display_grayscale: u16,
    pub recommended_display_cielab: [u16; 3],
    pub opacity: f64,
    pub opacity_set: bool,
    pub presentation_type: String,
    pub finite_volume: String,
    pub manifold: String,
    pub processing: Option<Processing>,
    pub ignored_primitives: i64,
}
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub common: DerivedCommon,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub identification: Identification,
    pub content_date: String,
    pub content_time: String,
    pub segments: Vec<Segment>,
    pub surfaces: Vec<Surface>,
    pub referenced_images: Vec<ReferencedImage>,
}
fn valid_uid(uid: &str) -> bool {
    is_valid_uid(trim_unicode_space(uid).as_bytes())
}
fn one_of(value: &str, options: &[&str]) -> bool {
    options.contains(&value)
}
fn yes_no(b: bool) -> &'static str {
    if b { YES } else { NO }
}
pub(crate) fn validate_vector_accuracy(values: &[f32]) -> Result<(), Error> {
    if !matches!(values.len(), 0 | 1 | 3) {
        return Err(invalid(format!(
            "Vector Accuracy has {} values; expected one shared value or three XYZ values",
            values.len()
        )));
    }
    if values.iter().any(|v| !(*v >= 0.0) || *v == f32::INFINITY) {
        return Err(invalid(
            "Vector Accuracy values must be finite and nonnegative",
        ));
    }
    Ok(())
}
fn validate_write_geometry_resources(surfaces: &[Surface]) -> Result<(), Error> {
    let (mut points, mut triangles, mut primitives, mut indices, mut bytes) =
        (0i64, 0i64, 0i64, 0i64, 0i64);
    for s in surfaces {
        let Some(m) = &s.mesh else {
            return Err(invalid("surface has no point data"));
        };
        let normals = m.normals.as_ref().map_or(0, Vec::len) as i64;
        let (v, t) = (m.vertices.len() as i64, m.triangles.len() as i64);
        let p = &s.primitives;
        points += v;
        triangles += t;
        primitives += p.count() as i64 + t;
        indices += p.vertices.len() as i64 + 2 * p.edges.len() as i64 + 3 * t;
        bytes += 24 * (v + normals);
        bytes += 8 * s.vector_accuracy.len() as i64;
        bytes += 36 * t;
        bytes +=
            8 * p.vertices.len() as i64 + 24 * p.edges.len() as i64 + 24 * p.lines.len() as i64;
        for line in &p.lines {
            indices += line.len() as i64;
            bytes += 8 * line.len() as i64;
        }
        if points > MAX_READ_POINTS
            || triangles > MAX_READ_TRIANGLES
            || primitives > MAX_READ_PRIMITIVES
            || indices > MAX_READ_INDICES
            || bytes > MAX_READ_GEOMETRY_BYTES
        {
            return Err(limit("document geometry exceeds default read/write limits"));
        }
    }
    Ok(())
}
impl Document {
    pub fn surface(&self, number: i64) -> Option<&Surface> {
        self.surfaces.iter().find(|s| s.number == number)
    }
    pub fn validate(&self) -> Result<(), Error> {
        self.resolved().map(|_| ())
    }
    pub(crate) fn resolved(&self) -> Result<Vec<Surface>, Error> {
        for (name, uid) in [
            ("SOP Instance UID", &self.sop_instance_uid),
            ("Study Instance UID", &self.study_instance_uid),
            ("Series Instance UID", &self.series_instance_uid),
            ("Frame of Reference UID", &self.frame_of_reference_uid),
        ] {
            if !valid_uid(uid) {
                return Err(invalid(format!("{name} {} is not a valid UID", quote(uid))));
            }
        }
        if self.surfaces.is_empty() || self.segments.is_empty() {
            return Err(invalid(format!(
                "{} segments and {} surfaces; at least one of each is required",
                self.segments.len(),
                self.surfaces.len()
            )));
        }
        validate_write_geometry_resources(&self.surfaces)?;
        let mut surfaces = Vec::with_capacity(self.surfaces.len());
        for (i, surface) in self.surfaces.iter().enumerate() {
            let mut surface = surface.clone();
            let position = i as i64 + 1;
            if surface.number == 0 {
                surface.number = position;
            }
            let n = surface.number;
            if n != position {
                return Err(invalid(format!(
                    "surface {position} has Surface Number {n}; numbers run from 1 in order"
                )));
            }
            let Some(m) = &surface.mesh else {
                return Err(invalid("surface has no point data"));
            };
            if let Err(e) = m.validate() {
                return Err(invalid(format!("surface {n}: {e}")));
            }
            let normals = m.normals.as_ref().map_or(0, Vec::len);
            if !surface.vector_accuracy.is_empty() && normals == 0 {
                return Err(invalid(format!(
                    "surface {n} Vector Accuracy requires normals"
                )));
            }
            validate_vector_accuracy(&surface.vector_accuracy)
                .map_err(|e| e.prefixed(format!("surface {n}")))?;
            if m.vertices.is_empty() || m.vertices.len() as u64 >= u64::from(u32::MAX) {
                return Err(invalid(format!(
                    "surface {n} has {} points",
                    m.vertices.len()
                )));
            }
            if let Err(e) = surface.primitives.validate(m.vertices.len() as u32) {
                return Err(invalid(format!("surface {n}: {e}")));
            }
            if m.triangles.is_empty() && surface.primitives.count() == 0 {
                return Err(invalid(format!("surface {n} has no mesh primitives")));
            }
            if surface.retired_index_lists && m.vertices.len() > usize::from(u16::MAX) {
                return Err(invalid(format!(
                    "surface {n} retired indices require at most 65535 points"
                )));
            }
            if !surface.opacity_set && surface.opacity == 0.0 {
                surface.opacity = 1.0;
            }
            if !(surface.opacity >= 0.0 && surface.opacity <= 1.0) {
                return Err(invalid(format!(
                    "surface {n} opacity {} outside [0, 1]",
                    float_v(surface.opacity)
                )));
            }
            if surface.presentation_type.is_empty() {
                surface.presentation_type = PRESENTATION_SURFACE.to_string();
            }
            if !one_of(
                &surface.presentation_type,
                &[
                    PRESENTATION_SURFACE,
                    PRESENTATION_WIREFRAME,
                    PRESENTATION_POINTS,
                ],
            ) {
                return Err(invalid(format!(
                    "surface {n} presentation type {}",
                    quote(&surface.presentation_type)
                )));
            }
            if surface.finite_volume.is_empty() || surface.manifold.is_empty() {
                let closure = m.check_closure();
                let has_triangles = !m.triangles.is_empty();
                if surface.finite_volume.is_empty() {
                    surface.finite_volume = yes_no(has_triangles && closure.closed()).to_string();
                }
                if surface.manifold.is_empty() {
                    surface.manifold = yes_no(
                        has_triangles
                            && closure.non_manifold_edges == 0
                            && closure.inconsistent_edges == 0,
                    )
                    .to_string();
                }
            }
            if !one_of(&surface.finite_volume, &[YES, NO, UNKNOWN])
                || !one_of(&surface.manifold, &[YES, NO, UNKNOWN])
            {
                return Err(invalid(format!(
                    "surface {n} Finite Volume {}, Manifold {}",
                    quote(&surface.finite_volume),
                    quote(&surface.manifold)
                )));
            }
            if let Some(p) = &surface.processing {
                if !(p.ratio >= 0.0) || p.ratio.is_infinite() {
                    return Err(invalid(format!(
                        "surface {n} processing ratio {}",
                        float_v(p.ratio)
                    )));
                }
                if let Err(e) = p.algorithm.validate() {
                    return Err(invalid(format!("surface {n} processing: {e}")));
                }
            }
            surfaces.push(surface);
        }
        let mut numbers = HashSet::new();
        for segment in &self.segments {
            let s = &segment.segment;
            let number = s.number;
            if number < 1 || number > i64::from(u16::MAX) || !numbers.insert(number) {
                return Err(invalid(format!(
                    "segment number {number} is not unique in 1..65535"
                )));
            }
            if trim_unicode_space(&s.label).is_empty() {
                return Err(invalid(format!("segment {number} has no label")));
            }
            if !one_of(
                &s.algorithm_type,
                &[
                    seg::ALGORITHM_MANUAL,
                    seg::ALGORITHM_AUTOMATIC,
                    seg::ALGORITHM_SEMIAUTOMATIC,
                ],
            ) {
                return Err(invalid(format!(
                    "segment {number} algorithm type {}",
                    quote(&s.algorithm_type)
                )));
            }
            if let Err(e) = s.validate_algorithm() {
                return Err(invalid(e.to_string()));
            }
            if !complete_code(&s.segmented_property_category)
                || !complete_code(&s.segmented_property_type)
            {
                return Err(invalid(format!(
                    "segment {number} needs complete Segmented Property Category and Type codes"
                )));
            }
            if segment.surfaces.is_empty() {
                return Err(invalid(format!("segment {number} references no surface")));
            }
            let mut seen = HashSet::new();
            for reference in &segment.surfaces {
                let r = reference.number;
                if r < 1 || r > surfaces.len() as i64 || !seen.insert(r) {
                    return Err(invalid(format!(
                        "segment {number} references surface {r} (there are {}, each referenced once per segment)",
                        surfaces.len()
                    )));
                }
                if let Err(e) = reference.algorithm.validate() {
                    return Err(invalid(format!(
                        "segment {number} surface {r} generation: {e}"
                    )));
                }
                for source in &reference.source_instances {
                    if !valid_uid(&source.sop_class_uid) || !valid_uid(&source.sop_instance_uid) {
                        return Err(invalid(format!(
                            "segment {number} surface {r} source instance {} of class {} needs valid UIDs",
                            quote(&source.sop_instance_uid),
                            quote(&source.sop_class_uid)
                        )));
                    }
                }
                let surface = &mut surfaces[r as usize - 1];
                if surface.recommended_display_cielab == [0; 3] {
                    surface.recommended_display_cielab = s.recommended_display_cielab;
                }
            }
        }
        for surface in &mut surfaces {
            if surface.recommended_display_cielab == [0; 3] {
                surface.recommended_display_cielab = seg::rgb_to_dicom_cielab([128, 128, 128]);
            }
            if surface.recommended_display_grayscale == 0 {
                surface.recommended_display_grayscale = surface.recommended_display_cielab[0];
            }
        }
        for reference in &self.referenced_images {
            for uid in [
                &reference.series_instance_uid,
                &reference.sop_class_uid,
                &reference.sop_instance_uid,
            ] {
                if !valid_uid(uid) {
                    return Err(invalid(format!(
                        "referenced image {}: {} is not a valid UID",
                        quote(&reference.sop_instance_uid),
                        quote(uid)
                    )));
                }
            }
        }
        Ok(surfaces)
    }
}
