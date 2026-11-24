use super::connected_components::{CONNECTIVITY_6, CONNECTIVITY_18, Connectivity3D};
use super::region_growing::{
    ERR_REGION_GROW_INVALID_PARAMETERS, ERR_REGION_GROW_INVALID_VOLUME, ERR_REGION_GROW_NO_SEEDS,
    ERR_REGION_GROW_RESOURCE_LIMIT, RegionGrowSeed, region_grow_context_error,
};
use super::roimask::{MaskRun, RasterMask};
use super::vector_confidence_stats::VectorConfidenceModel;
use super::voi::VoxelGrid;
use super::{Cancellation, Error};
use crate::fmath;
use crate::render::Vec3;
use crate::textfmt::quote;
use std::collections::BTreeMap;
pub(crate) type VectorConfidenceValueAt<'a> = &'a dyn Fn(i64, i64, i64) -> Option<f64>;
#[derive(Clone, Copy)]
pub struct VectorConfidenceChannel<'a> {
    pub id: &'a str,
    pub unit: &'a str,
    pub frame_of_reference_id: &'a str,
    pub grid: VoxelGrid,
    pub offset: f64,
    pub scale: f64,
    pub value_at: Option<VectorConfidenceValueAt<'a>>,
}
#[derive(Clone, Copy)]
pub struct VectorConfidenceVolume<'a> {
    pub grid: VoxelGrid,
    pub frame_of_reference_id: &'a str,
    pub channels: &'a [VectorConfidenceChannel<'a>],
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct VectorCovariancePolicy(pub u8);
pub const VECTOR_COVARIANCE_REQUIRE_POSITIVE_DEFINITE: VectorCovariancePolicy =
    VectorCovariancePolicy(0);
pub const VECTOR_COVARIANCE_RIDGE: VectorCovariancePolicy = VectorCovariancePolicy(1);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct VectorCovarianceNormalization(pub u8);
pub const VECTOR_COVARIANCE_SAMPLE: VectorCovarianceNormalization =
    VectorCovarianceNormalization(0);
pub const VECTOR_COVARIANCE_POPULATION: VectorCovarianceNormalization =
    VectorCovarianceNormalization(1);
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VectorConfidenceParams {
    pub multiplier: f64,
    pub iterations: i64,
    pub initial_radius: i64,
    pub covariance_policy: VectorCovariancePolicy,
    pub covariance_normalization: VectorCovarianceNormalization,
    pub ridge: f64,
    pub min_pivot_ratio: f64,
}
pub fn default_vector_confidence_params() -> VectorConfidenceParams {
    VectorConfidenceParams {
        multiplier: 2.5,
        iterations: 4,
        initial_radius: 1,
        ..VectorConfidenceParams::default()
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VectorConfidenceLimits {
    pub max_channels: i64,
    pub max_grid_voxels: i64,
    pub max_region_voxels: i64,
    pub max_neighborhood_samples: i64,
    pub max_seeds: i64,
    pub max_fills: i64,
    pub max_work: u64,
    pub max_estimated_bytes: u64,
}
pub fn default_vector_confidence_limits() -> VectorConfidenceLimits {
    VectorConfidenceLimits {
        max_channels: 16,
        max_grid_voxels: 1 << 24,
        max_region_voxels: 1 << 22,
        max_neighborhood_samples: 1 << 20,
        max_seeds: 4096,
        max_fills: 16,
        max_work: 1 << 29,
        max_estimated_bytes: 128 << 20,
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VectorConfidenceOptions {
    pub connectivity: Connectivity3D,
    pub limits: VectorConfidenceLimits,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VectorConfidenceChannelInfo {
    pub id: String,
    pub unit: String,
    pub offset: f64,
    pub scale: f64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VectorConfidenceResult {
    pub covariance_normalization: VectorCovarianceNormalization,
    pub masks: BTreeMap<i64, RasterMask>,
    pub grid: VoxelGrid,
    pub frame_of_reference_id: String,
    pub channels: Vec<VectorConfidenceChannelInfo>,
    pub mean: Vec<f64>,
    pub covariance: Vec<f64>,
    pub applied_covariance: Vec<f64>,
    pub effective_multiplier: f64,
    pub voxel_count: i64,
    pub initial_samples: i64,
    pub model_samples: i64,
    pub unique_seeds: i64,
    pub duplicate_seeds: i64,
    pub iterations: i64,
    pub fills: i64,
    pub callback_reads: u64,
    pub work: u64,
    pub estimated_bytes: u64,
}
pub const ERR_VECTOR_CONFIDENCE_GEOMETRY: &str = "roi: vector confidence geometry mismatch";
pub const ERR_VECTOR_CONFIDENCE_UNAVAILABLE: &str = "roi: vector confidence channel unavailable";
pub const ERR_VECTOR_CONFIDENCE_NUMERIC: &str = "roi: vector confidence numeric failure";
pub const ERR_VECTOR_CONFIDENCE_COVARIANCE: &str = "roi: vector confidence covariance not admitted";
pub(crate) fn finite(v: f64) -> bool {
    !v.is_nan() && !v.is_infinite()
}
fn text(v: &str) -> bool {
    v.len() <= 256 && !v.trim().is_empty()
}
fn finite_vec3(p: Vec3) -> bool {
    finite(p.x) && finite(p.y) && finite(p.z)
}
pub(crate) struct Ledger<'a> {
    ctx: Option<&'a dyn Cancellation>,
    max_work: u64,
    pub(crate) work: u64,
}
impl Ledger<'_> {
    pub(crate) fn charge(&mut self, n: u64) -> Result<(), Error> {
        region_grow_context_error(self.ctx)?;
        if n > self.max_work - self.work {
            return Err(Error::wrap(
                ERR_REGION_GROW_RESOURCE_LIMIT,
                "vector confidence work limit",
            ));
        }
        self.work += n;
        Ok(())
    }
}
struct Grower<'a> {
    ledger: Ledger<'a>,
    volume: VectorConfidenceVolume<'a>,
    params: VectorConfidenceParams,
    limits: VectorConfidenceLimits,
    connectivity: Connectivity3D,
    n: usize,
    region_cap: usize,
    estimate: u64,
    reads: u64,
    state: Vec<u8>,
    queue: Vec<u32>,
    seeds: Vec<u32>,
    seed_values: Vec<f64>,
    model: VectorConfidenceModel,
    count: usize,
    effective: f64,
}
pub fn grow_vector_confidence_connected<'a>(
    ctx: Option<&'a dyn Cancellation>,
    volume: VectorConfidenceVolume<'a>,
    seeds: &[RegionGrowSeed],
    params: VectorConfidenceParams,
    options: VectorConfidenceOptions,
) -> Result<VectorConfidenceResult, Error> {
    let mut g = Grower {
        ledger: Ledger {
            ctx,
            max_work: 0,
            work: 0,
        },
        volume,
        params,
        limits: VectorConfidenceLimits::default(),
        connectivity: options.connectivity,
        n: 0,
        region_cap: 0,
        estimate: 0,
        reads: 0,
        state: Vec::new(),
        queue: Vec::new(),
        seeds: Vec::new(),
        seed_values: Vec::new(),
        model: VectorConfidenceModel::default(),
        count: 0,
        effective: 0.0,
    };
    g.prepare(seeds, options.limits)?;
    let c = volume.channels.len();
    for i in 0..g.seeds.len() {
        let index = g.seeds[i];
        let mut dst = std::mem::take(&mut g.seed_values);
        let read = g.read(index, &mut dst[i * c..(i + 1) * c]);
        g.seed_values = dst;
        read?;
    }
    g.initial_statistics()?;
    let initial_samples = g.model.samples;
    for fill in 0..=params.iterations {
        if fill > 0 {
            g.region_statistics()?;
        }
        g.model.factor(&mut g.ledger, &g.params)?;
        g.effective = params.multiplier;
        for i in 0..g.seeds.len() {
            let d = g
                .model
                .distance(&mut g.ledger, &g.seed_values[i * c..(i + 1) * c])?;
            g.effective = fmath::max(g.effective, d);
        }
        g.fill()?;
    }
    let masks = g.masks()?;
    g.ledger.charge((2 * c * c + 2 * c) as u64)?;
    let result = VectorConfidenceResult {
        covariance_normalization: g.params.covariance_normalization,
        masks,
        grid: volume.grid,
        frame_of_reference_id: volume.frame_of_reference_id.to_string(),
        channels: volume
            .channels
            .iter()
            .map(|ch| VectorConfidenceChannelInfo {
                id: ch.id.to_string(),
                unit: ch.unit.to_string(),
                offset: ch.offset,
                scale: ch.scale,
            })
            .collect(),
        mean: g.model.mean.clone(),
        covariance: g.model.cov.clone(),
        applied_covariance: g.model.applied.clone(),
        effective_multiplier: g.effective,
        voxel_count: g.count as i64,
        initial_samples,
        model_samples: g.model.samples,
        unique_seeds: g.seeds.len() as i64,
        duplicate_seeds: (seeds.len() - g.seeds.len()) as i64,
        iterations: params.iterations,
        fills: params.iterations + 1,
        callback_reads: g.reads,
        work: g.ledger.work,
        estimated_bytes: g.estimate,
    };
    region_grow_context_error(ctx)?;
    Ok(result)
}
fn invalid_parameters(detail: impl std::fmt::Display) -> Error {
    Error::wrap(ERR_REGION_GROW_INVALID_PARAMETERS, detail)
}
fn resource_limit(detail: impl std::fmt::Display) -> Error {
    Error::wrap(ERR_REGION_GROW_RESOURCE_LIMIT, detail)
}
fn invalid_volume(detail: impl std::fmt::Display) -> Error {
    Error::wrap(ERR_REGION_GROW_INVALID_VOLUME, detail)
}
impl Grower<'_> {
    fn prepare(
        &mut self,
        seeds: &[RegionGrowSeed],
        limits: VectorConfidenceLimits,
    ) -> Result<(), Error> {
        let ctx = self.ledger.ctx;
        region_grow_context_error(ctx)?;
        self.limits = normalize_limits(limits)?;
        self.ledger.max_work = self.limits.max_work;
        if self.connectivity.0 == 0 {
            self.connectivity = CONNECTIVITY_6;
        }
        self.connectivity.validate()?;
        let p = &mut self.params;
        if p.covariance_normalization != VECTOR_COVARIANCE_SAMPLE
            && p.covariance_normalization != VECTOR_COVARIANCE_POPULATION
        {
            return Err(invalid_parameters("unknown covariance normalization"));
        }
        if !finite(p.multiplier)
            || p.multiplier <= 0.0
            || p.multiplier > 1e6
            || p.iterations < 0
            || p.iterations > 64
            || p.initial_radius < 0
            || p.initial_radius > 16
        {
            return Err(invalid_parameters("vector confidence parameters"));
        }
        if p.min_pivot_ratio == 0.0 {
            p.min_pivot_ratio = 1e-12;
        }
        if !finite(p.min_pivot_ratio)
            || p.min_pivot_ratio < 1e-15
            || p.min_pivot_ratio > 1e-2
            || !finite(p.ridge)
        {
            return Err(invalid_parameters("covariance admission parameters"));
        }
        match p.covariance_policy {
            VECTOR_COVARIANCE_REQUIRE_POSITIVE_DEFINITE => {
                if p.ridge != 0.0 {
                    return Err(invalid_parameters("strict covariance requires zero ridge"));
                }
            }
            VECTOR_COVARIANCE_RIDGE => {
                if p.ridge < 1e-200 || p.ridge > 1e200 {
                    return Err(invalid_parameters("ridge outside finite profile"));
                }
            }
            _ => return Err(invalid_parameters("unknown covariance policy")),
        }
        if p.iterations + 1 > self.limits.max_fills {
            return Err(resource_limit("fill limit"));
        }
        let initial_radius = p.initial_radius;
        let grid = self.volume.grid;
        if grid.columns <= 0 || grid.rows <= 0 || grid.slices <= 0 {
            return Err(invalid_volume("non-positive vector grid dimensions"));
        }
        let n = match product(&[grid.columns as u64, grid.rows as u64, grid.slices as u64]) {
            Some(n) if n <= self.limits.max_grid_voxels as u64 => n,
            _ => return Err(resource_limit("grid voxel limit")),
        };
        self.n = n as usize;
        self.region_cap = (n as i64).min(self.limits.max_region_voxels) as usize;
        validate_grid(&grid)?;
        let c = self.volume.channels.len();
        if c < 1 || c as i64 > self.limits.max_channels {
            return Err(resource_limit("channel count"));
        }
        if !text(self.volume.frame_of_reference_id) {
            return Err(invalid_volume("frame identity"));
        }
        for (i, ch) in self.volume.channels.iter().enumerate() {
            if !text(ch.id)
                || !text(ch.unit)
                || !text(ch.frame_of_reference_id)
                || ch.value_at.is_none()
            {
                return Err(invalid_volume(format_args!(
                    "channel {i} metadata or callback"
                )));
            }
            for other in &self.volume.channels[..i] {
                if other.id == ch.id {
                    return Err(invalid_volume("duplicate channel identity"));
                }
            }
            if ch.grid != grid || ch.frame_of_reference_id != self.volume.frame_of_reference_id {
                return Err(Error::wrap(
                    ERR_VECTOR_CONFIDENCE_GEOMETRY,
                    format_args!("channel {}", quote(ch.id)),
                ));
            }
            if !finite(ch.offset) || !finite(ch.scale) || ch.scale <= 0.0 {
                return Err(invalid_parameters(format_args!(
                    "channel {} normalization",
                    quote(ch.id)
                )));
            }
        }
        if seeds.is_empty() {
            return Err(Error::sentinel(ERR_REGION_GROW_NO_SEEDS));
        }
        if seeds.len() as i64 > self.limits.max_seeds {
            return Err(resource_limit("seed limit"));
        }
        for (i, s) in seeds.iter().enumerate() {
            bulk_poll(ctx, i)?;
            if s.x < 0
                || s.y < 0
                || s.slice < 0
                || s.x >= grid.columns
                || s.y >= grid.rows
                || s.slice >= grid.slices
            {
                return Err(invalid_parameters("seed outside vector grid"));
            }
        }
        let (cu, ns) = (c as u64, seeds.len() as u64);
        let rows = grid.rows as u64 * grid.slices as u64;
        self.estimate = n
            + 20 * self.region_cap as u64
            + 4 * ns
            + 8 * (ns * cu)
            + 24 * rows
            + 512 * grid.slices as u64
            + (16 * cu * cu + 16 * cu) * 8
            + cu * 1024
            + 4096;
        if self.estimate > self.limits.max_estimated_bytes {
            return Err(resource_limit("structural byte limit"));
        }
        let bits_len = (usize::BITS - seeds.len().leading_zeros()) as u64;
        self.ledger.charge(
            n + self.region_cap as u64 + ns * (1 + 4 * bits_len) + (4 * cu * cu + 4 * cu),
        )?;
        self.seeds = vec![0u32; seeds.len()];
        for (i, s) in seeds.iter().enumerate() {
            bulk_poll(ctx, i)?;
            self.seeds[i] = ((s.slice * grid.rows + s.y) * grid.columns + s.x) as u32;
        }
        sort_seeds(ctx, &mut self.seeds)?;
        let mut unique = 0usize;
        for i in 0..self.seeds.len() {
            let index = self.seeds[i];
            bulk_poll(ctx, i)?;
            if unique == 0 || self.seeds[unique - 1] != index {
                self.seeds[unique] = index;
                unique += 1;
            }
        }
        self.seeds.truncate(unique);
        let side = (2 * initial_radius + 1) as u64;
        let mut bx = side * side;
        if grid.slices > 1 {
            bx *= side;
        }
        if bx * unique as u64 > self.limits.max_neighborhood_samples as u64 {
            return Err(resource_limit("initial neighborhood sample limit"));
        }
        let samples = bx * unique as u64;
        if samples * (3 * cu) > self.limits.max_work - self.ledger.work {
            return Err(resource_limit("initial sample work minimum"));
        }
        region_grow_context_error(ctx)?;
        self.state = vec![0u8; self.n];
        region_grow_context_error(ctx)?;
        self.queue = vec![0u32; self.region_cap];
        region_grow_context_error(ctx)?;
        self.seed_values = vec![0.0; unique * c];
        region_grow_context_error(ctx)?;
        self.model = VectorConfidenceModel::new(c);
        region_grow_context_error(ctx)
    }
    fn coordinates(&self, index: u32) -> (i64, i64, i64) {
        let mut n = index as i64;
        let x = n % self.volume.grid.columns;
        n /= self.volume.grid.columns;
        (x, n % self.volume.grid.rows, n / self.volume.grid.rows)
    }
    fn read(&mut self, index: u32, dst: &mut [f64]) -> Result<(), Error> {
        let ctx = self.ledger.ctx;
        self.ledger
            .charge((3 * self.volume.channels.len()) as u64)?;
        let (x, y, z) = self.coordinates(index);
        for (i, ch) in self.volume.channels.iter().enumerate() {
            region_grow_context_error(ctx)?;
            self.reads += 1;
            let value = ch.value_at.and_then(|f| f(x, y, z));
            region_grow_context_error(ctx)?;
            let Some(value) = value else {
                return Err(Error::wrap(
                    ERR_VECTOR_CONFIDENCE_UNAVAILABLE,
                    format_args!("channel {} at ({x},{y},{z})", quote(ch.id)),
                ));
            };
            let value = (value - ch.offset) / ch.scale;
            if !finite(value) || value.abs() > 1e100 {
                return Err(Error::wrap(
                    ERR_VECTOR_CONFIDENCE_NUMERIC,
                    format_args!("channel {} at ({x},{y},{z})", quote(ch.id)),
                ));
            }
            dst[i] = value;
        }
        Ok(())
    }
    fn read_values(&mut self, index: u32) -> Result<(), Error> {
        let mut values = std::mem::take(&mut self.model.values);
        let read = self.read(index, &mut values);
        self.model.values = values;
        read
    }
    fn initial_statistics(&mut self) -> Result<(), Error> {
        let r = self.params.initial_radius;
        let grid = self.volume.grid;
        let rz = if grid.slices == 1 { 0 } else { r };
        for s in 0..self.seeds.len() {
            let (x, y, z) = self.coordinates(self.seeds[s]);
            for dz in -rz..=rz {
                let zz = (z + dz).max(0).min(grid.slices - 1);
                for dy in -r..=r {
                    let yy = (y + dy).max(0).min(grid.rows - 1);
                    for dx in -r..=r {
                        let xx = (x + dx).max(0).min(grid.columns - 1);
                        self.read_values(((zz * grid.rows + yy) * grid.columns + xx) as u32)?;
                        self.model.add_values(&mut self.ledger)?;
                    }
                }
            }
        }
        self.model.covariance(&mut self.ledger, &self.params)
    }
    fn region_statistics(&mut self) -> Result<(), Error> {
        self.model.reset(&mut self.ledger)?;
        self.ledger.charge(self.n as u64)?;
        for i in 0..self.state.len() {
            if i % 1024 == 0 {
                region_grow_context_error(self.ledger.ctx)?;
            }
            if self.state[i] == 2 {
                self.read_values(i as u32)?;
                self.model.add_values(&mut self.ledger)?;
            }
        }
        self.model.covariance(&mut self.ledger, &self.params)
    }
    fn fill(&mut self) -> Result<(), Error> {
        self.ledger.charge(self.n as u64)?;
        for i in 0..self.state.len() {
            if i % 1024 == 0 {
                region_grow_context_error(self.ledger.ctx)?;
            }
            self.state[i] = 0;
        }
        self.count = 0;
        for s in 0..self.seeds.len() {
            self.visit(self.seeds[s])?;
        }
        let grid = self.volume.grid;
        let mut head = 0usize;
        while head < self.count {
            self.ledger.charge(1)?;
            let (x, y, z) = self.coordinates(self.queue[head]);
            for dz in -1i64..=1 {
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let d = dx.abs() + dy.abs() + dz.abs();
                        if d == 0
                            || self.connectivity == CONNECTIVITY_6 && d > 1
                            || self.connectivity == CONNECTIVITY_18 && d > 2
                        {
                            continue;
                        }
                        self.ledger.charge(1)?;
                        let (xx, yy, zz) = (x + dx, y + dy, z + dz);
                        if xx < 0
                            || yy < 0
                            || zz < 0
                            || xx >= grid.columns
                            || yy >= grid.rows
                            || zz >= grid.slices
                        {
                            continue;
                        }
                        self.visit(((zz * grid.rows + yy) * grid.columns + xx) as u32)?;
                    }
                }
            }
            head += 1;
        }
        Ok(())
    }
    fn visit(&mut self, index: u32) -> Result<(), Error> {
        self.ledger.charge(1)?;
        if self.state[index as usize] != 0 {
            return Ok(());
        }
        self.state[index as usize] = 1;
        self.read_values(index)?;
        let d = self.model.distance_of_values(&mut self.ledger)?;
        if d > self.effective {
            return Ok(());
        }
        if self.count == self.region_cap {
            return Err(resource_limit("vector region voxel limit"));
        }
        self.state[index as usize] = 2;
        self.queue[self.count] = index;
        self.count += 1;
        Ok(())
    }
    fn masks(&mut self) -> Result<BTreeMap<i64, RasterMask>, Error> {
        let grid = self.volume.grid;
        let ctx = self.ledger.ctx;
        self.ledger
            .charge((2 * self.n as i64 + grid.rows * grid.slices) as u64)?;
        let (mut runs, mut occupied) = (0i64, 0i64);
        for z in 0..grid.slices {
            let mut has_slice = false;
            for y in 0..grid.rows {
                let mut inside = false;
                for x in 0..grid.columns {
                    let index = ((z * grid.rows + y) * grid.columns + x) as usize;
                    if index.is_multiple_of(1024) {
                        region_grow_context_error(ctx)?;
                    }
                    let on = self.state[index] == 2;
                    if on && !inside {
                        runs += 1;
                        has_slice = true;
                    }
                    inside = on;
                }
            }
            if has_slice {
                occupied += 1;
            }
        }
        self.ledger.charge((runs + occupied * grid.rows) as u64)?;
        region_grow_context_error(ctx)?;
        let mut out: BTreeMap<i64, RasterMask> = BTreeMap::new();
        let mut visits = 0usize;
        for z in 0..grid.slices {
            let mut mask: Option<RasterMask> = None;
            for y in 0..grid.rows {
                let mut row: Vec<MaskRun> = Vec::new();
                let mut run_start = -1i64;
                for x in 0..=grid.columns {
                    let index = ((z * grid.rows + y) * grid.columns + x) as usize;
                    bulk_poll(ctx, visits)?;
                    visits += 1;
                    let on = x < grid.columns && self.state[index] == 2;
                    if on && run_start < 0 {
                        run_start = x;
                    }
                    if !on && run_start >= 0 {
                        row.push(MaskRun {
                            start: run_start,
                            end: x,
                        });
                        run_start = -1;
                    }
                }
                if !row.is_empty() {
                    if mask.is_none() {
                        region_grow_context_error(ctx)?;
                        mask = Some(RasterMask::new(grid.columns, grid.rows));
                    }
                    if let Some(m) = mask.as_mut() {
                        m.runs[y as usize] = row;
                    }
                }
            }
            if let Some(m) = mask {
                out.insert(z, m);
            }
        }
        region_grow_context_error(ctx)?;
        Ok(out)
    }
}
fn normalize_limits(mut l: VectorConfidenceLimits) -> Result<VectorConfidenceLimits, Error> {
    let d = default_vector_confidence_limits();
    let defaults = [
        d.max_channels,
        d.max_grid_voxels,
        d.max_region_voxels,
        d.max_neighborhood_samples,
        d.max_seeds,
        d.max_fills,
    ];
    let hard: [i64; 6] = [32, 1 << 26, 1 << 26, 1 << 24, 65536, 65];
    let values = [
        &mut l.max_channels,
        &mut l.max_grid_voxels,
        &mut l.max_region_voxels,
        &mut l.max_neighborhood_samples,
        &mut l.max_seeds,
        &mut l.max_fills,
    ];
    for (i, v) in values.into_iter().enumerate() {
        if *v == 0 {
            *v = defaults[i];
        }
        if *v < 0 || *v > hard[i] {
            return Err(invalid_parameters(
                "vector confidence limit outside hard profile",
            ));
        }
    }
    if l.max_work == 0 {
        l.max_work = d.max_work;
    }
    if l.max_estimated_bytes == 0 {
        l.max_estimated_bytes = d.max_estimated_bytes;
    }
    if l.max_work > 1 << 34 || l.max_estimated_bytes > 1 << 30 {
        return Err(invalid_parameters("work/byte limit outside hard profile"));
    }
    Ok(l)
}
fn validate_grid(grid: &VoxelGrid) -> Result<(), Error> {
    if let Err(err) = grid.validate() {
        return Err(Error::wrap(ERR_VECTOR_CONFIDENCE_GEOMETRY, err));
    }
    for x in [0.0, (grid.columns - 1) as f64] {
        for y in [0.0, (grid.rows - 1) as f64] {
            for z in [0.0, (grid.slices - 1) as f64] {
                let p = grid.index_to_point(x, y, z);
                let ok = match grid.point_to_index(p) {
                    Some(index) => finite_vec3(p) && finite_vec3(index),
                    None => false,
                };
                if !ok {
                    return Err(Error::wrap(
                        ERR_VECTOR_CONFIDENCE_GEOMETRY,
                        "nonfinite grid transform/corner",
                    ));
                }
            }
        }
    }
    Ok(())
}
fn product(values: &[u64]) -> Option<u64> {
    let mut n = 1u64;
    for &v in values {
        if v != 0 && n > u64::MAX / v {
            return None;
        }
        n *= v;
    }
    Some(n)
}
fn bulk_poll(ctx: Option<&dyn Cancellation>, visit: usize) -> Result<(), Error> {
    if visit.is_multiple_of(1024) {
        return region_grow_context_error(ctx);
    }
    Ok(())
}
fn sort_seeds(ctx: Option<&dyn Cancellation>, indices: &mut [u32]) -> Result<(), Error> {
    region_grow_context_error(ctx)?;
    let mut visits = 0usize;
    let mut poll = |visits: &mut usize| -> Result<(), Error> {
        let r = bulk_poll(ctx, *visits);
        *visits += 1;
        r
    };
    fn down(
        indices: &mut [u32],
        mut root: usize,
        end: usize,
        visits: &mut usize,
        poll: &mut dyn FnMut(&mut usize) -> Result<(), Error>,
    ) -> Result<(), Error> {
        loop {
            let mut child = 2 * root + 1;
            if child >= end {
                break;
            }
            poll(visits)?;
            if child + 1 < end && indices[child] < indices[child + 1] {
                child += 1;
            }
            if indices[root] >= indices[child] {
                break;
            }
            indices.swap(root, child);
            root = child;
        }
        Ok(())
    }
    let len = indices.len();
    for root in (0..len / 2).rev() {
        poll(&mut visits)?;
        down(indices, root, len, &mut visits, &mut poll)?;
    }
    for end in (1..len).rev() {
        poll(&mut visits)?;
        indices.swap(0, end);
        down(indices, 0, end, &mut visits, &mut poll)?;
    }
    region_grow_context_error(ctx)
}
