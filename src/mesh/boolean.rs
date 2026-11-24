use super::attributes::{
    MAX_VERTEX_SCALAR_ATTRIBUTES, VertexScalarAttribute, validate_vertex_scalars,
};
use super::bvh::scale3;
use super::clean::connected_components;
use super::locator::closest_on_triangle;
use super::plane::{Plane, PlaneOptions, PlaneRun, PlaneVertex, SectionPolyline, plane_limit};
use super::quality::{
    QualityOptions, QualityPairKind, Triangle, analyze_quality, quality_distances,
    quality_intersect, quality_normal, quality_plane_interval, quality_roundoff,
};
use super::{ErrorKind, Mesh, MeshError, add3, cross3, dot3, sub3};
use crate::fmath;
use crate::numtext::format_float;
use std::collections::HashMap;
use std::rc::Rc;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BooleanOperation {
    #[default]
    Union,
    Intersection,
    Difference,
}
impl BooleanOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            BooleanOperation::Union => "union",
            BooleanOperation::Intersection => "intersection",
            BooleanOperation::Difference => "difference",
        }
    }
    pub fn parse(name: &str) -> Option<BooleanOperation> {
        match name {
            "union" => Some(BooleanOperation::Union),
            "intersection" => Some(BooleanOperation::Intersection),
            "difference" => Some(BooleanOperation::Difference),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct BooleanInput<'a> {
    pub mesh: Option<&'a Mesh>,
    pub reference: &'a str,
    pub vertex_scalars: &'a [VertexScalarAttribute],
    pub face_colors: Option<&'a [[u8; 3]]>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BooleanOptions {
    pub max_working_vertices: i64,
    pub tolerance_mm: f64,
    pub max_vertices: i64,
    pub max_triangles: i64,
    pub max_polygons: i64,
    pub max_polygon_vertices: i64,
    pub max_planes_per_face: i64,
    pub max_result_vertices: i64,
    pub max_result_triangles: i64,
    pub max_scratch_bytes: i64,
    pub max_output_bytes: i64,
    pub max_work: i64,
    pub max_candidates: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BooleanFaceSource {
    pub input: u8,
    pub triangle: u32,
    pub reversed: bool,
    pub barycentric: [[f64; 3]; 3],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BooleanCornerScalar {
    pub name: String,
    pub units: String,
    pub values: Vec<[f64; 3]>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BooleanResult {
    pub mesh: Mesh,
    pub operation: BooleanOperation,
    pub references: [String; 2],
    pub tolerance_mm: f64,
    pub faces: Vec<BooleanFaceSource>,
    pub corner_normals: Vec<[[f32; 3]; 3]>,
    pub source_normals: Vec<bool>,
    pub colors_present: Vec<bool>,
    pub face_colors: Vec<[u8; 3]>,
    pub corner_scalars: Vec<BooleanCornerScalar>,
    pub diagnostics: Vec<String>,
    pub scratch_bytes: i64,
    pub output_bytes: i64,
    pub work: i64,
    pub candidates: i64,
    pub max_projection_error_mm: f64,
}
pub(crate) fn boolean_error(detail: impl AsRef<str>) -> MeshError {
    MeshError {
        kind: ErrorKind::BooleanGeometry,
        message: format!("mesh: boolean geometry: {}", detail.as_ref()),
        budget: None,
    }
}
fn wrapped(e: MeshError) -> MeshError {
    boolean_error(e.message)
}
const EPSILON_128: f64 = 128.0 * f64::EPSILON;
#[derive(Debug, Clone)]
struct Polygon {
    p: Vec<[f64; 3]>,
    n: [f64; 3],
    origin: [f64; 3],
    input: u8,
    face: u32,
    reversed: bool,
}
type SplitParts = (Option<Rc<Polygon>>, Option<Rc<Polygon>>);
#[derive(Debug, Clone, Copy)]
struct Cut {
    n: [f64; 3],
    origin: [f64; 3],
    segment: [[f64; 3]; 2],
}
struct Tracked<T> {
    items: Vec<T>,
    cap: i64,
}
impl<T> Default for Tracked<T> {
    fn default() -> Self {
        Tracked {
            items: Vec::new(),
            cap: 0,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
struct BoxLimits {
    lo: [f64; 3],
    hi: [f64; 3],
}
struct Run<'a> {
    cuts: [Vec<Tracked<Cut>>; 2],
    o: BooleanOptions,
    inputs: [BooleanInput<'a>; 2],
    meshes: [&'a Mesh; 2],
    eps: f64,
    work: i64,
    candidates: i64,
    scratch: i64,
    output: i64,
    polygons: i64,
    projection_error: f64,
}
static EMPTY_MESH: Mesh = Mesh {
    vertices: Vec::new(),
    normals: None,
    triangles: Vec::new(),
};
fn boolean_options(mut o: BooleanOptions) -> Result<BooleanOptions, MeshError> {
    if !o.tolerance_mm.is_finite() || o.tolerance_mm < 0.0 {
        return Err(boolean_error("tolerance"));
    }
    if o.tolerance_mm == 0.0 {
        o.tolerance_mm = 1e-6;
    }
    let defaults = [
        200_000,
        400_000,
        200_000,
        128,
        2048,
        200_000,
        400_000,
        256 << 20,
        256 << 20,
        20_000_000,
        1_000_000,
        400_000,
    ];
    let limits = [
        &mut o.max_vertices,
        &mut o.max_triangles,
        &mut o.max_polygons,
        &mut o.max_polygon_vertices,
        &mut o.max_planes_per_face,
        &mut o.max_result_vertices,
        &mut o.max_result_triangles,
        &mut o.max_scratch_bytes,
        &mut o.max_output_bytes,
        &mut o.max_work,
        &mut o.max_candidates,
        &mut o.max_working_vertices,
    ];
    for (p, d) in limits.into_iter().zip(defaults) {
        if *p < 0 {
            return Err(boolean_error("negative quota"));
        }
        if *p == 0 {
            *p = d;
        }
    }
    let max_u32 = i64::from(u32::MAX);
    if o.max_planes_per_face > 4096
        || o.max_vertices > i64::from(i32::MAX)
        || o.max_triangles > max_u32
        || o.max_result_vertices > max_u32
        || o.max_result_triangles > max_u32
        || o.max_working_vertices > max_u32
    {
        return Err(boolean_error("plane/index capacity"));
    }
    Ok(o)
}
fn charge(counter: &mut i64, n: i64, limit: i64, resource: &str) -> Result<(), MeshError> {
    if n < 0 || n > limit - *counter {
        let used = if n >= 0 && *counter <= i64::MAX - n {
            *counter + n
        } else {
            i64::MAX
        };
        return Err(plane_limit(resource, used, limit));
    }
    *counter += n;
    Ok(())
}
fn triangle_of(m: &Mesh, id: usize) -> Triangle {
    let t = m.triangles[id];
    [m.point(t[0]), m.point(t[1]), m.point(t[2])]
}
fn boxes_overlap(a: &BoxLimits, b: &BoxLimits, tol: f64) -> bool {
    (0..3).all(|i| !(a.hi[i] < b.lo[i] - tol || b.hi[i] < a.lo[i] - tol))
}
fn intersection_segment(a: &Triangle, b: &Triangle) -> Option<[[f64; 3]; 2]> {
    let (na, nb) = (quality_normal(a), quality_normal(b));
    let axis = cross3(na, nb);
    let length = dot3(axis, axis).sqrt();
    if length <= EPSILON_128 {
        return None;
    }
    let axis = scale3(axis, 1.0 / length);
    let eps = quality_roundoff(a, b);
    let anchor = a[0];
    let interval = |t: &Triangle, d: [f64; 3]| -> (f64, f64, [[f64; 3]; 2], i32) {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut endpoints = [[0.0; 3]; 2];
        let mut count = 0;
        let mut add = |p: [f64; 3]| {
            let v = dot3(sub3(p, anchor), axis);
            if v < lo {
                lo = v;
                endpoints[0] = p;
            }
            if v > hi {
                hi = v;
                endpoints[1] = p;
            }
            count += 1;
        };
        for i in 0..3 {
            let j = (i + 1) % 3;
            let mut p = t[i];
            if d[i].abs() <= eps {
                add(p);
            }
            if (d[i] > eps && d[j] < -eps) || (d[i] < -eps && d[j] > eps) {
                let (mut q, mut dp, mut dq) = (t[j], d[i], d[j]);
                for k in 0..3 {
                    if p[k] != q[k] {
                        if p[k] > q[k] {
                            std::mem::swap(&mut p, &mut q);
                            std::mem::swap(&mut dp, &mut dq);
                        }
                        break;
                    }
                }
                add(add3(p, scale3(sub3(q, p), dp / (dp - dq))));
            }
        }
        (lo, hi, endpoints, count)
    };
    let (al, ah, p, ac) = interval(a, quality_distances(a, b[0], nb));
    let (bl, bh, q, bc) = interval(b, quality_distances(b, a[0], na));
    let (lo, hi) = (fmath::max(al, bl), fmath::min(ah, bh));
    if ac < 2 || bc < 2 || hi - lo <= eps {
        return None;
    }
    let mut ends = p;
    if bl > al {
        ends[0] = q[0];
    }
    if bh < ah {
        ends[1] = q[1];
    }
    Some(ends)
}
impl<'a> Run<'a> {
    fn step(&mut self) -> Result<(), MeshError> {
        charge(&mut self.work, 1, self.o.max_work, "boolean work")
    }
    fn reserve(&mut self, n: i64) -> Result<(), MeshError> {
        charge(
            &mut self.scratch,
            n,
            self.o.max_scratch_bytes,
            "boolean scratch bytes",
        )
    }
    fn grow(
        &mut self,
        len: usize,
        cap: &mut i64,
        maximum: i64,
        size: i64,
        output: bool,
    ) -> Result<(), MeshError> {
        let len = len as i64;
        if len >= maximum {
            return Err(plane_limit("boolean array elements", len + 1, maximum));
        }
        if len < *cap {
            return Ok(());
        }
        let n = maximum.min((*cap * 2).max(8));
        if size != 0 && n > i64::MAX / size {
            return Err(plane_limit("boolean array bytes", i64::MAX, 0));
        }
        if output {
            charge(
                &mut self.output,
                n * size,
                self.o.max_output_bytes,
                "boolean output bytes",
            )?;
        } else {
            self.reserve(n * size)?;
        }
        *cap = n;
        Ok(())
    }
    fn push<T>(
        &mut self,
        list: &mut Tracked<T>,
        item: T,
        maximum: i64,
        size: i64,
    ) -> Result<(), MeshError> {
        self.grow(list.items.len(), &mut list.cap, maximum, size, false)?;
        list.items.push(item);
        Ok(())
    }
    fn preflight(&mut self) -> Result<(), MeshError> {
        let mut scale = 0.0;
        let mut labels: [Vec<i64>; 2] = [Vec::new(), Vec::new()];
        let mut boxes: [Vec<BoxLimits>; 2] = [Vec::new(), Vec::new()];
        for input in 0..2 {
            let source = self.inputs[input];
            let Some(m) = source.mesh.filter(|_| source.reference.len() <= 4096) else {
                return Err(boolean_error("nil mesh/source reference"));
            };
            self.meshes[input] = m;
            let (n, t) = (m.vertices.len() as i64, m.triangles.len() as i64);
            if n > self.o.max_vertices {
                return Err(plane_limit(
                    "boolean input vertices",
                    n,
                    self.o.max_vertices,
                ));
            }
            if t > self.o.max_triangles {
                return Err(plane_limit(
                    "boolean input triangles",
                    t,
                    self.o.max_triangles,
                ));
            }
            if t == 0 && n != 0 {
                return Err(boolean_error("unused vertices in empty boundary"));
            }
            if source.vertex_scalars.len() > MAX_VERTEX_SCALAR_ATTRIBUTES {
                return Err(boolean_error("scalar attribute count"));
            }
            self.reserve(n * 64 + t * 1024 + 32768)?;
            charge(
                &mut self.work,
                n * (source.vertex_scalars.len() as i64 + 1),
                self.o.max_work,
                "boolean work",
            )?;
            validate_vertex_scalars(source.vertex_scalars, m.vertices.len(), false)?;
            if source
                .face_colors
                .is_some_and(|c| c.len() != m.triangles.len())
            {
                return Err(boolean_error("face color count"));
            }
            self.validate_solid(m, false)?;
            for p in &m.vertices {
                for v in p {
                    scale = fmath::max(scale, f64::from(*v).abs());
                }
            }
            charge(
                &mut self.work,
                n + 24 * t,
                self.o.max_work,
                "boolean component work",
            )?;
            let (component, count) = connected_components(m);
            self.orientation(m, &component, count)?;
            labels[input] = component;
            self.cuts[input] = (0..m.triangles.len()).map(|_| Tracked::default()).collect();
            for id in 0..m.triangles.len() {
                self.step()?;
                let tri = triangle_of(m, id);
                let mut limits = BoxLimits {
                    lo: tri[0],
                    hi: tri[0],
                };
                for p in &tri[1..] {
                    for a in 0..3 {
                        limits.lo[a] = fmath::min(limits.lo[a], p[a]);
                        limits.hi[a] = fmath::max(limits.hi[a], p[a]);
                    }
                }
                boxes[input].push(limits);
            }
        }
        self.eps = fmath::max(scale * 128.0 * f64::EPSILON, f64::from_bits(1_000_000));
        if self.o.tolerance_mm < 16.0 * self.eps {
            return Err(boolean_error("tolerance below coordinate roundoff"));
        }
        if !self.meshes[0].triangles.is_empty() && !self.meshes[1].triangles.is_empty() {
            let (a, b) = (self.inputs[0].vertex_scalars, self.inputs[1].vertex_scalars);
            if a.len() != b.len() {
                return Err(boolean_error("unsupported scalar schema mismatch"));
            }
            if a.iter()
                .zip(b)
                .any(|(x, y)| x.name != y.name || x.units != y.units)
            {
                return Err(boolean_error("unsupported scalar name/unit mismatch"));
            }
        }
        self.cross_inputs(&labels, &boxes)
    }
    fn validate_solid(&mut self, m: &Mesh, output: bool) -> Result<(), MeshError> {
        self.step()?;
        if m.normals
            .as_ref()
            .is_some_and(|n| n.len() != m.vertices.len())
        {
            return Err(boolean_error("normal count"));
        }
        if m.triangles.is_empty() {
            if !m.vertices.is_empty() {
                return Err(boolean_error("vertices without boundary"));
            }
            return Ok(());
        }
        let (max_v, max_t, altitude) = if output {
            (
                self.o.max_result_vertices,
                self.o.max_result_triangles,
                self.eps * 8.0,
            )
        } else {
            (
                self.o.max_vertices,
                self.o.max_triangles,
                self.o.tolerance_mm,
            )
        };
        if self.work >= self.o.max_work {
            return Err(plane_limit("boolean work", self.work + 1, self.o.max_work));
        }
        if self.candidates >= self.o.max_candidates {
            return Err(plane_limit(
                "boolean candidates",
                self.candidates + 1,
                self.o.max_candidates,
            ));
        }
        if self.scratch >= self.o.max_scratch_bytes {
            return Err(plane_limit(
                "boolean scratch bytes",
                self.scratch + 1,
                self.o.max_scratch_bytes,
            ));
        }
        let (q, result) = analyze_quality(
            Some(m),
            QualityOptions {
                distance_tolerance_mm: 0.0,
                max_vertices: max_v,
                max_triangles: max_t,
                max_bytes: self.o.max_scratch_bytes - self.scratch,
                max_work: self.o.max_work - self.work,
                max_candidates: self.o.max_candidates - self.candidates,
                max_results: 1,
                ..QualityOptions::default()
            },
        );
        charge(
            &mut self.work,
            q.work,
            self.o.max_work,
            "boolean quality work",
        )?;
        charge(
            &mut self.candidates,
            q.candidates,
            self.o.max_candidates,
            "boolean candidates",
        )?;
        self.reserve(q.reserved_bytes)?;
        result?;
        if !q.complete
            || !q.intersections_conclusive
            || q.degenerate.count
                + q.duplicate_faces.count
                + q.crossings
                + q.coplanar_overlaps
                + q.contacts
                + q.uncertain_pairs
                != 0
        {
            return Err(boolean_error(format!(
                "boundary quality (degenerate={} duplicate={} crossing={} coplanar={} contact={} uncertain={})",
                q.degenerate.count,
                q.duplicate_faces.count,
                q.crossings,
                q.coplanar_overlaps,
                q.contacts,
                q.uncertain_pairs
            )));
        }
        charge(
            &mut self.work,
            m.triangles.len() as i64 * 3,
            self.o.max_work,
            "boolean closure work",
        )?;
        if !m.check_closure().closed() {
            return Err(boolean_error("open/nonmanifold/inconsistent boundary"));
        }
        let mut p = PlaneRun::bare(
            &EMPTY_MESH,
            Vec::new(),
            Plane::default(),
            PlaneOptions {
                max_work: self.o.max_work - self.work,
                ..PlaneOptions::default()
            },
        );
        let links = p.validate_vertex_links(m);
        charge(
            &mut self.work,
            p.work,
            self.o.max_work,
            "boolean vertex links",
        )?;
        links.map_err(wrapped)?;
        for id in 0..m.triangles.len() {
            self.step()?;
            let t = triangle_of(m, id);
            let n = cross3(sub3(t[1], t[0]), sub3(t[2], t[0]));
            let mut longest = 0.0;
            for i in 0..3 {
                let v = sub3(t[(i + 1) % 3], t[i]);
                longest = fmath::max(longest, dot3(v, v).sqrt());
            }
            if dot3(n, n).sqrt() / longest <= altitude {
                return Err(boolean_error("face altitude within resolution"));
            }
        }
        self.adjacent_intersections(m)
    }
    fn adjacent_intersections(&mut self, m: &Mesh) -> Result<(), MeshError> {
        for i in 0..m.triangles.len() {
            let a = m.triangles[i];
            for j in i + 1..m.triangles.len() {
                self.step()?;
                let b = m.triangles[j];
                let mut shared = 0;
                let mut common = 0u32;
                for v in a {
                    for w in b {
                        if v == w {
                            shared += 1;
                            common = v;
                        }
                    }
                }
                if shared == 0 {
                    continue;
                }
                charge(
                    &mut self.candidates,
                    1,
                    self.o.max_candidates,
                    "boolean candidates",
                )?;
                let (ta, tb) = (triangle_of(m, i), triangle_of(m, j));
                let kind = quality_intersect(ta, tb, 0.0);
                if matches!(
                    kind,
                    Some(
                        QualityPairKind::Crossing
                            | QualityPairKind::CoplanarOverlap
                            | QualityPairKind::Uncertain
                    )
                ) {
                    return Err(boolean_error("folded adjacent faces"));
                }
                if shared != 1 || kind != Some(QualityPairKind::Contact) {
                    continue;
                }
                let (na, nb) = (quality_normal(&ta), quality_normal(&tb));
                let axis = cross3(na, nb);
                let length = dot3(axis, axis).sqrt();
                if length > EPSILON_128 {
                    let axis = scale3(axis, 1.0 / length);
                    let eps = quality_roundoff(&ta, &tb);
                    let (al, ah, _) =
                        quality_plane_interval(&ta, quality_distances(&ta, tb[0], nb), axis, eps);
                    let (bl, bh, _) =
                        quality_plane_interval(&tb, quality_distances(&tb, ta[0], na), axis, eps);
                    if fmath::min(ah, bh) - fmath::max(al, bl) > eps {
                        return Err(boolean_error(
                            "adjacent faces intersect beyond shared vertex",
                        ));
                    }
                } else {
                    for (side, t) in [ta, tb].iter().enumerate() {
                        let (ids, other) = if side == 1 { (b, &ta) } else { (a, &tb) };
                        for (v, p) in t.iter().enumerate() {
                            if ids[v] != common {
                                let (closest, _) =
                                    closest_on_triangle(*p, other[0], other[1], other[2]);
                                let d = sub3(*p, closest);
                                let eps = quality_roundoff(&ta, &tb);
                                if dot3(d, d) <= eps * eps {
                                    return Err(boolean_error("coplanar adjacent edge overlap"));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn orientation(&mut self, m: &Mesh, labels: &[i64], count: i64) -> Result<(), MeshError> {
        if count == 0 {
            return Ok(());
        }
        self.reserve(count * 64)?;
        let count = count as usize;
        let mut first: Vec<Option<usize>> = vec![None; count];
        let mut volumes = vec![0.0f64; count];
        for (id, t) in m.triangles.iter().enumerate() {
            self.step()?;
            let c = labels[id] as usize;
            let face = *first[c].get_or_insert(id);
            let origin = m.point(m.triangles[face][0]);
            let (a, b, d) = (
                sub3(m.point(t[0]), origin),
                sub3(m.point(t[1]), origin),
                sub3(m.point(t[2]), origin),
            );
            volumes[c] += dot3(a, cross3(b, d)) / 6.0;
        }
        for c in 0..count {
            let Some(face) = first[c] else { continue };
            let p = m.point(m.triangles[face][0]);
            let mut depth = 0;
            for other in 0..count {
                if other != c {
                    let w = self.winding(m, Some(labels), other as i64, p)?;
                    if w.abs() > 0.5 {
                        depth += 1;
                    }
                }
            }
            if volumes[c] == 0.0 || (volumes[c] > 0.0) != (depth % 2 == 0) {
                return Err(boolean_error("component orientation/nesting"));
            }
        }
        Ok(())
    }
    fn winding(
        &mut self,
        m: &Mesh,
        labels: Option<&[i64]>,
        component: i64,
        p: [f64; 3],
    ) -> Result<f64, MeshError> {
        let mut sum = 0.0;
        for (id, t) in m.triangles.iter().enumerate() {
            self.step()?;
            if component >= 0 && labels.is_some_and(|l| l[id] != component) {
                continue;
            }
            let (a, b, c) = (
                sub3(m.point(t[0]), p),
                sub3(m.point(t[1]), p),
                sub3(m.point(t[2]), p),
            );
            let (la, lb, lc) = (dot3(a, a).sqrt(), dot3(b, b).sqrt(), dot3(c, c).sqrt());
            if la == 0.0 || lb == 0.0 || lc == 0.0 {
                return Err(boolean_error("touching components"));
            }
            sum += 2.0
                * fmath::atan2(
                    dot3(a, cross3(b, c)),
                    la * lb * lc + dot3(a, b) * lc + dot3(b, c) * la + dot3(c, a) * lb,
                );
        }
        let w = sum / (4.0 * std::f64::consts::PI);
        if !w.is_finite() || (w - w.round()).abs() > 1e-6 || w.abs() > 1.000001 {
            return Err(boolean_error("inconclusive winding"));
        }
        Ok(w)
    }
    fn cross_inputs(
        &mut self,
        labels: &[Vec<i64>; 2],
        boxes: &[Vec<BoxLimits>; 2],
    ) -> Result<(), MeshError> {
        let mut pairs: HashMap<[i64; 2], (bool, bool)> = HashMap::new();
        let (ma, mb) = (self.meshes[0], self.meshes[1]);
        let tolerance = self.o.tolerance_mm;
        for (i, a) in boxes[0].iter().enumerate() {
            for (j, b) in boxes[1].iter().enumerate() {
                self.step()?;
                if !boxes_overlap(a, b, tolerance) {
                    continue;
                }
                charge(
                    &mut self.candidates,
                    1,
                    self.o.max_candidates,
                    "boolean candidates",
                )?;
                let (ta, tb) = (triangle_of(ma, i), triangle_of(mb, j));
                let Some(kind) = quality_intersect(ta, tb, tolerance) else {
                    continue;
                };
                if matches!(
                    kind,
                    QualityPairKind::Uncertain | QualityPairKind::CoplanarOverlap
                ) {
                    return Err(boolean_error("coplanar overlap/uncertain intersection"));
                }
                if kind == QualityPairKind::Contact
                    && quality_intersect(ta, tb, 0.0) != Some(QualityPairKind::Contact)
                {
                    return Err(boolean_error("intersection feature within tolerance"));
                }
                let key = [labels[0][i], labels[1][j]];
                if !pairs.contains_key(&key) {
                    self.reserve(128)?;
                }
                let v = pairs.entry(key).or_insert((false, false));
                v.0 = v.0 || kind == QualityPairKind::Crossing;
                v.1 = v.1 || kind == QualityPairKind::Contact;
                if let Some(segment) = intersection_segment(&ta, &tb) {
                    let ca = Cut {
                        n: quality_normal(&tb),
                        origin: tb[0],
                        segment,
                    };
                    let cb = Cut {
                        n: quality_normal(&ta),
                        origin: ta[0],
                        segment,
                    };
                    let planes = self.o.max_planes_per_face;
                    let (len_a, mut cap_a) = (self.cuts[0][i].items.len(), self.cuts[0][i].cap);
                    self.grow(len_a, &mut cap_a, planes, 96, false)?;
                    self.cuts[0][i].cap = cap_a;
                    let (len_b, mut cap_b) = (self.cuts[1][j].items.len(), self.cuts[1][j].cap);
                    self.grow(len_b, &mut cap_b, planes, 96, false)?;
                    self.cuts[1][j].cap = cap_b;
                    self.cuts[0][i].items.push(ca);
                    self.cuts[1][j].items.push(cb);
                }
            }
        }
        if pairs.values().any(|v| v.1 && !v.0) {
            return Err(boolean_error("tangent components"));
        }
        Ok(())
    }
    fn polygon(&mut self, points: &[[f64; 3]], source: &Polygon) -> Result<Polygon, MeshError> {
        let count = points.len() as i64;
        if count > self.o.max_polygon_vertices {
            return Err(plane_limit(
                "boolean polygon vertices",
                count,
                self.o.max_polygon_vertices,
            ));
        }
        charge(
            &mut self.polygons,
            1,
            self.o.max_polygons,
            "boolean polygons",
        )?;
        self.reserve(count * 24 + 128)?;
        Ok(Polygon {
            p: points.to_vec(),
            ..source.clone()
        })
    }
    fn source_polygons(&mut self, input: u8) -> Result<Vec<Rc<Polygon>>, MeshError> {
        let m = self.meshes[input as usize];
        self.reserve(m.triangles.len() as i64 * 8)?;
        let mut out = Vec::with_capacity(m.triangles.len());
        for face in 0..m.triangles.len() {
            self.step()?;
            let points = triangle_of(m, face);
            let seed = Polygon {
                p: Vec::new(),
                n: quality_normal(&points),
                origin: points[0],
                input,
                face: face as u32,
                reversed: false,
            };
            out.push(Rc::new(self.polygon(&points, &seed)?));
        }
        Ok(out)
    }
    fn split(&mut self, p: &Rc<Polygon>, cut: &Cut) -> Result<SplitParts, MeshError> {
        let (n, origin) = (cut.n, cut.origin);
        let eps = self.eps;
        self.step()?;
        self.reserve(p.p.len() as i64 * 8)?;
        let mut d = Vec::with_capacity(p.p.len());
        let (mut positive, mut negative) = (false, false);
        for v in &p.p {
            self.step()?;
            let distance = dot3(sub3(*v, origin), n);
            positive = positive || distance > eps;
            negative = negative || distance < -eps;
            d.push(distance);
        }
        if !positive && !negative {
            return Err(boolean_error("unresolved coplanar partition"));
        }
        if !negative {
            return Ok((Some(p.clone()), None));
        }
        if !positive {
            return Ok((None, Some(p.clone())));
        }
        self.reserve((p.p.len() as i64 + 2) * 48)?;
        let (mut f, mut b) = (Vec::new(), Vec::new());
        for (i, v) in p.p.iter().enumerate() {
            self.step()?;
            let j = (i + 1) % p.p.len();
            if d[i] >= -eps {
                f.push(*v);
            }
            if d[i] <= eps {
                b.push(*v);
            }
            if (d[i] > eps && d[j] < -eps) || (d[i] < -eps && d[j] > eps) {
                let weight = d[i] / (d[i] - d[j]);
                let mut point = add3(*v, scale3(sub3(p.p[j], *v), weight));
                let edge = sub3(p.p[j], *v);
                let edge2 = dot3(edge, edge);
                let mut snapped: Option<[f64; 3]> = None;
                for endpoint in cut.segment {
                    let t = dot3(sub3(endpoint, *v), edge) / edge2;
                    if t < 0.0 || t > 1.0 {
                        continue;
                    }
                    let delta = sub3(endpoint, add3(*v, scale3(edge, t)));
                    if dot3(delta, delta) > 64.0 * eps * eps {
                        continue;
                    }
                    let delta = sub3(endpoint, point);
                    if dot3(delta, delta).sqrt() > self.o.tolerance_mm {
                        continue;
                    }
                    if let Some(previous) = snapped {
                        let delta = sub3(endpoint, previous);
                        if dot3(delta, delta) > eps * eps {
                            return Err(boolean_error("ambiguous intersection endpoint"));
                        }
                    }
                    snapped = Some(endpoint);
                }
                if let Some(s) = snapped {
                    point = s;
                }
                f.push(point);
                b.push(point);
            }
        }
        if f.len() < 3 || b.len() < 3 {
            return Err(boolean_error("unresolved plane split"));
        }
        let front = Rc::new(self.polygon(&f, p)?);
        let back = Rc::new(self.polygon(&b, p)?);
        Ok((Some(front), Some(back)))
    }
    fn segment_interior(&mut self, p: &Polygon, s: [[f64; 3]; 2]) -> Result<bool, MeshError> {
        let (mut lo, mut hi) = (0.0, 1.0);
        let eps = self.eps;
        for (i, a) in p.p.iter().enumerate() {
            self.step()?;
            let edge = sub3(p.p[(i + 1) % p.p.len()], *a);
            let length = dot3(edge, edge).sqrt();
            if length <= eps {
                return Err(boolean_error("collapsed partition edge"));
            }
            let d0 = dot3(cross3(edge, sub3(s[0], *a)), p.n) / length;
            let d1 = dot3(cross3(edge, sub3(s[1], *a)), p.n) / length;
            if d0 < -eps && d1 < -eps {
                return Ok(false);
            }
            if d0 < 0.0 && d1 > d0 {
                lo = fmath::max(lo, -d0 / (d1 - d0));
            }
            if d1 < 0.0 && d0 > d1 {
                hi = fmath::min(hi, d0 / (d0 - d1));
            }
            if hi <= lo {
                return Ok(false);
            }
        }
        let delta = sub3(s[1], s[0]);
        let length = dot3(delta, delta).sqrt();
        if (hi - lo) * length <= 16.0 * eps {
            return Ok(false);
        }
        let midpoint = add3(s[0], scale3(delta, (lo + hi) / 2.0));
        for (i, a) in p.p.iter().enumerate() {
            self.step()?;
            let edge = sub3(p.p[(i + 1) % p.p.len()], *a);
            let length = dot3(edge, edge).sqrt();
            if dot3(cross3(edge, sub3(midpoint, *a)), p.n) / length <= eps {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn partition(
        &mut self,
        p: &Rc<Polygon>,
        inside: bool,
        reverse: bool,
        out: &mut Tracked<Rc<Polygon>>,
    ) -> Result<(), MeshError> {
        let max_polygons = self.o.max_polygons;
        let mut parts: Tracked<Rc<Polygon>> = Tracked::default();
        self.push(&mut parts, p.clone(), max_polygons, 8)?;
        let cuts: Vec<Cut> = self.cuts[p.input as usize][p.face as usize].items.clone();
        for cut in &cuts {
            let mut next: Tracked<Rc<Polygon>> = Tracked::default();
            for part in &parts.items {
                let crosses = self.segment_interior(part, cut.segment)?;
                let (front, back) = if crosses {
                    self.split(part, cut)?
                } else {
                    (Some(part.clone()), None)
                };
                for piece in [front, back].into_iter().flatten() {
                    self.push(&mut next, piece, max_polygons, 8)?;
                }
            }
            parts = next;
        }
        let other = self.meshes[1 - p.input as usize];
        for part in &parts.items {
            let mut center = [0.0; 3];
            for point in &part.p {
                center = add3(center, scale3(*point, 1.0 / part.p.len() as f64));
            }
            let winding = self.winding(other, None, -1, center)?;
            if winding < -0.5 {
                return Err(boolean_error("negative solid winding"));
            }
            if (winding > 0.5) == inside {
                self.keep(part, reverse, out)?;
            }
        }
        Ok(())
    }
    fn keep(
        &mut self,
        p: &Rc<Polygon>,
        reverse: bool,
        out: &mut Tracked<Rc<Polygon>>,
    ) -> Result<(), MeshError> {
        let max_polygons = self.o.max_polygons;
        self.grow(out.items.len(), &mut out.cap, max_polygons, 8, false)?;
        let mut kept = p.clone();
        if reverse {
            let mut q = self.polygon(&p.p, p)?;
            q.reversed = true;
            q.p.reverse();
            q.n = scale3(q.n, -1.0);
            kept = Rc::new(q);
        }
        out.items.push(kept);
        Ok(())
    }
}
#[derive(Default)]
struct Working {
    vertices: Tracked<PlaneVertex>,
}
impl Working {
    fn add(&mut self, r: &mut Run<'_>, p: [f64; 3]) -> Result<u32, MeshError> {
        for (i, q) in self.vertices.items.iter().enumerate() {
            r.step()?;
            let d = sub3(p, q.p);
            if dot3(d, d) <= r.eps * r.eps {
                return Ok(i as u32);
            }
        }
        let maximum = r.o.max_working_vertices;
        let id = self.vertices.items.len() as u32;
        r.push(
            &mut self.vertices,
            PlaneVertex {
                p,
                ..PlaneVertex::default()
            },
            maximum,
            40,
        )?;
        Ok(id)
    }
    fn p(&self, id: u32) -> [f64; 3] {
        self.vertices.items[id as usize].p
    }
}
impl Run<'_> {
    fn conform_working(
        &mut self,
        ids: &[u32],
        w: &Working,
        candidates: &[u32],
    ) -> Result<Vec<u32>, MeshError> {
        let maximum = self.o.max_polygon_vertices;
        let eps = self.eps;
        let mut boundary: Tracked<u32> = Tracked::default();
        let mut cuts: Vec<(u32, f64)> = Vec::new();
        for (j, a) in ids.iter().enumerate() {
            let b = ids[(j + 1) % ids.len()];
            self.push(&mut boundary, *a, maximum, 4)?;
            let (pa, pb) = (w.p(*a), w.p(b));
            let edge = sub3(pb, pa);
            let length2 = dot3(edge, edge);
            if length2 <= eps * eps {
                return Err(boolean_error("collapsed construction edge"));
            }
            cuts.clear();
            for id in candidates {
                self.step()?;
                if id == a || *id == b {
                    continue;
                }
                let point = w.p(*id);
                let t = dot3(sub3(point, pa), edge) / length2;
                if t <= 0.0 || t >= 1.0 {
                    continue;
                }
                let d = sub3(point, add3(pa, scale3(edge, t)));
                if dot3(d, d) > 64.0 * eps * eps {
                    continue;
                }
                let mut at = cuts.len();
                cuts.push((0, 0.0));
                while at > 0 && cuts[at - 1].1 > t {
                    self.step()?;
                    cuts[at] = cuts[at - 1];
                    at -= 1;
                }
                cuts[at] = (*id, t);
            }
            for cut in &cuts {
                self.push(&mut boundary, cut.0, maximum, 4)?;
            }
        }
        Ok(boundary.items)
    }
    fn simplify_working(&mut self, mut ids: Vec<u32>, w: &Working) -> Result<Vec<u32>, MeshError> {
        let eps = self.eps;
        let mut changed = true;
        while changed && ids.len() > 3 {
            changed = false;
            for i in 0..ids.len() {
                self.step()?;
                let len = ids.len();
                let (a, b) = (w.p(ids[(i + len - 1) % len]), w.p(ids[(i + 1) % len]));
                let edge = sub3(b, a);
                let l2 = dot3(edge, edge);
                if l2 <= eps * eps {
                    return Err(boolean_error("collapsed loop"));
                }
                let point = w.p(ids[i]);
                let t = dot3(sub3(point, a), edge) / l2;
                if t <= 0.0 || t >= 1.0 {
                    continue;
                }
                let d = sub3(point, add3(a, scale3(edge, t)));
                if dot3(d, d) <= 64.0 * eps * eps {
                    ids.remove(i);
                    changed = true;
                    break;
                }
            }
        }
        Ok(ids)
    }
    fn merge_pieces(&mut self, pieces: &[Rc<Polygon>]) -> Result<Vec<Polygon>, MeshError> {
        if pieces.is_empty() {
            return Ok(Vec::new());
        }
        let mut w = Working::default();
        self.reserve(pieces.len() as i64 * 24)?;
        let mut polys: Vec<Vec<u32>> = Vec::with_capacity(pieces.len());
        for p in pieces {
            self.reserve(p.p.len() as i64 * 4)?;
            let mut ids: Vec<u32> = Vec::with_capacity(p.p.len());
            for point in &p.p {
                let id = w.add(self, *point)?;
                if ids.last() != Some(&id) {
                    ids.push(id);
                }
            }
            if ids.len() > 1 && ids[0] == ids[ids.len() - 1] {
                ids.pop();
            }
            if ids.len() < 3 {
                return Err(boolean_error("collapsed construction polygon"));
            }
            polys.push(ids);
        }
        let initial = w.vertices.items.len();
        self.reserve(initial as i64 * 20)?;
        let all: Vec<u32> = (0..initial as u32).collect();
        for ids in polys.iter_mut() {
            *ids = self.conform_working(ids, &w, &all)?;
        }
        let max_polygons = self.o.max_polygons;
        let max_triangles = self.o.max_result_triangles;
        let mut groups: Tracked<(Rc<Polygon>, Vec<SectionPolyline>)> = Tracked::default();
        let mut start = 0;
        while start < pieces.len() {
            let mut end = start + 1;
            while end < pieces.len()
                && pieces[end].input == pieces[start].input
                && pieces[end].face == pieces[start].face
            {
                end += 1;
            }
            let mut triangles: Tracked<[u32; 3]> = Tracked::default();
            for ids in &polys[start..end] {
                if ids.len() == 3 {
                    self.push(&mut triangles, [ids[0], ids[1], ids[2]], max_triangles, 12)?;
                    continue;
                }
                let mut center = [0.0; 3];
                for id in ids {
                    center = add3(center, scale3(w.p(*id), 1.0 / ids.len() as f64));
                }
                let cid = w.add(self, center)?;
                for (j, id) in ids.iter().enumerate() {
                    self.push(
                        &mut triangles,
                        [cid, *id, ids[(j + 1) % ids.len()]],
                        max_triangles,
                        12,
                    )?;
                }
            }
            self.reserve(triangles.items.len() as i64 * 1024 + 32768)?;
            if self.work >= self.o.max_work {
                return Err(plane_limit(
                    "boolean merge work",
                    self.work + 1,
                    self.o.max_work,
                ));
            }
            let mut p = PlaneRun::bare(
                &EMPTY_MESH,
                w.vertices.items.clone(),
                Plane {
                    normal: pieces[start].n,
                    origin: pieces[start].origin,
                },
                PlaneOptions {
                    tolerance_mm: self.eps * 16.0,
                    max_work: self.o.max_work - self.work,
                    ..PlaneOptions::default()
                },
            );
            let boundary = p.boundary(&triangles.items);
            charge(
                &mut self.work,
                p.work,
                self.o.max_work,
                "boolean merge work",
            )?;
            let mut loops = boundary.map_err(wrapped)?;
            for l in loops.iter_mut() {
                l.point_ids.reverse();
                l.point_ids = self.simplify_working(std::mem::take(&mut l.point_ids), &w)?;
            }
            self.push(
                &mut groups,
                (pieces[start].clone(), loops),
                max_polygons,
                32,
            )?;
            start = end;
        }
        let total = w.vertices.items.len();
        self.reserve(total as i64 * 21)?;
        let mut used = vec![false; total];
        let mut candidates: Tracked<u32> = Tracked {
            items: Vec::new(),
            cap: initial as i64,
        };
        let max_working = self.o.max_working_vertices;
        for (_, loops) in &groups.items {
            for l in loops {
                for id in &l.point_ids {
                    self.step()?;
                    if !used[*id as usize] {
                        used[*id as usize] = true;
                        self.push(&mut candidates, *id, max_working, 4)?;
                    }
                }
            }
        }
        let mut out: Tracked<Polygon> = Tracked::default();
        for (source, loops) in groups.items.iter_mut() {
            for l in loops.iter_mut() {
                l.point_ids = self.conform_working(&l.point_ids, &w, &candidates.items)?;
            }
            self.reserve(total as i64 * 64 + 32768)?;
            charge(
                &mut self.work,
                total as i64 * 3,
                self.o.max_work,
                "boolean contour projection",
            )?;
            if self.work >= self.o.max_work {
                return Err(plane_limit(
                    "boolean triangulation work",
                    self.work + 1,
                    self.o.max_work,
                ));
            }
            let normal = source.n;
            let positions = &w.vertices.items;
            let accept = |t: [u32; 3]| -> bool {
                let rounded = |id: u32| positions[id as usize].p.map(|v| f64::from(v as f32));
                let (a, b, c) = (rounded(t[0]), rounded(t[1]), rounded(t[2]));
                dot3(cross3(sub3(b, a), sub3(c, a)), normal) > 0.0
            };
            let mut p = PlaneRun::bare(
                &EMPTY_MESH,
                positions.clone(),
                Plane::default(),
                PlaneOptions {
                    tolerance_mm: self.eps * 16.0,
                    max_work: self.o.max_work - self.work,
                    max_result_triangles: self.o.max_result_triangles,
                    ..PlaneOptions::default()
                },
            );
            p.accept_triangle = Some(&accept);
            let caps = p.caps(loops, normal);
            let spent = p.work;
            drop(p);
            charge(
                &mut self.work,
                spent,
                self.o.max_work,
                "boolean triangulation work",
            )?;
            for t in caps.map_err(wrapped)? {
                let points = [w.p(t[0]), w.p(t[1]), w.p(t[2])];
                let area = cross3(sub3(points[1], points[0]), sub3(points[2], points[0]));
                if dot3(area, area).sqrt() == 0.0 {
                    return Err(boolean_error("degenerate merged face"));
                }
                let q = self.polygon(&points, source)?;
                self.push(&mut out, q, max_polygons, 8)?;
            }
        }
        Ok(out.items)
    }
    fn finish(
        &mut self,
        pieces: &[Polygon],
        operation: BooleanOperation,
    ) -> Result<BooleanResult, MeshError> {
        charge(
            &mut self.output,
            512 + (self.inputs[0].reference.len() + self.inputs[1].reference.len()) as i64,
            self.o.max_output_bytes,
            "boolean output bytes",
        )?;
        let mut result = BooleanResult {
            operation,
            references: [
                self.inputs[0].reference.to_string(),
                self.inputs[1].reference.to_string(),
            ],
            tolerance_mm: self.o.tolerance_mm,
            ..BooleanResult::default()
        };
        let schema = if self.meshes[0].triangles.is_empty() {
            self.inputs[1].vertex_scalars
        } else {
            self.inputs[0].vertex_scalars
        };
        charge(
            &mut self.output,
            schema.len() as i64 * 64,
            self.o.max_output_bytes,
            "boolean attribute headers",
        )?;
        result.corner_scalars = schema
            .iter()
            .map(|s| BooleanCornerScalar {
                name: s.name.clone(),
                units: s.units.clone(),
                values: Vec::new(),
            })
            .collect();
        let mut points: Tracked<[f64; 3]> = Tracked::default();
        let (mut vertex_cap, mut face_cap) = (0i64, 0i64);
        for p in pieces {
            if p.p.len() != 3 {
                return Err(boolean_error("unfinished triangulation"));
            }
            let mut ids = [0u32; 3];
            for (i, point) in p.p.iter().enumerate() {
                ids[i] = self.add_vertex(&mut points, &mut vertex_cap, &mut result.mesh, *point)?;
            }
            self.emit_face(&mut result, &mut face_cap, p, ids)?;
        }
        if !result.mesh.triangles.is_empty() {
            let (v, t) = (
                result.mesh.vertices.len() as i64,
                result.mesh.triangles.len() as i64,
            );
            self.reserve(v * 64 + t * 1024 + 32768)?;
            self.validate_solid(&result.mesh, true)?;
            self.reserve(v * 16 + t * 64)?;
            charge(
                &mut self.work,
                v + 24 * t,
                self.o.max_work,
                "boolean output components",
            )?;
            let (labels, count) = connected_components(&result.mesh);
            self.orientation(&result.mesh, &labels, count)?;
        }
        result.diagnostics = vec![
            "shared geometric vertices; shading normals and scalars preserved per face corner"
                .to_string(),
        ];
        result.work = self.work;
        result.candidates = self.candidates;
        result.scratch_bytes = self.scratch;
        result.output_bytes = self.output;
        result.max_projection_error_mm = self.projection_error;
        Ok(result)
    }
    fn add_vertex(
        &mut self,
        points: &mut Tracked<[f64; 3]>,
        vertex_cap: &mut i64,
        mesh: &mut Mesh,
        p: [f64; 3],
    ) -> Result<u32, MeshError> {
        let f = [p[0] as f32, p[1] as f32, p[2] as f32];
        let converted = [f64::from(f[0]), f64::from(f[1]), f64::from(f[2])];
        let d = sub3(p, converted);
        if converted.iter().any(|c| !c.is_finite()) || dot3(d, d).sqrt() > self.o.tolerance_mm {
            return Err(boolean_error("float32 position exceeds tolerance"));
        }
        for (i, q) in points.items.iter().enumerate() {
            self.step()?;
            let d = sub3(p, *q);
            if dot3(d, d) <= self.eps * self.eps {
                return Ok(i as u32);
            }
            if f == mesh.vertices[i] {
                return Err(boolean_error(format!(
                    "float32 merges distinct positions (separation={} roundoff={})",
                    format_float(dot3(d, d).sqrt(), 'g', -1),
                    format_float(self.eps, 'g', -1)
                )));
            }
        }
        let maximum = self.o.max_result_vertices;
        let len = points.items.len();
        self.grow(len, &mut points.cap, maximum, 24, false)?;
        self.grow(len, vertex_cap, maximum, 12, true)?;
        points.items.push(p);
        mesh.vertices.push(f);
        Ok(len as u32)
    }
    fn emit_face(
        &mut self,
        result: &mut BooleanResult,
        face_cap: &mut i64,
        p: &Polygon,
        ids: [u32; 3],
    ) -> Result<(), MeshError> {
        self.step()?;
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0] {
            return Err(boolean_error("collapsed triangle"));
        }
        let out = &result.mesh;
        let (a, b, c) = (out.point(ids[0]), out.point(ids[1]), out.point(ids[2]));
        let geometric = cross3(sub3(b, a), sub3(c, a));
        let length = dot3(geometric, geometric).sqrt();
        if length == 0.0 || dot3(geometric, p.n) <= 0.0 {
            return Err(boolean_error("rounded face orientation"));
        }
        let geometric = scale3(geometric, 1.0 / length);
        let source_input = self.inputs[p.input as usize];
        let m = self.meshes[p.input as usize];
        let t = m.triangles[p.face as usize];
        let original = triangle_of(m, p.face as usize);
        let mut source = BooleanFaceSource {
            input: p.input,
            triangle: p.face,
            reversed: p.reversed,
            barycentric: [[0.0; 3]; 3],
        };
        let mut normals = [[0f32; 3]; 3];
        for (corner, id) in ids.iter().enumerate() {
            let point = out.point(*id);
            let (closest, bary) = closest_on_triangle(point, original[0], original[1], original[2]);
            let d = sub3(point, closest);
            let distance = dot3(d, d).sqrt();
            if distance > self.o.tolerance_mm {
                return Err(boolean_error("source interpolation exceeds tolerance"));
            }
            self.projection_error = fmath::max(self.projection_error, distance);
            source.barycentric[corner] = bary;
            let mut n = geometric;
            if let Some(source_normals) = &m.normals {
                n = [0.0; 3];
                for (j, vertex) in t.iter().enumerate() {
                    for axis in 0..3 {
                        n[axis] += bary[j] * f64::from(source_normals[*vertex as usize][axis]);
                    }
                }
                let l = dot3(n, n).sqrt();
                if l == 0.0 {
                    return Err(boolean_error("unsupported cancelling source normals"));
                }
                n = scale3(n, 1.0 / l);
                if p.reversed {
                    n = scale3(n, -1.0);
                }
            }
            normals[corner] = [n[0] as f32, n[1] as f32, n[2] as f32];
        }
        let limit = self.o.max_result_triangles;
        let len = result.mesh.triangles.len();
        let mut grown = *face_cap;
        for size in [12, 88, 36, 1, 1, 3] {
            grown = *face_cap;
            self.grow(len, &mut grown, limit, size, true)?;
        }
        for (i, s) in result.corner_scalars.iter_mut().enumerate() {
            let mut cap = *face_cap;
            self.grow(len, &mut cap, limit, 24, true)?;
            let mut values = [0.0f64; 3];
            for (corner, bary) in source.barycentric.iter().enumerate() {
                for (j, vertex) in t.iter().enumerate() {
                    values[corner] +=
                        bary[j] * source_input.vertex_scalars[i].values[*vertex as usize];
                }
            }
            if values.iter().any(|v| !v.is_finite()) {
                return Err(boolean_error("scalar interpolation overflow"));
            }
            s.values.push(values);
        }
        *face_cap = grown;
        let color = source_input
            .face_colors
            .map_or([0; 3], |colors| colors[p.face as usize]);
        result.mesh.triangles.push(ids);
        result.faces.push(source);
        result.corner_normals.push(normals);
        result.source_normals.push(m.normals.is_some());
        result
            .colors_present
            .push(source_input.face_colors.is_some());
        result.face_colors.push(color);
        Ok(())
    }
}
pub fn boolean(
    a: BooleanInput<'_>,
    b: BooleanInput<'_>,
    operation: BooleanOperation,
    options: &BooleanOptions,
) -> Result<BooleanResult, MeshError> {
    let o = boolean_options(*options)?;
    let mut r = Run {
        cuts: [Vec::new(), Vec::new()],
        o,
        inputs: [a, b],
        meshes: [&EMPTY_MESH, &EMPTY_MESH],
        eps: 0.0,
        work: 0,
        candidates: 0,
        scratch: 0,
        output: 0,
        polygons: 0,
        projection_error: 0.0,
    };
    r.preflight()?;
    let sources = [r.source_polygons(0)?, r.source_polygons(1)?];
    let mut pieces: Tracked<Rc<Polygon>> = Tracked::default();
    for (i, list) in sources.iter().enumerate() {
        let second = i == 1;
        let inside = operation == BooleanOperation::Intersection
            || (operation == BooleanOperation::Difference && second);
        let reverse = operation == BooleanOperation::Difference && second;
        for p in list {
            r.partition(p, inside, reverse, &mut pieces)?;
        }
    }
    let merged = r.merge_pieces(&pieces.items)?;
    r.finish(&merged, operation)
}
#[doc(hidden)]
pub fn boolean_named(
    a: BooleanInput<'_>,
    b: BooleanInput<'_>,
    operation: &str,
    options: &BooleanOptions,
) -> Result<BooleanResult, MeshError> {
    match BooleanOperation::parse(operation) {
        Some(op) => boolean(a, b, op, options),
        None => Err(boolean_error(format!(
            "operation {}",
            crate::textfmt::quote(operation)
        ))),
    }
}
