use super::bvh::scale3;
use super::{ErrorKind, Mesh, MeshError, add3, cross3, dot3, sub3};
use crate::fmath;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct QualityOptions {
    pub distance_tolerance_mm: f64,
    pub min_area_mm2: f64,
    pub min_angle_degrees: f64,
    pub max_aspect_ratio: f64,
    pub max_vertices: i64,
    pub max_triangles: i64,
    pub max_bytes: i64,
    pub max_work: i64,
    pub max_candidates: i64,
    pub max_results: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct QualityStatistics {
    pub samples: i64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QualityFinding {
    pub count: i64,
    pub indices: Vec<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPairKind {
    Crossing,
    CoplanarOverlap,
    Contact,
    Uncertain,
    Duplicate,
}
impl QualityPairKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            QualityPairKind::Crossing => "crossing",
            QualityPairKind::CoplanarOverlap => "coplanar-overlap",
            QualityPairKind::Contact => "contact",
            QualityPairKind::Uncertain => "uncertain",
            QualityPairKind::Duplicate => "duplicate-face",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityPair {
    pub first: u32,
    pub second: u32,
    pub kind: QualityPairKind,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QualityReport {
    pub options: QualityOptions,
    pub complete: bool,
    pub results_truncated: bool,
    pub intersections_conclusive: bool,
    pub state: String,
    pub vertices_checked: i64,
    pub triangles_checked: i64,
    pub work: i64,
    pub candidates: i64,
    pub adjacent_pairs_skipped: i64,
    pub reserved_bytes: i64,
    pub area_mm2: QualityStatistics,
    pub min_angle_degrees: QualityStatistics,
    pub aspect_ratio: QualityStatistics,
    pub invalid_vertices: QualityFinding,
    pub invalid_triangles: QualityFinding,
    pub degenerate: QualityFinding,
    pub small_area: QualityFinding,
    pub small_angle: QualityFinding,
    pub high_aspect: QualityFinding,
    pub duplicate_faces: QualityFinding,
    pub crossings: i64,
    pub coplanar_overlaps: i64,
    pub contacts: i64,
    pub uncertain_pairs: i64,
    pub pairs: Vec<QualityPair>,
}
const QUALITY_EXACT_SCRATCH_BYTES: i64 = 16 << 10;
const PER_TRIANGLE_BYTES: i64 = 2 * 56 + 2 * 16;
const PER_RESULT_BYTES: i64 = 7 * 4 + 24;
const ROUNDOFF: f64 = 64.0 * f64::EPSILON;
#[derive(Debug, Clone, Copy, Default)]
struct QualityBox {
    lo: [f64; 3],
    hi: [f64; 3],
    id: u32,
    degenerate: bool,
}
#[derive(Debug, Clone, Copy, Default)]
struct QualityKey {
    v: [u32; 3],
    id: u32,
}
fn limit_error(resource: &str, used: i64, limit: i64) -> MeshError {
    MeshError {
        kind: ErrorKind::BudgetExceeded,
        message: format!("mesh quality: {resource} limit ({used} > {limit})"),
        budget: None,
    }
}
fn quality_options(mut o: QualityOptions) -> (QualityOptions, Option<MeshError>) {
    for v in [
        o.distance_tolerance_mm,
        o.min_area_mm2,
        o.min_angle_degrees,
        o.max_aspect_ratio,
    ] {
        if v.is_nan() || v.is_infinite() || v < 0.0 {
            return (o, Some(MeshError::invalid("invalid quality tolerance")));
        }
    }
    if o.min_angle_degrees > 60.0 || (o.max_aspect_ratio > 0.0 && o.max_aspect_ratio < 1.0) {
        return (o, Some(MeshError::invalid("quality angle/aspect bound")));
    }
    let defaults = [
        10_000_000,
        10_000_000,
        256 << 20,
        20_000_000,
        1_000_000,
        1024,
    ];
    let mut failed = false;
    {
        let limits = [
            &mut o.max_vertices,
            &mut o.max_triangles,
            &mut o.max_bytes,
            &mut o.max_work,
            &mut o.max_candidates,
            &mut o.max_results,
        ];
        for (p, d) in limits.into_iter().zip(defaults) {
            if *p < 0 {
                failed = true;
                break;
            }
            if *p == 0 {
                *p = d;
            }
        }
    }
    if failed {
        return (o, Some(MeshError::invalid("negative quality limit")));
    }
    (o, None)
}
impl QualityReport {
    fn step(&mut self) -> Result<(), MeshError> {
        if self.work >= self.options.max_work {
            self.state = "limited".to_string();
            return Err(limit_error("work", self.work + 1, self.options.max_work));
        }
        self.work += 1;
        Ok(())
    }
    fn pair(&mut self, a: u32, b: u32, kind: QualityPairKind) {
        let (a, b) = if a > b { (b, a) } else { (a, b) };
        match kind {
            QualityPairKind::Crossing => self.crossings += 1,
            QualityPairKind::CoplanarOverlap => self.coplanar_overlaps += 1,
            QualityPairKind::Contact => self.contacts += 1,
            QualityPairKind::Uncertain => {
                self.uncertain_pairs += 1;
                self.intersections_conclusive = false;
            }
            QualityPairKind::Duplicate => {}
        }
        if (self.pairs.len() as i64) < self.options.max_results {
            self.pairs.push(QualityPair {
                first: a,
                second: b,
                kind,
            });
        } else {
            self.results_truncated = true;
        }
    }
}
fn finding(f: &mut QualityFinding, id: u32, max_results: i64) -> bool {
    f.count += 1;
    if (f.indices.len() as i64) < max_results {
        f.indices.push(id);
        false
    } else {
        true
    }
}
fn quality_stat(s: &mut QualityStatistics, x: f64) {
    s.samples += 1;
    if s.samples == 1 {
        s.min = x;
        s.max = x;
    }
    s.min = fmath::min(s.min, x);
    s.max = fmath::max(s.max, x);
    s.mean += (x - s.mean) / s.samples as f64;
}
fn quality_triangle(m: &Mesh, id: u32) -> [[f64; 3]; 3] {
    let t = m.triangles[id as usize];
    [m.point(t[0]), m.point(t[1]), m.point(t[2])]
}
fn quality_merge<T: Copy>(
    r: &mut QualityReport,
    v: &mut [T],
    less: impl Fn(&T, &T) -> bool,
) -> Result<(), MeshError> {
    let n = v.len();
    let mut src: Vec<T> = v.to_vec();
    let mut dst: Vec<T> = v.to_vec();
    let mut width = 1;
    while width < n {
        let mut lo = 0;
        while lo < n {
            let mid = (lo + width).min(n);
            let hi = (mid + width).min(n);
            let (mut i, mut j) = (lo, mid);
            for slot in dst.iter_mut().take(hi).skip(lo) {
                r.step()?;
                if i < mid && (j >= hi || !less(&src[j], &src[i])) {
                    *slot = src[i];
                    i += 1;
                } else {
                    *slot = src[j];
                    j += 1;
                }
            }
            lo += 2 * width;
        }
        std::mem::swap(&mut src, &mut dst);
        if width > n / 2 {
            break;
        }
        width *= 2;
    }
    for (slot, value) in v.iter_mut().zip(&src) {
        r.step()?;
        *slot = *value;
    }
    Ok(())
}
pub fn analyze_quality(
    m: Option<&Mesh>,
    options: QualityOptions,
) -> (QualityReport, Result<(), MeshError>) {
    let (o, invalid) = quality_options(options);
    let mut r = QualityReport {
        options: o,
        state: "invalid".to_string(),
        intersections_conclusive: true,
        ..QualityReport::default()
    };
    let result = match invalid {
        Some(e) => Err(e),
        None => analyze(m, &o, &mut r),
    };
    if !r.complete {
        r.intersections_conclusive = false;
    }
    (r, result)
}
fn analyze(m: Option<&Mesh>, o: &QualityOptions, r: &mut QualityReport) -> Result<(), MeshError> {
    let Some(m) = m else {
        return Err(MeshError::invalid("nil quality context/mesh"));
    };
    if let Some(normals) = &m.normals
        && normals.len() != m.vertices.len()
    {
        return Err(MeshError::invalid("normal count"));
    }
    let (n, nt) = (m.vertices.len() as i64, m.triangles.len() as i64);
    for (name, used, limit) in [
        ("vertices", n, o.max_vertices),
        ("triangles", nt, o.max_triangles),
    ] {
        if used > limit {
            r.state = "limited".to_string();
            return Err(limit_error(name, used, limit));
        }
    }
    if n > i64::from(u32::MAX) || nt > i64::from(u32::MAX) {
        return Err(MeshError::invalid("quality index range"));
    }
    if nt > i64::MAX / PER_TRIANGLE_BYTES
        || o.max_results
            > (i64::MAX - QUALITY_EXACT_SCRATCH_BYTES - nt * PER_TRIANGLE_BYTES) / PER_RESULT_BYTES
    {
        r.state = "limited".to_string();
        return Err(limit_error("bytes", i64::MAX, o.max_bytes));
    }
    r.reserved_bytes =
        nt * PER_TRIANGLE_BYTES + o.max_results * PER_RESULT_BYTES + QUALITY_EXACT_SCRATCH_BYTES;
    if r.reserved_bytes > o.max_bytes {
        r.state = "limited".to_string();
        return Err(limit_error("bytes", r.reserved_bytes, o.max_bytes));
    }
    let max_results = o.max_results;
    for (i, v) in m.vertices.iter().enumerate() {
        r.step()?;
        let mut bad = v.iter().any(|x| !x.is_finite());
        if let Some(normals) = &m.normals {
            bad = bad || normals[i].iter().any(|x| !x.is_finite());
        }
        r.vertices_checked += 1;
        if bad && finding(&mut r.invalid_vertices, i as u32, max_results) {
            r.results_truncated = true;
        }
    }
    for (i, t) in m.triangles.iter().enumerate() {
        r.step()?;
        if t.iter().any(|x| i64::from(*x) >= n)
            && finding(&mut r.invalid_triangles, i as u32, max_results)
        {
            r.results_truncated = true;
        }
    }
    if r.invalid_vertices.count + r.invalid_triangles.count > 0 {
        return Err(MeshError::invalid(
            "nonfinite coordinates/normals or missing indices",
        ));
    }
    r.state = "partial".to_string();
    let mut boxes = vec![QualityBox::default(); nt as usize];
    let mut keys = vec![QualityKey::default(); nt as usize];
    for (i, t) in m.triangles.iter().enumerate() {
        r.step()?;
        let id = i as u32;
        let p = quality_triangle(m, id);
        let mut b = QualityBox {
            lo: p[0],
            hi: p[0],
            id,
            degenerate: false,
        };
        for v in &p[1..] {
            for a in 0..3 {
                b.lo[a] = fmath::min(b.lo[a], v[a]);
                b.hi[a] = fmath::max(b.hi[a], v[a]);
            }
        }
        let c = cross3(sub3(p[1], p[0]), sub3(p[2], p[0]));
        let area = dot3(c, c).sqrt() / 2.0;
        quality_stat(&mut r.area_mm2, area);
        if area == 0.0 {
            b.degenerate = true;
            if finding(&mut r.degenerate, id, max_results) {
                r.results_truncated = true;
            }
            r.intersections_conclusive = false;
        } else {
            let mut min_angle = 180.0;
            let mut max_l2 = 0.0;
            for j in 0..3 {
                let u = sub3(p[(j + 1) % 3], p[j]);
                let v = sub3(p[(j + 2) % 3], p[j]);
                let cr = cross3(u, v);
                let angle =
                    fmath::atan2(dot3(cr, cr).sqrt(), dot3(u, v)) * 180.0 / std::f64::consts::PI;
                min_angle = fmath::min(min_angle, angle);
                max_l2 = fmath::max(max_l2, dot3(u, u));
            }
            let aspect = 3f64.sqrt() * max_l2 / (4.0 * area);
            quality_stat(&mut r.min_angle_degrees, min_angle);
            quality_stat(&mut r.aspect_ratio, aspect);
            if o.min_angle_degrees > 0.0
                && min_angle < o.min_angle_degrees
                && finding(&mut r.small_angle, id, max_results)
            {
                r.results_truncated = true;
            }
            if o.max_aspect_ratio > 0.0
                && aspect > o.max_aspect_ratio
                && finding(&mut r.high_aspect, id, max_results)
            {
                r.results_truncated = true;
            }
        }
        if area < o.min_area_mm2 && finding(&mut r.small_area, id, max_results) {
            r.results_truncated = true;
        }
        boxes[i] = b;
        let mut v = *t;
        if v[0] > v[1] {
            v.swap(0, 1);
        }
        if v[1] > v[2] {
            v.swap(1, 2);
        }
        if v[0] > v[1] {
            v.swap(0, 1);
        }
        keys[i] = QualityKey { v, id };
        r.triangles_checked += 1;
    }
    quality_merge(r, &mut keys, |a, b| {
        if a.v != b.v {
            return a.v < b.v;
        }
        a.id < b.id
    })?;
    for i in 1..keys.len() {
        r.step()?;
        if keys[i].v == keys[i - 1].v {
            if finding(&mut r.duplicate_faces, keys[i].id, max_results) {
                r.results_truncated = true;
            }
            r.pair(keys[i - 1].id, keys[i].id, QualityPairKind::Duplicate);
        }
    }
    quality_merge(r, &mut boxes, |a, b| {
        if a.lo[0] != b.lo[0] {
            return a.lo[0] < b.lo[0];
        }
        a.id < b.id
    })?;
    let tolerance = o.distance_tolerance_mm;
    for i in 0..boxes.len() {
        let a = boxes[i];
        for b in &boxes[i + 1..] {
            r.step()?;
            if b.lo[0] > a.hi[0] + tolerance {
                break;
            }
            if a.degenerate || b.degenerate {
                continue;
            }
            let overlap =
                (1..3).all(|k| a.lo[k] <= b.hi[k] + tolerance && b.lo[k] <= a.hi[k] + tolerance);
            if !overlap {
                continue;
            }
            let (ta, tb) = (m.triangles[a.id as usize], m.triangles[b.id as usize]);
            if ta.iter().any(|x| tb.contains(x)) {
                r.adjacent_pairs_skipped += 1;
                continue;
            }
            if r.candidates >= o.max_candidates {
                r.state = "limited".to_string();
                return Err(limit_error(
                    "candidates",
                    r.candidates + 1,
                    o.max_candidates,
                ));
            }
            r.candidates += 1;
            if let Some(kind) = quality_intersect(
                quality_triangle(m, a.id),
                quality_triangle(m, b.id),
                tolerance,
            ) {
                r.pair(a.id, b.id, kind);
            }
        }
    }
    r.complete = true;
    r.state = if r.intersections_conclusive {
        "complete"
    } else {
        "inconclusive"
    }
    .to_string();
    Ok(())
}
pub(crate) type Triangle = [[f64; 3]; 3];
pub(crate) fn quality_normal(t: &Triangle) -> [f64; 3] {
    let n = cross3(sub3(t[1], t[0]), sub3(t[2], t[0]));
    scale3(n, 1.0 / dot3(n, n).sqrt())
}
pub(crate) fn quality_distances(t: &Triangle, p: [f64; 3], n: [f64; 3]) -> [f64; 3] {
    [
        dot3(sub3(t[0], p), n),
        dot3(sub3(t[1], p), n),
        dot3(sub3(t[2], p), n),
    ]
}
fn quality_separated(d: [f64; 3], tol: f64) -> bool {
    (d[0] > tol && d[1] > tol && d[2] > tol) || (d[0] < -tol && d[1] < -tol && d[2] < -tol)
}
pub(crate) fn quality_roundoff(a: &Triangle, b: &Triangle) -> f64 {
    let mut s = 0.0;
    for t in [a, b] {
        for p in t {
            for x in p {
                s = fmath::max(s, x.abs());
            }
        }
    }
    ROUNDOFF * s
}
pub(crate) fn quality_intersect(a: Triangle, b: Triangle, tol: f64) -> Option<QualityPairKind> {
    let (na, nb) = (quality_normal(&a), quality_normal(&b));
    let (da, db) = (
        quality_distances(&a, b[0], nb),
        quality_distances(&b, a[0], na),
    );
    let eps = quality_roundoff(&a, &b);
    let band = fmath::max(tol, eps);
    if quality_separated(da, band) || quality_separated(db, band) {
        return None;
    }
    let axis = cross3(na, nb);
    let length = dot3(axis, axis).sqrt();
    if length <= ROUNDOFF {
        let mut max_d = 0.0;
        for d in da.iter().chain(&db) {
            max_d = fmath::max(max_d, d.abs());
        }
        if max_d <= eps && quality_exactly_coplanar(&a, &b) {
            return quality_coplanar(&a, &b, na, band);
        }
        if max_d <= band {
            return Some(QualityPairKind::Uncertain);
        }
        return None;
    }
    let axis = scale3(axis, 1.0 / length);
    let (alo, ahi, ac) = quality_plane_interval(&a, da, axis, eps);
    let (blo, bhi, bc) = quality_plane_interval(&b, db, axis, eps);
    if ac == 0 || bc == 0 {
        return Some(QualityPairKind::Uncertain);
    }
    let (lo, hi) = (fmath::max(alo, blo), fmath::min(ahi, bhi));
    if hi < lo - band {
        return None;
    }
    if hi - lo <= band {
        return Some(QualityPairKind::Contact);
    }
    if quality_straddles(da, eps) && quality_straddles(db, eps) {
        return Some(QualityPairKind::Crossing);
    }
    Some(QualityPairKind::Contact)
}
fn quality_straddles(d: [f64; 3], e: f64) -> bool {
    let (mut lo, mut hi) = (d[0], d[0]);
    for x in d {
        lo = fmath::min(lo, x);
        hi = fmath::max(hi, x);
    }
    lo < -e && hi > e
}
pub(crate) fn quality_plane_interval(
    t: &Triangle,
    d: [f64; 3],
    axis: [f64; 3],
    eps: f64,
) -> (f64, f64, i32) {
    let (mut lo, mut hi, mut count) = (f64::INFINITY, f64::NEG_INFINITY, 0);
    let mut add = |p: [f64; 3]| {
        let x = dot3(p, axis);
        lo = fmath::min(lo, x);
        hi = fmath::max(hi, x);
        count += 1;
    };
    for i in 0..3 {
        let j = (i + 1) % 3;
        if d[i].abs() <= eps {
            add(t[i]);
        }
        if (d[i] < -eps && d[j] > eps) || (d[i] > eps && d[j] < -eps) {
            let f = d[i] / (d[i] - d[j]);
            add(add3(t[i], scale3(sub3(t[j], t[i]), f)));
        }
    }
    (lo, hi, count)
}
fn quality_coplanar(a: &Triangle, b: &Triangle, n: [f64; 3], band: f64) -> Option<QualityPairKind> {
    let mut drop = 0;
    for i in 1..3 {
        if n[i].abs() > n[drop].abs() {
            drop = i;
        }
    }
    let (u, v) = ((drop + 1) % 3, (drop + 2) % 3);
    let project = |t: &Triangle| [[t[0][u], t[0][v]], [t[1][u], t[1][v]], [t[2][u], t[2][v]]];
    let (aa, bb) = (project(a), project(b));
    let mut contact = false;
    for t in [aa, bb] {
        for i in 0..3 {
            let j = (i + 1) % 3;
            let (dx, dy) = (t[j][0] - t[i][0], t[j][1] - t[i][1]);
            let l = fmath::hypot(dx, dy);
            if l == 0.0 {
                continue;
            }
            let (x, y) = (-dy / l, dx / l);
            let (mut amin, mut amax) = (f64::INFINITY, f64::NEG_INFINITY);
            let (mut bmin, mut bmax) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..3 {
                let ap = aa[k][0] * x + aa[k][1] * y;
                let bp = bb[k][0] * x + bb[k][1] * y;
                amin = fmath::min(amin, ap);
                amax = fmath::max(amax, ap);
                bmin = fmath::min(bmin, bp);
                bmax = fmath::max(bmax, bp);
            }
            let overlap = fmath::min(amax, bmax) - fmath::max(amin, bmin);
            if overlap < -band {
                return None;
            }
            if overlap <= band {
                contact = true;
            }
        }
    }
    Some(if contact {
        QualityPairKind::Contact
    } else {
        QualityPairKind::CoplanarOverlap
    })
}
#[derive(Clone, Copy, Default)]
struct Wide([u64; 16]);
impl Wide {
    fn dyadic(x: f64) -> Wide {
        let bits = (x as f32).to_bits();
        let exp = (bits >> 23) & 255;
        let mut mant = u64::from(bits & 0x7f_ffff);
        if exp != 0 {
            mant |= 1 << 23;
        }
        let mut out = Wide::default();
        let shift = exp.saturating_sub(1);
        let (limb, bit) = ((shift / 64) as usize, shift % 64);
        out.0[limb] = mant << bit;
        if bit > 40 {
            out.0[limb + 1] = mant >> (64 - bit);
        }
        if bits >> 31 != 0 {
            out = Wide::default().sub(&out);
        }
        out
    }
    fn add(&self, o: &Wide) -> Wide {
        let mut out = Wide::default();
        let mut carry = false;
        for i in 0..16 {
            let (s, c1) = self.0[i].overflowing_add(o.0[i]);
            let (s, c2) = s.overflowing_add(u64::from(carry));
            out.0[i] = s;
            carry = c1 || c2;
        }
        out
    }
    fn sub(&self, o: &Wide) -> Wide {
        let mut out = Wide::default();
        let mut borrow = false;
        for i in 0..16 {
            let (s, b1) = self.0[i].overflowing_sub(o.0[i]);
            let (s, b2) = s.overflowing_sub(u64::from(borrow));
            out.0[i] = s;
            borrow = b1 || b2;
        }
        out
    }
    fn mul(&self, o: &Wide) -> Wide {
        let mut out = Wide::default();
        for i in 0..16 {
            if self.0[i] == 0 {
                continue;
            }
            let mut carry = 0u128;
            for j in 0..16 - i {
                let cur =
                    u128::from(out.0[i + j]) + u128::from(self.0[i]) * u128::from(o.0[j]) + carry;
                out.0[i + j] = cur as u64;
                carry = cur >> 64;
            }
        }
        out
    }
    fn is_zero(&self) -> bool {
        self.0.iter().all(|l| *l == 0)
    }
}
fn quality_orientation_is_zero(t: &Triangle, p: [f64; 3]) -> bool {
    let mut d = [[Wide::default(); 3]; 3];
    for (i, row) in d.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            let v = if i < 2 { t[i + 1][j] } else { p[j] };
            *cell = Wide::dyadic(v).sub(&Wide::dyadic(t[0][j]));
        }
    }
    let mut det = Wide::default();
    for i in 0..3 {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        let cross = d[0][j].mul(&d[1][k]).sub(&d[0][k].mul(&d[1][j]));
        det = det.add(&cross.mul(&d[2][i]));
    }
    det.is_zero()
}
fn quality_exactly_coplanar(a: &Triangle, b: &Triangle) -> bool {
    for axis in 0..3 {
        if (0..3).all(|i| a[i][axis] == a[0][axis] && b[i][axis] == a[0][axis]) {
            return true;
        }
    }
    b.iter().all(|p| quality_orientation_is_zero(a, *p))
}
