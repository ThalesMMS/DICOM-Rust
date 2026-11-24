use super::bvh::{TriangleBvh, scale3};
use super::curvature::edge_id;
use super::{Mesh, MeshError, add3, cross3, dot3, sub3};
use crate::fmath;
use std::collections::HashMap;
use std::sync::OnceLock;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SurfacePoint {
    pub point: [f64; 3],
    pub triangle: u32,
    pub barycentric: [f64; 3],
    pub distance: f64,
}
impl SurfacePoint {
    pub fn nearest_vertex(&self, m: &Mesh) -> u32 {
        let t = m.triangles[self.triangle as usize];
        let mut best = 0;
        for k in 1..3 {
            if self.barycentric[k] > self.barycentric[best] {
                best = k;
            }
        }
        t[best]
    }
}
pub fn vertex_point(m: &Mesh, v: u32) -> Option<SurfacePoint> {
    if v as usize >= m.vertices.len() {
        return None;
    }
    for (i, t) in m.triangles.iter().enumerate() {
        for k in 0..3 {
            if t[k] == v {
                let mut weights = [0.0; 3];
                weights[k] = 1.0;
                return Some(SurfacePoint {
                    point: m.point(v),
                    triangle: i as u32,
                    barycentric: weights,
                    distance: 0.0,
                });
            }
        }
    }
    None
}
struct PseudoNormals {
    vertex: Vec<[f64; 3]>,
    edge: HashMap<u64, [f64; 3]>,
}
pub struct Locator<'a> {
    m: &'a Mesh,
    tree: Option<TriangleBvh>,
    pseudo: OnceLock<PseudoNormals>,
}
impl<'a> Locator<'a> {
    pub fn new(m: &'a Mesh) -> Result<Locator<'a>, MeshError> {
        m.validate()?;
        Ok(Locator {
            m,
            tree: (!m.triangles.is_empty()).then(|| TriangleBvh::new(m)),
            pseudo: OnceLock::new(),
        })
    }
    pub fn mesh(&self) -> &'a Mesh {
        self.m
    }
    pub fn closest_point(&self, p: [f64; 3]) -> Option<SurfacePoint> {
        self.closest(p, f64::INFINITY)
    }
    fn closest(&self, p: [f64; 3], bound: f64) -> Option<SurfacePoint> {
        let tree = self.tree.as_ref()?;
        let m = self.m;
        let mut best2 = bound * bound;
        let mut out = SurfacePoint::default();
        let mut found = false;
        let mut stack: Vec<i32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(index) = stack.pop() {
            let node = &tree.nodes[index as usize];
            if box_distance2(p, node.lo, node.hi) >= best2 {
                continue;
            }
            if node.count == 0 {
                let (mut near, mut far) = (node.first, node.first + 1);
                let (f, n) = (&tree.nodes[far as usize], &tree.nodes[near as usize]);
                if box_distance2(p, f.lo, f.hi) < box_distance2(p, n.lo, n.hi) {
                    std::mem::swap(&mut near, &mut far);
                }
                stack.push(far);
                stack.push(near);
                continue;
            }
            for ti in &tree.order[node.first as usize..(node.first + node.count) as usize] {
                let t = m.triangles[*ti as usize];
                let (q, weights) =
                    closest_on_triangle(p, m.point(t[0]), m.point(t[1]), m.point(t[2]));
                let d = sub3(p, q);
                let d2 = dot3(d, d);
                if d2 < best2 || (!found && d2 <= best2) {
                    best2 = d2;
                    found = true;
                    out = SurfacePoint {
                        point: q,
                        triangle: *ti,
                        barycentric: weights,
                        distance: 0.0,
                    };
                }
            }
        }
        out.distance = best2.sqrt();
        found.then_some(out)
    }
    pub fn signed_distance(&self, p: [f64; 3]) -> Option<(f64, SurfacePoint)> {
        let s = self.closest_point(p)?;
        Some((self.sign(p, &s) * s.distance, s))
    }
    fn sign(&self, p: [f64; 3], s: &SurfacePoint) -> f64 {
        if s.distance == 0.0 {
            return 1.0;
        }
        let pseudo = self.pseudo.get_or_init(|| self.build_pseudo_normals());
        let m = self.m;
        let t = m.triangles[s.triangle as usize];
        let zero = [
            s.barycentric[0] <= 0.0,
            s.barycentric[1] <= 0.0,
            s.barycentric[2] <= 0.0,
        ];
        let zeros = zero.iter().filter(|z| **z).count();
        let mut normal = [0.0; 3];
        match zeros {
            2 => {
                for k in 0..3 {
                    if !zero[k] {
                        normal = pseudo.vertex[t[k] as usize];
                    }
                }
            }
            1 => {
                for k in 0..3 {
                    if zero[k] {
                        normal = pseudo
                            .edge
                            .get(&edge_id(t[(k + 1) % 3], t[(k + 2) % 3]))
                            .copied()
                            .unwrap_or_default();
                    }
                }
            }
            _ => {
                normal = cross3(
                    sub3(m.point(t[1]), m.point(t[0])),
                    sub3(m.point(t[2]), m.point(t[0])),
                );
            }
        }
        if dot3(sub3(p, s.point), normal) < 0.0 {
            -1.0
        } else {
            1.0
        }
    }
    fn build_pseudo_normals(&self) -> PseudoNormals {
        let m = self.m;
        let mut vertex = vec![[0.0f64; 3]; m.vertices.len()];
        let mut edge: HashMap<u64, [f64; 3]> = HashMap::with_capacity(3 * m.triangles.len() / 2);
        for t in &m.triangles {
            let p = [m.point(t[0]), m.point(t[1]), m.point(t[2])];
            let n = cross3(sub3(p[1], p[0]), sub3(p[2], p[0]));
            let length = dot3(n, n).sqrt();
            if length == 0.0 {
                continue;
            }
            let n = scale3(n, 1.0 / length);
            for k in 0..3 {
                let (a, b) = (sub3(p[(k + 1) % 3], p[k]), sub3(p[(k + 2) % 3], p[k]));
                let c = cross3(a, b);
                let angle = fmath::atan2(dot3(c, c).sqrt(), dot3(a, b));
                let v = t[k] as usize;
                vertex[v] = add3(vertex[v], scale3(n, angle));
                let slot = edge.entry(edge_id(t[k], t[(k + 1) % 3])).or_default();
                *slot = add3(*slot, n);
            }
        }
        PseudoNormals { vertex, edge }
    }
    pub fn ray_hit(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        max_t: f64,
    ) -> Option<SurfacePoint> {
        let tree = self.tree.as_ref()?;
        let length = dot3(direction, direction).sqrt();
        if length.is_nan() || length <= 0.0 || length.is_infinite() {
            return None;
        }
        let d = scale3(direction, 1.0 / length);
        let max_t = if max_t > 0.0 { max_t } else { f64::INFINITY };
        let m = self.m;
        let (t, ti) = tree.closest_hit(m, origin, d, max_t, u32::MAX, 1e-9)?;
        let tri = m.triangles[ti as usize];
        let q = add3(origin, scale3(d, t));
        let (_, weights) =
            closest_on_triangle(q, m.point(tri[0]), m.point(tri[1]), m.point(tri[2]));
        Some(SurfacePoint {
            point: q,
            triangle: ti,
            barycentric: weights,
            distance: t,
        })
    }
    pub fn vertex_distances(&self, m: &Mesh, signed: bool) -> Result<Vec<f64>, MeshError> {
        m.validate()?;
        if self.tree.is_none() {
            return Err(MeshError::invalid(
                "surface distance to a mesh without triangles",
            ));
        }
        let mut out = vec![0.0; m.vertices.len()];
        let mut bound = f64::INFINITY;
        for (v, slot) in out.iter_mut().enumerate() {
            let p = m.point(v as u32);
            if v % OCCLUSION_CHUNK == 0 {
                bound = f64::INFINITY;
            } else if bound != f64::INFINITY {
                let step = sub3(p, m.point(v as u32 - 1));
                bound += dot3(step, step).sqrt();
            }
            let s = self
                .closest(p, bound * (1.0 + 1e-12) + 1e-9)
                .or_else(|| self.closest(p, f64::INFINITY))
                .unwrap_or_default();
            bound = s.distance;
            *slot = if signed {
                self.sign(p, &s) * s.distance
            } else {
                s.distance
            };
        }
        Ok(out)
    }
}
pub(crate) const OCCLUSION_CHUNK: usize = 256;
fn box_distance2(p: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> f64 {
    let mut d2 = 0.0;
    for k in 0..3 {
        if p[k] < lo[k] {
            d2 += (lo[k] - p[k]) * (lo[k] - p[k]);
        } else if p[k] > hi[k] {
            d2 += (p[k] - hi[k]) * (p[k] - hi[k]);
        }
    }
    d2
}
pub(crate) fn closest_on_triangle(
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
) -> ([f64; 3], [f64; 3]) {
    let (ab, ac, ap) = (sub3(b, a), sub3(c, a), sub3(p, a));
    let (d1, d2) = (dot3(ab, ap), dot3(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let bp = sub3(p, b);
    let (d3, d4) = (dot3(ab, bp), dot3(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }
    let cp = sub3(p, c);
    let (d5, d6) = (dot3(ab, cp), dot3(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (add3(a, scale3(ab, v)), [1.0 - v, v, 0.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (add3(a, scale3(ac, w)), [1.0 - w, 0.0, w]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (add3(b, scale3(sub3(c, b), w)), [0.0, 1.0 - w, w]);
    }
    let sum = va + vb + vc;
    if sum.is_nan() || sum <= 0.0 {
        return closest_on_degenerate(p, a, b, c);
    }
    let (v, w) = (vb / sum, vc / sum);
    (
        add3(a, add3(scale3(ab, v), scale3(ac, w))),
        [1.0 - v - w, v, w],
    )
}
fn closest_on_degenerate(
    p: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
) -> ([f64; 3], [f64; 3]) {
    let corners = [a, b, c];
    let mut best = [0.0; 3];
    let mut weights = [0.0; 3];
    let mut best2 = f64::INFINITY;
    for k in 0..3 {
        let (u, v) = (corners[k], corners[(k + 1) % 3]);
        let e = sub3(v, u);
        let mut s = 0.0;
        let l2 = dot3(e, e);
        if l2 > 0.0 {
            s = fmath::max(0.0, fmath::min(1.0, dot3(sub3(p, u), e) / l2));
        }
        let q = add3(u, scale3(e, s));
        let d = sub3(p, q);
        if dot3(d, d) < best2 {
            best2 = dot3(d, d);
            best = q;
            weights = [0.0; 3];
            weights[k] = 1.0 - s;
            weights[(k + 1) % 3] = s;
        }
    }
    (best, weights)
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DistanceSummary {
    pub mean: f64,
    pub rms: f64,
    pub max: f64,
    pub percentile95: f64,
    pub vertices: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceDistanceReport {
    pub a_to_b: Vec<f64>,
    pub b_to_a: Vec<f64>,
    pub a_to_b_summary: DistanceSummary,
    pub b_to_a_summary: DistanceSummary,
    pub hausdorff: f64,
    pub mean_surface_distance: f64,
}
pub fn surface_distances(
    a: &Mesh,
    b: &Mesh,
    signed: bool,
) -> Result<SurfaceDistanceReport, MeshError> {
    for m in [a, b] {
        m.validate()?;
        if m.triangles.is_empty() {
            return Err(MeshError::invalid(
                "surface distance of a mesh without triangles",
            ));
        }
    }
    let la = Locator::new(a)?;
    let lb = Locator::new(b)?;
    let mut report = SurfaceDistanceReport {
        a_to_b: lb.vertex_distances(a, signed)?,
        b_to_a: la.vertex_distances(b, signed)?,
        ..SurfaceDistanceReport::default()
    };
    let (wa, wb) = (vertex_area_weights(a), vertex_area_weights(b));
    report.a_to_b_summary = summarize_distances(&report.a_to_b, &wa);
    report.b_to_a_summary = summarize_distances(&report.b_to_a, &wb);
    report.hausdorff = fmath::max(report.a_to_b_summary.max, report.b_to_a_summary.max);
    let (mut sum, mut weight) = (0.0, 0.0);
    for (d, w) in report.a_to_b.iter().zip(&wa) {
        sum += w * d.abs();
        weight += w;
    }
    for (d, w) in report.b_to_a.iter().zip(&wb) {
        sum += w * d.abs();
        weight += w;
    }
    if weight > 0.0 {
        report.mean_surface_distance = sum / weight;
    }
    Ok(report)
}
fn vertex_area_weights(m: &Mesh) -> Vec<f64> {
    let mut w = vec![0.0; m.vertices.len()];
    let mut total = 0.0;
    for t in &m.triangles {
        let (a, b, c) = (m.point(t[0]), m.point(t[1]), m.point(t[2]));
        let n = cross3(sub3(b, a), sub3(c, a));
        let third = dot3(n, n).sqrt() / 6.0;
        for v in t {
            w[*v as usize] += third;
        }
        total += 3.0 * third;
    }
    if total.is_nan() || total <= 0.0 {
        w.fill(1.0);
    }
    w
}
fn summarize_distances(distances: &[f64], weights: &[f64]) -> DistanceSummary {
    let mut s = DistanceSummary {
        vertices: distances.len() as i64,
        ..DistanceSummary::default()
    };
    if distances.is_empty() {
        return s;
    }
    let mut samples: Vec<(f64, f64)> = Vec::with_capacity(distances.len());
    let (mut sum, mut sum2, mut total) = (0.0, 0.0, 0.0);
    for (d, w) in distances.iter().zip(weights) {
        let d = d.abs();
        samples.push((d, *w));
        sum += w * d;
        sum2 += w * d * d;
        total += w;
        s.max = fmath::max(s.max, d);
    }
    if total > 0.0 {
        s.mean = sum / total;
        s.rms = (sum2 / total).sqrt();
    }
    crate::roi::pdqsort_by::sort_slice(&mut samples, &|x, y| x.0 < y.0);
    let mut acc = 0.0;
    for (d, w) in samples {
        acc += w;
        s.percentile95 = d;
        if acc >= 0.95 * total {
            break;
        }
    }
    s
}
