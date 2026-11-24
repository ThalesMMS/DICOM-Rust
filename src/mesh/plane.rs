use super::attributes::{VertexScalarAttribute, validate_vertex_scalars};
use super::io::{PlyOptions, write_ply};
use super::quality::{QualityOptions, analyze_quality};
use super::{ErrorKind, Mesh, MeshError, cross3, dot3, sub3};
use crate::fmath;
use crate::textfmt::float_v;
use std::collections::{HashMap, HashSet};
use std::path::Path;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Plane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaneOptions {
    pub tolerance_mm: f64,
    pub keep_positive: bool,
    pub max_vertices: i64,
    pub max_triangles: i64,
    pub max_result_vertices: i64,
    pub max_result_triangles: i64,
    pub max_bytes: i64,
    pub max_work: i64,
    pub vertex_scalars: Vec<VertexScalarAttribute>,
    pub face_colors: Option<Vec<[u8; 3]>>,
    pub cap_color: [u8; 3],
    pub source_reference: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionPolyline {
    pub point_ids: Vec<u32>,
    pub closed: bool,
    pub hole: bool,
    pub depth: i64,
    pub parent: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaneSection {
    pub plane: Plane,
    pub tolerance_mm: f64,
    pub points: Vec<[f64; 3]>,
    pub polylines: Vec<SectionPolyline>,
    pub vertex_scalars: Vec<VertexScalarAttribute>,
    pub tangent_vertices: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaneClipResult {
    pub mesh: Mesh,
    pub section: PlaneSection,
    pub source_triangles: Vec<i64>,
    pub vertex_scalars: Vec<VertexScalarAttribute>,
    pub face_colors: Option<Vec<[u8; 3]>>,
    pub plane: Plane,
    pub keep_positive: bool,
    pub source_reference: String,
    pub reserved_bytes: i64,
    pub work: i64,
}
pub(crate) fn plane_error(detail: impl AsRef<str>) -> MeshError {
    MeshError {
        kind: ErrorKind::PlaneGeometry,
        message: format!("mesh: plane geometry: {}", detail.as_ref()),
        budget: None,
    }
}
pub(crate) fn plane_limit(resource: &str, used: i64, limit: i64) -> MeshError {
    MeshError {
        kind: ErrorKind::BudgetExceeded,
        message: format!("mesh quality: {resource} limit ({used} > {limit})"),
        budget: None,
    }
}
type PlaneKey = [u32; 2];
fn plane_edge(a: u32, b: u32) -> PlaneKey {
    if a > b { [b, a] } else { [a, b] }
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PlaneVertex {
    pub p: [f64; 3],
    pub a: u32,
    pub b: u32,
    pub w: f64,
}
pub(crate) struct PlaneRun<'a> {
    pub accept_triangle: Option<&'a dyn Fn([u32; 3]) -> bool>,
    pub m: &'a Mesh,
    pub o: PlaneOptions,
    pub plane: Plane,
    pub vertices: Vec<PlaneVertex>,
    pub originals: HashMap<u32, u32>,
    pub cuts: HashMap<PlaneKey, u32>,
    pub dist: Vec<f64>,
    pub side: Vec<i32>,
    pub work: i64,
    pub reserved: i64,
}
fn vec_finite(p: [f64; 3]) -> bool {
    p.iter().all(|v| v.is_finite())
}
pub(crate) fn plane_options(mut o: PlaneOptions) -> Result<PlaneOptions, MeshError> {
    if !o.tolerance_mm.is_finite() || o.tolerance_mm < 0.0 {
        return Err(plane_error("tolerance"));
    }
    if o.tolerance_mm == 0.0 {
        o.tolerance_mm = 1e-6;
    }
    let defaults = [
        1_000_000,
        2_000_000,
        2_000_000,
        4_000_000,
        512 << 20,
        20_000_000,
    ];
    let limits = [
        &mut o.max_vertices,
        &mut o.max_triangles,
        &mut o.max_result_vertices,
        &mut o.max_result_triangles,
        &mut o.max_bytes,
        &mut o.max_work,
    ];
    for (p, d) in limits.into_iter().zip(defaults) {
        if *p < 0 {
            return Err(plane_error("negative limit"));
        }
        if *p == 0 {
            *p = d;
        }
    }
    if o.source_reference.len() > 4096 {
        return Err(plane_error("source reference length"));
    }
    Ok(o)
}
pub(crate) fn plane_reserve(n: i64, t: i64, attributes: usize) -> Result<i64, MeshError> {
    if t > (i64::MAX - n) / 4 {
        return Err(plane_limit("plane bytes", i64::MAX, 0));
    }
    let count = n + 4 * t;
    let per = 1536 + 64 * attributes as i64;
    if count > (i64::MAX - 32768) / per {
        return Err(plane_limit("plane bytes", i64::MAX, 0));
    }
    Ok(count * per + 32768)
}
#[derive(Clone, Copy, Default)]
struct PlaneXy {
    x: f64,
    y: f64,
}
fn plane_basis(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let a = if n[0].abs() > 0.8 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let mut u = cross3(a, n);
    let l = dot3(u, u).sqrt();
    for c in u.iter_mut() {
        *c /= l;
    }
    (u, cross3(n, u))
}
fn plane_project(points: &[[f64; 3]], n: [f64; 3]) -> Vec<PlaneXy> {
    let (u, v) = plane_basis(n);
    let Some(origin) = points.first() else {
        return Vec::new();
    };
    points
        .iter()
        .map(|p| {
            let d = sub3(*p, *origin);
            PlaneXy {
                x: dot3(d, u),
                y: dot3(d, v),
            }
        })
        .collect()
}
fn plane_orient(a: PlaneXy, b: PlaneXy, c: PlaneXy) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn plane_area(p: &[PlaneXy], ids: &[u32]) -> f64 {
    let mut a = 0.0;
    if ids.is_empty() {
        return a;
    }
    let origin = p[ids[0] as usize];
    for i in 0..ids.len() {
        a += plane_orient(
            origin,
            p[ids[i] as usize],
            p[ids[(i + 1) % ids.len()] as usize],
        );
    }
    a / 2.0
}
fn cap_ear_nondegenerate(a: PlaneXy, b: PlaneXy, c: PlaneXy, tol: f64) -> bool {
    plane_orient(a, b, c) > tol * fmath::hypot(c.x - a.x, c.y - a.y)
}
fn point_in_cap_ear(a: PlaneXy, b: PlaneXy, c: PlaneXy, p: PlaneXy, tol: f64) -> bool {
    plane_orient(a, b, p) >= -tol * fmath::hypot(b.x - a.x, b.y - a.y)
        && plane_orient(b, c, p) >= -tol * fmath::hypot(c.x - b.x, c.y - b.y)
        && plane_orient(c, a, p) >= -tol * fmath::hypot(a.x - c.x, a.y - c.y)
}
fn plane_same(a: PlaneXy, b: PlaneXy) -> bool {
    a.x == b.x && a.y == b.y
}
fn plane_on_segment(p: PlaneXy, a: PlaneXy, b: PlaneXy, tol: f64) -> bool {
    let l = fmath::hypot(b.x - a.x, b.y - a.y);
    if l == 0.0 {
        return fmath::hypot(p.x - a.x, p.y - a.y) <= tol;
    }
    plane_orient(a, b, p).abs() <= tol * l
        && p.x >= fmath::min(a.x, b.x) - tol
        && p.x <= fmath::max(a.x, b.x) + tol
        && p.y >= fmath::min(a.y, b.y) - tol
        && p.y <= fmath::max(a.y, b.y) + tol
}
fn plane_segments_meet(a: PlaneXy, b: PlaneXy, c: PlaneXy, d: PlaneXy, tol: f64) -> bool {
    if plane_on_segment(a, c, d, tol)
        || plane_on_segment(b, c, d, tol)
        || plane_on_segment(c, a, b, tol)
        || plane_on_segment(d, a, b, tol)
    {
        return true;
    }
    let (x, y, z, w) = (
        plane_orient(a, b, c),
        plane_orient(a, b, d),
        plane_orient(c, d, a),
        plane_orient(c, d, b),
    );
    (x < 0.0) != (y < 0.0) && (z < 0.0) != (w < 0.0)
}
fn plane_inside(p: PlaneXy, xy: &[PlaneXy], ids: &[u32]) -> bool {
    let mut inside = false;
    for (i, a) in ids.iter().enumerate() {
        let b = ids[(i + 1) % ids.len()];
        let (u, v) = (xy[*a as usize], xy[b as usize]);
        if (u.y > p.y) != (v.y > p.y) && p.x < (v.x - u.x) * (p.y - u.y) / (v.y - u.y) + u.x {
            inside = !inside;
        }
    }
    inside
}
fn plane_domain(p: PlaneXy, xy: &[PlaneXy], loops: &[SectionPolyline], outer: usize) -> bool {
    if !plane_inside(p, xy, &loops[outer].point_ids) {
        return false;
    }
    !loops
        .iter()
        .any(|l| l.parent == outer as i64 && l.hole && plane_inside(p, xy, &l.point_ids))
}
impl<'a> PlaneRun<'a> {
    pub fn step(&mut self) -> Result<(), MeshError> {
        if self.work >= self.o.max_work {
            return Err(plane_limit("plane work", self.work + 1, self.o.max_work));
        }
        self.work += 1;
        Ok(())
    }
    pub fn bare(m: &'a Mesh, vertices: Vec<PlaneVertex>, plane: Plane, o: PlaneOptions) -> Self {
        PlaneRun {
            accept_triangle: None,
            m,
            o,
            plane,
            vertices,
            originals: HashMap::new(),
            cuts: HashMap::new(),
            dist: Vec::new(),
            side: Vec::new(),
            work: 0,
            reserved: 0,
        }
    }
    pub fn new(m: Option<&'a Mesh>, mut p: Plane, o: &PlaneOptions) -> Result<Self, MeshError> {
        let o = plane_options(o.clone())?;
        let Some(m) = m.filter(|_| vec_finite(p.origin) && vec_finite(p.normal)) else {
            return Err(plane_error("nil mesh/nonfinite plane"));
        };
        let length = fmath::hypot(fmath::hypot(p.normal[0], p.normal[1]), p.normal[2]);
        if length.is_nan() || length <= 0.0 || !length.is_finite() {
            return Err(plane_error("plane normal"));
        }
        for c in p.normal.iter_mut() {
            *c /= length;
        }
        let (n, t) = (m.vertices.len() as i64, m.triangles.len() as i64);
        if n > o.max_vertices {
            return Err(plane_limit("plane input vertices", n, o.max_vertices));
        }
        if t > o.max_triangles {
            return Err(plane_limit("plane input triangles", t, o.max_triangles));
        }
        if n > i64::from(u32::MAX) || t > i64::from(u32::MAX) {
            return Err(plane_error("index capacity"));
        }
        let reserved = plane_reserve(n, t, o.vertex_scalars.len())?;
        if reserved > o.max_bytes {
            return Err(plane_limit("plane bytes", reserved, o.max_bytes));
        }
        m.validate()?;
        validate_vertex_scalars(&o.vertex_scalars, m.vertices.len(), false)?;
        if let Some(colors) = &o.face_colors
            && colors.len() != m.triangles.len()
        {
            return Err(plane_error("face colors"));
        }
        let tolerance = o.tolerance_mm;
        let (max_vertices, max_triangles) = (o.max_vertices, o.max_triangles);
        let mut r = PlaneRun {
            accept_triangle: None,
            m,
            o,
            plane: p,
            vertices: Vec::new(),
            originals: HashMap::new(),
            cuts: HashMap::new(),
            dist: vec![0.0; m.vertices.len()],
            side: vec![0; m.vertices.len()],
            work: 0,
            reserved,
        };
        r.validate_solid(m, max_vertices, max_triangles)?;
        for i in 0..m.vertices.len() {
            r.step()?;
            let d = dot3(sub3(m.point(i as u32), p.origin), p.normal);
            if !d.is_finite() {
                return Err(plane_error("distance overflow"));
            }
            r.dist[i] = d;
            if d > tolerance {
                r.side[i] = 1;
            } else if d < -tolerance {
                r.side[i] = -1;
            }
        }
        Ok(r)
    }
    pub(crate) fn validate_solid(
        &mut self,
        m: &Mesh,
        max_vertices: i64,
        max_triangles: i64,
    ) -> Result<(), MeshError> {
        if self.work >= self.o.max_work {
            return Err(plane_limit("plane work", self.work + 1, self.o.max_work));
        }
        let remaining = self.o.max_work - self.work;
        let (q, result) = analyze_quality(
            Some(m),
            QualityOptions {
                max_vertices,
                max_triangles,
                max_bytes: self.o.max_bytes,
                max_work: remaining,
                max_candidates: remaining,
                max_results: 1,
                ..QualityOptions::default()
            },
        );
        self.work += q.work;
        result?;
        if !q.complete
            || !q.intersections_conclusive
            || q.degenerate.count != 0
            || q.duplicate_faces.count != 0
            || q.crossings + q.coplanar_overlaps + q.contacts + q.uncertain_pairs != 0
        {
            return Err(plane_error("degenerate/intersecting/inconclusive solid"));
        }
        self.step()?;
        if !m.check_closure().closed() {
            return Err(plane_error("open/nonmanifold/inconsistently wound solid"));
        }
        self.validate_vertex_links(m)
    }
    pub(crate) fn validate_vertex_links(&mut self, m: &Mesh) -> Result<(), MeshError> {
        let mut links: Vec<Vec<PlaneKey>> = vec![Vec::new(); m.vertices.len()];
        for t in &m.triangles {
            for (j, v) in t.iter().enumerate() {
                self.step()?;
                links[*v as usize].push([t[(j + 1) % 3], t[(j + 2) % 3]]);
            }
        }
        for edges in links.iter().filter(|e| !e.is_empty()) {
            let mut nodes: HashMap<u32, ([u32; 2], usize)> = HashMap::new();
            for e in edges {
                for (j, v) in e.iter().enumerate() {
                    self.step()?;
                    let n = nodes.entry(*v).or_insert(([0; 2], 0));
                    if n.1 == 2 {
                        return Err(plane_error("branching vertex link"));
                    }
                    n.0[n.1] = e[1 - j];
                    n.1 += 1;
                }
            }
            let mut start = u32::MAX;
            for (v, n) in &nodes {
                self.step()?;
                if n.1 != 2 {
                    return Err(plane_error("open vertex link"));
                }
                start = start.min(*v);
            }
            let mut seen: HashSet<u32> = HashSet::new();
            let (mut current, mut previous) = (start, start);
            loop {
                self.step()?;
                if !seen.insert(current) {
                    break;
                }
                let neighbors = nodes.get(&current).map_or([0; 2], |n| n.0);
                let mut next = neighbors[0];
                if next == previous {
                    next = neighbors[1];
                }
                previous = current;
                current = next;
            }
            if current != start || seen.len() != nodes.len() {
                return Err(plane_error("disconnected vertex link"));
            }
        }
        Ok(())
    }
    pub fn vertex(&mut self, v: PlaneVertex) -> Result<u32, MeshError> {
        let count = self.vertices.len() as i64;
        if count >= self.o.max_result_vertices {
            return Err(plane_limit(
                "plane result vertices",
                count + 1,
                self.o.max_result_vertices,
            ));
        }
        if count >= i64::from(u32::MAX) {
            return Err(plane_error("result index capacity"));
        }
        self.vertices.push(v);
        Ok(count as u32)
    }
    pub fn original(&mut self, a: u32) -> Result<u32, MeshError> {
        if let Some(id) = self.originals.get(&a) {
            return Ok(*id);
        }
        let mut p = self.m.point(a);
        if self.side[a as usize] == 0 {
            for k in 0..3 {
                p[k] -= self.dist[a as usize] * self.plane.normal[k];
            }
        }
        let id = self.vertex(PlaneVertex { p, a, b: a, w: 0.0 })?;
        self.originals.insert(a, id);
        Ok(id)
    }
    pub fn cut(&mut self, a: u32, b: u32) -> Result<u32, MeshError> {
        if self.side[a as usize] == 0 {
            return self.original(a);
        }
        if self.side[b as usize] == 0 {
            return self.original(b);
        }
        let key = plane_edge(a, b);
        if let Some(id) = self.cuts.get(&key) {
            return Ok(*id);
        }
        let (a, b) = (key[0], key[1]);
        let (da, db) = (self.dist[a as usize], self.dist[b as usize]);
        let den = da - db;
        if !den.is_finite() || den == 0.0 {
            return Err(plane_error("intersection denominator"));
        }
        let w = da / den;
        if !(w > 0.0 && w < 1.0) {
            return Err(plane_error("intersection parameter"));
        }
        let (pa, pb) = (self.m.point(a), self.m.point(b));
        let mut p = [0.0; 3];
        for i in 0..3 {
            p[i] = (1.0 - w) * pa[i] + w * pb[i];
        }
        if !vec_finite(p) {
            return Err(plane_error("intersection overflow"));
        }
        let id = self.vertex(PlaneVertex { p, a, b, w })?;
        self.cuts.insert(key, id);
        Ok(id)
    }
    fn scalars(&mut self, ids: &[u32]) -> Result<Vec<VertexScalarAttribute>, MeshError> {
        let mut out = Vec::with_capacity(self.o.vertex_scalars.len());
        for a in 0..self.o.vertex_scalars.len() {
            let mut values = Vec::with_capacity(ids.len());
            for id in ids {
                self.step()?;
                let v = self.vertices[*id as usize];
                let s = &self.o.vertex_scalars[a].values;
                let value = (1.0 - v.w) * s[v.a as usize] + v.w * s[v.b as usize];
                if !value.is_finite() {
                    return Err(plane_error("scalar interpolation overflow"));
                }
                values.push(value);
            }
            let s = &self.o.vertex_scalars[a];
            out.push(VertexScalarAttribute {
                name: s.name.clone(),
                units: s.units.clone(),
                values,
            });
        }
        Ok(out)
    }
    pub fn section(&mut self) -> Result<PlaneSection, MeshError> {
        let m = self.m;
        let mut edges: HashSet<PlaneKey> = HashSet::new();
        let mut coplanar: HashMap<PlaneKey, i64> = HashMap::new();
        let mut tangent: HashSet<u32> = HashSet::new();
        for t in &m.triangles {
            self.step()?;
            let mut ids: Vec<u32> = Vec::with_capacity(3);
            let side = |v: u32| self.side[v as usize];
            let all = side(t[0]) == 0 && side(t[1]) == 0 && side(t[2]) == 0;
            for (j, a) in t.iter().enumerate() {
                let b = t[(j + 1) % 3];
                if self.side[*a as usize] == 0 {
                    ids.push(self.original(*a)?);
                }
                if self.side[*a as usize] * self.side[b as usize] < 0 {
                    ids.push(self.cut(*a, b)?);
                }
            }
            if all {
                for j in 0..ids.len() {
                    *coplanar
                        .entry(plane_edge(ids[j], ids[(j + 1) % 3]))
                        .or_insert(0) += 1;
                }
            } else if ids.len() == 2 && ids[0] != ids[1] {
                edges.insert(plane_edge(ids[0], ids[1]));
            } else if ids.len() == 1 {
                tangent.insert(ids[0]);
            }
        }
        for (k, c) in &coplanar {
            if *c == 1 {
                edges.insert(*k);
            } else if *c != 2 {
                return Err(plane_error("coplanar edge multiplicity"));
            }
        }
        let mut keys: Vec<PlaneKey> = edges.into_iter().collect();
        for k in &keys {
            tangent.remove(&k[0]);
            tangent.remove(&k[1]);
        }
        keys.sort_unstable();
        let lines = self.walk_undirected(&keys)?;
        let mut used: HashMap<u32, u32> = HashMap::new();
        let mut ids: Vec<u32> = Vec::new();
        let mut s = PlaneSection {
            plane: self.plane,
            tolerance_mm: self.o.tolerance_mm,
            tangent_vertices: tangent.len() as i64,
            ..PlaneSection::default()
        };
        for mut l in lines {
            for id in l.point_ids.iter_mut() {
                let next = ids.len() as u32;
                let ni = *used.entry(*id).or_insert_with(|| {
                    ids.push(*id);
                    s.points.push(self.vertices[*id as usize].p);
                    next
                });
                *id = ni;
            }
            s.polylines.push(l);
        }
        s.vertex_scalars = self.scalars(&ids)?;
        let (normal, tolerance) = (self.plane.normal, self.o.tolerance_mm);
        self.nest(&s.points, &mut s.polylines, normal, tolerance)?;
        Ok(s)
    }
    fn walk_undirected(&mut self, edges: &[PlaneKey]) -> Result<Vec<SectionPolyline>, MeshError> {
        let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
        for e in edges {
            adj.entry(e[0]).or_default().push(e[1]);
            adj.entry(e[1]).or_default().push(e[0]);
        }
        if adj.values().any(|ns| ns.len() > 2) {
            return Err(plane_error("branching section"));
        }
        let neighbours = |id: u32| adj.get(&id).map_or(&[][..], Vec::as_slice);
        let mut used: HashSet<PlaneKey> = HashSet::new();
        let mut out = Vec::new();
        for edge in edges {
            if used.contains(edge) {
                continue;
            }
            let mut start = edge[0];
            let mut queue = std::collections::VecDeque::from([start]);
            let mut seen: HashSet<u32> = HashSet::from([start]);
            while let Some(id) = queue.pop_front() {
                self.step()?;
                if neighbours(id).len() == 1 {
                    start = id;
                    break;
                }
                for n in neighbours(id) {
                    if seen.insert(*n) {
                        queue.push_back(*n);
                    }
                }
            }
            let mut l = SectionPolyline {
                parent: -1,
                ..SectionPolyline::default()
            };
            let mut cur = start;
            loop {
                self.step()?;
                l.point_ids.push(cur);
                let Some(next) = neighbours(cur)
                    .iter()
                    .copied()
                    .find(|n| !used.contains(&plane_edge(cur, *n)))
                else {
                    break;
                };
                used.insert(plane_edge(cur, next));
                cur = next;
                if cur == start {
                    l.closed = true;
                    break;
                }
            }
            if l.closed && l.point_ids.len() < 3 {
                return Err(plane_error("collapsed section"));
            }
            out.push(l);
        }
        Ok(out)
    }
    fn nest(
        &mut self,
        points: &[[f64; 3]],
        loops: &mut [SectionPolyline],
        n: [f64; 3],
        tol: f64,
    ) -> Result<(), MeshError> {
        let xy = plane_project(points, n);
        let at = |id: u32| xy[id as usize];
        for a in 0..loops.len() {
            let l = &loops[a];
            if !l.closed {
                continue;
            }
            if plane_area(&xy, &l.point_ids).abs() <= tol * tol {
                return Err(plane_error("zero-area section loop"));
            }
            for (b, m) in loops.iter().enumerate().skip(a) {
                if !m.closed {
                    continue;
                }
                for (i, id) in l.point_ids.iter().enumerate() {
                    let u = l.point_ids[(i + 1) % l.point_ids.len()];
                    for (j, jd) in m.point_ids.iter().enumerate() {
                        self.step()?;
                        let v = m.point_ids[(j + 1) % m.point_ids.len()];
                        if a == b && (i == j || u == *jd || *id == v) {
                            continue;
                        }
                        if plane_segments_meet(at(*id), at(u), at(*jd), at(v), tol) {
                            return Err(plane_error("touching/intersecting section loops"));
                        }
                    }
                }
            }
        }
        for a in 0..loops.len() {
            loops[a].parent = -1;
            loops[a].depth = 0;
            loops[a].hole = false;
            if !loops[a].closed {
                continue;
            }
            let mut best = f64::INFINITY;
            for b in 0..loops.len() {
                self.step()?;
                if a == b || !loops[b].closed {
                    continue;
                }
                if plane_inside(at(loops[a].point_ids[0]), &xy, &loops[b].point_ids) {
                    let area = plane_area(&xy, &loops[b].point_ids).abs();
                    if area < best {
                        best = area;
                        loops[a].parent = b as i64;
                    }
                }
            }
        }
        for a in 0..loops.len() {
            if !loops[a].closed {
                continue;
            }
            let mut p = loops[a].parent;
            while p >= 0 {
                self.step()?;
                loops[a].depth += 1;
                if loops[a].depth > loops.len() as i64 {
                    return Err(plane_error("loop nesting cycle"));
                }
                p = loops[p as usize].parent;
            }
            loops[a].hole = loops[a].depth % 2 == 1;
        }
        Ok(())
    }
    pub fn caps(
        &mut self,
        loops: &mut [SectionPolyline],
        n: [f64; 3],
    ) -> Result<Vec<[u32; 3]>, MeshError> {
        let points: Vec<[f64; 3]> = self.vertices.iter().map(|v| v.p).collect();
        let xy = plane_project(&points, n);
        let tolerance = self.o.tolerance_mm;
        self.nest(&points, loops, n, tolerance)?;
        for l in loops.iter() {
            let area = plane_area(&xy, &l.point_ids);
            if (area > 0.0) == l.hole {
                return Err(plane_error("inconsistent nested shell winding"));
            }
        }
        let mut out: Vec<[u32; 3]> = Vec::new();
        for i in 0..loops.len() {
            if loops[i].hole {
                continue;
            }
            let mut ring = loops[i].point_ids.clone();
            let mut expected = plane_area(&xy, &ring);
            for h in 0..loops.len() {
                if loops[h].parent != i as i64 || !loops[h].hole {
                    continue;
                }
                expected += plane_area(&xy, &loops[h].point_ids);
                ring = self.bridge(&xy, &ring, h, loops, i)?;
            }
            let triangles = self.ears(&xy, &ring, loops, i)?;
            let mut area = 0.0;
            for t in &triangles {
                area += plane_orient(xy[t[0] as usize], xy[t[1] as usize], xy[t[2] as usize]) / 2.0;
            }
            if (area - expected).abs()
                > fmath::max(
                    tolerance * tolerance * ring.len() as f64,
                    expected.abs() * 1e-9,
                )
            {
                return Err(plane_error("cap area mismatch"));
            }
            let total = (out.len() + triangles.len()) as i64;
            if total > self.o.max_result_triangles {
                return Err(plane_limit(
                    "plane cap faces",
                    total,
                    self.o.max_result_triangles,
                ));
            }
            out.extend(triangles);
        }
        Ok(out)
    }
    fn bridge(
        &mut self,
        xy: &[PlaneXy],
        ring: &[u32],
        h: usize,
        loops: &[SectionPolyline],
        outer: usize,
    ) -> Result<Vec<u32>, MeshError> {
        let hole = &loops[h].point_ids;
        let tolerance = self.o.tolerance_mm;
        let mut found: Option<(usize, usize)> = None;
        let mut best = f64::INFINITY;
        for (i, a) in ring.iter().enumerate() {
            for (j, b) in hole.iter().enumerate() {
                self.step()?;
                if a == b {
                    continue;
                }
                let (pa, pb) = (xy[*a as usize], xy[*b as usize]);
                let (dx, dy) = (pa.x - pb.x, pa.y - pb.y);
                let d = dx * dx + dy * dy;
                if d >= best {
                    continue;
                }
                let mut ok = true;
                let boundaries = std::iter::once(ring).chain(
                    loops
                        .iter()
                        .filter(|l| l.closed)
                        .map(|l| l.point_ids.as_slice()),
                );
                'boundaries: for ids in boundaries {
                    for (k, c) in ids.iter().enumerate() {
                        self.step()?;
                        let dd = ids[(k + 1) % ids.len()];
                        if c == a || c == b || dd == *a || dd == *b {
                            continue;
                        }
                        if plane_segments_meet(pa, pb, xy[*c as usize], xy[dd as usize], tolerance)
                        {
                            ok = false;
                            break 'boundaries;
                        }
                    }
                }
                let mid = PlaneXy {
                    x: (pa.x + pb.x) / 2.0,
                    y: (pa.y + pb.y) / 2.0,
                };
                if ok && plane_domain(mid, xy, loops, outer) {
                    found = Some((i, j));
                    best = d;
                }
            }
        }
        let Some((ai, bi)) = found else {
            return Err(plane_error(format!("no visible cavity bridge (hole {h})")));
        };
        let mut result = Vec::with_capacity(ring.len() + hole.len() + 2);
        result.extend_from_slice(&ring[..=ai]);
        for k in 0..hole.len() {
            result.push(hole[(bi + k) % hole.len()]);
        }
        result.push(hole[bi]);
        result.push(ring[ai]);
        result.extend_from_slice(&ring[ai + 1..]);
        Ok(result)
    }
    fn ears(
        &mut self,
        xy: &[PlaneXy],
        ring: &[u32],
        loops: &[SectionPolyline],
        outer: usize,
    ) -> Result<Vec<[u32; 3]>, MeshError> {
        let mut ids: Vec<u32> = ring.to_vec();
        let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(ids.len().saturating_sub(2));
        let tolerance = self.o.tolerance_mm;
        let at = |id: u32| xy[id as usize];
        while ids.len() > 3 {
            let mut found = None;
            let len = ids.len();
            for i in 0..len {
                self.step()?;
                let (a, b, c) = (ids[(i + len - 1) % len], ids[i], ids[(i + 1) % len]);
                if a == b || b == c || a == c {
                    continue;
                }
                if !cap_ear_nondegenerate(at(a), at(b), at(c), tolerance) {
                    continue;
                }
                if let Some(accept) = self.accept_triangle {
                    if !accept([a, b, c]) {
                        continue;
                    }
                    if len == 4 {
                        let mut last = [0u32; 3];
                        let mut k = 0;
                        for (j, id) in ids.iter().enumerate() {
                            if j != i {
                                last[k] = *id;
                                k += 1;
                            }
                        }
                        if !cap_ear_nondegenerate(at(last[0]), at(last[1]), at(last[2]), tolerance)
                            || !accept(last)
                        {
                            continue;
                        }
                    }
                }
                let mut ok = true;
                for v in &ids {
                    self.step()?;
                    let v = *v;
                    if v == a
                        || v == b
                        || v == c
                        || plane_same(at(v), at(a))
                        || plane_same(at(v), at(b))
                        || plane_same(at(v), at(c))
                    {
                        continue;
                    }
                    if point_in_cap_ear(at(a), at(b), at(c), at(v), tolerance) {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                let centroid = PlaneXy {
                    x: (at(a).x + at(b).x + at(c).x) / 3.0,
                    y: (at(a).y + at(b).y + at(c).y) / 3.0,
                };
                if !plane_domain(centroid, xy, loops, outer) {
                    continue;
                }
                for k in 0..len {
                    self.step()?;
                    let (v, w) = (ids[k], ids[(k + 1) % len]);
                    if v == a || v == c || w == a || w == c {
                        continue;
                    }
                    if plane_segments_meet(at(a), at(c), at(v), at(w), 0.0) {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                triangles.push([a, b, c]);
                found = Some(i);
                break;
            }
            match found {
                Some(i) => {
                    ids.remove(i);
                }
                None => return Err(plane_error("cap triangulation inconclusive")),
            }
        }
        if ids.len() != 3 || !cap_ear_nondegenerate(at(ids[0]), at(ids[1]), at(ids[2]), tolerance) {
            return Err(plane_error("degenerate final cap ear"));
        }
        if let Some(accept) = self.accept_triangle
            && !accept([ids[0], ids[1], ids[2]])
        {
            return Err(plane_error("unrepresentable final ear"));
        }
        let center = PlaneXy {
            x: (at(ids[0]).x + at(ids[1]).x + at(ids[2]).x) / 3.0,
            y: (at(ids[0]).y + at(ids[1]).y + at(ids[2]).y) / 3.0,
        };
        if !plane_domain(center, xy, loops, outer) {
            return Err(plane_error("cap outside solid"));
        }
        triangles.push([ids[0], ids[1], ids[2]]);
        Ok(triangles)
    }
    pub fn boundary(&mut self, triangles: &[[u32; 3]]) -> Result<Vec<SectionPolyline>, MeshError> {
        let mut edges: HashMap<PlaneKey, usize> = HashMap::new();
        let mut uses: Vec<(u32, u32, i32)> = Vec::new();
        for t in triangles {
            for (j, a) in t.iter().enumerate() {
                self.step()?;
                let b = t[(j + 1) % 3];
                match edges.get(&plane_edge(*a, b)) {
                    None => {
                        edges.insert(plane_edge(*a, b), uses.len());
                        uses.push((*a, b, 1));
                    }
                    Some(index) => {
                        let u = &mut uses[*index];
                        if u.2 != 1 || u.0 != b || u.1 != *a {
                            return Err(plane_error("side edge multiplicity/winding"));
                        }
                        u.2 = 2;
                    }
                }
            }
        }
        let mut next: HashMap<u32, u32> = HashMap::new();
        let mut incoming: HashSet<u32> = HashSet::new();
        let mut keys: Vec<u32> = Vec::new();
        for (a, b, count) in &uses {
            if *count != 1 {
                continue;
            }
            for id in [a, b] {
                let d = dot3(
                    sub3(self.vertices[*id as usize].p, self.plane.origin),
                    self.plane.normal,
                );
                if d.abs() > self.o.tolerance_mm * 2.0 {
                    return Err(plane_error("boundary off cut plane"));
                }
            }
            if next.contains_key(b) || incoming.contains(a) {
                return Err(plane_error("branching cap boundary"));
            }
            next.insert(*b, *a);
            incoming.insert(*a);
            keys.push(*b);
        }
        keys.sort_unstable();
        let mut visited: HashSet<u32> = HashSet::new();
        let mut loops = Vec::new();
        for start in keys {
            if visited.contains(&start) {
                continue;
            }
            let mut l = SectionPolyline {
                closed: true,
                parent: -1,
                ..SectionPolyline::default()
            };
            let mut cur = start;
            loop {
                self.step()?;
                if visited.contains(&cur) {
                    if cur != start {
                        return Err(plane_error("joined cap loops"));
                    }
                    break;
                }
                visited.insert(cur);
                l.point_ids.push(cur);
                match next.get(&cur) {
                    Some(v) => cur = *v,
                    None => return Err(plane_error("open cap boundary")),
                }
            }
            if l.point_ids.len() < 3 {
                return Err(plane_error("collapsed cap loop"));
            }
            loops.push(l);
        }
        Ok(loops)
    }
    fn retained_components(&mut self, keep: i32) -> Result<(Vec<bool>, bool), MeshError> {
        let m = self.m;
        let count = m.vertices.len();
        let mut parents: Vec<usize> = Vec::with_capacity(count);
        let mut sizes = vec![1usize; count];
        for i in 0..count {
            self.step()?;
            parents.push(i);
        }
        fn find(
            r: &mut PlaneRun<'_>,
            parents: &mut [usize],
            mut a: usize,
        ) -> Result<usize, MeshError> {
            while parents[a] != a {
                r.step()?;
                parents[a] = parents[parents[a]];
                a = parents[a];
            }
            Ok(a)
        }
        for t in &m.triangles {
            for j in 1..3 {
                self.step()?;
                let mut a = find(self, &mut parents, t[0] as usize)?;
                let mut b = find(self, &mut parents, t[j] as usize)?;
                if a != b {
                    if sizes[a] < sizes[b] {
                        std::mem::swap(&mut a, &mut b);
                    }
                    parents[b] = a;
                    sizes[a] += sizes[b];
                }
            }
        }
        let mut strict = vec![false; count];
        for i in 0..count {
            self.step()?;
            if self.side[i] == keep {
                let a = find(self, &mut parents, i)?;
                strict[a] = true;
            }
        }
        let mut result = vec![false; m.triangles.len()];
        let mut any = false;
        for (i, t) in m.triangles.iter().enumerate() {
            self.step()?;
            let a = find(self, &mut parents, t[0] as usize)?;
            result[i] = strict[a];
            any = any || result[i];
        }
        Ok((result, any))
    }
}
pub fn section_by_plane(
    m: Option<&Mesh>,
    p: Plane,
    o: &PlaneOptions,
) -> Result<PlaneSection, MeshError> {
    PlaneRun::new(m, p, o)?.section()
}
pub fn clip_closed_by_plane(
    m: Option<&Mesh>,
    p: Plane,
    o: &PlaneOptions,
) -> Result<PlaneClipResult, MeshError> {
    let mut r = PlaneRun::new(m, p, o)?;
    let m = r.m;
    let section = r.section()?;
    let mut result = PlaneClipResult {
        plane: r.plane,
        keep_positive: o.keep_positive,
        source_reference: o.source_reference.clone(),
        section,
        reserved_bytes: r.reserved,
        ..PlaneClipResult::default()
    };
    let keep: i32 = if o.keep_positive { 1 } else { -1 };
    let (retained, any) = r.retained_components(keep)?;
    if !any {
        result.work = r.work;
        return Ok(result);
    }
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut sources: Vec<i64> = Vec::new();
    fn append_face(
        r: &mut PlaneRun<'_>,
        triangles: &mut Vec<[u32; 3]>,
        sources: &mut Vec<i64>,
        t: [u32; 3],
        source: i64,
    ) -> Result<(), MeshError> {
        r.step()?;
        if triangles.len() as i64 >= r.o.max_result_triangles {
            return Err(plane_limit(
                "plane result triangles",
                triangles.len() as i64 + 1,
                r.o.max_result_triangles,
            ));
        }
        let p = |i: usize| r.vertices[t[i] as usize].p;
        let n = cross3(sub3(p(1), p(0)), sub3(p(2), p(0)));
        if dot3(n, n) == 0.0 {
            return Err(plane_error("collapsed clipped face"));
        }
        triangles.push(t);
        sources.push(source);
        Ok(())
    }
    for (tid, t) in m.triangles.iter().enumerate() {
        if !retained[tid] {
            continue;
        }
        r.step()?;
        let mut poly: Vec<u32> = Vec::with_capacity(4);
        for (j, a) in t.iter().enumerate() {
            let b = t[(j + 1) % 3];
            let inside = r.side[*a as usize] * keep >= 0;
            let next = r.side[b as usize] * keep >= 0;
            if inside {
                poly.push(r.original(*a)?);
            }
            if inside != next {
                poly.push(r.cut(*a, b)?);
            }
        }
        poly.dedup();
        if poly.len() > 1 && poly[0] == poly[poly.len() - 1] {
            poly.pop();
        }
        for j in 1..poly.len().saturating_sub(1) {
            append_face(
                &mut r,
                &mut triangles,
                &mut sources,
                [poly[0], poly[j], poly[j + 1]],
                tid as i64,
            )?;
        }
    }
    let mut loops = r.boundary(&triangles)?;
    let mut cap_normal = r.plane.normal;
    for c in cap_normal.iter_mut() {
        *c *= -f64::from(keep);
    }
    if !loops.is_empty() {
        for t in r.caps(&mut loops, cap_normal)? {
            append_face(&mut r, &mut triangles, &mut sources, t, -1)?;
        }
    }
    let mut ids: Vec<u32> = Vec::new();
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut out = Mesh {
        normals: m.normals.as_ref().map(|_| Vec::new()),
        ..Mesh::default()
    };
    for t in triangles.iter_mut() {
        r.step()?;
        for corner in t.iter_mut() {
            let id = *corner;
            let v = match remap.get(&id) {
                Some(v) => *v,
                None => {
                    let v = ids.len() as u32;
                    remap.insert(id, v);
                    ids.push(id);
                    let pv = r.vertices[id as usize];
                    let mut q = [0f32; 3];
                    for k in 0..3 {
                        q[k] = pv.p[k] as f32;
                        if !q[k].is_finite() {
                            return Err(plane_error("float32 output overflow"));
                        }
                    }
                    out.vertices.push(q);
                    if let (Some(source), Some(normals)) = (&m.normals, &mut out.normals) {
                        let (na, nb) = (source[pv.a as usize], source[pv.b as usize]);
                        let mut n = [0f64; 3];
                        for k in 0..3 {
                            n[k] = (1.0 - pv.w) * f64::from(na[k]) + pv.w * f64::from(nb[k]);
                        }
                        let l = dot3(n, n).sqrt();
                        let mut unit = [0f32; 3];
                        if l > 0.0 {
                            for k in 0..3 {
                                unit[k] = (n[k] / l) as f32;
                            }
                        }
                        normals.push(unit);
                    }
                    v
                }
            };
            *corner = v;
        }
    }
    out.triangles = triangles;
    if !out.triangles.is_empty() {
        let (max_vertices, max_triangles) = (r.o.max_result_vertices, r.o.max_result_triangles);
        r.validate_solid(&out, max_vertices, max_triangles)?;
    }
    result.mesh = out;
    result.vertex_scalars = r.scalars(&ids)?;
    if let Some(colors) = &o.face_colors {
        result.face_colors = Some(
            sources
                .iter()
                .map(|id| {
                    if *id < 0 {
                        o.cap_color
                    } else {
                        colors[*id as usize]
                    }
                })
                .collect(),
        );
    }
    result.source_triangles = sources;
    result.work = r.work;
    Ok(result)
}
fn vec_v(v: [f64; 3]) -> String {
    format!("[{} {} {}]", float_v(v[0]), float_v(v[1]), float_v(v[2]))
}
impl PlaneClipResult {
    pub fn ply_bytes(&self, max_bytes: i64) -> Result<Vec<u8>, MeshError> {
        if max_bytes < 0 {
            return Err(plane_error("export request"));
        }
        let max_bytes = if max_bytes == 0 { 512 << 20 } else { max_bytes };
        self.mesh.validate()?;
        if !self.mesh.check_closure().closed() {
            return Err(plane_error("export requires closed result"));
        }
        let comments = vec![
            format!("derived source {}", self.source_reference),
            format!(
                "plane origin {} normal {} keep_positive {}",
                vec_v(self.plane.origin),
                vec_v(self.plane.normal),
                self.keep_positive
            ),
            "source triangle correspondence is provided in PlaneClipResult.SourceTriangles; cap=-1"
                .to_string(),
        ];
        let mut out = Vec::new();
        write_ply(
            &mut out,
            &self.mesh,
            &PlyOptions {
                vertex_scalars: self.vertex_scalars.clone(),
                face_colors: self.face_colors.clone(),
                comments,
            },
        )?;
        let total = out.len() as i64;
        if total > max_bytes {
            const CHUNK: i64 = 1 << 16;
            let used = total.min((max_bytes / CHUNK + 1) * CHUNK);
            return Err(plane_limit("plane export bytes", used, max_bytes));
        }
        Ok(out)
    }
    pub fn write_ply_file(&self, path: &Path, max_bytes: i64) -> Result<(), MeshError> {
        if path.as_os_str().is_empty() {
            return Err(plane_error("export request"));
        }
        let bytes = self.ply_bytes(max_bytes)?;
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = format!(
            ".mesh-plane-{}-{:x}.ply",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let temporary = directory.join(stem);
        let staged = (|| -> std::io::Result<()> {
            let mut file = std::fs::File::create_new(&temporary)?;
            std::io::Write::write_all(&mut file, &bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        })();
        if staged.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        Ok(staged?)
    }
}
