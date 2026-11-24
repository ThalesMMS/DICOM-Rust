#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::Error;
use super::levelset::level_set_context_error;
use super::roimask::{MaskRun, RasterMask};
use crate::render::parallel::Cancellation;
pub const ERR_CANNY_DOMAIN: &str = "roi: Canny requires a dense finite domain";
pub const ERR_CANNY_NO_EDGES: &str = "roi: Canny found no edges";
pub const ERR_EDGE_LEVEL_SET_BOUNDARY: &str = "roi: edge contour reached image or crop boundary";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeLevelSetStopping(pub u8);
impl EdgeLevelSetStopping {
    pub const RMS_OR_STABLE_MASK: EdgeLevelSetStopping = EdgeLevelSetStopping(0);
    pub const RMS_ONLY: EdgeLevelSetStopping = EdgeLevelSetStopping(1);
    pub const MASK_STABILITY_ONLY: EdgeLevelSetStopping = EdgeLevelSetStopping(2);
    pub const FIXED_ITERATIONS: EdgeLevelSetStopping = EdgeLevelSetStopping(3);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LevelSetStopReason(pub u8);
impl LevelSetStopReason {
    pub const UNSPECIFIED: LevelSetStopReason = LevelSetStopReason(0);
    pub const ITERATION_LIMIT: LevelSetStopReason = LevelSetStopReason(1);
    pub const RMS: LevelSetStopReason = LevelSetStopReason(2);
    pub const STABLE_MASK: LevelSetStopReason = LevelSetStopReason(3);
    pub const EMPTY_BAND: LevelSetStopReason = LevelSetStopReason(4);
    pub const ZERO_TIME_STEP: LevelSetStopReason = LevelSetStopReason(5);
    pub const BOUNDARY_ACTIVE_EXHAUSTED: LevelSetStopReason = LevelSetStopReason(6);
}
fn check(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match level_set_context_error(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
pub(crate) fn quantile32_context(
    ctx: Option<&dyn Cancellation>,
    values: &[f32],
    q: f64,
) -> Result<f64, Error> {
    check(ctx)?;
    if values.is_empty() {
        return Ok(0.0);
    }
    let mut data = Vec::with_capacity(values.len());
    for (i, &v) in values.iter().enumerate() {
        if i % 4096 == 0 {
            check(ctx)?;
        }
        data.push(v);
    }
    let rank = (q * (data.len() - 1) as f64 + 0.5) as i64;
    let (mut lo, mut hi) = (0i64, data.len() as i64 - 1);
    let mut steps = 0u64;
    let step = |steps: &mut u64| -> Result<(), Error> {
        *steps += 1;
        if (*steps).is_multiple_of(4096) {
            check(ctx)?;
        }
        Ok(())
    };
    while lo < hi {
        let pivot = data[(lo + (hi - lo) / 2) as usize];
        let (mut i, mut j) = (lo, hi);
        while i <= j {
            while data[i as usize] < pivot {
                i += 1;
                step(&mut steps)?;
            }
            while data[j as usize] > pivot {
                j -= 1;
                step(&mut steps)?;
            }
            if i <= j {
                data.swap(i as usize, j as usize);
                i += 1;
                j -= 1;
                step(&mut steps)?;
            }
        }
        if rank <= j {
            hi = j;
        } else if rank >= i {
            lo = i;
        } else {
            break;
        }
    }
    let v = f64::from(data[rank as usize]);
    check(ctx)?;
    Ok(v)
}
pub(crate) fn sort_level_set_band(
    ctx: Option<&dyn Cancellation>,
    data: &mut [i32],
) -> Result<(), Error> {
    let mut steps = 0u64;
    let mut down = |data: &mut [i32], mut root: usize, end: usize| -> Result<(), Error> {
        while root * 2 + 1 < end {
            steps += 1;
            if steps.is_multiple_of(4096) {
                check(ctx)?;
            }
            let mut child = root * 2 + 1;
            if child + 1 < end && data[child] < data[child + 1] {
                child += 1;
            }
            if data[root] >= data[child] {
                return Ok(());
            }
            data.swap(root, child);
            root = child;
        }
        Ok(())
    };
    let n = data.len();
    for i in (0..n / 2).rev() {
        down(data, i, n)?;
    }
    for end in (1..n).rev() {
        data.swap(0, end);
        down(data, 0, end)?;
    }
    check(ctx)
}
pub(crate) fn level_set_mask_runs(
    ctx: Option<&dyn Cancellation>,
    m: Option<&RasterMask>,
    mut f: impl FnMut(i64, MaskRun) -> Result<(), Error>,
) -> Result<(), Error> {
    let Some(m) = m else {
        return Ok(());
    };
    let mut visited = 0u64;
    for (y, runs) in m.runs.iter().enumerate() {
        if y % 4096 == 0 {
            check(ctx)?;
        }
        for &r in runs {
            if visited.is_multiple_of(4096) {
                check(ctx)?;
            }
            visited += 1;
            f(y as i64, r)?;
        }
    }
    check(ctx)
}
use super::distance3d::VolumeMask;
use super::levelset::{
    ERR_LEVEL_SET_INVALID_INPUT, ERR_LEVEL_SET_RESOURCE_LIMIT, EdgePotentialParams, GridProfile,
    LevelSetGrid, LevelSetInitialization, LevelSetOptions, LevelSetResult, LevelSetTerms,
    LevelSetVolume, ShapePriorTerm, finite_non_negative, invalid, level_set_stopping,
    margin_or_default,
};
use super::levelset_interface::{
    ShapeEvolutionProfile, ShapeInitializationProfile, ShapeSurfaceSampling,
};
use super::livewire_canny::filter_error;
use crate::fmath;
use crate::imagefilter::{
    self, CannyOptions, CannyParameters, ErrorKind, FilterError, canny_gaussian_scratch_bytes,
    canny_working_bytes, default_canny_parameters,
};
use crate::render::parallel;
const EDGE_MAX_VOXELS: i64 = 16 << 20;
const EDGE_MAX_BYTES: i64 = 1 << 30;
const EDGE_QUANTILE: f64 = 0.5;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeCurvatureConvention(pub u8);
impl EdgeCurvatureConvention {
    pub const MEAN_CURVATURE: EdgeCurvatureConvention = EdgeCurvatureConvention(0);
    pub const SUM_CURVATURE: EdgeCurvatureConvention = EdgeCurvatureConvention(1);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeBoundaryPolicy(pub u8);
impl EdgeBoundaryPolicy {
    pub const REPORT_TOUCHES: EdgeBoundaryPolicy = EdgeBoundaryPolicy(0);
    pub const REJECT_TOUCHES: EdgeBoundaryPolicy = EdgeBoundaryPolicy(1);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShapeDetectionInput(pub u8);
impl ShapeDetectionInput {
    pub const INTENSITY_IMAGE: ShapeDetectionInput = ShapeDetectionInput(0);
    pub const EDGE_POTENTIAL_IMAGE: ShapeDetectionInput = ShapeDetectionInput(1);
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShapeDetectionParams {
    pub evolution: ShapeEvolutionProfile,
    pub surface_sampling: ShapeSurfaceSampling,
    pub initialization: ShapeInitializationProfile,
    pub stopping: EdgeLevelSetStopping,
    pub curvature_convention: EdgeCurvatureConvention,
    pub boundary: EdgeBoundaryPolicy,
    pub input: ShapeDetectionInput,
    pub edge: EdgePotentialParams,
    pub propagation_scaling: f64,
    pub curvature_scaling: f64,
    pub max_iterations: i64,
    pub maximum_rms_change: f64,
}
pub fn default_shape_detection_params() -> ShapeDetectionParams {
    ShapeDetectionParams {
        propagation_scaling: 1.0,
        curvature_scaling: 1.0,
        ..Default::default()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CannyLevelSetParams {
    pub stopping: EdgeLevelSetStopping,
    pub curvature_convention: EdgeCurvatureConvention,
    pub boundary: EdgeBoundaryPolicy,
    pub canny: CannyParameters,
    pub propagation_scaling: f64,
    pub curvature_scaling: f64,
    pub advection_scaling: f64,
    pub max_iterations: i64,
    pub maximum_rms_change: f64,
}
pub fn default_canny_level_set_params() -> CannyLevelSetParams {
    CannyLevelSetParams {
        canny: default_canny_parameters(),
        curvature_scaling: 0.2,
        advection_scaling: 1.0,
        ..Default::default()
    }
}
fn edge_policies(
    stop: EdgeLevelSetStopping,
    curvature: EdgeCurvatureConvention,
    boundary: EdgeBoundaryPolicy,
) -> Result<(), Error> {
    if stop.0 > EdgeLevelSetStopping::FIXED_ITERATIONS.0
        || curvature.0 > EdgeCurvatureConvention::SUM_CURVATURE.0
        || boundary.0 > EdgeBoundaryPolicy::REJECT_TOUCHES.0
    {
        return Err(invalid("edge stopping/curvature/boundary option"));
    }
    Ok(())
}
fn edge_curvature(c: f64, convention: EdgeCurvatureConvention, slices: i64) -> Result<f64, Error> {
    let mut c = c;
    if convention == EdgeCurvatureConvention::SUM_CURVATURE && slices > 1 {
        c *= 2.0;
    }
    if !finite_non_negative(c) || c > f64::from(f32::MAX) {
        return Err(invalid("curvature convention scaling overflow"));
    }
    Ok(c)
}
fn edge_boundary_result(
    r: LevelSetResult,
    policy: EdgeBoundaryPolicy,
) -> Result<LevelSetResult, Error> {
    if policy == EdgeBoundaryPolicy::REJECT_TOUCHES
        && (r.touches_region_boundary
            || r.touches_volume_boundary
            || r.stop_reason == LevelSetStopReason::BOUNDARY_ACTIVE_EXHAUSTED)
    {
        return Err(Error::sentinel(ERR_EDGE_LEVEL_SET_BOUNDARY));
    }
    Ok(r)
}
fn edge_stopping(iterations: i64, rms: f64) -> Result<(i64, f64), Error> {
    if iterations > 10000 {
        return Err(Error::wrap(
            ERR_LEVEL_SET_RESOURCE_LIMIT,
            "at most 10000 iterations",
        ));
    }
    level_set_stopping(iterations, rms)
}
fn edge_scalings(p: f64, c: f64, a: f64) -> Result<(), Error> {
    let max = f64::from(f32::MAX);
    if p.is_nan()
        || p.is_infinite()
        || p.abs() > max
        || !finite_non_negative(c)
        || c > max
        || !finite_non_negative(a)
        || a > max
    {
        return Err(invalid("edge scalings"));
    }
    Ok(())
}
fn shape_profile_options(p: &ShapeDetectionParams) -> Result<(), Error> {
    if p.evolution.0 > ShapeEvolutionProfile::INTERFACE_LOCAL.0
        || p.surface_sampling.0 > ShapeSurfaceSampling::INTERPOLATED_INTERFACE.0
        || p.initialization.0 > ShapeInitializationProfile::BOUNDED_FACE_NORMAL_SEEDS.0
        || p.evolution == ShapeEvolutionProfile::DENSE_BAND
            && (p.surface_sampling != ShapeSurfaceSampling::GRID_CENTRES
                || p.initialization != ShapeInitializationProfile::CITY_BLOCK_SEEDS)
    {
        return Err(invalid("Shape evolution/sampling"));
    }
    if p.evolution == ShapeEvolutionProfile::INTERFACE_LOCAL
        && p.input != ShapeDetectionInput::EDGE_POTENTIAL_IMAGE
    {
        return Err(invalid("InterfaceLocal requires supplied edge potential"));
    }
    Ok(())
}
pub(crate) trait ShapePrior {
    fn prepare(&mut self, g: &LevelSetGrid) -> Result<(), Error>;
    fn target(&self) -> &[f32];
    fn weight(&self) -> f64;
    fn measure(
        &mut self,
        ctx: Option<&dyn Cancellation>,
        phi: &[f32],
        initial: bool,
    ) -> Result<(), Error>;
}
pub fn shape_detection_level_set(
    ctx: Option<&dyn Cancellation>,
    volume: LevelSetVolume,
    init: &LevelSetInitialization,
    p: ShapeDetectionParams,
    o: LevelSetOptions,
) -> Result<LevelSetResult, Error> {
    shape_detection_with_prior(ctx, volume, init, p, o, None)
}
pub(crate) fn shape_detection_with_prior(
    ctx: Option<&dyn Cancellation>,
    volume: LevelSetVolume,
    init: &LevelSetInitialization,
    p: ShapeDetectionParams,
    o: LevelSetOptions,
    mut prior: Option<&mut dyn ShapePrior>,
) -> Result<LevelSetResult, Error> {
    let mut p = p;
    shape_profile_options(&p)?;
    edge_policies(p.stopping, p.curvature_convention, p.boundary)?;
    p.curvature_scaling =
        edge_curvature(p.curvature_scaling, p.curvature_convention, volume.slices)?;
    let e = p.edge;
    if !finite_non_negative(e.sigma_mm)
        || e.alpha.is_nan()
        || e.alpha.is_infinite()
        || e.beta.is_nan()
        || e.beta.is_infinite()
    {
        return Err(invalid("shape Edge parameters"));
    }
    if p.input != ShapeDetectionInput::INTENSITY_IMAGE
        && p.input != ShapeDetectionInput::EDGE_POTENTIAL_IMAGE
    {
        return Err(invalid("shape input mode"));
    }
    if p.input == ShapeDetectionInput::EDGE_POTENTIAL_IMAGE
        && p.edge != EdgePotentialParams::default()
    {
        return Err(invalid("Edge parameters with provided potential"));
    }
    edge_scalings(p.propagation_scaling, p.curvature_scaling, 0.0)?;
    let (it, tol) = edge_stopping(p.max_iterations, p.maximum_rms_change)?;
    let interface = p.evolution == ShapeEvolutionProfile::INTERFACE_LOCAL;
    let g = new_edge_level_set_grid(ctx, volume, init, o, interface, interface)?;
    if prior.is_some() && g.unavailable.is_some() {
        return Err(Error::wrap(
            super::pca_shape::ERR_PCA_SHAPE_INVALID,
            "shape prior requires a finite dense crop",
        ));
    }
    let (edge, alpha, beta);
    if p.input == ShapeDetectionInput::EDGE_POTENTIAL_IMAGE {
        let mut out = vec![0.0f32; g.n];
        for (i, &v) in g.image.iter().enumerate() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            if g.available(i) && !(v >= 0.0 && v <= 1.0) {
                return Err(invalid("potential outside [0,1]"));
            }
            if g.available(i) {
                out[i] = v;
            }
        }
        (edge, alpha, beta) = (out, 0.0, 0.0);
    } else {
        (edge, alpha, beta) = g.edge_potential(p.edge, init, EDGE_QUANTILE)?;
    }
    let phi = g.initial_level_set(init, &edge)?;
    let mut target: Vec<f32> = Vec::new();
    let mut weight = 0.0;
    if let Some(prior) = prior.as_deref_mut() {
        prior.prepare(&g)?;
        target = prior.target().to_vec();
        weight = prior.weight();
    }
    let terms = LevelSetTerms {
        stopping: p.stopping,
        propagation: p.propagation_scaling as f32,
        curvature: p.curvature_scaling as f32,
        speed: Some(&edge),
        curvature_weight: Some(&edge),
        shape_prior: prior.is_some().then_some(ShapePriorTerm {
            target: &target,
            weight,
        }),
        ..Default::default()
    };
    let r = if interface {
        g.evolve_interface(
            phi,
            &terms,
            p.surface_sampling,
            p.initialization,
            it,
            tol,
            o.progress,
        )?
    } else if let Some(pr) = prior {
        let ctx = g.ctx;
        let mut measure =
            |phi: &[f32], initial: bool| -> Result<(), Error> { pr.measure(ctx, phi, initial) };
        g.evolve_measured(phi, &terms, it, tol, o.progress, Some(&mut measure))?
    } else {
        g.evolve(phi, &terms, it, tol, o.progress)?
    };
    check(g.ctx)?;
    let mut r = r;
    r.alpha = alpha;
    r.beta = beta;
    edge_boundary_result(r, p.boundary)
}
pub fn canny_level_set(
    ctx: Option<&dyn Cancellation>,
    volume: LevelSetVolume,
    init: &LevelSetInitialization,
    p: CannyLevelSetParams,
    o: LevelSetOptions,
) -> Result<LevelSetResult, Error> {
    let mut p = p;
    edge_policies(p.stopping, p.curvature_convention, p.boundary)?;
    p.curvature_scaling =
        edge_curvature(p.curvature_scaling, p.curvature_convention, volume.slices)?;
    edge_scalings(
        p.propagation_scaling,
        p.curvature_scaling,
        p.advection_scaling,
    )?;
    let (it, tol) = edge_stopping(p.max_iterations, p.maximum_rms_change)?;
    let dimensions = if volume.slices == 1 { 2 } else { 3 };
    p.canny = match p.canny.normalize(dimensions) {
        Ok(c) => c,
        Err(e) => {
            return Err(Error::wrap_err(
                ERR_LEVEL_SET_INVALID_INPUT,
                &filter_error(e),
            ));
        }
    };
    let g = new_edge_level_set_grid(ctx, volume, init, o, true, false)?;
    let src = imagefilter::Volume {
        dims: [g.nx as i64, g.ny as i64, g.nz as i64],
        spacing_mm: g.h,
        data: g.image.clone(),
    };
    let edges = imagefilter::canny(
        g.ctx,
        &src,
        CannyOptions {
            parameters: p.canny,
            workers: g.workers,
            ..Default::default()
        },
    )
    .map_err(|e| filter_err(&g, e))?;
    if edges.edge_count == 0 {
        return Err(Error::sentinel(ERR_CANNY_NO_EDGES));
    }
    let (distance, field) = canny_distance_fields(&g, &edges.edges.data)?;
    let mut unit = vec![0.0f32; g.n];
    for (i, u) in unit.iter_mut().enumerate() {
        if i % 4096 == 0 {
            check(g.ctx)?;
        }
        *u = 1.0;
    }
    let phi = g.initial_level_set(init, &unit)?;
    let terms = LevelSetTerms {
        stopping: p.stopping,
        propagation: p.propagation_scaling as f32,
        curvature: p.curvature_scaling as f32,
        advection: p.advection_scaling as f32,
        speed: Some(&distance),
        advection_field: Some(&field),
        ..Default::default()
    };
    let r = g.evolve(phi, &terms, it, tol, o.progress)?;
    check(g.ctx)?;
    edge_boundary_result(r, p.boundary)
}
fn filter_err(g: &LevelSetGrid, e: FilterError) -> Error {
    if let Some(c) = level_set_context_error(g.ctx) {
        return c;
    }
    let sentinel = if e.is(ErrorKind::CannyResourceLimit) {
        ERR_LEVEL_SET_RESOURCE_LIMIT
    } else {
        ERR_LEVEL_SET_INVALID_INPUT
    };
    Error::wrap_err(sentinel, &filter_error(e))
}
fn roi_filter_err(g: &LevelSetGrid, e: Error) -> Error {
    if let Some(c) = level_set_context_error(g.ctx) {
        return c;
    }
    Error::wrap_err(ERR_LEVEL_SET_INVALID_INPUT, &e)
}
fn new_edge_level_set_grid<'a>(
    ctx: Option<&'a dyn Cancellation>,
    v: LevelSetVolume<'a>,
    init: &LevelSetInitialization,
    o: LevelSetOptions,
    dense: bool,
    interface_local: bool,
) -> Result<LevelSetGrid<'a>, Error> {
    check(ctx)?;
    let sizes = [v.columns, v.rows, v.slices];
    for (a, &n) in sizes.iter().enumerate() {
        if a == 2 && n == 1 {
            continue;
        }
        let h = v.spacing_mm[a];
        if !(h >= 2.0 * SMALLEST_F32 && h <= f64::MAX)
            || !(n as f64 * h <= f64::from(f32::MAX).sqrt() / 4.0)
        {
            return Err(invalid(format!("physical edge spacing/extent axis {a}")));
        }
    }
    if !finite_non_negative(init.initial_radius_mm)
        || init.initial_radius_mm > f64::from(f32::MAX) / 4.0
    {
        return Err(invalid("edge initialization radius range"));
    }
    let l = o.limits.normalize()?;
    if l.max_voxels > EDGE_MAX_VOXELS || l.max_bytes > EDGE_MAX_BYTES {
        return Err(invalid("new edge limits exceed finite ceilings"));
    }
    if init.seeds.len() > 64
        || v.columns <= 0
        || v.rows <= 0
        || v.slices <= 0
        || v.columns > EDGE_MAX_VOXELS
        || v.slices > EDGE_MAX_VOXELS
        || v.rows > l.max_voxels
        || init.masks.len() as i64 > l.max_voxels / v.rows
    {
        return Err(Error::wrap(
            ERR_LEVEL_SET_RESOURCE_LIMIT,
            "initialization dimensions/visits",
        ));
    }
    let mut visits = init.seeds.len() as i64;
    for (&z, m) in &init.masks {
        let Some(m) = m else {
            continue;
        };
        if z < 0 || z >= v.slices || m.columns != v.columns || m.rows != v.rows {
            return Err(invalid("initial mask"));
        }
        for (y, runs) in m.runs.iter().enumerate() {
            if y % 4096 == 0 {
                check(ctx)?;
            }
            visits += 1;
            let mut end = 0;
            for (k, run) in runs.iter().enumerate() {
                if interface_local && k % 4096 == 0 {
                    check(ctx)?;
                }
                if run.start < end || run.start < 0 || run.end <= run.start || run.end > m.columns {
                    return Err(invalid("initial mask run"));
                }
                end = run.end;
                visits += run.end - run.start + 1;
                if visits > l.max_voxels * 4 {
                    return Err(Error::wrap(
                        ERR_LEVEL_SET_RESOURCE_LIMIT,
                        "initial mask visits",
                    ));
                }
            }
        }
    }
    let workers = parallel::default_workers();
    let seeds = init.seeds.clone();
    let radius0 = init.initial_radius_mm;
    let mut reserve = |g: &LevelSetGrid| -> Result<GridProfile, Error> {
        let n = g.n as i64;
        let size = [g.nx as i64, g.ny as i64, g.nz as i64];
        let mut bytes =
            n * 192 + v.rows * size[2] * 32 + size[2] * 128 + seeds.len() as i64 * 256 + 4096;
        if interface_local {
            if size[0] < 3 || size[1] < 3 || size[2] != 1 && size[2] < 3 {
                return Err(invalid(
                    "InterfaceLocal requires at least 3 samples on active axes",
                ));
            }
            bytes += n * 32 + workers * 128;
        }
        bytes += canny_gaussian_scratch_bytes(size, workers).map_err(|e| filter_err(g, e))?;
        let mut edt = 0i64;
        for &d in &size {
            let lines = n / d;
            let w = (lines / 16).max(1).min(16);
            edt = edt.max(w * (d * 32 + 8));
            bytes += d * 8;
        }
        bytes += edt;
        if dense && !interface_local {
            if size[0] < 2 || size[1] < 2 {
                return Err(invalid("Canny crop must have at least 2x2 samples"));
            }
            let c = canny_working_bytes(size, workers).map_err(|e| filter_err(g, e))?;
            if c > 512 << 20 {
                return Err(Error::wrap(
                    ERR_LEVEL_SET_RESOURCE_LIMIT,
                    "Canny working bytes",
                ));
            }
        }
        if bytes > l.max_bytes {
            return Err(Error::wrap(
                ERR_LEVEL_SET_RESOURCE_LIMIT,
                format!("reserved {bytes} bytes exceeds {}", l.max_bytes),
            ));
        }
        let mut sample_visits = 0i64;
        let mut radius = radius0;
        if radius == 0.0 {
            radius = g.default_initial_radius();
        }
        for seed in &seeds {
            let mut bx = 1i64;
            let pos = [
                seed.x - g.origin[0],
                seed.y - g.origin[1],
                seed.slice - g.origin[2],
            ];
            for a in 0..3 {
                let n = size[a];
                let r = fmath::f64_to_i64(fmath::min(n as f64, (radius / g.h[a]).floor()));
                bx *= (n - 1).min(pos[a] + r) - 0.max(pos[a] - r) + 1;
            }
            sample_visits += bx;
            if sample_visits > n * 4 {
                return Err(Error::wrap(
                    ERR_LEVEL_SET_RESOURCE_LIMIT,
                    "seed sampling visits",
                ));
            }
        }
        Ok(GridProfile {
            bounded: true,
            dense,
            interface_local,
            workers,
        })
    };
    let margin = o.margin_mm;
    LevelSetGrid::new_reserved(
        ctx,
        v,
        init,
        &|_| margin_or_default(margin),
        o,
        Some(&mut reserve),
    )
}
fn canny_distance_fields(g: &LevelSetGrid, edges: &[f32]) -> Result<(Vec<f32>, Vec<f32>), Error> {
    let mut axes: [Vec<f64>; 3] = Default::default();
    for (a, &n) in g.size.iter().enumerate() {
        axes[a] = vec![0.0; n];
        for (i, x) in axes[a].iter_mut().enumerate() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            *x = i as f64 * g.h[a];
        }
    }
    let [x, y, z] = axes;
    let mut mask = VolumeMask {
        columns: g.nx as i64,
        rows: g.ny as i64,
        slices: g.nz as i64,
        x,
        y,
        z,
        voxels: vec![false; g.n],
    };
    for (i, &v) in edges.iter().enumerate() {
        if i % 4096 == 0 {
            check(g.ctx)?;
        }
        mask.voxels[i] = v != 0.0;
    }
    let d = mask
        .distance_transform(g.ctx, true)
        .map_err(|e| roi_filter_err(g, e))?;
    let (mut speed, mut field) = (vec![0.0f32; g.n], vec![0.0f32; g.n]);
    for (i, &v) in d.iter().enumerate() {
        if i % 4096 == 0 {
            check(g.ctx)?;
        }
        let a = v * v / 2.0;
        let max = f64::from(f32::MAX);
        if !(v >= 0.0 && v <= max && a <= max) {
            return Err(invalid("non-finite Canny distance/D squared"));
        }
        speed[i] = v as f32;
        field[i] = a as f32;
    }
    Ok((speed, field))
}
const SMALLEST_F32: f64 = 1.401298464324817e-45;
