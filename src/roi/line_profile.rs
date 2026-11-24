#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::Vec3;
use crate::render::grid::{clamp_continuous_index, nearest_index};
use crate::render::view_state::Interpolation;
use crate::render::volume::Volume;
use crate::render::volume_sampler::VolumeReader;
use crate::textfmt::float_v;
use std::sync::Arc;
pub const MAX_PROFILE_SAMPLES: i64 = 1 << 20;
const PROFILE_ENDPOINT_TOLERANCE: f64 = 1e-6;
const IMAGE_PLANE_TOLERANCE_MM: f64 = 1e-6;
pub trait ProfileSampler {
    fn sample_mm(&self, p: Vec3, mode: Interpolation) -> Option<f64>;
    fn validate(&self) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProfileSample {
    pub distance_mm: f64,
    pub point: Vec3,
    pub value: f64,
    pub valid: bool,
}
pub fn line_profile(
    sampler: Option<&dyn ProfileSampler>,
    a: Vec3,
    b: Vec3,
    step_mm: f64,
    mode: Interpolation,
) -> Result<Vec<ProfileSample>, Error> {
    line_profile_context(None, sampler, a, b, step_mm, mode)
}
pub fn line_profile_context(
    ctx: Option<&dyn Cancellation>,
    sampler: Option<&dyn ProfileSampler>,
    a: Vec3,
    b: Vec3,
    step_mm: f64,
    mode: Interpolation,
) -> Result<Vec<ProfileSample>, Error> {
    let Some(sampler) = sampler else {
        return Err(Error::msg("roi: line profile needs a sampler"));
    };
    sampler.validate()?;
    if mode != Interpolation::UNKNOWN
        && mode != Interpolation::NEAREST
        && mode != Interpolation::LINEAR
        && mode != Interpolation::CUBIC
    {
        return Err(Error::msg(format!(
            "roi: unsupported line profile interpolation {}",
            mode.0
        )));
    }
    if !finite_vec3(a) || !finite_vec3(b) {
        return Err(Error::msg("roi: line profile end points must be finite"));
    }
    let (intervals, length) = profile_intervals(a, b, step_mm)?;
    let delta = b.sub(a);
    let mut samples = Vec::with_capacity(intervals as usize + 1);
    for i in 0..=intervals {
        if i & 1023 == 0
            && let Some(err) = ctx_err(ctx)
        {
            return Err(err);
        }
        let (distance, point) = if i == 0 {
            (0.0, a)
        } else if i == intervals {
            (length, b)
        } else {
            let distance = i as f64 * step_mm;
            (distance, a.add(delta.scale(distance / length)))
        };
        let value = sampler.sample_mm(point, mode).filter(|v| v.is_finite());
        samples.push(ProfileSample {
            distance_mm: distance,
            point,
            value: value.unwrap_or(0.0),
            valid: value.is_some(),
        });
    }
    Ok(samples)
}
fn profile_intervals(a: Vec3, b: Vec3, step_mm: f64) -> Result<(i64, f64), Error> {
    if !(step_mm > 0.0) || step_mm.is_infinite() {
        return Err(Error::msg(format!(
            "roi: line profile step {} mm must be finite and positive",
            float_v(step_mm)
        )));
    }
    let length = b.sub(a).length();
    if !length.is_finite() {
        return Err(Error::msg("roi: line profile length is not finite"));
    }
    if length == 0.0 {
        return Ok((0, 0.0));
    }
    let steps = length / step_mm;
    if steps >= MAX_PROFILE_SAMPLES as f64 {
        return Err(Error::msg(format!(
            "roi: line profile of {} mm at {} mm needs more than {} samples",
            float_v(length),
            float_v(step_mm),
            MAX_PROFILE_SAMPLES
        )));
    }
    let intervals = fmath::f64_to_i64((steps - PROFILE_ENDPOINT_TOLERANCE).ceil()).max(1);
    Ok((intervals, length))
}
fn finite_vec3(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}
#[derive(Clone, Copy)]
pub struct VolumeProfileSampler<'a> {
    pub reader: Option<&'a VolumeReader>,
}
impl ProfileSampler for VolumeProfileSampler<'_> {
    fn sample_mm(&self, p: Vec3, mode: Interpolation) -> Option<f64> {
        self.reader?.sample_patient_at(p, mode)
    }
    fn validate(&self) -> Result<(), Error> {
        if self.reader.is_none() {
            return Err(Error::msg("roi: volume line profile needs a volume reader"));
        }
        Ok(())
    }
}
pub fn volume_line_profile(
    ctx: Option<&dyn Cancellation>,
    volume: Option<&Arc<Volume>>,
    a: Vec3,
    b: Vec3,
    step_mm: f64,
    mode: Interpolation,
) -> Result<Vec<ProfileSample>, Error> {
    let Some(volume) = volume else {
        return Err(Error::msg("roi: volume line profile needs a volume"));
    };
    let reader = volume.acquire_reader()?;
    let sampler = VolumeProfileSampler {
        reader: Some(&reader),
    };
    line_profile_context(ctx, Some(&sampler), a, b, step_mm, mode)
}
pub type PixelValueAt<'a> = &'a dyn Fn(i64, i64) -> Option<f64>;
#[derive(Clone, Copy)]
pub struct ImageProfileSampler<'a> {
    pub columns: i64,
    pub rows: i64,
    pub column_spacing_mm: f64,
    pub row_spacing_mm: f64,
    pub value_at: Option<PixelValueAt<'a>>,
}
impl ImageProfileSampler<'_> {
    pub fn point_mm(&self, x: f64, y: f64) -> Vec3 {
        Vec3::new(x * self.column_spacing_mm, y * self.row_spacing_mm, 0.0)
    }
    fn weighted_sum(
        &self,
        value_at: PixelValueAt<'_>,
        xi: &[i64],
        xw: &[f64],
        yi: &[i64],
        yw: &[f64],
    ) -> Option<f64> {
        let mut sum = 0.0;
        for j in 0..yi.len() {
            let mut row = 0.0;
            for i in 0..xi.len() {
                let value = value_at(xi[i], yi[j])?;
                row += xw[i] * value;
            }
            sum += yw[j] * row;
        }
        Some(sum)
    }
}
impl ProfileSampler for ImageProfileSampler<'_> {
    fn validate(&self) -> Result<(), Error> {
        if self.columns <= 0 || self.rows <= 0 {
            return Err(Error::msg(format!(
                "roi: image line profile needs positive dimensions, got {}x{}",
                self.columns, self.rows
            )));
        }
        if !(self.column_spacing_mm > 0.0)
            || self.column_spacing_mm.is_infinite()
            || !(self.row_spacing_mm > 0.0)
            || self.row_spacing_mm.is_infinite()
        {
            return Err(Error::msg(format!(
                "roi: image line profile needs finite positive spacing, got {} x {} mm",
                float_v(self.column_spacing_mm),
                float_v(self.row_spacing_mm)
            )));
        }
        if self.value_at.is_none() {
            return Err(Error::msg("roi: image line profile needs a pixel accessor"));
        }
        Ok(())
    }
    fn sample_mm(&self, p: Vec3, mode: Interpolation) -> Option<f64> {
        if self.validate().is_err() || p.z.abs() > IMAGE_PLANE_TOLERANCE_MM {
            return None;
        }
        let value_at = self.value_at?;
        let x = clamp_to_pixel_grid(p.x / self.column_spacing_mm, self.columns);
        let y = clamp_to_pixel_grid(p.y / self.row_spacing_mm, self.rows);
        let (x, y) = (x?, y?);
        if mode == Interpolation::NEAREST {
            let nearest = |v: f64, n: i64| nearest_index(v).min(n - 1);
            return value_at(nearest(x, self.columns), nearest(y, self.rows));
        }
        if mode == Interpolation::CUBIC {
            let (xi, xw, xn) = catmull_rom_taps(x, self.columns);
            let (yi, yw, yn) = catmull_rom_taps(y, self.rows);
            return self.weighted_sum(value_at, &xi[..xn], &xw[..xn], &yi[..yn], &yw[..yn]);
        }
        let (xi, xw, xn) = linear_taps(x, self.columns);
        let (yi, yw, yn) = linear_taps(y, self.rows);
        self.weighted_sum(value_at, &xi[..xn], &xw[..xn], &yi[..yn], &yw[..yn])
    }
}
fn clamp_to_pixel_grid(v: f64, n: i64) -> Option<f64> {
    clamp_continuous_index(v, n, 1e-3)
}
fn linear_taps(v: f64, n: i64) -> ([i64; 2], [f64; 2], usize) {
    let base = fmath::f64_to_i64(v.floor());
    let f = v - base as f64;
    if f == 0.0 || base >= n - 1 {
        return ([base.min(n - 1), 0], [1.0, 0.0], 1);
    }
    ([base, base + 1], [1.0 - f, f], 2)
}
fn catmull_rom_taps(v: f64, n: i64) -> ([i64; 4], [f64; 4], usize) {
    let base = fmath::f64_to_i64(v.floor());
    let f = v - base as f64;
    if f == 0.0 {
        return ([base.min(n - 1), 0, 0, 0], [1.0, 0.0, 0.0, 0.0], 1);
    }
    let (f2, f3) = (f * f, f * f * f);
    let weight = [
        (-f3 + 2.0 * f2 - f) / 2.0,
        (3.0 * f3 - 5.0 * f2 + 2.0) / 2.0,
        (-3.0 * f3 + 4.0 * f2 + f) / 2.0,
        (f3 - f2) / 2.0,
    ];
    let mut index = [0i64; 4];
    for (i, slot) in index.iter_mut().enumerate() {
        *slot = (base - 1 + i as i64).min(n - 1).max(0);
    }
    (index, weight, 4)
}
pub fn bresenham_line(p0: &[i64], p1: &[i64]) -> Result<Vec<Vec<i64>>, Error> {
    if p0.is_empty() || p0.len() != p1.len() {
        return Err(Error::msg(format!(
            "roi: Bresenham line end points need the same non-zero dimension, got {} and {}",
            p0.len(),
            p1.len()
        )));
    }
    let dims = p0.len();
    let mut abs_delta = vec![0i64; dims];
    let mut step = vec![1i64; dims];
    let (mut main_axis, mut max_abs_delta) = (0usize, 0i64);
    for i in 0..dims {
        let mut delta = p1[i].wrapping_sub(p0[i]);
        if delta < 0 {
            step[i] = -1;
            delta = delta.wrapping_neg();
        }
        abs_delta[i] = delta;
        if delta > max_abs_delta {
            main_axis = i;
            max_abs_delta = delta;
        }
    }
    if max_abs_delta >= MAX_PROFILE_SAMPLES {
        return Err(Error::msg(format!(
            "roi: Bresenham line of {max_abs_delta} steps exceeds {MAX_PROFILE_SAMPLES} points"
        )));
    }
    let mut points = Vec::with_capacity(max_abs_delta as usize + 1);
    let mut current = p0.to_vec();
    let mut accumulated = vec![0i64; dims];
    for _ in 0..=max_abs_delta {
        points.push(current.clone());
        for i in 0..dims {
            if i == main_axis {
                current[i] = current[i].wrapping_add(step[i]);
                continue;
            }
            accumulated[i] = accumulated[i].wrapping_add(abs_delta[i].wrapping_mul(2));
            if accumulated[i] >= max_abs_delta {
                current[i] = current[i].wrapping_add(step[i]);
                accumulated[i] = accumulated[i].wrapping_sub(max_abs_delta.wrapping_mul(2));
            }
        }
    }
    Ok(points)
}
