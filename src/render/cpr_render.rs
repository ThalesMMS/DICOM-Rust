#![forbid(unsafe_code)]
use super::cpr::{CprPath, CprPathSample};
use super::cpr_limits::{
    CprError, MAX_CPR_OUTPUT_DIMENSION, MAX_CPR_OUTPUT_PIXELS, MAX_CPR_SAMPLE_OPERATIONS,
    MAX_CPR_SLAB_SAMPLES, checked_cpr_product,
};
use super::cpr_path_assistant::{finite_vec3, positive_finite};
use super::error::RenderError;
use super::frame::{RenderImage, display_gray_mapped};
use super::mpr::Plane;
use super::parallel::{Cancellation, ctx_err, parallel_rows_worker_context};
use super::vec3::Vec3;
use super::view_state::Interpolation;
use super::volume::{Sampler, Volume};
use super::volume_interpolation::sample_at;
use super::window::{
    DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH, WindowLevel, normalize_window, prepare_window,
};
use crate::fmath;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CprMode(pub i64);
impl CprMode {
    pub const STRAIGHTENED: CprMode = CprMode(0);
    pub const STRETCHED: CprMode = CprMode(1);
    pub const TRANSVERSE: CprMode = CprMode(2);
    pub const SLAB: CprMode = CprMode(3);
}
pub const CPR_SLAB_NONE: i64 = 0;
pub const CPR_SLAB_MIP: i64 = 1;
pub const CPR_SLAB_MIN_IP: i64 = 2;
pub const CPR_SLAB_AVERAGE: i64 = 3;
#[derive(Debug, Clone, Default)]
pub struct CprRequest {
    pub mode: CprMode,
    pub volume: Option<Arc<Volume>>,
    pub path: Option<Arc<CprPath>>,
    pub window: WindowLevel,
    pub width: i64,
    pub arc_spacing: f64,
    pub cross_spacing: f64,
    pub arc_length: f64,
    pub interpolation: Interpolation,
    pub thickness_mm: f64,
    pub thickness: i64,
    pub slab_mode: i64,
    pub stretch_dir: Vec3,
    pub rotation_degrees: f64,
    pub generation: u64,
}
#[derive(Debug, Clone, PartialEq)]
pub enum CprRenderError {
    Input,
    Cpr(CprError),
    Context(RenderError),
    Superseded,
}
impl CprRenderError {
    pub fn is_input(&self) -> bool {
        match self {
            CprRenderError::Input => true,
            CprRenderError::Cpr(e) => e.is_input(),
            _ => false,
        }
    }
    pub fn is_limit(&self) -> bool {
        matches!(self, CprRenderError::Cpr(e) if e.is_limit())
    }
}
impl fmt::Display for CprRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CprRenderError::Input => f.write_str("render: invalid CPR input"),
            CprRenderError::Cpr(e) => e.fmt(f),
            CprRenderError::Context(e) => e.fmt(f),
            CprRenderError::Superseded => {
                f.write_str("render: CPR render superseded by a newer generation")
            }
        }
    }
}
impl std::error::Error for CprRenderError {}
impl From<CprError> for CprRenderError {
    fn from(e: CprError) -> Self {
        CprRenderError::Cpr(e)
    }
}
type Ctx<'a> = Option<&'a dyn Cancellation>;
fn check(ctx: Ctx<'_>) -> Result<(), CprRenderError> {
    match ctx_err(ctx) {
        Some(e) => Err(CprRenderError::Context(e)),
        None => Ok(()),
    }
}
impl CprRequest {
    fn out_width(&self) -> i64 {
        if self.width > 0 { self.width } else { 64 }
    }
    fn arc_step(&self) -> f64 {
        if self.arc_spacing > 0.0 {
            return self.arc_spacing;
        }
        match &self.volume {
            Some(v) if v.row_spacing > 0.0 => v.row_spacing,
            _ => 1.0,
        }
    }
    fn cross_step(&self) -> f64 {
        if self.cross_spacing > 0.0 {
            return self.cross_spacing;
        }
        match &self.volume {
            Some(v) if v.col_spacing > 0.0 => v.col_spacing,
            _ => 1.0,
        }
    }
}
pub(crate) fn validate_cpr_request(req: &CprRequest) -> Result<(), CprError> {
    if req.width < 0 {
        return Err(CprError::input("width", "width cannot be negative"));
    }
    if req.out_width() as u64 > MAX_CPR_OUTPUT_DIMENSION {
        return Err(CprError::limit(
            "output_dimension",
            req.out_width() as u64,
            MAX_CPR_OUTPUT_DIMENSION,
        ));
    }
    for (name, value) in [
        ("arc_spacing", req.arc_spacing),
        ("cross_spacing", req.cross_spacing),
        ("thickness_mm", req.thickness_mm),
    ] {
        if value < 0.0 || value.is_nan() || value.is_infinite() {
            return Err(CprError::input(
                name,
                "value must be zero or positive and finite",
            ));
        }
    }
    for (name, value) in [
        ("arc_length", req.arc_length),
        ("rotation_degrees", req.rotation_degrees),
        ("window_center", req.window.center),
        ("window_width", req.window.width),
    ] {
        if value.is_nan() || value.is_infinite() {
            return Err(CprError::input(name, "value is not finite"));
        }
    }
    if req.stretch_dir != Vec3::ZERO && !finite_vec3(req.stretch_dir) {
        return Err(CprError::input(
            "stretch_direction",
            "coordinate is not finite",
        ));
    }
    if req.thickness < 0 {
        return Err(CprError::input(
            "thickness",
            "sample count cannot be negative",
        ));
    }
    if req.thickness as u64 > MAX_CPR_SLAB_SAMPLES {
        return Err(CprError::limit(
            "slab_samples",
            req.thickness as u64,
            MAX_CPR_SLAB_SAMPLES,
        ));
    }
    if req.slab_mode < CPR_SLAB_NONE || req.slab_mode > CPR_SLAB_AVERAGE {
        return Err(CprError::input("slab_mode", "mode is not supported"));
    }
    if req.mode.0 < CprMode::STRAIGHTENED.0 || req.mode.0 > CprMode::SLAB.0 {
        return Err(CprError::input("mode", "mode is not supported"));
    }
    Ok(())
}
fn validate_cpr_output(width: i64, height: i64, slab_samples: i64) -> Result<(), CprError> {
    if width <= 0 || height <= 0 {
        return Err(CprError::input(
            "output_dimensions",
            "dimensions must be positive",
        ));
    }
    if width as u64 > MAX_CPR_OUTPUT_DIMENSION || height as u64 > MAX_CPR_OUTPUT_DIMENSION {
        return Err(CprError::limit(
            "output_dimension",
            width.max(height) as u64,
            MAX_CPR_OUTPUT_DIMENSION,
        ));
    }
    let pixels = checked_cpr_product(
        "output_pixels",
        MAX_CPR_OUTPUT_PIXELS,
        &[width as u64, height as u64],
    )?;
    let slab = slab_samples.max(1) as u64;
    checked_cpr_product(
        "sample_operations",
        MAX_CPR_SAMPLE_OPERATIONS,
        &[pixels, slab],
    )?;
    Ok(())
}
fn cpr_physical_sampling_step(volume: Option<&Volume>) -> f64 {
    let mut step = f64::INFINITY;
    if let Some(v) = volume {
        for spacing in [v.row_spacing, v.col_spacing, v.slice_spacing] {
            if positive_finite(spacing) && spacing < step {
                step = spacing;
            }
        }
    }
    if !positive_finite(step) {
        return 1.0;
    }
    step
}
fn cpr_slab_sample_count(req: &CprRequest) -> Result<i64, CprError> {
    if req.slab_mode == CPR_SLAB_NONE {
        return Ok(0);
    }
    let thickness = req.thickness_mm;
    if positive_finite(thickness) {
        let step = cpr_physical_sampling_step(req.volume.as_deref());
        let count_float = (thickness / step).ceil() + 1.0;
        if !positive_finite(count_float) {
            return Err(CprError::input(
                "slab_samples",
                "sample count is not finite",
            ));
        }
        if count_float > MAX_CPR_SLAB_SAMPLES as f64 {
            return Err(CprError::limit(
                "slab_samples",
                MAX_CPR_SLAB_SAMPLES + 1,
                MAX_CPR_SLAB_SAMPLES,
            ));
        }
        return Ok(fmath::f64_to_i64(count_float).max(2));
    }
    if req.thickness <= 1 {
        return Ok(0);
    }
    if req.thickness as u64 > MAX_CPR_SLAB_SAMPLES {
        return Err(CprError::limit(
            "slab_samples",
            req.thickness as u64,
            MAX_CPR_SLAB_SAMPLES,
        ));
    }
    Ok(req.thickness)
}
fn cpr_slab_sample_offsets(req: &CprRequest, legacy_step: f64) -> Result<Vec<f64>, CprError> {
    let count = cpr_slab_sample_count(req)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let legacy_step = if positive_finite(legacy_step) {
        legacy_step
    } else {
        1.0
    };
    let thickness = req.thickness_mm;
    if positive_finite(thickness) {
        return Ok((0..count)
            .map(|i| -thickness / 2.0 + thickness * i as f64 / (count - 1) as f64)
            .collect());
    }
    let half = (count - 1) as f64 / 2.0;
    Ok((0..count)
        .map(|i| (i as f64 - half) * legacy_step)
        .collect())
}
pub(crate) fn rotate_cpr_basis(normal: Vec3, tangent: Vec3, degrees: f64) -> (Vec3, Vec3) {
    let degrees = if degrees.is_nan() || degrees.is_infinite() {
        0.0
    } else {
        degrees
    };
    let normal = normal
        .rotate(tangent, degrees * std::f64::consts::PI / 180.0)
        .normalize();
    let binormal = tangent.cross(normal).normalize();
    let normal = binormal.cross(tangent).normalize();
    (normal, binormal)
}
#[derive(Debug, Clone, Copy, Default)]
struct PreparedBasis {
    cross: Vec3,
    slab: Vec3,
}
const CPR_PREPARED_BASIS_BYTES: u64 = 48;
const MAX_CPR_PREPARED_BASIS_BYTES: u64 = CPR_PREPARED_BASIS_BYTES * MAX_CPR_OUTPUT_DIMENSION;
fn prepare_cpr_bases(
    ctx: Ctx<'_>,
    req: &CprRequest,
    samples: &[CprPathSample],
    stretched: bool,
) -> Result<Vec<PreparedBasis>, CprRenderError> {
    check(ctx)?;
    checked_cpr_product(
        "prepared_basis_bytes",
        MAX_CPR_PREPARED_BASIS_BYTES,
        &[samples.len() as u64, CPR_PREPARED_BASIS_BYTES],
    )?;
    let mut bases = vec![PreparedBasis::default(); samples.len()];
    let mut reference = req.stretch_dir;
    if stretched && reference == Vec3::ZERO && !samples.is_empty() {
        reference = samples[0].normal;
    }
    for (c, s) in samples.iter().enumerate() {
        if c & 63 == 0 {
            check(ctx)?;
        }
        let mut cross = s.normal;
        if stretched {
            cross = reference
                .sub(s.tangent.scale(reference.dot(s.tangent)))
                .normalize();
            if cross == Vec3::ZERO {
                cross = s.normal;
            }
        }
        let (rotated, slab) = rotate_cpr_basis(cross, s.tangent, req.rotation_degrees);
        bases[c] = PreparedBasis {
            cross: rotated,
            slab,
        };
    }
    check(ctx)?;
    Ok(bases)
}
struct PreparedMapping<'a> {
    vol: &'a Volume,
    regular: bool,
}
impl<'a> PreparedMapping<'a> {
    fn new(source: &Arc<Volume>, sampled: &'a Arc<Volume>) -> PreparedMapping<'a> {
        let v = sampled;
        let regular = Arc::ptr_eq(source, sampled)
            && v.cpr_regular_layout()
            && finite_vec3(v.origin)
            && finite_vec3(v.axis_x)
            && finite_vec3(v.axis_y)
            && finite_vec3(v.normal)
            && v.col_spacing > 0.0
            && v.row_spacing > 0.0
            && !v.col_spacing.is_infinite()
            && !v.row_spacing.is_infinite();
        PreparedMapping { vol: v, regular }
    }
    fn at(&self, p: Vec3, interval: &mut i64) -> Vec3 {
        if !self.regular {
            return self.vol.patient_to_voxel(p);
        }
        let v = self.vol;
        let d = p.sub(v.origin);
        let z = if v.slice_positions().is_empty() {
            d.dot(v.normal) / v.effective_slice_spacing()
        } else {
            self.position_to_index(d.dot(v.normal), interval)
        };
        Vec3::new(
            d.dot(v.axis_x) / v.col_spacing,
            d.dot(v.axis_y) / v.row_spacing,
            z,
        )
    }
    fn position_to_index(&self, pos: f64, interval: &mut i64) -> f64 {
        let p = self.vol.slice_positions();
        let n = p.len() as i64;
        let lo = *interval;
        if n > 1
            && lo >= 0
            && lo < n - 1
            && pos > p[0]
            && pos < p[(n - 1) as usize]
            && p[lo as usize] <= pos
            && pos < p[lo as usize + 1]
        {
            let span = p[lo as usize + 1] - p[lo as usize];
            if span != 0.0 {
                return lo as f64 + (pos - p[lo as usize]) / span;
            }
        }
        let z = self.vol.slice_index_of_position(pos);
        *interval = -1;
        if z >= 0.0 && z < (n - 1) as f64 {
            *interval = fmath::f64_to_i64(z.floor());
        }
        z
    }
}
fn reduce_slab(
    mode: i64,
    count: usize,
    mut sample: impl FnMut(usize) -> Option<f64>,
) -> Option<f64> {
    let mut acc = 0.0f64;
    let mut n = 0i64;
    for i in 0..count {
        let Some(v) = sample(i) else { continue };
        match mode {
            CPR_SLAB_MIN_IP => {
                if n == 0 || v < acc {
                    acc = v;
                }
            }
            CPR_SLAB_AVERAGE => acc += v,
            _ => {
                if n == 0 || v > acc {
                    acc = v;
                }
            }
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    if mode == CPR_SLAB_AVERAGE {
        return Some(acc / n as f64);
    }
    Some(acc)
}
fn sample_rows(
    ctx: Ctx<'_>,
    cols: usize,
    rows: usize,
    row: impl Fn(usize, &mut [Option<f64>]) + Sync,
) -> Result<Vec<Vec<Option<f64>>>, CprRenderError> {
    let cells: Vec<OnceLock<Vec<Option<f64>>>> = (0..rows).map(|_| OnceLock::new()).collect();
    parallel_rows_worker_context(ctx, rows as i64, || {
        |r: i64| {
            let mut out = vec![None; cols];
            row(r as usize, &mut out);
            let _ = cells[r as usize].set(out);
        }
    })
    .map_err(CprRenderError::Context)?;
    Ok(cells
        .into_iter()
        .map(|c| c.into_inner().unwrap_or_else(|| vec![None; cols]))
        .collect())
}
struct Sampled {
    cols: usize,
    rows: usize,
    values: Vec<Vec<Option<f64>>>,
    photometric: String,
}
fn sample_longitudinal(
    ctx: Ctx<'_>,
    req: &CprRequest,
    vol: &Arc<Volume>,
    path: &CprPath,
    stretched: bool,
) -> Result<Sampled, CprRenderError> {
    let sample_count = path.sample_count_checked(req.arc_step())?;
    let width = req.out_width();
    let cross = req.cross_step();
    let slab_samples = cpr_slab_sample_count(req)?;
    validate_cpr_output(sample_count, width, slab_samples)?;
    let samples = path.resample_checked(req.arc_step())?;
    let slab_offsets = cpr_slab_sample_offsets(req, cross)?;
    let sampler = Sampler::new(vol).map_err(|_| CprRenderError::Input)?;
    let half = (width - 1) as f64 / 2.0;
    let bases = prepare_cpr_bases(ctx, req, &samples, stretched)?;
    let mapping = PreparedMapping::new(vol, &sampler.vol);
    let (mode, interpolation) = (req.slab_mode, req.interpolation);
    let values = sample_rows(ctx, samples.len(), width as usize, |r, out| {
        let offset = (r as f64 - half) * cross;
        for (c, s) in samples.iter().enumerate() {
            let (cross_dir, slab_dir) = (bases[c].cross, bases[c].slab);
            let base = s.position.add(cross_dir.scale(offset));
            if !slab_offsets.is_empty() {
                let mut interval = -1i64;
                out[c] = reduce_slab(mode, slab_offsets.len(), |k| {
                    let p = base.add(slab_dir.scale(slab_offsets[k]));
                    sample_at(&sampler, mapping.at(p, &mut interval), interpolation)
                });
                continue;
            }
            out[c] = sample_at(&sampler, sampler.vol.patient_to_voxel(base), interpolation);
        }
    })?;
    Ok(Sampled {
        cols: samples.len(),
        rows: width as usize,
        values,
        photometric: vol.photometric().to_string(),
    })
}
fn sample_transverse(
    ctx: Ctx<'_>,
    req: &CprRequest,
    vol: &Arc<Volume>,
    path: &CprPath,
) -> Result<(Sampled, Plane), CprRenderError> {
    let frame = path.frame_at(req.arc_length);
    let (normal, binormal) = rotate_cpr_basis(frame.normal, frame.tangent, req.rotation_degrees);
    let side = req.out_width();
    let cross = req.cross_step();
    let slab_samples = cpr_slab_sample_count(req)?;
    validate_cpr_output(side, side, slab_samples)?;
    let slab_offsets = cpr_slab_sample_offsets(req, cross)?;
    let sampler = Sampler::new(vol).map_err(|_| CprRenderError::Input)?;
    let half = (side - 1) as f64 / 2.0;
    let extent = (side - 1) as f64 * cross;
    let plane = Plane {
        origin: frame
            .position
            .add(normal.scale(-half * cross))
            .add(binormal.scale(-half * cross)),
        u: normal.scale(extent),
        v: binormal.scale(extent),
    };
    let mapping = PreparedMapping::new(vol, &sampler.vol);
    let (mode, interpolation) = (req.slab_mode, req.interpolation);
    let values = sample_rows(ctx, side as usize, side as usize, |j, out| {
        let voff = (j as f64 - half) * cross;
        for (i, cell) in out.iter_mut().enumerate() {
            let uoff = (i as f64 - half) * cross;
            let base = frame
                .position
                .add(normal.scale(uoff))
                .add(binormal.scale(voff));
            if !slab_offsets.is_empty() {
                let mut interval = -1i64;
                *cell = reduce_slab(mode, slab_offsets.len(), |k| {
                    let p = base.add(frame.tangent.scale(slab_offsets[k]));
                    sample_at(&sampler, mapping.at(p, &mut interval), interpolation)
                });
                continue;
            }
            *cell = sample_at(&sampler, sampler.vol.patient_to_voxel(base), interpolation);
        }
    })?;
    Ok((
        Sampled {
            cols: side as usize,
            rows: side as usize,
            values,
            photometric: vol.photometric().to_string(),
        },
        plane,
    ))
}
fn gray_image(req: &CprRequest, sampled: &Sampled) -> RenderImage {
    let window = normalize_window(
        &req.window,
        &WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH),
    );
    let mapper = prepare_window(&window);
    let mut pixels = vec![0u8; sampled.cols * sampled.rows];
    for (y, row) in sampled.values.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if let Some(v) = value {
                pixels[y * sampled.cols + x] =
                    display_gray_mapped(*v, &mapper, &sampled.photometric);
            }
        }
    }
    RenderImage::gray(sampled.cols, sampled.rows, pixels)
}
pub fn render_cpr(ctx: Ctx<'_>, req: &CprRequest) -> Result<RenderImage, CprRenderError> {
    let (Some(vol), Some(path)) = (&req.volume, &req.path) else {
        return Err(CprRenderError::Input);
    };
    check(ctx)?;
    validate_cpr_request(req)?;
    let sampled = match req.mode {
        CprMode::TRANSVERSE => sample_transverse(ctx, req, vol, path)?.0,
        CprMode::STRETCHED => sample_longitudinal(ctx, req, vol, path, true)?,
        _ => sample_longitudinal(ctx, req, vol, path, false)?,
    };
    Ok(gray_image(req, &sampled))
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CprScalar {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f64>,
    pub valid: Vec<bool>,
    pub plane: Option<Plane>,
    pub column_spacing: f64,
    pub row_spacing: f64,
}
pub fn render_cpr_scalar(ctx: Ctx<'_>, req: &CprRequest) -> Result<CprScalar, CprRenderError> {
    let (Some(vol), Some(path)) = (&req.volume, &req.path) else {
        return Err(CprRenderError::Input);
    };
    check(ctx)?;
    validate_cpr_request(req)?;
    let (sampled, plane, column_spacing) = match req.mode {
        CprMode::TRANSVERSE => {
            let (sampled, plane) = sample_transverse(ctx, req, vol, path)?;
            (sampled, Some(plane), req.cross_step())
        }
        mode => (
            sample_longitudinal(ctx, req, vol, path, mode == CprMode::STRETCHED)?,
            None,
            req.arc_step(),
        ),
    };
    let mut out = CprScalar {
        width: sampled.cols,
        height: sampled.rows,
        values: vec![0.0; sampled.cols * sampled.rows],
        valid: vec![false; sampled.cols * sampled.rows],
        plane,
        column_spacing,
        row_spacing: req.cross_step(),
    };
    for (y, row) in sampled.values.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if let Some(v) = value {
                out.values[y * sampled.cols + x] = *v;
                out.valid[y * sampled.cols + x] = true;
            }
        }
    }
    Ok(out)
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CprColumn {
    pub origin_lps: [f64; 3],
    pub step_v_lps: [f64; 3],
    pub slab_axis_lps: [f64; 3],
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CprGeometry {
    pub columns: Vec<CprColumn>,
    pub width: i64,
    pub height: i64,
    pub slab_thickness_mm: f64,
    pub slab_sample_spacing_mm: f64,
    pub column_spacing_mm: f64,
    pub row_spacing_mm: f64,
    pub plane: Option<Plane>,
}
fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
pub fn prepare_cpr_geometry(ctx: Ctx<'_>, req: &CprRequest) -> Result<CprGeometry, CprRenderError> {
    check(ctx)?;
    validate_cpr_request(req)?;
    let Some(path) = &req.path else {
        return Err(CprRenderError::Input);
    };
    let rows = req.out_width();
    let mut count = rows;
    if req.mode != CprMode::TRANSVERSE {
        count = path.sample_count_checked(req.arc_step())?;
    }
    let slab_count = cpr_slab_sample_count(req)?;
    validate_cpr_output(count, rows, slab_count)?;
    let offsets = cpr_slab_sample_offsets(req, req.cross_step())?;
    let mut geometry = CprGeometry {
        width: count,
        height: rows,
        columns: vec![CprColumn::default(); count as usize],
        column_spacing_mm: req.arc_step(),
        row_spacing_mm: req.cross_step(),
        ..Default::default()
    };
    if offsets.len() > 1 {
        geometry.slab_thickness_mm = offsets[offsets.len() - 1] - offsets[0];
        geometry.slab_sample_spacing_mm = offsets[1] - offsets[0];
    }
    let cross = req.cross_step();
    let half = (rows - 1) as f64 / 2.0;
    if req.mode == CprMode::TRANSVERSE {
        let frame = path.frame_at(req.arc_length);
        let (u, v) = rotate_cpr_basis(frame.normal, frame.tangent, req.rotation_degrees);
        for (x, column) in geometry.columns.iter_mut().enumerate() {
            check(ctx)?;
            let origin = frame
                .position
                .add(u.scale((x as f64 - half) * cross))
                .add(v.scale(-half * cross));
            *column = CprColumn {
                origin_lps: array(origin),
                step_v_lps: array(v.scale(cross)),
                slab_axis_lps: array(frame.tangent),
            };
        }
        geometry.column_spacing_mm = cross;
        geometry.plane = Some(Plane {
            origin: frame
                .position
                .add(u.scale(-half * cross))
                .add(v.scale(-half * cross)),
            u: u.scale((count - 1) as f64 * cross),
            v: v.scale((rows - 1) as f64 * cross),
        });
    } else {
        let samples = path.resample_checked(req.arc_step())?;
        let bases = prepare_cpr_bases(ctx, req, &samples, req.mode == CprMode::STRETCHED)?;
        for (x, sample) in samples.iter().enumerate() {
            check(ctx)?;
            let basis = bases[x];
            geometry.columns[x] = CprColumn {
                origin_lps: array(sample.position.add(basis.cross.scale(-half * cross))),
                step_v_lps: array(basis.cross.scale(cross)),
                slab_axis_lps: array(basis.slab),
            };
        }
    }
    Ok(geometry)
}
#[derive(Debug, Default)]
pub struct CprRenderer {
    generation: AtomicU64,
}
impl CprRenderer {
    pub fn new() -> CprRenderer {
        CprRenderer::default()
    }
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }
    pub fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    pub fn render(&self, ctx: Ctx<'_>, req: &CprRequest) -> Result<RenderImage, CprRenderError> {
        if req.generation != 0 && req.generation != self.current_generation() {
            return Err(CprRenderError::Superseded);
        }
        let image = render_cpr(ctx, req)?;
        if req.generation != 0 && req.generation != self.current_generation() {
            return Err(CprRenderError::Superseded);
        }
        Ok(image)
    }
}
