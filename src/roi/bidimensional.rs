#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::advanced_geometry::Point2D;
use super::measure_reslice::MeasureSpacing;
use super::measure_subpixel::subpixel_length_mm;
use crate::fmath;
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bidimensional {
    pub long_start: Point2D,
    pub long_end: Point2D,
    pub short_start: Point2D,
    pub short_end: Point2D,
}
pub const BIDIMENSIONAL_LONG_START: i64 = 0;
pub const BIDIMENSIONAL_LONG_END: i64 = 1;
pub const BIDIMENSIONAL_SHORT_START: i64 = 2;
pub const BIDIMENSIONAL_SHORT_END: i64 = 3;
#[derive(Clone, Copy, Default)]
struct Frame {
    a: [f64; 2],
    u: [f64; 2],
    n: [f64; 2],
    length: f64,
    t: f64,
    d0: f64,
    d1: f64,
    sx: f64,
    sy: f64,
}
fn bidimensional_scale(sp: MeasureSpacing) -> (f64, f64) {
    if sp.valid() {
        return sp.spacing_xy();
    }
    (1.0, 1.0)
}
fn new_bidimensional_axis(start: Point2D, end: Point2D, sp: MeasureSpacing) -> Option<Frame> {
    let (sx, sy) = bidimensional_scale(sp);
    let mut f = Frame {
        sx,
        sy,
        a: [start.x * sx, start.y * sy],
        ..Frame::default()
    };
    let (dx, dy) = ((end.x - start.x) * sx, (end.y - start.y) * sy);
    f.length = fmath::hypot(dx, dy);
    if !(f.length > 0.0) || f.length.is_infinite() {
        return None;
    }
    f.u = [dx / f.length, dy / f.length];
    f.n = [-f.u[1], f.u[0]];
    Some(f)
}
impl Frame {
    fn mm(&self, p: Point2D) -> [f64; 2] {
        [p.x * self.sx, p.y * self.sy]
    }
    fn along(&self, p: Point2D) -> f64 {
        let q = self.mm(p);
        (q[0] - self.a[0]) * self.u[0] + (q[1] - self.a[1]) * self.u[1]
    }
    fn across(&self, p: Point2D) -> f64 {
        let q = self.mm(p);
        (q[0] - self.a[0]) * self.n[0] + (q[1] - self.a[1]) * self.n[1]
    }
    fn point(&self, along: f64, across: f64) -> Point2D {
        let x = self.a[0] + self.u[0] * along + self.n[0] * across;
        let y = self.a[1] + self.u[1] * along + self.n[1] * across;
        Point2D::new(x / self.sx, y / self.sy)
    }
    fn foot(&self) -> f64 {
        self.length * fmath::max(0.0, fmath::min(1.0, self.t))
    }
    fn build(&self) -> Bidimensional {
        let foot = self.foot();
        Bidimensional {
            long_start: self.point(0.0, 0.0),
            long_end: self.point(self.length, 0.0),
            short_start: self.point(foot, self.d0),
            short_end: self.point(foot, self.d1),
        }
    }
    fn short_axis_usable(&self) -> bool {
        (self.d1 - self.d0).abs() > 1e-6 * fmath::max(1.0, self.length)
    }
}
fn finite_all(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}
impl Bidimensional {
    fn frame(&self, sp: MeasureSpacing) -> Option<Frame> {
        let mut f = new_bidimensional_axis(self.long_start, self.long_end, sp)?;
        f.t = (f.along(self.short_start) + f.along(self.short_end)) / 2.0 / f.length;
        f.d0 = f.across(self.short_start);
        f.d1 = f.across(self.short_end);
        finite_all(&[f.t, f.d0, f.d1]).then_some(f)
    }
    pub fn constrained(&self, sp: MeasureSpacing) -> Option<Bidimensional> {
        let mut f = self.frame(sp)?;
        if f.d0 * f.d1 > 0.0 {
            if f.d0.abs() < f.d1.abs() {
                f.d0 = 0.0;
            } else {
                f.d1 = 0.0;
            }
        }
        if !f.short_axis_usable() {
            return None;
        }
        let mut out = f.build();
        out.long_start = self.long_start;
        out.long_end = self.long_end;
        Some(out)
    }
    pub fn intersection(&self, sp: MeasureSpacing) -> Option<Point2D> {
        let f = self.frame(sp)?;
        Some(f.point(f.foot(), 0.0))
    }
    pub fn perpendicularity_error(&self, sp: MeasureSpacing) -> f64 {
        let Some(f) = self.frame(sp) else {
            return f64::NAN;
        };
        let (s0, s1) = (f.mm(self.short_start), f.mm(self.short_end));
        let (dx, dy) = (s1[0] - s0[0], s1[1] - s0[1]);
        let length = fmath::hypot(dx, dy);
        if length == 0.0 {
            return f64::NAN;
        }
        let cosine = (dx * f.u[0] + dy * f.u[1]).abs() / length;
        90.0 - fmath::acos(fmath::min(1.0, cosine)) * 180.0 / PI
    }
    pub fn points(&self) -> Vec<Point2D> {
        vec![
            self.long_start,
            self.long_end,
            self.short_start,
            self.short_end,
        ]
    }
}
pub fn new_bidimensional(
    long_start: Point2D,
    long_end: Point2D,
    short_point: Point2D,
    sp: MeasureSpacing,
) -> Option<Bidimensional> {
    let mut f = new_bidimensional_axis(long_start, long_end, sp)?;
    let d = f.across(short_point);
    f.t = f.along(short_point) / f.length;
    f.d0 = d;
    f.d1 = -d;
    if !f.short_axis_usable() || !finite_all(&[d, f.t]) {
        return None;
    }
    let mut out = f.build();
    out.long_start = long_start;
    out.long_end = long_end;
    Some(out)
}
pub fn move_bidimensional_handle(
    b: Bidimensional,
    handle: i64,
    target: Point2D,
    sp: MeasureSpacing,
) -> Option<Bidimensional> {
    if !finite_all(&[target.x, target.y]) {
        return None;
    }
    let mut f = b.frame(sp)?;
    match handle {
        BIDIMENSIONAL_LONG_START | BIDIMENSIONAL_LONG_END => {
            let (mut start, mut end) = (b.long_start, b.long_end);
            if handle == BIDIMENSIONAL_LONG_START {
                start = target;
            } else {
                end = target;
            }
            let mut g = new_bidimensional_axis(start, end, sp)?;
            g.t = f.t;
            g.d0 = f.d0;
            g.d1 = f.d1;
            let mut out = g.build();
            out.long_start = start;
            out.long_end = end;
            Some(out)
        }
        BIDIMENSIONAL_SHORT_START | BIDIMENSIONAL_SHORT_END => {
            let mut d = f.across(target);
            let other = if handle == BIDIMENSIONAL_SHORT_END {
                f.d0
            } else {
                f.d1
            };
            if d * other > 0.0 {
                d = 0.0;
            }
            if handle == BIDIMENSIONAL_SHORT_START {
                f.d0 = d;
            } else {
                f.d1 = d;
            }
            if !f.short_axis_usable() {
                return None;
            }
            let mut out = f.build();
            out.long_start = b.long_start;
            out.long_end = b.long_end;
            Some(out)
        }
        _ => None,
    }
}
pub fn slide_bidimensional_short_axis(
    b: Bidimensional,
    target: Point2D,
    sp: MeasureSpacing,
) -> Option<Bidimensional> {
    let mut f = b.frame(sp)?;
    if !finite_all(&[target.x, target.y]) {
        return None;
    }
    f.t = f.along(target) / f.length;
    let mut out = f.build();
    out.long_start = b.long_start;
    out.long_end = b.long_end;
    Some(out)
}
pub fn bidimensional_from_points(points: &[Point2D]) -> Option<Bidimensional> {
    if points.len() != 4 {
        return None;
    }
    Some(Bidimensional {
        long_start: points[0],
        long_end: points[1],
        short_start: points[2],
        short_end: points[3],
    })
}
pub fn bidimensional_axes_mm(b: Bidimensional, sp: MeasureSpacing) -> (f64, f64, bool) {
    let long = subpixel_length_mm(b.long_start, b.long_end, sp);
    let short = subpixel_length_mm(b.short_start, b.short_end, sp);
    (
        long.unwrap_or(0.0),
        short.unwrap_or(0.0),
        long.is_some() && short.is_some(),
    )
}
