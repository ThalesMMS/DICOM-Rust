use super::distance3d::{ERR_INVALID_VOLUME_MASK, VolumeMask};
use super::polygon_raster::finite;
use super::texture::{ERR_INVALID_TEXTURE, TEXTURE_DIRECTIONS, TextureAggregation};
use super::voi::VoxelValueFn;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::parallel;
use std::borrow::Cow;
use std::collections::HashSet;
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LocalTextureFeature(pub Cow<'static, str>);
impl LocalTextureFeature {
    pub const CONTRAST: LocalTextureFeature = LocalTextureFeature(Cow::Borrowed("Contrast"));
    pub const ENTROPY: LocalTextureFeature = LocalTextureFeature(Cow::Borrowed("Joint entropy"));
    pub const ENERGY: LocalTextureFeature =
        LocalTextureFeature(Cow::Borrowed("Angular second moment"));
    pub const HOMOGENEITY: LocalTextureFeature =
        LocalTextureFeature(Cow::Borrowed("Inverse difference moment"));
}
pub const LOCAL_TEXTURE_MAX_VOXELS: i64 = 1 << 20;
pub const LOCAL_TEXTURE_MAX_BYTES: i64 = 256 << 20;
pub const LOCAL_TEXTURE_MAX_VISITS: i64 = 200_000_000;
pub const ERR_LOCAL_TEXTURE_LIMIT: &str = "roi: local texture resource limit";
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LocalTextureQuantization(pub Cow<'static, str>);
impl LocalTextureQuantization {
    pub const GLOBAL: LocalTextureQuantization =
        LocalTextureQuantization(Cow::Borrowed("global minimum"));
    pub const WINDOW: LocalTextureQuantization =
        LocalTextureQuantization(Cow::Borrowed("window minimum"));
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LocalTextureRegion {
    pub min: [i64; 3],
    pub max: [i64; 3],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalTextureOptions {
    pub feature: LocalTextureFeature,
    pub radius_mm: [f64; 3],
    pub bin_width: f64,
    pub lower_bound: Option<f64>,
    pub quantization: LocalTextureQuantization,
    pub aggregation: TextureAggregation,
    pub directions: Vec<[i64; 3]>,
    pub max_grey_levels: i64,
    pub workers: i64,
    pub max_working_bytes: i64,
    pub max_visits: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalTexturePlan {
    pub region: LocalTextureRegion,
    pub halo: LocalTextureRegion,
    pub dimensions: [i64; 3],
    pub radius_voxels: [i64; 3],
    pub workers: i64,
    pub reserved_bytes: i64,
    pub worst_case_visits: i64,
    pub options: LocalTextureOptions,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalTextureMap {
    pub plan: LocalTexturePlan,
    pub values: Vec<f32>,
    pub global_minimum: f64,
}
impl LocalTextureOptions {
    fn normalized(mut self) -> (LocalTextureOptions, Option<Error>) {
        if self.feature.0.is_empty() {
            self.feature = LocalTextureFeature::CONTRAST;
        }
        if self.quantization.0.is_empty() {
            self.quantization = LocalTextureQuantization::GLOBAL;
        }
        if self.max_grey_levels == 0 {
            self.max_grey_levels = 16;
        }
        if self.workers == 0 {
            self.workers = parallel::default_workers().min(16);
        }
        if self.max_working_bytes == 0 {
            self.max_working_bytes = LOCAL_TEXTURE_MAX_BYTES;
        }
        if self.max_visits == 0 {
            self.max_visits = LOCAL_TEXTURE_MAX_VISITS;
        }
        let mut o = self;
        if o.feature != LocalTextureFeature::CONTRAST
            && o.feature != LocalTextureFeature::ENTROPY
            && o.feature != LocalTextureFeature::ENERGY
            && o.feature != LocalTextureFeature::HOMOGENEITY
            || !finite(o.bin_width)
            || o.bin_width <= 0.0
            || o.lower_bound.is_some_and(|v| !finite(v))
            || o.quantization != LocalTextureQuantization::GLOBAL
                && o.quantization != LocalTextureQuantization::WINDOW
            || o.aggregation != TextureAggregation::AVERAGED
                && o.aggregation != TextureAggregation::MERGED
        {
            return (o, Some(Error::sentinel(ERR_INVALID_TEXTURE)));
        }
        if o.max_grey_levels < 1
            || o.max_grey_levels > 256
            || o.workers < 1
            || o.workers > 16
            || o.max_working_bytes < 1
            || o.max_working_bytes > LOCAL_TEXTURE_MAX_BYTES
            || o.max_visits < 1
            || o.max_visits > LOCAL_TEXTURE_MAX_VISITS
        {
            return (o, Some(Error::sentinel(ERR_LOCAL_TEXTURE_LIMIT)));
        }
        let dirs: Vec<[i64; 3]> = if o.directions.is_empty() {
            TEXTURE_DIRECTIONS.to_vec()
        } else {
            std::mem::take(&mut o.directions)
        };
        if dirs.len() > 13 {
            o.directions = dirs;
            return (o, Some(Error::sentinel(ERR_LOCAL_TEXTURE_LIMIT)));
        }
        o.directions = Vec::with_capacity(dirs.len());
        let mut seen: HashSet<[i64; 3]> = HashSet::new();
        for mut d in dirs {
            let mut last: Option<usize> = None;
            for (a, v) in d.into_iter().enumerate() {
                if !(-32..=32).contains(&v) {
                    return (o, Some(Error::sentinel(ERR_INVALID_TEXTURE)));
                }
                if v != 0 {
                    last = Some(a);
                }
            }
            let Some(last) = last else {
                return (o, Some(Error::sentinel(ERR_INVALID_TEXTURE)));
            };
            if d[last] < 0 {
                for v in d.iter_mut() {
                    *v = -*v;
                }
            }
            if !seen.insert(d) {
                return (
                    o,
                    Some(Error::wrap(
                        ERR_INVALID_TEXTURE,
                        "duplicate symmetric direction",
                    )),
                );
            }
            o.directions.push(d);
        }
        (o, None)
    }
}
impl VolumeMask {
    pub fn plan_local_texture(
        &self,
        region: Option<&LocalTextureRegion>,
        options: LocalTextureOptions,
    ) -> (LocalTexturePlan, Option<Error>) {
        let m = self;
        let (o, err) = options.normalized();
        if let Some(err) = err {
            return (LocalTexturePlan::default(), Some(err));
        }
        let mut p = LocalTexturePlan {
            dimensions: [m.columns, m.rows, m.slices],
            workers: o.workers,
            options: o,
            ..LocalTexturePlan::default()
        };
        let limit = |p: LocalTexturePlan| (p, Some(Error::sentinel(ERR_LOCAL_TEXTURE_LIMIT)));
        let mut n = 1i64;
        for d in p.dimensions {
            if !(1..=LOCAL_TEXTURE_MAX_VOXELS).contains(&d) || n > LOCAL_TEXTURE_MAX_VOXELS / d {
                return limit(p);
            }
            n *= d;
        }
        if m.voxels.len() as i64 != n {
            return (p, Some(Error::sentinel(ERR_INVALID_VOLUME_MASK)));
        }
        p.region.max = p.dimensions;
        if let Some(region) = region {
            p.region = *region;
        }
        let (mut window, mut halo, mut out) = (1i64, 1i64, 1i64);
        for (a, axis) in [&m.x, &m.y, &m.z].into_iter().enumerate() {
            let d = p.dimensions[a];
            let radius = p.options.radius_mm[a];
            if axis.len() as i64 != d || !finite(radius) || radius < 0.0 || radius > 32.0 {
                return (p, Some(Error::sentinel(ERR_INVALID_TEXTURE)));
            }
            let mut step = 1.0f64;
            if d > 1 {
                step = axis[1] - axis[0];
                if !finite(step) || step <= 0.0 {
                    return (p, Some(Error::sentinel(ERR_INVALID_VOLUME_MASK)));
                }
            }
            for (i, &v) in axis.iter().enumerate() {
                if !finite(v) || i > 0 && (v - axis[i - 1] - step).abs() > 1e-6 * step {
                    return (
                        p,
                        Some(Error::wrap(ERR_INVALID_VOLUME_MASK, "irregular axis")),
                    );
                }
            }
            if d > 1 {
                let r = (radius / step + 1e-10).floor();
                if !finite(r) || r > 32.0 {
                    return limit(p);
                }
                p.radius_voxels[a] = fmath::f64_to_i64(r);
            }
            let (lo, hi) = (p.region.min[a], p.region.max[a]);
            if lo < 0 || hi <= lo || hi > d {
                return (p, Some(Error::sentinel(ERR_INVALID_TEXTURE)));
            }
            p.halo.min[a] = (lo - p.radius_voxels[a]).max(0);
            p.halo.max[a] = (hi + p.radius_voxels[a]).min(d);
            window *= d.min(2 * p.radius_voxels[a] + 1);
            halo *= p.halo.max[a] - p.halo.min[a];
            out *= hi - lo;
        }
        p.workers = p.workers.min(out);
        let bins = p.options.max_grey_levels;
        p.reserved_bytes = n
            + 8 * (m.columns + m.rows + m.slices)
            + 8 * halo
            + 4 * out
            + p.workers * (16 * bins * bins + 4096)
            + 65536;
        p.worst_case_visits =
            2 * n + out * (window + p.options.directions.len() as i64 * (window + 3 * bins * bins));
        if p.reserved_bytes > p.options.max_working_bytes
            || p.worst_case_visits > p.options.max_visits
        {
            return limit(p);
        }
        (p, None)
    }
    pub fn local_texture(
        &self,
        ctx: Option<&dyn Cancellation>,
        value_at: Option<&mut VoxelValueFn<'_>>,
        region: Option<&LocalTextureRegion>,
        options: LocalTextureOptions,
    ) -> Result<LocalTextureMap, Error> {
        let m = self;
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        let Some(value_at) = value_at else {
            return Err(Error::sentinel(ERR_INVALID_TEXTURE));
        };
        let (p, err) = m.plan_local_texture(region, options);
        if let Some(err) = err {
            return Err(err);
        }
        let o = &p.options;
        let window_mode = o.quantization == LocalTextureQuantization::WINDOW;
        let global_mode = o.quantization == LocalTextureQuantization::GLOBAL;
        let averaged = o.aggregation == TextureAggregation::AVERAGED;
        let hd = [
            p.halo.max[0] - p.halo.min[0],
            p.halo.max[1] - p.halo.min[1],
            p.halo.max[2] - p.halo.min[2],
        ];
        let mut cache = vec![f64::NAN; (hd[0] * hd[1] * hd[2]) as usize];
        let halo_index = |x: i64, y: i64, z: i64| -> usize {
            (((z - p.halo.min[2]) * hd[1] + y - p.halo.min[1]) * hd[0] + x - p.halo.min[0]) as usize
        };
        let (mut minimum, mut maximum) = (f64::INFINITY, f64::NEG_INFINITY);
        for z in 0..m.slices {
            for y in 0..m.rows {
                if let Some(err) = ctx_err(ctx) {
                    return Err(err);
                }
                for x in 0..m.columns {
                    let in_halo = x >= p.halo.min[0]
                        && x < p.halo.max[0]
                        && y >= p.halo.min[1]
                        && y < p.halo.max[1]
                        && z >= p.halo.min[2]
                        && z < p.halo.max[2];
                    if !m.voxels[((z * m.rows + y) * m.columns + x) as usize]
                        || !in_halo && window_mode
                    {
                        continue;
                    }
                    let Some(v) = value_at(x, y, z) else {
                        continue;
                    };
                    if !finite(v) || o.lower_bound.is_some_and(|lower| v < lower) {
                        continue;
                    }
                    minimum = fmath::min(minimum, v);
                    maximum = fmath::max(maximum, v);
                    if in_halo {
                        cache[halo_index(x, y, z)] = v;
                    }
                }
            }
        }
        let mut base = minimum;
        if global_mode && let Some(lower) = o.lower_bound {
            base = lower;
        }
        if global_mode
            && finite(maximum)
            && ((maximum - base) / o.bin_width).floor() >= o.max_grey_levels as f64
        {
            return Err(Error::wrap(ERR_LOCAL_TEXTURE_LIMIT, "increase bin width"));
        }
        let d = [
            p.region.max[0] - p.region.min[0],
            p.region.max[1] - p.region.min[1],
            p.region.max[2] - p.region.min[2],
        ];
        let mut values = vec![f32::NAN; (d[0] * d[1] * d[2]) as usize];
        let global_minimum = if window_mode { f64::NAN } else { base };
        let at = |x: i64, y: i64, z: i64| -> f64 { cache[halo_index(x, y, z)] };
        let bins = o.max_grey_levels;
        let size = (bins * bins) as usize;
        let (mut hist, mut merged) = (vec![0.0f64; size], vec![0.0f64; size]);
        for index in 0..values.len() as i64 {
            let (x, y, z) = (
                index % d[0] + p.region.min[0],
                index / d[0] % d[1] + p.region.min[1],
                index / (d[0] * d[1]) + p.region.min[2],
            );
            if !finite(at(x, y, z)) {
                continue;
            }
            let (mut lo, mut hi) = ([0i64; 3], [0i64; 3]);
            for (a, c) in [x, y, z].into_iter().enumerate() {
                lo[a] = (c - p.radius_voxels[a]).max(0);
                hi[a] = (c + p.radius_voxels[a] + 1).min(p.dimensions[a]);
            }
            let mut origin = base;
            if window_mode {
                origin = f64::INFINITY;
                let mut window_maximum = f64::NEG_INFINITY;
                for k in lo[2]..hi[2] {
                    for j in lo[1]..hi[1] {
                        for i in lo[0]..hi[0] {
                            let v = at(i, j, k);
                            if finite(v) {
                                origin = fmath::min(origin, v);
                                window_maximum = fmath::max(window_maximum, v);
                            }
                        }
                    }
                }
                if ((window_maximum - origin) / o.bin_width).floor() >= bins as f64 {
                    return Err(Error::wrap(
                        ERR_LOCAL_TEXTURE_LIMIT,
                        "increase window bin width",
                    ));
                }
            }
            let level = |v: f64| -> Result<Option<usize>, Error> {
                if !finite(v) {
                    return Ok(None);
                }
                let b = ((v - origin) / o.bin_width).floor();
                if !finite(b) || b < 0.0 || b >= bins as f64 {
                    return Err(Error::wrap(
                        ERR_LOCAL_TEXTURE_LIMIT,
                        "window grey levels exceed limit",
                    ));
                }
                Ok(Some(b as usize))
            };
            merged.fill(0.0);
            let (mut total, mut directions, mut sum) = (0.0f64, 0i64, 0.0f64);
            for dir in &o.directions {
                hist.fill(0.0);
                let mut pairs = 0.0f64;
                for k in lo[2]..hi[2] {
                    for j in lo[1]..hi[1] {
                        for i in lo[0]..hi[0] {
                            let (u, v, w) = (i + dir[0], j + dir[1], k + dir[2]);
                            if u < lo[0]
                                || u >= hi[0]
                                || v < lo[1]
                                || v >= hi[1]
                                || w < lo[2]
                                || w >= hi[2]
                            {
                                continue;
                            }
                            let a = level(at(i, j, k));
                            let b = level(at(u, v, w));
                            let (Some(a), Some(b)) = (a?, b?) else {
                                continue;
                            };
                            hist[a * bins as usize + b] += 1.0;
                            hist[b * bins as usize + a] += 1.0;
                            pairs += 2.0;
                        }
                    }
                }
                if pairs == 0.0 {
                    continue;
                }
                directions += 1;
                if averaged {
                    sum += local_texture_feature(&hist, pairs, bins as usize, &o.feature);
                } else {
                    total += pairs;
                    for (i, v) in hist.iter().enumerate() {
                        merged[i] += v;
                    }
                }
            }
            if directions > 0 {
                values[index as usize] = if averaged {
                    (sum / directions as f64) as f32
                } else {
                    local_texture_feature(&merged, total, bins as usize, &o.feature) as f32
                };
            }
        }
        if let Some(err) = ctx_err(ctx) {
            return Err(ctx_err(ctx).unwrap_or(err));
        }
        Ok(LocalTextureMap {
            plan: p,
            values,
            global_minimum,
        })
    }
}
fn local_texture_feature(
    hist: &[f64],
    total: f64,
    bins: usize,
    feature: &LocalTextureFeature,
) -> f64 {
    #[derive(Clone, Copy)]
    enum Kind {
        Contrast,
        Entropy,
        Energy,
        Homogeneity,
        Other,
    }
    let kind = if *feature == LocalTextureFeature::CONTRAST {
        Kind::Contrast
    } else if *feature == LocalTextureFeature::ENTROPY {
        Kind::Entropy
    } else if *feature == LocalTextureFeature::ENERGY {
        Kind::Energy
    } else if *feature == LocalTextureFeature::HOMOGENEITY {
        Kind::Homogeneity
    } else {
        Kind::Other
    };
    let mut sum = 0.0f64;
    for (i, &c) in hist.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let p = c / total;
        let delta = (i / bins) as f64 - (i % bins) as f64;
        match kind {
            Kind::Contrast => sum += p * delta * delta,
            Kind::Entropy => sum -= p * fmath::log2(p),
            Kind::Energy => sum += p * p,
            Kind::Homogeneity => sum += p / (1.0 + delta * delta),
            Kind::Other => {}
        }
    }
    sum
}
