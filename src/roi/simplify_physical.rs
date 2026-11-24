use super::advanced_geometry::Point2D;
use super::simplify_physical_exact::{
    polyline_exact_segment_distance, polyline_independent, polyline_segments_intersect,
    polyline_within,
};
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolylineTopology(pub u8);
impl PolylineTopology {
    pub const REQUIRE_SIMPLE: PolylineTopology = PolylineTopology(0);
    pub const IGNORE_INTERSECTIONS: PolylineTopology = PolylineTopology(1);
}
pub const ERR_POLYLINE_INVALID: &str = "roi: invalid physical polyline or options";
pub const ERR_POLYLINE_LIMIT: &str = "roi: physical polyline resource limit exceeded";
pub const ERR_POLYLINE_TOPOLOGY: &str = "roi: physical polyline intersects or overlaps itself";
const POLYLINE_RANGE: f64 = 1e9;
const POLYLINE_FIXED_BYTES: i64 = 256 * 1024;
const POLYLINE_BYTES_PER_POINT: i64 = 256;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolylineLimits {
    pub max_points: i64,
    pub max_work: i64,
    pub max_bytes: i64,
}
pub fn default_polyline_limits() -> PolylineLimits {
    PolylineLimits {
        max_points: 4096,
        max_work: 32_000_000,
        max_bytes: 8 * 1024 * 1024,
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolylineSimplifyOptions {
    pub closed: bool,
    pub tolerance_mm: f64,
    pub protected_indices: Vec<i64>,
    pub topology: PolylineTopology,
    pub limits: PolylineLimits,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolylineSimplifyResult {
    pub points: Vec<Point3D>,
    pub original_indices: Vec<i64>,
    pub closed: bool,
    pub explicit_closure: bool,
    pub topology: PolylineTopology,
    pub input_length_mm: f64,
    pub output_length_mm: f64,
    pub max_deviation_mm: f64,
    pub removed_points: i64,
    pub reduction: f64,
    pub work: i64,
    pub estimated_bytes: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PolylinePixelPlane {
    pub origin_mm: Point3D,
    pub column_step_mm: Point3D,
    pub row_step_mm: Point3D,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PixelPolylineSimplifyResult {
    pub points: Vec<Point2D>,
    pub physical: PolylineSimplifyResult,
}
struct PolylineRun<'a> {
    ctx: &'a dyn Cancellation,
    limits: PolylineLimits,
    work: i64,
}
impl PolylineRun<'_> {
    fn step(&mut self) -> Result<(), Error> {
        if let Some(err) = ctx_err(Some(self.ctx)) {
            return Err(err);
        }
        if self.work >= self.limits.max_work {
            return Err(Error::sentinel(ERR_POLYLINE_LIMIT));
        }
        self.work += 1;
        Ok(())
    }
}
fn invalid() -> Error {
    Error::sentinel(ERR_POLYLINE_INVALID)
}
fn new_polyline_run<'a>(
    ctx: Option<&'a dyn Cancellation>,
    n: usize,
    o: &PolylineSimplifyOptions,
) -> Result<(PolylineRun<'a>, i64), Error> {
    let Some(ctx) = ctx else {
        return Err(invalid());
    };
    if !polyline_scalar(o.tolerance_mm)
        || o.tolerance_mm < 0.0
        || o.topology.0 > PolylineTopology::IGNORE_INTERSECTIONS.0
    {
        return Err(invalid());
    }
    if let Some(err) = ctx_err(Some(ctx)) {
        return Err(err);
    }
    let mut l = o.limits;
    if l.max_points < 0
        || l.max_work < 0
        || l.max_bytes < 0
        || l.max_points > 100_000
        || l.max_work > 1_000_000_000
        || l.max_bytes > 256 * 1024 * 1024
    {
        return Err(invalid());
    }
    let d = default_polyline_limits();
    if l.max_points == 0 {
        l.max_points = d.max_points;
    }
    if l.max_work == 0 {
        l.max_work = d.max_work;
    }
    if l.max_bytes == 0 {
        l.max_bytes = d.max_bytes;
    }
    let n = n as i64;
    if n > l.max_points || o.protected_indices.len() as i64 > l.max_points {
        return Err(Error::sentinel(ERR_POLYLINE_LIMIT));
    }
    let estimate = POLYLINE_FIXED_BYTES + POLYLINE_BYTES_PER_POINT * n;
    if estimate > l.max_bytes {
        return Err(Error::sentinel(ERR_POLYLINE_LIMIT));
    }
    Ok((
        PolylineRun {
            ctx,
            limits: l,
            work: 0,
        },
        estimate,
    ))
}
fn polyline_scalar(v: f64) -> bool {
    v.is_finite() && v.abs() <= POLYLINE_RANGE
}
fn polyline_point(p: Point3D) -> bool {
    polyline_scalar(p.x) && polyline_scalar(p.y) && polyline_scalar(p.z)
}
fn polyline_point_distance(a: Point3D, b: Point3D) -> f64 {
    fmath::hypot(fmath::hypot(a.x - b.x, a.y - b.y), a.z - b.z)
}
pub fn simplify_physical_polyline(
    ctx: Option<&dyn Cancellation>,
    points: &[Point3D],
    o: &PolylineSimplifyOptions,
) -> Result<PolylineSimplifyResult, Error> {
    let (mut r, estimate) = new_polyline_run(ctx, points.len(), o)?;
    r.simplify(points, o, estimate)
}
pub fn simplify_pixel_polyline(
    ctx: Option<&dyn Cancellation>,
    points: &[Point2D],
    plane: PolylinePixelPlane,
    o: &PolylineSimplifyOptions,
) -> Result<PixelPolylineSimplifyResult, Error> {
    let (mut r, estimate) = new_polyline_run(ctx, points.len(), o)?;
    if !polyline_point(plane.origin_mm)
        || !polyline_point(plane.column_step_mm)
        || !polyline_point(plane.row_step_mm)
        || !polyline_independent(plane.column_step_mm, plane.row_step_mm)
    {
        return Err(invalid());
    }
    let mut physical = Vec::with_capacity(points.len());
    for p in points {
        r.step()?;
        if !polyline_scalar(p.x) || !polyline_scalar(p.y) {
            return Err(invalid());
        }
        let v = Point3D {
            x: plane.origin_mm.x + p.x * plane.column_step_mm.x + p.y * plane.row_step_mm.x,
            y: plane.origin_mm.y + p.x * plane.column_step_mm.y + p.y * plane.row_step_mm.y,
            z: plane.origin_mm.z + p.x * plane.column_step_mm.z + p.y * plane.row_step_mm.z,
        };
        if !polyline_point(v) {
            return Err(invalid());
        }
        physical.push(v);
    }
    let mut result = r.simplify(&physical, o, estimate)?;
    let mut pixels = Vec::with_capacity(result.original_indices.len());
    for &index in &result.original_indices {
        r.step()?;
        pixels.push(points[index as usize]);
    }
    result.work = r.work;
    Ok(PixelPolylineSimplifyResult {
        points: pixels,
        physical: result,
    })
}
impl PolylineRun<'_> {
    fn simplify(
        &mut self,
        points: &[Point3D],
        o: &PolylineSimplifyOptions,
        estimate: i64,
    ) -> Result<PolylineSimplifyResult, Error> {
        let n = points.len();
        for &p in points {
            self.step()?;
            if !polyline_point(p) {
                return Err(invalid());
            }
        }
        let mut keep = vec![false; n];
        for &index in &o.protected_indices {
            self.step()?;
            if index < 0 || index >= n as i64 {
                return Err(invalid());
            }
            keep[index as usize] = true;
        }
        let explicit = o.closed && n > 1 && points[0] == points[n - 1];
        let mut canonical = n;
        if explicit {
            canonical -= 1;
            keep[n - 1] = true;
        }
        if canonical > 0 {
            keep[0] = true;
        }
        if !o.closed && n > 0 {
            keep[n - 1] = true;
        }
        if o.closed {
            if canonical < 3 {
                return Err(invalid());
            }
            let (second, third) = self.closed_anchors(&points[..canonical])?;
            keep[second] = true;
            keep[third] = true;
        }
        if o.topology == PolylineTopology::REQUIRE_SIMPLE {
            self.simple(points, o.closed)?;
        }
        let mut stack: Vec<(usize, usize)> = Vec::with_capacity(canonical + 1);
        let mut previous = 0;
        for (i, &kept) in keep.iter().enumerate().take(canonical).skip(1) {
            self.step()?;
            if kept {
                stack.push((previous, i));
                previous = i;
            }
        }
        let end = if o.closed {
            canonical as i64
        } else {
            canonical as i64 - 1
        };
        if end > previous as i64 {
            stack.push((previous, end as usize));
        }
        let at = |i: usize| {
            if i == canonical { points[0] } else { points[i] }
        };
        while !stack.is_empty() {
            self.step()?;
            let Some((sa, sb)) = stack.pop() else {
                break;
            };
            if sb <= sa + 1 {
                continue;
            }
            let (a, b) = (at(sa), at(sb));
            let mut split: Option<usize> = None;
            let mut max_distance = -1.0;
            let mut first_outside: Option<usize> = None;
            for i in sa + 1..sb {
                self.step()?;
                let p = at(i);
                let distance = polyline_segment_distance(p, a, b);
                if distance > max_distance {
                    max_distance = distance;
                    split = Some(i);
                }
                if !polyline_within(p, a, b, o.tolerance_mm) && first_outside.is_none() {
                    first_outside = Some(i);
                }
            }
            let Some(first_outside) = first_outside else {
                continue;
            };
            self.step()?;
            let split = match split {
                Some(split) if !polyline_within(at(split), a, b, o.tolerance_mm) => split,
                _ => first_outside,
            };
            keep[split] = true;
            stack.push((sa, split));
            stack.push((split, sb));
        }
        let mut out = PolylineSimplifyResult {
            closed: o.closed,
            explicit_closure: explicit,
            topology: o.topology,
            estimated_bytes: estimate,
            ..PolylineSimplifyResult::default()
        };
        for (i, &p) in points.iter().enumerate() {
            self.step()?;
            if keep[i] {
                out.points.push(p);
                out.original_indices.push(i as i64);
            }
        }
        if o.topology == PolylineTopology::REQUIRE_SIMPLE {
            self.simple(&out.points, o.closed)?;
        }
        out.input_length_mm = self.length(points, o.closed)?;
        out.output_length_mm = self.length(&out.points, o.closed)?;
        out.max_deviation_mm =
            self.deviation(points, &out.original_indices, canonical, o.closed)?;
        out.removed_points = (n - out.points.len()) as i64;
        if n > 0 {
            out.reduction = out.removed_points as f64 / n as f64;
        }
        out.work = self.work;
        Ok(out)
    }
    fn closed_anchors(&mut self, points: &[Point3D]) -> Result<(usize, usize), Error> {
        let mut second: Option<usize> = None;
        let mut distance = -1.0;
        for i in 1..points.len() {
            self.step()?;
            if points[i] != points[0] {
                let d = polyline_point_distance(points[i], points[0]);
                if d > distance {
                    distance = d;
                    second = Some(i);
                }
            }
        }
        let Some(second) = second else {
            return Err(invalid());
        };
        let mut third: Option<usize> = None;
        distance = -1.0;
        for i in 1..points.len() {
            self.step()?;
            if points[i] != points[0] && points[i] != points[second] {
                let d = polyline_segment_distance(points[i], points[0], points[second]);
                if d > distance {
                    distance = d;
                    third = Some(i);
                }
            }
        }
        let Some(third) = third else {
            return Err(invalid());
        };
        Ok((second, third))
    }
    fn length(&mut self, points: &[Point3D], closed: bool) -> Result<f64, Error> {
        let mut total = 0.0;
        for i in 1..points.len() {
            self.step()?;
            total += polyline_point_distance(points[i - 1], points[i]);
        }
        if closed && points.len() > 1 {
            self.step()?;
            total += polyline_point_distance(points[points.len() - 1], points[0]);
        }
        Ok(total)
    }
    fn deviation(
        &mut self,
        points: &[Point3D],
        indices: &[i64],
        canonical: usize,
        closed: bool,
    ) -> Result<f64, Error> {
        let mut max_deviation = 0.0;
        for j in 1..indices.len() {
            let (a, b) = (indices[j - 1] as usize, indices[j] as usize);
            for i in a + 1..b {
                self.step()?;
                max_deviation = fmath::max(
                    max_deviation,
                    polyline_exact_segment_distance(points[i], points[a], points[b]),
                );
            }
        }
        if closed
            && let Some(&last) = indices.last()
            && (last as usize) < canonical
        {
            let a = last as usize;
            for i in a + 1..canonical {
                self.step()?;
                max_deviation = fmath::max(
                    max_deviation,
                    polyline_exact_segment_distance(points[i], points[a], points[0]),
                );
            }
        }
        Ok(max_deviation)
    }
    fn simple(&mut self, points: &[Point3D], closed: bool) -> Result<(), Error> {
        let mut edges: Vec<(Point3D, Point3D)> = Vec::with_capacity(points.len());
        for i in 1..points.len() {
            self.step()?;
            if points[i - 1] != points[i] {
                edges.push((points[i - 1], points[i]));
            }
        }
        if closed && points.len() > 1 && points[points.len() - 1] != points[0] {
            edges.push((points[points.len() - 1], points[0]));
        }
        for i in 0..edges.len() {
            for j in i + 1..edges.len() {
                self.step()?;
                let (a, b) = (edges[i], edges[j]);
                let adjacent = j == i + 1 || (closed && i == 0 && j == edges.len() - 1);
                if adjacent {
                    let (outside_a, joint, outside_b) = if j == i + 1 {
                        (a.0, a.1, b.1)
                    } else {
                        (a.1, a.0, b.0)
                    };
                    if polyline_within(outside_a, joint, outside_b, 0.0)
                        || polyline_within(outside_b, outside_a, joint, 0.0)
                    {
                        return Err(Error::sentinel(ERR_POLYLINE_TOPOLOGY));
                    }
                } else if polyline_segments_intersect(a.0, a.1, b.0, b.1) {
                    return Err(Error::sentinel(ERR_POLYLINE_TOPOLOGY));
                }
            }
        }
        Ok(())
    }
}
fn polyline_segment_distance(p: Point3D, a: Point3D, b: Point3D) -> f64 {
    let d = Point3D {
        x: b.x - a.x,
        y: b.y - a.y,
        z: b.z - a.z,
    };
    let length = fmath::hypot(fmath::hypot(d.x, d.y), d.z);
    if length == 0.0 {
        return polyline_point_distance(p, a);
    }
    let (ux, uy, uz) = (d.x / length, d.y / length, d.z / length);
    let t = (p.x - a.x) * ux + (p.y - a.y) * uy + (p.z - a.z) * uz;
    if t <= 0.0 {
        return polyline_point_distance(p, a);
    }
    if t >= length {
        return polyline_point_distance(p, b);
    }
    fmath::hypot(
        fmath::hypot((p.x - a.x) - t * ux, (p.y - a.y) - t * uy),
        (p.z - a.z) - t * uz,
    )
}
