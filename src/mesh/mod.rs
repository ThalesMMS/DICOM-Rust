#![forbid(unsafe_code)]
use std::fmt;
pub mod attributes;
pub mod boolean;
pub(crate) mod bvh;
pub mod cases;
pub mod clean;
pub mod cubes;
pub mod curvature;
pub mod decimate;
pub mod extract;
pub mod field;
pub mod geodesic;
pub mod gltf;
pub mod io;
pub mod label_inputs;
pub mod locator;
pub mod occlusion;
pub mod plane;
pub mod pointcloud;
pub mod quality;
pub mod read;
pub mod smooth;
pub mod subdivide;
pub mod surface_nets;
pub mod voxel;
pub use attributes::VertexScalarAttribute;
pub use clean::{CleanReport, clean, connected_components, fill_holes, largest_component};
pub use extract::{Method, Options, extract, extract_volume};
pub use field::{DenseField, Field, SnapshotField, grid_affine, snapshot_field};
pub use gltf::{GlbOptions, Part, write_glb};
pub use io::{
    Closure, PlyOptions, merge, obj_material_name, write_mtl, write_obj, write_obj_parts,
    write_ply, write_stl,
};
pub use label_inputs::{label_field_from_masks, label_field_from_seg};
pub use read::{Material, MaterialResource, ObjModel, ReadOptions, read_mtl, read_obj, read_stl};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidField,
    BudgetExceeded,
    InvalidModel,
    ModelLimit,
    UnsupportedModel,
    SubdivisionPrecision,
    NoPath,
    PlaneGeometry,
    BooleanGeometry,
    Other,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MeshError {
    pub kind: ErrorKind,
    pub message: String,
    pub budget: Option<BudgetError>,
}
impl MeshError {
    pub fn invalid(detail: impl fmt::Display) -> MeshError {
        MeshError {
            kind: ErrorKind::InvalidField,
            message: format!("mesh: invalid field: {detail}"),
            budget: None,
        }
    }
    pub fn other(message: impl Into<String>) -> MeshError {
        MeshError {
            kind: ErrorKind::Other,
            message: message.into(),
            budget: None,
        }
    }
    pub fn wrap(mut self, prefix: &str) -> MeshError {
        self.message = format!("{prefix}{}", self.message);
        self
    }
}
impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for MeshError {}
impl From<std::io::Error> for MeshError {
    fn from(e: std::io::Error) -> MeshError {
        MeshError::other(e.to_string())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum VolumeMeshError {
    Render(crate::render::RenderError),
    Mesh(MeshError),
}
impl fmt::Display for VolumeMeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VolumeMeshError::Render(e) => write!(f, "{e}"),
            VolumeMeshError::Mesh(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for VolumeMeshError {}
impl From<crate::render::RenderError> for VolumeMeshError {
    fn from(e: crate::render::RenderError) -> VolumeMeshError {
        VolumeMeshError::Render(e)
    }
}
impl From<MeshError> for VolumeMeshError {
    fn from(e: MeshError) -> VolumeMeshError {
        VolumeMeshError::Mesh(e)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetError {
    pub vertices: i64,
    pub triangles: i64,
    pub bytes: i64,
    pub max_triangles: i64,
    pub max_bytes: i64,
}
impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mesh: surface of {} triangles and {} vertices ({} bytes) exceeds the budget of {} triangles and {} bytes",
            self.triangles, self.vertices, self.bytes, self.max_triangles, self.max_bytes
        )
    }
}
impl std::error::Error for BudgetError {}
impl From<BudgetError> for MeshError {
    fn from(e: BudgetError) -> MeshError {
        MeshError {
            kind: ErrorKind::BudgetExceeded,
            message: format!(
                "mesh: surface of {} triangles and {} vertices ({} bytes) exceeds the budget of {} triangles and {} bytes",
                e.triangles, e.vertices, e.bytes, e.max_triangles, e.max_bytes
            ),
            budget: Some(e),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub triangles: Vec<[u32; 3]>,
}
pub(crate) fn mesh_bytes(vertices: i64, triangles: i64, normals: bool) -> i64 {
    let per_vertex: i64 = if normals { 24 } else { 12 };
    vertices
        .wrapping_mul(per_vertex)
        .wrapping_add(triangles.wrapping_mul(12))
}
impl Mesh {
    pub fn bytes(&self) -> i64 {
        mesh_bytes(
            self.vertices.len() as i64,
            self.triangles.len() as i64,
            self.normals.is_some(),
        )
    }
    pub(crate) fn point(&self, i: u32) -> [f64; 3] {
        let v = self.vertices[i as usize];
        [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
    }
    pub fn validate(&self) -> Result<(), MeshError> {
        if let Some(normals) = &self.normals
            && normals.len() != self.vertices.len()
        {
            return Err(MeshError::other(format!(
                "mesh: {} normals for {} vertices",
                normals.len(),
                self.vertices.len()
            )));
        }
        for (i, v) in self.vertices.iter().enumerate() {
            if v.iter().any(|c| !c.is_finite()) {
                return Err(MeshError::other(format!("mesh: vertex {i} is not finite")));
            }
        }
        let n = self.vertices.len() as u32;
        for (i, t) in self.triangles.iter().enumerate() {
            if t[0] >= n || t[1] >= n || t[2] >= n {
                return Err(MeshError::other(format!(
                    "mesh: triangle {i} indexes a missing vertex"
                )));
            }
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                return Err(MeshError::other(format!(
                    "mesh: triangle {i} repeats a vertex"
                )));
            }
        }
        Ok(())
    }
    pub(crate) fn triangle_normal(&self, t: [u32; 3]) -> [f32; 3] {
        let (a, b, c) = (self.point(t[0]), self.point(t[1]), self.point(t[2]));
        unit32(cross3(sub3(b, a), sub3(c, a)))
    }
}
#[doc(hidden)]
pub fn validate_opt(m: Option<&Mesh>) -> Result<(), MeshError> {
    match m {
        None => Err(MeshError::invalid("nil mesh")),
        Some(m) => m.validate(),
    }
}
pub(crate) fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn unit32(v: [f64; 3]) -> [f32; 3] {
    let l = dot3(v, v).sqrt();
    if l == 0.0 || l.is_nan() || l.is_infinite() {
        return [0.0; 3];
    }
    [(v[0] / l) as f32, (v[1] / l) as f32, (v[2] / l) as f32]
}
pub(crate) fn face_normals(m: &mut Mesh, need: Option<&[bool]>) {
    let count = m.vertices.len();
    m.normals
        .get_or_insert_with(Vec::new)
        .resize(count, [0.0; 3]);
    let mut acc: Vec<[f64; 3]> = Vec::new();
    let mut index = vec![-1i32; m.vertices.len()];
    for (i, slot) in index.iter_mut().enumerate() {
        if need.is_none_or(|n| n[i]) {
            *slot = acc.len() as i32;
            acc.push([0.0; 3]);
        }
    }
    if acc.is_empty() {
        return;
    }
    for t in &m.triangles {
        let (a, b, c) = (
            m.vertices[t[0] as usize],
            m.vertices[t[1] as usize],
            m.vertices[t[2] as usize],
        );
        let u = [
            f64::from(b[0] - a[0]),
            f64::from(b[1] - a[1]),
            f64::from(b[2] - a[2]),
        ];
        let v = [
            f64::from(c[0] - a[0]),
            f64::from(c[1] - a[1]),
            f64::from(c[2] - a[2]),
        ];
        let n = cross3(u, v);
        for k in t {
            let j = index[*k as usize];
            if j >= 0 {
                acc[j as usize] = add3(acc[j as usize], n);
            }
        }
    }
    let Some(normals) = m.normals.as_mut() else {
        return;
    };
    for (i, j) in index.iter().enumerate() {
        if *j >= 0 {
            normals[i] = unit32(acc[*j as usize]);
        }
    }
}
pub(crate) fn dims_v(d: [i64; 3]) -> String {
    format!("[{} {} {}]", d[0], d[1], d[2])
}
fn shortest_f32_digits(x: f32) -> (Vec<u8>, i32) {
    let exact = f64::from(x);
    let reads_back = |digits: u64, exp: i32| -> bool {
        format!("{digits}e{exp}")
            .parse::<f32>()
            .is_ok_and(|v| v == x)
    };
    for nd in 1..=9usize {
        let text = format!("{:.*e}", nd - 1, exact);
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let digits: u64 = mantissa
            .bytes()
            .filter(u8::is_ascii_digit)
            .fold(0, |n, d| n * 10 + u64::from(d - b'0'));
        let exp: i32 = exponent.parse().unwrap_or(0);
        let scale = exp - (nd as i32 - 1);
        let mut best: Option<u64> = None;
        if reads_back(digits, scale) {
            best = Some(digits);
        } else {
            let distance = |d: u64| {
                format!("{d}e{scale}")
                    .parse::<f64>()
                    .map_or(f64::INFINITY, |v| (v - exact).abs())
            };
            for candidate in [digits.wrapping_sub(1), digits + 1] {
                let in_range = candidate >= 10u64.pow(nd as u32 - 1).max(1)
                    && candidate < 10u64.pow(nd as u32);
                if in_range
                    && reads_back(candidate, scale)
                    && best.is_none_or(|b| distance(candidate) < distance(b))
                {
                    best = Some(candidate);
                }
            }
        }
        if let Some(digits) = best {
            return (digits.to_string().into_bytes(), exp);
        }
    }
    let text = format!("{:.8e}", exact);
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    (
        mantissa.bytes().filter(u8::is_ascii_digit).collect(),
        exponent.parse().unwrap_or(0),
    )
}
pub(crate) fn format_f32(c: f32) -> String {
    if c.is_nan() {
        return "NaN".into();
    }
    if c.is_infinite() {
        return if c > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let mut out = String::new();
    if c.is_sign_negative() {
        out.push('-');
    }
    if c == 0.0 {
        out.push('0');
        return out;
    }
    let (digits, exp) = shortest_f32_digits(c.abs());
    let nd = digits.len() as i32;
    if !(-4..6).contains(&exp) {
        out.push(digits[0] as char);
        if nd > 1 {
            out.push('.');
            out.extend(digits[1..].iter().map(|d| *d as char));
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        let e = exp.unsigned_abs();
        if e < 10 {
            out.push('0');
        }
        out.push_str(&e.to_string());
    } else if exp < 0 {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.extend(digits.iter().map(|d| *d as char));
    } else {
        let int_len = (exp + 1) as usize;
        for i in 0..int_len {
            out.push(if i < digits.len() {
                digits[i] as char
            } else {
                '0'
            });
        }
        if digits.len() > int_len {
            out.push('.');
            out.extend(digits[int_len..].iter().map(|d| *d as char));
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn float32_text_like_go() {
        assert_eq!(format_f32(18.914_063), "18.914062");
        assert_eq!(format_f32(0.1), "0.1");
        assert_eq!(format_f32(-0.0), "-0");
        assert_eq!(format_f32(1e6), "1e+06");
        assert_eq!(format_f32(123456.0), "123456");
        assert_eq!(format_f32(1.5e-5), "1.5e-05");
        assert_eq!(format_f32(0.00012), "0.00012");
        assert_eq!(format_f32(f32::MAX), "3.4028235e+38");
        assert_eq!(format_f32(1e-45), "1e-45");
        assert_eq!(format_f32(16_777_216.0), "1.6777216e+07");
    }
}
