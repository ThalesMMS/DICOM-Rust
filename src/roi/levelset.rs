#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
pub(crate) struct LevelSetStencil<'a> {
    pub(crate) phi: &'a [f32],
    pub(crate) size: [usize; 3],
    pub(crate) stride: [usize; 3],
    pub(crate) h: [f64; 3],
    pub(crate) dims: i64,
}
impl LevelSetStencil<'_> {
    fn coords(&self, i: usize) -> [usize; 3] {
        let (nx, ny) = (self.size[0], self.size[1]);
        [i % nx, (i / nx) % ny, i / (nx * ny)]
    }
    fn neighbour(&self, i: usize, pos: [usize; 3], axis: usize, dir: i64) -> usize {
        let c = pos[axis] as i64 + dir;
        if c < 0 || c >= self.size[axis] as i64 {
            return i;
        }
        (i as i64 + dir * self.stride[axis] as i64) as usize
    }
    fn mixed(&self, i: usize, pos: [usize; 3], a: usize, b: usize, interior: bool) -> f64 {
        let at = |da: i64, db: i64| -> f64 {
            if interior {
                let index = i as i64 + da * self.stride[a] as i64 + db * self.stride[b] as i64;
                return f64::from(self.phi[index as usize]);
            }
            let j = self.neighbour(i, pos, a, da);
            let mut at = pos;
            if j != i {
                at[a] = (at[a] as i64 + da) as usize;
            }
            f64::from(self.phi[self.neighbour(j, at, b, db)])
        };
        (at(1, 1) - at(1, -1) - at(-1, 1) + at(-1, -1)) / (4.0 * self.h[a] * self.h[b])
    }
    pub(crate) fn curvature_rate(&self, i: usize) -> f32 {
        let pos = self.coords(i);
        let phi = self.phi;
        let c = f64::from(phi[i]);
        let mut interior = true;
        for axis in 0..3 {
            if !interior {
                break;
            }
            interior = self.size[axis] == 1 || (pos[axis] > 0 && pos[axis] < self.size[axis] - 1);
        }
        let (mut minus, mut plus) = ([0usize; 3], [0usize; 3]);
        for axis in 0..3 {
            if self.size[axis] == 1 {
                minus[axis] = i;
                plus[axis] = i;
            } else if interior {
                minus[axis] = i - self.stride[axis];
                plus[axis] = i + self.stride[axis];
            } else {
                minus[axis] = self.neighbour(i, pos, axis, -1);
                plus[axis] = self.neighbour(i, pos, axis, 1);
            }
        }
        let (mut d1, mut d2) = ([0.0f64; 3], [0.0f64; 3]);
        for axis in 0..3 {
            let h = self.h[axis];
            let (a, b) = (f64::from(phi[minus[axis]]), f64::from(phi[plus[axis]]));
            d1[axis] = (b - a) / (2.0 * h);
            d2[axis] = (b - 2.0 * c + a) / (h * h);
        }
        let mut rate = 0.0f64;
        let mut weight = 1.0f64;
        let (gx, gy, gz) = (d1[0], d1[1], d1[2]);
        let mut num = d2[0] * (gy * gy + gz * gz) + d2[1] * (gx * gx + gz * gz);
        let [nx, ny, nz] = self.size;
        if nx > 1 && ny > 1 {
            num -= 2.0 * gx * gy * self.mixed(i, pos, 0, 1, interior);
        }
        if nz > 1 {
            num += d2[2] * (gx * gx + gy * gy);
            if nx > 1 {
                num -= 2.0 * gx * gz * self.mixed(i, pos, 0, 2, interior);
            }
            if ny > 1 {
                num -= 2.0 * gy * gz * self.mixed(i, pos, 1, 2, interior);
            }
        }
        weight /= (self.dims - 1).max(1) as f64;
        let norm2 = gx * gx + gy * gy + gz * gz;
        if norm2 > 1e-12 {
            rate += weight * num / norm2;
        }
        rate as f32
    }
}
pub(crate) fn finite_non_negative(v: f64) -> bool {
    v >= 0.0 && !v.is_infinite() && !v.is_nan()
}
use super::edge_levelset::{
    ERR_CANNY_DOMAIN, EdgeLevelSetStopping, LevelSetStopReason, level_set_mask_runs,
    quantile32_context, sort_level_set_band,
};
use super::region_growing::RegionGrowSeed;
use super::roimask::{MaskRun, RasterMask};
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::imagefilter::{self, RecursiveGaussianOptions, isotropic_sigma};
use crate::render::fast_marching::{
    FastMarchingGrid, FastMarchingOptions, FastMarchingSeed, fast_marching,
    fast_marching_fixed_buffers,
};
use crate::render::vec3::Vec3;
use crate::textfmt::float_v;
use std::collections::BTreeMap;
pub const ERR_LEVEL_SET_INVALID_INPUT: &str = "roi: invalid level set input";
pub const ERR_LEVEL_SET_EMPTY: &str = "roi: level set has no initial region";
pub const ERR_LEVEL_SET_RESOURCE_LIMIT: &str = "roi: level set resource limit exceeded";
pub const ERR_LEVEL_SET_CANCELLED: &str = "roi: level set cancelled";
pub const ERR_SHAPE_INTERFACE_DOMAIN: &str =
    "roi: InterfaceLocal Shape requires a finite dense domain";
const DEFAULT_MAX_VOXELS: i64 = 16 << 20;
const DEFAULT_MAX_BYTES: i64 = 1 << 30;
const DEFAULT_MARGIN_MM: f64 = 30.0;
const DEFAULT_MAX_ITERATIONS: i64 = 1000;
const DEFAULT_RMS_CHANGE: f64 = 0.005;
const BYTES_PER_VOXEL: i64 = 64;
const BAND_VOXELS: f64 = 3.0;
const ARRIVAL_SLACK: f64 = 1.3;
const CREST_QUANTILE: f64 = 0.75;
const EDGE_QUANTILE: f64 = 0.5;
const WINDOW: i64 = 10;
const WINDOW_FLIPS: f64 = 0.002;
const SMALLEST_F32: f64 = 1.401298464324817e-45;
#[derive(Clone, Copy)]
pub struct LevelSetVolume<'a> {
    pub columns: i64,
    pub rows: i64,
    pub slices: i64,
    pub spacing_mm: [f64; 3],
    pub value_at: Option<&'a dyn Fn(i64, i64, i64) -> Option<f64>>,
}
#[derive(Debug, Clone, Default)]
pub struct LevelSetInitialization {
    pub seeds: Vec<RegionGrowSeed>,
    pub initial_radius_mm: f64,
    pub masks: BTreeMap<i64, Option<RasterMask>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LevelSetLimits {
    pub max_voxels: i64,
    pub max_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LevelSetProgress {
    pub iteration: i64,
    pub max_iterations: i64,
    pub rms_change: f64,
}
#[derive(Clone, Copy, Default)]
pub struct LevelSetOptions<'a> {
    pub margin_mm: f64,
    pub limits: LevelSetLimits,
    pub progress: Option<&'a dyn Fn(LevelSetProgress)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EdgePotentialParams {
    pub sigma_mm: f64,
    pub alpha: f64,
    pub beta: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FastMarchingSegmentParams {
    pub edge: EdgePotentialParams,
    pub stopping_time: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GeodesicActiveContourParams {
    pub edge: EdgePotentialParams,
    pub propagation_scaling: f64,
    pub curvature_scaling: f64,
    pub advection_scaling: f64,
    pub max_iterations: i64,
    pub maximum_rms_change: f64,
}
pub fn default_geodesic_active_contour_params() -> GeodesicActiveContourParams {
    GeodesicActiveContourParams {
        propagation_scaling: 1.0,
        curvature_scaling: 1.0,
        advection_scaling: 1.0,
        ..Default::default()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ThresholdLevelSetParams {
    pub lower: f64,
    pub upper: f64,
    pub propagation_scaling: f64,
    pub curvature_scaling: f64,
    pub max_iterations: i64,
    pub maximum_rms_change: f64,
}
pub fn default_threshold_level_set_params(lower: f64, upper: f64) -> ThresholdLevelSetParams {
    ThresholdLevelSetParams {
        lower,
        upper,
        propagation_scaling: 1.0,
        curvature_scaling: 0.2,
        ..Default::default()
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LevelSetResult {
    pub masks: BTreeMap<i64, RasterMask>,
    pub voxel_count: i64,
    pub volume_mm3: f64,
    pub iterations: i64,
    pub converged: bool,
    pub rms_change: f64,
    pub region_origin: [i64; 3],
    pub region_size: [i64; 3],
    pub touches_region_boundary: bool,
    pub stop_reason: LevelSetStopReason,
    pub touches_volume_boundary: bool,
    pub alpha: f64,
    pub beta: f64,
}
pub(crate) fn invalid(detail: impl std::fmt::Display) -> Error {
    Error::wrap(ERR_LEVEL_SET_INVALID_INPUT, detail)
}
impl LevelSetLimits {
    pub(crate) fn normalize(self) -> Result<LevelSetLimits, Error> {
        if self.max_voxels < 0 || self.max_bytes < 0 {
            return Err(invalid("negative limit"));
        }
        let mut l = self;
        if l.max_voxels == 0 {
            l.max_voxels = DEFAULT_MAX_VOXELS;
        }
        if l.max_bytes == 0 {
            l.max_bytes = DEFAULT_MAX_BYTES;
        }
        Ok(l)
    }
}
pub(crate) fn level_set_context_error(ctx: Option<&dyn Cancellation>) -> Option<Error> {
    ctx_err(ctx).map(|e| Error::wrap_err(ERR_LEVEL_SET_CANCELLED, &e))
}
pub(crate) fn check(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match level_set_context_error(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
pub(crate) fn min32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == b {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}
fn sq(x: f64) -> f64 {
    x * x
}
pub(crate) fn abs32(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}
pub fn fast_marching_segment(
    ctx: Option<&dyn Cancellation>,
    volume: LevelSetVolume,
    seeds: &[RegionGrowSeed],
    params: FastMarchingSegmentParams,
    options: LevelSetOptions,
) -> Result<LevelSetResult, Error> {
    if !(params.stopping_time > 0.0) || params.stopping_time.is_infinite() {
        return Err(invalid(format!(
            "stopping time {}",
            float_v(params.stopping_time)
        )));
    }
    let init = LevelSetInitialization {
        seeds: seeds.to_vec(),
        ..Default::default()
    };
    let stop = params.stopping_time;
    let g = LevelSetGrid::new(ctx, volume, &init, &|h| stop + 2.0 * h, options)?;
    if g.seeds.is_empty() {
        return Err(Error::sentinel(ERR_LEVEL_SET_EMPTY));
    }
    let (edge, alpha, beta) = g.edge_potential(params.edge, &init, CREST_QUANTILE)?;
    let times = g.fast_marching_times(&edge, stop)?;
    let Some(mut result) = g.result(|i| times[i] <= stop) else {
        return Err(level_set_context_error(g.ctx).unwrap_or_else(|| invalid("result")));
    };
    result.alpha = alpha;
    result.beta = beta;
    Ok(result)
}
pub fn geodesic_active_contour(
    ctx: Option<&dyn Cancellation>,
    volume: LevelSetVolume,
    init: &LevelSetInitialization,
    params: GeodesicActiveContourParams,
    options: LevelSetOptions,
) -> Result<LevelSetResult, Error> {
    if !finite_non_negative(params.curvature_scaling)
        || !finite_non_negative(params.advection_scaling)
        || params.propagation_scaling.is_nan()
        || params.propagation_scaling.is_infinite()
    {
        return Err(invalid("scaling"));
    }
    let (iterations, tolerance) =
        level_set_stopping(params.max_iterations, params.maximum_rms_change)?;
    let margin = options.margin_mm;
    let g = LevelSetGrid::new(ctx, volume, init, &|_| margin_or_default(margin), options)?;
    let (edge, alpha, beta) = g.edge_potential(params.edge, init, EDGE_QUANTILE)?;
    let phi = g.initial_level_set(init, &edge)?;
    let terms = LevelSetTerms {
        propagation: params.propagation_scaling as f32,
        curvature: params.curvature_scaling as f32,
        advection: params.advection_scaling as f32,
        speed: Some(&edge),
        curvature_weight: Some(&edge),
        advection_field: Some(&edge),
        ..Default::default()
    };
    let mut result = g.evolve(phi, &terms, iterations, tolerance, options.progress)?;
    result.alpha = alpha;
    result.beta = beta;
    Ok(result)
}
pub fn threshold_level_set(
    ctx: Option<&dyn Cancellation>,
    volume: LevelSetVolume,
    init: &LevelSetInitialization,
    params: ThresholdLevelSetParams,
    options: LevelSetOptions,
) -> Result<LevelSetResult, Error> {
    let (lower, upper) = (params.lower, params.upper);
    if lower.is_nan()
        || upper.is_nan()
        || lower > upper
        || lower.is_infinite()
        || upper.is_infinite()
    {
        return Err(invalid(format!(
            "interval [{}, {}]",
            float_v(lower),
            float_v(upper)
        )));
    }
    if !finite_non_negative(params.curvature_scaling)
        || params.propagation_scaling.is_nan()
        || params.propagation_scaling.is_infinite()
    {
        return Err(invalid("scaling"));
    }
    let (iterations, tolerance) =
        level_set_stopping(params.max_iterations, params.maximum_rms_change)?;
    let margin = options.margin_mm;
    let g = LevelSetGrid::new(ctx, volume, init, &|_| margin_or_default(margin), options)?;
    let mid = (lower + upper) / 2.0;
    let mut half = (upper - lower) / 2.0;
    if half <= 0.0 {
        half = SMALLEST_F32;
    }
    let speed: Vec<f32> = g
        .image
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            if g.available(i) {
                fmath::max(
                    -1.0,
                    fmath::min(1.0, 1.0 - (f64::from(v) - mid).abs() / half),
                ) as f32
            } else {
                -1.0
            }
        })
        .collect();
    let phi = g.initial_level_set(init, &speed)?;
    let terms = LevelSetTerms {
        propagation: params.propagation_scaling as f32,
        curvature: params.curvature_scaling as f32,
        speed: Some(&speed),
        ..Default::default()
    };
    g.evolve(phi, &terms, iterations, tolerance, options.progress)
}
pub(crate) fn level_set_stopping(iterations: i64, tolerance: f64) -> Result<(i64, f64), Error> {
    if iterations < 0 || !finite_non_negative(tolerance) {
        return Err(invalid("stopping criteria"));
    }
    let iterations = if iterations == 0 {
        DEFAULT_MAX_ITERATIONS
    } else {
        iterations
    };
    let tolerance = if tolerance == 0.0 {
        DEFAULT_RMS_CHANGE
    } else {
        tolerance
    };
    Ok((iterations, tolerance))
}
pub(crate) fn margin_or_default(margin: f64) -> f64 {
    if margin == 0.0 {
        DEFAULT_MARGIN_MM
    } else {
        margin
    }
}
pub(crate) struct LevelSetGrid<'a> {
    pub(crate) ctx: Option<&'a dyn Cancellation>,
    pub(crate) volume: LevelSetVolume<'a>,
    pub(crate) nx: usize,
    pub(crate) ny: usize,
    pub(crate) nz: usize,
    pub(crate) n: usize,
    pub(crate) size: [usize; 3],
    pub(crate) stride: [usize; 3],
    pub(crate) bounded: bool,
    pub(crate) dense: bool,
    pub(crate) interface_local: bool,
    pub(crate) workers: i64,
    pub(crate) origin: [i64; 3],
    pub(crate) h: [f64; 3],
    pub(crate) hmin: f64,
    pub(crate) hmax: f64,
    pub(crate) dims: i64,
    pub(crate) image: Vec<f32>,
    pub(crate) unavailable: Option<Vec<bool>>,
    pub(crate) seeds: Vec<[i64; 3]>,
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GridProfile {
    pub(crate) bounded: bool,
    pub(crate) dense: bool,
    pub(crate) interface_local: bool,
    pub(crate) workers: i64,
}
pub(crate) type Reserve<'r> = &'r mut dyn FnMut(&LevelSetGrid) -> Result<GridProfile, Error>;
impl<'a> LevelSetGrid<'a> {
    pub(crate) fn available(&self, i: usize) -> bool {
        self.unavailable.as_ref().is_none_or(|u| !u[i])
    }
    pub(crate) fn index(&self, x: usize, y: usize, z: usize) -> usize {
        (z * self.ny + y) * self.nx + x
    }
    pub(crate) fn coords(&self, i: usize) -> (usize, usize, usize) {
        (
            i % self.nx,
            (i / self.nx) % self.ny,
            i / (self.nx * self.ny),
        )
    }
    pub(crate) fn new(
        ctx: Option<&'a dyn Cancellation>,
        volume: LevelSetVolume<'a>,
        init: &LevelSetInitialization,
        margin: &dyn Fn(f64) -> f64,
        options: LevelSetOptions,
    ) -> Result<LevelSetGrid<'a>, Error> {
        Self::new_reserved(ctx, volume, init, margin, options, None)
    }
    pub(crate) fn new_reserved(
        ctx: Option<&'a dyn Cancellation>,
        volume: LevelSetVolume<'a>,
        init: &LevelSetInitialization,
        margin: &dyn Fn(f64) -> f64,
        options: LevelSetOptions,
        reserve: Option<Reserve>,
    ) -> Result<LevelSetGrid<'a>, Error> {
        check(ctx)?;
        let value_at = match volume.value_at {
            Some(f) if volume.columns > 0 && volume.rows > 0 && volume.slices > 0 => f,
            _ => {
                return Err(invalid(format!(
                    "volume {}x{}x{}",
                    volume.columns, volume.rows, volume.slices
                )));
            }
        };
        let limits = options.limits.normalize()?;
        if !finite_non_negative(options.margin_mm) || !finite_non_negative(init.initial_radius_mm) {
            return Err(invalid("negative or non-finite margin or radius"));
        }
        let mut h = volume.spacing_mm;
        let sizes = [volume.columns, volume.rows, volume.slices];
        let (mut hmin, mut hmax) = (f64::INFINITY, 0.0);
        let mut dims = 0;
        for axis in 0..3 {
            if sizes[axis] == 1 && axis == 2 {
                h[axis] = 1.0;
                continue;
            }
            let s = h[axis];
            if !(s > 0.0) || s.is_infinite() {
                return Err(invalid(format!(
                    "spacing {} mm along axis {axis}",
                    float_v(s)
                )));
            }
            dims += 1;
            hmin = fmath::min(hmin, s);
            hmax = fmath::max(hmax, s);
        }
        let mut lo = sizes;
        let mut hi = [-1i64; 3];
        let mut extend = |p: [i64; 3]| {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
        };
        for (k, seed) in init.seeds.iter().enumerate() {
            if k % 4096 == 0 {
                check(ctx)?;
            }
            if seed.x < 0
                || seed.y < 0
                || seed.slice < 0
                || seed.x >= sizes[0]
                || seed.y >= sizes[1]
                || seed.slice >= sizes[2]
            {
                return Err(invalid(format!(
                    "seed ({}, {}, {}) outside the volume",
                    seed.x, seed.y, seed.slice
                )));
            }
            extend([seed.x, seed.y, seed.slice]);
        }
        for (&slice, mask) in &init.masks {
            let Some(mask) = mask else {
                continue;
            };
            if slice < 0 || slice >= sizes[2] || mask.columns != sizes[0] || mask.rows != sizes[1] {
                return Err(invalid(format!("initial mask of slice {slice}")));
            }
            level_set_mask_runs(ctx, Some(mask), |y, run: MaskRun| {
                extend([run.start, y, slice]);
                extend([run.end - 1, y, slice]);
                Ok(())
            })?;
        }
        if hi[0] < 0 {
            return Err(Error::sentinel(ERR_LEVEL_SET_EMPTY));
        }
        let m = margin(hmax);
        if !finite_non_negative(m) {
            return Err(invalid(format!("margin {}", float_v(m))));
        }
        let mut size = [0i64; 3];
        for axis in 0..3 {
            let mut pad = 0;
            if sizes[axis] > 1 {
                pad = fmath::f64_to_i64(fmath::min((m / h[axis]).ceil(), sizes[axis] as f64));
            }
            lo[axis] = 0.max(lo[axis] - pad);
            hi[axis] = (sizes[axis] - 1).min(hi[axis] + pad);
            size[axis] = hi[axis] - lo[axis] + 1;
        }
        if size[0] > limits.max_voxels / size[1] || size[0] * size[1] > limits.max_voxels / size[2]
        {
            return Err(Error::wrap(
                ERR_LEVEL_SET_RESOURCE_LIMIT,
                format!(
                    "a {}x{}x{} sub-volume exceeds {} voxels",
                    size[0], size[1], size[2], limits.max_voxels
                ),
            ));
        }
        let n = size[0] * size[1] * size[2];
        if n > limits.max_bytes / BYTES_PER_VOXEL {
            return Err(Error::wrap(
                ERR_LEVEL_SET_RESOURCE_LIMIT,
                format!(
                    "a {}x{}x{} sub-volume needs about {} MiB",
                    size[0],
                    size[1],
                    size[2],
                    (n * BYTES_PER_VOXEL) >> 20
                ),
            ));
        }
        let us = [size[0] as usize, size[1] as usize, size[2] as usize];
        let mut g = LevelSetGrid {
            ctx,
            volume,
            nx: us[0],
            ny: us[1],
            nz: us[2],
            n: n as usize,
            size: us,
            stride: [1, us[0], us[0] * us[1]],
            bounded: false,
            dense: false,
            interface_local: false,
            workers: 0,
            origin: lo,
            h,
            hmin,
            hmax,
            dims,
            image: Vec::new(),
            unavailable: None,
            seeds: Vec::new(),
        };
        if let Some(reserve) = reserve {
            let p = reserve(&g)?;
            g.bounded = p.bounded;
            g.dense = p.dense;
            g.interface_local = p.interface_local;
            g.workers = p.workers;
        }
        g.seeds = init
            .seeds
            .iter()
            .map(|s| [s.x - lo[0], s.y - lo[1], s.slice - lo[2]])
            .collect();
        g.image = vec![0.0; g.n];
        for z in 0..g.nz {
            check(ctx)?;
            for y in 0..g.ny {
                for x in 0..g.nx {
                    if (y * g.nx + x).is_multiple_of(4096) {
                        check(ctx)?;
                    }
                    let got = value_at(x as i64 + lo[0], y as i64 + lo[1], z as i64 + lo[2]);
                    if g.interface_local {
                        check(ctx)?;
                    }
                    let (mut v, ok) = match got {
                        Some(v) => (v, true),
                        None => (0.0, false),
                    };
                    if g.bounded
                        && ok
                        && (v.is_nan() || v.is_infinite() || v.abs() > f64::from(f32::MAX))
                    {
                        if g.interface_local {
                            return Err(Error::wrap(
                                ERR_SHAPE_INTERFACE_DOMAIN,
                                "nonfinite/overflow sample",
                            ));
                        }
                        return Err(invalid("non-finite/overflow sample"));
                    }
                    if g.dense && !ok {
                        check(ctx)?;
                        if g.interface_local {
                            return Err(Error::sentinel(ERR_SHAPE_INTERFACE_DOMAIN));
                        }
                        return Err(Error::sentinel(ERR_CANNY_DOMAIN));
                    }
                    let i = g.index(x, y, z);
                    if !ok || v.is_nan() || v.is_infinite() {
                        let n = g.n;
                        g.unavailable.get_or_insert_with(|| vec![false; n])[i] = true;
                        v = 0.0;
                    }
                    g.image[i] = v as f32;
                }
            }
        }
        Ok(g)
    }
    pub(crate) fn default_initial_radius(&self) -> f64 {
        fmath::max(3.0 * self.hmin, 2.0 * self.hmax)
    }
    pub(crate) fn mask_voxels(
        &self,
        init: &LevelSetInitialization,
        mut f: impl FnMut(usize),
    ) -> Result<(), Error> {
        for (&slice, mask) in &init.masks {
            let z = slice - self.origin[2];
            let Some(mask) = mask else {
                continue;
            };
            if z < 0 || z >= self.nz as i64 {
                continue;
            }
            level_set_mask_runs(self.ctx, Some(mask), |y, run| {
                for x in run.start..run.end {
                    if (x - run.start) % 4096 == 0 {
                        check(self.ctx)?;
                    }
                    f(self.index(
                        (x - self.origin[0]) as usize,
                        (y - self.origin[1]) as usize,
                        z as usize,
                    ));
                }
                Ok(())
            })?;
        }
        Ok(())
    }
    pub(crate) fn initial_level_set(
        &self,
        init: &LevelSetInitialization,
        speed: &[f32],
    ) -> Result<Vec<f32>, Error> {
        let far = (4.0 * BAND_VOXELS * self.hmax) as f32;
        let mut phi = vec![0.0f32; self.n];
        for (i, p) in phi.iter_mut().enumerate() {
            if i % 4096 == 0 {
                check(self.ctx)?;
            }
            *p = far;
        }
        let mut empty = true;
        if !init.masks.is_empty() {
            let half = (self.hmin / 2.0) as f32;
            for (i, p) in phi.iter_mut().enumerate() {
                if i % 4096 == 0 {
                    check(self.ctx)?;
                }
                *p = half;
            }
            self.mask_voxels(init, |i| {
                if self.available(i) {
                    phi[i] = -half;
                    empty = false;
                }
            })?;
        }
        if !self.seeds.is_empty() {
            let mut radius = init.initial_radius_mm;
            if radius == 0.0 {
                radius = self.default_initial_radius();
            }
            let times = self.fast_marching_times(speed, ARRIVAL_SLACK * radius + f64::from(far))?;
            for (i, &t) in times.iter().enumerate() {
                if i % 4096 == 0 {
                    check(self.ctx)?;
                }
                if t == f64::INFINITY {
                    continue;
                }
                let (x, y, z) = self.coords(i);
                let mut d = f64::INFINITY;
                for seed in &self.seeds {
                    let dx = (x as i64 - seed[0]) as f64 * self.h[0];
                    let dy = (y as i64 - seed[1]) as f64 * self.h[1];
                    let dz = (z as i64 - seed[2]) as f64 * self.h[2];
                    d = fmath::min(d, dx * dx + dy * dy + dz * dz);
                }
                let v = fmath::min(
                    fmath::max(d.sqrt() - radius, t - ARRIVAL_SLACK * radius),
                    f64::from(far),
                ) as f32;
                phi[i] = min32(phi[i], v);
                if v < 0.0 {
                    empty = false;
                }
            }
            for seed in &self.seeds {
                let i = self.index(seed[0] as usize, seed[1] as usize, seed[2] as usize);
                if self.available(i) && phi[i] >= 0.0 {
                    phi[i] = -((self.hmin / 2.0) as f32);
                    empty = false;
                }
            }
        }
        if empty {
            return Err(Error::sentinel(ERR_LEVEL_SET_EMPTY));
        }
        Ok(phi)
    }
    pub(crate) fn fast_marching_times(&self, speed: &[f32], stop: f64) -> Result<Vec<f64>, Error> {
        let mut grid = FastMarchingGrid {
            nx: self.nx as i64,
            ny: self.ny as i64,
            nz: self.nz as i64,
            spacing: Vec3::new(self.h[0], self.h[1], self.h[2]),
            speed: vec![0.0; self.n],
        };
        for (i, &s) in speed.iter().enumerate() {
            if i % 4096 == 0 {
                check(self.ctx)?;
            }
            if self.available(i) && s > 0.0 {
                grid.speed[i] = f64::from(s);
            }
        }
        let mut seeds: Vec<FastMarchingSeed> = Vec::new();
        for seed in &self.seeds {
            let i = self.index(seed[0] as usize, seed[1] as usize, seed[2] as usize);
            if !self.available(i) {
                continue;
            }
            if grid.speed[i] <= 0.0 {
                grid.speed[i] = SMALLEST_F32;
            }
            seeds.push(FastMarchingSeed {
                index: *seed,
                value: 0.0,
            });
        }
        if seeds.is_empty() {
            return Err(Error::sentinel(ERR_LEVEL_SET_EMPTY));
        }
        let options = FastMarchingOptions {
            stopping_value: stop,
            ..Default::default()
        };
        let result = if self.interface_local {
            fast_marching_fixed_buffers(self.ctx, &grid, &seeds, &options)
        } else {
            fast_marching(self.ctx, &grid, &seeds, &options)
        };
        let mut result = match result {
            Ok(r) => r,
            Err(e) => {
                check(self.ctx)?;
                return Err(invalid(format!("fast marching: {e}")));
            }
        };
        for i in 0..result.times.len() {
            if i % 4096 == 0 {
                check(self.ctx)?;
            }
            if !self.available(i) {
                result.times[i] = f64::INFINITY;
            }
        }
        Ok(result.times)
    }
    pub(crate) fn edge_potential(
        &self,
        params: EdgePotentialParams,
        init: &LevelSetInitialization,
        edge_quantile: f64,
    ) -> Result<(Vec<f32>, f64, f64), Error> {
        if !finite_non_negative(params.sigma_mm)
            || params.alpha.is_nan()
            || params.alpha.is_infinite()
            || params.beta.is_nan()
            || params.beta.is_infinite()
        {
            return Err(invalid("edge potential parameters"));
        }
        let mut sigma = params.sigma_mm;
        if sigma == 0.0 {
            sigma = fmath::max(1.0, self.hmin);
        }
        let src = imagefilter::Volume {
            dims: [self.nx as i64, self.ny as i64, self.nz as i64],
            spacing_mm: self.h,
            data: self.image.clone(),
        };
        let gradient = match imagefilter::gradient_magnitude_recursive_gaussian(
            self.ctx,
            &src,
            RecursiveGaussianOptions {
                sigma_mm: isotropic_sigma(sigma),
                workers: self.workers,
                ..Default::default()
            },
        ) {
            Ok(v) => v,
            Err(e) => {
                check(self.ctx)?;
                return Err(invalid(format!("gradient magnitude: {e}")));
            }
        };
        let mut grad = gradient.data;
        let (mut alpha, mut beta) = (params.alpha, params.beta);
        if alpha == 0.0 {
            (alpha, beta) = self.estimate_sigmoid(&grad, init, edge_quantile)?;
        }
        for i in 0..grad.len() {
            if i % 4096 == 0 {
                check(self.ctx)?;
            }
            let mut e = 1.0 / (1.0 + fmath::exp(-(f64::from(grad[i]) - beta) / alpha));
            if !self.available(i) {
                e = 0.0;
            }
            if self.bounded && (e.is_nan() || e.is_infinite()) {
                return Err(invalid("non-finite potential"));
            }
            grad[i] = e as f32;
        }
        Ok((grad, alpha, beta))
    }
    pub(crate) fn estimate_sigmoid(
        &self,
        grad: &[f32],
        init: &LevelSetInitialization,
        edge_quantile: f64,
    ) -> Result<(f64, f64), Error> {
        let mut radius = init.initial_radius_mm;
        if radius == 0.0 {
            radius = self.default_initial_radius();
        }
        let mut inside: Vec<f32> = Vec::new();
        let mut border: Vec<f32> = Vec::new();
        let n3 = [self.nx as i64, self.ny as i64, self.nz as i64];
        for seed in &self.seeds {
            let (mut lo, mut hi) = ([0i64; 3], [0i64; 3]);
            for axis in 0..3 {
                let r = fmath::f64_to_i64(radius / self.h[axis]);
                lo[axis] = 0.max(seed[axis].wrapping_sub(r));
                hi[axis] = (n3[axis] - 1).min(seed[axis].wrapping_add(r));
            }
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    check(self.ctx)?;
                    for x in lo[0]..=hi[0] {
                        if x % 4096 == 0 {
                            check(self.ctx)?;
                        }
                        let dx = (x - seed[0]) as f64 * self.h[0];
                        let dy = (y - seed[1]) as f64 * self.h[1];
                        let dz = (z - seed[2]) as f64 * self.h[2];
                        if dx * dx + dy * dy + dz * dz <= radius * radius {
                            let i = self.index(x as usize, y as usize, z as usize);
                            if self.available(i) {
                                inside.push(grad[i]);
                            }
                        }
                    }
                }
            }
        }
        if !init.masks.is_empty() {
            let mut mask = vec![false; self.n];
            self.mask_voxels(init, |i| mask[i] = true)?;
            for (i, &m) in mask.iter().enumerate() {
                if i % 4096 == 0 {
                    check(self.ctx)?;
                }
                if !m || !self.available(i) {
                    continue;
                }
                if self.on_border(&mask, i) {
                    border.push(grad[i]);
                } else {
                    inside.push(grad[i]);
                }
            }
        }
        if inside.is_empty() {
            inside = border.clone();
        }
        let k2 = quantile32_context(self.ctx, &inside, 0.5)?;
        let mut deviations = Vec::with_capacity(inside.len());
        for (i, &v) in inside.iter().enumerate() {
            if i % 4096 == 0 {
                check(self.ctx)?;
            }
            deviations.push((f64::from(v) - k2).abs() as f32);
        }
        let deviation = quantile32_context(self.ctx, &deviations, 0.5)?;
        let spread = fmath::max(1.4826 * deviation, 1e-6 * fmath::max(1.0, k2));
        let significant = k2 + 3.0 * spread;
        let mut k1 = 0.0;
        if !border.is_empty() {
            k1 = quantile32_context(self.ctx, &border, 0.5)?;
        }
        if !(k1 > significant) {
            let stride = 1.max(self.n / (1 << 18));
            let mut edges: Vec<f32> = Vec::new();
            let mut i = 0;
            while i < self.n {
                if i % 4096 == 0 {
                    check(self.ctx)?;
                }
                if self.available(i) && f64::from(grad[i]) > significant {
                    edges.push(grad[i]);
                }
                i += stride;
            }
            k1 = quantile32_context(self.ctx, &edges, edge_quantile)?;
        }
        if !(k1 > significant) {
            k1 = k2 + 6.0 * spread;
        }
        check(self.ctx)?;
        Ok((-(k1 - k2) / 6.0, (k1 + k2) / 2.0))
    }
    fn on_border(&self, mask: &[bool], i: usize) -> bool {
        let (x, y, z) = self.coords(i);
        let (x, y, z) = (x as i64, y as i64, z as i64);
        for nb in [
            [x - 1, y, z],
            [x + 1, y, z],
            [x, y - 1, z],
            [x, y + 1, z],
            [x, y, z - 1],
            [x, y, z + 1],
        ] {
            if nb[0] < 0
                || nb[1] < 0
                || nb[2] < 0
                || nb[0] >= self.nx as i64
                || nb[1] >= self.ny as i64
                || nb[2] >= self.nz as i64
            {
                continue;
            }
            if !mask[self.index(nb[0] as usize, nb[1] as usize, nb[2] as usize)] {
                return true;
            }
        }
        false
    }
    pub(crate) fn result(&self, inside: impl Fn(usize) -> bool) -> Option<LevelSetResult> {
        let mut out = LevelSetResult {
            region_origin: self.origin,
            region_size: [self.nx as i64, self.ny as i64, self.nz as i64],
            ..Default::default()
        };
        let v = &self.volume;
        let sizes = [v.columns, v.rows, v.slices];
        let cut = |axis: usize, c: usize, n: usize| -> bool {
            (c == 0 && self.origin[axis] > 0)
                || (c == n - 1 && self.origin[axis] + (n as i64) < sizes[axis])
        };
        for z in 0..self.nz {
            let mut mask: Option<RasterMask> = None;
            for y in 0..self.ny {
                if level_set_context_error(self.ctx).is_some() {
                    return None;
                }
                let mut start: i64 = -1;
                for x in 0..=self.nx {
                    if x % 4096 == 0 && level_set_context_error(self.ctx).is_some() {
                        return None;
                    }
                    let inn = x < self.nx
                        && inside(self.index(x, y, z))
                        && self.available(self.index(x, y, z));
                    if inn {
                        out.voxel_count += 1;
                        let (gx, gy, gz) = (
                            x as i64 + self.origin[0],
                            y as i64 + self.origin[1],
                            z as i64 + self.origin[2],
                        );
                        if self.bounded
                            && (gx == 0
                                || gx == sizes[0] - 1
                                || gy == 0
                                || gy == sizes[1] - 1
                                || v.slices > 1 && (gz == 0 || gz == sizes[2] - 1))
                        {
                            out.touches_volume_boundary = true;
                        }
                        if cut(0, x, self.nx) || cut(1, y, self.ny) || cut(2, z, self.nz) {
                            out.touches_region_boundary = true;
                        }
                        if start < 0 {
                            start = x as i64;
                        }
                        continue;
                    }
                    if start >= 0 {
                        let m = mask.get_or_insert_with(|| RasterMask::new(v.columns, v.rows));
                        let row = y as i64 + self.origin[1];
                        let (s, e) = (start + self.origin[0], x as i64 + self.origin[0]);
                        if self.bounded {
                            m.runs[row as usize].push(MaskRun { start: s, end: e });
                        } else {
                            m.set_run(row, s, e);
                        }
                        start = -1;
                    }
                }
            }
            if let Some(mask) = mask {
                out.masks.insert(z as i64 + self.origin[2], mask);
            }
        }
        let mut voxel = self.h[0] * self.h[1];
        if v.slices > 1 {
            voxel *= self.h[2];
        }
        out.volume_mm3 = out.voxel_count as f64 * voxel;
        Some(out)
    }
    pub(crate) fn evolve(
        &self,
        phi: Vec<f32>,
        terms: &LevelSetTerms,
        max_iterations: i64,
        tolerance: f64,
        progress: Option<&dyn Fn(LevelSetProgress)>,
    ) -> Result<LevelSetResult, Error> {
        self.evolve_measured(phi, terms, max_iterations, tolerance, progress, None)
    }
    pub(crate) fn evolve_measured(
        &self,
        phi: Vec<f32>,
        terms: &LevelSetTerms,
        max_iterations: i64,
        tolerance: f64,
        progress: Option<&dyn Fn(LevelSetProgress)>,
        mut measure: Option<Measure>,
    ) -> Result<LevelSetResult, Error> {
        let mut s = LevelSetState {
            g: self,
            phi,
            band: Vec::new(),
            width: (BAND_VOXELS * self.hmax) as f32,
            status: vec![0u8; self.n],
            touched: Vec::new(),
            heap: LevelSetHeap::default(),
        };
        if self.bounded {
            s.heap.max_items = self.n * 7;
        }
        s.reinitialize(None)?;
        if terms.shape_prior.is_some()
            && let Some(m) = measure.as_mut()
        {
            m(&s.phi, true)?;
        }
        let mut rates: Vec<f32> = Vec::new();
        let tol = tolerance * self.hmin;
        let mut moved = 0.0;
        let (mut iterations, mut rms, mut converged) = (0i64, 0.0f64, false);
        let mut reason = LevelSetStopReason::ITERATION_LIMIT;
        let mut window = vec![false; self.n];
        let snapshot = |window: &mut [bool], phi: &[f32]| {
            for (i, &v) in phi.iter().enumerate() {
                if i % 4096 == 0 && level_set_context_error(self.ctx).is_some() {
                    return;
                }
                window[i] = v < 0.0;
            }
        };
        snapshot(&mut window, &s.phi);
        let mut quiet = 0;
        while iterations < max_iterations {
            check(self.ctx)?;
            if s.band.is_empty() {
                reason = LevelSetStopReason::EMPTY_BAND;
                converged = true;
                break;
            }
            rates.resize(s.band.len(), 0.0);
            let (first_order, curvature_max) = s.rates(terms, &mut rates);
            check(self.ctx)?;
            if self.bounded
                && (first_order.is_nan()
                    || first_order.is_infinite()
                    || curvature_max.is_nan()
                    || curvature_max.is_infinite())
            {
                return Err(invalid("non-finite rate extremes"));
            }
            let mut dt = level_set_time_step(first_order, curvature_max, self.hmin, self.dims);
            if let Some(prior) = terms.shape_prior {
                dt = 1.0
                    / (first_order / (0.5 * self.hmin)
                        + curvature_max * 2.0 * self.dims as f64 / (self.hmin * self.hmin)
                        + 2.0 * prior.weight);
            }
            if dt == 0.0 {
                reason = LevelSetStopReason::ZERO_TIME_STEP;
                converged = true;
                break;
            }
            let front = self.hmax as f32;
            let (mut sum, mut count, mut largest) = (0.0f64, 0i64, 0.0f64);
            for k in 0..s.band.len() {
                if k % 4096 == 0 {
                    check(self.ctx)?;
                }
                let i = s.band[k] as usize;
                let change = dt * f64::from(rates[k]);
                let p = s.phi[i];
                if p < front && p > -front {
                    sum += change * change;
                    count += 1;
                    largest = fmath::max(largest, change.abs());
                }
                if self.bounded
                    && (change.is_nan()
                        || change.is_infinite()
                        || (change + f64::from(s.phi[i])).abs() > f64::from(f32::MAX))
                {
                    return Err(invalid("non-finite level-set update"));
                }
                s.phi[i] += change as f32;
            }
            iterations += 1;
            rms = 0.0;
            if count > 0 {
                rms = (sum / count as f64).sqrt();
            }
            if let Some(progress) = progress {
                progress(LevelSetProgress {
                    iteration: iterations,
                    max_iterations,
                    rms_change: rms,
                });
            }
            if rms < tol
                && terms.stopping != EdgeLevelSetStopping::MASK_STABILITY_ONLY
                && terms.stopping != EdgeLevelSetStopping::FIXED_ITERATIONS
            {
                reason = LevelSetStopReason::RMS;
                converged = true;
                break;
            }
            if iterations % WINDOW == 0 {
                let mut flips = 0i64;
                for (i, &v) in s.phi.iter().enumerate() {
                    if i % 4096 == 0 {
                        check(self.ctx)?;
                    }
                    if (v < 0.0) != window[i] {
                        flips += 1;
                    }
                }
                if flips as f64 <= WINDOW_FLIPS * count as f64 {
                    quiet += 1;
                } else {
                    quiet = 0;
                }
                if quiet >= 2
                    && terms.stopping != EdgeLevelSetStopping::RMS_ONLY
                    && terms.stopping != EdgeLevelSetStopping::FIXED_ITERATIONS
                {
                    reason = LevelSetStopReason::STABLE_MASK;
                    converged = true;
                    break;
                }
                snapshot(&mut window, &s.phi);
            }
            moved += largest;
            if moved >= 0.5 * self.hmin {
                let band = std::mem::take(&mut s.band);
                s.reinitialize(Some(band))?;
                moved = 0.0;
            }
        }
        if terms.shape_prior.is_some()
            && let Some(m) = measure.as_mut()
        {
            m(&s.phi, false)?;
        }
        let phi = &s.phi;
        let Some(mut result) = self.result(|i| phi[i] < 0.0) else {
            return Err(level_set_context_error(self.ctx).unwrap_or_else(|| invalid("result")));
        };
        result.iterations = iterations;
        result.converged = converged;
        result.rms_change = rms;
        if self.bounded {
            result.stop_reason = reason;
        }
        Ok(result)
    }
}
pub(crate) fn level_set_time_step(first_order: f64, curvature: f64, hmin: f64, dims: i64) -> f64 {
    let rate = first_order / (0.5 * hmin) + curvature * 2.0 * dims as f64 / (hmin * hmin);
    if !(rate > 0.0) {
        return 0.0;
    }
    1.0 / rate
}
#[derive(Default)]
pub(crate) struct LevelSetTerms<'t> {
    pub(crate) stopping: EdgeLevelSetStopping,
    pub(crate) propagation: f32,
    pub(crate) curvature: f32,
    pub(crate) advection: f32,
    pub(crate) speed: Option<&'t [f32]>,
    pub(crate) curvature_weight: Option<&'t [f32]>,
    pub(crate) advection_field: Option<&'t [f32]>,
    pub(crate) shape_prior: Option<ShapePriorTerm<'t>>,
}
#[derive(Clone, Copy)]
pub(crate) struct ShapePriorTerm<'t> {
    pub(crate) target: &'t [f32],
    pub(crate) weight: f64,
}
pub(crate) type Measure<'m> = &'m mut dyn FnMut(&[f32], bool) -> Result<(), Error>;
pub(crate) const LS_FAR: u8 = 0;
pub(crate) const LS_TRIAL: u8 = 1;
const LS_SEED: u8 = 2;
pub(crate) const LS_FROZEN: u8 = 3;
pub(crate) struct LevelSetState<'g, 'a> {
    pub(crate) g: &'g LevelSetGrid<'a>,
    pub(crate) phi: Vec<f32>,
    pub(crate) band: Vec<i32>,
    pub(crate) width: f32,
    pub(crate) status: Vec<u8>,
    pub(crate) touched: Vec<i32>,
    pub(crate) heap: LevelSetHeap,
}
impl LevelSetState<'_, '_> {
    fn rates(&self, terms: &LevelSetTerms, out: &mut [f32]) -> (f64, f64) {
        let (mut first, mut curvature) = (0.0f64, 0.0f64);
        for k in 0..self.band.len() {
            if k % 4096 == 0 && ctx_err(self.g.ctx).is_some() {
                break;
            }
            let (rate, f, c) = self.rate(self.band[k] as usize, terms);
            out[k] = rate;
            first = fmath::max(first, f);
            curvature = fmath::max(curvature, c);
        }
        (first, curvature)
    }
    pub(crate) fn neighbour(&self, i: usize, pos: [usize; 3], axis: usize, dir: i64) -> usize {
        let c = pos[axis] as i64 + dir;
        if c < 0 || c >= self.g.size[axis] as i64 {
            return i;
        }
        let j = (i as i64 + dir * self.g.stride[axis] as i64) as usize;
        if !self.g.available(j) {
            return i;
        }
        j
    }
    fn mixed(
        &self,
        i: usize,
        pos: [usize; 3],
        (a, b): (usize, usize),
        interior: bool,
        allowed: Option<&[u8]>,
    ) -> f64 {
        let g = self.g;
        let value = |index: usize| -> f64 {
            match allowed {
                Some(allowed) => f64::from(self.interface_stencil_value(index, allowed)),
                None => f64::from(self.phi[index]),
            }
        };
        let at = |da: i64, db: i64| -> f64 {
            if interior {
                let index = i as i64 + da * g.stride[a] as i64 + db * g.stride[b] as i64;
                return value(index as usize);
            }
            let j = self.neighbour(i, pos, a, da);
            let mut at = pos;
            if j != i {
                at[a] = (at[a] as i64 + da) as usize;
            }
            value(self.neighbour(j, at, b, db))
        };
        (at(1, 1) - at(1, -1) - at(-1, 1) + at(-1, -1)) / (4.0 * g.h[a] * g.h[b])
    }
    fn rate(&self, i: usize, terms: &LevelSetTerms) -> (f32, f64, f64) {
        self.rate_potential_boundary(i, terms, None, 0.0, None)
    }
    pub(crate) fn rate_potential_boundary(
        &self,
        i: usize,
        terms: &LevelSetTerms,
        sampled: Option<f32>,
        regularization: f64,
        allowed: Option<&[u8]>,
    ) -> (f32, f64, f64) {
        let g = self.g;
        let (x, y, z) = g.coords(i);
        let phi = &self.phi;
        let c = f64::from(phi[i]);
        let pos = [x, y, z];
        let mut interior = g.unavailable.is_none();
        for axis in 0..3 {
            if !interior {
                break;
            }
            interior = g.size[axis] == 1 || (pos[axis] > 0 && pos[axis] < g.size[axis] - 1);
        }
        let (mut minus, mut plus) = ([0usize; 3], [0usize; 3]);
        for axis in 0..3 {
            if g.size[axis] == 1 {
                minus[axis] = i;
                plus[axis] = i;
            } else if interior {
                minus[axis] = i - g.stride[axis];
                plus[axis] = i + g.stride[axis];
            } else {
                minus[axis] = self.neighbour(i, pos, axis, -1);
                plus[axis] = self.neighbour(i, pos, axis, 1);
            }
        }
        let (mut dm, mut dp, mut d1, mut d2) = ([0.0f64; 3], [0.0f64; 3], [0.0f64; 3], [0.0f64; 3]);
        for axis in 0..3 {
            let h = g.h[axis];
            let (a, b) = match allowed {
                Some(allowed) => (
                    f64::from(self.interface_stencil_value(minus[axis], allowed)),
                    f64::from(self.interface_stencil_value(plus[axis], allowed)),
                ),
                None => (f64::from(phi[minus[axis]]), f64::from(phi[plus[axis]])),
            };
            dm[axis] = (c - a) / h;
            dp[axis] = (b - c) / h;
            d1[axis] = (b - a) / (2.0 * h);
            d2[axis] = (b - 2.0 * c + a) / (h * h);
        }
        let mut rate = 0.0f64;
        let mut first = 0.0f64;
        if terms.propagation != 0.0
            && let Some(speed) = terms.speed
        {
            let f = f64::from(terms.propagation) * f64::from(sampled.unwrap_or(speed[i]));
            let mut grad2 = 0.0f64;
            for axis in 0..3 {
                if f > 0.0 {
                    grad2 += sq(fmath::max(dm[axis], 0.0)) + sq(fmath::min(dp[axis], 0.0));
                } else {
                    grad2 += sq(fmath::min(dm[axis], 0.0)) + sq(fmath::max(dp[axis], 0.0));
                }
            }
            rate -= f * grad2.sqrt();
            first += f.abs();
        }
        if terms.advection != 0.0
            && let Some(field) = terms.advection_field
        {
            for axis in 0..3 {
                let v = -f64::from(terms.advection)
                    * f64::from(field[plus[axis]] - field[minus[axis]])
                    / (2.0 * g.h[axis]);
                if v > 0.0 {
                    rate -= v * dm[axis];
                } else {
                    rate -= v * dp[axis];
                }
                first += v.abs() * g.hmin / g.h[axis];
            }
        }
        let mut curvature = 0.0f64;
        if terms.curvature != 0.0 {
            let mut weight = f64::from(terms.curvature);
            if let Some(w) = terms.curvature_weight {
                weight *= f64::from(sampled.unwrap_or(w[i]));
            }
            if weight != 0.0 {
                let (gx, gy, gz) = (d1[0], d1[1], d1[2]);
                let mut num = d2[0] * (gy * gy + gz * gz) + d2[1] * (gx * gx + gz * gz);
                if g.nx > 1 && g.ny > 1 {
                    num -= 2.0 * gx * gy * self.mixed(i, pos, (0, 1), interior, allowed);
                }
                if g.nz > 1 {
                    num += d2[2] * (gx * gx + gy * gy);
                    if g.nx > 1 {
                        num -= 2.0 * gx * gz * self.mixed(i, pos, (0, 2), interior, allowed);
                    }
                    if g.ny > 1 {
                        num -= 2.0 * gy * gz * self.mixed(i, pos, (1, 2), interior, allowed);
                    }
                }
                weight /= (g.dims - 1).max(1) as f64;
                let norm2 = gx * gx + gy * gy + gz * gz;
                if regularization > 0.0 {
                    rate += weight * num / (norm2 + regularization);
                } else if norm2 > 1e-12 {
                    rate += weight * num / norm2;
                }
                curvature = weight.abs();
            }
        }
        if let Some(prior) = terms.shape_prior {
            let reaction = prior.weight * (c - f64::from(prior.target[i]));
            rate -= reaction;
            first += reaction.abs();
        }
        (rate as f32, first, curvature)
    }
    fn reinitialize(&mut self, candidates: Option<Vec<i32>>) -> Result<(), Error> {
        let g = self.g;
        check(g.ctx)?;
        let count = candidates.as_ref().map_or(g.n, |c| c.len());
        let at = |k: usize| -> usize { candidates.as_ref().map_or(k, |c| c[k] as usize) };
        let mut seeds: Vec<(i32, f32)> = Vec::new();
        for k in 0..count {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let i = at(k);
            if !g.available(i) {
                continue;
            }
            if let Some(d) = self.interface_distance(i) {
                seeds.push((i as i32, d));
            }
        }
        for k in 0..count {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            self.reset(at(k));
        }
        self.heap.items.clear();
        self.touched.clear();
        for (k, &(i, d)) in seeds.iter().enumerate() {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let iu = i as usize;
            self.phi[iu] = if self.phi[iu] < 0.0 { -d } else { d };
            if self.status[iu] == LS_FAR {
                self.touched.push(i);
            }
            self.status[iu] = LS_SEED;
            self.heap.push(HeapItem { d, i });
        }
        let mut band: Vec<i32> = Vec::new();
        let mut pops = 0u64;
        while !self.heap.items.is_empty() {
            if pops & 4095 == 0 {
                check(g.ctx)?;
            }
            pops += 1;
            let item = self.heap.pop();
            let i = item.i as usize;
            if self.status[i] == LS_FROZEN || item.d != abs32(self.phi[i]) {
                continue;
            }
            if item.d >= self.width {
                break;
            }
            self.status[i] = LS_FROZEN;
            band.push(item.i);
            let (x, y, z) = g.coords(i);
            let (x, y, z) = (x as i64, y as i64, z as i64);
            for nb in [
                [x - 1, y, z],
                [x + 1, y, z],
                [x, y - 1, z],
                [x, y + 1, z],
                [x, y, z - 1],
                [x, y, z + 1],
            ] {
                if nb[0] < 0
                    || nb[1] < 0
                    || nb[2] < 0
                    || nb[0] >= g.nx as i64
                    || nb[1] >= g.ny as i64
                    || nb[2] >= g.nz as i64
                {
                    continue;
                }
                let (bx, by, bz) = (nb[0] as usize, nb[1] as usize, nb[2] as usize);
                let j = g.index(bx, by, bz);
                let st = self.status[j];
                if st == LS_FROZEN || st == LS_SEED || !g.available(j) {
                    continue;
                }
                let d = self.eikonal(bx, by, bz) as f32;
                if d >= abs32(self.phi[j]) {
                    continue;
                }
                if self.status[j] == LS_FAR {
                    self.touched.push(j as i32);
                }
                self.status[j] = LS_TRIAL;
                self.phi[j] = if self.phi[j] < 0.0 { -d } else { d };
                self.heap.push(HeapItem { d, i: j as i32 });
            }
        }
        if self.heap.overflow {
            return Err(Error::wrap(
                ERR_LEVEL_SET_RESOURCE_LIMIT,
                "reinitialization heap",
            ));
        }
        for k in 0..self.touched.len() {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let i = self.touched[k] as usize;
            if self.status[i] != LS_FROZEN {
                self.reset(i);
            }
            self.status[i] = LS_FAR;
        }
        if g.bounded {
            sort_level_set_band(g.ctx, &mut band)?;
        } else {
            band.sort_unstable();
        }
        self.band = band;
        Ok(())
    }
    fn reset(&mut self, i: usize) {
        self.phi[i] = if self.phi[i] < 0.0 {
            -self.width
        } else {
            self.width
        };
    }
    fn interface_distance(&self, i: usize) -> Option<f32> {
        let g = self.g;
        let (x, y, z) = g.coords(i);
        let p = f64::from(self.phi[i]);
        let inside = p < 0.0;
        let pos = [x, y, z];
        let n = [g.nx, g.ny, g.nz];
        let stride = [1, g.nx, g.nx * g.ny];
        let mut sum = 0.0f64;
        let mut found = false;
        for axis in 0..3 {
            let mut best = f64::INFINITY;
            for delta in [-1i64, 1] {
                let c = pos[axis] as i64 + delta;
                if c < 0 || c >= n[axis] as i64 {
                    continue;
                }
                let j = (i as i64 + delta * stride[axis] as i64) as usize;
                let q = f64::from(self.phi[j]);
                if (q < 0.0) == inside || !g.available(j) {
                    continue;
                }
                let mut d = 0.0;
                let total = p.abs() + q.abs();
                if total > 0.0 {
                    d = g.h[axis] * p.abs() / total;
                }
                best = fmath::min(best, d);
            }
            if best == f64::INFINITY {
                continue;
            }
            found = true;
            if best == 0.0 {
                return Some(0.0);
            }
            sum += 1.0 / (best * best);
        }
        if !found {
            return None;
        }
        Some((1.0 / sum.sqrt()) as f32)
    }
    fn eikonal(&self, x: usize, y: usize, z: usize) -> f64 {
        let g = self.g;
        let (mut a, mut h) = ([0.0f64; 3], [0.0f64; 3]);
        let mut n = 0;
        let pos = [x as i64, y as i64, z as i64];
        let sizes = [g.nx as i64, g.ny as i64, g.nz as i64];
        for axis in 0..3 {
            let mut best = f64::INFINITY;
            for delta in [-1i64, 1] {
                let mut c = pos;
                c[axis] += delta;
                if c[axis] < 0 || c[axis] >= sizes[axis] {
                    continue;
                }
                let j = g.index(c[0] as usize, c[1] as usize, c[2] as usize);
                if self.status[j] == LS_FROZEN {
                    best = fmath::min(best, f64::from(abs32(self.phi[j])));
                }
            }
            if best != f64::INFINITY {
                a[n] = best;
                h[n] = g.h[axis];
                n += 1;
            }
        }
        for i in 1..n {
            let mut j = i;
            while j > 0 && a[j] < a[j - 1] {
                a.swap(j, j - 1);
                h.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut solution = f64::INFINITY;
        let (mut sum_w, mut sum_wa, mut sum_wa2) = (0.0f64, 0.0f64, 0.0f64);
        for k in 0..n {
            if k > 0 && solution <= a[k] {
                break;
            }
            let w = 1.0 / (h[k] * h[k]);
            sum_w += w;
            sum_wa += w * a[k];
            sum_wa2 += w * a[k] * a[k];
            let disc = sum_wa * sum_wa - sum_w * (sum_wa2 - 1.0);
            if disc < 0.0 {
                break;
            }
            solution = (sum_wa + disc.sqrt()) / sum_w;
        }
        solution
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeapItem {
    pub(crate) d: f32,
    pub(crate) i: i32,
}
#[derive(Debug, Default)]
pub(crate) struct LevelSetHeap {
    pub(crate) items: Vec<HeapItem>,
    pub(crate) max_items: usize,
    pub(crate) overflow: bool,
}
impl LevelSetHeap {
    pub(crate) fn push(&mut self, item: HeapItem) {
        if self.max_items > 0 && self.items.len() >= self.max_items {
            self.overflow = true;
            return;
        }
        self.items.push(item);
        let mut k = self.items.len() - 1;
        while k > 0 {
            let parent = (k - 1) / 2;
            if self.items[parent].d <= self.items[k].d {
                break;
            }
            self.items.swap(parent, k);
            k = parent;
        }
    }
    pub(crate) fn pop(&mut self) -> HeapItem {
        let top = self.items[0];
        let last = self.items.len() - 1;
        self.items[0] = self.items[last];
        self.items.truncate(last);
        let mut k = 0;
        loop {
            let (l, r) = (2 * k + 1, 2 * k + 2);
            let mut m = k;
            if l < last && self.items[l].d < self.items[m].d {
                m = l;
            }
            if r < last && self.items[r].d < self.items[m].d {
                m = r;
            }
            if m == k {
                break;
            }
            self.items.swap(k, m);
            k = m;
        }
        top
    }
}
