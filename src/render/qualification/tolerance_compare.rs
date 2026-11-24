use super::frame_metrics::rgba_frame_bytes;
use super::{Error, Result};
use crate::render::png_encode::{PngSource, encode_png};
use std::path::{Path, PathBuf};
pub const MAX_TOLERANCE_RADIUS: i64 = 8;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RgbaToleranceV1 {
    pub tolerance_radius: i64,
    pub difference_threshold: [u8; 4],
    pub ignore_boundary_pixels: bool,
    pub maximum_different_pixels: u64,
}
impl RgbaToleranceV1 {
    pub fn validate(&self) -> Result<()> {
        if self.tolerance_radius < 0 || self.tolerance_radius > MAX_TOLERANCE_RADIUS {
            return Err(Error::metrics(format!(
                "tolerance radius {} outside [0, {MAX_TOLERANCE_RADIUS}]",
                self.tolerance_radius
            )));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RgbaToleranceResultV1 {
    pub width: u32,
    pub height: u32,
    pub compared_pixels: u64,
    pub different_pixels: u64,
    pub maximum_difference: u8,
    pub mean_difference: f64,
    pub difference: Vec<u8>,
}
impl RgbaToleranceResultV1 {
    pub fn pass(&self, tolerance: &RgbaToleranceV1) -> bool {
        self.different_pixels <= tolerance.maximum_different_pixels
    }
}
pub fn compare_rgba8_with_tolerance(
    test: &[u8],
    baseline: &[u8],
    width: u32,
    height: u32,
    tolerance: RgbaToleranceV1,
) -> Result<RgbaToleranceResultV1> {
    tolerance.validate()?;
    if width == 0 || height == 0 {
        return Err(Error::metrics("zero frame extent"));
    }
    let (pixel_count, _) = rgba_frame_bytes(width, height, test.len(), baseline.len())?;
    let (w, h, r) = (
        width as usize,
        height as usize,
        tolerance.tolerance_radius as usize,
    );
    let mut result = RgbaToleranceResultV1 {
        width,
        height,
        difference: vec![0u8; pixel_count as usize],
        ..Default::default()
    };
    let mut sum = 0f64;
    for y in 0..h {
        for x in 0..w {
            if tolerance.ignore_boundary_pixels && (x < r || y < r || x + r >= w || y + r >= h) {
                continue;
            }
            result.compared_pixels += 1;
            let t = &test[4 * (y * w + x)..4 * (y * w + x) + 4];
            let mut best = 256u32;
            let mut ny = y.saturating_sub(r);
            while ny <= (h - 1).min(y + r) && best > 0 {
                for nx in x.saturating_sub(r)..=(w - 1).min(x + r) {
                    let b = &baseline[4 * (ny * w + nx)..4 * (ny * w + nx) + 4];
                    let mut largest = 0u32;
                    let mut within = true;
                    for c in 0..4 {
                        let d = u32::from(t[c].abs_diff(b[c]));
                        largest = largest.max(d);
                        if d > u32::from(tolerance.difference_threshold[c]) {
                            within = false;
                        }
                    }
                    if within {
                        best = 0;
                        break;
                    }
                    best = best.min(largest);
                }
                ny += 1;
            }
            if best == 0 {
                continue;
            }
            result.different_pixels += 1;
            result.difference[y * w + x] = best as u8;
            result.maximum_difference = result.maximum_difference.max(best as u8);
            sum += f64::from(best);
        }
    }
    if result.different_pixels > 0 {
        result.mean_difference = sum / result.different_pixels as f64;
    }
    Ok(result)
}
pub fn compare_rgba8_with_baselines(
    test: &[u8],
    baselines: &[&[u8]],
    width: u32,
    height: u32,
    tolerance: RgbaToleranceV1,
) -> Result<(RgbaToleranceResultV1, usize)> {
    let mut best: Option<(RgbaToleranceResultV1, usize)> = None;
    for (i, baseline) in baselines.iter().enumerate() {
        let result = compare_rgba8_with_tolerance(test, baseline, width, height, tolerance)
            .map_err(|e| e.prefixed(format!("baseline {i}")))?;
        let passed = result.pass(&tolerance);
        if best
            .as_ref()
            .is_none_or(|(b, _)| result.different_pixels < b.different_pixels)
        {
            best = Some((result, i));
        }
        if passed {
            break;
        }
    }
    best.ok_or_else(|| Error::metrics("no baseline"))
}
pub fn write_comparison_pngs(
    directory: &Path,
    name: &str,
    test: &[u8],
    baseline: &[u8],
    result: &RgbaToleranceResultV1,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(directory).map_err(|e| Error::io(&e))?;
    let (w, h) = (result.width as usize, result.height as usize);
    let mut diff = vec![0u8; w * h];
    if result.maximum_difference > 0 {
        let maximum = u32::from(result.maximum_difference);
        for (out, v) in diff.iter_mut().zip(&result.difference) {
            *out = ((u32::from(*v) * 255 + maximum / 2) / maximum) as u8;
        }
    }
    let mut paths = Vec::with_capacity(3);
    for (suffix, source) in [
        ("test", PngSource::Nrgba(test)),
        ("baseline", PngSource::Nrgba(baseline)),
        ("diff", PngSource::Gray(&diff)),
    ] {
        let encoded = encode_png(source, w, h).map_err(|e| Error::other(e.to_string()))?;
        let path = directory.join(format!("{name}.{suffix}.png"));
        std::fs::write(&path, encoded).map_err(|e| Error::io(&e))?;
        paths.push(path);
    }
    Ok(paths)
}
