#![forbid(unsafe_code)]
use super::cpr_limits::{
    CprError, MAX_CPR_CONTROL_POINTS, MAX_CPR_PATH_LENGTH_MM, MAX_CPR_PATH_SAMPLES,
};
use super::cpr_path_assistant::{clamp_int, finite_vec3, positive_finite};
use super::vec3::Vec3;
use crate::fmath;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CprPathSample {
    pub arc_length: f64,
    pub position: Vec3,
    pub tangent: Vec3,
    pub normal: Vec3,
    pub binormal: Vec3,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CprPath {
    points: Vec<Vec3>,
    spans: Vec<CprSpan>,
    knot_arc: Vec<f64>,
    frames: Vec<CprFrame>,
    length: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct CprSpan {
    a: Vec3,
    b: Vec3,
    c: Vec3,
    d: Vec3,
}
impl CprSpan {
    fn point(&self, t: f64) -> Vec3 {
        self.a
            .scale(t)
            .add(self.b)
            .scale(t)
            .add(self.c)
            .scale(t)
            .add(self.d)
    }
    fn velocity(&self, t: f64) -> Vec3 {
        self.a
            .scale(3.0 * t)
            .add(self.b.scale(2.0))
            .scale(t)
            .add(self.c)
    }
    fn linear(&self) -> bool {
        self.a == Vec3::ZERO && self.b == Vec3::ZERO
    }
    fn arc_length(&self, t0: f64, t1: f64) -> f64 {
        let half = (t1 - t0) / 2.0;
        let mid = (t1 + t0) / 2.0;
        let mut sum = 0.0;
        for (k, x) in CPR_GAUSS_NODES.iter().enumerate() {
            sum += CPR_GAUSS_WEIGHTS[k] * self.velocity(mid + half * x).length();
        }
        sum * half
    }
    fn invert_arc_length(
        &self,
        t_lo: f64,
        t_hi: f64,
        target: f64,
        guess: f64,
        tolerance: f64,
    ) -> f64 {
        let (mut lo, mut hi, mut t) = (t_lo, t_hi, guess);
        for _ in 0..8 {
            let f = self.arc_length(t_lo, t) - target;
            if f.abs() <= tolerance {
                break;
            }
            if f > 0.0 {
                hi = t;
            } else {
                lo = t;
            }
            let mut next = (lo + hi) / 2.0;
            let speed = self.velocity(t).length();
            if speed > 0.0 {
                let newton = t - f / speed;
                if newton > lo && newton < hi {
                    next = newton;
                }
            }
            t = next;
        }
        t
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct CprFrame {
    s: f64,
    span: usize,
    t: f64,
    pos: Vec3,
    tangent: Vec3,
    normal: Vec3,
}
const CPR_TABLE_STEP_MM: f64 = 0.5;
const CPR_TABLE_MIN_PER_SPAN: i64 = 4;
const CPR_TABLE_MAX_PER_SPAN: i64 = 64;
const CPR_TABLE_MAX_ENTRIES: i64 = 1 << 19;
#[allow(clippy::excessive_precision)]
const CPR_GAUSS_NODES: [f64; 5] = [
    -0.9061798459386640,
    -0.5384693101056831,
    0.0,
    0.5384693101056831,
    0.9061798459386640,
];
#[allow(clippy::excessive_precision)]
const CPR_GAUSS_WEIGHTS: [f64; 5] = [
    0.2369268850561891,
    0.4786286704993665,
    0.5688888888888889,
    0.4786286704993665,
    0.2369268850561891,
];
fn f64_to_u64_amd64(x: f64) -> u64 {
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    if x < TWO63 {
        fmath::f64_to_i64(x) as u64
    } else {
        (fmath::f64_to_i64(x - TWO63) as u64) | (1 << 63)
    }
}
pub fn new_cpr_path(control_points: &[Vec3]) -> Option<CprPath> {
    new_cpr_path_checked(control_points).ok()
}
pub fn new_cpr_path_checked(control_points: &[Vec3]) -> Result<CprPath, CprError> {
    if control_points.len() as u64 > MAX_CPR_CONTROL_POINTS {
        return Err(CprError::limit(
            "control_points",
            control_points.len() as u64,
            MAX_CPR_CONTROL_POINTS,
        ));
    }
    let mut pts: Vec<Vec3> = Vec::with_capacity(control_points.len());
    for (index, p) in control_points.iter().enumerate() {
        if !finite_vec3(*p) {
            return Err(CprError::input(
                format!("control_points[{index}]"),
                "coordinate is not finite",
            ));
        }
        if pts.last().is_some_and(|last| last == p) {
            continue;
        }
        pts.push(*p);
    }
    if pts.len() < 2 {
        return Err(CprError::input(
            "control_points",
            "at least two distinct points are required",
        ));
    }
    let mut chord_length = 0.0;
    for i in 1..pts.len() {
        let segment = pts[i].sub(pts[i - 1]).length();
        if !positive_finite(segment) {
            return Err(CprError::input(
                format!("segment[{}]", i - 1),
                "length is zero or not finite",
            ));
        }
        chord_length += segment;
        if !positive_finite(chord_length) {
            return Err(CprError::input("path_length", "length is not finite"));
        }
        if chord_length > MAX_CPR_PATH_LENGTH_MM as f64 {
            return Err(CprError::limit(
                "path_length_mm",
                f64_to_u64_amd64(chord_length.ceil()),
                MAX_CPR_PATH_LENGTH_MM,
            ));
        }
    }
    let spans = centripetal_catmull_rom_spans(&pts);
    let mut path = CprPath {
        points: pts,
        spans,
        knot_arc: Vec::new(),
        frames: Vec::new(),
        length: 0.0,
    };
    path.build_frame_table()?;
    Ok(path)
}
fn centripetal_catmull_rom_spans(pts: &[Vec3]) -> Vec<CprSpan> {
    let n = pts.len();
    let mut spans = vec![CprSpan::default(); n - 1];
    for i in 0..n - 1 {
        let d = pts[i + 1].sub(pts[i]);
        let (mut prev, mut next) = (d, d);
        if i > 0 {
            prev = pts[i].sub(pts[i - 1]);
        }
        if i + 2 < n {
            next = pts[i + 2].sub(pts[i + 1]);
        }
        let (m1, m2) = centripetal_catmull_rom_tangents(prev, d, next);
        let (e1, e2) = (m1.sub(d), m2.sub(d));
        spans[i] = CprSpan {
            a: e1.add(e2),
            b: e1.scale(-2.0).sub(e2),
            c: m1,
            d: pts[i],
        };
    }
    spans
}
pub fn centripetal_catmull_rom_tangents(prev: Vec3, chord: Vec3, next: Vec3) -> (Vec3, Vec3) {
    let k0 = prev.length().sqrt();
    let k1 = chord.length().sqrt();
    let k2 = next.length().sqrt();
    let start = prev
        .scale(k1 / k0)
        .sub(prev.add(chord).scale(k1 / (k0 + k1)))
        .add(chord);
    let end = chord
        .sub(chord.add(next).scale(k1 / (k1 + k2)))
        .add(next.scale(k1 / k2));
    (start, end)
}
impl CprPath {
    fn build_frame_table(&mut self) -> Result<(), CprError> {
        let span_count = self.spans.len() as i64;
        let per_span_max = clamp_int(
            CPR_TABLE_MAX_ENTRIES / span_count,
            2,
            CPR_TABLE_MAX_PER_SPAN,
        );
        let per_span_min = CPR_TABLE_MIN_PER_SPAN.min(per_span_max);
        let mut counts = vec![0i64; self.spans.len()];
        let mut total = 1usize;
        for i in 0..self.spans.len() {
            let chord = self.points[i + 1].sub(self.points[i]).length();
            let mut steps = per_span_max;
            let want = (chord / CPR_TABLE_STEP_MM).ceil();
            if want < per_span_max as f64 {
                steps = per_span_min.max(fmath::f64_to_i64(want));
            }
            counts[i] = steps;
            total += steps as usize;
        }
        let mut frames: Vec<CprFrame> = Vec::with_capacity(total);
        let mut knot_arc = vec![0f64; self.points.len()];
        let tangent = self.tangent_at_param(0, 0.0);
        frames.push(CprFrame {
            pos: self.points[0],
            tangent,
            normal: initial_normal(tangent),
            ..Default::default()
        });
        let mut s = 0.0;
        for i in 0..self.spans.len() {
            let span = self.spans[i];
            let steps = counts[i];
            let span_start = s;
            let speed = span.c.length();
            for j in 1..=steps {
                let t0 = (j - 1) as f64 / steps as f64;
                let t1 = j as f64 / steps as f64;
                if span.linear() {
                    s = span_start + speed * t1;
                } else {
                    s += span.arc_length(t0, t1);
                }
                let mut pos = span.point(t1);
                if j == steps {
                    pos = self.points[i + 1];
                }
                let prev = frames[frames.len() - 1];
                let tangent = self.tangent_at_param(i, t1);
                let normal =
                    rotation_minimizing_normal(prev.pos, prev.tangent, prev.normal, pos, tangent);
                frames.push(CprFrame {
                    s,
                    span: i,
                    t: t1,
                    pos,
                    tangent,
                    normal,
                });
            }
            if !positive_finite(s) || s <= span_start {
                return Err(CprError::input(
                    format!("segment[{i}]"),
                    "arc length is zero or not finite",
                ));
            }
            if s > MAX_CPR_PATH_LENGTH_MM as f64 {
                return Err(CprError::limit(
                    "path_length_mm",
                    f64_to_u64_amd64(s.ceil()),
                    MAX_CPR_PATH_LENGTH_MM,
                ));
            }
            knot_arc[i + 1] = s;
        }
        self.frames = frames;
        self.knot_arc = knot_arc;
        self.length = s;
        Ok(())
    }
    fn tangent_at_param(&self, i: usize, t: f64) -> Vec3 {
        let span = &self.spans[i];
        let tangent = span.velocity(t).normalize();
        if tangent != Vec3::ZERO {
            return tangent;
        }
        const H: f64 = 1e-6;
        let tangent = span
            .point(fmath::min(1.0, t + H))
            .sub(span.point(fmath::max(0.0, t - H)))
            .normalize();
        if tangent != Vec3::ZERO {
            return tangent;
        }
        self.points[i + 1].sub(self.points[i]).normalize()
    }
    pub fn length(&self) -> f64 {
        self.length
    }
    pub fn control_point_count(&self) -> usize {
        self.points.len()
    }
    pub fn control_point_arc_lengths(&self) -> Vec<f64> {
        self.knot_arc.clone()
    }
    pub fn point_at(&self, s: f64) -> Vec3 {
        let (k, span, t, exact) = self.locate(s);
        if exact {
            return self.frames[k].pos;
        }
        self.spans[span].point(t)
    }
    pub fn tangent_at(&self, s: f64) -> Vec3 {
        let (k, span, t, exact) = self.locate(s);
        if exact {
            return self.frames[k].tangent;
        }
        self.tangent_at_param(span, t)
    }
    fn locate(&self, s: f64) -> (usize, usize, f64, bool) {
        let s = self.clamp_arc(s);
        let last = self.frames.len() - 1;
        let (mut lo_i, mut hi_i) = (0usize, self.frames.len());
        while lo_i < hi_i {
            let h = lo_i + (hi_i - lo_i) / 2;
            if !(self.frames[h].s > s) {
                lo_i = h + 1;
            } else {
                hi_i = h;
            }
        }
        let k = lo_i.saturating_sub(1);
        let lo = self.frames[k];
        if k >= last || lo.s == s {
            return (k, lo.span, lo.t, true);
        }
        let hi = self.frames[k + 1];
        let mut t_lo = lo.t;
        if lo.span != hi.span {
            t_lo = 0.0;
        }
        let ds = hi.s - lo.s;
        let mut frac = 0.0;
        if ds > 0.0 {
            frac = (s - lo.s) / ds;
        }
        let mut t = t_lo + (hi.t - t_lo) * frac;
        let sp = &self.spans[hi.span];
        if !sp.linear() {
            t = sp.invert_arc_length(t_lo, hi.t, s - lo.s, t, 1e-12 * fmath::max(1.0, ds));
        }
        (k, hi.span, t, false)
    }
    fn clamp_arc(&self, s: f64) -> f64 {
        if s <= 0.0 || s.is_nan() {
            return 0.0;
        }
        if s > self.length {
            return self.length;
        }
        s
    }
    pub fn resample(&self, spacing: f64) -> Option<Vec<CprPathSample>> {
        self.resample_checked(spacing).ok()
    }
    pub fn resample_checked(&self, spacing: f64) -> Result<Vec<CprPathSample>, CprError> {
        let sample_count = self.sample_count_checked(spacing)?;
        let regular_count = fmath::f64_to_i64((self.length / spacing).floor()) + 1;
        let mut samples: Vec<CprPathSample> = Vec::with_capacity(sample_count as usize);
        for k in 0..regular_count {
            samples.push(self.frame_at(k as f64 * spacing));
        }
        if let Some(last) = samples.last()
            && self.length - last.arc_length > 1e-9
        {
            samples.push(self.frame_at(self.length));
        }
        Ok(samples)
    }
    pub fn sample_count_checked(&self, spacing: f64) -> Result<i64, CprError> {
        if !positive_finite(spacing) {
            return Err(CprError::input(
                "spacing",
                "spacing must be positive and finite",
            ));
        }
        let regular_count = (self.length / spacing).floor() + 1.0;
        if !positive_finite(regular_count) {
            return Err(CprError::input(
                "sample_count",
                "sample count is not finite",
            ));
        }
        let mut count = regular_count;
        let last = (regular_count - 1.0) * spacing;
        if self.length - last > 1e-9 {
            count += 1.0;
        }
        if count > MAX_CPR_PATH_SAMPLES as f64 {
            return Err(CprError::limit(
                "path_samples",
                MAX_CPR_PATH_SAMPLES + 1,
                MAX_CPR_PATH_SAMPLES,
            ));
        }
        if count < 1.0 {
            return Err(CprError::input(
                "sample_count",
                "sample count is less than one",
            ));
        }
        Ok(fmath::f64_to_i64(count))
    }
    pub fn frame_at(&self, s: f64) -> CprPathSample {
        let s = self.clamp_arc(s);
        let (k, span, t, exact) = self.locate(s);
        let lo = self.frames[k];
        let (mut position, mut tangent, mut normal) = (lo.pos, lo.tangent, lo.normal);
        if !exact {
            position = self.spans[span].point(t);
            tangent = self.tangent_at_param(span, t);
            normal = rotation_minimizing_normal(lo.pos, lo.tangent, lo.normal, position, tangent);
        }
        CprPathSample {
            arc_length: s,
            position,
            tangent,
            normal,
            binormal: tangent.cross(normal).normalize(),
        }
    }
}
fn initial_normal(tangent: Vec3) -> Vec3 {
    let mut reference = Vec3::new(1.0, 0.0, 0.0);
    if tangent.x.abs() > 0.9 {
        reference = Vec3::new(0.0, 1.0, 0.0);
    }
    let mut n = reference
        .sub(tangent.scale(reference.dot(tangent)))
        .normalize();
    if n == Vec3::ZERO {
        reference = Vec3::new(0.0, 0.0, 1.0);
        n = reference
            .sub(tangent.scale(reference.dot(tangent)))
            .normalize();
    }
    n
}
fn rotation_minimizing_normal(
    from_pos: Vec3,
    from_tangent: Vec3,
    from_normal: Vec3,
    to_pos: Vec3,
    to_tangent: Vec3,
) -> Vec3 {
    let (mut reflected_normal, mut reflected_tangent) = (from_normal, from_tangent);
    let chord = to_pos.sub(from_pos);
    let c1 = chord.dot(chord);
    if c1 > 0.0 {
        reflected_normal = from_normal.sub(chord.scale(2.0 * chord.dot(from_normal) / c1));
        reflected_tangent = from_tangent.sub(chord.scale(2.0 * chord.dot(from_tangent) / c1));
    }
    let mut normal = reflected_normal;
    let v2 = to_tangent.sub(reflected_tangent);
    let c2 = v2.dot(v2);
    if c2 > 0.0 {
        normal = reflected_normal.sub(v2.scale(2.0 * v2.dot(reflected_normal) / c2));
    }
    let mut binormal = to_tangent.cross(normal).normalize();
    if binormal == Vec3::ZERO {
        binormal = to_tangent
            .cross(parallel_transport(from_normal, from_tangent, to_tangent))
            .normalize();
        if binormal == Vec3::ZERO {
            return initial_normal(to_tangent);
        }
    }
    binormal.cross(to_tangent).normalize()
}
fn parallel_transport(normal: Vec3, prev_tangent: Vec3, tangent: Vec3) -> Vec3 {
    let axis = prev_tangent.cross(tangent);
    let axis_len = axis.length();
    if axis_len < 1e-9 {
        return normal;
    }
    let axis = axis.scale(1.0 / axis_len);
    let angle = fmath::atan2(axis_len, prev_tangent.dot(tangent));
    normal.rotate(axis, angle)
}
