use super::ImagePoint;
use super::advanced_geometry::{Point2D, finite_point};
use super::measure::{Stats, stats_2d_with_histogram, stats_2d_with_histogram_bounds};
use super::polygon_raster::{image_points_to_point_2d, rasterize_polygon_scanlines, snap_points};
use super::roimask::RasterMask;
use crate::fmath;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RoiShape {
    #[default]
    Rectangle,
    Ellipse,
    Polygon,
    Other(i64),
}
impl RoiShape {
    pub fn from_i64(v: i64) -> RoiShape {
        match v {
            0 => RoiShape::Rectangle,
            1 => RoiShape::Ellipse,
            2 => RoiShape::Polygon,
            other => RoiShape::Other(other),
        }
    }
    pub fn to_i64(self) -> i64 {
        match self {
            RoiShape::Rectangle => 0,
            RoiShape::Ellipse => 1,
            RoiShape::Polygon => 2,
            RoiShape::Other(v) => v,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VectorRoi {
    pub shape: RoiShape,
    pub points: Vec<ImagePoint>,
    pub subpixel_points: Vec<Point2D>,
}
impl VectorRoi {
    pub fn control_points(&self) -> Vec<Point2D> {
        self.points_2d()
    }
    pub(crate) fn points_2d(&self) -> Vec<Point2D> {
        if !self.subpixel_points.is_empty() {
            return self.subpixel_points.clone();
        }
        image_points_to_point_2d(&self.points)
    }
    pub fn contains(&self, x: i64, y: i64) -> bool {
        match self.shape {
            RoiShape::Ellipse => ellipse_contains(self.bbox(), x, y),
            RoiShape::Polygon => point_in_polygon_even_odd(x as f64, y as f64, &self.points_2d()),
            _ => rect_contains(self.bbox(), x, y),
        }
    }
    pub(crate) fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let points = self.points_2d();
        if points.len() < 2 {
            return None;
        }
        let (mut min_x, mut max_x) = (points[0].x, points[0].x);
        let (mut min_y, mut max_y) = (points[0].y, points[0].y);
        for p in &points {
            if !finite_point(*p) {
                return None;
            }
            min_x = fmath::min(min_x, p.x);
            max_x = fmath::max(max_x, p.x);
            min_y = fmath::min(min_y, p.y);
            max_y = fmath::max(max_y, p.y);
        }
        Some((min_x, min_y, max_x, max_y))
    }
    pub fn rasterize(&self, columns: i64, rows: i64) -> RasterMask {
        let mut mask = RasterMask::new(columns, rows);
        if self.shape == RoiShape::Polygon {
            self.rasterize_polygon_scanlines(&mut mask);
            return mask;
        }
        let bbox = self.bbox();
        let ellipse = self.shape == RoiShape::Ellipse;
        let inside = |x: i64, y: i64| {
            if ellipse {
                ellipse_contains(bbox, x, y)
            } else {
                rect_contains(bbox, x, y)
            }
        };
        for y in 0..rows {
            let mut x = 0;
            while x < columns {
                if !inside(x, y) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < columns && inside(x, y) {
                    x += 1;
                }
                mask.set_run(y, start, x);
            }
        }
        mask
    }
    pub fn statistics(
        &self,
        columns: i64,
        rows: i64,
        value_at: impl FnMut(i64, i64) -> Option<f64>,
        bins: i64,
    ) -> Stats {
        stats_2d_with_histogram(&self.rasterize(columns, rows), value_at, bins)
    }
    pub fn statistics_with_histogram_bounds(
        &self,
        columns: i64,
        rows: i64,
        value_at: impl FnMut(i64, i64) -> Option<f64>,
        bins: i64,
        histogram_min: f64,
        histogram_max: f64,
    ) -> Stats {
        stats_2d_with_histogram_bounds(
            &self.rasterize(columns, rows),
            value_at,
            bins,
            histogram_min,
            histogram_max,
        )
    }
    fn rasterize_polygon_scanlines(&self, mask: &mut RasterMask) {
        let mut points = self.points_2d();
        if points.len() < 3 {
            return;
        }
        if !self.subpixel_points.is_empty() {
            points = snap_points(&points);
        }
        rasterize_polygon_scanlines(mask, &points);
    }
}
fn rect_contains(bbox: Option<(f64, f64, f64, f64)>, x: i64, y: i64) -> bool {
    let Some((min_x, min_y, max_x, max_y)) = bbox else {
        return false;
    };
    let (px, py) = (x as f64, y as f64);
    px >= min_x && px <= max_x && py >= min_y && py <= max_y
}
fn ellipse_contains(bbox: Option<(f64, f64, f64, f64)>, x: i64, y: i64) -> bool {
    let Some((min_x, min_y, max_x, max_y)) = bbox else {
        return false;
    };
    let cx = (min_x + max_x) / 2.0;
    let cy = (min_y + max_y) / 2.0;
    let rx = (max_x - min_x) / 2.0;
    let ry = (max_y - min_y) / 2.0;
    if rx <= 0.0 || ry <= 0.0 {
        return false;
    }
    let dx = (x as f64 - cx) / rx;
    let dy = (y as f64 - cy) / ry;
    dx * dx + dy * dy <= 1.0
}
#[allow(dead_code)]
pub(crate) fn roi_point_in_polygon(x: i64, y: i64, pts: &[ImagePoint]) -> bool {
    point_in_polygon_even_odd(x as f64, y as f64, &image_points_to_point_2d(pts))
}
pub(crate) fn point_in_polygon_even_odd(px: f64, py: f64, pts: &[Point2D]) -> bool {
    if pts.len() < 3 {
        return false;
    }
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (pts[i].x, pts[i].y);
        let (xj, yj) = (pts[j].x, pts[j].y);
        if (yi > py) != (yj > py) {
            let cross = (xj - xi) * (py - yi) / (yj - yi) + xi;
            if px < cross {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
