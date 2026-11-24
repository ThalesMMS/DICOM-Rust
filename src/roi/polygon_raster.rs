use super::advanced_geometry::{Point2D, point_2d};
use super::roimask::{MaskRun, RasterMask};
use super::{ImagePoint, pdqsort};
use crate::fmath;
pub fn rasterize_polygon(points: &[Point2D], columns: i64, rows: i64) -> RasterMask {
    let mut mask = RasterMask::new(columns, rows);
    if points.len() < 3 {
        return mask;
    }
    let snapped = snap_points(points);
    rasterize_polygon_scanlines(&mut mask, &snapped);
    mask
}
pub(crate) fn snap_points(points: &[Point2D]) -> Vec<Point2D> {
    points
        .iter()
        .map(|p| Point2D::new(snap_to_pixel_centre(p.x), snap_to_pixel_centre(p.y)))
        .collect()
}
pub(crate) const PIXEL_CENTRE_SNAP_TOLERANCE: f64 = 1e-6;
pub(crate) fn snap_to_pixel_centre(value: f64) -> f64 {
    let nearest = value.round();
    if (value - nearest).abs() <= PIXEL_CENTRE_SNAP_TOLERANCE {
        nearest
    } else {
        value
    }
}
pub(crate) fn image_points_to_point_2d(points: &[ImagePoint]) -> Vec<Point2D> {
    points.iter().map(|p| point_2d(*p)).collect()
}
pub(crate) fn rasterize_polygon_scanlines(mask: &mut RasterMask, points: &[Point2D]) {
    if points.len() < 3 || mask.columns <= 0 || mask.rows <= 0 {
        return;
    }
    let mut min_vertex_y = f64::INFINITY;
    let mut max_vertex_y = f64::NEG_INFINITY;
    for p in points {
        if !finite(p.x) || !finite(p.y) {
            return;
        }
        min_vertex_y = fmath::min(min_vertex_y, p.y);
        max_vertex_y = fmath::max(max_vertex_y, p.y);
    }
    let min_y = polygon_scanline_boundary(min_vertex_y, mask.rows);
    let max_y = polygon_scanline_boundary(max_vertex_y, mask.rows) - 1;
    if min_y > max_y {
        return;
    }
    struct Edge {
        point_index: usize,
        end_y: i64,
        next_start: isize,
    }
    let n = points.len();
    let mut edges: Vec<Edge> = Vec::with_capacity(n);
    let mut start_heads = vec![-1isize; (max_y - min_y + 1) as usize];
    for current in (0..n).rev() {
        let previous = if current == 0 { n - 1 } else { current - 1 };
        let (cp, pp) = (points[current], points[previous]);
        let mut start_y = polygon_scanline_boundary(fmath::min(cp.y, pp.y), mask.rows);
        let mut end_y = polygon_scanline_boundary(fmath::max(cp.y, pp.y), mask.rows);
        start_y = start_y.max(min_y);
        end_y = end_y.min(max_y + 1);
        if start_y >= end_y {
            continue;
        }
        let bucket = (start_y - min_y) as usize;
        edges.push(Edge {
            point_index: current,
            end_y,
            next_start: start_heads[bucket],
        });
        start_heads[bucket] = edges.len() as isize - 1;
    }
    let mut active: Vec<usize> = Vec::with_capacity(edges.len());
    let mut intersections: Vec<f64> = Vec::with_capacity(n);
    for y in min_y..=max_y {
        active.retain(|&e| edges[e].end_y > y);
        let mut e = start_heads[(y - min_y) as usize];
        while e >= 0 {
            active.push(e as usize);
            e = edges[e as usize].next_start;
        }
        intersections.clear();
        let py = y as f64;
        for &ei in &active {
            let edge = &edges[ei];
            let cp = points[edge.point_index];
            let pp = points[(edge.point_index + n - 1) % n];
            let (xi, yi, xj, yj) = (cp.x, cp.y, pp.x, pp.y);
            intersections.push((xj - xi) * (py - yi) / (yj - yi) + xi);
        }
        pdqsort::sort_float64s(&mut intersections);
        let mut runs: Vec<MaskRun> = Vec::with_capacity(intersections.len() / 2);
        let mut i = 0;
        while i + 1 < intersections.len() {
            let start = polygon_scanline_boundary(intersections[i], mask.columns);
            let end = polygon_scanline_boundary(intersections[i + 1], mask.columns);
            i += 2;
            if start >= end {
                continue;
            }
            if let Some(last) = runs.last_mut()
                && last.end >= start
            {
                if end > last.end {
                    last.end = end;
                }
                continue;
            }
            runs.push(MaskRun { start, end });
        }
        if !runs.is_empty() {
            mask.runs[y as usize] = runs;
        }
    }
}
pub(crate) fn polygon_scanline_boundary(coordinate: f64, limit: i64) -> i64 {
    if coordinate <= 0.0 {
        return 0;
    }
    if coordinate >= limit as f64 {
        return limit;
    }
    fmath::f64_to_i64(coordinate.ceil())
}
pub(crate) fn finite(value: f64) -> bool {
    value.is_finite()
}
