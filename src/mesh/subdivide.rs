use super::attributes::{
    MAX_VERTEX_SCALAR_ATTRIBUTES, VertexScalarAttribute, scalar_name, scalar_units,
};
use super::curvature::{curvature_topology, sorted_corner_edges, validate_curvature_input};
use super::{ErrorKind, Mesh, MeshError, cross3, dot3, mesh_bytes, sub3};
use crate::fmath;
use crate::textfmt;
use std::fmt;
pub const SUBDIVISION_LINEAR: u8 = 0;
pub const SUBDIVISION_EQUAL_ONLY: u8 = 1;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubdivisionVertexAttribute {
    pub attribute: VertexScalarAttribute,
    pub policy: u8,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubdivisionOptions {
    pub max_edge_length_mm: f64,
    pub max_vertices: i64,
    pub max_triangles: i64,
    pub max_bytes: i64,
    pub max_passes: i64,
    pub vertex_attributes: Vec<SubdivisionVertexAttribute>,
    pub face_attributes: Vec<VertexScalarAttribute>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubdivisionReport {
    pub criterion_mm: f64,
    pub input_vertices: i64,
    pub input_triangles: i64,
    pub vertices: i64,
    pub triangles: i64,
    pub passes: i64,
    pub allocated_bytes: i64,
    pub max_edge_length_mm: f64,
    pub stop: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubdivisionResult {
    pub mesh: Mesh,
    pub vertex_attributes: Vec<SubdivisionVertexAttribute>,
    pub face_attributes: Vec<VertexScalarAttribute>,
    pub parent_faces: Vec<u32>,
    pub report: SubdivisionReport,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SubdivisionError {
    pub report: SubdivisionReport,
    pub cause: MeshError,
}
impl fmt::Display for SubdivisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mesh: subdivision {} after {} passes: {}",
            self.report.stop, self.report.passes, self.cause
        )
    }
}
impl std::error::Error for SubdivisionError {}
fn budget_error(detail: String) -> MeshError {
    MeshError {
        kind: ErrorKind::BudgetExceeded,
        message: if detail.is_empty() {
            "mesh: surface exceeds budget".to_string()
        } else {
            format!("mesh: surface exceeds budget: {detail}")
        },
        budget: None,
    }
}
fn precision_error() -> MeshError {
    MeshError {
        kind: ErrorKind::SubdivisionPrecision,
        message: "mesh: subdivision midpoint/normal is not representable".to_string(),
        budget: None,
    }
}
fn subdivision_counts(n: i64, t: i64, o: &SubdivisionOptions) -> Result<(), MeshError> {
    let max_u32 = i64::from(u32::MAX);
    if n < 0
        || t < 0
        || n >= max_u32
        || t > (max_u32 - 1) / 3
        || n > o.max_vertices
        || t > o.max_triangles
    {
        return Err(budget_error(format!(
            "subdivision needs {n} vertices/{t} triangles, limits {}/{}",
            o.max_vertices, o.max_triangles
        )));
    }
    Ok(())
}
fn subdivision_charge(
    r: &mut SubdivisionReport,
    b: i64,
    o: &SubdivisionOptions,
) -> Result<(), MeshError> {
    if b < 0 || r.allocated_bytes > o.max_bytes || b > o.max_bytes - r.allocated_bytes {
        return Err(budget_error(format!(
            "subdivision logical bytes charged {} + {b} exceed {}",
            r.allocated_bytes, o.max_bytes
        )));
    }
    r.allocated_bytes += b;
    Ok(())
}
fn subdivision_output_bytes(n: i64, t: i64, normals: bool, va: usize, fa: usize) -> i64 {
    mesh_bytes(n, t, normals)
        + 4 * t
        + 8 * n * va as i64
        + 8 * t * fa as i64
        + 256 * (va + fa) as i64
        + 512
}
fn subdivision_allocate(
    normals: bool,
    n: usize,
    t: usize,
    va: &[SubdivisionVertexAttribute],
    fa: &[VertexScalarAttribute],
) -> SubdivisionResult {
    SubdivisionResult {
        mesh: Mesh {
            vertices: vec![[0.0; 3]; n],
            normals: normals.then(|| vec![[0.0; 3]; n]),
            triangles: vec![[0; 3]; t],
        },
        vertex_attributes: va
            .iter()
            .map(|a| SubdivisionVertexAttribute {
                attribute: VertexScalarAttribute {
                    name: a.attribute.name.clone(),
                    values: vec![0.0; n],
                    units: a.attribute.units.clone(),
                },
                policy: a.policy,
            })
            .collect(),
        face_attributes: fa
            .iter()
            .map(|a| VertexScalarAttribute {
                name: a.name.clone(),
                values: vec![0.0; t],
                units: a.units.clone(),
            })
            .collect(),
        parent_faces: vec![0; t],
        report: SubdivisionReport::default(),
    }
}
fn subdivision_attributes(m: &Mesh, o: &SubdivisionOptions) -> Result<(), MeshError> {
    let validate = |a: &VertexScalarAttribute, n: usize| -> Result<(), MeshError> {
        if matches!(
            a.name.to_ascii_lowercase().as_str(),
            "x" | "y" | "z" | "nx" | "ny" | "nz"
        ) {
            return Err(MeshError::invalid("reserved attribute name"));
        }
        if !scalar_name(&a.name) || !scalar_units(&a.units) || a.values.len() != n {
            return Err(MeshError::invalid(format!(
                "subdivision attribute contract {}",
                textfmt::quote(&a.name)
            )));
        }
        if a.values.iter().any(|v| !v.is_finite()) {
            return Err(MeshError::invalid(format!(
                "subdivision nonfinite attribute {}",
                textfmt::quote(&a.name)
            )));
        }
        Ok(())
    };
    for (i, a) in o.vertex_attributes.iter().enumerate() {
        if a.policy > SUBDIVISION_EQUAL_ONLY {
            return Err(MeshError::invalid("subdivision interpolation policy"));
        }
        validate(&a.attribute, m.vertices.len())?;
        if o.vertex_attributes[..i]
            .iter()
            .any(|b| b.attribute.name == a.attribute.name)
        {
            return Err(MeshError::invalid("duplicate vertex attribute"));
        }
    }
    for (i, a) in o.face_attributes.iter().enumerate() {
        validate(a, m.triangles.len())?;
        if o.face_attributes[..i].iter().any(|b| b.name == a.name) {
            return Err(MeshError::invalid("duplicate face attribute"));
        }
    }
    Ok(())
}
fn subdivision_edge_length(m: &Mesh, a: u32, b: u32) -> f64 {
    let d = sub3(m.point(a), m.point(b));
    fmath::hypot(fmath::hypot(d[0], d[1]), d[2])
}
fn subdivision_geometry(m: &Mesh) -> Result<(), MeshError> {
    for (i, f) in m.triangles.iter().enumerate() {
        let (a, b, c) = (m.point(f[0]), m.point(f[1]), m.point(f[2]));
        let normal = cross3(sub3(b, a), sub3(c, a));
        if dot3(normal, normal) == 0.0 {
            return Err(MeshError::invalid(format!(
                "subdivision degenerate triangle {i}"
            )));
        }
    }
    Ok(())
}
fn subdivision_midpoint(
    current: &SubdivisionResult,
    next: &mut SubdivisionResult,
    a: u32,
    b: u32,
    mid: u32,
) -> Result<(), MeshError> {
    let (a, b, mid) = (a as usize, b as usize, mid as usize);
    let (pa, pb) = (current.mesh.vertices[a], current.mesh.vertices[b]);
    let mut p = [0f32; 3];
    for k in 0..3 {
        p[k] = (f64::from(pa[k]) * 0.5 + f64::from(pb[k]) * 0.5) as f32;
    }
    if p == pa || p == pb {
        return Err(precision_error());
    }
    next.mesh.vertices[mid] = p;
    if let (Some(normals), Some(out)) = (&current.mesh.normals, &mut next.mesh.normals) {
        let (na, nb) = (normals[a], normals[b]);
        let mut v = [0f64; 3];
        for k in 0..3 {
            v[k] = f64::from(na[k]) * 0.5 + f64::from(nb[k]) * 0.5;
        }
        let length = fmath::hypot(fmath::hypot(v[0], v[1]), v[2]);
        if length == 0.0 {
            return Err(precision_error());
        }
        for k in 0..3 {
            out[mid][k] = (v[k] / length) as f32;
        }
    }
    for (i, attr) in current.vertex_attributes.iter().enumerate() {
        let (x, y) = (attr.attribute.values[a], attr.attribute.values[b]);
        if attr.policy == SUBDIVISION_EQUAL_ONLY && x != y {
            return Err(MeshError::invalid(format!(
                "noninterpolable vertex attribute {} crosses unequal endpoints",
                textfmt::quote(&attr.attribute.name)
            )));
        }
        let sum = x + y;
        let mut value = sum * 0.5;
        if sum.is_infinite() {
            value = x * 0.5 + y * 0.5;
        }
        next.vertex_attributes[i].attribute.values[mid] = value;
    }
    Ok(())
}
const NO_MID: u32 = u32::MAX;
fn subdivision_children(f: [u32; 3], mid: [u32; 3]) -> ([[u32; 3]; 4], usize) {
    let mut out = [[0u32; 3]; 4];
    let n = mid.iter().filter(|m| **m != NO_MID).count();
    if n == 0 {
        out[0] = f;
        return (out, 1);
    }
    if n == 3 {
        let (a, b, c) = (f[0], f[1], f[2]);
        let (x, y, z) = (mid[0], mid[1], mid[2]);
        return ([[a, x, z], [x, b, y], [z, y, c], [x, y, z]], 4);
    }
    for i in 0..3 {
        let (a, b, c) = (f[i], f[(i + 1) % 3], f[(i + 2) % 3]);
        if n == 1 && mid[i] != NO_MID {
            let x = mid[i];
            out[0] = [a, x, c];
            out[1] = [x, b, c];
            return (out, 2);
        }
        if n == 2 && mid[i] != NO_MID && mid[(i + 1) % 3] != NO_MID {
            let (x, y) = (mid[i], mid[(i + 1) % 3]);
            out[0] = [b, y, x];
            out[1] = [a, x, c];
            out[2] = [x, y, c];
            return (out, 3);
        }
    }
    out[0] = f;
    (out, 1)
}
pub fn subdivide_edges(
    m: &Mesh,
    o: &SubdivisionOptions,
) -> Result<SubdivisionResult, Box<SubdivisionError>> {
    let mut report = SubdivisionReport {
        criterion_mm: o.max_edge_length_mm,
        ..SubdivisionReport::default()
    };
    match subdivide(m, o, &mut report) {
        Ok(result) => Ok(result),
        Err(cause) => {
            if report.stop.is_empty() {
                report.stop = "refused".to_string();
            }
            Err(Box::new(SubdivisionError { report, cause }))
        }
    }
}
#[doc(hidden)]
pub fn subdivision_options_error(max_edge_length_mm: f64) -> SubdivisionError {
    SubdivisionError {
        report: SubdivisionReport {
            criterion_mm: max_edge_length_mm,
            stop: "refused".to_string(),
            ..SubdivisionReport::default()
        },
        cause: MeshError::invalid("subdivision options/mesh"),
    }
}
fn stopped(report: &mut SubdivisionReport, reason: &str, e: MeshError) -> MeshError {
    report.stop = reason.to_string();
    e
}
fn subdivide(
    m: &Mesh,
    o: &SubdivisionOptions,
    report: &mut SubdivisionReport,
) -> Result<SubdivisionResult, MeshError> {
    if !o.max_edge_length_mm.is_finite()
        || o.max_edge_length_mm <= 0.0
        || o.max_vertices <= 0
        || o.max_triangles <= 0
        || o.max_bytes <= 0
        || o.max_passes <= 0
        || o.max_passes > 64
        || o.vertex_attributes.len() > MAX_VERTEX_SCALAR_ATTRIBUTES
        || o.face_attributes.len() > MAX_VERTEX_SCALAR_ATTRIBUTES
    {
        return Err(MeshError::invalid("subdivision options/mesh"));
    }
    let (mut n, mut t) = (m.vertices.len() as i64, m.triangles.len() as i64);
    report.input_vertices = n;
    report.input_triangles = t;
    report.vertices = n;
    report.triangles = t;
    if let Err(e) = subdivision_counts(n, t, o) {
        return Err(stopped(report, "counts", e));
    }
    let (va, fa) = (o.vertex_attributes.len(), o.face_attributes.len());
    if let Err(e) = subdivision_charge(report, 4096 + 256 * (va + fa) as i64 + 111 * t + 5 * n, o) {
        return Err(stopped(report, "bytes", e));
    }
    validate_curvature_input(m)?;
    if let Some(normals) = &m.normals
        && normals.iter().flatten().any(|c| !c.is_finite())
    {
        return Err(MeshError::invalid("subdivision normal"));
    }
    subdivision_geometry(m)?;
    curvature_topology(m, &mut vec![0u8; m.vertices.len()])?;
    subdivision_attributes(m, o)?;
    let has_normals = m.normals.is_some();
    if let Err(e) = subdivision_charge(
        report,
        subdivision_output_bytes(n, t, has_normals, va, fa),
        o,
    ) {
        return Err(stopped(report, "bytes", e));
    }
    let mut current = SubdivisionResult {
        mesh: m.clone(),
        vertex_attributes: o.vertex_attributes.clone(),
        face_attributes: o.face_attributes.clone(),
        parent_faces: (0..t as u32).collect(),
        report: SubdivisionReport::default(),
    };
    loop {
        n = current.mesh.vertices.len() as i64;
        t = current.mesh.triangles.len() as i64;
        if let Err(e) = subdivision_charge(report, 108 * t, o) {
            return Err(stopped(report, "bytes", e));
        }
        let edges = sorted_corner_edges(&current.mesh.triangles);
        let mut mids = vec![NO_MID; edges.len()];
        let mut split: i64 = 0;
        report.max_edge_length_mm = 0.0;
        let mut start = 0;
        while start < edges.len() {
            let mut end = start + 1;
            while end < edges.len() && edges[end].key == edges[start].key {
                end += 1;
            }
            let (a, b) = ((edges[start].key >> 32) as u32, edges[start].key as u32);
            let length = subdivision_edge_length(&current.mesh, a, b);
            report.max_edge_length_mm = fmath::max(report.max_edge_length_mm, length);
            if length > o.max_edge_length_mm {
                let mid = (n + split) as u32;
                split += 1;
                for e in &edges[start..end] {
                    mids[e.corner as usize] = mid;
                }
            }
            start = end;
        }
        if split == 0 {
            report.stop = "complete".to_string();
            report.vertices = n;
            report.triangles = t;
            current.report = report.clone();
            return Ok(current);
        }
        if report.passes == o.max_passes {
            return Err(stopped(report, "passes", budget_error(String::new())));
        }
        let next_t = t + mids.iter().filter(|m| **m != NO_MID).count() as i64;
        if let Err(e) = subdivision_counts(n + split, next_t, o) {
            return Err(stopped(report, "counts", e));
        }
        if let Err(e) = subdivision_charge(
            report,
            subdivision_output_bytes(n + split, next_t, has_normals, va, fa),
            o,
        ) {
            return Err(stopped(report, "bytes", e));
        }
        let mut next = subdivision_allocate(
            has_normals,
            (n + split) as usize,
            next_t as usize,
            &current.vertex_attributes,
            &current.face_attributes,
        );
        let old = n as usize;
        next.mesh.vertices[..old].copy_from_slice(&current.mesh.vertices);
        if let (Some(from), Some(to)) = (&current.mesh.normals, &mut next.mesh.normals) {
            to[..old].copy_from_slice(from);
        }
        for (to, from) in next
            .vertex_attributes
            .iter_mut()
            .zip(&current.vertex_attributes)
        {
            to.attribute.values[..old].copy_from_slice(&from.attribute.values);
        }
        let mut start = 0;
        while start < edges.len() {
            let mut end = start + 1;
            while end < edges.len() && edges[end].key == edges[start].key {
                end += 1;
            }
            let mid = mids[edges[start].corner as usize];
            if mid != NO_MID {
                let (a, b) = ((edges[start].key >> 32) as u32, edges[start].key as u32);
                if let Err(e) = subdivision_midpoint(&current, &mut next, a, b, mid) {
                    return Err(stopped(report, "precision-or-attribute", e));
                }
            }
            start = end;
        }
        let mut face_at = 0;
        for (f, face) in current.mesh.triangles.iter().enumerate() {
            let (children, count) =
                subdivision_children(*face, [mids[3 * f], mids[3 * f + 1], mids[3 * f + 2]]);
            let (pa, pb, pc) = (
                current.mesh.point(face[0]),
                current.mesh.point(face[1]),
                current.mesh.point(face[2]),
            );
            let parent_normal = cross3(sub3(pb, pa), sub3(pc, pa));
            for child in &children[..count] {
                let (ca, cb, cc) = (
                    next.mesh.point(child[0]),
                    next.mesh.point(child[1]),
                    next.mesh.point(child[2]),
                );
                if dot3(parent_normal, cross3(sub3(cb, ca), sub3(cc, ca))) <= 0.0 {
                    return Err(stopped(report, "precision", precision_error()));
                }
                next.mesh.triangles[face_at] = *child;
                next.parent_faces[face_at] = current.parent_faces[f];
                for (to, from) in next
                    .face_attributes
                    .iter_mut()
                    .zip(&current.face_attributes)
                {
                    to.values[face_at] = from.values[f];
                }
                face_at += 1;
            }
        }
        if let Err(e) = subdivision_geometry(&next.mesh) {
            return Err(stopped(report, "precision", e));
        }
        current = next;
        report.passes += 1;
        report.vertices = n + split;
        report.triangles = next_t;
    }
}
