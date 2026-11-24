use super::ImagePoint;
use super::advanced_geometry::{Point2D, finite_number, finite_point};
use super::measure_reslice::MeasureSpacing;
use super::polygon_raster::image_points_to_point_2d;
use crate::fmath;
pub fn point_2ds_from_image(points: &[ImagePoint]) -> Vec<Point2D> {
    image_points_to_point_2d(points)
}
pub fn subpixel_length_mm(a: Point2D, b: Point2D, sp: MeasureSpacing) -> Option<f64> {
    if !sp.valid() || !finite_point(a) || !finite_point(b) {
        return None;
    }
    let (x, y) = sp.spacing_xy();
    Some(fmath::hypot((a.x - b.x) * x, (a.y - b.y) * y))
}
pub fn subpixel_angle_deg(a: Point2D, vertex: Point2D, b: Point2D, sp: MeasureSpacing) -> f64 {
    let (mut x, mut y) = (1.0, 1.0);
    if sp.valid() {
        (x, y) = sp.spacing_xy();
    }
    let (v1x, v1y) = ((a.x - vertex.x) * x, (a.y - vertex.y) * y);
    let (v2x, v2y) = ((b.x - vertex.x) * x, (b.y - vertex.y) * y);
    let (m1, m2) = (fmath::hypot(v1x, v1y), fmath::hypot(v2x, v2y));
    if m1 == 0.0 || m2 == 0.0 || m1.is_nan() || m2.is_nan() {
        return 0.0;
    }
    let c = (v1x * v2x + v1y * v2y) / (m1 * m2);
    fmath::acos(fmath::max(-1.0, fmath::min(1.0, c))) * 180.0 / std::f64::consts::PI
}
pub fn subpixel_rectangle_area_mm2(a: Point2D, b: Point2D, sp: MeasureSpacing) -> Option<f64> {
    if !sp.valid() || !finite_point(a) || !finite_point(b) {
        return None;
    }
    let (x, y) = sp.spacing_xy();
    Some((a.x - b.x).abs() * x * (a.y - b.y).abs() * y)
}
pub fn subpixel_ellipse_area_mm2(a: Point2D, b: Point2D, sp: MeasureSpacing) -> Option<f64> {
    if !sp.valid() || !finite_point(a) || !finite_point(b) {
        return None;
    }
    let (x, y) = sp.spacing_xy();
    Some(std::f64::consts::PI * (a.x - b.x).abs() / 2.0 * x * (a.y - b.y).abs() / 2.0 * y)
}
pub fn subpixel_polygon_length_mm(pts: &[Point2D], sp: MeasureSpacing) -> Option<f64> {
    if !sp.valid() || pts.len() < 2 {
        return None;
    }
    let (x, y) = sp.spacing_xy();
    let mut total = 0.0;
    for i in 1..pts.len() {
        total += fmath::hypot((pts[i].x - pts[i - 1].x) * x, (pts[i].y - pts[i - 1].y) * y);
    }
    finite_number(total).then_some(total)
}
pub fn subpixel_polygon_area_mm2(pts: &[Point2D], sp: MeasureSpacing) -> Option<f64> {
    if !sp.valid() || pts.len() < 3 {
        return None;
    }
    let (x, y) = sp.spacing_xy();
    let area = closed_polygon_area(pts).abs() * x * y;
    finite_number(area).then_some(area)
}
pub fn subpixel_cobb_angle_deg(
    l1a: Point2D,
    l1b: Point2D,
    l2a: Point2D,
    l2b: Point2D,
    sp: MeasureSpacing,
) -> f64 {
    let (mut x, mut y) = (1.0, 1.0);
    if sp.valid() {
        (x, y) = sp.spacing_xy();
    }
    let first = Point2D::new((l1b.x - l1a.x) * x, (l1b.y - l1a.y) * y);
    let second = Point2D::new((l2b.x - l2a.x) * x, (l2b.y - l2a.y) * y);
    let zero = Point2D::default();
    if first == zero || second == zero || !finite_point(first) || !finite_point(second) {
        return f64::NAN;
    }
    let a1 = fmath::atan2(first.y, first.x);
    let a2 = fmath::atan2(second.y, second.x);
    let mut d = ((a1 - a2).abs() * 180.0 / std::f64::consts::PI) % 180.0;
    if d > 90.0 {
        d = 180.0 - d;
    }
    d
}
pub fn subpixel_deviation_mm(
    line_a: Point2D,
    line_b: Point2D,
    point: Point2D,
    sp: MeasureSpacing,
) -> Option<f64> {
    if !sp.valid() {
        return None;
    }
    let (x, y) = sp.spacing_xy();
    let (ax, ay) = (line_a.x * x, line_a.y * y);
    let (bx, by) = (line_b.x * x, line_b.y * y);
    let (px, py) = (point.x * x, point.y * y);
    let (dx, dy) = (bx - ax, by - ay);
    let l = fmath::hypot(dx, dy);
    if l == 0.0 || !finite_number(l) {
        return None;
    }
    Some(((px - ax) * dy - (py - ay) * dx).abs() / l)
}
pub fn closed_polygon_area(points: &[Point2D]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let origin = points[0];
    let (mut ax, mut ay) = (points[1].x - origin.x, points[1].y - origin.y);
    let mut area = 0.0;
    for p in &points[2..] {
        let (bx, by) = (p.x - origin.x, p.y - origin.y);
        area += ax * by - bx * ay;
        ax = bx;
        ay = by;
    }
    area / 2.0
}
