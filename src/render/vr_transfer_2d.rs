#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::view_state::TransferSample;
use super::vr_raycast::dvr_transfer_sample;
use super::vr_transfer::{
    Rgba, VrTransferDomain, VrTransferFunction, clamp_unit, finite, lerp, srgb_to_linear,
};
use super::window::WindowLevel;
use crate::fmath;
use crate::textfmt::float_v;
pub const DEFAULT_VR_TRANSFER_2D_WIDTH: i64 = 256;
pub const DEFAULT_VR_TRANSFER_2D_HEIGHT: i64 = 128;
pub const MAX_VR_TRANSFER_2D_SIDE: i64 = 1024;
pub const MAX_VR_TRANSFER_2D_REGIONS: usize = 16;
pub const ERR_INVALID_VR_TRANSFER_2D: &str = "render: invalid VR 2D transfer function";
fn invalid(detail: String) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVrTransfer2d,
        format!("{ERR_INVALID_VR_TRANSFER_2D}: {detail}"),
    )
}
fn bare_invalid() -> RenderError {
    RenderError::new(ErrorKind::InvalidVrTransfer2d, ERR_INVALID_VR_TRANSFER_2D)
}
fn unit_channel(value: f64) -> bool {
    finite(value) && (0.0..=1.0).contains(&value)
}
#[derive(Debug, Clone, PartialEq)]
pub struct VrTransfer2d {
    width: i64,
    height: i64,
    intensity_min: f64,
    intensity_max: f64,
    gradient_max: f64,
    table: Vec<Rgba>,
    column_max_alpha: Vec<f64>,
}
fn validate_shape(
    width: i64,
    height: i64,
    intensity_min: f64,
    intensity_max: f64,
    gradient_max: f64,
) -> Result<(), RenderError> {
    if width < 2
        || height < 2
        || width > MAX_VR_TRANSFER_2D_SIDE
        || height > MAX_VR_TRANSFER_2D_SIDE
    {
        return Err(invalid(format!("size {width}x{height}")));
    }
    if !finite(intensity_min) || !finite(intensity_max) || intensity_max <= intensity_min {
        return Err(invalid(format!(
            "intensity range {}..{}",
            float_v(intensity_min),
            float_v(intensity_max)
        )));
    }
    if !finite(gradient_max) || gradient_max <= 0.0 {
        return Err(invalid(format!(
            "gradient maximum {}",
            float_v(gradient_max)
        )));
    }
    Ok(())
}
impl VrTransfer2d {
    pub fn new(
        width: i64,
        height: i64,
        intensity_min: f64,
        intensity_max: f64,
        gradient_max: f64,
        table: &[Rgba],
    ) -> Result<VrTransfer2d, RenderError> {
        validate_shape(width, height, intensity_min, intensity_max, gradient_max)?;
        if table.len() as i64 != width * height {
            return Err(invalid(format!(
                "{} entries for {width}x{height}",
                table.len()
            )));
        }
        for (index, entry) in table.iter().enumerate() {
            if !unit_channel(entry.r)
                || !unit_channel(entry.g)
                || !unit_channel(entry.b)
                || !unit_channel(entry.a)
            {
                return Err(invalid(format!(
                    "entry {index} {{R:{} G:{} B:{} A:{}}}",
                    float_v(entry.r),
                    float_v(entry.g),
                    float_v(entry.b),
                    float_v(entry.a)
                )));
            }
        }
        Ok(VrTransfer2d::build(
            width,
            height,
            intensity_min,
            intensity_max,
            gradient_max,
            table.to_vec(),
        ))
    }
    fn build(
        width: i64,
        height: i64,
        intensity_min: f64,
        intensity_max: f64,
        gradient_max: f64,
        table: Vec<Rgba>,
    ) -> VrTransfer2d {
        let w = width as usize;
        let mut column_max_alpha = vec![0.0f64; w];
        for row in table.chunks(w) {
            for (column, entry) in row.iter().enumerate() {
                column_max_alpha[column] = fmath::max(column_max_alpha[column], entry.a);
            }
        }
        VrTransfer2d {
            width,
            height,
            intensity_min,
            intensity_max,
            gradient_max,
            table,
            column_max_alpha,
        }
    }
    pub fn size(&self) -> (i64, i64) {
        (self.width, self.height)
    }
    pub fn intensity_range(&self) -> (f64, f64) {
        (self.intensity_min, self.intensity_max)
    }
    pub fn gradient_max(&self) -> f64 {
        self.gradient_max
    }
    pub fn at(&self, column: i64, row: i64) -> Rgba {
        if column < 0 || row < 0 || column >= self.width || row >= self.height {
            return Rgba::default();
        }
        self.table[(row * self.width + column) as usize]
    }
    pub fn table(&self) -> Vec<Rgba> {
        self.table.clone()
    }
    pub(crate) fn column_max_alpha(&self) -> &[f64] {
        &self.column_max_alpha
    }
    pub(crate) fn column_position(&self, value: f64) -> f64 {
        if !finite(value) {
            return 0.0;
        }
        clamp_unit((value - self.intensity_min) / (self.intensity_max - self.intensity_min))
            * (self.width - 1) as f64
    }
    pub fn lookup(&self, value: f64, gradient_magnitude: f64) -> Rgba {
        let x = self.column_position(value);
        let mut y = 0.0;
        if finite(gradient_magnitude) {
            y = clamp_unit(gradient_magnitude / self.gradient_max) * (self.height - 1) as f64;
        }
        let x0 = fmath::f64_to_i64(x).min(self.width - 2);
        let y0 = fmath::f64_to_i64(y).min(self.height - 2);
        let (fx, fy) = (x - x0 as f64, y - y0 as f64);
        let w = self.width as usize;
        let row0 = &self.table[y0 as usize * w..];
        let row1 = &self.table[(y0 as usize + 1) * w..];
        let mix = |a: Rgba, b: Rgba, f: f64| Rgba {
            r: lerp(a.r, b.r, f),
            g: lerp(a.g, b.g, f),
            b: lerp(a.b, b.b, f),
            a: lerp(a.a, b.a, f),
        };
        let x0 = x0 as usize;
        mix(
            mix(row0[x0], row0[x0 + 1], fx),
            mix(row1[x0], row1[x0 + 1], fx),
            fy,
        )
    }
    pub(crate) fn max_alpha_at(&self, value: f64) -> f64 {
        let x = self.column_position(value);
        let x0 = fmath::f64_to_i64(x).min(self.width - 2) as usize;
        lerp(
            self.column_max_alpha[x0],
            self.column_max_alpha[x0 + 1],
            x - x0 as f64,
        )
    }
    pub fn from_transfer_function(
        tf: &VrTransferFunction,
        window: &WindowLevel,
        width: i64,
        height: i64,
        intensity_min: f64,
        intensity_max: f64,
        gradient_max: f64,
    ) -> Result<VrTransfer2d, RenderError> {
        validate_shape(width, height, intensity_min, intensity_max, gradient_max)?;
        let lut = if tf.domain() == VrTransferDomain::Hu {
            tf.bake_lut(intensity_min, intensity_max, tf.preferred_lut_size())
        } else {
            tf.bake_window_lut(window, tf.preferred_lut_size())
        };
        let (w, h) = (width as usize, height as usize);
        let mut table = vec![Rgba::default(); w * h];
        for column in 0..w {
            let value = intensity_min
                + column as f64 / (width - 1) as f64 * (intensity_max - intensity_min);
            let entry = dvr_transfer_sample(tf, &lut, value, window, intensity_min, intensity_max);
            for row in 0..h {
                table[row * w + column] = entry;
            }
        }
        Ok(VrTransfer2d::build(
            width,
            height,
            intensity_min,
            intensity_max,
            gradient_max,
            table,
        ))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrTransfer2dRegion {
    pub intensity_min: f64,
    pub intensity_max: f64,
    pub gradient_min: f64,
    pub gradient_max: f64,
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub opacity: f64,
    pub softness: f64,
}
impl VrTransfer2dRegion {
    fn value_text(&self) -> String {
        format!(
            "{{IntensityMin:{} IntensityMax:{} GradientMin:{} GradientMax:{} R:{} G:{} B:{} Opacity:{} Softness:{}}}",
            float_v(self.intensity_min),
            float_v(self.intensity_max),
            float_v(self.gradient_min),
            float_v(self.gradient_max),
            float_v(self.r),
            float_v(self.g),
            float_v(self.b),
            float_v(self.opacity),
            float_v(self.softness)
        )
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        if !finite(self.intensity_min)
            || !finite(self.intensity_max)
            || self.intensity_max <= self.intensity_min
            || !finite(self.gradient_min)
            || !finite(self.gradient_max)
            || self.gradient_min < 0.0
            || self.gradient_max <= self.gradient_min
        {
            return Err(invalid(format!("region ranges {}", self.value_text())));
        }
        for channel in [self.r, self.g, self.b, self.opacity, self.softness] {
            if !unit_channel(channel) {
                return Err(invalid(format!("region channels {}", self.value_text())));
            }
        }
        Ok(())
    }
}
pub fn validate_vr_transfer_2d_regions(regions: &[VrTransfer2dRegion]) -> Result<(), RenderError> {
    if regions.is_empty() || regions.len() > MAX_VR_TRANSFER_2D_REGIONS {
        return Err(invalid(format!(
            "{} regions, 1 to {}",
            regions.len(),
            MAX_VR_TRANSFER_2D_REGIONS
        )));
    }
    for (index, region) in regions.iter().enumerate() {
        if let Err(e) = region.validate() {
            return Err(RenderError::new(
                ErrorKind::InvalidVrTransfer2d,
                format!("region {index}: {e}"),
            ));
        }
    }
    Ok(())
}
pub fn bake_vr_transfer_2d_regions(
    regions: &[VrTransfer2dRegion],
    width: i64,
    height: i64,
    intensity_min: f64,
    intensity_max: f64,
    gradient_max: f64,
) -> Result<VrTransfer2d, RenderError> {
    validate_vr_transfer_2d_regions(regions)?;
    validate_shape(width, height, intensity_min, intensity_max, gradient_max)?;
    let linear: Vec<(VrTransfer2dRegion, f64, f64, f64)> = regions
        .iter()
        .map(|region| {
            (
                *region,
                srgb_to_linear(region.r),
                srgb_to_linear(region.g),
                srgb_to_linear(region.b),
            )
        })
        .collect();
    let ramp =
        |v: f64, lower: f64, upper: f64, domain_lower: f64, domain_upper: f64, softness: f64| {
            if v < lower || v > upper {
                return 0.0;
            }
            let feather = softness * (upper - lower) / 2.0;
            if feather <= 0.0 {
                return 1.0;
            }
            let mut weight = 1.0;
            if lower > domain_lower {
                weight = fmath::min(weight, (v - lower) / feather);
            }
            if upper < domain_upper {
                weight = fmath::min(weight, (upper - v) / feather);
            }
            clamp_unit(weight)
        };
    let (w, h) = (width as usize, height as usize);
    let mut table = vec![Rgba::default(); w * h];
    for row in 0..h {
        let gradient = row as f64 / (height - 1) as f64 * gradient_max;
        for column in 0..w {
            let value = intensity_min
                + column as f64 / (width - 1) as f64 * (intensity_max - intensity_min);
            let (mut transparency, mut weight) = (1.0f64, 0.0f64);
            let (mut r, mut g, mut b) = (0.0f64, 0.0f64, 0.0f64);
            for (region, lr, lg, lb) in &linear {
                let alpha = region.opacity
                    * ramp(
                        value,
                        region.intensity_min,
                        region.intensity_max,
                        intensity_min,
                        intensity_max,
                        region.softness,
                    )
                    * ramp(
                        gradient,
                        region.gradient_min,
                        region.gradient_max,
                        0.0,
                        gradient_max,
                        region.softness,
                    );
                if alpha <= 0.0 {
                    continue;
                }
                transparency *= 1.0 - alpha;
                weight += alpha;
                r += alpha * lr;
                g += alpha * lg;
                b += alpha * lb;
            }
            if weight > 0.0 {
                table[row * w + column] = Rgba {
                    r: clamp_unit(r / weight),
                    g: clamp_unit(g / weight),
                    b: clamp_unit(b / weight),
                    a: clamp_unit(1.0 - transparency),
                };
            }
        }
    }
    Ok(VrTransfer2d::build(
        width,
        height,
        intensity_min,
        intensity_max,
        gradient_max,
        table,
    ))
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrTransfer2dState {
    pub width: u32,
    pub height: u32,
    pub intensity_min: f64,
    pub intensity_max: f64,
    pub gradient_max: f64,
    pub samples: Vec<TransferSample>,
}
impl VrTransfer2dState {
    pub fn enabled(&self) -> bool {
        self.width != 0 || self.height != 0
    }
    pub fn payload_bytes(&self) -> Result<u64, RenderError> {
        if !self.enabled() {
            if self.intensity_min != 0.0 || self.intensity_max != 0.0 || self.gradient_max != 0.0 {
                return Err(bare_invalid());
            }
            return Ok(0);
        }
        let side = MAX_VR_TRANSFER_2D_SIDE as u32;
        if self.width < 2
            || self.height < 2
            || self.width > side
            || self.height > side
            || !finite(self.intensity_min)
            || !finite(self.intensity_max)
            || self.intensity_max <= self.intensity_min
            || !finite(self.gradient_max)
            || self.gradient_max <= 0.0
        {
            return Err(bare_invalid());
        }
        Ok(u64::from(self.width) * u64::from(self.height) * 16)
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        let bytes = self.payload_bytes()?;
        if self.samples.len() as u64 * 16 != bytes {
            return Err(invalid("incomplete GPU table".into()));
        }
        for sample in &self.samples {
            for value in [sample.r, sample.g, sample.b, sample.a] {
                if value.is_nan() || !(0.0..=1.0).contains(&value) {
                    return Err(invalid("invalid GPU table channel".into()));
                }
            }
        }
        Ok(())
    }
    pub fn from_table(table: Option<&VrTransfer2d>) -> VrTransfer2dState {
        let Some(table) = table else {
            return VrTransfer2dState::default();
        };
        let (w, h) = table.size();
        let (low, high) = table.intensity_range();
        VrTransfer2dState {
            width: w as u32,
            height: h as u32,
            intensity_min: low,
            intensity_max: high,
            gradient_max: table.gradient_max(),
            samples: table
                .table
                .iter()
                .map(|v| TransferSample {
                    r: v.r as f32,
                    g: v.g as f32,
                    b: v.b as f32,
                    a: v.a as f32,
                })
                .collect(),
        }
    }
}
