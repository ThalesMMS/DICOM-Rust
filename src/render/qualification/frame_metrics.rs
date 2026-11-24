use super::{Error, Result};
pub const RGBA_FRAME_ERROR_METRICS_V1_SCHEMA: &str = "rgba8-frame-error-metrics-v1";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RgbaFrameThresholdsV1 {
    pub absolute_component: [u8; 4],
    pub alpha_silhouette_cutoff: u8,
}
impl RgbaFrameThresholdsV1 {
    pub fn validate(&self) -> Result<()> {
        if self.alpha_silhouette_cutoff == 0 {
            return Err(Error::metrics("zero alpha silhouette cutoff"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RgbaFrameErrorMetricsV1 {
    pub schema: String,
    pub width: u32,
    pub height: u32,
    pub pixel_count: u64,
    pub thresholds: RgbaFrameThresholdsV1,
    pub mean_absolute_error: [f64; 4],
    pub root_mean_square_error: [f64; 4],
    pub p99_absolute_error: [f64; 4],
    pub maximum_absolute_error: [u8; 4],
    pub components_over_threshold: [u64; 4],
    pub pixels_with_any_component_over_limit: u64,
    pub alpha_silhouette_mismatch_pixels: u64,
}
impl RgbaFrameErrorMetricsV1 {
    pub fn validate(&self) -> Result<()> {
        if self.schema != RGBA_FRAME_ERROR_METRICS_V1_SCHEMA
            || self.width == 0
            || self.height == 0
            || self.pixel_count != u64::from(self.width) * u64::from(self.height)
        {
            return Err(Error::metrics("dimensions or schema"));
        }
        self.thresholds.validate()?;
        if self.pixels_with_any_component_over_limit > self.pixel_count
            || self.alpha_silhouette_mismatch_pixels > self.pixel_count
        {
            return Err(Error::metrics("pixel count overflow"));
        }
        for channel in 0..4 {
            if self.components_over_threshold[channel] > self.pixel_count
                || !bounded_frame_error(self.mean_absolute_error[channel])
                || !bounded_frame_error(self.root_mean_square_error[channel])
                || !bounded_frame_error(self.p99_absolute_error[channel])
            {
                return Err(Error::metrics(format!("channel {channel}")));
            }
            let maximum = f64::from(self.maximum_absolute_error[channel]);
            if self.p99_absolute_error[channel] > maximum
                || self.mean_absolute_error[channel] > maximum
                || self.root_mean_square_error[channel] > maximum
            {
                return Err(Error::metrics(format!(
                    "inconsistent channel {channel} distribution"
                )));
            }
        }
        Ok(())
    }
    pub fn pixel_fraction_over_threshold(&self) -> f64 {
        if self.pixel_count == 0 {
            return f64::NAN;
        }
        self.pixels_with_any_component_over_limit as f64 / self.pixel_count as f64
    }
    pub fn alpha_silhouette_mismatch_fraction(&self) -> f64 {
        if self.pixel_count == 0 {
            return f64::NAN;
        }
        self.alpha_silhouette_mismatch_pixels as f64 / self.pixel_count as f64
    }
    pub fn require_within(&self, limits: &RgbaFrameMetricLimitsV1) -> Result<()> {
        self.validate()?;
        limits.validate()?;
        for channel in 0..4 {
            if self.mean_absolute_error[channel] > limits.maximum_mean_absolute_error[channel]
                || self.root_mean_square_error[channel]
                    > limits.maximum_root_mean_square_error[channel]
                || self.p99_absolute_error[channel] > limits.maximum_p99_absolute_error[channel]
            {
                return Err(Error::metrics(format!(
                    "channel {channel} exceeds frozen limit"
                )));
            }
        }
        if self.pixel_fraction_over_threshold() > limits.maximum_pixel_fraction_over_threshold {
            return Err(Error::metrics(
                "pixel threshold fraction exceeds frozen limit",
            ));
        }
        if self.alpha_silhouette_mismatch_fraction()
            > limits.maximum_alpha_silhouette_mismatch_fraction
        {
            return Err(Error::metrics(
                "alpha silhouette fraction exceeds frozen limit",
            ));
        }
        Ok(())
    }
}
pub(crate) fn rgba_frame_bytes(
    width: u32,
    height: u32,
    first: usize,
    second: usize,
) -> Result<(u64, usize)> {
    let pixel_count = u64::from(width) * u64::from(height);
    if pixel_count > (i64::MAX as u64) / 4 {
        return Err(Error::metrics("frame exceeds the slice capacity"));
    }
    let want = pixel_count * 4;
    if first as u64 != want || second as u64 != want {
        return Err(Error::metrics(format!(
            "frame lengths {first}/{second}, want {want}"
        )));
    }
    Ok((pixel_count, want as usize))
}
pub fn compare_rgba8_frames(
    reference: &[u8],
    candidate: &[u8],
    width: u32,
    height: u32,
    thresholds: RgbaFrameThresholdsV1,
) -> Result<RgbaFrameErrorMetricsV1> {
    if width == 0 || height == 0 {
        return Err(Error::metrics("zero frame extent"));
    }
    thresholds.validate()?;
    let (pixel_count, _) = rgba_frame_bytes(width, height, reference.len(), candidate.len())?;
    let mut metrics = RgbaFrameErrorMetricsV1 {
        schema: RGBA_FRAME_ERROR_METRICS_V1_SCHEMA.to_string(),
        width,
        height,
        pixel_count,
        thresholds,
        ..Default::default()
    };
    let mut histogram = [[0u64; 256]; 4];
    let mut absolute_sums = [0f64; 4];
    let mut squared_sums = [0f64; 4];
    for (r, c) in reference
        .as_chunks::<4>()
        .0
        .iter()
        .zip(candidate.as_chunks::<4>().0)
    {
        let mut pixel_over = false;
        for channel in 0..4 {
            let difference = r[channel].abs_diff(c[channel]);
            histogram[channel][usize::from(difference)] += 1;
            let d = f64::from(difference);
            absolute_sums[channel] += d;
            squared_sums[channel] += d * d;
            if difference > metrics.maximum_absolute_error[channel] {
                metrics.maximum_absolute_error[channel] = difference;
            }
            if difference > thresholds.absolute_component[channel] {
                metrics.components_over_threshold[channel] += 1;
                pixel_over = true;
            }
        }
        if pixel_over {
            metrics.pixels_with_any_component_over_limit += 1;
        }
        let reference_visible = r[3] >= thresholds.alpha_silhouette_cutoff;
        let candidate_visible = c[3] >= thresholds.alpha_silhouette_cutoff;
        if reference_visible != candidate_visible {
            metrics.alpha_silhouette_mismatch_pixels += 1;
        }
    }
    let count = pixel_count as f64;
    let nearest_rank = (0.99 * count).ceil() as u64;
    for channel in 0..4 {
        metrics.mean_absolute_error[channel] = absolute_sums[channel] / count;
        metrics.root_mean_square_error[channel] = (squared_sums[channel] / count).sqrt();
        let mut cumulative = 0u64;
        for (difference, n) in histogram[channel].iter().enumerate() {
            cumulative += n;
            if cumulative >= nearest_rank {
                metrics.p99_absolute_error[channel] = difference as f64;
                break;
            }
        }
    }
    metrics.validate()?;
    Ok(metrics)
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RgbaFrameMetricLimitsV1 {
    pub maximum_mean_absolute_error: [f64; 4],
    pub maximum_root_mean_square_error: [f64; 4],
    pub maximum_p99_absolute_error: [f64; 4],
    pub maximum_pixel_fraction_over_threshold: f64,
    pub maximum_alpha_silhouette_mismatch_fraction: f64,
}
impl RgbaFrameMetricLimitsV1 {
    pub fn validate(&self) -> Result<()> {
        for channel in 0..4 {
            for value in [
                self.maximum_mean_absolute_error[channel],
                self.maximum_root_mean_square_error[channel],
                self.maximum_p99_absolute_error[channel],
            ] {
                if !bounded_frame_error(value) {
                    return Err(Error::metrics(format!("limit channel {channel}")));
                }
            }
        }
        for fraction in [
            self.maximum_pixel_fraction_over_threshold,
            self.maximum_alpha_silhouette_mismatch_fraction,
        ] {
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(Error::metrics("limit fraction"));
            }
        }
        Ok(())
    }
}
fn bounded_frame_error(value: f64) -> bool {
    (0.0..=255.0).contains(&value)
}
