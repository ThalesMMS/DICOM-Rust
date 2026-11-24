#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::Error;
use super::advanced_geometry::{Point2D, finite_point, orientation, segments_intersect};
use super::pdqsort_by::sort_slice;
use crate::fmath;
use crate::render::Vec3;
use crate::render::cpr::centripetal_catmull_rom_tangents;
pub const MAX_CURVE_CONTROL_POINTS: usize = 256;
pub const MAX_CURVE_CONTOUR_POINTS: usize = 4096;
pub const CURVE_TOLERANCE_PX: f64 = 0.01;
const MAX_CURVE_COORDINATE: f64 = 1048576.0;
const MIN_CURVE_CONTROL_SPACING_PX: f64 = 1e-6;
const CURVE_LENGTH_ERROR_PER_SPAN_PX: f64 = 0.005;
const MAX_CURVE_DEPTH: i64 = 20;
const MAX_CURVE_INTERSECTION_CHECKS: i64 = 1 << 20;
pub const ERR_CURVE_GEOMETRY: &str =
    "curve requires distinct finite controls and a simple, nonzero closed interior";
pub const ERR_CURVE_LIMIT: &str =
    "curve exceeds the control-point, contour-point or approximation limit";
pub fn approximate_control_curve(points: &[Point2D], closed: bool) -> Result<Vec<Point2D>, Error> {
    let contour = flatten_control_curve(points, closed)?;
    if closed {
        validate_closed_curve(&contour)?;
    }
    Ok(contour)
}
fn flatten_control_curve(points: &[Point2D], closed: bool) -> Result<Vec<Point2D>, Error> {
    if points.len() > MAX_CURVE_CONTROL_POINTS {
        return Err(Error::sentinel(ERR_CURVE_LIMIT));
    }
    let mut points = points;
    if closed && points.len() > 1 && points[0] == points[points.len() - 1] {
        points = &points[..points.len() - 1];
    }
    if points.len() < 2 || (closed && points.len() < 3) {
        return Err(Error::sentinel(ERR_CURVE_GEOMETRY));
    }
    let mut controls = Vec::with_capacity(points.len());
    for &point in points {
        if !finite_point(point)
            || point.x.abs() > MAX_CURVE_COORDINATE
            || point.y.abs() > MAX_CURVE_COORDINATE
        {
            return Err(Error::sentinel(ERR_CURVE_GEOMETRY));
        }
        controls.push(Vec3::new(point.x, point.y, 0.0));
    }
    let n = controls.len();
    let mut spans = n - 1;
    if closed {
        spans += 1;
    }
    let mut chords = Vec::with_capacity(spans);
    for i in 0..spans {
        let chord = controls[(i + 1) % n].sub(controls[i]);
        if chord.length() < MIN_CURVE_CONTROL_SPACING_PX {
            return Err(Error::sentinel(ERR_CURVE_GEOMETRY));
        }
        chords.push(chord);
    }
    let mut contour = Vec::with_capacity(MAX_CURVE_CONTOUR_POINTS.min(spans * 4 + 1));
    contour.push(points[0]);
    for (i, &chord) in chords.iter().enumerate() {
        let (mut prev, mut next) = (chord, chord);
        if i > 0 || closed {
            prev = chords[(i + spans - 1) % spans];
        }
        if i + 1 < spans || closed {
            next = chords[(i + 1) % spans];
        }
        let (start, end) = (controls[i], controls[(i + 1) % n]);
        let (start_tangent, end_tangent) = centripetal_catmull_rom_tangents(prev, chord, next);
        let curve = CurveBezier {
            start,
            c1: start.add(start_tangent.scale(1.0 / 3.0)),
            c2: end.sub(end_tangent.scale(1.0 / 3.0)),
            end,
        };
        if !curve.flatten(&mut contour, CURVE_LENGTH_ERROR_PER_SPAN_PX, 0) {
            return Err(Error::sentinel(ERR_CURVE_LIMIT));
        }
    }
    if closed {
        contour.pop();
    }
    Ok(contour)
}
#[derive(Clone, Copy)]
struct CurveBezier {
    start: Vec3,
    c1: Vec3,
    c2: Vec3,
    end: Vec3,
}
impl CurveBezier {
    fn flatten(&self, out: &mut Vec<Point2D>, length_error: f64, depth: i64) -> bool {
        let chord = self.end.sub(self.start);
        let control_length = self.c1.sub(self.start).length()
            + self.c2.sub(self.c1).length()
            + self.end.sub(self.c2).length();
        if control_length - chord.length() <= length_error
            && chord_distance(self.c1.sub(self.start), chord) <= CURVE_TOLERANCE_PX
            && chord_distance(self.c2.sub(self.start), chord) <= CURVE_TOLERANCE_PX
        {
            if out.len() >= MAX_CURVE_CONTOUR_POINTS {
                return false;
            }
            out.push(Point2D::new(self.end.x, self.end.y));
            return true;
        }
        if depth >= MAX_CURVE_DEPTH {
            return false;
        }
        let a = self.start.add(self.c1).scale(0.5);
        let m = self.c1.add(self.c2).scale(0.5);
        let c = self.c2.add(self.end).scale(0.5);
        let d = a.add(m).scale(0.5);
        let e = m.add(c).scale(0.5);
        let split = d.add(e).scale(0.5);
        CurveBezier {
            start: self.start,
            c1: a,
            c2: d,
            end: split,
        }
        .flatten(out, length_error / 2.0, depth + 1)
            && CurveBezier {
                start: split,
                c1: e,
                c2: c,
                end: self.end,
            }
            .flatten(out, length_error / 2.0, depth + 1)
    }
}
fn chord_distance(point: Vec3, chord: Vec3) -> f64 {
    if chord == Vec3::ZERO {
        return point.length();
    }
    let fraction = fmath::max(0.0, fmath::min(1.0, point.dot(chord) / chord.dot(chord)));
    point.sub(chord.scale(fraction)).length()
}
#[derive(Clone, Copy)]
struct CurveEdge {
    index: usize,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}
fn validate_closed_curve(points: &[Point2D]) -> Result<(), Error> {
    let n = points.len();
    let mut edges = Vec::with_capacity(n);
    let origin = points[0];
    let mut area = 0.0;
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % n];
        let c = points[(i + 2) % n];
        if a == b
            || orientation(a, b, c) == 0.0
                && ((a.x - b.x) * (c.x - b.x) + (a.y - b.y) * (c.y - b.y)) > 0.0
        {
            return Err(Error::sentinel(ERR_CURVE_GEOMETRY));
        }
        area += (a.x - origin.x) * (b.y - origin.y) - (b.x - origin.x) * (a.y - origin.y);
        edges.push(CurveEdge {
            index: i,
            min_x: fmath::min(a.x, b.x),
            max_x: fmath::max(a.x, b.x),
            min_y: fmath::min(a.y, b.y),
            max_y: fmath::max(a.y, b.y),
        });
    }
    if area.abs() <= 1e-9 {
        return Err(Error::sentinel(ERR_CURVE_GEOMETRY));
    }
    sort_slice(&mut edges, &|a: &CurveEdge, b: &CurveEdge| {
        a.min_x < b.min_x
    });
    let mut checks = 0i64;
    for (i, first) in edges.iter().enumerate() {
        let (a, b) = (points[first.index], points[(first.index + 1) % n]);
        for second in &edges[i + 1..] {
            if !(second.min_x <= first.max_x + 1e-9) {
                break;
            }
            checks += 1;
            if checks > MAX_CURVE_INTERSECTION_CHECKS {
                return Err(Error::sentinel(ERR_CURVE_LIMIT));
            }
            if second.min_y > first.max_y + 1e-9
                || first.min_y > second.max_y + 1e-9
                || (first.index + 1) % n == second.index
                || (second.index + 1) % n == first.index
            {
                continue;
            }
            let (c, d) = (points[second.index], points[(second.index + 1) % n]);
            if segments_intersect(a, b, c, d) {
                return Err(Error::sentinel(ERR_CURVE_GEOMETRY));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn candidate_pairs(points: &[Point2D]) -> i64 {
        let n = points.len();
        let mut spans: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                (a.x.min(b.x), a.x.max(b.x))
            })
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut pairs = 0;
        for (i, first) in spans.iter().enumerate() {
            pairs += spans[i + 1..]
                .iter()
                .take_while(|second| second.0 <= first.1 + 1e-9)
                .count() as i64;
        }
        pairs
    }
    fn zigzag(n: usize, amplitude: f64, step: f64) -> Vec<Point2D> {
        let mut points: Vec<Point2D> = (0..n)
            .map(|i| Point2D::new((i % 2) as f64 * amplitude, i as f64 * step))
            .collect();
        points.push(Point2D::new(amplitude + 50.0, (n - 1) as f64 * step + 50.0));
        points.push(Point2D::new(amplitude + 50.0, -50.0));
        points
    }
    #[test]
    fn intersection_check_limit() {
        let points = zigzag(250, 1.0, 20.0);
        let contour = flatten_control_curve(&points, true).unwrap();
        assert!(contour.len() <= MAX_CURVE_CONTOUR_POINTS);
        assert!(candidate_pairs(&contour) > MAX_CURVE_INTERSECTION_CHECKS);
        let err = approximate_control_curve(&points, true).unwrap_err();
        assert!(err.is(ERR_CURVE_LIMIT));
        assert!(approximate_control_curve(&points, false).is_ok());
    }
}
