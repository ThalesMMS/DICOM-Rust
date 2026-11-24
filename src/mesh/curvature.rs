use super::extract::DEFAULT_MAX_BYTES;
use super::{ErrorKind, Mesh, MeshError, cross3, dot3, sub3};
use crate::fmath;
pub const DEFAULT_CURVATURE_MAX_VERTICES: i64 = 2_000_000;
pub const DEFAULT_CURVATURE_MAX_TRIANGLES: i64 = 4_000_000;
const CURVATURE_RELATIVE_EPSILON: f64 = 64.0 * f64::EPSILON;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CurvatureOptions {
    pub max_vertices: i64,
    pub max_triangles: i64,
    pub max_bytes: i64,
}
pub const CURVATURE_BOUNDARY: u8 = 1;
pub const CURVATURE_UNUSED: u8 = 2;
pub const CURVATURE_PRINCIPAL_CLAMPED: u8 = 4;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CurvatureResult {
    pub mean: Vec<f64>,
    pub gaussian: Vec<f64>,
    pub principal_min: Vec<f64>,
    pub principal_max: Vec<f64>,
    pub mixed_area: Vec<f64>,
    pub flags: Vec<u8>,
}
fn budget_exceeded(detail: &str) -> MeshError {
    MeshError {
        kind: ErrorKind::BudgetExceeded,
        message: format!("mesh: surface exceeds budget: {detail}"),
        budget: None,
    }
}
pub fn vertex_curvatures(m: &Mesh, opts: &CurvatureOptions) -> Result<CurvatureResult, MeshError> {
    let n = m.vertices.len();
    curvature_budget(n as i64, m.triangles.len() as i64, opts)?;
    validate_curvature_input(m)?;
    let mut result = CurvatureResult::default();
    if n == 0 {
        return Ok(result);
    }
    result.mean = vec![0.0; n];
    result.gaussian = vec![0.0; n];
    result.principal_min = vec![0.0; n];
    result.principal_max = vec![0.0; n];
    result.mixed_area = vec![0.0; n];
    result.flags = vec![0; n];
    curvature_topology(m, &mut result.flags)?;
    let mut normals = vec![[0.0f64; 3]; n];
    let mut laplacian = vec![[0.0f64; 3]; n];
    let mut angles = vec![0.0f64; n];
    let mut normal_scale = vec![0.0f64; n];
    for (f, face) in m.triangles.iter().enumerate() {
        let p = [m.point(face[0]), m.point(face[1]), m.point(face[2])];
        let d = [sub3(p[1], p[0]), sub3(p[2], p[1]), sub3(p[0], p[2])];
        let length2 = [dot3(d[0], d[0]), dot3(d[1], d[1]), dot3(d[2], d[2])];
        let cross = cross3(d[0], sub3(p[2], p[0]));
        let twice_area = dot3(cross, cross).sqrt();
        let shortest = length2[0].min(length2[1]).min(length2[2]);
        let longest = length2[0].max(length2[1]).max(length2[2]);
        if shortest == 0.0 || twice_area <= CURVATURE_RELATIVE_EPSILON * longest {
            return Err(MeshError::invalid(format!(
                "singular curvature triangle {f}"
            )));
        }
        let mut cot = [0.0f64; 3];
        let mut obtuse: Option<usize> = None;
        for (i, v) in face.iter().enumerate() {
            let v = *v as usize;
            let (a, b) = (sub3(p[(i + 1) % 3], p[i]), sub3(p[(i + 2) % 3], p[i]));
            let dot = dot3(a, b);
            cot[i] = dot / twice_area;
            if dot < 0.0 {
                obtuse = Some(i);
            }
            angles[v] += fmath::atan2(twice_area, dot);
            normal_scale[v] += twice_area;
            for k in 0..3 {
                normals[v][k] += cross[k];
            }
        }
        for (i, v) in face.iter().enumerate() {
            let v = *v as usize;
            match obtuse {
                Some(corner) => {
                    let fraction = if i == corner { 0.25 } else { 0.125 };
                    result.mixed_area[v] += fraction * twice_area;
                }
                None => {
                    result.mixed_area[v] += (cot[(i + 1) % 3] * length2[(i + 2) % 3]
                        + cot[(i + 2) % 3] * length2[i])
                        / 8.0;
                }
            }
            let j = (i + 1) % 3;
            let weight = cot[(i + 2) % 3];
            let other = face[j] as usize;
            for k in 0..3 {
                let value = weight * (p[i][k] - p[j][k]);
                laplacian[v][k] += value;
                laplacian[other][k] -= value;
            }
        }
    }
    for i in 0..n {
        if result.flags[i] & (CURVATURE_BOUNDARY | CURVATURE_UNUSED) != 0 {
            continue;
        }
        let area = result.mixed_area[i];
        let normal_length = dot3(normals[i], normals[i]).sqrt();
        if area <= 0.0 || normal_length <= CURVATURE_RELATIVE_EPSILON * normal_scale[i] {
            return Err(MeshError::invalid(format!(
                "singular curvature area/normal at vertex {i}"
            )));
        }
        let h = dot3(laplacian[i], normals[i]) / (4.0 * area * normal_length);
        let g = (std::f64::consts::TAU - angles[i]) / area;
        if !h.is_finite() || !g.is_finite() || !area.is_finite() {
            return Err(MeshError::invalid(format!(
                "non-finite curvature at vertex {i}"
            )));
        }
        let mut delta = h * h - g;
        if delta < 0.0 {
            delta = 0.0;
            result.flags[i] |= CURVATURE_PRINCIPAL_CLAMPED;
        }
        let root = delta.sqrt();
        result.mean[i] = h;
        result.gaussian[i] = g;
        result.principal_min[i] = h - root;
        result.principal_max[i] = h + root;
    }
    Ok(result)
}
pub(crate) fn validate_curvature_input(m: &Mesh) -> Result<(), MeshError> {
    if let Some(normals) = &m.normals
        && normals.len() != m.vertices.len()
    {
        return Err(MeshError::invalid(format!(
            "{} normals for {} vertices",
            normals.len(),
            m.vertices.len()
        )));
    }
    for (i, v) in m.vertices.iter().enumerate() {
        if v.iter().any(|c| !c.is_finite()) {
            return Err(MeshError::invalid(format!(
                "curvature vertex {i} is not finite"
            )));
        }
    }
    let n = m.vertices.len() as u32;
    for (i, face) in m.triangles.iter().enumerate() {
        if face[0] >= n
            || face[1] >= n
            || face[2] >= n
            || face[0] == face[1]
            || face[1] == face[2]
            || face[0] == face[2]
        {
            return Err(MeshError::invalid(format!(
                "curvature triangle {i} has missing/repeated indices"
            )));
        }
    }
    Ok(())
}
fn curvature_budget(n: i64, t: i64, opts: &CurvatureOptions) -> Result<(), MeshError> {
    if opts.max_vertices < 0 || opts.max_triangles < 0 || opts.max_bytes < 0 {
        return Err(MeshError::invalid("negative curvature budget"));
    }
    let pick = |v: i64, d: i64| if v == 0 { d } else { v };
    let max_vertices = pick(opts.max_vertices, DEFAULT_CURVATURE_MAX_VERTICES);
    let max_triangles = pick(opts.max_triangles, DEFAULT_CURVATURE_MAX_TRIANGLES);
    let max_bytes = pick(opts.max_bytes, DEFAULT_MAX_BYTES);
    let mut bytes = 0;
    if n != 0 || t != 0 {
        let max_u32 = i64::from(u32::MAX);
        if n >= max_u32
            || t > (max_u32 - 1) / 3
            || n > (i64::MAX - 8192) / 128
            || t > (i64::MAX - 8192 - 128 * n) / 128
        {
            return Err(budget_exceeded("curvature index/count capacity"));
        }
        bytes = 128 * n + 128 * t + 8192;
    }
    if n > max_vertices || t > max_triangles || bytes > max_bytes {
        return Err(MeshError {
            kind: ErrorKind::BudgetExceeded,
            message: format!(
                "mesh: curvature of {n} vertices and {t} triangles reserves {bytes} bytes; limits {max_vertices} vertices, {max_triangles} triangles, {max_bytes} bytes"
            ),
            budget: None,
        });
    }
    Ok(())
}
pub(crate) fn edge_id(a: u32, b: u32) -> u64 {
    let (a, b) = if a > b { (b, a) } else { (a, b) };
    u64::from(a) << 32 | u64::from(b)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CurvatureEdge {
    pub key: u64,
    pub corner: u32,
}
pub(crate) fn sorted_corner_edges(triangles: &[[u32; 3]]) -> Vec<CurvatureEdge> {
    let mut edges = Vec::with_capacity(3 * triangles.len());
    for (f, face) in triangles.iter().enumerate() {
        for i in 0..3 {
            edges.push(CurvatureEdge {
                key: edge_id(face[i], face[(i + 1) % 3]),
                corner: (3 * f + i) as u32,
            });
        }
    }
    edges.sort_unstable();
    edges
}
pub(crate) fn curvature_topology(m: &Mesh, flags: &mut [u8]) -> Result<(), MeshError> {
    let count = 3 * m.triangles.len();
    let mut parent: Vec<u32> = (0..count as u32).collect();
    let mut rank = vec![0u8; count];
    let mut first = vec![u32::MAX; m.vertices.len()];
    let ordered = sorted_corner_edges(&m.triangles);
    fn find(parent: &mut [u32], mut v: u32) -> u32 {
        while parent[v as usize] != v {
            parent[v as usize] = parent[parent[v as usize] as usize];
            v = parent[v as usize];
        }
        v
    }
    let mut start = 0;
    while start < count {
        let mut end = start + 1;
        while end < count && ordered[end].key == ordered[start].key {
            end += 1;
            if end - start > 2 {
                break;
            }
        }
        let (lo, hi) = ((ordered[start].key >> 32) as u32, ordered[start].key as u32);
        match end - start {
            1 => {
                flags[lo as usize] |= CURVATURE_BOUNDARY;
                flags[hi as usize] |= CURVATURE_BOUNDARY;
            }
            2 => {
                let (a, b) = (ordered[start], ordered[start + 1]);
                let (af, ai) = ((a.corner / 3) as usize, (a.corner % 3) as usize);
                let (bf, bi) = ((b.corner / 3) as usize, (b.corner % 3) as usize);
                let (at, bt) = (m.triangles[af], m.triangles[bf]);
                if at[(ai + 2) % 3] == bt[(bi + 2) % 3] {
                    return Err(MeshError::invalid(format!(
                        "duplicate curvature faces {af}/{bf}"
                    )));
                }
                if (at[ai] == lo) == (bt[bi] == lo) {
                    return Err(MeshError::invalid(format!(
                        "inconsistent curvature edge ({lo},{hi})"
                    )));
                }
                for vertex in [lo, hi] {
                    let (mut ac, mut bc) = (a.corner, b.corner);
                    if at[ai] != vertex {
                        ac = (3 * af + (ai + 1) % 3) as u32;
                    }
                    if bt[bi] != vertex {
                        bc = (3 * bf + (bi + 1) % 3) as u32;
                    }
                    let (mut x, mut y) = (find(&mut parent, ac), find(&mut parent, bc));
                    if x == y {
                        continue;
                    }
                    if rank[x as usize] < rank[y as usize] {
                        std::mem::swap(&mut x, &mut y);
                    }
                    parent[y as usize] = x;
                    if rank[x as usize] == rank[y as usize] {
                        rank[x as usize] += 1;
                    }
                }
            }
            _ => {
                return Err(MeshError::invalid(format!(
                    "non-manifold curvature edge ({lo},{hi})"
                )));
            }
        }
        start = end;
    }
    for (f, face) in m.triangles.iter().enumerate() {
        for (i, vertex) in face.iter().enumerate() {
            let root = find(&mut parent, (3 * f + i) as u32);
            let slot = &mut first[*vertex as usize];
            if *slot == u32::MAX {
                *slot = root;
            } else if *slot != root {
                return Err(MeshError::invalid(format!(
                    "disconnected curvature face fan at vertex {vertex}"
                )));
            }
        }
    }
    for (i, corner) in first.iter().enumerate() {
        if *corner == u32::MAX {
            flags[i] |= CURVATURE_UNUSED;
        }
    }
    Ok(())
}
