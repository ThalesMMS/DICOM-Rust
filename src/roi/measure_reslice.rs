use super::advanced_geometry::point_2d;
use super::measure_subpixel::{
    subpixel_ellipse_area_mm2, subpixel_length_mm, subpixel_polygon_area_mm2,
    subpixel_polygon_length_mm, subpixel_rectangle_area_mm2,
};
use super::polygon_raster::image_points_to_point_2d;
use super::{ImagePoint, ImageRect};
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeasureSpacing {
    pub x: f64,
    pub y: f64,
    pub column_mm: f64,
    pub row_mm: f64,
}
impl MeasureSpacing {
    pub(crate) fn valid(&self) -> bool {
        let (x, y) = self.spacing_xy();
        x > 0.0 && y > 0.0 && x.is_finite() && y.is_finite()
    }
    pub(crate) fn spacing_xy(&self) -> (f64, f64) {
        let (mut x, mut y) = (self.x, self.y);
        if x <= 0.0 {
            x = self.column_mm;
        }
        if y <= 0.0 {
            y = self.row_mm;
        }
        (x, y)
    }
}
pub fn length_mm_with(a: ImagePoint, b: ImagePoint, sp: MeasureSpacing) -> Option<f64> {
    subpixel_length_mm(point_2d(a), point_2d(b), sp)
}
pub fn rectangle_area_mm2_with(a: ImagePoint, b: ImagePoint, sp: MeasureSpacing) -> Option<f64> {
    subpixel_rectangle_area_mm2(point_2d(a), point_2d(b), sp)
}
pub fn ellipse_area_mm2_with(bbox: ImageRect, sp: MeasureSpacing) -> Option<f64> {
    subpixel_ellipse_area_mm2(point_2d(bbox.min), point_2d(bbox.max), sp)
}
pub fn polygon_length_mm_with(pts: &[ImagePoint], sp: MeasureSpacing) -> Option<f64> {
    subpixel_polygon_length_mm(&image_points_to_point_2d(pts), sp)
}
pub fn polygon_area_mm2_with(pts: &[ImagePoint], sp: MeasureSpacing) -> Option<f64> {
    subpixel_polygon_area_mm2(&image_points_to_point_2d(pts), sp)
}
