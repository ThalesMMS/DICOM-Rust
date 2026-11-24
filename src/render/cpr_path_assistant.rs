#![forbid(unsafe_code)]
use super::cpr_limits::{CprError, MAX_CPR_PATH_LENGTH_MM, MAX_CPR_PATH_SAMPLES};
use super::error::{ErrorKind, RenderError};
use super::fast_marching::{
    FastMarchingError, FastMarchingErrorKind, FastMarchingGrid, FastMarchingOptions,
    FastMarchingSeed, fast_marching,
};
use super::grid::nearest_index;
use super::parallel::{Cancellation, ctx_err};
use super::vec3::Vec3;
use super::volume::{Sampler, Volume};
use crate::fmath;
use std::fmt;
use std::sync::Arc;
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub const ERR_CPR_PATH_ASSISTANT_INPUT: &str = "render: invalid CPR path assistant input";
pub const ERR_CPR_PATH_ASSISTANT_MEMORY: &str =
    "render: CPR path assistant search exceeds memory budget";
pub const ERR_CPR_PATH_ASSISTANT_NOT_FOUND: &str = "render: CPR path assistant route not found";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CprPathMethod(pub i64);
impl CprPathMethod {
    pub const FAST_MARCHING: CprPathMethod = CprPathMethod(0);
    pub(crate) const DIJKSTRA: CprPathMethod = CprPathMethod(1);
}
#[derive(Debug, Clone, Default)]
pub struct CprPathAssistantOptions {
    pub method: CprPathMethod,
    pub grid_spacing_mm: f64,
    pub margin_mm: f64,
    pub output_spacing_mm: f64,
    pub intensity_weight: f64,
    pub max_grid_voxels: i64,
    pub max_expanded_nodes: i64,
    pub max_queue_entries: i64,
    pub max_output_points: i64,
    pub smoothing_mm: f64,
    pub speed_volume: Option<Arc<Volume>>,
    pub vessel_guided: bool,
    pub speed_exponent: f64,
    pub centring_exponent: f64,
    pub no_centring: bool,
}
const DEFAULT_GRID_SPACING_MM: f64 = 2.0;
const DEFAULT_FAST_MARCHING_GRID_MM: f64 = 1.0;
const DEFAULT_SPEED_EXPONENT: f64 = 2.0;
const DEFAULT_CENTRING_EXPONENT: f64 = 1.0;
const SPEED_FLOOR: f64 = 1e-3;
const DEFAULT_MARGIN_MM: f64 = 30.0;
const DEFAULT_OUTPUT_SPACING_MM: f64 = 3.0;
const DEFAULT_INTENSITY_WEIGHT: f64 = 64.0;
const DEFAULT_MAX_GRID_VOXELS: i64 = 1_000_000;
const DEFAULT_MAX_QUEUE_ENTRIES: i64 = 2_000_000;
const DEFAULT_MAX_OUTPUT_POINTS: i64 = 250_000;
pub const MAX_CPR_ASSISTANT_GRID_VOXELS: i64 = 4_000_000;
pub const MAX_CPR_ASSISTANT_EXPANDED_NODES: i64 = 4_000_000;
pub const MAX_CPR_ASSISTANT_QUEUE_ENTRIES: i64 = 4_000_000;
pub const MAX_CPR_ASSISTANT_OUTPUT_POINTS: i64 = MAX_CPR_PATH_SAMPLES as i64;
pub const MAX_CPR_ASSISTANT_SMOOTHING_MM: f64 = 20.0;
const MAX_SMOOTHING_PASSES: i64 = 256;
#[derive(Debug, Clone, PartialEq)]
pub enum CprAssistError {
    Input(Option<String>),
    Limit {
        resource: String,
        value: i64,
        limit: i64,
    },
    NotFound(Option<String>),
    Segment {
        from: i64,
        to: i64,
        err: Box<CprAssistError>,
    },
    Cpr(CprError),
    CprBare,
    Context(RenderError),
    FastMarching(FastMarchingError),
}
impl CprAssistError {
    pub fn is_input(&self) -> bool {
        match self {
            CprAssistError::Input(_) => true,
            CprAssistError::Segment { err, .. } => err.is_input(),
            _ => false,
        }
    }
    pub fn is_memory(&self) -> bool {
        match self {
            CprAssistError::Limit { .. } => true,
            CprAssistError::Segment { err, .. } => err.is_memory(),
            _ => false,
        }
    }
    pub fn is_not_found(&self) -> bool {
        match self {
            CprAssistError::NotFound(_) => true,
            CprAssistError::Segment { err, .. } => err.is_not_found(),
            _ => false,
        }
    }
    pub fn is_canceled(&self) -> bool {
        match self {
            CprAssistError::Context(e) => e.is(ErrorKind::Canceled),
            CprAssistError::FastMarching(e) => e.is(FastMarchingErrorKind::Canceled),
            CprAssistError::Segment { err, .. } => err.is_canceled(),
            _ => false,
        }
    }
    pub fn is_cpr_input(&self) -> bool {
        match self {
            CprAssistError::Cpr(e) => e.is_input(),
            CprAssistError::CprBare => true,
            CprAssistError::Segment { err, .. } => err.is_cpr_input(),
            _ => false,
        }
    }
    pub fn segment(&self) -> Option<(i64, i64)> {
        match self {
            CprAssistError::Segment { from, to, .. } => Some((*from, *to)),
            _ => None,
        }
    }
    pub fn limit(&self) -> Option<(&str, i64, i64)> {
        match self {
            CprAssistError::Limit {
                resource,
                value,
                limit,
            } => Some((resource, *value, *limit)),
            CprAssistError::Segment { err, .. } => err.limit(),
            _ => None,
        }
    }
    pub fn cpr_input(&self) -> Option<&CprError> {
        match self {
            CprAssistError::Cpr(e) => Some(e),
            CprAssistError::Segment { err, .. } => err.cpr_input(),
            _ => None,
        }
    }
    fn limit_error(resource: &str, value: i64, limit: i64) -> CprAssistError {
        CprAssistError::Limit {
            resource: resource.to_string(),
            value,
            limit,
        }
    }
    fn input(detail: impl Into<String>) -> CprAssistError {
        CprAssistError::Input(Some(detail.into()))
    }
}
impl fmt::Display for CprAssistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CprAssistError::Input(None) => f.write_str(ERR_CPR_PATH_ASSISTANT_INPUT),
            CprAssistError::Input(Some(d)) => write!(f, "{ERR_CPR_PATH_ASSISTANT_INPUT}: {d}"),
            CprAssistError::Limit {
                resource,
                value,
                limit,
            } => write!(
                f,
                "{ERR_CPR_PATH_ASSISTANT_MEMORY}: {resource}={value}, limit={limit}"
            ),
            CprAssistError::NotFound(None) => f.write_str(ERR_CPR_PATH_ASSISTANT_NOT_FOUND),
            CprAssistError::NotFound(Some(d)) => {
                write!(f, "{ERR_CPR_PATH_ASSISTANT_NOT_FOUND}: {d}")
            }
            CprAssistError::Segment { from, to, err } => write!(f, "waypoints {from}-{to}: {err}"),
            CprAssistError::Cpr(e) => e.fmt(f),
            CprAssistError::CprBare => f.write_str("render: invalid CPR input"),
            CprAssistError::Context(e) => e.fmt(f),
            CprAssistError::FastMarching(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for CprAssistError {}
fn ctx_check(ctx: Ctx<'_>) -> Result<(), CprAssistError> {
    match ctx_err(ctx) {
        Some(e) => Err(CprAssistError::Context(e)),
        None => Ok(()),
    }
}
pub fn find_cpr_assisted_path(
    ctx: Ctx<'_>,
    volume: Option<&Arc<Volume>>,
    waypoints: &[Vec3],
    options: &CprPathAssistantOptions,
) -> Result<Vec<Vec3>, CprAssistError> {
    ctx_check(ctx)?;
    let Some(volume) = volume.filter(|v| {
        v.cols > 0 && v.rows > 0 && v.depth > 0 && waypoints.len() >= 2 && waypoints.len() <= 5
    }) else {
        return Err(CprAssistError::Input(None));
    };
    for (i, point) in waypoints.iter().enumerate() {
        if !finite_vec3(*point) || !voxel_inside_volume(volume, volume.patient_to_voxel(*point)) {
            return Err(CprAssistError::input(format!(
                "waypoint {} lies outside the volume",
                i + 1
            )));
        }
    }
    let mut options = normalized_options(options)?;
    let mut vessel_speed = None;
    if options.vessel_guided {
        let speed = prepare_cpr_vessel_speed(ctx, volume, waypoints, &options)?;
        options.speed_volume = Some(Arc::clone(&speed));
        vessel_speed = Some(speed);
    }
    let result = find_assisted_segments(ctx, volume, waypoints, &options);
    if let Some(speed) = vessel_speed {
        speed.close();
    }
    result
}
fn find_assisted_segments(
    ctx: Ctx<'_>,
    volume: &Arc<Volume>,
    waypoints: &[Vec3],
    options: &CprPathAssistantOptions,
) -> Result<Vec<Vec3>, CprAssistError> {
    let options = options.clone();
    let mut centerline: Vec<Vec3> = Vec::with_capacity(waypoints.len() * 16);
    for i in 1..waypoints.len() {
        let wrap = |err: CprAssistError| CprAssistError::Segment {
            from: i as i64,
            to: i as i64 + 1,
            err: Box::new(err),
        };
        let find = if options.method == CprPathMethod::DIJKSTRA {
            find_dijkstra_segment
        } else {
            find_fast_marching_segment
        };
        let mut segment =
            find(ctx, volume, waypoints[i - 1], waypoints[i], &options).map_err(wrap)?;
        if options.smoothing_mm > 0.0 {
            segment = smooth_segment(ctx, &segment, &options).map_err(wrap)?;
        }
        let remaining = options.max_output_points - centerline.len() as i64;
        segment =
            resample_segment(ctx, &segment, options.output_spacing_mm, remaining).map_err(wrap)?;
        if segment.len() < 2 {
            return Err(wrap(CprAssistError::NotFound(None)));
        }
        let mut start = 0;
        if let Some(last) = centerline.last()
            && segment[0].sub(*last).length() < 1e-9
        {
            start = 1;
        }
        centerline.extend_from_slice(&segment[start..]);
    }
    Ok(centerline)
}
fn normalized_options(
    options: &CprPathAssistantOptions,
) -> Result<CprPathAssistantOptions, CprAssistError> {
    for (name, value) in [
        ("grid_spacing_mm", options.grid_spacing_mm),
        ("margin_mm", options.margin_mm),
        ("output_spacing_mm", options.output_spacing_mm),
        ("intensity_weight", options.intensity_weight),
        ("smoothing_mm", options.smoothing_mm),
        ("speed_exponent", options.speed_exponent),
        ("centring_exponent", options.centring_exponent),
    ] {
        if value < 0.0 || value.is_nan() || value.is_infinite() {
            return Err(CprAssistError::input(format!(
                "{name} must be zero or positive and finite"
            )));
        }
    }
    if options.smoothing_mm > MAX_CPR_ASSISTANT_SMOOTHING_MM {
        return Err(CprAssistError::input("smoothing_mm must not exceed 20"));
    }
    if options.method != CprPathMethod::FAST_MARCHING && options.method != CprPathMethod::DIJKSTRA {
        return Err(CprAssistError::input(format!(
            "unknown path method {}",
            options.method.0
        )));
    }
    if (options.speed_volume.is_some() || options.vessel_guided)
        && options.method != CprPathMethod::FAST_MARCHING
    {
        return Err(CprAssistError::input(
            "a speed volume requires fast marching",
        ));
    }
    if options.vessel_guided && options.speed_volume.is_some() {
        return Err(CprAssistError::input(
            "vessel guidance and an explicit speed volume are mutually exclusive",
        ));
    }
    let mut o = options.clone();
    if o.grid_spacing_mm == 0.0 {
        o.grid_spacing_mm = if o.method == CprPathMethod::FAST_MARCHING {
            DEFAULT_FAST_MARCHING_GRID_MM
        } else {
            DEFAULT_GRID_SPACING_MM
        };
    }
    if o.speed_exponent == 0.0 {
        o.speed_exponent = DEFAULT_SPEED_EXPONENT;
    }
    if o.centring_exponent == 0.0 {
        o.centring_exponent = DEFAULT_CENTRING_EXPONENT;
    }
    if o.margin_mm == 0.0 {
        o.margin_mm = DEFAULT_MARGIN_MM;
    }
    if o.output_spacing_mm == 0.0 {
        o.output_spacing_mm = DEFAULT_OUTPUT_SPACING_MM;
    }
    if o.intensity_weight == 0.0 {
        o.intensity_weight = DEFAULT_INTENSITY_WEIGHT;
    }
    for (name, value) in [
        ("max_grid_voxels", o.max_grid_voxels),
        ("max_expanded_nodes", o.max_expanded_nodes),
        ("max_queue_entries", o.max_queue_entries),
        ("max_output_points", o.max_output_points),
    ] {
        if value < 0 {
            return Err(CprAssistError::input(format!("{name} cannot be negative")));
        }
    }
    if o.max_grid_voxels == 0 {
        o.max_grid_voxels = DEFAULT_MAX_GRID_VOXELS;
    }
    if o.max_expanded_nodes == 0 {
        o.max_expanded_nodes = o.max_grid_voxels;
    }
    if o.max_queue_entries == 0 {
        o.max_queue_entries = DEFAULT_MAX_QUEUE_ENTRIES;
    }
    if o.max_output_points == 0 {
        o.max_output_points = DEFAULT_MAX_OUTPUT_POINTS;
    }
    for (name, value, max) in [
        (
            "grid_voxels",
            o.max_grid_voxels,
            MAX_CPR_ASSISTANT_GRID_VOXELS,
        ),
        (
            "expanded_nodes",
            o.max_expanded_nodes,
            MAX_CPR_ASSISTANT_EXPANDED_NODES,
        ),
        (
            "queue_entries",
            o.max_queue_entries,
            MAX_CPR_ASSISTANT_QUEUE_ENTRIES,
        ),
        (
            "output_points",
            o.max_output_points,
            MAX_CPR_ASSISTANT_OUTPUT_POINTS,
        ),
    ] {
        if value > max {
            return Err(CprAssistError::limit_error(name, value, max));
        }
    }
    Ok(o)
}
pub(crate) struct AssistGrid<'a> {
    volume: &'a Arc<Volume>,
    pub(crate) min: Vec3,
    pub(crate) step: Vec3,
    pub(crate) nx: i64,
    pub(crate) ny: i64,
    pub(crate) nz: i64,
    pub(crate) values: Vec<f64>,
    pub(crate) valid: Vec<bool>,
    value_min: f64,
    value_max: f64,
}
fn volume_spacing(volume: &Volume) -> Vec3 {
    let fix = |v: f64| if positive_finite(v) { v } else { 1.0 };
    Vec3::new(
        fix(volume.col_spacing),
        fix(volume.row_spacing),
        fix(volume.slice_spacing),
    )
}
pub(crate) fn new_assist_grid<'a>(
    ctx: Ctx<'_>,
    volume: &'a Arc<Volume>,
    start: Vec3,
    end: Vec3,
    options: &CprPathAssistantOptions,
) -> Result<AssistGrid<'a>, CprAssistError> {
    let start_voxel = volume.patient_to_voxel(start);
    let end_voxel = volume.patient_to_voxel(end);
    let spacing = volume_spacing(volume);
    let mut grid_spacing = options.grid_spacing_mm;
    let overflow = || {
        CprAssistError::limit_error(
            "grid_voxels",
            options.max_grid_voxels + 1,
            options.max_grid_voxels,
        )
    };
    for _ in 0..8 {
        ctx_check(ctx)?;
        let margin = Vec3::new(
            options.margin_mm / spacing.x,
            options.margin_mm / spacing.y,
            options.margin_mm / spacing.z,
        );
        let min_voxel = Vec3::new(
            fmath::max(0.0, fmath::min(start_voxel.x, end_voxel.x) - margin.x),
            fmath::max(0.0, fmath::min(start_voxel.y, end_voxel.y) - margin.y),
            fmath::max(0.0, fmath::min(start_voxel.z, end_voxel.z) - margin.z),
        );
        let max_voxel = Vec3::new(
            fmath::min(
                (volume.cols - 1) as f64,
                fmath::max(start_voxel.x, end_voxel.x) + margin.x,
            ),
            fmath::min(
                (volume.rows - 1) as f64,
                fmath::max(start_voxel.y, end_voxel.y) + margin.y,
            ),
            fmath::min(
                (volume.depth - 1) as f64,
                fmath::max(start_voxel.z, end_voxel.z) + margin.z,
            ),
        );
        let step = Vec3::new(
            fmath::max(1.0, grid_spacing / spacing.x),
            fmath::max(1.0, grid_spacing / spacing.y),
            fmath::max(1.0, grid_spacing / spacing.z),
        );
        let count =
            |hi: f64, lo: f64, st: f64| fmath::f64_to_i64(((hi - lo) / st).ceil()).wrapping_add(1);
        let nx = count(max_voxel.x, min_voxel.x, step.x);
        let ny = count(max_voxel.y, min_voxel.y, step.y);
        let nz = count(max_voxel.z, min_voxel.z, step.z);
        let (count64, ok) = bounded_cpr_assistant_product(nx, ny, nz, options.max_grid_voxels);
        if ok {
            let mut grid = AssistGrid {
                volume,
                min: min_voxel,
                step,
                nx,
                ny,
                nz,
                values: vec![0.0; count64 as usize],
                valid: vec![false; count64 as usize],
                value_min: f64::INFINITY,
                value_max: f64::NEG_INFINITY,
            };
            grid.sample_values(ctx)?;
            return Ok(grid);
        }
        if nx <= 0 || ny <= 0 || nz <= 0 {
            break;
        }
        let estimate = nx as f64 * ny as f64 * nz as f64;
        if estimate.is_infinite() || estimate.is_nan() {
            return Err(overflow());
        }
        let mut ratio = fmath::cbrt(estimate / options.max_grid_voxels as f64);
        if ratio < 1.15 {
            ratio = 1.15;
        }
        grid_spacing *= ratio * 1.02;
    }
    Err(overflow())
}
impl AssistGrid<'_> {
    fn sample_values(&mut self, ctx: Ctx<'_>) -> Result<(), CprAssistError> {
        let sampler = Sampler::new(self.volume).map_err(|_| CprAssistError::CprBare)?;
        for index in 0..self.values.len() {
            if index & 1023 == 0 {
                ctx_check(ctx)?;
            }
            let voxel = self.voxel_at(index as i64);
            let patient = self.volume.voxel_to_patient(voxel);
            let Some(value) = sampler.trilinear_at(sampler.vol.patient_to_voxel(patient)) else {
                continue;
            };
            if value.is_nan() || value.is_infinite() {
                continue;
            }
            self.values[index] = value;
            self.valid[index] = true;
            self.value_min = fmath::min(self.value_min, value);
            self.value_max = fmath::max(self.value_max, value);
        }
        Ok(())
    }
    fn index(&self, x: i64, y: i64, z: i64) -> i64 {
        (z * self.ny + y) * self.nx + x
    }
    pub(crate) fn xyz(&self, index: i64) -> (i64, i64, i64) {
        let x = index % self.nx;
        let rest = index / self.nx;
        (x, rest % self.ny, rest / self.ny)
    }
    pub(crate) fn voxel_at(&self, index: i64) -> Vec3 {
        let (x, y, z) = self.xyz(index);
        Vec3::new(
            fmath::min(
                (self.volume.cols - 1) as f64,
                self.min.x + x as f64 * self.step.x,
            ),
            fmath::min(
                (self.volume.rows - 1) as f64,
                self.min.y + y as f64 * self.step.y,
            ),
            fmath::min(
                (self.volume.depth - 1) as f64,
                self.min.z + z as f64 * self.step.z,
            ),
        )
    }
    pub(crate) fn nearest_index(&self, voxel: Vec3) -> i64 {
        let x = clamp_int(
            nearest_index((voxel.x - self.min.x) / self.step.x),
            0,
            self.nx - 1,
        );
        let y = clamp_int(
            nearest_index((voxel.y - self.min.y) / self.step.y),
            0,
            self.ny - 1,
        );
        let z = clamp_int(
            nearest_index((voxel.z - self.min.z) / self.step.z),
            0,
            self.nz - 1,
        );
        self.index(x, y, z)
    }
    fn physical_spacing(&self) -> Vec3 {
        let s = volume_spacing(self.volume);
        Vec3::new(self.step.x * s.x, self.step.y * s.y, self.step.z * s.z)
    }
    fn continuous_voxel(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            fmath::min(
                (self.volume.cols - 1) as f64,
                self.min.x + p.x * self.step.x,
            ),
            fmath::min(
                (self.volume.rows - 1) as f64,
                self.min.y + p.y * self.step.y,
            ),
            fmath::min(
                (self.volume.depth - 1) as f64,
                self.min.z + p.z * self.step.z,
            ),
        )
    }
    fn fast_marching_speed(
        &self,
        ctx: Ctx<'_>,
        start_index: usize,
        end_index: usize,
        spacing: Vec3,
        options: &CprPathAssistantOptions,
    ) -> Result<Vec<f64>, CprAssistError> {
        let mut speed = vec![0.0; self.values.len()];
        if let Some(sv) = options.speed_volume.as_ref() {
            self.sample_speed_volume(ctx, sv, &mut speed, options)?;
            return Ok(speed);
        }
        let seed_value = (self.values[start_index] + self.values[end_index]) / 2.0;
        let mut value_range = self.value_max - self.value_min;
        if !positive_finite(value_range) {
            value_range = 1.0;
        }
        let mut outside = vec![false; speed.len()];
        let mut any_outside = false;
        for (index, value) in self.values.iter().enumerate() {
            if !self.valid[index] {
                outside[index] = true;
                any_outside = true;
                continue;
            }
            let deviation = (value - seed_value) / value_range;
            let penalty = 1.0 + options.intensity_weight * deviation * deviation;
            speed[index] = 1.0 / penalty;
            if penalty > 2.0 {
                outside[index] = true;
                any_outside = true;
            }
        }
        if options.no_centring || !any_outside {
            return Ok(speed);
        }
        let distance =
            squared_distance_to_features(ctx, &outside, self.nx, self.ny, self.nz, spacing)?;
        let h = fmath::min(spacing.x, fmath::min(spacing.y, spacing.z));
        for (index, s) in speed.iter_mut().enumerate() {
            if *s > 0.0 {
                *s *= fmath::pow(distance[index].sqrt() + h, options.centring_exponent);
            }
        }
        Ok(speed)
    }
    fn sample_speed_volume(
        &self,
        ctx: Ctx<'_>,
        speed_volume: &Arc<Volume>,
        speed: &mut [f64],
        options: &CprPathAssistantOptions,
    ) -> Result<(), CprAssistError> {
        let sampler = Sampler::new(speed_volume)
            .map_err(|_| CprAssistError::input("speed volume cannot be sampled"))?;
        let mut max_value = 0.0;
        for index in 0..speed.len() {
            if index & 1023 == 0 {
                ctx_check(ctx)?;
            }
            if !self.valid[index] {
                continue;
            }
            let patient = self.volume.voxel_to_patient(self.voxel_at(index as i64));
            match sampler.trilinear_at(speed_volume.patient_to_voxel(patient)) {
                Some(value) if value > 0.0 && !value.is_infinite() => {
                    speed[index] = value;
                    max_value = fmath::max(max_value, value);
                }
                _ => speed[index] = -1.0,
            }
        }
        if !(max_value > 0.0) {
            return Err(CprAssistError::NotFound(Some(
                "the speed volume has no positive response between the waypoints".to_string(),
            )));
        }
        for s in speed.iter_mut() {
            if *s > 0.0 {
                *s = fmath::pow(*s / max_value, options.speed_exponent) + SPEED_FLOOR;
            } else if *s < 0.0 {
                *s = SPEED_FLOOR;
            }
        }
        Ok(())
    }
}
fn find_fast_marching_segment(
    ctx: Ctx<'_>,
    volume: &Arc<Volume>,
    start: Vec3,
    end: Vec3,
    options: &CprPathAssistantOptions,
) -> Result<Vec<Vec3>, CprAssistError> {
    let grid = new_assist_grid(ctx, volume, start, end, options)?;
    let start_index = grid.nearest_index(volume.patient_to_voxel(start));
    let end_index = grid.nearest_index(volume.patient_to_voxel(end));
    if !grid.valid[start_index as usize] || !grid.valid[end_index as usize] {
        return Err(CprAssistError::NotFound(None));
    }
    if start_index == end_index {
        return Ok(vec![start, end]);
    }
    let spacing = grid.physical_spacing();
    let speed = grid.fast_marching_speed(
        ctx,
        start_index as usize,
        end_index as usize,
        spacing,
        options,
    )?;
    let (sx, sy, sz) = grid.xyz(start_index);
    let (ex, ey, ez) = grid.xyz(end_index);
    let arrival = fast_marching(
        ctx,
        &FastMarchingGrid {
            nx: grid.nx,
            ny: grid.ny,
            nz: grid.nz,
            spacing,
            speed,
        },
        &[FastMarchingSeed {
            index: [sx, sy, sz],
            value: 0.0,
        }],
        &FastMarchingOptions {
            targets: vec![[ex, ey, ez]],
            max_frozen: options.max_expanded_nodes,
            max_trial: options.max_queue_entries,
            ..Default::default()
        },
    );
    let arrival = match arrival {
        Ok(a) => a,
        Err(e) => {
            if let Some((resource, limit)) = e.limit() {
                let resource = if resource == "trial" {
                    "queue_entries"
                } else {
                    "expanded_nodes"
                };
                return Err(CprAssistError::limit_error(resource, limit + 1, limit));
            }
            return Err(CprAssistError::FastMarching(e));
        }
    };
    if !arrival.targets_reached {
        return Err(CprAssistError::NotFound(None));
    }
    let end_voxel = volume.patient_to_voxel(end);
    let clamp = |v: f64, n: i64| fmath::max(0.0, fmath::min((n - 1) as f64, v));
    let from = Vec3::new(
        clamp((end_voxel.x - grid.min.x) / grid.step.x, grid.nx),
        clamp((end_voxel.y - grid.min.y) / grid.step.y, grid.ny),
        clamp((end_voxel.z - grid.min.z) / grid.step.z, grid.nz),
    );
    let step = fmath::min(spacing.x, fmath::min(spacing.y, spacing.z)) / 2.0;
    let route = arrival
        .minimal_path(ctx, from, step)
        .map_err(CprAssistError::FastMarching)?;
    let Some(last) = route.last() else {
        return Err(CprAssistError::NotFound(None));
    };
    if *last != Vec3::new(sx as f64, sy as f64, sz as f64) {
        return Err(CprAssistError::NotFound(None));
    }
    let mut path = Vec::with_capacity(route.len() + 1);
    path.push(start);
    for i in (1..route.len().saturating_sub(1)).rev() {
        path.push(volume.voxel_to_patient(grid.continuous_voxel(route[i])));
    }
    path.push(end);
    Ok(path)
}
pub(crate) fn squared_distance_to_features(
    ctx: Ctx<'_>,
    features: &[bool],
    nx: i64,
    ny: i64,
    nz: i64,
    spacing: Vec3,
) -> Result<Vec<f64>, CprAssistError> {
    let mut out: Vec<f64> = features
        .iter()
        .map(|f| if *f { 0.0 } else { f64::INFINITY })
        .collect();
    let longest = nx.max(ny.max(nz)) as usize;
    let mut f = vec![0.0; longest];
    let mut d = vec![0.0; longest];
    let mut v = vec![0usize; longest];
    let mut z = vec![0.0; longest + 1];
    let mut pass = |out: &mut Vec<f64>,
                    n: i64,
                    lines: i64,
                    step: f64,
                    at: &dyn Fn(i64, i64) -> i64|
     -> Result<(), CprAssistError> {
        let n = n as usize;
        for line in 0..lines {
            if line & 255 == 0 {
                ctx_check(ctx)?;
            }
            for i in 0..n {
                f[i] = out[at(line, i as i64) as usize];
            }
            lower_envelope(&f[..n], &mut d[..n], &mut v, &mut z, step * step);
            for i in 0..n {
                out[at(line, i as i64) as usize] = d[i];
            }
        }
        Ok(())
    };
    pass(&mut out, nx, ny * nz, spacing.x, &|line, i| line * nx + i)?;
    pass(&mut out, ny, nx * nz, spacing.y, &|line, i| {
        (line / nx * ny + i) * nx + line % nx
    })?;
    pass(&mut out, nz, nx * ny, spacing.z, &|line, i| {
        i * nx * ny + line
    })?;
    Ok(out)
}
#[allow(clippy::needless_range_loop)]
fn lower_envelope(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64], w: f64) {
    let n = f.len();
    let mut k: i64 = -1;
    for q in 0..n {
        if f[q] == f64::INFINITY {
            continue;
        }
        while k >= 0 {
            let ku = k as usize;
            let p = v[ku];
            let s = ((f[q] + w * (q * q) as f64) - (f[p] + w * (p * p) as f64))
                / (2.0 * w * (q as f64 - p as f64));
            if s > z[ku] {
                k += 1;
                let ku = k as usize;
                v[ku] = q;
                z[ku] = s;
                z[ku + 1] = f64::INFINITY;
                break;
            }
            k -= 1;
        }
        if k < 0 {
            k = 0;
            v[0] = q;
            z[0] = f64::NEG_INFINITY;
            z[1] = f64::INFINITY;
        }
    }
    if k < 0 {
        d.iter_mut().for_each(|x| *x = f64::INFINITY);
        return;
    }
    let mut j = 0usize;
    for q in 0..n {
        while z[j + 1] < q as f64 {
            j += 1;
        }
        let p = v[j];
        let dq = q as i64 - p as i64;
        d[q] = w * (dq * dq) as f64 + f[p];
    }
}
#[derive(Clone, Copy)]
struct QueueItem {
    index: i64,
    cost: f64,
}
struct CostHeap {
    items: Vec<QueueItem>,
}
impl CostHeap {
    fn less(&self, i: usize, j: usize) -> bool {
        self.items[i].cost < self.items[j].cost
    }
    fn up(&mut self, mut j: usize) {
        while j > 0 {
            let i = (j - 1) / 2;
            if !self.less(j, i) {
                break;
            }
            self.items.swap(i, j);
            j = i;
        }
    }
    fn down(&mut self, i0: usize, n: usize) {
        let mut i = i0;
        loop {
            let j1 = 2 * i + 1;
            if j1 >= n {
                break;
            }
            let mut j = j1;
            let j2 = j1 + 1;
            if j2 < n && self.less(j2, j1) {
                j = j2;
            }
            if !self.less(j, i) {
                break;
            }
            self.items.swap(i, j);
            i = j;
        }
    }
    fn push(&mut self, item: QueueItem) {
        self.items.push(item);
        let last = self.items.len() - 1;
        self.up(last);
    }
    fn pop(&mut self) -> Option<QueueItem> {
        let n = self.items.len().checked_sub(1)?;
        self.items.swap(0, n);
        self.down(0, n);
        self.items.pop()
    }
}
fn find_dijkstra_segment(
    ctx: Ctx<'_>,
    volume: &Arc<Volume>,
    start: Vec3,
    end: Vec3,
    options: &CprPathAssistantOptions,
) -> Result<Vec<Vec3>, CprAssistError> {
    let grid = new_assist_grid(ctx, volume, start, end, options)?;
    let start_index = grid.nearest_index(volume.patient_to_voxel(start));
    let end_index = grid.nearest_index(volume.patient_to_voxel(end));
    if !grid.valid[start_index as usize] || !grid.valid[end_index as usize] {
        return Err(CprAssistError::NotFound(None));
    }
    if start_index == end_index {
        return Ok(vec![start, end]);
    }
    let seed_value = (grid.values[start_index as usize] + grid.values[end_index as usize]) / 2.0;
    let mut value_range = grid.value_max - grid.value_min;
    if !positive_finite(value_range) {
        value_range = 1.0;
    }
    let penalty_at = |index: usize| {
        let deviation = (grid.values[index] - seed_value).abs() / value_range;
        1.0 + options.intensity_weight * deviation * deviation
    };
    let count = grid.values.len();
    let mut distances = vec![f64::INFINITY; count];
    let mut previous = vec![-1i32; count];
    let mut visited = vec![false; count];
    distances[start_index as usize] = 0.0;
    let mut queue = CostHeap {
        items: vec![QueueItem {
            index: start_index,
            cost: 0.0,
        }],
    };
    let mut pushes: i64 = 1;
    let mut expanded: i64 = 0;
    let mut pops: i64 = 0;
    while !queue.items.is_empty() {
        if pops & 1023 == 0 {
            ctx_check(ctx)?;
        }
        pops += 1;
        let Some(current) = queue.pop() else {
            break;
        };
        let ci = current.index as usize;
        if visited[ci] || current.cost != distances[ci] {
            continue;
        }
        expanded += 1;
        if expanded > options.max_expanded_nodes {
            return Err(CprAssistError::limit_error(
                "expanded_nodes",
                expanded,
                options.max_expanded_nodes,
            ));
        }
        visited[ci] = true;
        if current.index == end_index {
            break;
        }
        let (x, y, z) = grid.xyz(current.index);
        let current_voxel = grid.voxel_at(current.index);
        let current_penalty = penalty_at(ci);
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                    if nx < 0 || ny < 0 || nz < 0 || nx >= grid.nx || ny >= grid.ny || nz >= grid.nz
                    {
                        continue;
                    }
                    let next = grid.index(nx, ny, nz);
                    let ni = next as usize;
                    if visited[ni] || !grid.valid[ni] {
                        continue;
                    }
                    let edge_length = volume
                        .voxel_to_patient(grid.voxel_at(next))
                        .sub(volume.voxel_to_patient(current_voxel))
                        .length();
                    let candidate =
                        current.cost + edge_length * (current_penalty + penalty_at(ni)) / 2.0;
                    if candidate < distances[ni] {
                        distances[ni] = candidate;
                        previous[ni] = current.index as i32;
                        pushes += 1;
                        if pushes > options.max_queue_entries {
                            return Err(CprAssistError::limit_error(
                                "queue_entries",
                                pushes,
                                options.max_queue_entries,
                            ));
                        }
                        queue.push(QueueItem {
                            index: next,
                            cost: candidate,
                        });
                    }
                }
            }
        }
    }
    if !visited[end_index as usize] {
        return Err(CprAssistError::NotFound(None));
    }
    let mut indices: Vec<i64> = Vec::with_capacity(64);
    let mut index = end_index;
    while index >= 0 {
        indices.push(index);
        if index == start_index {
            break;
        }
        index = i64::from(previous[index as usize]);
    }
    if indices.last() != Some(&start_index) {
        return Err(CprAssistError::NotFound(None));
    }
    let mut path = Vec::with_capacity(indices.len() + 2);
    path.push(start);
    for i in (1..indices.len().saturating_sub(1)).rev() {
        path.push(volume.voxel_to_patient(grid.voxel_at(indices[i])));
    }
    path.push(end);
    Ok(path)
}
fn smooth_segment(
    ctx: Ctx<'_>,
    points: &[Vec3],
    options: &CprPathAssistantOptions,
) -> Result<Vec<Vec3>, CprAssistError> {
    let sigma = options.smoothing_mm;
    let mut step = fmath::min(options.grid_spacing_mm, options.output_spacing_mm) / 2.0;
    step = fmath::max(step, sigma / (MAX_SMOOTHING_PASSES as f64 / 2.0).sqrt());
    let mut dense = resample_segment(ctx, points, step, MAX_CPR_ASSISTANT_OUTPUT_POINTS)?;
    if dense.len() < 3 {
        return Ok(dense);
    }
    let passes = clamp_int(
        fmath::f64_to_i64((2.0 * (sigma / step) * (sigma / step)).ceil()),
        1,
        MAX_SMOOTHING_PASSES,
    );
    let n = dense.len();
    let mut next = vec![Vec3::ZERO; n];
    next[0] = dense[0];
    next[n - 1] = dense[n - 1];
    for _ in 0..passes {
        ctx_check(ctx)?;
        for i in 1..n - 1 {
            next[i] = dense[i - 1]
                .add(dense[i].scale(2.0))
                .add(dense[i + 1])
                .scale(0.25);
        }
        std::mem::swap(&mut dense, &mut next);
    }
    Ok(dense)
}
fn resample_segment(
    ctx: Ctx<'_>,
    points: &[Vec3],
    spacing: f64,
    limit: i64,
) -> Result<Vec<Vec3>, CprAssistError> {
    let path = Polyline::new(points).map_err(CprAssistError::Cpr)?;
    let count_float = (path.length() / spacing).floor() + 2.0;
    if !positive_finite(count_float) {
        return Err(CprAssistError::input("output sample count is not finite"));
    }
    if limit < 2 || count_float > limit as f64 {
        return Err(CprAssistError::limit_error(
            "output_points",
            limit.wrapping_add(1),
            limit.max(0),
        ));
    }
    let count = fmath::f64_to_i64((path.length() / spacing).floor());
    let mut result = Vec::with_capacity(count as usize + 2);
    for i in 0..=count {
        if i & 1023 == 0 {
            ctx_check(ctx)?;
        }
        result.push(path.point_at(i as f64 * spacing));
    }
    let end = path.point_at(path.length());
    let n = result.len();
    if result[n - 1].sub(end).length() > 1e-9 {
        result.push(end);
    } else {
        result[n - 1] = end;
    }
    Ok(result)
}
struct Polyline {
    points: Vec<Vec3>,
    cum: Vec<f64>,
}
impl Polyline {
    fn new(points: &[Vec3]) -> Result<Polyline, CprError> {
        let mut pts: Vec<Vec3> = Vec::with_capacity(points.len());
        for (index, p) in points.iter().enumerate() {
            if !finite_vec3(*p) {
                return Err(CprError::input(
                    format!("route[{index}]"),
                    "coordinate is not finite",
                ));
            }
            if pts.last() == Some(p) {
                continue;
            }
            pts.push(*p);
        }
        if pts.len() < 2 {
            return Err(CprError::input(
                "route",
                "at least two distinct points are required",
            ));
        }
        let mut cum = vec![0.0; pts.len()];
        for i in 1..pts.len() {
            cum[i] = cum[i - 1] + pts[i].sub(pts[i - 1]).length();
            if !positive_finite(cum[i]) || cum[i] > MAX_CPR_PATH_LENGTH_MM as f64 {
                return Err(CprError::input(
                    "route_length",
                    "length is not finite or exceeds the CPR path limit",
                ));
            }
        }
        Ok(Polyline { points: pts, cum })
    }
    fn length(&self) -> f64 {
        self.cum[self.cum.len() - 1]
    }
    fn point_at(&self, s: f64) -> Vec3 {
        if s <= 0.0 || s.is_nan() {
            return self.points[0];
        }
        let last = self.points.len() - 1;
        if s >= self.cum[last] {
            return self.points[last];
        }
        let i = self.cum.partition_point(|c| *c < s);
        if self.cum[i] == s {
            return self.points[i];
        }
        let lo = i - 1;
        let frac = (s - self.cum[lo]) / (self.cum[i] - self.cum[lo]);
        self.points[lo].add(self.points[i].sub(self.points[lo]).scale(frac))
    }
}
pub(crate) fn voxel_inside_volume(volume: &Volume, point: Vec3) -> bool {
    point.x >= 0.0
        && point.y >= 0.0
        && point.z >= 0.0
        && point.x <= (volume.cols - 1) as f64
        && point.y <= (volume.rows - 1) as f64
        && point.z <= (volume.depth - 1) as f64
}
pub(crate) fn bounded_cpr_assistant_product(nx: i64, ny: i64, nz: i64, limit: i64) -> (i64, bool) {
    if nx <= 0 || ny <= 0 || nz <= 0 || limit <= 0 {
        return (0, false);
    }
    let mut product: i64 = 1;
    for value in [nx, ny, nz] {
        if value > limit / product {
            return (0, false);
        }
        product *= value;
    }
    (product, product <= limit)
}
pub(crate) fn finite_vec3(point: Vec3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}
pub(crate) fn positive_finite(value: f64) -> bool {
    value > 0.0 && !value.is_nan() && !value.is_infinite()
}
pub(crate) fn clamp_int(value: i64, min_value: i64, max_value: i64) -> i64 {
    if value < min_value {
        return min_value;
    }
    if value > max_value {
        return max_value;
    }
    value
}
pub const MAX_CPR_VESSEL_VOXELS: i64 = 8_000_000;
const CPR_VESSEL_WORKING_BYTES: i64 = 64 << 20;
pub const CPR_VESSEL_MIN_SIGMA_MM: f64 = 0.75;
pub const CPR_VESSEL_MAX_SIGMA_MM: f64 = 4.0;
fn prepare_cpr_vessel_speed(
    ctx: Ctx<'_>,
    volume: &Arc<Volume>,
    waypoints: &[Vec3],
    options: &CprPathAssistantOptions,
) -> Result<Arc<Volume>, CprAssistError> {
    let lease = volume
        .acquire_store_snapshot_context(ctx)
        .map_err(CprAssistError::Context)?;
    let result = vessel_speed_with_lease(ctx, volume, waypoints, options);
    lease.release();
    result
}
fn vessel_speed_with_lease(
    ctx: Ctx<'_>,
    volume: &Arc<Volume>,
    waypoints: &[Vec3],
    options: &CprPathAssistantOptions,
) -> Result<Arc<Volume>, CprAssistError> {
    let source = volume.regular_grid().map_err(CprAssistError::Context)?;
    let sampler = Sampler::direct(Arc::clone(&source)).map_err(CprAssistError::Context)?;
    let crop = plan_cpr_vessel_crop(&source, waypoints, options.margin_mm)?;
    let (low, high, dims) = (crop.low, crop.high, crop.dims);
    let (spacing, native) = (crop.spacing, crop.native_spacing);
    let mut input = crate::imagefilter::Volume::new(dims, spacing);
    let context = |ctx: Ctx<'_>| match ctx_err(ctx) {
        Some(e) => Err(CprAssistError::Context(e)),
        None => Ok(()),
    };
    for z in 0..dims[2] {
        context(ctx)?;
        for y in 0..dims[1] {
            context(ctx)?;
            for x in 0..dims[0] {
                let p = Vec3::new(
                    fmath::min(
                        high[0] as f64,
                        low[0] as f64 + x as f64 * spacing[0] / native[0],
                    ),
                    fmath::min(
                        high[1] as f64,
                        low[1] as f64 + y as f64 * spacing[1] / native[1],
                    ),
                    fmath::min(
                        high[2] as f64,
                        low[2] as f64 + z as f64 * spacing[2] / native[2],
                    ),
                );
                match sampler.trilinear_at(p) {
                    Some(v) if v.is_finite() => {
                        input.data[((z * dims[1] + y) * dims[0] + x) as usize] = v as f32;
                    }
                    _ => return Err(CprAssistError::Input(None)),
                }
            }
        }
    }
    let response = crate::imagefilter::vesselness(
        ctx,
        &input,
        &crate::imagefilter::VesselnessOptions {
            sigma_min_mm: CPR_VESSEL_MIN_SIGMA_MM,
            sigma_max_mm: CPR_VESSEL_MAX_SIGMA_MM,
            sigma_steps: 6,
            gamma: -1.0,
            max_working_bytes: CPR_VESSEL_WORKING_BYTES,
            ..crate::imagefilter::VesselnessOptions::default()
        },
    )
    .map_err(|e| CprAssistError::Context(super::vr_vessel_enhance::filter_error(e)))?;
    context(ctx)?;
    let origin = source.voxel_to_patient(Vec3::new(low[0] as f64, low[1] as f64, low[2] as f64));
    Ok(Arc::new(Volume::derived_f32(
        &source,
        dims[0],
        dims[1],
        dims[2],
        origin,
        spacing[0],
        spacing[1],
        spacing[2],
        response.response.data,
    )))
}
struct CprVesselCrop {
    low: [i64; 3],
    high: [i64; 3],
    dims: [i64; 3],
    spacing: [f64; 3],
    native_spacing: [f64; 3],
}
fn plan_cpr_vessel_crop(
    source: &Volume,
    waypoints: &[Vec3],
    search_margin_mm: f64,
) -> Result<CprVesselCrop, CprAssistError> {
    let native = [source.col_spacing, source.row_spacing, source.slice_spacing];
    let mut spacing = native;
    let dimensions = [source.cols, source.rows, source.depth];
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for patient in waypoints {
        let v = source.patient_to_voxel(*patient);
        for (axis, value) in [v.x, v.y, v.z].into_iter().enumerate() {
            minimum[axis] = fmath::min(minimum[axis], value);
            maximum[axis] = fmath::max(maximum[axis], value);
        }
    }
    let margin = search_margin_mm + 4.0 * CPR_VESSEL_MAX_SIGMA_MM;
    let (mut low, mut high, mut dims) = ([0i64; 3], [0i64; 3], [0i64; 3]);
    for axis in 0..3 {
        low[axis] = fmath::f64_to_i64(fmath::max(
            0.0,
            (minimum[axis] - margin / native[axis]).floor(),
        ));
        high[axis] = fmath::f64_to_i64(fmath::min(
            (dimensions[axis] - 1) as f64,
            (maximum[axis] + margin / native[axis]).ceil(),
        ));
        let span_mm = (high[axis] - low[axis]) as f64 * native[axis];
        dims[axis] =
            fmath::f64_to_i64((span_mm / fmath::max(native[axis], CPR_VESSEL_MIN_SIGMA_MM)).ceil())
                + 1;
        if dims[axis] > 1 {
            spacing[axis] = span_mm / (dims[axis] - 1) as f64;
        }
    }
    let (_, ok) = bounded_cpr_assistant_product(dims[0], dims[1], dims[2], MAX_CPR_VESSEL_VOXELS);
    if !ok {
        return Err(CprAssistError::Limit {
            resource: "vessel_voxels".into(),
            value: MAX_CPR_VESSEL_VOXELS + 1,
            limit: MAX_CPR_VESSEL_VOXELS,
        });
    }
    Ok(CprVesselCrop {
        low,
        high,
        dims,
        spacing,
        native_spacing: native,
    })
}
