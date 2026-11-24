#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::frame::{RenderImage, blank_image, display_gray_mapped, pixel_value_at, render_frame};
use super::mpr::{MprPlane, Plane};
use super::parallel::parallel_rows;
use super::vec3::Vec3;
use super::volume::{
    Sampler, Stack, Volume, first_renderable_slice, series_pixel_spacing, series_slice_spacing,
};
use super::window::{
    DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH, PreparedVoi, WindowLevel, normalize_window,
    prepare_window,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
pub const MAX_OBLIQUE_OUTPUT_DIMENSION: i64 = 2048;
pub const DEFAULT_OBLIQUE_WORKING_SET_BYTES: i64 = 64 << 20;
pub const MAX_OBLIQUE_SLAB_SAMPLES: i64 = 4096;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MprLimits {
    pub max_output_dimension: i64,
    pub max_output_pixels: i64,
    pub max_working_bytes: i64,
    pub max_slab_samples: i64,
}
impl MprLimits {
    pub fn defaults() -> MprLimits {
        MprLimits {
            max_output_dimension: MAX_OBLIQUE_OUTPUT_DIMENSION,
            max_output_pixels: MAX_OBLIQUE_OUTPUT_DIMENSION * MAX_OBLIQUE_OUTPUT_DIMENSION,
            max_working_bytes: DEFAULT_OBLIQUE_WORKING_SET_BYTES,
            max_slab_samples: MAX_OBLIQUE_SLAB_SAMPLES,
        }
    }
}
pub(crate) fn invalid_field(field: &str) -> RenderError {
    RenderError::new(
        ErrorKind::MprInvalidInput,
        format!("dicom/render: invalid MPR input: {field}"),
    )
}
pub(crate) fn limited_field(field: &str) -> RenderError {
    RenderError::new(
        ErrorKind::MprResourceLimit,
        format!("dicom/render: MPR resource limit exceeded: {field}"),
    )
}
pub(crate) fn normalize_mpr_limits(mut limits: MprLimits) -> Result<MprLimits, RenderError> {
    if limits.max_output_dimension < 0 {
        return Err(invalid_field("MaxOutputDimension"));
    }
    if limits.max_output_pixels < 0 {
        return Err(invalid_field("MaxOutputPixels"));
    }
    if limits.max_working_bytes < 0 {
        return Err(invalid_field("MaxWorkingBytes"));
    }
    if limits.max_slab_samples < 0 {
        return Err(invalid_field("MaxSlabSamples"));
    }
    let d = MprLimits::defaults();
    if limits.max_output_dimension == 0 || limits.max_output_dimension > d.max_output_dimension {
        limits.max_output_dimension = d.max_output_dimension;
    }
    if limits.max_output_pixels == 0 || limits.max_output_pixels > d.max_output_pixels {
        limits.max_output_pixels = d.max_output_pixels;
    }
    if limits.max_working_bytes == 0 || limits.max_working_bytes > d.max_working_bytes {
        limits.max_working_bytes = d.max_working_bytes;
    }
    if limits.max_slab_samples == 0 || limits.max_slab_samples > d.max_slab_samples {
        limits.max_slab_samples = d.max_slab_samples;
    }
    Ok(limits)
}
pub fn validate_mpr_output(
    width: i64,
    height: i64,
    slab_samples: i64,
    retained_bytes_per_pixel: i64,
    limits: MprLimits,
) -> Result<MprLimits, RenderError> {
    let limits = normalize_mpr_limits(limits)?;
    if width <= 0 {
        return Err(invalid_field("Width"));
    }
    if height <= 0 {
        return Err(invalid_field("Height"));
    }
    if slab_samples <= 0 {
        return Err(invalid_field("SlabSamples"));
    }
    if width > limits.max_output_dimension {
        return Err(limited_field("Width"));
    }
    if height > limits.max_output_dimension {
        return Err(limited_field("Height"));
    }
    if slab_samples > limits.max_slab_samples {
        return Err(limited_field("SlabSamples"));
    }
    if retained_bytes_per_pixel <= 0 {
        return Err(invalid_field("BytesPerPixel"));
    }
    let pixels = width.wrapping_mul(height);
    if pixels <= 0 || pixels > limits.max_output_pixels {
        return Err(limited_field("OutputPixels"));
    }
    let extra = slab_samples - 1;
    if extra > (i64::MAX - retained_bytes_per_pixel) / 8 {
        return Err(limited_field("WorkingBytes"));
    }
    let per_pixel = retained_bytes_per_pixel + extra * 8;
    if pixels > i64::MAX / per_pixel || pixels * per_pixel > limits.max_working_bytes {
        return Err(limited_field("WorkingBytes"));
    }
    Ok(limits)
}
pub(crate) fn valid_mpr_plane(plane: &Plane) -> bool {
    let f = |v: Vec3| v.finite();
    if !f(plane.origin) || !f(plane.u) || !f(plane.v) {
        return false;
    }
    if !f(plane.origin.add(plane.u))
        || !f(plane.origin.add(plane.v))
        || !f(plane.origin.add(plane.u).add(plane.v))
    {
        return false;
    }
    let u = plane.u.length();
    let v = plane.v.length();
    let n = plane.u.cross(plane.v).length();
    u > 0.0 && v > 0.0 && n > 0.0 && u.is_finite() && v.is_finite() && n.is_finite()
}
pub fn validate_mpr_reslice(
    plane: &Plane,
    width: i64,
    height: i64,
    slab_samples: i64,
    retained_bytes_per_pixel: i64,
    limits: MprLimits,
) -> Result<MprLimits, RenderError> {
    if !valid_mpr_plane(plane) {
        return Err(invalid_field("Plane"));
    }
    validate_mpr_output(
        width,
        height,
        slab_samples,
        retained_bytes_per_pixel,
        limits,
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GantryTiltRenderMode {
    #[default]
    Corrected,
    SourceGeometry,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MprRenderOptions {
    pub gantry_tilt_mode: GantryTiltRenderMode,
    pub limits: MprLimits,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SlabMode {
    #[default]
    None,
    Mip,
    MinIp,
    Average,
    Sum,
}
impl SlabMode {
    pub fn from_code(code: i64) -> Option<SlabMode> {
        Some(match code {
            0 => SlabMode::None,
            1 => SlabMode::Mip,
            2 => SlabMode::MinIp,
            3 => SlabMode::Average,
            4 => SlabMode::Sum,
            _ => return None,
        })
    }
}
fn par_gray(width: usize, height: usize, row: impl Fn(usize, &mut [u8]) + Sync) -> Vec<u8> {
    let cells: Vec<AtomicU8> = (0..width * height).map(|_| AtomicU8::new(0)).collect();
    parallel_rows(height as i64, |y| {
        let y = y as usize;
        let mut buffer = vec![0u8; width];
        row(y, &mut buffer);
        for (x, v) in buffer.into_iter().enumerate() {
            cells[y * width + x].store(v, Ordering::Relaxed);
        }
    });
    cells.into_iter().map(AtomicU8::into_inner).collect()
}
fn resampled_slice_index(out_z: i64, output_height: i64, slice_count: i64) -> i64 {
    if slice_count <= 1 || output_height <= 1 {
        return 0;
    }
    let index = crate::fmath::f64_to_i64(
        ((out_z as f64 + 0.5) * slice_count as f64 / output_height as f64).floor(),
    );
    index.clamp(0, slice_count - 1)
}
pub(crate) fn orthogonal_display_slice_index(
    out_z: i64,
    output_height: i64,
    slice_count: i64,
) -> i64 {
    resampled_slice_index(output_height - 1 - out_z, output_height, slice_count)
}
fn bad_spacing(v: f64) -> bool {
    v <= 0.0 || !v.is_finite()
}
pub(crate) fn orthogonal_mpr_height(series: &Stack, in_plane: f64) -> i64 {
    if series.frames.is_empty() {
        return 0;
    }
    let slice_spacing = series_slice_spacing(series);
    let n = series.frames.len() as i64;
    if bad_spacing(slice_spacing) || bad_spacing(in_plane) {
        return n;
    }
    crate::fmath::f64_to_i64((n as f64 * slice_spacing / in_plane).round()).max(1)
}
pub(crate) fn orthogonal_volume_height(depth: i64, slice_spacing: f64, in_plane: f64) -> i64 {
    if depth <= 0 {
        return 0;
    }
    if bad_spacing(slice_spacing) || bad_spacing(in_plane) {
        return depth;
    }
    crate::fmath::f64_to_i64((depth as f64 * slice_spacing / in_plane).round()).max(1)
}
pub(crate) fn clamp_index(index: i64, count: i64) -> i64 {
    if count <= 0 || index < 0 {
        0
    } else if index >= count {
        count - 1
    } else {
        index
    }
}
fn corrected_orthogonal_height(normal_span: f64, in_plane: f64) -> i64 {
    if bad_spacing(normal_span) || bad_spacing(in_plane) {
        return 1;
    }
    crate::fmath::f64_to_i64((normal_span / in_plane).round())
        .wrapping_add(1)
        .max(1)
}
impl Volume {
    pub(crate) fn corrected_orthogonal_plane(
        &self,
        plane: MprPlane,
        index: i64,
    ) -> Option<(Plane, i64, i64)> {
        if !self.gantry_tilt_correction().applied || self.depth < 2 {
            return None;
        }
        let normal_span = self.index_to_position((self.depth - 1) as f64);
        if bad_spacing(normal_span) {
            return None;
        }
        let top = self.origin.add(self.normal.scale(normal_span));
        match plane {
            MprPlane::Coronal => {
                let index = clamp_index(index, self.rows);
                Some((
                    Plane {
                        origin: top.add(self.axis_y.scale(index as f64 * self.row_spacing)),
                        u: self.axis_x.scale((self.cols - 1) as f64 * self.col_spacing),
                        v: self.normal.scale(-normal_span),
                    },
                    self.cols,
                    corrected_orthogonal_height(normal_span, self.col_spacing),
                ))
            }
            MprPlane::Sagittal => {
                let index = clamp_index(index, self.cols);
                Some((
                    Plane {
                        origin: top.add(self.axis_x.scale(index as f64 * self.col_spacing)),
                        u: self.axis_y.scale((self.rows - 1) as f64 * self.row_spacing),
                        v: self.normal.scale(-normal_span),
                    },
                    self.rows,
                    corrected_orthogonal_height(normal_span, self.row_spacing),
                ))
            }
            MprPlane::Axial => None,
        }
    }
    fn corrected_slab_axis(&self, plane: MprPlane) -> (Vec3, f64, i64) {
        match plane {
            MprPlane::Coronal => (self.axis_y, self.row_spacing, self.rows),
            MprPlane::Sagittal => (self.axis_x, self.col_spacing, self.cols),
            MprPlane::Axial => (Vec3::ZERO, 0.0, 0),
        }
    }
    pub fn orthogonal_plane(&self, plane: MprPlane, center: Vec3) -> Plane {
        let cv = self.patient_to_voxel(center);
        let last_slice = (self.depth - 1).max(0) as f64;
        match plane {
            MprPlane::Sagittal => {
                let origin = self.voxel_to_patient(Vec3::new(cv.x, 0.0, last_slice));
                Plane {
                    origin,
                    u: self.axis_y.scale((self.rows - 1) as f64 * self.row_spacing),
                    v: self.voxel_to_patient(Vec3::new(cv.x, 0.0, 0.0)).sub(origin),
                }
            }
            MprPlane::Coronal => {
                let origin = self.voxel_to_patient(Vec3::new(0.0, cv.y, last_slice));
                Plane {
                    origin,
                    u: self.axis_x.scale((self.cols - 1) as f64 * self.col_spacing),
                    v: self.voxel_to_patient(Vec3::new(0.0, cv.y, 0.0)).sub(origin),
                }
            }
            MprPlane::Axial => Plane {
                origin: self.voxel_to_patient(Vec3::new(0.0, 0.0, cv.z)),
                u: self.axis_x.scale((self.cols - 1) as f64 * self.col_spacing),
                v: self.axis_y.scale((self.rows - 1) as f64 * self.row_spacing),
            },
        }
    }
    pub fn trilinear_at(self: &Arc<Self>, p: Vec3) -> Option<f64> {
        let sampler = Sampler::new(self).ok()?;
        let mut p = p;
        if !Arc::ptr_eq(&sampler.vol, self) {
            p = sampler.vol.patient_to_voxel(self.voxel_to_patient(p));
        }
        sampler.trilinear_at(p)
    }
}
pub(crate) fn stack_volume_sampler(
    series: &Stack,
    regularize: bool,
) -> Result<(Arc<Volume>, Sampler), RenderError> {
    let vol = series.volume()?;
    let sampler = if regularize {
        Sampler::new(&vol).ok()
    } else {
        Sampler::decoded(&vol)
    };
    match sampler {
        Some(s) => Ok((vol, s)),
        None => Err(RenderError::other(
            "render: volume contains no decodable voxels",
        )),
    }
}
fn reslice_with_sampler(
    sampler: &Sampler,
    plane: &Plane,
    out_w: i64,
    out_h: i64,
    mapper: &PreparedVoi,
    limits: MprLimits,
) -> Result<RenderImage, RenderError> {
    validate_mpr_reslice(plane, out_w, out_h, 1, 1, limits)?;
    let denom_w = if out_w - 1 <= 0 {
        1.0
    } else {
        (out_w - 1) as f64
    };
    let denom_h = if out_h - 1 <= 0 {
        1.0
    } else {
        (out_h - 1) as f64
    };
    let pixels = par_gray(out_w as usize, out_h as usize, |j, row| {
        let t = j as f64 / denom_h;
        let line = sampler
            .vol
            .voxel_line(plane.origin.add(plane.v.scale(t)), plane.u);
        for (i, out) in row.iter_mut().enumerate() {
            let voxel = line.at(i as f64 / denom_w);
            if let Some(val) = sampler.trilinear_at(voxel) {
                *out = sampler.display_gray(val, mapper);
            }
        }
    });
    Ok(RenderImage::gray(out_w as usize, out_h as usize, pixels))
}
pub fn reslice_oblique(
    vol: Option<&Arc<Volume>>,
    plane: &Plane,
    out_w: i64,
    out_h: i64,
    window: &WindowLevel,
    limits: MprLimits,
) -> (RenderImage, Option<RenderError>) {
    if let Err(e) = validate_mpr_reslice(plane, out_w, out_h, 1, 1, limits) {
        return (blank_image(1, 1), Some(e));
    }
    let Some(vol) = vol else {
        return (blank_image(out_w, out_h), None);
    };
    let window = normalize_window(
        window,
        &WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH),
    );
    let mapper = prepare_window(&window);
    let Ok(sampler) = Sampler::new(vol) else {
        return (blank_image(out_w, out_h), None);
    };
    match reslice_with_sampler(&sampler, plane, out_w, out_h, &mapper, limits) {
        Ok(img) => (img, None),
        Err(e) => (blank_image(1, 1), Some(e)),
    }
}
fn mpr_voxel_gray(slice: Option<&super::frame::Frame>, x: i64, y: i64, mapper: &PreparedVoi) -> u8 {
    match (slice, pixel_value_at(slice, x, y)) {
        (Some(s), Some(v)) => {
            display_gray_mapped(v.rescaled, mapper, &s.metadata.photometric_interpretation)
        }
        _ => 0,
    }
}
pub fn render_mpr_plane(
    series: &Stack,
    plane: MprPlane,
    index: i64,
    window: &WindowLevel,
    options: MprRenderOptions,
) -> (RenderImage, Option<RenderError>) {
    if series.frames.is_empty() {
        return (
            blank_image(512, 512),
            Some(RenderError::other("render: no series selected")),
        );
    }
    let Some(first) = first_renderable_slice(series) else {
        return (
            blank_image(512, 512),
            Some(RenderError::other(
                "render: series has no displayable images",
            )),
        );
    };
    let rows = i64::from(first.metadata.rows);
    let cols = i64::from(first.metadata.columns);
    let limits = match normalize_mpr_limits(options.limits) {
        Ok(l) => l,
        Err(e) => return (blank_image(1, 1), Some(e)),
    };
    let window = normalize_window(window, &series.default_window);
    let mapper = prepare_window(&window);
    if plane != MprPlane::Axial {
        let correct = options.gantry_tilt_mode != GantryTiltRenderMode::SourceGeometry;
        match stack_volume_sampler(series, correct) {
            Ok((vol, sampler)) => match render_cached_mpr_plane(
                &vol, &sampler, plane, index, &mapper, correct, limits,
            ) {
                Err(e) => return (blank_image(1, 1), Some(e)),
                Ok(Some(img)) => return (img, None),
                Ok(None) => {}
            },
            Err(e) if e.is(ErrorKind::NonCorrectableGeometry) => {
                return (blank_image(512, 512), Some(e));
            }
            Err(_) => {}
        }
    }
    let n = series.frames.len() as i64;
    match plane {
        MprPlane::Axial => {
            if let Err(e) = validate_mpr_output(cols, rows, 1, 1, limits) {
                return (blank_image(1, 1), Some(e));
            }
            let index = index.clamp(0, n - 1);
            render_frame(series.frame(index as usize), &window)
        }
        MprPlane::Coronal | MprPlane::Sagittal => {
            let coronal = plane == MprPlane::Coronal;
            let (rs, cs) = series_pixel_spacing(series);
            let (width, limit, spacing) = if coronal {
                (cols, rows, cs)
            } else {
                (rows, cols, rs)
            };
            let index = if index < 0 { 0 } else { index.min(limit - 1) };
            let height = orthogonal_mpr_height(series, spacing);
            if let Err(e) = validate_mpr_output(width, height, 1, 1, limits) {
                return (blank_image(1, 1), Some(e));
            }
            let mut pixels = vec![0u8; (width * height) as usize];
            for out_z in 0..height {
                let slice = series.frame(orthogonal_display_slice_index(out_z, height, n) as usize);
                for x in 0..width {
                    let (px, py) = if coronal { (x, index) } else { (index, x) };
                    pixels[(out_z * width + x) as usize] = mpr_voxel_gray(slice, px, py, &mapper);
                }
            }
            (
                RenderImage::gray(width as usize, height as usize, pixels),
                None,
            )
        }
    }
}
fn render_cached_mpr_plane(
    vol: &Volume,
    sampler: &Sampler,
    plane: MprPlane,
    index: i64,
    mapper: &PreparedVoi,
    correct_tilt: bool,
    limits: MprLimits,
) -> Result<Option<RenderImage>, RenderError> {
    let (rows, cols, depth) = (sampler.rows, sampler.cols, sampler.depth);
    if rows <= 0 || cols <= 0 || depth <= 0 {
        return Ok(None);
    }
    if correct_tilt
        && let Some((corrected, width, height)) = vol.corrected_orthogonal_plane(plane, index)
    {
        return reslice_with_sampler(sampler, &corrected, width, height, mapper, limits).map(Some);
    }
    let coronal = match plane {
        MprPlane::Coronal => true,
        MprPlane::Sagittal => false,
        MprPlane::Axial => return Ok(None),
    };
    let (width, limit, spacing) = if coronal {
        (cols, rows, vol.col_spacing)
    } else {
        (rows, cols, vol.row_spacing)
    };
    let index = if index < 0 { 0 } else { index.min(limit - 1) };
    let height = orthogonal_volume_height(depth, vol.slice_spacing, spacing);
    validate_mpr_output(width, height, 1, 1, limits)?;
    let pixels = par_gray(width as usize, height as usize, |out_z, row| {
        let z = orthogonal_display_slice_index(out_z as i64, height, depth);
        for (x, out) in row.iter_mut().enumerate() {
            let (px, py) = if coronal {
                (x as i64, index)
            } else {
                (index, x as i64)
            };
            if let Some(value) = sampler.value_at(px, py, z) {
                *out = sampler.display_gray(value, mapper);
            }
        }
    });
    Ok(Some(RenderImage::gray(
        width as usize,
        height as usize,
        pixels,
    )))
}
pub(crate) fn mip_projection_range(center: i64, thickness: i64, count: i64) -> (i64, i64) {
    if count <= 1 {
        return (0, 0);
    }
    let thickness = if thickness <= 0 || thickness > count {
        count
    } else {
        thickness
    };
    let center = center.clamp(0, count - 1);
    let before = (thickness - 1) / 2;
    let after = thickness - 1 - before;
    let (mut start, mut end) = (center - before, center + after);
    if start < 0 {
        end -= start;
        start = 0;
    }
    if end >= count {
        start -= end - count + 1;
        end = count - 1;
    }
    (start.max(0), end)
}
#[derive(Clone, Copy)]
struct SlabAcc {
    mode: SlabMode,
    acc: f64,
    count: i64,
}
impl SlabAcc {
    fn new(mode: SlabMode) -> SlabAcc {
        SlabAcc {
            mode,
            acc: 0.0,
            count: 0,
        }
    }
    fn add(&mut self, v: f64) {
        match self.mode {
            SlabMode::MinIp => {
                if self.count == 0 || v < self.acc {
                    self.acc = v;
                }
            }
            SlabMode::Average | SlabMode::Sum => self.acc += v,
            _ => {
                if self.count == 0 || v > self.acc {
                    self.acc = v;
                }
            }
        }
        self.count += 1;
    }
    fn finish(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        if self.mode == SlabMode::Average {
            return Some(self.acc / self.count as f64);
        }
        Some(self.acc)
    }
}
#[allow(clippy::too_many_arguments)]
fn reslice_slab_range(
    sampler: &Sampler,
    plane: &Plane,
    slab_axis: Vec3,
    out_w: i64,
    out_h: i64,
    first_offset: f64,
    sample_count: i64,
    step: f64,
    mode: SlabMode,
    mapper: &PreparedVoi,
    limits: MprLimits,
) -> Result<RenderImage, RenderError> {
    validate_mpr_reslice(plane, out_w, out_h, sample_count, 1, limits)?;
    let normal = slab_axis.normalize();
    let step = if bad_spacing(step) { 1.0 } else { step };
    let denom_w = if out_w - 1 <= 0 {
        1.0
    } else {
        (out_w - 1) as f64
    };
    let denom_h = if out_h - 1 <= 0 {
        1.0
    } else {
        (out_h - 1) as f64
    };
    let pixels = par_gray(out_w as usize, out_h as usize, |j, row| {
        let t = j as f64 / denom_h;
        for (i, out) in row.iter_mut().enumerate() {
            let s = i as f64 / denom_w;
            let ray = sampler.vol.voxel_line(plane.at(s, t), normal);
            let mut acc = SlabAcc::new(mode);
            for k in 0..sample_count {
                if let Some(v) = sampler.trilinear_at(ray.at((first_offset + k as f64) * step)) {
                    acc.add(v);
                }
            }
            if let Some(value) = acc.finish() {
                *out = sampler.display_gray(value, mapper);
            }
        }
    });
    Ok(RenderImage::gray(out_w as usize, out_h as usize, pixels))
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObliqueSlab {
    pub thickness: i64,
    pub mode: SlabMode,
}
pub fn reslice_oblique_slab(
    vol: Option<&Arc<Volume>>,
    plane: &Plane,
    out_w: i64,
    out_h: i64,
    slab: ObliqueSlab,
    window: &WindowLevel,
    limits: MprLimits,
) -> (RenderImage, Option<RenderError>) {
    let ObliqueSlab { thickness, mode } = slab;
    let slab_samples = if thickness > 1 && mode != SlabMode::None {
        thickness
    } else {
        1
    };
    if let Err(e) = validate_mpr_reslice(plane, out_w, out_h, slab_samples, 1, limits) {
        return (blank_image(1, 1), Some(e));
    }
    let Some(vol) = vol else {
        return (blank_image(out_w, out_h), None);
    };
    if thickness <= 1 || mode == SlabMode::None {
        return reslice_oblique(Some(vol), plane, out_w, out_h, window, limits);
    }
    let window = normalize_window(
        window,
        &WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH),
    );
    let mapper = prepare_window(&window);
    let Ok(sampler) = Sampler::new(vol) else {
        return (blank_image(out_w, out_h), None);
    };
    let half = (thickness - 1) as f64 / 2.0;
    let axis = plane.u.cross(plane.v);
    match reslice_slab_range(
        &sampler,
        plane,
        axis,
        out_w,
        out_h,
        -half,
        thickness,
        vol.slice_spacing,
        mode,
        &mapper,
        limits,
    ) {
        Ok(img) => (img, None),
        Err(e) => (blank_image(1, 1), Some(e)),
    }
}
pub fn render_slab(
    series: &Stack,
    plane: MprPlane,
    center: i64,
    thickness: i64,
    mode: SlabMode,
    window: &WindowLevel,
    options: MprRenderOptions,
) -> (RenderImage, Option<RenderError>) {
    if mode == SlabMode::None {
        return render_mpr_plane(series, plane, center, window, options);
    }
    if series.frames.is_empty() {
        return (
            blank_image(512, 512),
            Some(RenderError::other("render: no series selected")),
        );
    }
    let Some(first) = first_renderable_slice(series) else {
        return (
            blank_image(512, 512),
            Some(RenderError::other(
                "render: series has no displayable images",
            )),
        );
    };
    let rows = i64::from(first.metadata.rows);
    let cols = i64::from(first.metadata.columns);
    let limits = match normalize_mpr_limits(options.limits) {
        Ok(l) => l,
        Err(e) => return (blank_image(1, 1), Some(e)),
    };
    let window = normalize_window(window, &series.default_window);
    let mapper = prepare_window(&window);
    let correct = options.gantry_tilt_mode != GantryTiltRenderMode::SourceGeometry;
    match stack_volume_sampler(series, correct) {
        Ok((vol, sampler)) => match render_cached_slab(
            &vol, &sampler, plane, center, thickness, mode, &mapper, correct, limits,
        ) {
            Err(e) => return (blank_image(1, 1), Some(e)),
            Ok(Some(img)) => return (img, None),
            Ok(None) => {}
        },
        Err(e) if e.is(ErrorKind::NonCorrectableGeometry) => {
            return (blank_image(512, 512), Some(e));
        }
        Err(_) => {}
    }
    let n = series.frames.len() as i64;
    let sample = |frame: Option<&super::frame::Frame>, x: i64, y: i64| -> Option<(f64, String)> {
        let v = pixel_value_at(frame, x, y)?;
        Some((
            v.rescaled,
            frame?.metadata.photometric_interpretation.clone(),
        ))
    };
    let reduce = |start: i64, end: i64, f: &dyn Fn(i64) -> Option<(f64, String)>| {
        let mut acc = SlabAcc::new(mode);
        let mut photometric = String::new();
        for i in start..=end {
            if let Some((v, ph)) = f(i) {
                acc.add(v);
                photometric = ph;
            }
        }
        acc.finish()
            .map(|v| display_gray_mapped(v, &mapper, &photometric))
    };
    match plane {
        MprPlane::Axial => {
            let (start, end) = mip_projection_range(center, thickness, n);
            if let Err(e) = validate_mpr_output(cols, rows, end - start + 1, 1, limits) {
                return (blank_image(1, 1), Some(e));
            }
            let mut pixels = vec![0u8; (cols * rows) as usize];
            for y in 0..rows {
                for x in 0..cols {
                    if let Some(g) = reduce(start, end, &|z| sample(series.frame(z as usize), x, y))
                    {
                        pixels[(y * cols + x) as usize] = g;
                    }
                }
            }
            (
                RenderImage::gray(cols as usize, rows as usize, pixels),
                None,
            )
        }
        MprPlane::Coronal | MprPlane::Sagittal => {
            let coronal = plane == MprPlane::Coronal;
            let (rs, cs) = series_pixel_spacing(series);
            let (width, axis_count, spacing) = if coronal {
                (cols, rows, cs)
            } else {
                (rows, cols, rs)
            };
            let (start, end) = mip_projection_range(center, thickness, axis_count);
            let height = orthogonal_mpr_height(series, spacing);
            if let Err(e) = validate_mpr_output(width, height, end - start + 1, 1, limits) {
                return (blank_image(1, 1), Some(e));
            }
            let mut pixels = vec![0u8; (width * height) as usize];
            for out_z in 0..height {
                let slice = series.frame(orthogonal_display_slice_index(out_z, height, n) as usize);
                for w in 0..width {
                    let g = reduce(start, end, &|k| {
                        if coronal {
                            sample(slice, w, k)
                        } else {
                            sample(slice, k, w)
                        }
                    });
                    if let Some(g) = g {
                        pixels[(out_z * width + w) as usize] = g;
                    }
                }
            }
            (
                RenderImage::gray(width as usize, height as usize, pixels),
                None,
            )
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn render_cached_slab(
    vol: &Volume,
    sampler: &Sampler,
    plane: MprPlane,
    center: i64,
    thickness: i64,
    mode: SlabMode,
    mapper: &PreparedVoi,
    correct_tilt: bool,
    limits: MprLimits,
) -> Result<Option<RenderImage>, RenderError> {
    let (rows, cols, depth) = (sampler.rows, sampler.cols, sampler.depth);
    if rows <= 0 || cols <= 0 || depth <= 0 {
        return Ok(None);
    }
    if correct_tilt
        && let Some((corrected, width, height)) = vol.corrected_orthogonal_plane(plane, center)
    {
        let (axis, step, count) = vol.corrected_slab_axis(plane);
        let clamped = clamp_index(center, count);
        let (start, end) = mip_projection_range(clamped, thickness, count);
        return reslice_slab_range(
            sampler,
            &corrected,
            axis,
            width,
            height,
            (start - clamped) as f64,
            end - start + 1,
            step,
            mode,
            mapper,
            limits,
        )
        .map(Some);
    }
    let reduce = |start: i64, end: i64, f: &dyn Fn(i64) -> Option<f64>| {
        let mut acc = SlabAcc::new(mode);
        for i in start..=end {
            if let Some(v) = f(i) {
                acc.add(v);
            }
        }
        acc.finish()
    };
    match plane {
        MprPlane::Axial => {
            let (start, end) = mip_projection_range(center, thickness, depth);
            validate_mpr_output(cols, rows, end - start + 1, 1, limits)?;
            let pixels = par_gray(cols as usize, rows as usize, |y, row| {
                for (x, out) in row.iter_mut().enumerate() {
                    if let Some(v) =
                        reduce(start, end, &|z| sampler.value_at(x as i64, y as i64, z))
                    {
                        *out = sampler.display_gray(v, mapper);
                    }
                }
            });
            Ok(Some(RenderImage::gray(
                cols as usize,
                rows as usize,
                pixels,
            )))
        }
        MprPlane::Coronal | MprPlane::Sagittal => {
            let coronal = plane == MprPlane::Coronal;
            let (width, axis_count, spacing) = if coronal {
                (cols, rows, vol.col_spacing)
            } else {
                (rows, cols, vol.row_spacing)
            };
            let (start, end) = mip_projection_range(center, thickness, axis_count);
            let height = orthogonal_volume_height(depth, vol.slice_spacing, spacing);
            validate_mpr_output(width, height, end - start + 1, 1, limits)?;
            let pixels = par_gray(width as usize, height as usize, |out_z, row| {
                let z = orthogonal_display_slice_index(out_z as i64, height, depth);
                for (w, out) in row.iter_mut().enumerate() {
                    let w = w as i64;
                    let v = reduce(start, end, &|k| {
                        if coronal {
                            sampler.value_at(w, k, z)
                        } else {
                            sampler.value_at(k, w, z)
                        }
                    });
                    if let Some(v) = v {
                        *out = sampler.display_gray(v, mapper);
                    }
                }
            });
            Ok(Some(RenderImage::gray(
                width as usize,
                height as usize,
                pixels,
            )))
        }
    }
}
pub fn render_mip(series: &Stack, window: &WindowLevel) -> (RenderImage, Option<RenderError>) {
    render_slab(
        series,
        MprPlane::Axial,
        0,
        0,
        SlabMode::Mip,
        window,
        MprRenderOptions::default(),
    )
}
pub fn render_volume_projection(
    series: &Stack,
    window: &WindowLevel,
) -> (RenderImage, Option<RenderError>) {
    if series.frames.is_empty() {
        return (
            blank_image(512, 512),
            Some(RenderError::other("render: no series selected")),
        );
    }
    let Some(first) = first_renderable_slice(series) else {
        return (
            blank_image(512, 512),
            Some(RenderError::other(
                "render: series has no displayable images",
            )),
        );
    };
    let (rows, cols) = (
        i64::from(first.metadata.rows),
        i64::from(first.metadata.columns),
    );
    let window = normalize_window(window, &series.default_window);
    let mapper = prepare_window(&window);
    let composite = |values: &mut dyn Iterator<Item = u8>| -> Option<u8> {
        let (mut color, mut alpha) = (0.0f64, 0.0f64);
        let mut found = false;
        for g in values {
            found = true;
            let gray = f64::from(g) / 255.0;
            let opacity = gray;
            color += (1.0 - alpha) * opacity * gray;
            alpha += (1.0 - alpha) * opacity;
            if alpha >= 0.995 {
                break;
            }
        }
        found.then(|| crate::fmath::f64_to_u8((clamp_unit(color) * 255.0).round()))
    };
    match series.volume() {
        Ok(vol) => {
            let Ok(sampler) = Sampler::new(&vol) else {
                return (
                    blank_image(512, 512),
                    Some(RenderError::other(
                        "render: volume contains no decodable voxels",
                    )),
                );
            };
            let (r, c) = (sampler.rows, sampler.cols);
            let mut pixels = vec![0u8; (r * c) as usize];
            for y in 0..r {
                for x in 0..c {
                    let mut it = (0..sampler.depth)
                        .filter_map(|z| sampler.value_at(x, y, z))
                        .map(|v| sampler.display_gray(v, &mapper));
                    if let Some(g) = composite(&mut it) {
                        pixels[(y * c + x) as usize] = g;
                    }
                }
            }
            (RenderImage::gray(c as usize, r as usize, pixels), None)
        }
        Err(e) if e.is(ErrorKind::NonCorrectableGeometry) || e.is(ErrorKind::VolumeStoreClosed) => {
            (blank_image(512, 512), Some(e))
        }
        Err(_) => {
            let mut frames: Vec<(Vec<f32>, i64, i64, String)> = Vec::new();
            for f in series.frames.iter().flatten() {
                if f.decode_err.is_some() {
                    continue;
                }
                if let Ok(values) = super::frame::decode_grayscale_f32(f)
                    && !values.is_empty()
                {
                    let md = super::frame::effective_frame_metadata(f);
                    frames.push((
                        values,
                        i64::from(md.rows),
                        i64::from(md.columns),
                        super::frame::normalized_photometric(&md.photometric_interpretation),
                    ));
                }
            }
            let mut pixels = vec![0u8; (rows * cols) as usize];
            for y in 0..rows {
                for x in 0..cols {
                    let mut it = frames.iter().filter_map(|(v, fr, fc, ph)| {
                        if x >= *fc || y >= *fr {
                            return None;
                        }
                        let value = f64::from(*v.get((y * fc + x) as usize)?);
                        Some(display_gray_mapped(value, &mapper, ph))
                    });
                    if let Some(g) = composite(&mut it) {
                        pixels[(y * cols + x) as usize] = g;
                    }
                }
            }
            (
                RenderImage::gray(cols as usize, rows as usize, pixels),
                None,
            )
        }
    }
}
fn clamp_unit(value: f64) -> f64 {
    if value < 0.0 || value.is_nan() {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}
