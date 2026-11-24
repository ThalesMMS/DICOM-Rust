#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::distance3d::VolumeMask;
use super::polygon_raster::finite;
use super::voi::{VoiMask, VoxelValueFn};
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TextureAggregation(pub i64);
impl TextureAggregation {
    pub const AVERAGED: TextureAggregation = TextureAggregation(0);
    pub const MERGED: TextureAggregation = TextureAggregation(1);
}
pub const DEFAULT_MAX_TEXTURE_GREY_LEVELS: i64 = 1024;
pub const ERR_INVALID_TEXTURE: &str = "roi: invalid texture input";
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextureOptions {
    pub bin_width: f64,
    pub lower_bound: Option<f64>,
    pub distance: i64,
    pub aggregation: TextureAggregation,
    pub max_grey_levels: i64,
    pub workers: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlcmFeatures {
    pub joint_maximum: f64,
    pub joint_average: f64,
    pub joint_variance: f64,
    pub joint_entropy: f64,
    pub difference_average: f64,
    pub difference_variance: f64,
    pub difference_entropy: f64,
    pub sum_average: f64,
    pub sum_variance: f64,
    pub sum_entropy: f64,
    pub angular_second_moment: f64,
    pub contrast: f64,
    pub dissimilarity: f64,
    pub inverse_difference: f64,
    pub normalised_inverse_difference: f64,
    pub inverse_difference_moment: f64,
    pub normalised_inverse_difference_moment: f64,
    pub inverse_variance: f64,
    pub correlation: f64,
    pub autocorrelation: f64,
    pub cluster_tendency: f64,
    pub cluster_shade: f64,
    pub cluster_prominence: f64,
    pub information_correlation_1: f64,
    pub information_correlation_2: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlrlmFeatures {
    pub short_runs_emphasis: f64,
    pub long_runs_emphasis: f64,
    pub low_grey_level_run_emphasis: f64,
    pub high_grey_level_run_emphasis: f64,
    pub short_run_low_grey_level_emphasis: f64,
    pub short_run_high_grey_level_emphasis: f64,
    pub long_run_low_grey_level_emphasis: f64,
    pub long_run_high_grey_level_emphasis: f64,
    pub grey_level_non_uniformity: f64,
    pub normalised_grey_level_non_uniformity: f64,
    pub run_length_non_uniformity: f64,
    pub normalised_run_length_non_uniformity: f64,
    pub run_percentage: f64,
    pub grey_level_variance: f64,
    pub run_length_variance: f64,
    pub run_entropy: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextureFeatures {
    pub voxels: i64,
    pub excluded: i64,
    pub non_finite: i64,
    pub grey_levels: i64,
    pub bin_width: f64,
    pub lower_bound: f64,
    pub glcm_directions: i64,
    pub glrlm_directions: i64,
    pub glcm: GlcmFeatures,
    pub glrlm: GlrlmFeatures,
}
pub(crate) const TEXTURE_DIRECTIONS: [[i64; 3]; 13] = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 1, 0],
    [1, -1, 0],
    [1, 0, 1],
    [1, 0, -1],
    [0, 1, 1],
    [0, 1, -1],
    [1, 1, 1],
    [1, 1, -1],
    [1, -1, 1],
    [1, -1, -1],
];
struct TextureGrid {
    dims: [i64; 3],
    level: Vec<i32>,
    levels: i64,
}
impl TextureGrid {
    fn at(&self, x: i64, y: i64, z: i64) -> i32 {
        if x < 0 || y < 0 || z < 0 || x >= self.dims[0] || y >= self.dims[1] || z >= self.dims[2] {
            return 0;
        }
        self.level[((z * self.dims[1] + y) * self.dims[0] + x) as usize]
    }
}
struct TextureVoxel {
    x: i64,
    y: i64,
    z: i64,
    value: f64,
}
impl TextureOptions {
    pub(crate) fn normalized(mut self) -> Result<TextureOptions, Error> {
        if self.distance == 0 {
            self.distance = 1;
        }
        if self.max_grey_levels == 0 {
            self.max_grey_levels = DEFAULT_MAX_TEXTURE_GREY_LEVELS;
        }
        let o = self;
        if !(o.bin_width > 0.0) || o.bin_width.is_infinite() {
            return Err(Error::wrap(
                ERR_INVALID_TEXTURE,
                format!("bin width {}", float_v(o.bin_width)),
            ));
        }
        if let Some(lower) = o.lower_bound
            && !finite(lower)
        {
            return Err(Error::wrap(
                ERR_INVALID_TEXTURE,
                format!("lower bound {}", float_v(lower)),
            ));
        }
        if o.distance < 1 {
            return Err(Error::wrap(
                ERR_INVALID_TEXTURE,
                format!("distance {}", o.distance),
            ));
        }
        if o.aggregation != TextureAggregation::AVERAGED
            && o.aggregation != TextureAggregation::MERGED
        {
            return Err(Error::wrap(
                ERR_INVALID_TEXTURE,
                format!("aggregation {}", o.aggregation.0),
            ));
        }
        if o.max_grey_levels < 1 {
            return Err(Error::wrap(
                ERR_INVALID_TEXTURE,
                format!("{} grey levels", o.max_grey_levels),
            ));
        }
        if o.workers < 0 {
            return Err(Error::wrap(
                ERR_INVALID_TEXTURE,
                format!("{} workers", o.workers),
            ));
        }
        Ok(o)
    }
}
type TextureVisit<'a> = dyn FnMut(i64, i64, i64, Option<f64>) + 'a;
fn compute_texture(
    ctx: Option<&dyn Cancellation>,
    source: impl FnOnce(&mut TextureVisit<'_>) -> Result<(), Error>,
    opts: TextureOptions,
) -> (TextureFeatures, Option<Error>) {
    let opts = match opts.normalized() {
        Ok(o) => o,
        Err(err) => return (TextureFeatures::default(), Some(err)),
    };
    let mut out = TextureFeatures::default();
    let mut voxels: Vec<TextureVoxel> = Vec::new();
    let (mut lo, mut hi) = ([i64::MAX; 3], [-1i64; 3]);
    let mut minimum = f64::INFINITY;
    let visited = source(&mut |x: i64, y: i64, z: i64, value: Option<f64>| {
        let Some(value) = value.filter(|v| finite(*v)) else {
            out.non_finite += 1;
            return;
        };
        if let Some(lower) = opts.lower_bound
            && value < lower
        {
            out.excluded += 1;
            return;
        }
        voxels.push(TextureVoxel { x, y, z, value });
        minimum = fmath::min(minimum, value);
        for (axis, c) in [x, y, z].into_iter().enumerate() {
            lo[axis] = lo[axis].min(c);
            hi[axis] = hi[axis].max(c);
        }
    });
    if let Err(err) = visited {
        return (TextureFeatures::default(), Some(err));
    }
    if let Some(err) = ctx_err(ctx) {
        return (TextureFeatures::default(), Some(err));
    }
    out.voxels = voxels.len() as i64;
    out.bin_width = opts.bin_width;
    if voxels.is_empty() {
        return (
            out,
            Some(Error::wrap(ERR_INVALID_TEXTURE, "no voxel with a value")),
        );
    }
    out.lower_bound = minimum;
    if let Some(lower) = opts.lower_bound {
        out.lower_bound = lower;
    }
    let dims = [
        hi[0].wrapping_sub(lo[0]).wrapping_add(1),
        hi[1].wrapping_sub(lo[1]).wrapping_add(1),
        hi[2].wrapping_sub(lo[2]).wrapping_add(1),
    ];
    let mut g = TextureGrid {
        dims,
        level: vec![0i32; (dims[0] * dims[1] * dims[2]) as usize],
        levels: 0,
    };
    for v in &voxels {
        let level = ((v.value - out.lower_bound) / opts.bin_width).floor() + 1.0;
        if level > opts.max_grey_levels as f64 {
            return (
                out,
                Some(Error::wrap(
                    ERR_INVALID_TEXTURE,
                    format!(
                        "more than {} grey levels with bin width {}; use a wider bin",
                        opts.max_grey_levels,
                        float_v(opts.bin_width)
                    ),
                )),
            );
        }
        let at = ((v.z - lo[2]) * g.dims[1] + v.y - lo[1]) * g.dims[0] + v.x - lo[0];
        g.level[at as usize] = fmath::f64_to_i32(level);
        g.levels = g.levels.max(fmath::f64_to_i64(level));
    }
    out.grey_levels = g.levels;
    let (glcm, glrlm) = match g.directional_matrices(ctx, &opts) {
        Ok(m) => m,
        Err(err) => return (out, Some(err)),
    };
    (out.glcm, out.glcm_directions) = aggregate_glcm(&glcm, g.levels as usize, opts.aggregation);
    (out.glrlm, out.glrlm_directions) = aggregate_glrlm(
        &glrlm,
        g.levels as usize,
        voxels.len() as i64,
        opts.aggregation,
    );
    (out, None)
}
#[derive(Default)]
struct GlcmMatrix {
    counts: Vec<f64>,
    pairs: f64,
}
#[derive(Default)]
struct GlrlmMatrix {
    counts: Vec<f64>,
    max_run: usize,
    runs: f64,
}
impl TextureGrid {
    fn directional_matrices(
        &self,
        ctx: Option<&dyn Cancellation>,
        opts: &TextureOptions,
    ) -> Result<(Vec<GlcmMatrix>, Vec<GlrlmMatrix>), Error> {
        let g = self;
        let ng = g.levels as usize;
        let max_run = g.dims[0].max(g.dims[1]).max(g.dims[2]) as usize;
        let mut glcm: Vec<GlcmMatrix> = Vec::with_capacity(13);
        let mut glrlm: Vec<GlrlmMatrix> = Vec::with_capacity(13);
        let mut next = 0usize;
        'directions: loop {
            if ctx_err(ctx).is_some() || next >= TEXTURE_DIRECTIONS.len() {
                break;
            }
            if ctx_err(ctx).is_some() {
                break;
            }
            let dir = TEXTURE_DIRECTIONS[next];
            next += 1;
            let (dx, dy, dz) = (
                dir[0].wrapping_mul(opts.distance),
                dir[1].wrapping_mul(opts.distance),
                dir[2].wrapping_mul(opts.distance),
            );
            let mut co = GlcmMatrix {
                counts: vec![0.0; ng * ng],
                pairs: 0.0,
            };
            let mut runs = GlrlmMatrix {
                counts: vec![0.0; ng * max_run],
                max_run,
                runs: 0.0,
            };
            for z in 0..g.dims[2] {
                if ctx_err(ctx).is_some() {
                    continue 'directions;
                }
                for y in 0..g.dims[1] {
                    let row = (z * g.dims[1] + y) * g.dims[0];
                    for x in 0..g.dims[0] {
                        let a = g.level[(row + x) as usize];
                        if a == 0 {
                            continue;
                        }
                        let b = g.at(x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz));
                        if b != 0 {
                            co.counts[(a - 1) as usize * ng + (b - 1) as usize] += 1.0;
                            co.counts[(b - 1) as usize * ng + (a - 1) as usize] += 1.0;
                            co.pairs += 2.0;
                        }
                        if g.at(x - dir[0], y - dir[1], z - dir[2]) == a {
                            continue;
                        }
                        let mut length = 1i64;
                        while g.at(
                            x + length * dir[0],
                            y + length * dir[1],
                            z + length * dir[2],
                        ) == a
                        {
                            length += 1;
                        }
                        runs.counts[(a - 1) as usize * max_run + length as usize - 1] += 1.0;
                        runs.runs += 1.0;
                    }
                }
            }
            glcm.push(co);
            glrlm.push(runs);
        }
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        Ok((glcm, glrlm))
    }
}
fn log2_term(p: f64) -> f64 {
    if p <= 0.0 {
        return 0.0;
    }
    p * fmath::log2(p)
}
fn glcm_features(counts: &[f64], total: f64, ng: usize) -> GlcmFeatures {
    let mut f = GlcmFeatures::default();
    let n = ng as f64;
    let mut px = vec![0.0f64; ng];
    let mut diff = vec![0.0f64; ng];
    let mut sum = vec![0.0f64; 2 * ng + 1];
    for i in 1..=ng {
        for j in 1..=ng {
            let c = counts[(i - 1) * ng + j - 1];
            if c == 0.0 {
                continue;
            }
            let p = c / total;
            px[i - 1] += p;
            let k = i.abs_diff(j);
            diff[k] += p;
            sum[i + j] += p;
            let (fi, fj, fk) = (i as f64, j as f64, k as f64);
            f.joint_maximum = fmath::max(f.joint_maximum, p);
            f.joint_average += fi * p;
            f.joint_entropy -= log2_term(p);
            f.angular_second_moment += p * p;
            f.contrast += fk * fk * p;
            f.dissimilarity += fk * p;
            f.inverse_difference += p / (1.0 + fk);
            f.normalised_inverse_difference += p / (1.0 + fk / n);
            f.inverse_difference_moment += p / (1.0 + fk * fk);
            f.normalised_inverse_difference_moment += p / (1.0 + fk * fk / (n * n));
            f.autocorrelation += fi * fj * p;
        }
    }
    let mu = f.joint_average;
    let (mut hx, mut hxy1, mut hxy2) = (0.0f64, 0.0f64, 0.0f64);
    for i in 1..=ng {
        let pi = px[i - 1];
        f.joint_variance += (i as f64 - mu) * (i as f64 - mu) * pi;
        hx -= log2_term(pi);
    }
    for i in 1..=ng {
        for j in 1..=ng {
            let pipj = px[i - 1] * px[j - 1];
            if pipj == 0.0 {
                continue;
            }
            let p = counts[(i - 1) * ng + j - 1] / total;
            hxy1 -= p * fmath::log2(pipj);
            hxy2 -= log2_term(pipj);
            let s = (i + j) as f64 - 2.0 * mu;
            f.cluster_tendency += s * s * p;
            f.cluster_shade += s * s * s * p;
            f.cluster_prominence += s * s * s * s * p;
        }
    }
    for (k, &p) in diff.iter().enumerate() {
        f.difference_average += k as f64 * p;
        f.difference_entropy -= log2_term(p);
        if k > 0 {
            f.inverse_variance += p / (k * k) as f64;
        }
    }
    for (k, &p) in diff.iter().enumerate() {
        let d = k as f64 - f.difference_average;
        f.difference_variance += d * d * p;
    }
    for (k, &p) in sum.iter().enumerate().skip(2) {
        f.sum_average += k as f64 * p;
        f.sum_entropy -= log2_term(p);
    }
    for (k, &p) in sum.iter().enumerate().skip(2) {
        let d = k as f64 - f.sum_average;
        f.sum_variance += d * d * p;
    }
    if f.joint_variance > 0.0 {
        f.correlation = (f.autocorrelation - mu * mu) / f.joint_variance;
    } else {
        f.correlation = f64::NAN;
    }
    if hx > 0.0 {
        f.information_correlation_1 = (f.joint_entropy - hxy1) / hx;
    } else {
        f.information_correlation_1 = f64::NAN;
    }
    f.information_correlation_2 =
        (1.0 - fmath::exp(-2.0 * fmath::max(0.0, hxy2 - f.joint_entropy))).sqrt();
    f
}
fn glrlm_features(
    counts: &[f64],
    runs: f64,
    ng: usize,
    max_run: usize,
    voxels: f64,
) -> GlrlmFeatures {
    let mut f = GlrlmFeatures::default();
    let mut by_level = vec![0.0f64; ng];
    let mut by_length = vec![0.0f64; max_run];
    let (mut mu_level, mut mu_length) = (0.0f64, 0.0f64);
    for i in 1..=ng {
        for j in 1..=max_run {
            let r = counts[(i - 1) * max_run + j - 1];
            if r == 0.0 {
                continue;
            }
            let (fi, fj) = (i as f64, j as f64);
            by_level[i - 1] += r;
            by_length[j - 1] += r;
            f.short_run_low_grey_level_emphasis += r / (fi * fi * fj * fj);
            f.short_run_high_grey_level_emphasis += r * fi * fi / (fj * fj);
            f.long_run_low_grey_level_emphasis += r * fj * fj / (fi * fi);
            f.long_run_high_grey_level_emphasis += r * fi * fi * fj * fj;
            let p = r / runs;
            mu_level += fi * p;
            mu_length += fj * p;
            f.run_entropy -= log2_term(p);
        }
    }
    for i in 1..=ng {
        let (r, fi) = (by_level[i - 1], i as f64);
        f.low_grey_level_run_emphasis += r / (fi * fi);
        f.high_grey_level_run_emphasis += r * fi * fi;
        f.grey_level_non_uniformity += r * r;
    }
    for j in 1..=max_run {
        let (r, fj) = (by_length[j - 1], j as f64);
        f.short_runs_emphasis += r / (fj * fj);
        f.long_runs_emphasis += r * fj * fj;
        f.run_length_non_uniformity += r * r;
    }
    for i in 1..=ng {
        for j in 1..=max_run {
            let r = counts[(i - 1) * max_run + j - 1];
            if r != 0.0 {
                let p = r / runs;
                f.grey_level_variance += (i as f64 - mu_level) * (i as f64 - mu_level) * p;
                f.run_length_variance += (j as f64 - mu_length) * (j as f64 - mu_length) * p;
            }
        }
    }
    f.short_runs_emphasis /= runs;
    f.long_runs_emphasis /= runs;
    f.low_grey_level_run_emphasis /= runs;
    f.high_grey_level_run_emphasis /= runs;
    f.short_run_low_grey_level_emphasis /= runs;
    f.short_run_high_grey_level_emphasis /= runs;
    f.long_run_low_grey_level_emphasis /= runs;
    f.long_run_high_grey_level_emphasis /= runs;
    f.grey_level_non_uniformity /= runs;
    f.run_length_non_uniformity /= runs;
    f.normalised_grey_level_non_uniformity = f.grey_level_non_uniformity / runs;
    f.normalised_run_length_non_uniformity = f.run_length_non_uniformity / runs;
    f.run_percentage = runs / voxels;
    f
}
fn aggregate_glcm(
    matrices: &[GlcmMatrix],
    ng: usize,
    aggregation: TextureAggregation,
) -> (GlcmFeatures, i64) {
    let mut directions = 0i64;
    if aggregation == TextureAggregation::MERGED {
        let mut merged = vec![0.0f64; ng * ng];
        let mut total = 0.0f64;
        for m in matrices {
            if m.pairs > 0.0 {
                directions += 1;
                total += m.pairs;
                for (i, c) in m.counts.iter().enumerate() {
                    merged[i] += c;
                }
            }
        }
        if directions == 0 {
            return (GlcmFeatures::from_fields([f64::NAN; 25]), 0);
        }
        return (glcm_features(&merged, total, ng), directions);
    }
    let features: Vec<[f64; 25]> = matrices
        .iter()
        .filter(|m| m.pairs > 0.0)
        .map(|m| glcm_features(&m.counts, m.pairs, ng).fields())
        .collect();
    if features.is_empty() {
        return (GlcmFeatures::from_fields([f64::NAN; 25]), 0);
    }
    (
        GlcmFeatures::from_fields(average_features(&features)),
        features.len() as i64,
    )
}
fn aggregate_glrlm(
    matrices: &[GlrlmMatrix],
    ng: usize,
    voxels: i64,
    aggregation: TextureAggregation,
) -> (GlrlmFeatures, i64) {
    let mut directions = 0i64;
    if aggregation == TextureAggregation::MERGED {
        let max_run = matrices[0].max_run;
        let mut merged = vec![0.0f64; ng * max_run];
        let mut runs = 0.0f64;
        for m in matrices {
            if m.runs > 0.0 {
                directions += 1;
                runs += m.runs;
                for (i, c) in m.counts.iter().enumerate() {
                    merged[i] += c;
                }
            }
        }
        return (
            glrlm_features(&merged, runs, ng, max_run, (voxels * directions) as f64),
            directions,
        );
    }
    let features: Vec<[f64; 16]> = matrices
        .iter()
        .filter(|m| m.runs > 0.0)
        .map(|m| glrlm_features(&m.counts, m.runs, ng, m.max_run, voxels as f64).fields())
        .collect();
    (
        GlrlmFeatures::from_fields(average_features(&features)),
        features.len() as i64,
    )
}
fn average_features<const N: usize>(features: &[[f64; N]]) -> [f64; N] {
    let mut out = [0.0f64; N];
    let mut defined = [0i64; N];
    for f in features {
        for (k, v) in f.iter().enumerate() {
            if !v.is_nan() {
                out[k] += v;
                defined[k] += 1;
            }
        }
    }
    for (k, v) in out.iter_mut().enumerate() {
        if defined[k] == 0 {
            *v = f64::NAN;
        } else {
            *v /= defined[k] as f64;
        }
    }
    out
}
impl GlcmFeatures {
    pub fn fields(&self) -> [f64; 25] {
        [
            self.joint_maximum,
            self.joint_average,
            self.joint_variance,
            self.joint_entropy,
            self.difference_average,
            self.difference_variance,
            self.difference_entropy,
            self.sum_average,
            self.sum_variance,
            self.sum_entropy,
            self.angular_second_moment,
            self.contrast,
            self.dissimilarity,
            self.inverse_difference,
            self.normalised_inverse_difference,
            self.inverse_difference_moment,
            self.normalised_inverse_difference_moment,
            self.inverse_variance,
            self.correlation,
            self.autocorrelation,
            self.cluster_tendency,
            self.cluster_shade,
            self.cluster_prominence,
            self.information_correlation_1,
            self.information_correlation_2,
        ]
    }
    fn from_fields(v: [f64; 25]) -> GlcmFeatures {
        GlcmFeatures {
            joint_maximum: v[0],
            joint_average: v[1],
            joint_variance: v[2],
            joint_entropy: v[3],
            difference_average: v[4],
            difference_variance: v[5],
            difference_entropy: v[6],
            sum_average: v[7],
            sum_variance: v[8],
            sum_entropy: v[9],
            angular_second_moment: v[10],
            contrast: v[11],
            dissimilarity: v[12],
            inverse_difference: v[13],
            normalised_inverse_difference: v[14],
            inverse_difference_moment: v[15],
            normalised_inverse_difference_moment: v[16],
            inverse_variance: v[17],
            correlation: v[18],
            autocorrelation: v[19],
            cluster_tendency: v[20],
            cluster_shade: v[21],
            cluster_prominence: v[22],
            information_correlation_1: v[23],
            information_correlation_2: v[24],
        }
    }
}
impl GlrlmFeatures {
    pub fn fields(&self) -> [f64; 16] {
        [
            self.short_runs_emphasis,
            self.long_runs_emphasis,
            self.low_grey_level_run_emphasis,
            self.high_grey_level_run_emphasis,
            self.short_run_low_grey_level_emphasis,
            self.short_run_high_grey_level_emphasis,
            self.long_run_low_grey_level_emphasis,
            self.long_run_high_grey_level_emphasis,
            self.grey_level_non_uniformity,
            self.normalised_grey_level_non_uniformity,
            self.run_length_non_uniformity,
            self.normalised_run_length_non_uniformity,
            self.run_percentage,
            self.grey_level_variance,
            self.run_length_variance,
            self.run_entropy,
        ]
    }
    fn from_fields(v: [f64; 16]) -> GlrlmFeatures {
        GlrlmFeatures {
            short_runs_emphasis: v[0],
            long_runs_emphasis: v[1],
            low_grey_level_run_emphasis: v[2],
            high_grey_level_run_emphasis: v[3],
            short_run_low_grey_level_emphasis: v[4],
            short_run_high_grey_level_emphasis: v[5],
            long_run_low_grey_level_emphasis: v[6],
            long_run_high_grey_level_emphasis: v[7],
            grey_level_non_uniformity: v[8],
            normalised_grey_level_non_uniformity: v[9],
            run_length_non_uniformity: v[10],
            normalised_run_length_non_uniformity: v[11],
            run_percentage: v[12],
            grey_level_variance: v[13],
            run_length_variance: v[14],
            run_entropy: v[15],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureFeature {
    pub family: &'static str,
    pub name: &'static str,
    pub value: f64,
}
const GLCM_NAMES: [&str; 25] = [
    "Joint maximum",
    "Joint average",
    "Joint variance",
    "Joint entropy",
    "Difference average",
    "Difference variance",
    "Difference entropy",
    "Sum average",
    "Sum variance",
    "Sum entropy",
    "Angular second moment",
    "Contrast",
    "Dissimilarity",
    "Inverse difference",
    "Normalised inverse difference",
    "Inverse difference moment",
    "Normalised inverse difference moment",
    "Inverse variance",
    "Correlation",
    "Autocorrelation",
    "Cluster tendency",
    "Cluster shade",
    "Cluster prominence",
    "Information correlation 1",
    "Information correlation 2",
];
const GLRLM_NAMES: [&str; 16] = [
    "Short runs emphasis",
    "Long runs emphasis",
    "Low grey level run emphasis",
    "High grey level run emphasis",
    "Short run low grey level emphasis",
    "Short run high grey level emphasis",
    "Long run low grey level emphasis",
    "Long run high grey level emphasis",
    "Grey level non-uniformity",
    "Normalised grey level non-uniformity",
    "Run length non-uniformity",
    "Normalised run length non-uniformity",
    "Run percentage",
    "Grey level variance",
    "Run length variance",
    "Run entropy",
];
impl TextureFeatures {
    pub fn list(&self) -> Vec<TextureFeature> {
        let mut out = Vec::with_capacity(GLCM_NAMES.len() + GLRLM_NAMES.len());
        for (name, value) in GLCM_NAMES.iter().zip(self.glcm.fields()) {
            out.push(TextureFeature {
                family: "GLCM",
                name,
                value,
            });
        }
        for (name, value) in GLRLM_NAMES.iter().zip(self.glrlm.fields()) {
            out.push(TextureFeature {
                family: "GLRLM",
                name,
                value,
            });
        }
        out
    }
}
impl VolumeMask {
    pub fn texture_features(
        &self,
        ctx: Option<&dyn Cancellation>,
        value_at: Option<&mut VoxelValueFn<'_>>,
        opts: TextureOptions,
    ) -> (TextureFeatures, Option<Error>) {
        if let Err(err) = self.validate() {
            return (TextureFeatures::default(), Some(err));
        }
        let Some(value_at) = value_at else {
            return (
                TextureFeatures::default(),
                Some(Error::wrap(ERR_INVALID_TEXTURE, "no value accessor")),
            );
        };
        let m = self;
        let plane = (m.columns * m.rows) as usize;
        compute_texture(
            ctx,
            |visit| {
                for z in 0..m.slices {
                    if let Some(err) = ctx_err(ctx) {
                        return Err(err);
                    }
                    let start = z as usize * plane;
                    for (i, &on) in m.voxels[start..start + plane].iter().enumerate() {
                        if on {
                            let (x, y) = (i as i64 % m.columns, i as i64 / m.columns);
                            visit(x, y, z, value_at(x, y, z));
                        }
                    }
                }
                Ok(())
            },
            opts,
        )
    }
}
impl VoiMask {
    pub fn texture_features(
        &self,
        ctx: Option<&dyn Cancellation>,
        value_at: Option<&mut VoxelValueFn<'_>>,
        opts: TextureOptions,
    ) -> (TextureFeatures, Option<Error>) {
        let Some(value_at) = value_at else {
            return (
                TextureFeatures::default(),
                Some(Error::wrap(
                    ERR_INVALID_TEXTURE,
                    "texture needs a mask and a value accessor",
                )),
            );
        };
        compute_texture(
            ctx,
            |visit| {
                for voxel in &self.voxels {
                    if voxel.centre_inside {
                        let v = value_at(voxel.column, voxel.row, voxel.slice);
                        visit(voxel.column, voxel.row, voxel.slice, v);
                    }
                }
                Ok(())
            },
            opts,
        )
    }
}
