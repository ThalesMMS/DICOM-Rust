use super::advanced_geometry::point_2d;
use super::measure_reslice::{
    MeasureSpacing, ellipse_area_mm2_with, length_mm_with, polygon_area_mm2_with,
    polygon_length_mm_with, rectangle_area_mm2_with,
};
use super::measure_subpixel::{subpixel_angle_deg, subpixel_cobb_angle_deg, subpixel_deviation_mm};
use super::roimask::RasterMask;
use super::segment_stats::{CentralMoments, VolumeStats};
use super::segmentation::Segmentation3D;
use super::{ImagePoint, ImageRect, pdqsort};
use crate::fmath;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub count: i64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub median: f64,
    pub variance: f64,
    pub std_dev: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    pub histogram: Vec<i64>,
    pub histogram_min: f64,
    pub histogram_bin_width: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SummaryStats {
    pub count: i64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub variance: f64,
    pub std_dev: f64,
}
pub fn length_mm(a: ImagePoint, b: ImagePoint, spacing: MeasureSpacing) -> Option<f64> {
    length_mm_with(a, b, spacing)
}
pub fn angle_deg(a: ImagePoint, vertex: ImagePoint, b: ImagePoint) -> f64 {
    subpixel_angle_deg(
        point_2d(a),
        point_2d(vertex),
        point_2d(b),
        MeasureSpacing::default(),
    )
}
pub fn rectangle_area_mm2(a: ImagePoint, b: ImagePoint, spacing: MeasureSpacing) -> Option<f64> {
    rectangle_area_mm2_with(a, b, spacing)
}
pub fn ellipse_area_mm2(bbox: ImageRect, spacing: MeasureSpacing) -> Option<f64> {
    ellipse_area_mm2_with(bbox, spacing)
}
pub fn polygon_length_mm(pts: &[ImagePoint], spacing: MeasureSpacing) -> Option<f64> {
    polygon_length_mm_with(pts, spacing)
}
pub fn polygon_area_mm2(pts: &[ImagePoint], spacing: MeasureSpacing) -> Option<f64> {
    polygon_area_mm2_with(pts, spacing)
}
pub fn cobb_angle_deg(l1a: ImagePoint, l1b: ImagePoint, l2a: ImagePoint, l2b: ImagePoint) -> f64 {
    subpixel_cobb_angle_deg(
        point_2d(l1a),
        point_2d(l1b),
        point_2d(l2a),
        point_2d(l2b),
        MeasureSpacing::default(),
    )
}
pub fn deviation_mm(
    line_a: ImagePoint,
    line_b: ImagePoint,
    point: ImagePoint,
    spacing: MeasureSpacing,
) -> Option<f64> {
    subpixel_deviation_mm(point_2d(line_a), point_2d(line_b), point_2d(point), spacing)
}
pub fn stats_2d(mask: &RasterMask, value_at: impl FnMut(i64, i64) -> Option<f64>) -> Stats {
    stats_2d_with_histogram(mask, value_at, 0)
}
pub fn stats_2d_summary(
    mask: &RasterMask,
    mut value_at: impl FnMut(i64, i64) -> Option<f64>,
) -> SummaryStats {
    let mut summary = SummaryStats::default();
    let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut m2 = 0.0f64;
    mask.for_each_pixel(|x, y| {
        let Some(value) = value_at(x, y) else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        let previous_count = summary.count as f64;
        summary.count += 1;
        let count = summary.count as f64;
        let delta = value - summary.mean;
        let delta_over_count = delta / count;
        m2 += delta * delta_over_count * previous_count;
        summary.mean += delta_over_count;
        summary.sum += value;
        if value < min {
            min = value;
        }
        if value > max {
            max = value;
        }
    });
    if summary.count == 0 {
        return SummaryStats::default();
    }
    summary.min = min;
    summary.max = max;
    summary.variance = m2 / summary.count as f64;
    if summary.variance < 0.0 {
        summary.variance = 0.0;
    }
    summary.std_dev = summary.variance.sqrt();
    summary
}
pub fn stats_2d_with_histogram(
    mask: &RasterMask,
    value_at: impl FnMut(i64, i64) -> Option<f64>,
    bins: i64,
) -> Stats {
    stats_2d_impl(mask, value_at, bins, 0.0, 0.0, false)
}
pub fn stats_2d_with_histogram_bounds(
    mask: &RasterMask,
    value_at: impl FnMut(i64, i64) -> Option<f64>,
    bins: i64,
    histogram_min: f64,
    histogram_max: f64,
) -> Stats {
    if bins <= 0
        || !histogram_min.is_finite()
        || !histogram_max.is_finite()
        || histogram_max <= histogram_min
    {
        return stats_2d_with_histogram(mask, value_at, bins);
    }
    stats_2d_impl(mask, value_at, bins, histogram_min, histogram_max, true)
}
fn stats_2d_impl(
    mask: &RasterMask,
    mut value_at: impl FnMut(i64, i64) -> Option<f64>,
    bins: i64,
    histogram_min: f64,
    histogram_max: f64,
    use_histogram_bounds: bool,
) -> Stats {
    let mut stats = Stats::default();
    let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut moments = CentralMoments::default();
    let mut values: Vec<f64> = Vec::new();
    mask.for_each_pixel(|x, y| {
        let Some(v) = value_at(x, y) else {
            return;
        };
        if !v.is_finite() {
            return;
        }
        moments.add(v);
        stats.sum += v;
        values.push(v);
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
    });
    if moments.count == 0 {
        return Stats::default();
    }
    stats.count = moments.count;
    stats.min = min;
    stats.max = max;
    stats.mean = moments.mean;
    let n = stats.count as f64;
    stats.variance = moments.m2 / n;
    if stats.variance < 0.0 {
        stats.variance = 0.0;
    }
    stats.std_dev = stats.variance.sqrt();
    if stats.std_dev > 0.0 {
        stats.skewness = (moments.m3 / n) / fmath::pow(stats.std_dev, 3.0);
        stats.kurtosis = (moments.m4 / n) / (stats.variance * stats.variance) - 3.0;
    }
    pdqsort::sort_float64s(&mut values);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        stats.median = (values[middle - 1] + values[middle]) / 2.0;
    } else {
        stats.median = values[middle];
    }
    if bins > 0 {
        stats.histogram = vec![0; bins as usize];
        stats.histogram_min = min;
        let mut histogram_range_max = max;
        if use_histogram_bounds {
            stats.histogram_min = histogram_min;
            histogram_range_max = histogram_max;
        }
        if histogram_range_max == stats.histogram_min {
            stats.histogram_bin_width = 1.0;
            stats.histogram[0] = stats.count;
            return stats;
        }
        stats.histogram_bin_width = (histogram_range_max - stats.histogram_min) / bins as f64;
        for value in &values {
            let mut bin =
                fmath::f64_to_i64((value - stats.histogram_min) / stats.histogram_bin_width);
            if bin >= bins {
                bin = bins - 1;
            }
            if bin < 0 {
                bin = 0;
            }
            stats.histogram[bin as usize] += 1;
        }
    }
    stats
}
pub fn stats_3d(
    seg: &Segmentation3D,
    value_at: impl FnMut(i64, i64, i64) -> Option<f64>,
    bins: i64,
) -> VolumeStats {
    seg.statistics(value_at, bins)
}
