#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err};
use super::vec3::Vec3;
use super::volume::{Sampler, checked_voxel_count};
use super::volume_store::VolumeResidencyKey;
use super::vr_prepared::VrPreparedVolume;
use super::vr_transfer::{VrPrefilterId, finite};
use crate::fmath;
use crate::textfmt::float_v;
use std::sync::{Arc, Mutex};
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub const DEFAULT_VR_HISTOGRAM_BINS: i64 = 256;
const DEFAULT_VR_HISTOGRAM_CACHE_ENTRIES: usize = 8;
const MAXIMUM_VR_HISTOGRAM_BINS: i64 = 65536;
pub const MAX_VR_JOINT_HISTOGRAM_SAMPLES: u64 = 1 << 21;
const VR_JOINT_HISTOGRAM_GRADIENT_PERCENTILE: f64 = 0.99;
fn invalid() -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        "render: invalid volume snapshot",
    )
}
fn invalid_detail(detail: &str) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        format!("render: invalid volume snapshot: {detail}"),
    )
}
fn check(ctx: Ctx<'_>) -> Result<(), RenderError> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VrHistogramDescriptor {
    pub bins: i64,
    pub domain_min: f64,
    pub domain_max: f64,
    pub prefilter: VrPrefilterId,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VrHistogramKey {
    pub prepared_identity: VolumeResidencyKey,
    pub prefilter: VrPrefilterId,
    pub bins: i64,
    pub domain_min: f64,
    pub domain_max: f64,
}
impl VrHistogramKey {
    pub fn valid(&self) -> bool {
        self.prepared_identity.valid()
            && self.bins > 0
            && finite(self.domain_min)
            && finite(self.domain_max)
            && self.domain_max >= self.domain_min
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct VrHistogram {
    pub descriptor: VrHistogramDescriptor,
    pub counts: Vec<u64>,
    pub voxel_count: u64,
    pub peak_bin: i64,
    pub generation: u64,
    pub key: VrHistogramKey,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VrHistogramCacheOptions {
    pub max_entries: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VrHistogramCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub builds: u64,
    pub evictions: u64,
    pub entries: i64,
}
struct Entry {
    key: VrHistogramKey,
    histogram: VrHistogram,
    last_used: u64,
    _keep: Arc<VrPreparedVolume>,
}
#[derive(Default)]
struct CacheState {
    clock: u64,
    entries: Vec<Entry>,
    hits: u64,
    misses: u64,
    builds: u64,
    evictions: u64,
}
pub struct VrHistogramCache {
    max_entries: usize,
    state: Mutex<CacheState>,
}
fn same_key(a: &VrHistogramKey, b: &VrHistogramKey) -> bool {
    a.prepared_identity == b.prepared_identity
        && a.prefilter == b.prefilter
        && a.bins == b.bins
        && a.domain_min == b.domain_min
        && a.domain_max == b.domain_max
}
impl VrHistogramCache {
    pub fn new(options: VrHistogramCacheOptions) -> VrHistogramCache {
        let max_entries = if options.max_entries > 0 {
            options.max_entries as usize
        } else {
            DEFAULT_VR_HISTOGRAM_CACHE_ENTRIES
        };
        VrHistogramCache {
            max_entries,
            state: Mutex::new(CacheState::default()),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, CacheState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn histogram(
        &self,
        prepared: &Arc<VrPreparedVolume>,
        bins: i64,
    ) -> Result<VrHistogram, RenderError> {
        self.histogram_context(None, prepared, bins)
    }
    pub fn histogram_context(
        &self,
        ctx: Ctx<'_>,
        prepared: &Arc<VrPreparedVolume>,
        bins: i64,
    ) -> Result<VrHistogram, RenderError> {
        check(ctx)?;
        let (descriptor, key) = histogram_descriptor(ctx, prepared, bins)?;
        {
            let mut s = self.lock();
            s.clock += 1;
            let clock = s.clock;
            if let Some(i) = s.entries.iter().position(|e| same_key(&e.key, &key)) {
                s.entries[i].last_used = clock;
                s.hits += 1;
                return Ok(s.entries[i].histogram.clone());
            }
            s.misses += 1;
            s.builds += 1;
        }
        let mut result = compute_histogram(ctx, prepared, descriptor);
        if let Ok(h) = &mut result {
            h.descriptor = descriptor;
            h.key = key;
            h.generation = key.prepared_identity.generation;
            if let Err(e) = validate_histogram(h) {
                result = Err(e);
            }
        }
        if result.is_ok()
            && let Some(e) = ctx_err(ctx)
        {
            result = Err(e);
        }
        let histogram = result?;
        let mut s = self.lock();
        let clock = s.clock;
        s.entries.push(Entry {
            key,
            histogram: histogram.clone(),
            last_used: clock,
            _keep: Arc::clone(prepared),
        });
        self.evict_locked(&mut s, &key);
        Ok(histogram)
    }
    fn evict_locked(&self, s: &mut CacheState, keep: &VrHistogramKey) {
        while s.entries.len() > self.max_entries {
            let oldest = s
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| !same_key(&e.key, keep))
                .min_by_key(|(_, e)| e.last_used)
                .map(|(i, _)| i);
            let Some(i) = oldest else {
                return;
            };
            s.entries.remove(i);
            s.evictions += 1;
        }
    }
    pub fn stats(&self) -> VrHistogramCacheStats {
        let s = self.lock();
        VrHistogramCacheStats {
            hits: s.hits,
            misses: s.misses,
            builds: s.builds,
            evictions: s.evictions,
            entries: s.entries.len() as i64,
        }
    }
    pub fn clear(&self) {
        self.lock().entries.clear();
    }
}
pub(crate) fn prepared_residency_key(prepared: &Arc<VrPreparedVolume>) -> VolumeResidencyKey {
    VolumeResidencyKey {
        store_identity: Arc::as_ptr(prepared) as usize as u64,
        generation: 1,
    }
}
fn histogram_descriptor(
    ctx: Ctx<'_>,
    prepared: &Arc<VrPreparedVolume>,
    bins: i64,
) -> Result<(VrHistogramDescriptor, VrHistogramKey), RenderError> {
    if bins <= 0 || bins > MAXIMUM_VR_HISTOGRAM_BINS {
        return Err(RenderError::other(format!(
            "render: invalid VR histogram bin count {bins}"
        )));
    }
    let (minimum, maximum) = intensity_range(ctx, prepared)?;
    let descriptor = VrHistogramDescriptor {
        bins,
        domain_min: minimum,
        domain_max: maximum,
        prefilter: prepared.prefilter,
    };
    let key = VrHistogramKey {
        prepared_identity: prepared_residency_key(prepared),
        prefilter: prepared.prefilter,
        bins,
        domain_min: minimum,
        domain_max: maximum,
    };
    if !key.valid() {
        return Err(invalid());
    }
    Ok((descriptor, key))
}
pub fn intensity_range(
    ctx: Ctx<'_>,
    prepared: &VrPreparedVolume,
) -> Result<(f64, f64), RenderError> {
    check(ctx)?;
    let (mut minimum, mut maximum) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut count = 0u64;
    visit_voxels(ctx, prepared, |value| {
        minimum = fmath::min(minimum, value);
        maximum = fmath::max(maximum, value);
        count += 1;
        Ok(())
    })?;
    if count == 0 {
        return Err(invalid());
    }
    Ok((minimum, maximum))
}
fn visit_voxels(
    ctx: Ctx<'_>,
    prepared: &VrPreparedVolume,
    mut visit: impl FnMut(f64) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let sampler = prepared.new_sampler()?;
    for z in 0..sampler.depth {
        for y in 0..sampler.rows {
            check(ctx)?;
            for x in 0..sampler.cols {
                match sampler.value_at(x, y, z) {
                    Some(value) if finite(value) => visit(value)?,
                    _ => return Err(invalid_detail("invalid prepared voxel")),
                }
            }
        }
    }
    Ok(())
}
fn compute_histogram(
    ctx: Ctx<'_>,
    prepared: &VrPreparedVolume,
    descriptor: VrHistogramDescriptor,
) -> Result<VrHistogram, RenderError> {
    let bins = descriptor.bins as usize;
    let mut counts = vec![0u64; bins];
    let mut voxel_count = 0u64;
    visit_voxels(ctx, prepared, |value| {
        let mut index = 0usize;
        if descriptor.domain_max > descriptor.domain_min {
            let u =
                (value - descriptor.domain_min) / (descriptor.domain_max - descriptor.domain_min);
            index = if u <= 0.0 {
                0
            } else if u >= 1.0 {
                bins - 1
            } else {
                (fmath::f64_to_i64((u * bins as f64).floor()) as usize).min(bins - 1)
            };
        }
        counts[index] += 1;
        voxel_count += 1;
        Ok(())
    })?;
    let v = &prepared.volume;
    match checked_voxel_count(v.cols, v.rows, v.depth) {
        Some(expected) if voxel_count == expected as u64 => {}
        _ => return Err(invalid_detail("VR histogram voxel count mismatch")),
    }
    let mut peak = 0usize;
    for index in 1..counts.len() {
        if counts[index] > counts[peak] {
            peak = index;
        }
    }
    Ok(VrHistogram {
        descriptor,
        counts,
        voxel_count,
        peak_bin: peak as i64,
        generation: 0,
        key: VrHistogramKey {
            prepared_identity: VolumeResidencyKey::default(),
            prefilter: descriptor.prefilter,
            bins: 0,
            domain_min: 0.0,
            domain_max: 0.0,
        },
    })
}
fn validate_histogram(h: &VrHistogram) -> Result<(), RenderError> {
    if !h.key.valid()
        || h.descriptor.bins != h.counts.len() as i64
        || h.descriptor.bins != h.key.bins
        || h.generation == 0
        || h.peak_bin < 0
        || h.peak_bin >= h.counts.len() as i64
    {
        return Err(invalid());
    }
    let mut sum = 0u64;
    for count in &h.counts {
        if u64::MAX - sum < *count {
            return Err(RenderError::other("render: VR histogram total overflow"));
        }
        sum += count;
    }
    if sum != h.voxel_count {
        return Err(invalid_detail("VR histogram count mismatch"));
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq)]
pub struct VrJointHistogram {
    pub intensity_bins: i64,
    pub gradient_bins: i64,
    pub intensity_min: f64,
    pub intensity_max: f64,
    pub gradient_max: f64,
    pub counts: Vec<u64>,
    pub samples: u64,
    pub stride: i64,
}
impl VrJointHistogram {
    pub fn at(&self, column: i64, row: i64) -> u64 {
        if column < 0
            || row < 0
            || column >= self.intensity_bins
            || row >= self.gradient_bins
            || self.counts.len() as i64 != self.intensity_bins * self.gradient_bins
        {
            return 0;
        }
        self.counts[(row * self.intensity_bins + column) as usize]
    }
}
fn value_clamped(s: &Sampler, col: i64, row: i64, slice: i64) -> f64 {
    s.value_or_zero(
        col.min(s.cols - 1).max(0),
        row.min(s.rows - 1).max(0),
        slice.min(s.depth - 1).max(0),
    )
}
fn ceil_div(n: i64, d: i64) -> i64 {
    (n + d - 1) / d
}
pub fn joint_histogram_context(
    ctx: Ctx<'_>,
    prepared: &VrPreparedVolume,
    intensity_bins: i64,
    gradient_bins: i64,
    gradient_max: f64,
) -> Result<VrJointHistogram, RenderError> {
    if intensity_bins < 2 || gradient_bins < 2 || intensity_bins > 4096 || gradient_bins > 4096 {
        return Err(RenderError::other(format!(
            "render: invalid VR joint histogram bins {intensity_bins}x{gradient_bins}"
        )));
    }
    if gradient_max.is_nan() || gradient_max.is_infinite() {
        return Err(RenderError::other(format!(
            "render: invalid VR joint histogram gradient maximum {}",
            float_v(gradient_max)
        )));
    }
    let (minimum, maximum) = intensity_range(ctx, prepared)?;
    let sampler = prepared.new_sampler()?;
    let mut stride = 1i64;
    loop {
        let count = ceil_div(sampler.cols, stride) as u64
            * ceil_div(sampler.rows, stride) as u64
            * ceil_div(sampler.depth, stride) as u64;
        if count <= MAX_VR_JOINT_HISTOGRAM_SAMPLES {
            break;
        }
        stride += 1;
    }
    let (nx, ny, nz) = (
        ceil_div(sampler.cols, stride),
        ceil_div(sampler.rows, stride),
        ceil_div(sampler.depth, stride),
    );
    let n = (nx * ny * nz) as usize;
    let mut values = vec![0f32; n];
    let mut magnitudes = vec![0f32; n];
    let vol = &sampler.vol;
    for k in 0..nz {
        check(ctx)?;
        let z = k * stride;
        for j in 0..ny {
            let y = j * stride;
            for i in 0..nx {
                let x = i * stride;
                let index = ((k * ny + j) * nx + i) as usize;
                values[index] = value_clamped(&sampler, x, y, z) as f32;
                let gradient = Vec3::new(
                    value_clamped(&sampler, x + 1, y, z) - value_clamped(&sampler, x - 1, y, z),
                    value_clamped(&sampler, x, y + 1, z) - value_clamped(&sampler, x, y - 1, z),
                    value_clamped(&sampler, x, y, z + 1) - value_clamped(&sampler, x, y, z - 1),
                );
                magnitudes[index] = vol.physical_gradient_magnitude(gradient) as f32;
            }
        }
    }
    let mut gradient_max = gradient_max;
    if !(gradient_max > 0.0) {
        let mut sorted = magnitudes.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        gradient_max = 1.0;
        if !sorted.is_empty() {
            let at = (VR_JOINT_HISTOGRAM_GRADIENT_PERCENTILE * (sorted.len() - 1) as f64) as usize;
            gradient_max = fmath::max(1.0, f64::from(sorted[at]));
        }
    }
    let bin = |u: f64, n: i64| -> i64 {
        if !(u > 0.0) {
            return 0;
        }
        fmath::f64_to_i64(u * n as f64).min(n - 1)
    };
    let mut counts = vec![0u64; (intensity_bins * gradient_bins) as usize];
    for (index, value) in values.iter().enumerate() {
        let mut column = 0;
        if maximum > minimum {
            column = bin(
                (f64::from(*value) - minimum) / (maximum - minimum),
                intensity_bins,
            );
        }
        let row = bin(f64::from(magnitudes[index]) / gradient_max, gradient_bins);
        counts[(row * intensity_bins + column) as usize] += 1;
    }
    check(ctx)?;
    Ok(VrJointHistogram {
        intensity_bins,
        gradient_bins,
        intensity_min: minimum,
        intensity_max: maximum,
        gradient_max,
        counts,
        samples: n as u64,
        stride,
    })
}
