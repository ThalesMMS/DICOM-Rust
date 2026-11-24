#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::mpr::Plane;
use super::mpr_render::valid_mpr_plane;
use super::vec3::Vec3;
use crate::fmath;
pub const MAX_VIEW_OUTPUT_DIMENSION: u64 = 8192;
pub const MAX_VIEW_OUTPUT_BYTES: u64 = 256 << 20;
pub const MAX_PLANE_SWEEP_COUNT: i64 = 512;
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn view_state_error(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidViewState,
        format!("render: invalid view state: {detail}"),
    )
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaneSweepKind(pub String);
impl PlaneSweepKind {
    pub fn parallel() -> PlaneSweepKind {
        PlaneSweepKind("parallel".into())
    }
    pub fn rotational() -> PlaneSweepKind {
        PlaneSweepKind("rotational".into())
    }
    fn is_parallel(&self) -> bool {
        self.0 == "parallel"
    }
    fn is_rotational(&self) -> bool {
        self.0 == "rotational"
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaneSweepOptions {
    pub kind: PlaneSweepKind,
    pub base: Plane,
    pub start: f64,
    pub end: f64,
    pub step: f64,
    pub axis: Vec3,
    pub pivot: Vec3,
    pub slice_thickness_mm: f64,
    pub max_planes: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneSweep {
    options: PlaneSweepOptions,
    count: i64,
    direction: f64,
    normal: Vec3,
}
fn finite_sweep_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
fn valid_sweep_plane(p: &Plane) -> bool {
    let (u, v) = (p.u.length(), p.v.length());
    finite_sweep_vec(p.origin)
        && finite_sweep_vec(p.u)
        && finite_sweep_vec(p.v)
        && finite(u)
        && finite(v)
        && u > 1e-12
        && v > 1e-12
        && p.u.normalize().dot(p.v.normalize()).abs() <= 1e-6
}
impl PlaneSweep {
    pub fn new(options: PlaneSweepOptions) -> Result<PlaneSweep, RenderError> {
        let fail = |detail: &str| Err(view_state_error(format!("plane sweep {detail}")));
        let mut o = options;
        if !o.kind.is_parallel() && !o.kind.is_rotational() {
            return fail("kind");
        }
        if !valid_sweep_plane(&o.base)
            || !finite(o.start)
            || !finite(o.end)
            || !finite(o.step)
            || o.step <= 0.0
            || !finite(o.slice_thickness_mm)
            || o.slice_thickness_mm <= 0.0
            || o.slice_thickness_mm > 1000.0
        {
            return fail("geometry, interval or thickness");
        }
        if o.max_planes == 0 {
            o.max_planes = MAX_PLANE_SWEEP_COUNT;
        }
        if o.max_planes < 1 || o.max_planes > MAX_PLANE_SWEEP_COUNT {
            return fail("count cap");
        }
        let mut bound = 10000.0;
        if o.kind.is_rotational() {
            bound = 360.0;
            if !finite_sweep_vec(o.axis)
                || !finite_sweep_vec(o.pivot)
                || o.axis.length() < 1e-12
                || !finite(o.axis.length())
            {
                return fail("rotation axis or pivot");
            }
            o.axis = o.axis.normalize();
        }
        let span = (o.end - o.start).abs();
        if o.start.abs() > bound || o.end.abs() > bound || span > bound || o.step > 2.0 * bound {
            return fail("physical/angular range");
        }
        let mut intervals = span / o.step;
        if !finite(intervals) || intervals > (o.max_planes - 1) as f64 {
            return fail("exceeds count cap");
        }
        let nearest = intervals.round();
        if (intervals - nearest).abs() <= 1e-10 {
            intervals = nearest;
        }
        let count = fmath::f64_to_i64(intervals.ceil()) + 1;
        if count > o.max_planes {
            return fail("exceeds count cap");
        }
        let direction = if o.end < o.start { -1.0 } else { 1.0 };
        let normal = o.base.u.normalize().cross(o.base.v.normalize()).normalize();
        let sweep = PlaneSweep {
            options: o,
            count,
            direction,
            normal,
        };
        for i in 0..count {
            if sweep.plane(i).is_none() {
                return fail("generated geometry is not finite");
            }
        }
        Ok(sweep)
    }
    pub fn count(&self) -> i64 {
        self.count
    }
    pub fn options(&self) -> &PlaneSweepOptions {
        &self.options
    }
    pub fn position(&self, index: i64) -> Option<f64> {
        if index < 0 || index >= self.count {
            return None;
        }
        if index == self.count - 1 {
            return Some(self.options.end);
        }
        Some(self.options.start + self.direction * index as f64 * self.options.step)
    }
    pub fn plane_with_validity(&self, index: i64) -> Option<(Plane, bool)> {
        let position = self.position(index)?;
        let o = &self.options;
        if o.kind.is_parallel() {
            let mut out = o.base;
            out.origin = out.origin.add(self.normal.scale(position));
            return Some((out, valid_sweep_plane(&out)));
        }
        let angle = position * std::f64::consts::PI / 180.0;
        let out = Plane {
            origin: o
                .pivot
                .add(o.base.origin.sub(o.pivot).rotate(o.axis, angle)),
            u: o.base.u.rotate(o.axis, angle),
            v: o.base.v.rotate(o.axis, angle),
        };
        Some((out, valid_sweep_plane(&out)))
    }
    pub fn plane(&self, index: i64) -> Option<Plane> {
        self.plane_with_validity(index)
            .and_then(|(p, ok)| ok.then_some(p))
    }
}
pub fn plane_cross_section(reference: &Plane, target: &Plane) -> ([[f64; 2]; 2], bool, bool) {
    let mut line = [[0.0; 2]; 2];
    if !valid_sweep_plane(reference) || !valid_sweep_plane(target) {
        return (line, false, false);
    }
    let n = target.u.normalize().cross(target.v.normalize()).normalize();
    let (a, b, c) = (
        n.dot(reference.u),
        n.dot(reference.v),
        n.dot(reference.origin.sub(target.origin)),
    );
    let tolerance = 1e-9 * fmath::max(1.0, fmath::max(reference.u.length(), reference.v.length()));
    if a.abs() <= tolerance && b.abs() <= tolerance {
        return (line, false, c.abs() <= tolerance);
    }
    let mut points = [[0.0f64; 2]; 4];
    let mut count = 0usize;
    let mut add = |x: f64, y: f64| {
        if !finite(x) || !finite(y) || x < -1e-9 || x > 1.0 + 1e-9 || y < -1e-9 || y > 1.0 + 1e-9 {
            return;
        }
        let x = fmath::max(0.0, fmath::min(1.0, x));
        let y = fmath::max(0.0, fmath::min(1.0, y));
        for p in points.iter().take(count) {
            if fmath::hypot(p[0] - x, p[1] - y) < 1e-9 {
                return;
            }
        }
        if count < points.len() {
            points[count] = [x, y];
            count += 1;
        }
    };
    if b.abs() > tolerance {
        add(0.0, -c / b);
        add(1.0, -(a + c) / b);
    }
    if a.abs() > tolerance {
        add(-c / a, 0.0);
        add(-(b + c) / a, 1.0);
    }
    if count < 2 {
        return (line, false, false);
    }
    line = [points[0], points[1]];
    (line, true, false)
}
pub const VIEWPORT_FIT_CONTAIN: u8 = 0;
pub const VIEWPORT_FIT_COVER: u8 = 1;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewportResliceRequest {
    pub source_width: i64,
    pub source_height: i64,
    pub output_width: i64,
    pub output_height: i64,
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
    pub rotation: f64,
    pub fit: u8,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewportReslicePlan {
    pub plane: Plane,
    pub width: i64,
    pub height: i64,
    request: ViewportResliceRequest,
    scale: f64,
}
pub fn plan_viewport_reslice(
    source: &Plane,
    request: ViewportResliceRequest,
) -> Result<ViewportReslicePlan, RenderError> {
    let invalid = |field: &str| Err(view_state_error(format!("viewport {field}")));
    let mut r = request;
    if !valid_mpr_plane(source) {
        return invalid("source plane");
    }
    if r.source_width < 2 || r.source_height < 2 || r.output_width < 2 || r.output_height < 2 {
        return invalid("dimensions require at least two pixel centres");
    }
    let (ow, oh) = (r.output_width as u64, r.output_height as u64);
    if ow > MAX_VIEW_OUTPUT_DIMENSION
        || oh > MAX_VIEW_OUTPUT_DIMENSION
        || ow > MAX_VIEW_OUTPUT_BYTES / 4 / oh
    {
        return invalid("output limits");
    }
    if !finite(r.viewport_width)
        || !finite(r.viewport_height)
        || r.viewport_width <= 0.0
        || r.viewport_height <= 0.0
    {
        return invalid("viewport size");
    }
    if r.zoom == 0.0 {
        r.zoom = 1.0;
    }
    if !finite(r.zoom)
        || r.zoom <= 0.0
        || !finite(r.pan_x)
        || !finite(r.pan_y)
        || !finite(r.rotation)
    {
        return invalid("transform");
    }
    let sx = r.viewport_width / r.source_width as f64;
    let sy = r.viewport_height / r.source_height as f64;
    let mut scale = fmath::min(sx, sy);
    match r.fit {
        VIEWPORT_FIT_CONTAIN => {}
        VIEWPORT_FIT_COVER => scale = fmath::max(sx, sy),
        _ => return invalid("fit"),
    }
    if !finite(scale) || scale <= 0.0 || !finite(scale * r.zoom) || scale * r.zoom <= 0.0 {
        return invalid("transform scale");
    }
    let mut plan = ViewportReslicePlan {
        plane: Plane::default(),
        width: r.output_width,
        height: r.output_height,
        request: r,
        scale,
    };
    let (x0, y0, ok) = plan.output_to_source(0.0, 0.0);
    if !ok {
        return invalid("origin");
    }
    let (x1, y1, ok) = plan.output_to_source((r.output_width - 1) as f64, 0.0);
    if !ok {
        return invalid("horizontal edge");
    }
    let (x2, y2, ok) = plan.output_to_source(0.0, (r.output_height - 1) as f64);
    if !ok {
        return invalid("vertical edge");
    }
    let point = |x: f64, y: f64| {
        source.at(
            x / (r.source_width - 1) as f64,
            y / (r.source_height - 1) as f64,
        )
    };
    let origin = point(x0, y0);
    plan.plane = Plane {
        origin,
        u: point(x1, y1).sub(origin),
        v: point(x2, y2).sub(origin),
    };
    if !valid_mpr_plane(&plan.plane) {
        return invalid("result plane");
    }
    Ok(plan)
}
impl ViewportReslicePlan {
    pub fn output_to_source(&self, x: f64, y: f64) -> (f64, f64, bool) {
        if self.scale <= 0.0 || self.width < 2 || self.height < 2 || !finite(x) || !finite(y) {
            return (0.0, 0.0, false);
        }
        let r = &self.request;
        let dx = ((x + 0.5) * r.viewport_width / self.width as f64
            - r.viewport_width / 2.0
            - r.pan_x * self.scale)
            / (r.zoom * self.scale);
        let dy = ((y + 0.5) * r.viewport_height / self.height as f64
            - r.viewport_height / 2.0
            - r.pan_y * self.scale)
            / (r.zoom * self.scale);
        let (c, s) = (fmath::cos(r.rotation), fmath::sin(r.rotation));
        let sx = (r.source_width - 1) as f64 / 2.0 + dx * c + dy * s;
        let sy = (r.source_height - 1) as f64 / 2.0 - dx * s + dy * c;
        (sx, sy, finite(sx) && finite(sy))
    }
    pub fn source_to_output(&self, x: f64, y: f64) -> (f64, f64, bool) {
        if self.scale <= 0.0 || self.width < 2 || self.height < 2 || !finite(x) || !finite(y) {
            return (0.0, 0.0, false);
        }
        let r = &self.request;
        let dx = x - (r.source_width - 1) as f64 / 2.0;
        let dy = y - (r.source_height - 1) as f64 / 2.0;
        let (c, s) = (fmath::cos(r.rotation), fmath::sin(r.rotation));
        let vx =
            r.viewport_width / 2.0 + r.pan_x * self.scale + r.zoom * self.scale * (dx * c - dy * s);
        let vy = r.viewport_height / 2.0
            + r.pan_y * self.scale
            + r.zoom * self.scale * (dx * s + dy * c);
        let ox = vx * self.width as f64 / r.viewport_width - 0.5;
        let oy = vy * self.height as f64 / r.viewport_height - 0.5;
        (ox, oy, finite(ox) && finite(oy))
    }
}
