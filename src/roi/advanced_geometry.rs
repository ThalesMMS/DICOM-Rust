use super::ImagePoint;
use super::measure_reslice::MeasureSpacing;
use crate::fmath;
use crate::render::grid::nearest_index;
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}
impl Point2D {
    pub const fn new(x: f64, y: f64) -> Point2D {
        Point2D { x, y }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Line2D {
    pub start: Point2D,
    pub end: Point2D,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rectangle2D {
    pub min: Point2D,
    pub max: Point2D,
}
pub fn mean_axis_lines(points: [ImagePoint; 4]) -> (Line2D, Line2D, bool) {
    let primary = Line2D {
        start: midpoint(points[0], points[1]),
        end: midpoint(points[2], points[3]),
    };
    let secondary = Line2D {
        start: midpoint(points[0], points[3]),
        end: midpoint(points[1], points[2]),
    };
    let valid = primary.start != primary.end && secondary.start != secondary.end;
    (primary, secondary, valid)
}
pub fn triangle_perpendicular_bisectors(points: [ImagePoint; 3]) -> Option<[Line2D; 3]> {
    subpixel_triangle_perpendicular_bisectors([
        point_2d(points[0]),
        point_2d(points[1]),
        point_2d(points[2]),
    ])
}
pub fn subpixel_triangle_perpendicular_bisectors(points: [Point2D; 3]) -> Option<[Line2D; 3]> {
    let circumcenter = triangle_circumcenter(points)?;
    let mut longest_side = 0.0;
    for i in 0..3 {
        let j = (i + 1) % 3;
        let length = fmath::hypot(points[j].x - points[i].x, points[j].y - points[i].y);
        if length == 0.0 || !finite_number(length) {
            return None;
        }
        longest_side = fmath::max(longest_side, length);
    }
    let mut result = [Line2D::default(); 3];
    for (i, line) in result.iter_mut().enumerate() {
        let j = (i + 1) % 3;
        let (start, end) = (points[i], points[j]);
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length = fmath::hypot(dx, dy);
        let middle = Point2D::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
        let normal = Point2D::new(-dy / length, dx / length);
        let center_distance =
            ((circumcenter.x - middle.x) * normal.x + (circumcenter.y - middle.y) * normal.y).abs();
        let half_length = fmath::max(longest_side, center_distance + longest_side / 2.0);
        *line = Line2D {
            start: Point2D::new(
                middle.x - normal.x * half_length,
                middle.y - normal.y * half_length,
            ),
            end: Point2D::new(
                middle.x + normal.x * half_length,
                middle.y + normal.y * half_length,
            ),
        };
    }
    Some(result)
}
pub fn project_point_to_line(
    point: ImagePoint,
    start: ImagePoint,
    end: ImagePoint,
) -> Option<Point2D> {
    subpixel_project_point_to_line(point_2d(point), point_2d(start), point_2d(end))
}
pub fn subpixel_project_point_to_line(
    point: Point2D,
    start: Point2D,
    end: Point2D,
) -> Option<Point2D> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let denominator = dx * dx + dy * dy;
    if denominator == 0.0 || !finite_number(denominator) {
        return None;
    }
    let u = ((point.x - start.x) * dx + (point.y - start.y) * dy) / denominator;
    Some(Point2D::new(start.x + u * dx, start.y + u * dy))
}
pub fn perpendicular_line_points(
    b_end: ImagePoint,
    c_end: ImagePoint,
    main_start: ImagePoint,
    main_end: ImagePoint,
) -> Option<[ImagePoint; 6]> {
    let sub = subpixel_perpendicular_line_points(
        point_2d(b_end),
        point_2d(c_end),
        point_2d(main_start),
        point_2d(main_end),
    )?;
    Some([
        ImagePoint::new(nearest_index(sub[0].x), nearest_index(sub[0].y)),
        b_end,
        ImagePoint::new(nearest_index(sub[2].x), nearest_index(sub[2].y)),
        c_end,
        main_start,
        main_end,
    ])
}
pub fn subpixel_perpendicular_line_points(
    b_end: Point2D,
    c_end: Point2D,
    main_start: Point2D,
    main_end: Point2D,
) -> Option<[Point2D; 6]> {
    let b_foot = subpixel_project_point_to_line(b_end, main_start, main_end)?;
    let c_foot = subpixel_project_point_to_line(c_end, main_start, main_end)?;
    Some([b_foot, b_end, c_foot, c_end, main_start, main_end])
}
pub fn dynamic_angle_deg(
    line1_start: ImagePoint,
    line1_end: ImagePoint,
    line2_start: ImagePoint,
    line2_end: ImagePoint,
    spacing: MeasureSpacing,
) -> Option<(f64, f64)> {
    subpixel_dynamic_angle_deg(
        point_2d(line1_start),
        point_2d(line1_end),
        point_2d(line2_start),
        point_2d(line2_end),
        spacing,
    )
}
pub fn subpixel_dynamic_angle_deg(
    line1_start: Point2D,
    line1_end: Point2D,
    line2_start: Point2D,
    line2_end: Point2D,
    spacing: MeasureSpacing,
) -> Option<(f64, f64)> {
    if !spacing.valid() {
        return None;
    }
    let (x, y) = spacing.spacing_xy();
    let v1x = (line1_end.x - line1_start.x) * x;
    let v1y = (line1_end.y - line1_start.y) * y;
    let v2x = (line2_end.x - line2_start.x) * x;
    let v2y = (line2_end.y - line2_start.y) * y;
    if fmath::hypot(v1x, v1y) == 0.0 || fmath::hypot(v2x, v2y) == 0.0 {
        return None;
    }
    let mut delta = ((fmath::atan2(v1y, v1x) - fmath::atan2(v2y, v2x)).abs() * 180.0 / PI) % 180.0;
    if delta > 90.0 {
        delta = 180.0 - delta;
    }
    Some((delta, 360.0 - delta))
}
pub fn sphere_slice_radius_mm(radius_mm: f64, plane_offset_mm: f64) -> Option<f64> {
    if radius_mm <= 0.0 || !radius_mm.is_finite() || !plane_offset_mm.is_finite() {
        return None;
    }
    let offset = plane_offset_mm.abs();
    if offset > radius_mm {
        return None;
    }
    let mut value = radius_mm * radius_mm - offset * offset;
    if value < 0.0 {
        value = 0.0;
    }
    Some(value.sqrt())
}
pub fn repulse_polyline(
    points: &[Point2D],
    center: Point2D,
    radius: f64,
    strength: f64,
) -> Option<Vec<Point2D>> {
    if points.is_empty()
        || !finite_point(center)
        || !finite_number(radius)
        || radius <= 0.0
        || !finite_number(strength)
        || strength < 0.0
    {
        return None;
    }
    let mut result = Vec::with_capacity(points.len());
    for &point in points {
        if !finite_point(point) {
            return None;
        }
        let mut moved = point;
        let dx = point.x - center.x;
        let dy = point.y - center.y;
        let distance = fmath::hypot(dx, dy);
        if distance >= radius || strength == 0.0 {
            result.push(moved);
            continue;
        }
        let falloff = 1.0 - distance / radius;
        if distance == 0.0 {
            moved.x += strength;
            result.push(moved);
            continue;
        }
        let displacement = strength * falloff / distance;
        moved.x += dx * displacement;
        moved.y += dy * displacement;
        result.push(moved);
    }
    Some(result)
}
pub fn polyline_intersects_rectangle(points: &[Point2D], rectangle: Rectangle2D) -> bool {
    let Some(rectangle) = canonical_rectangle(rectangle) else {
        return false;
    };
    if points.is_empty() {
        return false;
    }
    for &point in points {
        if !finite_point(point) {
            return false;
        }
        if point_in_rectangle(point, rectangle) {
            return true;
        }
    }
    points
        .windows(2)
        .any(|w| segment_intersects_rectangle(w[0], w[1], rectangle))
}
pub fn polygon_intersects_rectangle(points: &[Point2D], rectangle: Rectangle2D) -> bool {
    let Some(rectangle) = canonical_rectangle(rectangle) else {
        return false;
    };
    if points.len() < 3 {
        return false;
    }
    for &point in points {
        if !finite_point(point) {
            return false;
        }
        if point_in_rectangle(point, rectangle) {
            return true;
        }
    }
    for i in 0..points.len() {
        if segment_intersects_rectangle(points[i], points[(i + 1) % points.len()], rectangle) {
            return true;
        }
    }
    point_in_polygon_2d(rectangle.min, points)
}
fn triangle_circumcenter(points: [Point2D; 3]) -> Option<Point2D> {
    let (ax, ay) = (points[0].x, points[0].y);
    let (bx, by) = (points[1].x, points[1].y);
    let (cx, cy) = (points[2].x, points[2].y);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d == 0.0 {
        return None;
    }
    let aa = ax * ax + ay * ay;
    let bb = bx * bx + by * by;
    let cc = cx * cx + cy * cy;
    let center = Point2D::new(
        (aa * (by - cy) + bb * (cy - ay) + cc * (ay - by)) / d,
        (aa * (cx - bx) + bb * (ax - cx) + cc * (bx - ax)) / d,
    );
    finite_point(center).then_some(center)
}
fn canonical_rectangle(rectangle: Rectangle2D) -> Option<Rectangle2D> {
    if !finite_point(rectangle.min) || !finite_point(rectangle.max) {
        return None;
    }
    Some(Rectangle2D {
        min: Point2D::new(
            fmath::min(rectangle.min.x, rectangle.max.x),
            fmath::min(rectangle.min.y, rectangle.max.y),
        ),
        max: Point2D::new(
            fmath::max(rectangle.min.x, rectangle.max.x),
            fmath::max(rectangle.min.y, rectangle.max.y),
        ),
    })
}
fn point_in_rectangle(point: Point2D, rectangle: Rectangle2D) -> bool {
    point.x >= rectangle.min.x
        && point.x <= rectangle.max.x
        && point.y >= rectangle.min.y
        && point.y <= rectangle.max.y
}
fn segment_intersects_rectangle(start: Point2D, end: Point2D, rectangle: Rectangle2D) -> bool {
    if point_in_rectangle(start, rectangle) || point_in_rectangle(end, rectangle) {
        return true;
    }
    let top_left = rectangle.min;
    let top_right = Point2D::new(rectangle.max.x, rectangle.min.y);
    let bottom_right = rectangle.max;
    let bottom_left = Point2D::new(rectangle.min.x, rectangle.max.y);
    segments_intersect(start, end, top_left, top_right)
        || segments_intersect(start, end, top_right, bottom_right)
        || segments_intersect(start, end, bottom_right, bottom_left)
        || segments_intersect(start, end, bottom_left, top_left)
}
pub(crate) fn segments_intersect(a: Point2D, b: Point2D, c: Point2D, d: Point2D) -> bool {
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);
    if opposite_signs(o1, o2) && opposite_signs(o3, o4) {
        return true;
    }
    o1 == 0.0 && point_on_segment(c, a, b)
        || o2 == 0.0 && point_on_segment(d, a, b)
        || o3 == 0.0 && point_on_segment(a, c, d)
        || o4 == 0.0 && point_on_segment(b, c, d)
}
pub(crate) fn orientation(a: Point2D, b: Point2D, c: Point2D) -> f64 {
    let value = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if value.abs() <= 1e-9 {
        return 0.0;
    }
    value
}
fn opposite_signs(a: f64, b: f64) -> bool {
    a < 0.0 && b > 0.0 || a > 0.0 && b < 0.0
}
fn point_on_segment(point: Point2D, start: Point2D, end: Point2D) -> bool {
    point.x >= fmath::min(start.x, end.x) - 1e-9
        && point.x <= fmath::max(start.x, end.x) + 1e-9
        && point.y >= fmath::min(start.y, end.y) - 1e-9
        && point.y <= fmath::max(start.y, end.y) + 1e-9
}
fn point_in_polygon_2d(point: Point2D, polygon: &[Point2D]) -> bool {
    let mut inside = false;
    let n = polygon.len();
    for (i, &current) in polygon.iter().enumerate() {
        let previous = polygon[(i + n - 1) % n];
        if orientation(previous, current, point) == 0.0
            && point_on_segment(point, previous, current)
        {
            return true;
        }
        if (current.y > point.y) != (previous.y > point.y) {
            let crossing_x = (previous.x - current.x) * (point.y - current.y)
                / (previous.y - current.y)
                + current.x;
            if point.x < crossing_x {
                inside = !inside;
            }
        }
    }
    inside
}
pub(crate) fn point_2d(point: ImagePoint) -> Point2D {
    Point2D::new(point.x as f64, point.y as f64)
}
pub(crate) fn finite_point(point: Point2D) -> bool {
    finite_number(point.x) && finite_number(point.y)
}
pub(crate) fn finite_number(value: f64) -> bool {
    value.is_finite()
}
fn midpoint(a: ImagePoint, b: ImagePoint) -> Point2D {
    Point2D::new(
        a.x.wrapping_add(b.x) as f64 / 2.0,
        a.y.wrapping_add(b.y) as f64 / 2.0,
    )
}
