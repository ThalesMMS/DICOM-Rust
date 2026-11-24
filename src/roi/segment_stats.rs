use super::polygon_raster::finite;
use super::segmentation::Segmentation3D;
use crate::fmath;
use crate::render::VolumeGeometry;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeStats {
    pub voxel_count: i64,
    pub non_finite_count: i64,
    pub area_mm2: f64,
    pub volume_mm3: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub variance: f64,
    pub std_dev: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    pub histogram: Vec<i64>,
    pub histogram_min: f64,
    pub histogram_bin_width: f64,
}
impl Segmentation3D {
    pub fn statistics(
        &self,
        mut value_at: impl FnMut(i64, i64, i64) -> Option<f64>,
        bins: i64,
    ) -> VolumeStats {
        let mut stats = VolumeStats::default();
        let slices = self.slices();
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut moments = CentralMoments::default();
        let mut non_finite = 0i64;
        for &z in &slices {
            let mask = &self.masks[&z];
            let (col_sp, row_sp) = slice_in_plane_spacing(&self.geometry, z);
            let thickness = slice_thickness(&self.geometry, z);
            let area_per_voxel = col_sp * row_sp;
            mask.for_each_pixel(|x, y| {
                let Some(v) = value_at(x, y, z) else {
                    return;
                };
                if !finite(v) {
                    non_finite += 1;
                    return;
                }
                moments.add(v);
                if v < min {
                    min = v;
                }
                if v > max {
                    max = v;
                }
                stats.area_mm2 += area_per_voxel;
                stats.volume_mm3 += area_per_voxel * thickness;
            });
        }
        if moments.count == 0 {
            return VolumeStats {
                non_finite_count: non_finite,
                ..VolumeStats::default()
            };
        }
        stats.non_finite_count = non_finite;
        stats.voxel_count = moments.count;
        let n = moments.count as f64;
        stats.min = min;
        stats.max = max;
        stats.mean = moments.mean;
        stats.variance = moments.m2 / n;
        if stats.variance < 0.0 {
            stats.variance = 0.0;
        }
        stats.std_dev = stats.variance.sqrt();
        if stats.std_dev > 0.0 {
            stats.skewness = (moments.m3 / n) / fmath::pow(stats.std_dev, 3.0);
            stats.kurtosis = (moments.m4 / n) / (stats.variance * stats.variance) - 3.0;
        }
        if bins > 0 && max > min {
            stats.histogram = vec![0; bins as usize];
            stats.histogram_min = min;
            stats.histogram_bin_width = (max - min) / bins as f64;
            let width = stats.histogram_bin_width;
            let histogram = &mut stats.histogram;
            for &z in &slices {
                self.masks[&z].for_each_pixel(|x, y| {
                    let Some(v) = value_at(x, y, z) else {
                        return;
                    };
                    if !finite(v) {
                        return;
                    }
                    let mut b = fmath::f64_to_i64((v - min) / width);
                    if b >= bins {
                        b = bins - 1;
                    }
                    if b < 0 {
                        b = 0;
                    }
                    histogram[b as usize] += 1;
                });
            }
        }
        stats
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CentralMoments {
    pub(crate) count: i64,
    pub(crate) mean: f64,
    pub(crate) m2: f64,
    pub(crate) m3: f64,
    pub(crate) m4: f64,
}
impl CentralMoments {
    pub(crate) fn add(&mut self, value: f64) {
        let previous_count = self.count as f64;
        self.count += 1;
        let count = self.count as f64;
        let delta = value - self.mean;
        let delta_over_count = delta / count;
        let delta_over_count_squared = delta_over_count * delta_over_count;
        let term = delta * delta_over_count * previous_count;
        self.m4 += term * delta_over_count_squared * (count * count - 3.0 * count + 3.0)
            + 6.0 * delta_over_count_squared * self.m2
            - 4.0 * delta_over_count * self.m3;
        self.m3 += term * delta_over_count * (count - 2.0) - 3.0 * delta_over_count * self.m2;
        self.m2 += term;
        self.mean += delta_over_count;
    }
}
pub(crate) fn slice_in_plane_spacing(g: &VolumeGeometry, slice: i64) -> (f64, f64) {
    let (mut col_sp, mut row_sp) = (0.0, 0.0);
    if let Some(slices) = g.slices.as_deref()
        && slice >= 0
        && (slice as usize) < slices.len()
    {
        col_sp = slices[slice as usize].col_spacing;
        row_sp = slices[slice as usize].row_spacing;
    }
    if col_sp <= 0.0 {
        col_sp = 1.0;
    }
    if row_sp <= 0.0 {
        row_sp = 1.0;
    }
    (col_sp, row_sp)
}
pub(crate) fn slice_thickness(g: &VolumeGeometry, slice: i64) -> f64 {
    let positions = g.positions.as_deref().unwrap_or(&[]);
    let n = positions.len();
    if n < 2 {
        if g.mean_spacing > 0.0 {
            return g.mean_spacing;
        }
        return 1.0;
    }
    let slice = (slice.max(0) as usize).min(n - 1);
    if slice == 0 {
        positions[1] - positions[0]
    } else if slice == n - 1 {
        positions[n - 1] - positions[n - 2]
    } else {
        (positions[slice + 1] - positions[slice - 1]) / 2.0
    }
}
