use super::ImagePoint;
use super::roimask::RasterMask;
use crate::fmath;
use std::collections::HashMap;
struct RasterBoundaryEdge {
    from: ImagePoint,
    to: ImagePoint,
    used: bool,
}
impl RasterMask {
    pub fn contours(&self) -> Vec<Vec<ImagePoint>> {
        if self.is_empty() {
            return Vec::new();
        }
        let mut edges: Vec<RasterBoundaryEdge> = Vec::new();
        self.for_each_pixel(|x, y| {
            let mut add = |from: ImagePoint, to: ImagePoint| {
                edges.push(RasterBoundaryEdge {
                    from,
                    to,
                    used: false,
                });
            };
            if !self.get(x, y - 1) {
                add(ImagePoint::new(x, y), ImagePoint::new(x + 1, y));
            }
            if !self.get(x + 1, y) {
                add(ImagePoint::new(x + 1, y), ImagePoint::new(x + 1, y + 1));
            }
            if !self.get(x, y + 1) {
                add(ImagePoint::new(x + 1, y + 1), ImagePoint::new(x, y + 1));
            }
            if !self.get(x - 1, y) {
                add(ImagePoint::new(x, y + 1), ImagePoint::new(x, y));
            }
        });
        let mut by_start: HashMap<ImagePoint, Vec<usize>> = HashMap::with_capacity(edges.len());
        for (index, edge) in edges.iter().enumerate() {
            by_start.entry(edge.from).or_default().push(index);
        }
        let mut contours = Vec::new();
        for start_index in 0..edges.len() {
            if edges[start_index].used {
                continue;
            }
            let start = edges[start_index].from;
            let mut contour = vec![start];
            let mut current_index = start_index;
            let mut closed = false;
            for _ in 0..=edges.len() {
                if edges[current_index].used {
                    break;
                }
                edges[current_index].used = true;
                let (from, to) = (edges[current_index].from, edges[current_index].to);
                if to == start {
                    closed = true;
                    break;
                }
                contour.push(to);
                let candidates = by_start.get(&to).map_or(&[][..], |v| v.as_slice());
                match next_raster_boundary_edge(&edges, candidates, from, to) {
                    Some(next) => current_index = next,
                    None => break,
                }
            }
            let contour = remove_collinear_closed_contour_points(contour);
            if closed && contour.len() >= 3 {
                contours.push(contour);
            }
        }
        contours
    }
    pub fn largest_contour(&self) -> Vec<ImagePoint> {
        let mut largest: Vec<ImagePoint> = Vec::new();
        let mut max_area = 0.0;
        for contour in self.contours() {
            let area = closed_contour_signed_area(&contour).abs();
            if area > max_area {
                max_area = area;
                largest = contour;
            }
        }
        largest
    }
}
pub fn simplify_closed_contour(points: &[ImagePoint], tolerance: f64) -> Vec<ImagePoint> {
    let points = remove_collinear_closed_contour_points(points.to_vec());
    if points.len() < 4 || tolerance <= 0.0 || !tolerance.is_finite() {
        return points;
    }
    let (first, second) = closed_contour_split_pair(&points);
    if first == second {
        return points;
    }
    let forward = closed_contour_arc(&points, first, second);
    let backward = closed_contour_arc(&points, second, first);
    let forward = simplify_open_contour(&forward, tolerance);
    let backward = simplify_open_contour(&backward, tolerance);
    let mut out = forward;
    if backward.len() > 2 {
        out.extend_from_slice(&backward[1..backward.len() - 1]);
    }
    let out = remove_collinear_closed_contour_points(out);
    if out.len() < 3 {
        return points;
    }
    out
}
fn next_raster_boundary_edge(
    edges: &[RasterBoundaryEdge],
    candidates: &[usize],
    previous: ImagePoint,
    current: ImagePoint,
) -> Option<usize> {
    let mut best = None;
    let mut best_rank = 5;
    let incoming = raster_edge_heading(previous, current);
    for &index in candidates {
        if index >= edges.len() || edges[index].used {
            continue;
        }
        let outgoing = raster_edge_heading(current, edges[index].to);
        let delta = (outgoing - incoming + 4) % 4;
        let rank = match delta {
            1 => 0,
            0 => 1,
            3 => 2,
            _ => 3,
        };
        if rank < best_rank {
            best = Some(index);
            best_rank = rank;
        }
    }
    best
}
fn raster_edge_heading(from: ImagePoint, to: ImagePoint) -> i64 {
    if to.x > from.x {
        0
    } else if to.y > from.y {
        1
    } else if to.x < from.x {
        2
    } else {
        3
    }
}
pub(crate) fn remove_collinear_closed_contour_points(
    mut points: Vec<ImagePoint>,
) -> Vec<ImagePoint> {
    while points.len() > 3 {
        let mut removed = false;
        let n = points.len();
        let mut out = Vec::with_capacity(n);
        for (index, &point) in points.iter().enumerate() {
            let previous = points[(index + n - 1) % n];
            let next = points[(index + 1) % n];
            let first = point - previous;
            let second = next - point;
            let cross = first
                .x
                .wrapping_mul(second.y)
                .wrapping_sub(first.y.wrapping_mul(second.x));
            let dot = first
                .x
                .wrapping_mul(second.x)
                .wrapping_add(first.y.wrapping_mul(second.y));
            if cross == 0 && dot >= 0 {
                removed = true;
                continue;
            }
            out.push(point);
        }
        points = out;
        if !removed {
            break;
        }
    }
    points
}
pub(crate) fn closed_contour_signed_area(points: &[ImagePoint]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut double_area = 0i64;
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        double_area = double_area.wrapping_add(
            point
                .x
                .wrapping_mul(next.y)
                .wrapping_sub(next.x.wrapping_mul(point.y)),
        );
    }
    double_area as f64 / 2.0
}
fn closed_contour_split_pair(points: &[ImagePoint]) -> (usize, usize) {
    if points.len() < 2 {
        return (0, 0);
    }
    let mut extremes = [0usize; 4];
    for index in 1..points.len() {
        if points[index].x < points[extremes[0]].x {
            extremes[0] = index;
        }
        if points[index].x > points[extremes[1]].x {
            extremes[1] = index;
        }
        if points[index].y < points[extremes[2]].y {
            extremes[2] = index;
        }
        if points[index].y > points[extremes[3]].y {
            extremes[3] = index;
        }
    }
    let (mut first, mut second) = (extremes[0], extremes[1]);
    let mut max_distance = -1i64;
    for &a in &extremes {
        for &b in &extremes {
            if a == b {
                continue;
            }
            let delta = points[b] - points[a];
            let distance = delta
                .x
                .wrapping_mul(delta.x)
                .wrapping_add(delta.y.wrapping_mul(delta.y));
            if distance > max_distance {
                first = a;
                second = b;
                max_distance = distance;
            }
        }
    }
    (first, second)
}
fn closed_contour_arc(points: &[ImagePoint], first: usize, last: usize) -> Vec<ImagePoint> {
    let mut out = vec![points[first]];
    let mut index = first;
    while index != last {
        index = (index + 1) % points.len();
        out.push(points[index]);
    }
    out
}
fn simplify_open_contour(points: &[ImagePoint], tolerance: f64) -> Vec<ImagePoint> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let last = points[points.len() - 1];
    let mut max_distance = 0.0;
    let mut max_index = 0;
    for index in 1..points.len() - 1 {
        let distance = point_segment_distance_float(points[index], points[0], last);
        if distance > max_distance {
            max_distance = distance;
            max_index = index;
        }
    }
    if max_distance <= tolerance {
        return vec![points[0], last];
    }
    let mut left = simplify_open_contour(&points[..max_index + 1], tolerance);
    let right = simplify_open_contour(&points[max_index..], tolerance);
    left.pop();
    left.extend_from_slice(&right);
    left
}
fn point_segment_distance_float(point: ImagePoint, start: ImagePoint, end: ImagePoint) -> f64 {
    let dx = end.x.wrapping_sub(start.x) as f64;
    let dy = end.y.wrapping_sub(start.y) as f64;
    let px = point.x.wrapping_sub(start.x) as f64;
    let py = point.y.wrapping_sub(start.y) as f64;
    if dx == 0.0 && dy == 0.0 {
        return fmath::hypot(px, py);
    }
    let t = (px * dx + py * dy) / (dx * dx + dy * dy);
    let t = fmath::max(0.0, fmath::min(1.0, t));
    let projection_x = start.x as f64 + t * dx;
    let projection_y = start.y as f64 + t * dy;
    fmath::hypot(point.x as f64 - projection_x, point.y as f64 - projection_y)
}
