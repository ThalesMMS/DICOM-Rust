#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::connected_components::{
    CONNECTIVITY_6, Connectivity3D, FillNeighbour, SpanFillError, SpanFiller, SpanStop,
    fill_neighbour_rows,
};
use super::roimask::{RasterMask, raster_mask_pixel_count};
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::textfmt::float_v;
use std::collections::BTreeMap;
use std::fmt;
pub const ERR_REGION_GROW_INVALID_VOLUME: &str = "roi: invalid region growing volume";
pub const ERR_REGION_GROW_NO_SEEDS: &str = "roi: region growing requires at least one seed";
pub const ERR_REGION_GROW_INVALID_PARAMETERS: &str = "roi: invalid region growing parameters";
pub const ERR_REGION_GROW_RESOURCE_LIMIT: &str = "roi: region growing resource limit exceeded";
pub const ERR_REGION_GROW_CANCELLED: &str = "roi: region growing cancelled";
const DEFAULT_REGION_GROW_MAX_REGION_VOXELS: i64 = 64 * 1024 * 1024;
const DEFAULT_REGION_GROW_MAX_NEIGHBORHOOD_VOXELS: i64 = 33 * 33 * 33;
const DEFAULT_REGION_GROW_MAX_FILLS: i64 = 64;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RegionGrowSeed {
    pub x: i64,
    pub y: i64,
    pub slice: i64,
}
impl fmt::Display for RegionGrowSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{X:{} Y:{} Slice:{}}}", self.x, self.y, self.slice)
    }
}
pub(crate) type RegionGrowValueAt<'a> = &'a dyn Fn(i64, i64, i64) -> Option<f64>;
#[derive(Clone, Copy, Default)]
pub struct RegionGrowVolume<'a> {
    pub columns: i64,
    pub rows: i64,
    pub slices: i64,
    pub value_at: Option<RegionGrowValueAt<'a>>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegionGrowLimits {
    pub max_region_voxels: i64,
    pub max_neighborhood_voxels: i64,
    pub max_fills: i64,
}
pub fn default_region_grow_limits() -> RegionGrowLimits {
    RegionGrowLimits {
        max_region_voxels: DEFAULT_REGION_GROW_MAX_REGION_VOXELS,
        max_neighborhood_voxels: DEFAULT_REGION_GROW_MAX_NEIGHBORHOOD_VOXELS,
        max_fills: DEFAULT_REGION_GROW_MAX_FILLS,
    }
}
impl RegionGrowLimits {
    fn normalize(mut self) -> Result<RegionGrowLimits, Error> {
        if self.max_region_voxels < 0 || self.max_neighborhood_voxels < 0 || self.max_fills < 0 {
            return Err(Error::wrap(
                ERR_REGION_GROW_INVALID_PARAMETERS,
                "negative limit",
            ));
        }
        let defaults = default_region_grow_limits();
        if self.max_region_voxels == 0 {
            self.max_region_voxels = defaults.max_region_voxels;
        }
        if self.max_neighborhood_voxels == 0 {
            self.max_neighborhood_voxels = defaults.max_neighborhood_voxels;
        }
        if self.max_fills == 0 {
            self.max_fills = defaults.max_fills;
        }
        Ok(self)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegionGrowOptions {
    pub connectivity: Connectivity3D,
    pub limits: RegionGrowLimits,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegionGrowResult {
    pub masks: BTreeMap<i64, RasterMask>,
    pub voxel_count: i64,
    pub lower: f64,
    pub upper: f64,
    pub ignored_seeds: Vec<RegionGrowSeed>,
    pub fills: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NeighborhoodRadius {
    pub x: i64,
    pub y: i64,
    pub slice: i64,
}
impl fmt::Display for NeighborhoodRadius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{X:{} Y:{} Slice:{}}}", self.x, self.y, self.slice)
    }
}
pub fn uniform_neighborhood_radius(r: i64) -> NeighborhoodRadius {
    NeighborhoodRadius {
        x: r,
        y: r,
        slice: r,
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ConfidenceConnectedParams {
    pub multiplier: f64,
    pub iterations: i64,
    pub initial_radius: i64,
}
pub fn default_confidence_connected_params() -> ConfidenceConnectedParams {
    ConfidenceConnectedParams {
        multiplier: 2.5,
        iterations: 4,
        initial_radius: 1,
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ConfidenceStop {
    #[default]
    Iterations,
    ZeroVariance,
    EmptyRegion,
}
impl fmt::Display for ConfidenceStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfidenceStop::ZeroVariance => "zero variance",
            ConfidenceStop::EmptyRegion => "empty region",
            ConfidenceStop::Iterations => "iterations completed",
        })
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfidenceConnectedResult {
    pub region: RegionGrowResult,
    pub mean: f64,
    pub variance: f64,
    pub initial_samples: i64,
    pub iterations: i64,
    pub stop: ConfidenceStop,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IsolatedConnectedParams {
    pub lower: f64,
    pub upper: f64,
    pub find_upper_threshold: bool,
    pub tolerance: f64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IsolatedConnectedResult {
    pub region: RegionGrowResult,
    pub isolated_value: f64,
    pub thresholding_failed: bool,
}
struct RegionGrower<'a> {
    ctx: Option<&'a dyn Cancellation>,
    columns: i64,
    rows: i64,
    slices: i64,
    value_at: RegionGrowValueAt<'a>,
    neighbours: Vec<FillNeighbour>,
    limits: RegionGrowLimits,
    fills: i64,
}
pub(crate) fn region_grow_context_error(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(err) => Err(Error::wrap_err(ERR_REGION_GROW_CANCELLED, &err)),
        None => Ok(()),
    }
}
fn usable_value(value_at: RegionGrowValueAt<'_>, x: i64, y: i64, z: i64) -> Option<f64> {
    value_at(x, y, z).filter(|v| !v.is_nan())
}
fn in_interval<'a>(
    value_at: RegionGrowValueAt<'a>,
    lower: f64,
    upper: f64,
) -> impl FnMut(i64, i64, i64) -> bool + 'a {
    move |x, y, z| usable_value(value_at, x, y, z).is_some_and(|v| v >= lower && v <= upper)
}
impl<'a> RegionGrower<'a> {
    fn new(
        ctx: Option<&'a dyn Cancellation>,
        volume: &RegionGrowVolume<'a>,
        options: &RegionGrowOptions,
    ) -> Result<RegionGrower<'a>, Error> {
        region_grow_context_error(ctx)?;
        let Some(value_at) = volume.value_at else {
            return Err(Error::wrap(
                ERR_REGION_GROW_INVALID_VOLUME,
                "nil value source",
            ));
        };
        if let Err(err) = raster_mask_pixel_count(volume.columns, volume.rows) {
            return Err(Error::wrap(ERR_REGION_GROW_INVALID_VOLUME, err));
        }
        if volume.slices <= 0 {
            return Err(Error::wrap(
                ERR_REGION_GROW_INVALID_VOLUME,
                format!("{} slices", volume.slices),
            ));
        }
        let mut connectivity = options.connectivity;
        if connectivity.0 == 0 {
            connectivity = CONNECTIVITY_6;
        }
        connectivity.validate()?;
        let limits = options.limits.normalize()?;
        Ok(RegionGrower {
            ctx,
            columns: volume.columns,
            rows: volume.rows,
            slices: volume.slices,
            value_at,
            neighbours: fill_neighbour_rows(connectivity),
            limits,
            fills: 0,
        })
    }
    fn contains(&self, seed: RegionGrowSeed) -> bool {
        seed.x >= 0
            && seed.y >= 0
            && seed.slice >= 0
            && seed.x < self.columns
            && seed.y < self.rows
            && seed.slice < self.slices
    }
    fn fill(
        &mut self,
        seeds: &[RegionGrowSeed],
        inside: &mut dyn FnMut(i64, i64, i64) -> bool,
        mut stop: Option<&mut dyn FnMut(i64, i64, i64, i64) -> bool>,
    ) -> Result<(BTreeMap<i64, RasterMask>, i64), Error> {
        if self.fills >= self.limits.max_fills {
            return Err(Error::wrap(
                ERR_REGION_GROW_RESOURCE_LIMIT,
                format!("more than {} fills", self.limits.max_fills),
            ));
        }
        self.fills += 1;
        let mut masks = BTreeMap::new();
        let mut stop_span;
        let stop: Option<SpanStop<'_>> = match stop.as_mut() {
            Some(stop) => {
                stop_span = |z: i64, y: i64, x0: i64, x1: i64| stop(z, y, x0, x1);
                Some(&mut stop_span)
            }
            None => None,
        };
        let filled = {
            let mut f = SpanFiller::new(
                self.columns,
                self.rows,
                self.slices,
                inside,
                self.neighbours.clone(),
                &mut masks,
            );
            f.ctx = self.ctx;
            f.max_voxels = self.limits.max_region_voxels;
            f.stop = stop;
            for &seed in seeds {
                if !self.contains(seed) {
                    continue;
                }
                f.fill(seed.x, seed.y, seed.slice);
                match f.err.take() {
                    Some(SpanFillError::Cancelled(err)) => {
                        return Err(Error::wrap_err(ERR_REGION_GROW_CANCELLED, &err));
                    }
                    Some(SpanFillError::ResourceLimit(max_voxels)) => {
                        return Err(Error::wrap(
                            ERR_REGION_GROW_RESOURCE_LIMIT,
                            format!("region exceeds {max_voxels} voxels"),
                        ));
                    }
                    None => {}
                }
                if f.stopped {
                    break;
                }
            }
            f.filled
        };
        region_grow_context_error(self.ctx)?;
        Ok((masks, filled))
    }
    fn result(
        &self,
        seeds: &[RegionGrowSeed],
        masks: BTreeMap<i64, RasterMask>,
        count: i64,
        lower: f64,
        upper: f64,
    ) -> RegionGrowResult {
        let ignored_seeds = seeds
            .iter()
            .copied()
            .filter(|&seed| !self.contains(seed) || !mask_get(&masks, seed))
            .collect();
        RegionGrowResult {
            masks,
            voxel_count: count,
            lower,
            upper,
            ignored_seeds,
            fills: self.fills,
        }
    }
    fn box_voxels(&self, radius: NeighborhoodRadius) -> Result<i64, Error> {
        if radius.x < 0 || radius.y < 0 || radius.slice < 0 {
            return Err(Error::wrap(
                ERR_REGION_GROW_INVALID_PARAMETERS,
                format!("negative radius {radius}"),
            ));
        }
        let mut size = 1i64;
        for r in [radius.x, radius.y, radius.slice] {
            let side = r.wrapping_mul(2).wrapping_add(1);
            if side <= 0 || size > self.limits.max_neighborhood_voxels / side {
                return Err(Error::wrap(
                    ERR_REGION_GROW_RESOURCE_LIMIT,
                    format!(
                        "neighbourhood {radius} exceeds {} voxels",
                        self.limits.max_neighborhood_voxels
                    ),
                ));
            }
            size *= side;
        }
        Ok(size)
    }
}
fn mask_get(masks: &BTreeMap<i64, RasterMask>, seed: RegionGrowSeed) -> bool {
    masks
        .get(&seed.slice)
        .is_some_and(|mask| mask.get(seed.x, seed.y))
}
fn validate_interval(lower: f64, upper: f64) -> Result<(), Error> {
    if lower.is_nan() || upper.is_nan() || lower > upper {
        return Err(Error::wrap(
            ERR_REGION_GROW_INVALID_PARAMETERS,
            format!("interval [{}, {}]", float_v(lower), float_v(upper)),
        ));
    }
    Ok(())
}
pub fn grow_connected_threshold(
    ctx: Option<&dyn Cancellation>,
    volume: &RegionGrowVolume<'_>,
    seeds: &[RegionGrowSeed],
    lower: f64,
    upper: f64,
    options: &RegionGrowOptions,
) -> Result<RegionGrowResult, Error> {
    let mut g = RegionGrower::new(ctx, volume, options)?;
    if seeds.is_empty() {
        return Err(Error::sentinel(ERR_REGION_GROW_NO_SEEDS));
    }
    validate_interval(lower, upper)?;
    let (masks, count) = g.fill(seeds, &mut in_interval(g.value_at, lower, upper), None)?;
    Ok(g.result(seeds, masks, count, lower, upper))
}
fn box_range(c: i64, r: i64, n: i64) -> (i64, i64) {
    ((c - r).max(0), (c + r).min(n - 1))
}
pub fn grow_neighborhood_connected(
    ctx: Option<&dyn Cancellation>,
    volume: &RegionGrowVolume<'_>,
    seeds: &[RegionGrowSeed],
    lower: f64,
    upper: f64,
    radius: NeighborhoodRadius,
    options: &RegionGrowOptions,
) -> Result<RegionGrowResult, Error> {
    let mut g = RegionGrower::new(ctx, volume, options)?;
    if seeds.is_empty() {
        return Err(Error::sentinel(ERR_REGION_GROW_NO_SEEDS));
    }
    validate_interval(lower, upper)?;
    let mut radius = radius;
    if volume.slices == 1 {
        radius.slice = 0;
    }
    g.box_voxels(radius)?;
    let value_at = g.value_at;
    let (columns, rows, slices) = (g.columns, g.rows, g.slices);
    let outside = move |x: i64, y: i64, z: i64| match usable_value(value_at, x, y, z) {
        None => true,
        Some(v) => v < lower || v > upper,
    };
    let mut inside = move |x: i64, y: i64, z: i64| {
        if outside(x, y, z) {
            return false;
        }
        let (z0, z1) = box_range(z, radius.slice, slices);
        let (y0, y1) = box_range(y, radius.y, rows);
        let (x0, x1) = box_range(x, radius.x, columns);
        for zz in z0..=z1 {
            for yy in y0..=y1 {
                for xx in x0..=x1 {
                    if outside(xx, yy, zz) {
                        return false;
                    }
                }
            }
        }
        true
    };
    let (masks, count) = g.fill(seeds, &mut inside, None)?;
    Ok(g.result(seeds, masks, count, lower, upper))
}
const ZERO_VARIANCE: f64 = f64::from_bits(0x3c79_9999_9999_9999);
fn sample_variance(centred_squares: f64, n: i64) -> f64 {
    if n < 2 || !(centred_squares > 0.0) {
        return 0.0;
    }
    centred_squares / (n - 1) as f64
}
pub fn grow_confidence_connected(
    ctx: Option<&dyn Cancellation>,
    volume: &RegionGrowVolume<'_>,
    seeds: &[RegionGrowSeed],
    params: ConfidenceConnectedParams,
    options: &RegionGrowOptions,
) -> Result<ConfidenceConnectedResult, Error> {
    let mut g = RegionGrower::new(ctx, volume, options)?;
    if seeds.is_empty() {
        return Err(Error::sentinel(ERR_REGION_GROW_NO_SEEDS));
    }
    if !params.multiplier.is_finite()
        || params.multiplier < 0.0
        || params.iterations < 0
        || params.initial_radius < 0
    {
        return Err(Error::wrap(
            ERR_REGION_GROW_INVALID_PARAMETERS,
            format!(
                "confidence parameters {{Multiplier:{} Iterations:{} InitialRadius:{} quantizeBounds:<nil>}}",
                float_v(params.multiplier),
                params.iterations,
                params.initial_radius
            ),
        ));
    }
    if params.iterations >= g.limits.max_fills {
        return Err(Error::wrap(
            ERR_REGION_GROW_RESOURCE_LIMIT,
            format!(
                "{} iterations need more than {} fills",
                params.iterations, g.limits.max_fills
            ),
        ));
    }
    let mut radius = uniform_neighborhood_radius(params.initial_radius);
    if volume.slices == 1 {
        radius.slice = 0;
    }
    g.box_voxels(radius)?;
    let value_at = g.value_at;
    let (mut mean_sum, mut sum_squares) = (0.0f64, 0.0f64);
    let (mut samples, mut used_seeds) = (0i64, 0i64);
    let (mut lowest_seed, mut highest_seed) = (f64::INFINITY, f64::NEG_INFINITY);
    for &seed in seeds {
        if !g.contains(seed) {
            continue;
        }
        let Some(seed_value) =
            usable_value(value_at, seed.x, seed.y, seed.slice).filter(|v| !v.is_infinite())
        else {
            continue;
        };
        let (mut sum, mut squares) = (0.0f64, 0.0f64);
        let mut n = 0i64;
        let (z0, z1) = box_range(seed.slice, radius.slice, volume.slices);
        let (y0, y1) = box_range(seed.y, radius.y, volume.rows);
        let (x0, x1) = box_range(seed.x, radius.x, volume.columns);
        for dz in -radius.slice..=radius.slice {
            let zz = (seed.slice + dz).max(z0).min(z1);
            for dy in -radius.y..=radius.y {
                let yy = (seed.y + dy).max(y0).min(y1);
                for dx in -radius.x..=radius.x {
                    let xx = (seed.x + dx).max(x0).min(x1);
                    let Some(v) = usable_value(value_at, xx, yy, zz).filter(|v| !v.is_infinite())
                    else {
                        continue;
                    };
                    sum += v;
                    squares += v * v;
                    n += 1;
                }
            }
        }
        if n == 0 {
            continue;
        }
        mean_sum += sum / n as f64;
        sum_squares += squares;
        samples += n;
        used_seeds += 1;
        lowest_seed = fmath::min(lowest_seed, seed_value);
        highest_seed = fmath::max(highest_seed, seed_value);
    }
    let mut result = ConfidenceConnectedResult {
        initial_samples: samples,
        ..ConfidenceConnectedResult::default()
    };
    if used_seeds == 0 {
        result.region = g.result(seeds, BTreeMap::new(), 0, f64::NAN, f64::NAN);
        result.mean = f64::NAN;
        result.variance = f64::NAN;
        result.stop = ConfidenceStop::EmptyRegion;
        return Ok(result);
    }
    let mean = mean_sum / used_seeds as f64;
    let variance = sample_variance(sum_squares - mean * mean * samples as f64, samples);
    let bounds = |mean: f64, variance: f64| -> (f64, f64) {
        let spread = params.multiplier * variance.sqrt();
        let (lower, upper) = (mean - spread, mean + spread);
        (
            fmath::min(lower, lowest_seed),
            fmath::max(upper, highest_seed),
        )
    };
    let (mut lower, mut upper) = bounds(mean, variance);
    let (mut masks, mut count) = g.fill(seeds, &mut in_interval(value_at, lower, upper), None)?;
    result.mean = mean;
    result.variance = variance;
    for _ in 0..params.iterations {
        let (mut sum, mut squares) = (0.0f64, 0.0f64);
        let mut n = 0i64;
        for (&slice, mask) in &masks {
            mask.for_each_run(|y, run| {
                for x in run.start..run.end {
                    if let Some(v) = usable_value(value_at, x, y, slice) {
                        sum += v;
                        squares += v * v;
                        n += 1;
                    }
                }
            });
        }
        region_grow_context_error(g.ctx)?;
        if n == 0 {
            result.stop = ConfidenceStop::EmptyRegion;
            break;
        }
        let region_mean = sum / n as f64;
        let region_variance = sample_variance(squares - sum * sum / n as f64, n);
        if region_variance <= ZERO_VARIANCE {
            result.stop = ConfidenceStop::ZeroVariance;
            break;
        }
        (lower, upper) = bounds(region_mean, region_variance);
        (masks, count) = g.fill(seeds, &mut in_interval(value_at, lower, upper), None)?;
        result.mean = region_mean;
        result.variance = region_variance;
        result.iterations += 1;
    }
    result.region = g.result(seeds, masks, count, lower, upper);
    Ok(result)
}
pub fn grow_isolated_connected(
    ctx: Option<&dyn Cancellation>,
    volume: &RegionGrowVolume<'_>,
    seeds1: &[RegionGrowSeed],
    seeds2: &[RegionGrowSeed],
    params: IsolatedConnectedParams,
    options: &RegionGrowOptions,
) -> Result<IsolatedConnectedResult, Error> {
    let mut g = RegionGrower::new(ctx, volume, options)?;
    if seeds1.is_empty() || seeds2.is_empty() {
        return Err(Error::wrap(
            ERR_REGION_GROW_NO_SEEDS,
            "isolated growing needs seeds on both sides",
        ));
    }
    for &seed in seeds1.iter().chain(seeds2) {
        if !g.contains(seed) {
            return Err(Error::wrap(
                ERR_REGION_GROW_INVALID_PARAMETERS,
                format!("seed {seed} outside the volume"),
            ));
        }
    }
    validate_interval(params.lower, params.upper)?;
    if params.lower.is_infinite() || params.upper.is_infinite() {
        return Err(Error::wrap(
            ERR_REGION_GROW_INVALID_PARAMETERS,
            "the isolated search range must be finite",
        ));
    }
    let mut tolerance = params.tolerance;
    if tolerance == 0.0 {
        tolerance = 1.0;
    }
    if !(tolerance > 0.0) || tolerance.is_infinite() {
        return Err(Error::wrap(
            ERR_REGION_GROW_INVALID_PARAMETERS,
            format!("tolerance {}", float_v(params.tolerance)),
        ));
    }
    let value_at = g.value_at;
    let midpoint = |a: f64, b: f64| (a + b) / 2.0;
    let reached = |g: &mut RegionGrower<'_>, lo: f64, hi: f64| -> Result<bool, Error> {
        let mut touched = false;
        let mut stop = |z: i64, y: i64, x0: i64, x1: i64| {
            if seeds2
                .iter()
                .any(|seed| seed.slice == z && seed.y == y && seed.x >= x0 && seed.x < x1)
            {
                touched = true;
            }
            touched
        };
        g.fill(seeds1, &mut in_interval(value_at, lo, hi), Some(&mut stop))?;
        Ok(touched)
    };
    let (mut lower, mut upper) = (params.lower, params.upper);
    let isolated = if params.find_upper_threshold {
        let mut guess = upper;
        while lower + tolerance < guess {
            if reached(&mut g, params.lower, guess)? {
                upper = guess;
            } else {
                lower = guess;
            }
            guess = midpoint(lower, upper);
        }
        lower
    } else {
        let mut guess = lower;
        while guess < upper - tolerance {
            if reached(&mut g, guess, params.upper)? {
                lower = guess;
            } else {
                upper = guess;
            }
            guess = midpoint(lower, upper);
        }
        upper
    };
    let (region_lower, region_upper) = if params.find_upper_threshold {
        (params.lower, isolated)
    } else {
        (isolated, params.upper)
    };
    let (masks, count) = g.fill(
        seeds1,
        &mut in_interval(value_at, region_lower, region_upper),
        None,
    )?;
    let region = g.result(seeds1, masks, count, region_lower, region_upper);
    let mut failed = !region.ignored_seeds.is_empty();
    for &seed in seeds2 {
        if mask_get(&region.masks, seed) {
            failed = true;
        }
    }
    Ok(IsolatedConnectedResult {
        region,
        isolated_value: isolated,
        thresholding_failed: failed,
    })
}
