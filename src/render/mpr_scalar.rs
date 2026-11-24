#![forbid(unsafe_code)]
use super::error::RenderError;
use super::frame::{RenderImage, blank_image, display_gray_mapped, pixel_value_at};
use super::grid::{clamp_continuous_index, nearest_index};
use super::mpr::{MeasureSpacing, MprPlane, Plane};
use super::mpr_render::{
    GantryTiltRenderMode, MAX_OBLIQUE_OUTPUT_DIMENSION, MAX_OBLIQUE_SLAB_SAMPLES, MprLimits,
    MprRenderOptions, SlabMode, clamp_index, invalid_field, limited_field, mip_projection_range,
    normalize_mpr_limits, orthogonal_display_slice_index, orthogonal_mpr_height,
    orthogonal_volume_height, stack_volume_sampler, valid_mpr_plane, validate_mpr_output,
    validate_mpr_reslice,
};
use super::parallel::{Cancellation, ctx_err, parallel_rows, parallel_rows_worker_context};
use super::vec3::Vec3;
use super::view_state::Interpolation;
use super::volume::series_pixel_spacing;
use super::volume::{
    Sampler, Stack, Volume, VoxelLine, checked_voxel_count, first_renderable_slice,
};
use super::volume_interpolation::{cubic_taps, sample_at};
use super::vr_surface::gradient_at;
use super::vr_transfer::VrLightingMaterial;
use super::vr_volume::{VR_OPACITY_THRESHOLD, headlight_shading};
use super::window::{
    DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH, WindowLevel, normalize_window, prepare_window,
};
use crate::fmath;
use std::sync::{Arc, OnceLock};
const DEFAULT_OBLIQUE_WORKING_SET_BYTES: i64 = 64 << 20;
const VOXEL_BORDER_TOLERANCE: f64 = 1e-3;
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarPlane {
    pub(crate) width: i64,
    pub(crate) height: i64,
    pub(crate) values: Vec<f64>,
    pub(crate) valid: Vec<u8>,
    pub(crate) photometric: String,
    pub(crate) default_voi: WindowLevel,
    pub(crate) antialiased: bool,
}
fn default_scalar_voi() -> WindowLevel {
    WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH)
}
impl ScalarPlane {
    pub(crate) fn new(
        width: i64,
        height: i64,
        slab_samples: i64,
        photometric: &str,
        default_voi: &WindowLevel,
        limits: MprLimits,
    ) -> Result<ScalarPlane, RenderError> {
        validate_mpr_output(width, height, slab_samples, 9, limits)?;
        let count = (width * height) as usize;
        Ok(ScalarPlane {
            width,
            height,
            values: vec![0.0; count],
            valid: vec![0; count],
            photometric: photometric.to_string(),
            default_voi: normalize_window(default_voi, &default_scalar_voi()),
            antialiased: false,
        })
    }
    pub fn antialiased(&self) -> bool {
        self.antialiased
    }
    pub fn dimensions(&self) -> (i64, i64) {
        (self.width, self.height)
    }
    pub fn bytes(&self) -> i64 {
        self.values.len() as i64 * 8 + self.valid.len() as i64
    }
    pub fn photometric(&self) -> &str {
        &self.photometric
    }
    pub(crate) fn default_voi(&self) -> &WindowLevel {
        &self.default_voi
    }
    pub fn value_at(&self, x: i64, y: i64) -> Option<f64> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let index = (y * self.width + x) as usize;
        if index >= self.values.len() || index >= self.valid.len() || self.valid[index] == 0 {
            return None;
        }
        Some(self.values[index])
    }
    pub fn apply_window(&self, window: &WindowLevel) -> RenderImage {
        self.apply_window_context(None, window)
            .unwrap_or_else(|_| blank_image(512, 512))
    }
    pub fn apply_window_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        window: &WindowLevel,
    ) -> Result<RenderImage, RenderError> {
        let n = self.width.wrapping_mul(self.height);
        if self.width <= 0
            || self.height <= 0
            || (self.values.len() as i64) < n
            || (self.valid.len() as i64) < n
        {
            return Ok(blank_image(512, 512));
        }
        let mapper = prepare_window(&normalize_window(window, &self.default_voi));
        let w = self.width as usize;
        let rows = gather_rows(ctx, self.height, w, || {
            |y: i64, out: &mut [u8]| {
                let offset = y as usize * w;
                for (x, px) in out.iter_mut().enumerate() {
                    if self.valid[offset + x] != 0 {
                        *px = display_gray_mapped(
                            self.values[offset + x],
                            &mapper,
                            &self.photometric,
                        );
                    }
                }
            }
        })?;
        Ok(RenderImage::gray(w, self.height as usize, rows.concat()))
    }
    fn fill_rows(&mut self, rows: Vec<(Vec<f64>, Vec<u8>)>) {
        let w = self.width as usize;
        for (y, (values, valid)) in rows.into_iter().enumerate() {
            self.values[y * w..(y + 1) * w].copy_from_slice(&values);
            self.valid[y * w..(y + 1) * w].copy_from_slice(&valid);
        }
    }
}
pub(crate) fn gather_rows<T, F, W>(
    ctx: Option<&dyn Cancellation>,
    height: i64,
    width: usize,
    new_worker: F,
) -> Result<Vec<Vec<T>>, RenderError>
where
    T: Clone + Default + Send + Sync,
    F: Fn() -> W + Sync,
    W: FnMut(i64, &mut [T]),
{
    let slots: Vec<OnceLock<Vec<T>>> = (0..height.max(0)).map(|_| OnceLock::new()).collect();
    parallel_rows_worker_context(ctx, height, || {
        let mut inner = new_worker();
        let slots = &slots;
        move |y: i64| {
            let mut row = vec![T::default(); width];
            inner(y, &mut row);
            let _ = slots[y as usize].set(row);
        }
    })?;
    Ok(slots
        .into_iter()
        .map(|s| s.into_inner().unwrap_or_else(|| vec![T::default(); width]))
        .collect())
}
fn scalar_rows<F, W>(
    ctx: Option<&dyn Cancellation>,
    plane: &mut ScalarPlane,
    new_worker: F,
) -> Result<(), RenderError>
where
    F: Fn() -> W + Sync,
    W: FnMut(i64, &mut [f64], &mut [u8]),
{
    let width = plane.width as usize;
    let rows = gather_rows::<(f64, u8), _, _>(ctx, plane.height, width, || {
        let mut inner = new_worker();
        let mut values = vec![0.0; width];
        let mut valid = vec![0u8; width];
        move |y: i64, out: &mut [(f64, u8)]| {
            values.fill(0.0);
            valid.fill(0);
            inner(y, &mut values, &mut valid);
            for (x, cell) in out.iter_mut().enumerate() {
                *cell = (values[x], valid[x]);
            }
        }
    })?;
    plane.fill_rows(
        rows.into_iter()
            .map(|row| row.into_iter().unzip())
            .collect(),
    );
    Ok(())
}
fn row_set(values: &mut [f64], valid: &mut [u8], x: usize, value: Option<f64>) {
    if let Some(v) = value {
        values[x] = v;
        valid[x] = 1;
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SlabSampling {
    pub trapezoid: bool,
    pub spacing_fraction: f64,
}
impl SlabSampling {
    pub fn sample_count(&self, coarse_samples: i64, limits: MprLimits) -> Result<i64, RenderError> {
        let mut fraction = self.spacing_fraction;
        if fraction == 0.0 {
            fraction = 1.0;
        }
        if !(fraction > 0.0 && fraction <= 1.0) {
            return Err(invalid_field("Slab.SpacingFraction"));
        }
        let limits = normalize_mpr_limits(limits)?;
        if coarse_samples < 1 {
            return Err(invalid_field("SlabSamples"));
        }
        let intervals = ((coarse_samples - 1) as f64 / fraction).round();
        if intervals.is_infinite() || intervals > (limits.max_slab_samples - 1) as f64 {
            return Err(limited_field("SlabSamples"));
        }
        Ok(fmath::f64_to_i64(intervals) + 1)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MprScalarOptions {
    pub gantry_tilt_mode: GantryTiltRenderMode,
    pub limits: MprLimits,
    pub interpolation: Interpolation,
    pub slab: SlabSampling,
}
impl From<MprRenderOptions> for MprScalarOptions {
    fn from(options: MprRenderOptions) -> MprScalarOptions {
        MprScalarOptions {
            gantry_tilt_mode: options.gantry_tilt_mode,
            limits: options.limits,
            ..MprScalarOptions::default()
        }
    }
}
impl MprScalarOptions {
    pub fn image_options(&self) -> MprRenderOptions {
        MprRenderOptions {
            gantry_tilt_mode: self.gantry_tilt_mode,
            limits: self.limits,
        }
    }
    pub fn with_slab(mut self, slab: SlabSampling) -> MprScalarOptions {
        self.slab = slab;
        self
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MprResliceOptions {
    pub limits: MprLimits,
    pub antialias_2x: bool,
    pub slab: SlabSampling,
    pub interpolation: Interpolation,
}
fn slab_mode(code: i64) -> Result<SlabMode, RenderError> {
    SlabMode::from_code(code).ok_or_else(|| invalid_field("SlabMode"))
}
fn scalar_projection_range(center: i64, thickness: i64, count: i64, mode: SlabMode) -> (i64, i64) {
    let thickness = if mode == SlabMode::None { 1 } else { thickness };
    mip_projection_range(center, thickness, count)
}
pub fn render_scalar_slab(
    ctx: Option<&dyn Cancellation>,
    series: Option<&Stack>,
    plane: &str,
    center: i64,
    thickness: i64,
    mode: i64,
    options: MprScalarOptions,
) -> Result<ScalarPlane, RenderError> {
    let Some(series) = series.filter(|s| !s.frames.is_empty()) else {
        return Err(RenderError::other("render: no series selected"));
    };
    let Some(first) = first_renderable_slice(series) else {
        return Err(RenderError::other(
            "render: series has no displayable images",
        ));
    };
    let rows = i64::from(first.metadata.rows);
    let cols = i64::from(first.metadata.columns);
    if rows <= 0 || cols <= 0 {
        return Err(RenderError::other("render: invalid slice dimensions"));
    }
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let mode = slab_mode(mode)?;
    let limits = normalize_mpr_limits(options.limits)?;
    let plane = MprPlane::parse(plane).ok_or_else(|| invalid_field("Plane"))?;
    let axis_count = match plane {
        MprPlane::Axial => series.frames.len() as i64,
        MprPlane::Coronal => rows,
        MprPlane::Sagittal => cols,
    };
    let (start, end) = scalar_projection_range(center, thickness, axis_count, mode);
    options.slab.sample_count(end - start + 1, limits)?;
    let correct_tilt = options.gantry_tilt_mode != GantryTiltRenderMode::SourceGeometry;
    let default_voi = normalize_window(&series.default_window, &default_scalar_voi());
    if plane == MprPlane::Axial && mode == SlabMode::None {
        return render_fallback_scalar_slab(
            ctx,
            series,
            plane,
            center,
            thickness,
            mode,
            rows,
            cols,
            &default_voi,
            limits,
            options,
        );
    }
    match stack_volume_sampler(series, correct_tilt) {
        Ok((vol, sampler)) => {
            if let Some(result) = render_cached_scalar_slab(
                ctx,
                &vol,
                &sampler,
                plane,
                center,
                thickness,
                mode,
                correct_tilt,
                &default_voi,
                limits,
                options,
            ) {
                return result;
            }
        }
        Err(e) if e.is(super::error::ErrorKind::NonCorrectableGeometry) => return Err(e),
        Err(_) => {}
    }
    render_fallback_scalar_slab(
        ctx,
        series,
        plane,
        center,
        thickness,
        mode,
        rows,
        cols,
        &default_voi,
        limits,
        options,
    )
}
fn corrected_slab_axis(vol: &Volume, plane: MprPlane) -> (Vec3, f64, i64) {
    match plane {
        MprPlane::Coronal => (vol.axis_y, vol.row_spacing, vol.rows),
        MprPlane::Sagittal => (vol.axis_x, vol.col_spacing, vol.cols),
        MprPlane::Axial => (Vec3::ZERO, 0.0, 0),
    }
}
#[allow(clippy::too_many_arguments)]
fn render_cached_scalar_slab(
    ctx: Option<&dyn Cancellation>,
    vol: &Volume,
    sampler: &Sampler,
    plane: MprPlane,
    center: i64,
    thickness: i64,
    mode: SlabMode,
    correct_tilt: bool,
    default_voi: &WindowLevel,
    limits: MprLimits,
    options: MprScalarOptions,
) -> Option<Result<ScalarPlane, RenderError>> {
    if sampler.rows <= 0 || sampler.cols <= 0 || sampler.depth <= 0 {
        return None;
    }
    if correct_tilt
        && let Some((corrected, width, height)) = vol.corrected_orthogonal_plane(plane, center)
    {
        if mode == SlabMode::None || thickness <= 1 {
            return Some(reslice_scalar_with_sampler(
                ctx,
                sampler,
                &corrected,
                width,
                height,
                default_voi,
                limits,
                options.interpolation,
            ));
        }
        let (slab_axis, mut step, sample_count) = corrected_slab_axis(vol, plane);
        let clamped = clamp_index(center, sample_count);
        let (start, end) = mip_projection_range(clamped, thickness, sample_count);
        let samples = match options.slab.sample_count(end - start + 1, limits) {
            Ok(s) => s,
            Err(e) => return Some(Err(e)),
        };
        let mut first_offset = (start - clamped) as f64;
        if samples > 1 && samples != end - start + 1 {
            let refined = step * (end - start) as f64 / (samples - 1) as f64;
            first_offset *= step / refined;
            step = refined;
        }
        return Some(reslice_scalar_slab_with_sampler(
            ctx,
            sampler,
            &corrected,
            slab_axis,
            width,
            height,
            first_offset,
            samples,
            step,
            mode,
            default_voi,
            limits,
            options.interpolation,
            options.slab.trapezoid,
        ));
    }
    let (rows, cols, depth) = (sampler.rows, sampler.cols, sampler.depth);
    let (axis_count, width, height) = match plane {
        MprPlane::Axial => (depth, cols, rows),
        MprPlane::Coronal => (
            rows,
            cols,
            orthogonal_volume_height(depth, vol.slice_spacing, vol.col_spacing),
        ),
        MprPlane::Sagittal => (
            cols,
            rows,
            orthogonal_volume_height(depth, vol.slice_spacing, vol.row_spacing),
        ),
    };
    let (start, end) = scalar_projection_range(center, thickness, axis_count, mode);
    let result = (|| {
        let samples = options.slab.sample_count(end - start + 1, limits)?;
        let mut scalar = ScalarPlane::new(
            width,
            height,
            samples,
            &sampler.photometric,
            default_voi,
            limits,
        )?;
        render_cached_scalar_slab_rows(
            ctx,
            &mut scalar,
            sampler,
            plane,
            start,
            end,
            samples,
            mode,
            options.interpolation,
            options.slab.trapezoid,
        )?;
        Ok(scalar)
    })();
    Some(result)
}
#[allow(clippy::too_many_arguments)]
fn render_fallback_scalar_slab(
    ctx: Option<&dyn Cancellation>,
    series: &Stack,
    plane: MprPlane,
    center: i64,
    thickness: i64,
    mode: SlabMode,
    rows: i64,
    cols: i64,
    default_voi: &WindowLevel,
    limits: MprLimits,
    options: MprScalarOptions,
) -> Result<ScalarPlane, RenderError> {
    let photometric = first_renderable_slice(series)
        .map(|f| f.metadata.photometric_interpretation.clone())
        .unwrap_or_default();
    let n = series.frames.len() as i64;
    let interpolation = options.interpolation;
    let trapezoid = options.slab.trapezoid;
    let sample = |frame: Option<&super::frame::Frame>, x: i64, y: i64| -> Option<f64> {
        pixel_value_at(frame, x, y).map(|v| v.rescaled)
    };
    let (axis_count, width, height) = match plane {
        MprPlane::Axial => (n, cols, rows),
        MprPlane::Coronal => {
            let (_, column) = series_pixel_spacing(series);
            (rows, cols, orthogonal_mpr_height(series, column))
        }
        MprPlane::Sagittal => {
            let (row, _) = series_pixel_spacing(series);
            (cols, rows, orthogonal_mpr_height(series, row))
        }
    };
    let (start, end) = scalar_projection_range(center, thickness, axis_count, mode);
    let samples = options.slab.sample_count(end - start + 1, limits)?;
    let mut scalar = ScalarPlane::new(width, height, samples, &photometric, default_voi, limits)?;
    let w = width as usize;
    scalar_rows(ctx, &mut scalar, || {
        |y: i64, values: &mut [f64], valid: &mut [u8]| {
            let frame = match plane {
                MprPlane::Axial => None,
                _ => series.frame(orthogonal_display_slice_index(y, height, n) as usize),
            };
            for x in 0..w {
                let xi = x as i64;
                let value = reduce_sampled_scalar(mode, start, end, samples, trapezoid, |c| {
                    interpolate_scalar_axis(c, axis_count, interpolation, |k| match plane {
                        MprPlane::Axial => sample(series.frame(k as usize), xi, y),
                        MprPlane::Coronal => sample(frame, xi, k),
                        MprPlane::Sagittal => sample(frame, k, xi),
                    })
                });
                row_set(values, valid, x, value);
            }
        }
    })?;
    Ok(scalar)
}
fn interpolate_scalar_axis(
    coordinate: f64,
    count: i64,
    interpolation: Interpolation,
    sample: impl Fn(i64) -> Option<f64>,
) -> Option<f64> {
    if coordinate == coordinate.trunc() {
        return sample(fmath::f64_to_i64(coordinate));
    }
    if interpolation == Interpolation::NEAREST {
        return sample(nearest_index(coordinate).min(count - 1));
    }
    if interpolation == Interpolation::CUBIC {
        let (indices, weights, taps) = cubic_taps(coordinate, count);
        let mut value = 0.0;
        for k in 0..taps {
            value += weights[k] * sample(indices[k])?;
        }
        return Some(value);
    }
    let lo = fmath::f64_to_i64(coordinate.floor());
    let a = sample(lo);
    let b = sample((lo + 1).min(count - 1));
    let (a, b) = (a?, b?);
    let fraction = coordinate - lo as f64;
    Some(a * (1.0 - fraction) + b * fraction)
}
fn scalar_slab_coordinate(start: i64, end: i64, samples: i64, k: i64) -> f64 {
    let mut coordinate = start as f64;
    if samples > 1 {
        coordinate += k as f64 * (end - start) as f64 / (samples - 1) as f64;
        if k == samples - 1 {
            coordinate = end as f64;
        }
    }
    coordinate
}
fn reduce_sampled_scalar(
    mode: SlabMode,
    start: i64,
    end: i64,
    count: i64,
    trapezoid: bool,
    sample: impl Fn(f64) -> Option<f64>,
) -> Option<f64> {
    let at = |k: i64| sample(scalar_slab_coordinate(start, end, count, k));
    if trapezoid && (mode == SlabMode::Average || mode == SlabMode::Sum) {
        return reduce_trapezoid(mode, count, at);
    }
    reduce_cached_scalar(mode, 0, count - 1, at)
}
fn reduce_trapezoid(
    mode: SlabMode,
    count: i64,
    mut sample: impl FnMut(i64) -> Option<f64>,
) -> Option<f64> {
    let (mut sum, mut weight) = (0.0, 0.0);
    for k in 0..count {
        let Some(value) = sample(k) else { continue };
        let w = if count > 1 && (k == 0 || k == count - 1) {
            0.5
        } else {
            1.0
        };
        sum += w * value;
        weight += w;
    }
    if weight == 0.0 {
        return None;
    }
    if mode == SlabMode::Average {
        return Some(sum / weight);
    }
    Some(sum)
}
fn reduce_cached_scalar(
    mode: SlabMode,
    start: i64,
    end: i64,
    mut sample: impl FnMut(i64) -> Option<f64>,
) -> Option<f64> {
    let mut acc = 0.0;
    let mut count = 0i64;
    for index in start..=end {
        let Some(value) = sample(index) else { continue };
        match mode {
            SlabMode::MinIp => {
                if count == 0 || value < acc {
                    acc = value;
                }
            }
            SlabMode::Average | SlabMode::Sum => acc += value,
            _ => {
                if count == 0 || value > acc {
                    acc = value;
                }
            }
        }
        count += 1;
    }
    if count == 0 {
        return None;
    }
    if mode == SlabMode::Average {
        return Some(acc / count as f64);
    }
    Some(acc)
}
pub fn reslice_oblique_scalar(
    ctx: Option<&dyn Cancellation>,
    vol: Option<&Arc<Volume>>,
    plane: &Plane,
    out_w: i64,
    out_h: i64,
    options: MprResliceOptions,
) -> Result<ScalarPlane, RenderError> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let limits = validate_mpr_reslice(plane, out_w, out_h, 1, 9, options.limits)?;
    if options.antialias_2x && options.interpolation == Interpolation::NEAREST {
        return Err(invalid_field("Antialias2x"));
    }
    let Some(vol) = vol else {
        return ScalarPlane::new(out_w, out_h, 1, "", &default_scalar_voi(), limits);
    };
    if options.antialias_2x {
        let (level, antialiased) = mpr_antialias_volume(ctx, vol, plane, out_w, out_h, limits)?;
        let sampler = Sampler::new(&level)?;
        let mut result = reslice_scalar_with_sampler(
            ctx,
            &sampler,
            plane,
            out_w,
            out_h,
            &default_scalar_voi(),
            limits,
            options.interpolation,
        )?;
        result.antialiased = antialiased;
        return Ok(result);
    }
    let Ok(sampler) = Sampler::new(vol) else {
        return ScalarPlane::new(
            out_w,
            out_h,
            1,
            vol.photometric(),
            &default_scalar_voi(),
            limits,
        );
    };
    reslice_scalar_with_sampler(
        ctx,
        &sampler,
        plane,
        out_w,
        out_h,
        &default_scalar_voi(),
        limits,
        options.interpolation,
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScalarSlab {
    pub thickness: i64,
    pub mode: i64,
}
pub fn reslice_oblique_slab_scalar(
    ctx: Option<&dyn Cancellation>,
    vol: Option<&Arc<Volume>>,
    plane: &Plane,
    out_w: i64,
    out_h: i64,
    slab: ScalarSlab,
    mut options: MprResliceOptions,
) -> Result<ScalarPlane, RenderError> {
    let ScalarSlab { thickness, mode } = slab;
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let mode = slab_mode(mode)?;
    let coarse = if thickness > 1 && mode != SlabMode::None {
        thickness
    } else {
        1
    };
    let slab_samples = options.slab.sample_count(coarse, options.limits)?;
    let limits = validate_mpr_reslice(plane, out_w, out_h, slab_samples, 9, options.limits)?;
    if thickness <= 1 || mode == SlabMode::None {
        options.limits = limits;
        return reslice_oblique_scalar(ctx, vol, plane, out_w, out_h, options);
    }
    if options.antialias_2x {
        return Err(invalid_field("Antialias2x"));
    }
    let Some(vol) = vol else {
        return ScalarPlane::new(
            out_w,
            out_h,
            slab_samples,
            "",
            &default_scalar_voi(),
            limits,
        );
    };
    let Ok(sampler) = Sampler::new(vol) else {
        return ScalarPlane::new(
            out_w,
            out_h,
            slab_samples,
            vol.photometric(),
            &default_scalar_voi(),
            limits,
        );
    };
    let half = (slab_samples - 1) as f64 / 2.0;
    let step = vol.slice_spacing * (thickness - 1) as f64 / (slab_samples - 1) as f64;
    reslice_scalar_slab_with_sampler(
        ctx,
        &sampler,
        plane,
        plane.u.cross(plane.v),
        out_w,
        out_h,
        -half,
        slab_samples,
        step,
        mode,
        &default_scalar_voi(),
        limits,
        options.interpolation,
        options.slab.trapezoid,
    )
}
fn denominator(n: i64) -> f64 {
    (n - 1).max(1) as f64
}
#[allow(clippy::too_many_arguments)]
fn reslice_scalar_with_sampler(
    ctx: Option<&dyn Cancellation>,
    sampler: &Sampler,
    plane: &Plane,
    out_w: i64,
    out_h: i64,
    default_voi: &WindowLevel,
    limits: MprLimits,
    interpolation: Interpolation,
) -> Result<ScalarPlane, RenderError> {
    let mut scalar = ScalarPlane::new(out_w, out_h, 1, &sampler.photometric, default_voi, limits)?;
    let denom_w = denominator(out_w);
    let denom_h = denominator(out_h);
    let column = |x: i64| x as f64 / denom_w;
    scalar_rows(ctx, &mut scalar, || {
        let mut plan = (interpolation == Interpolation::CUBIC)
            .then(|| CubicLinePlan::new(sampler, out_w, column));
        move |y: i64, values: &mut [f64], valid: &mut [u8]| {
            let t = y as f64 / denom_h;
            let row = sampler
                .vol
                .voxel_line(plane.origin.add(plane.v.scale(t)), plane.u);
            if let Some(plan) = plan.as_mut()
                && plan.prepare(&row)
            {
                for x in 0..out_w {
                    row_set(values, valid, x as usize, plan.sample(x as usize));
                }
                return;
            }
            for x in 0..out_w {
                let value = sample_at(sampler, row.at(column(x)), interpolation);
                row_set(values, valid, x as usize, value);
            }
        }
    })?;
    Ok(scalar)
}
#[allow(clippy::too_many_arguments)]
fn reslice_scalar_slab_with_sampler(
    ctx: Option<&dyn Cancellation>,
    sampler: &Sampler,
    plane: &Plane,
    slab_axis: Vec3,
    out_w: i64,
    out_h: i64,
    first_offset: f64,
    sample_count: i64,
    step: f64,
    mode: SlabMode,
    default_voi: &WindowLevel,
    limits: MprLimits,
    interpolation: Interpolation,
    trapezoid: bool,
) -> Result<ScalarPlane, RenderError> {
    let step = if step <= 0.0 { 1.0 } else { step };
    let normal = slab_axis.normalize();
    if scalar_slab_rows_eligible(sampler, normal, sample_count, mode, interpolation) {
        return reslice_scalar_slab_rows(
            ctx,
            sampler,
            plane,
            normal,
            out_w,
            out_h,
            first_offset,
            sample_count,
            step,
            mode,
            default_voi,
            limits,
            interpolation,
            trapezoid,
        );
    }
    let mut scalar = ScalarPlane::new(
        out_w,
        out_h,
        sample_count,
        &sampler.photometric,
        default_voi,
        limits,
    )?;
    let denom_w = denominator(out_w);
    let denom_h = denominator(out_h);
    let offset = |sample: i64| (first_offset + sample as f64) * step;
    scalar_rows(ctx, &mut scalar, || {
        let mut plan = (interpolation == Interpolation::CUBIC)
            .then(|| CubicLinePlan::new(sampler, sample_count, offset));
        move |y: i64, values: &mut [f64], valid: &mut [u8]| {
            let t = y as f64 / denom_h;
            for x in 0..out_w {
                let ray = sampler
                    .vol
                    .voxel_line(plane.at(x as f64 / denom_w, t), normal);
                let prepared = plan.as_mut().is_some_and(|p| p.prepare(&ray));
                let value = match plan.as_ref() {
                    Some(plan) if prepared => {
                        reduce_slab(mode, trapezoid, sample_count, |k| plan.sample(k as usize))
                    }
                    _ => reduce_slab(mode, trapezoid, sample_count, |k| {
                        sample_at(sampler, ray.at(offset(k)), interpolation)
                    }),
                };
                row_set(values, valid, x as usize, value);
            }
        }
    })?;
    Ok(scalar)
}
fn reduce_slab(
    mode: SlabMode,
    trapezoid: bool,
    count: i64,
    sample: impl FnMut(i64) -> Option<f64>,
) -> Option<f64> {
    if trapezoid && (mode == SlabMode::Average || mode == SlabMode::Sum) {
        reduce_trapezoid(mode, count, sample)
    } else {
        reduce_cached_scalar(mode, 0, count - 1, sample)
    }
}
struct ScalarSlabRow {
    weights: Vec<f64>,
    mode: SlabMode,
    trapezoid: bool,
}
impl ScalarSlabRow {
    fn new(width: usize, samples: i64, mode: SlabMode, trapezoid: bool) -> ScalarSlabRow {
        let weights = if mode == SlabMode::Average && samples > 1 {
            vec![0.0; width]
        } else {
            Vec::new()
        };
        ScalarSlabRow {
            weights,
            mode,
            trapezoid,
        }
    }
    fn bind(&mut self) {
        self.weights.fill(0.0);
    }
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        values: &mut [f64],
        valid: &mut [u8],
        x: usize,
        k: i64,
        samples: i64,
        value: Option<f64>,
    ) {
        let Some(value) = value else { return };
        match self.mode {
            SlabMode::MinIp => {
                if valid[x] == 0 || value < values[x] {
                    values[x] = value;
                }
            }
            SlabMode::Average | SlabMode::Sum => {
                let mut weight = 1.0;
                if self.trapezoid {
                    if samples > 1 && (k == 0 || k == samples - 1) {
                        weight = 0.5;
                    }
                    values[x] += weight * value;
                } else {
                    values[x] += value;
                }
                if !self.weights.is_empty() {
                    self.weights[x] += weight;
                }
            }
            _ => {
                if valid[x] == 0 || value > values[x] {
                    values[x] = value;
                }
            }
        }
        valid[x] = 1;
    }
    fn finish(&self, values: &mut [f64]) {
        if self.mode != SlabMode::Average || self.weights.is_empty() {
            return;
        }
        for (x, weight) in self.weights.iter().enumerate() {
            if *weight != 0.0 {
                values[x] /= weight;
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn render_cached_scalar_slab_rows(
    ctx: Option<&dyn Cancellation>,
    scalar: &mut ScalarPlane,
    sampler: &Sampler,
    plane: MprPlane,
    start: i64,
    end: i64,
    samples: i64,
    mode: SlabMode,
    interpolation: Interpolation,
    trapezoid: bool,
) -> Result<(), RenderError> {
    let (width, height) = (scalar.width, scalar.height);
    scalar_rows(ctx, scalar, || {
        let mut row = ScalarSlabRow::new(width as usize, samples, mode, trapezoid);
        move |y: i64, values: &mut [f64], valid: &mut [u8]| {
            row.bind();
            let z = orthogonal_display_slice_index(y, height, sampler.depth);
            for k in 0..samples {
                let coordinate = scalar_slab_coordinate(start, end, samples, k);
                let integer = coordinate == coordinate.trunc();
                for x in 0..width {
                    let mut p = Vec3::new(x as f64, y as f64, coordinate);
                    match plane {
                        MprPlane::Coronal => {
                            p.y = coordinate;
                            p.z = z as f64;
                        }
                        MprPlane::Sagittal => {
                            p = Vec3::new(coordinate, x as f64, z as f64);
                        }
                        MprPlane::Axial => {}
                    }
                    let value = if integer {
                        sampler.value_at(
                            fmath::f64_to_i64(p.x),
                            fmath::f64_to_i64(p.y),
                            fmath::f64_to_i64(p.z),
                        )
                    } else {
                        sample_at(sampler, p, interpolation)
                    };
                    row.add(values, valid, x as usize, k, samples, value);
                }
            }
            row.finish(values);
        }
    })
}
fn scalar_slab_rows_eligible(
    sampler: &Sampler,
    normal: Vec3,
    samples: i64,
    mode: SlabMode,
    interpolation: Interpolation,
) -> bool {
    let vol = &sampler.vol;
    if vol.gantry_tilt_correction().applied
        || (!vol.slice_positions().is_empty() && vol.uniform_slice_step_value() <= 0.0)
    {
        return false;
    }
    let mut scratch = 24;
    if mode == SlabMode::Average && samples > 1 {
        scratch += 8;
    }
    if samples <= 1 || samples - 1 < scratch / 8 {
        return false;
    }
    if interpolation == Interpolation::CUBIC
        && separable_axis(&vol.voxel_line(vol.origin, normal)).is_some()
    {
        return false;
    }
    true
}
#[allow(clippy::too_many_arguments)]
fn reslice_scalar_slab_rows(
    ctx: Option<&dyn Cancellation>,
    sampler: &Sampler,
    plane: &Plane,
    normal: Vec3,
    width: i64,
    height: i64,
    first_offset: f64,
    samples: i64,
    step: f64,
    mode: SlabMode,
    default_voi: &WindowLevel,
    limits: MprLimits,
    interpolation: Interpolation,
    trapezoid: bool,
) -> Result<ScalarPlane, RenderError> {
    let mut scalar = ScalarPlane::new(
        width,
        height,
        samples,
        &sampler.photometric,
        default_voi,
        limits,
    )?;
    let denom_w = denominator(width);
    let denom_h = denominator(height);
    let vol = &sampler.vol;
    let (_, _, _, ddx, ddy, ddpos) = vol
        .voxel_line(plane.origin, normal)
        .affine_parts()
        .unwrap_or_default();
    scalar_rows(ctx, &mut scalar, || {
        let mut origins = vec![Vec3::ZERO; width as usize];
        let mut row = ScalarSlabRow::new(width as usize, samples, mode, trapezoid);
        move |y: i64, values: &mut [f64], valid: &mut [u8]| {
            row.bind();
            let t = y as f64 / denom_h;
            for (x, origin) in origins.iter_mut().enumerate() {
                let ray = vol.voxel_line(plane.at(x as f64 / denom_w, t), normal);
                let (x0, y0, pos0, _, _, _) = ray.affine_parts().unwrap_or_default();
                *origin = Vec3::new(x0, y0, pos0);
            }
            for k in 0..samples {
                let distance = (first_offset + k as f64) * step;
                for (x, origin) in origins.iter().enumerate() {
                    let position = Vec3::new(
                        origin.x + distance * ddx,
                        origin.y + distance * ddy,
                        vol.slice_of_position(origin.z + distance * ddpos),
                    );
                    let value = sample_at(sampler, position, interpolation);
                    row.add(values, valid, x, k, samples, value);
                }
            }
            row.finish(values);
        }
    })?;
    Ok(scalar)
}
#[derive(Debug, Clone, Copy, Default)]
struct CubicAxisTaps {
    ok: bool,
    n: usize,
    index: [i64; 4],
    weight: [f64; 4],
}
impl CubicAxisTaps {
    fn new(v: f64, n: i64) -> CubicAxisTaps {
        let Some(v) = clamp_continuous_index(v, n, VOXEL_BORDER_TOLERANCE) else {
            return CubicAxisTaps::default();
        };
        let (index, weight, taps) = cubic_taps(v, n);
        CubicAxisTaps {
            ok: true,
            n: taps,
            index,
            weight,
        }
    }
}
fn cubic_from_taps(
    s: &Sampler,
    x: &CubicAxisTaps,
    y: &CubicAxisTaps,
    z: &CubicAxisTaps,
) -> Option<f64> {
    if !x.ok || !y.ok || !z.ok {
        return None;
    }
    let mut sum = 0.0;
    for k in 0..z.n {
        let mut plane = 0.0;
        for j in 0..y.n {
            let mut row = 0.0;
            for i in 0..x.n {
                row += x.weight[i] * s.value_or_zero(x.index[i], y.index[j], z.index[k]);
            }
            plane += y.weight[j] * row;
        }
        sum += z.weight[k] * plane;
    }
    Some(sum)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineAxis {
    Constant,
    X,
    Y,
    Z,
}
fn separable_axis(line: &VoxelLine<'_>) -> Option<LineAxis> {
    let (_, _, _, dx, dy, dpos) = line.affine_parts()?;
    let mut axis = LineAxis::Constant;
    let mut varying = 0;
    if dx != 0.0 {
        axis = LineAxis::X;
        varying += 1;
    }
    if dy != 0.0 {
        axis = LineAxis::Y;
        varying += 1;
    }
    if dpos != 0.0 {
        axis = LineAxis::Z;
        varying += 1;
    }
    (varying <= 1).then_some(axis)
}
struct CubicLinePlan<'a, P: Fn(i64) -> f64> {
    sampler: &'a Sampler,
    count: i64,
    param: P,
    key: Option<(LineAxis, u64, u64)>,
    table: Vec<CubicAxisTaps>,
    x: CubicAxisTaps,
    y: CubicAxisTaps,
    z: CubicAxisTaps,
    axis: LineAxis,
}
impl<'a, P: Fn(i64) -> f64> CubicLinePlan<'a, P> {
    fn new(sampler: &'a Sampler, count: i64, param: P) -> Self {
        CubicLinePlan {
            sampler,
            count,
            param,
            key: None,
            table: Vec::new(),
            x: CubicAxisTaps::default(),
            y: CubicAxisTaps::default(),
            z: CubicAxisTaps::default(),
            axis: LineAxis::Constant,
        }
    }
    fn prepare(&mut self, line: &VoxelLine<'_>) -> bool {
        let Some(axis) = separable_axis(line) else {
            return false;
        };
        let s = self.sampler;
        let first = line.at((self.param)(0));
        self.axis = axis;
        self.x = CubicAxisTaps::new(first.x, s.cols);
        self.y = CubicAxisTaps::new(first.y, s.rows);
        self.z = CubicAxisTaps::new(first.z, s.depth);
        if axis == LineAxis::Constant {
            return true;
        }
        let Some((x0, y0, pos0, dx, dy, dpos)) = line.affine_parts() else {
            return false;
        };
        let (start, step) = match axis {
            LineAxis::X => (x0, dx),
            LineAxis::Y => (y0, dy),
            _ => (pos0, dpos),
        };
        let key_eq = |a: f64, b: u64| a == f64::from_bits(b);
        if let Some((k_axis, k_start, k_step)) = self.key
            && k_axis == axis
            && key_eq(start, k_start)
            && key_eq(step, k_step)
        {
            return true;
        }
        self.table.clear();
        for i in 0..self.count.max(0) {
            let p = line.at((self.param)(i));
            self.table.push(match axis {
                LineAxis::X => CubicAxisTaps::new(p.x, s.cols),
                LineAxis::Y => CubicAxisTaps::new(p.y, s.rows),
                _ => CubicAxisTaps::new(p.z, s.depth),
            });
        }
        self.key = Some((axis, start.to_bits(), step.to_bits()));
        true
    }
    fn sample(&self, i: usize) -> Option<f64> {
        let s = self.sampler;
        match self.axis {
            LineAxis::X => cubic_from_taps(s, &self.table[i], &self.y, &self.z),
            LineAxis::Y => cubic_from_taps(s, &self.x, &self.table[i], &self.z),
            LineAxis::Z => cubic_from_taps(s, &self.x, &self.y, &self.table[i]),
            LineAxis::Constant => cubic_from_taps(s, &self.x, &self.y, &self.z),
        }
    }
}
pub const SLAB_OPACITY_LINEAR: i64 = 0;
pub const SLAB_OPACITY_LOGARITHMIC_INVERSE: i64 = 1;
pub const SLAB_OPACITY_LOGARITHMIC: i64 = 2;
pub const SLAB_OPACITY_SMOOTH: i64 = 3;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VolumeSlabOptions {
    pub opacity: i64,
    pub shading: bool,
    pub ambient: f64,
    pub diffuse: f64,
    pub specular: f64,
    pub specular_power: f64,
    pub gantry_tilt_mode: GantryTiltRenderMode,
}
pub fn default_volume_slab_options() -> VolumeSlabOptions {
    VolumeSlabOptions {
        opacity: SLAB_OPACITY_LINEAR,
        shading: false,
        ambient: 0.15,
        diffuse: 0.90,
        specular: 0.30,
        specular_power: 15.0,
        gantry_tilt_mode: GantryTiltRenderMode::Corrected,
    }
}
pub(crate) fn clamp01(value: f64) -> f64 {
    if value.is_nan() || value < 0.0 {
        return 0.0;
    }
    if value > 1.0 || value == f64::INFINITY {
        return 1.0;
    }
    value
}
fn normalize_volume_slab_options(mut o: VolumeSlabOptions) -> VolumeSlabOptions {
    if o.opacity < SLAB_OPACITY_LINEAR || o.opacity > SLAB_OPACITY_SMOOTH {
        o.opacity = SLAB_OPACITY_LINEAR;
    }
    o.ambient = clamp01(o.ambient);
    o.diffuse = clamp01(o.diffuse);
    o.specular = clamp01(o.specular);
    if o.specular_power.is_nan() || o.specular_power.is_infinite() || o.specular_power < 1.0 {
        o.specular_power = 1.0;
    }
    if o.specular_power > 128.0 {
        o.specular_power = 128.0;
    }
    o
}
fn volume_slab_opacity(mode: i64, value: f64) -> f64 {
    let value = clamp01(value);
    match mode {
        SLAB_OPACITY_LOGARITHMIC_INVERSE => {
            1.0 - fmath::log1p(9.0 * (1.0 - value)) / fmath::log(10.0)
        }
        SLAB_OPACITY_LOGARITHMIC => fmath::log1p(9.0 * value) / fmath::log(10.0),
        SLAB_OPACITY_SMOOTH => value * value * (3.0 - 2.0 * value),
        _ => value,
    }
}
fn volume_slab_shade(gradient: Vec3, view_dir: Vec3, o: &VolumeSlabOptions) -> (f64, f64) {
    let length = gradient.length();
    if length < 1e-6 {
        return (o.ambient, 0.0);
    }
    headlight_shading(
        gradient.scale(-1.0 / length),
        view_dir.scale(-1.0).normalize(),
        &VrLightingMaterial {
            ambient: o.ambient,
            diffuse: o.diffuse,
            specular: o.specular,
            specular_power: o.specular_power,
        },
    )
}
fn normalized_voxel_position(position: f64, count: i64) -> f64 {
    if count <= 1 {
        return 0.0;
    }
    clamp01(position / (count - 1) as f64)
}
fn composite_volume_slab_samples(
    sampler: &Sampler,
    mapper: &super::window::PreparedVoi,
    o: &VolumeSlabOptions,
    start: i64,
    end: i64,
    view_dir: Vec3,
    sample_at: impl Fn(i64, bool) -> Option<(f64, Vec3)>,
) -> u8 {
    let (mut acc_color, mut acc_alpha) = (0.0f64, 0.0f64);
    let mut stride = 1;
    let count = end - start + 1;
    if count > 64 {
        stride = (count + 63) / 64;
    }
    let mut sample = start;
    while sample <= end && acc_alpha <= VR_OPACITY_THRESHOLD {
        let current = sample;
        sample += stride;
        let Some((value, texture)) = sample_at(current, o.shading) else {
            continue;
        };
        let intensity = f64::from(sampler.display_gray(value, mapper)) / 255.0;
        let mut alpha = volume_slab_opacity(o.opacity, intensity);
        if alpha <= 0.0 {
            continue;
        }
        alpha = 1.0 - fmath::pow(1.0 - alpha, 0.25 * stride as f64);
        let (mut lit, mut specular) = (1.0, 0.0);
        if o.shading {
            (lit, specular) = volume_slab_shade(gradient_at(sampler, texture), view_dir, o);
        }
        let weight = (1.0 - acc_alpha) * alpha;
        acc_color += weight * (intensity * lit + specular);
        acc_alpha += weight;
    }
    fmath::f64_to_u8((255.0 * clamp01(acc_color)).round())
}
pub fn render_volume_slab(
    series: Option<&Stack>,
    plane: &str,
    center: i64,
    thickness: i64,
    window: &WindowLevel,
    options: VolumeSlabOptions,
) -> (RenderImage, Option<RenderError>) {
    let Some(series) = series.filter(|s| !s.frames.is_empty()) else {
        return (
            blank_image(512, 512),
            Some(RenderError::other("render: no series selected")),
        );
    };
    let correct = options.gantry_tilt_mode != GantryTiltRenderMode::SourceGeometry;
    let (vol, sampler) = match stack_volume_sampler(series, correct) {
        Ok(v) => v,
        Err(e) => {
            return (
                blank_image(512, 512),
                Some(RenderError::new(
                    e.kind(),
                    format!("render: series has no displayable volume: {e}"),
                )),
            );
        }
    };
    let window = normalize_window(window, &series.default_window);
    let mapper = prepare_window(&window);
    let o = normalize_volume_slab_options(options);
    let (rows, cols, depth) = (sampler.rows, sampler.cols, sampler.depth);
    let parsed = MprPlane::parse(plane);
    if correct
        && let Some(p) = parsed
        && let Some((corrected, width, height)) = vol.corrected_orthogonal_plane(p, center)
    {
        let (slab_axis, step, sample_count) = corrected_slab_axis(&vol, p);
        let view_dir = if p == MprPlane::Sagittal {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let clamped = clamp_index(center, sample_count);
        let (start, end) = mip_projection_range(clamped, thickness, sample_count);
        let slab_axis = slab_axis.normalize();
        let denom_w = if width - 1 <= 0 {
            1.0
        } else {
            (width - 1) as f64
        };
        let denom_h = if height - 1 <= 0 {
            1.0
        } else {
            (height - 1) as f64
        };
        let pixels = gray_rows(width, height, |j, out| {
            let t = j as f64 / denom_h;
            let row_base = corrected.origin.add(corrected.v.scale(t));
            for (i, px) in out.iter_mut().enumerate() {
                let base = row_base.add(corrected.u.scale(i as f64 / denom_w));
                let ray = sampler.vol.voxel_line(base, slab_axis);
                *px = composite_volume_slab_samples(
                    &sampler,
                    &mapper,
                    &o,
                    start,
                    end,
                    view_dir,
                    |sample, need| {
                        let voxel = ray.at((sample - clamped) as f64 * step);
                        let value = sampler.trilinear_at(voxel)?;
                        let texture = if need {
                            Vec3::new(
                                normalized_voxel_position(voxel.x, sampler.cols),
                                normalized_voxel_position(voxel.y, sampler.rows),
                                normalized_voxel_position(voxel.z, sampler.depth),
                            )
                        } else {
                            Vec3::ZERO
                        };
                        Some((value, texture))
                    },
                );
            }
        });
        return (
            RenderImage::gray(width as usize, height as usize, pixels),
            None,
        );
    }
    let texture_of = |x: i64, y: i64, z: i64| {
        Vec3::new(
            normalized_voxel_position(x as f64, sampler.cols),
            normalized_voxel_position(y as f64, sampler.rows),
            normalized_voxel_position(z as f64, sampler.depth),
        )
    };
    let ray = |start: i64, end: i64, view: Vec3, coordinate: &dyn Fn(i64) -> (i64, i64, i64)| {
        composite_volume_slab_samples(&sampler, &mapper, &o, start, end, view, |sample, need| {
            let (x, y, z) = coordinate(sample);
            let value = sampler.value_at(x, y, z)?;
            Some((
                value,
                if need {
                    texture_of(x, y, z)
                } else {
                    Vec3::ZERO
                },
            ))
        })
    };
    match parsed {
        Some(MprPlane::Axial) => {
            let (start, end) = mip_projection_range(center, thickness, depth);
            let pixels = gray_rows(cols, rows, |y, out| {
                for (x, px) in out.iter_mut().enumerate() {
                    *px = ray(start, end, Vec3::new(0.0, 0.0, 1.0), &|s| (x as i64, y, s));
                }
            });
            (
                RenderImage::gray(cols as usize, rows as usize, pixels),
                None,
            )
        }
        Some(MprPlane::Coronal) => {
            let (start, end) = mip_projection_range(center, thickness, rows);
            let height = orthogonal_volume_height(depth, vol.slice_spacing, vol.col_spacing);
            let pixels = gray_rows(cols, height, |out_z, out| {
                let z = orthogonal_display_slice_index(out_z, height, depth);
                for (x, px) in out.iter_mut().enumerate() {
                    *px = ray(start, end, Vec3::new(0.0, 1.0, 0.0), &|s| (x as i64, s, z));
                }
            });
            (
                RenderImage::gray(cols as usize, height as usize, pixels),
                None,
            )
        }
        Some(MprPlane::Sagittal) => {
            let (start, end) = mip_projection_range(center, thickness, cols);
            let height = orthogonal_volume_height(depth, vol.slice_spacing, vol.row_spacing);
            let pixels = gray_rows(rows, height, |out_z, out| {
                let z = orthogonal_display_slice_index(out_z, height, depth);
                for (y, px) in out.iter_mut().enumerate() {
                    *px = ray(start, end, Vec3::new(1.0, 0.0, 0.0), &|s| (s, y as i64, z));
                }
            });
            (
                RenderImage::gray(rows as usize, height as usize, pixels),
                None,
            )
        }
        None => (
            blank_image(512, 512),
            Some(RenderError::other(format!(
                "render: unsupported slab plane {}",
                crate::textfmt::quote(plane)
            ))),
        ),
    }
}
fn gray_rows(width: i64, height: i64, row: impl Fn(i64, &mut [u8]) + Sync) -> Vec<u8> {
    let w = width.max(0) as usize;
    let slots: Vec<OnceLock<Vec<u8>>> = (0..height.max(0)).map(|_| OnceLock::new()).collect();
    parallel_rows(height, |y| {
        let mut out = vec![0u8; w];
        row(y, &mut out);
        let _ = slots[y as usize].set(out);
    });
    slots
        .into_iter()
        .flat_map(|s| s.into_inner().unwrap_or_else(|| vec![0u8; w]))
        .collect()
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObliqueResolutionRequest {
    pub target_long_edge: i64,
    pub preview: bool,
    pub slab_samples: i64,
    pub max_dimension: i64,
    pub max_working_bytes: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObliqueResolution {
    pub width: i64,
    pub height: i64,
    pub pixel_spacing: MeasureSpacing,
    pub estimated_working_bytes: i64,
    pub bounded: bool,
}
fn finite_positive_or(value: f64, fallback: f64) -> f64 {
    if value <= 0.0 || value.is_nan() || value.is_infinite() {
        fallback
    } else {
        value
    }
}
fn finest_volume_spacing(v: Option<&Volume>) -> f64 {
    let Some(v) = v else { return 1.0 };
    let mut spacing = f64::INFINITY;
    for candidate in [v.col_spacing, v.row_spacing, v.slice_spacing] {
        if candidate > 0.0 && candidate.is_finite() && candidate < spacing {
            spacing = candidate;
        }
    }
    if spacing == f64::INFINITY {
        1.0
    } else {
        spacing
    }
}
fn physical_aspect_dimensions(u_length: f64, v_length: f64, long_edge: i64) -> (i64, i64) {
    let short =
        |a: f64, b: f64| (fmath::f64_to_i64(((long_edge - 1) as f64 * a / b).round()) + 1).max(2);
    if u_length >= v_length {
        (long_edge, short(v_length, u_length))
    } else {
        (short(u_length, v_length), long_edge)
    }
}
pub fn plan_oblique_resolution(
    vol: Option<&Volume>,
    plane: &Plane,
    request: ObliqueResolutionRequest,
) -> Result<ObliqueResolution, RenderError> {
    if !valid_mpr_plane(plane) {
        return Err(invalid_field("Plane"));
    }
    if request.slab_samples < 0 {
        return Err(invalid_field("SlabSamples"));
    }
    let mut max_dimension = request.max_dimension;
    let mut bounded = false;
    if max_dimension == 0 {
        max_dimension = MAX_OBLIQUE_OUTPUT_DIMENSION;
    } else if max_dimension < 2 {
        return Err(invalid_field("MaxDimension"));
    }
    if max_dimension > MAX_OBLIQUE_OUTPUT_DIMENSION {
        max_dimension = MAX_OBLIQUE_OUTPUT_DIMENSION;
        bounded = true;
    }
    let mut max_working_bytes = request.max_working_bytes;
    if max_working_bytes == 0 || max_working_bytes > DEFAULT_OBLIQUE_WORKING_SET_BYTES {
        if max_working_bytes > DEFAULT_OBLIQUE_WORKING_SET_BYTES {
            bounded = true;
        }
        max_working_bytes = DEFAULT_OBLIQUE_WORKING_SET_BYTES;
    } else if max_working_bytes < 0 {
        return Err(invalid_field("MaxWorkingBytes"));
    }
    let u_length = finite_positive_or(plane.u.length(), 1.0);
    let v_length = finite_positive_or(plane.v.length(), 1.0);
    let mut long_edge = request.target_long_edge;
    if long_edge <= 0 {
        let spacing = finest_volume_spacing(vol);
        let native = fmath::max(u_length, v_length) / spacing;
        if native.is_nan() || native.is_infinite() || native >= (max_dimension - 1) as f64 {
            long_edge = max_dimension;
            bounded = true;
        } else {
            long_edge = fmath::f64_to_i64(native.round()) + 1;
        }
    }
    if long_edge < 2 {
        long_edge = 2;
        bounded = true;
    }
    if long_edge > max_dimension {
        long_edge = max_dimension;
        bounded = true;
    }
    if request.preview && long_edge > 2 {
        long_edge = 2.max((long_edge + 1) / 2);
    }
    let (mut width, mut height) = physical_aspect_dimensions(u_length, v_length, long_edge);
    let mut slab_samples = request.slab_samples.max(1);
    if slab_samples > MAX_OBLIQUE_SLAB_SAMPLES {
        slab_samples = MAX_OBLIQUE_SLAB_SAMPLES;
        bounded = true;
    }
    let bytes_per_pixel = 1 + 8 * slab_samples;
    if max_working_bytes < 4 * bytes_per_pixel {
        return Err(limited_field("MaxWorkingBytes"));
    }
    let max_pixels = max_working_bytes / bytes_per_pixel;
    let pixels = width * height;
    if pixels > max_pixels {
        let scale = (max_pixels as f64 / pixels as f64).sqrt();
        width = fmath::f64_to_i64((width as f64 * scale).floor()).max(2);
        height = fmath::f64_to_i64((height as f64 * scale).floor()).max(2);
        if width * height > max_pixels {
            if width >= height {
                width = width.min(max_pixels / height).max(2);
            } else {
                height = height.min(max_pixels / width).max(2);
            }
        }
        bounded = true;
    }
    Ok(ObliqueResolution {
        width,
        height,
        pixel_spacing: plane.pixel_spacing_mm(width, height),
        estimated_working_bytes: width * height * bytes_per_pixel,
        bounded,
    })
}
const INTERACTIVE_SHRINK_FACTOR: i64 = 2;
const MIN_INTERACTIVE_SHRINK_AXIS: i64 = 2 * INTERACTIVE_SHRINK_FACTOR;
fn mpr_antialias_volume(
    ctx: Option<&dyn Cancellation>,
    source: &Arc<Volume>,
    plane: &Plane,
    width: i64,
    height: i64,
    limits: MprLimits,
) -> Result<(Arc<Volume>, bool), RenderError> {
    if !source.is_regular() || source.geometry().requires_resampling || width < 2 || height < 2 {
        return Ok((source.clone(), false));
    }
    let coarsest = fmath::max(
        source.col_spacing,
        fmath::max(source.row_spacing, source.slice_spacing),
    );
    let spacing = plane.pixel_spacing_mm(width, height);
    if coarsest <= 0.0
        || coarsest.is_nan()
        || coarsest.is_infinite()
        || spacing.x < 2.0 * coarsest
        || spacing.y < 2.0 * coarsest
    {
        return Ok((source.clone(), false));
    }
    let mut sizes = [source.cols, source.rows, source.depth];
    let mut shrinks = false;
    for size in sizes.iter_mut() {
        if *size >= MIN_INTERACTIVE_SHRINK_AXIS {
            if *size % 2 != 0 {
                return Ok((source.clone(), false));
            }
            *size /= 2;
            shrinks = true;
        }
    }
    if !shrinks {
        return Ok((source.clone(), false));
    }
    let count = checked_voxel_count(sizes[0], sizes[1], sizes[2]);
    let output_bytes = width * height * 9;
    match count {
        Some(c) if (c as i64) <= (limits.max_working_bytes - output_bytes) / 4 => {}
        _ => return Err(limited_field("Antialias2x")),
    }
    let level = interactive_volume(ctx, source)?;
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let differs = !Arc::ptr_eq(&level, source);
    Ok((level, differs))
}
#[doc(hidden)]
pub fn bin_shrink_volume(
    ctx: Option<&dyn Cancellation>,
    v: Option<&Arc<Volume>>,
    mut factors: [i64; 3],
) -> Result<Arc<Volume>, RenderError> {
    let Some(v) = v.filter(|v| v.cols > 0 && v.rows > 0 && v.depth > 0) else {
        return Err(RenderError::other("render: cannot shrink an empty volume"));
    };
    let source = if v.geometry().requires_resampling {
        v.regular_grid()?
    } else {
        v.clone()
    };
    let sampler = Sampler::new(&source)?;
    let sizes = [source.cols, source.rows, source.depth];
    let mut out = [0i64; 3];
    for axis in 0..3 {
        factors[axis] = factors[axis].min(sizes[axis]).max(1);
        out[axis] = (sizes[axis] / factors[axis]).max(1);
    }
    let [cols, rows, depth] = out;
    if checked_voxel_count(cols, rows, depth).is_none() {
        return Err(RenderError::other(
            "render: shrunk volume dimensions overflow",
        ));
    }
    let [fx, fy, fz] = factors;
    let block = (fx * fy * fz) as f64;
    let plane = (cols * rows) as usize;
    let slabs = gather_rows::<f32, _, _>(ctx, depth, plane, || {
        |z: i64, out: &mut [f32]| {
            for y in 0..rows {
                for x in 0..cols {
                    let mut sum = 0.0;
                    for dz in 0..fz {
                        for dy in 0..fy {
                            for dx in 0..fx {
                                sum += sampler.value_or_zero(x * fx + dx, y * fy + dy, z * fz + dz);
                            }
                        }
                    }
                    out[(y * cols + x) as usize] = (sum / block) as f32;
                }
            }
        }
    })?;
    let values = slabs.concat();
    let origin = source
        .origin
        .add(
            source
                .axis_x
                .scale((fx - 1) as f64 / 2.0 * source.col_spacing),
        )
        .add(
            source
                .axis_y
                .scale((fy - 1) as f64 / 2.0 * source.row_spacing),
        )
        .add(
            source
                .normal
                .scale((fz - 1) as f64 / 2.0 * source.slice_spacing),
        );
    Ok(Arc::new(Volume::derived_f32(
        &source,
        cols,
        rows,
        depth,
        origin,
        source.col_spacing * fx as f64,
        source.row_spacing * fy as f64,
        source.slice_spacing * fz as f64,
        values,
    )))
}
pub fn interactive_volume(
    ctx: Option<&dyn Cancellation>,
    v: &Arc<Volume>,
) -> Result<Arc<Volume>, RenderError> {
    let mut factors = [1i64; 3];
    let mut shrinks = false;
    for (axis, n) in [v.cols, v.rows, v.depth].into_iter().enumerate() {
        if n >= MIN_INTERACTIVE_SHRINK_AXIS {
            factors[axis] = INTERACTIVE_SHRINK_FACTOR;
            shrinks = true;
        }
    }
    if !shrinks {
        return Ok(v.clone());
    }
    bin_shrink_volume(ctx, Some(v), factors)
}
pub fn mpr_antialias_level(
    source: &Arc<Volume>,
    plane: &Plane,
    width: i64,
    height: i64,
    limits: MprLimits,
) -> Result<(Arc<Volume>, bool), RenderError> {
    mpr_antialias_volume(None, source, plane, width, height, limits)
}
pub fn mpr_antialias_refusal() -> RenderError {
    invalid_field("Antialias2x")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_count_refines_and_bounds() {
        let s = SlabSampling {
            trapezoid: false,
            spacing_fraction: 0.5,
        };
        assert_eq!(s.sample_count(5, MprLimits::default()).unwrap(), 9);
        assert!(s.sample_count(0, MprLimits::default()).is_err());
        let bad = SlabSampling {
            trapezoid: false,
            spacing_fraction: 2.0,
        };
        assert!(bad.sample_count(3, MprLimits::default()).is_err());
    }
    #[test]
    fn trapezoid_weights_endpoints() {
        let values = [2.0, 4.0, 6.0];
        let sum = reduce_trapezoid(SlabMode::Sum, 3, |k| Some(values[k as usize]));
        assert_eq!(sum, Some(1.0 + 4.0 + 3.0));
        let avg = reduce_trapezoid(SlabMode::Average, 3, |k| Some(values[k as usize]));
        assert_eq!(avg, Some(8.0 / 2.0));
    }
}
