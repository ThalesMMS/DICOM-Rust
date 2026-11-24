use super::median::{MedianHistogram, max_nan32, min_nan32, select_rank};
use super::slab::clamp_axis;
use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
pub const PHYSICAL_BOX: u8 = 0;
pub const PHYSICAL_ELLIPSOID: u8 = 1;
pub const CLIP_BOUNDARY: u8 = 0;
pub const REPLICATE_BOUNDARY: u8 = 1;
pub const REQUIRE_VALID_CENTER: u8 = 0;
pub const ANY_VALID_NEIGHBORHOOD: u8 = 1;
pub const REJECT_NON_FINITE: u8 = 0;
pub const EXCLUDE_NON_FINITE: u8 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RankRegion {
    pub origin: [i64; 3],
    pub size: [i64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RankLimits {
    pub max_voxels: i64,
    pub max_kernel_samples: i64,
    pub max_working_bytes: i64,
    pub max_sample_visits: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MaskedRankOptions {
    pub rank: f64,
    pub radius_mm: [f64; 3],
    pub neighborhood: u8,
    pub boundary: u8,
    pub centers: u8,
    pub non_finite: u8,
    pub empty_value: f32,
    pub workers: i64,
    pub limits: RankLimits,
}
pub fn default_masked_rank_options() -> MaskedRankOptions {
    MaskedRankOptions {
        rank: 0.5,
        ..Default::default()
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct MaskedRankResult {
    pub values: Volume,
    pub valid: Vec<bool>,
    pub region: RankRegion,
    pub working_bytes: i64,
    pub worker_limit: i64,
}
fn invalid(detail: &str) -> FilterError {
    FilterError::invalid(detail)
}
fn limit(detail: &str) -> FilterError {
    FilterError::new(
        ErrorKind::RankResourceLimit,
        format!("imagefilter: masked rank resource limit: {detail}"),
    )
}
fn rank_finite(v: f32) -> bool {
    (-f32::MAX..=f32::MAX).contains(&v)
}
fn rank_zero(v: f32) -> f32 {
    if v == 0.0 { 0.0 } else { v }
}
fn rank_index(q: f64, n: usize) -> usize {
    if q == 1.0 {
        return n - 1;
    }
    (q * (n - 1) as f64).floor() as usize
}
fn normalize_limits(mut l: RankLimits) -> Result<RankLimits, FilterError> {
    for (v, def, cap) in [
        (&mut l.max_voxels, 1i64 << 24, 1i64 << 24),
        (&mut l.max_kernel_samples, 1 << 16, 1 << 16),
        (&mut l.max_working_bytes, 256 << 20, 1 << 30),
        (&mut l.max_sample_visits, 1 << 30, 1 << 40),
    ] {
        if *v < 0 || *v > cap {
            return Err(invalid("invalid rank quota"));
        }
        if *v == 0 {
            *v = def;
        }
    }
    Ok(l)
}
struct RankRequest<'a> {
    src: &'a Volume,
    valid: Option<&'a [bool]>,
    opts: MaskedRankOptions,
    region: RankRegion,
    dims: [usize; 3],
    radius: [i64; 3],
    samples: usize,
    rows: usize,
    outputs: usize,
    workers: i64,
    working: i64,
    lo: f32,
    span: usize,
    histogram: bool,
    kernel: Vec<(i64, i64, i64)>,
}
pub fn masked_rank(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    valid: Option<&[bool]>,
    opts: &MaskedRankOptions,
) -> Result<MaskedRankResult, FilterError> {
    let region = RankRegion {
        origin: [0; 3],
        size: src.map_or([0; 3], |s| s.dims),
    };
    masked_rank_impl(ctx, src, valid, region, opts)
}
pub fn masked_rank_region(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    valid: Option<&[bool]>,
    region: RankRegion,
    opts: &MaskedRankOptions,
) -> Result<MaskedRankResult, FilterError> {
    masked_rank_impl(ctx, src, valid, region, opts)
}
fn rank_admit<'a>(
    ctx: Ctx<'_>,
    src: Option<&'a Volume>,
    valid: Option<&'a [bool]>,
    region: RankRegion,
    opts: &MaskedRankOptions,
) -> Result<RankRequest<'a>, FilterError> {
    ctx_err(ctx)?;
    let Some(src) = src else {
        return Err(invalid("nil rank source"));
    };
    if opts.neighborhood > PHYSICAL_ELLIPSOID
        || opts.boundary > REPLICATE_BOUNDARY
        || opts.centers > ANY_VALID_NEIGHBORHOOD
        || opts.non_finite > EXCLUDE_NON_FINITE
        || !(opts.rank >= 0.0 && opts.rank <= 1.0)
        || !rank_finite(opts.empty_value)
        || opts.workers < 0
        || opts.workers > 64
    {
        return Err(invalid("rank options"));
    }
    let limits = normalize_limits(opts.limits)?;
    let mut opts = *opts;
    opts.limits = limits;
    let n = voxel_count(src.dims);
    let n = match n {
        Some(n) if src.data.len() as i64 == n && valid.is_none_or(|v| v.len() as i64 == n) => n,
        _ => return Err(invalid("rank dimensions/data/mask length")),
    };
    if n > limits.max_voxels {
        return Err(limit("rank source voxels"));
    }
    for (a, d) in src.dims.iter().copied().enumerate() {
        if d > 1 && !(src.spacing_mm[a] > 0.0 && src.spacing_mm[a] <= f64::MAX) {
            return Err(invalid(&format!("rank spacing axis {a}")));
        }
        if !(opts.radius_mm[a] >= 0.0 && opts.radius_mm[a] <= f64::MAX) {
            return Err(invalid(&format!("rank radius axis {a}")));
        }
        if region.origin[a] < 0
            || region.size[a] <= 0
            || region.origin[a] > d
            || region.size[a] > d - region.origin[a]
        {
            return Err(invalid("rank region"));
        }
    }
    let Some(m) = voxel_count(region.size) else {
        return Err(invalid("rank output dimensions"));
    };
    let mut r = RankRequest {
        src,
        valid,
        opts,
        region,
        dims: src.dims.map(|d| d as usize),
        radius: [0; 3],
        samples: 0,
        rows: 0,
        outputs: m as usize,
        workers: 0,
        working: 0,
        lo: 0.0,
        span: 0,
        histogram: false,
        kernel: Vec::new(),
    };
    let mut bounding: i64 = 1;
    for a in 0..3 {
        if src.dims[a] > 1 {
            r.radius[a] = r.axis_radius(a)?;
        }
        let width = 2 * r.radius[a] + 1;
        if bounding > limits.max_kernel_samples / width {
            return Err(limit("rank bounding kernel"));
        }
        bounding *= width;
    }
    let (rows, samples) = r.count_kernel(ctx)?;
    r.rows = rows;
    r.samples = samples;
    if m > (limits.max_sample_visits - n - bounding) / samples as i64 {
        return Err(limit("rank sample visits"));
    }
    r.workers = opts.workers;
    if r.workers == 0 {
        r.workers = 63.min(crate::render::parallel::budget()) + 1;
    }
    r.workers = 1.max(r.workers.min(region.size[1] * region.size[2]));
    let hist_bytes = 4 * ((1i64 << 16) + 256);
    r.working = 5 * m
        + 12 * rows as i64
        + r.workers * (16 * rows as i64 + (4 * samples as i64).max(hist_bytes) + 256)
        + 4096;
    if r.working > limits.max_working_bytes {
        return Err(limit(&format!("rank working bytes ({})", r.working)));
    }
    ctx_err(ctx)?;
    Ok(r)
}
impl RankRequest<'_> {
    fn inside(&self, offsets: [i64; 3]) -> bool {
        let mut sum = 0.0;
        for (a, d) in offsets.iter().copied().enumerate() {
            if self.src.dims[a] == 1 {
                if d != 0 {
                    return false;
                }
                continue;
            }
            let radius = self.opts.radius_mm[a];
            if radius == 0.0 {
                if d != 0 {
                    return false;
                }
                continue;
            }
            let distance = d as f64 * self.src.spacing_mm[a];
            if self.opts.neighborhood == PHYSICAL_BOX {
                if distance > radius || -distance > radius {
                    return false;
                }
                continue;
            }
            let q = distance / radius;
            sum += q * q;
        }
        sum <= 1.0
    }
    fn axis_radius(&self, axis: usize) -> Result<i64, FilterError> {
        let maximum = (self.opts.limits.max_kernel_samples - 1) / 2;
        let mut offsets = [0i64; 3];
        offsets[axis] = maximum + 1;
        if self.inside(offsets) {
            return Err(limit(&format!("rank axial kernel axis {axis}")));
        }
        let (mut lo, mut hi) = (0i64, maximum);
        while lo < hi {
            let mid = lo + (hi - lo + 1) / 2;
            offsets[axis] = mid;
            if self.inside(offsets) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Ok(lo)
    }
    fn each_kernel_row(
        &self,
        ctx: Ctx<'_>,
        f: &mut dyn FnMut(i64, i64, i64),
    ) -> Result<(), FilterError> {
        let r = self.radius;
        for dz in -r[2]..=r[2] {
            for dy in -r[1]..=r[1] {
                let mut half = -1;
                for dx in 0..=r[0] {
                    if self.inside([dx, dy, dz]) {
                        half = dx;
                    }
                }
                if half >= 0 {
                    f(dy, dz, half);
                }
            }
        }
        ctx_err(ctx)
    }
    fn count_kernel(&self, ctx: Ctx<'_>) -> Result<(usize, usize), FilterError> {
        let (mut rows, mut samples) = (0usize, 0usize);
        self.each_kernel_row(ctx, &mut |_, _, half| {
            rows += 1;
            samples += 2 * half as usize + 1;
        })?;
        Ok((rows, samples))
    }
    fn make_kernel(&mut self, ctx: Ctx<'_>) -> Result<(), FilterError> {
        ctx_err(ctx)?;
        let mut kernel = Vec::with_capacity(self.rows);
        self.each_kernel_row(ctx, &mut |dy, dz, half| kernel.push((dy, dz, half)))?;
        self.kernel = kernel;
        Ok(())
    }
    fn eligible(&self, i: usize) -> bool {
        self.valid.is_none_or(|v| v[i]) && rank_finite(self.src.data[i])
    }
    fn scan(&mut self, ctx: Ctx<'_>) -> Result<(), FilterError> {
        let (mut found, mut integral) = (false, true);
        let mut hi = 0f32;
        for (i, v) in self.src.data.iter().copied().enumerate() {
            if i % 4096 == 4095 {
                ctx_err(ctx)?;
            }
            if self.valid.is_some_and(|m| !m[i]) {
                continue;
            }
            if !rank_finite(v) {
                if self.opts.non_finite == REJECT_NON_FINITE {
                    return Err(invalid(&format!("nonfinite rank sample {i}")));
                }
                continue;
            }
            if !found {
                self.lo = v;
                hi = v;
                found = true;
            } else {
                self.lo = min_nan32(self.lo, v);
                hi = max_nan32(hi, v);
            }
            if v != f64::from(v).trunc() as f32 || v >= 16_777_216.0 || v <= -16_777_216.0 {
                integral = false;
            }
        }
        if found && integral {
            self.span = (hi - self.lo) as i64 as usize + 1;
            self.histogram = self.span <= 1 << 16 && self.samples >= 2 * self.rows;
        }
        ctx_err(ctx)
    }
    fn line_rows(&self, y: i64, z: i64) -> Vec<(Option<usize>, i64)> {
        let d = self.dims;
        self.kernel
            .iter()
            .map(|&(dy, dz, half)| {
                let (cy, cz) = (y + dy, z + dz);
                if self.opts.boundary == CLIP_BOUNDARY
                    && (cy < 0 || cy >= d[1] as i64 || cz < 0 || cz >= d[2] as i64)
                {
                    return (None, 0);
                }
                let (cy, cz) = (clamp_axis(cy, d[1]), clamp_axis(cz, d[2]));
                (Some((cz * d[1] + cy) * d[0]), half)
            })
            .collect()
    }
    fn line(
        &self,
        values: &mut [f32],
        hist: &mut Option<MedianHistogram>,
        out: &mut MaskedRankResult,
        line: usize,
    ) {
        let size = self.region.size.map(|s| s as usize);
        let origin = self.region.origin.map(|o| o as usize);
        let y = origin[1] + line % size[1];
        let z = origin[2] + line / size[1];
        let rows = self.line_rows(y as i64, z as i64);
        if let Some(h) = hist.as_mut() {
            self.histogram_line(h, &rows, out, line, y, z);
            return;
        }
        let d = self.dims;
        for x in 0..size[0] {
            let gx = origin[0] + x;
            let i = line * size[0] + x;
            if self.opts.centers == REQUIRE_VALID_CENTER
                && !self.eligible((z * d[1] + y) * d[0] + gx)
            {
                out.values.data[i] = rank_zero(self.opts.empty_value);
                continue;
            }
            let mut n = 0;
            for &(base, half) in &rows {
                let Some(base) = base else {
                    continue;
                };
                for dx in -half..=half {
                    let cx = gx as i64 + dx;
                    if self.opts.boundary == CLIP_BOUNDARY && (cx < 0 || cx >= d[0] as i64) {
                        continue;
                    }
                    let j = base + clamp_axis(cx, d[0]);
                    if self.eligible(j) {
                        values[n] = rank_zero(self.src.data[j]);
                        n += 1;
                    }
                }
            }
            if n == 0 {
                out.values.data[i] = rank_zero(self.opts.empty_value);
                continue;
            }
            let v = rank_select(&mut values[..n], rank_index(self.opts.rank, n));
            out.values.data[i] = rank_zero(v);
            out.valid[i] = true;
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn hist_add(&self, h: &mut MedianHistogram, n: &mut i64, base: usize, cx: i64, delta: i32) {
        let d = self.dims;
        if self.opts.boundary == CLIP_BOUNDARY && (cx < 0 || cx >= d[0] as i64) {
            return;
        }
        let j = base + clamp_axis(cx, d[0]);
        if !self.eligible(j) {
            return;
        }
        let b = (self.src.data[j] - self.lo) as i64 as usize;
        h.fine[b] += delta;
        h.coarse[b >> 8] += delta;
        *n += i64::from(delta);
        if b < h.med {
            h.below += i64::from(delta);
        }
    }
    fn histogram_line(
        &self,
        h: &mut MedianHistogram,
        rows: &[(Option<usize>, i64)],
        out: &mut MaskedRankResult,
        line: usize,
        y: usize,
        z: usize,
    ) {
        h.fine.iter_mut().for_each(|f| *f = 0);
        h.coarse.iter_mut().for_each(|c| *c = 0);
        h.med = 0;
        h.below = 0;
        let d = self.dims;
        let mut n: i64 = 0;
        let size = self.region.size.map(|s| s as usize);
        let x0 = self.region.origin[0];
        for &(base, half) in rows {
            let Some(base) = base else {
                continue;
            };
            for dx in -half..=half {
                self.hist_add(h, &mut n, base, x0 + dx, 1);
            }
        }
        for x in 0..size[0] {
            let gx = x0 + x as i64;
            let i = line * size[0] + x;
            let good = self.opts.centers == ANY_VALID_NEIGHBORHOOD
                || self.eligible((z * d[1] + y) * d[0] + gx as usize);
            if good && n > 0 {
                let k = rank_index(self.opts.rank, n as usize) as i64;
                out.values.data[i] = rank_zero(self.lo + h.median(k) as f32);
                out.valid[i] = true;
            } else {
                out.values.data[i] = rank_zero(self.opts.empty_value);
            }
            if x + 1 < size[0] {
                for &(base, half) in rows {
                    let Some(base) = base else {
                        continue;
                    };
                    self.hist_add(h, &mut n, base, gx - half, -1);
                    self.hist_add(h, &mut n, base, gx + half + 1, 1);
                }
            }
        }
    }
}
fn masked_rank_impl(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    valid: Option<&[bool]>,
    region: RankRegion,
    opts: &MaskedRankOptions,
) -> Result<MaskedRankResult, FilterError> {
    let mut r = rank_admit(ctx, src, valid, region, opts)?;
    r.scan(ctx)?;
    r.make_kernel(ctx)?;
    ctx_err(ctx)?;
    let mut out = MaskedRankResult {
        values: Volume {
            dims: region.size,
            spacing_mm: r.src.spacing_mm,
            data: vec![0.0; r.outputs],
        },
        valid: vec![false; r.outputs],
        region,
        working_bytes: r.working,
        worker_limit: r.workers,
    };
    let mut hist = r.histogram.then(|| MedianHistogram::new(r.span));
    let mut values = if r.histogram {
        Vec::new()
    } else {
        vec![0f32; r.samples]
    };
    let lines = (region.size[1] * region.size[2]) as usize;
    for line in 0..lines {
        ctx_err(ctx)?;
        r.line(&mut values, &mut hist, &mut out, line);
    }
    ctx_err(ctx)?;
    Ok(out)
}
fn rank_select(v: &mut [f32], k: usize) -> f32 {
    if v.len() <= 32 {
        return select_rank(v, k);
    }
    let depth = 2 * (usize::BITS - v.len().leading_zeros()) as usize;
    rank_select_depth(v, k, depth)
}
fn rank_select_depth(v: &mut [f32], k: usize, mut depth: usize) -> f32 {
    let (mut lo, mut hi) = (0i64, v.len() as i64 - 1);
    let kk = k as i64;
    while lo < hi {
        if depth == 0 {
            rank_heap_sort(&mut v[lo as usize..=hi as usize]);
            return v[k];
        }
        depth -= 1;
        let mid = lo + (hi - lo) / 2;
        let (l, m, h) = (lo as usize, mid as usize, hi as usize);
        if v[m] < v[l] {
            v.swap(m, l);
        }
        if v[h] < v[l] {
            v.swap(h, l);
        }
        if v[h] < v[m] {
            v.swap(h, m);
        }
        let pivot = v[m];
        let (mut i, mut j) = (lo, hi);
        while i <= j {
            while v[i as usize] < pivot {
                i += 1;
            }
            while pivot < v[j as usize] {
                j -= 1;
            }
            if i <= j {
                v.swap(i as usize, j as usize);
                i += 1;
                j -= 1;
            }
        }
        if kk <= j {
            hi = j;
        } else if kk >= i {
            lo = i;
        } else {
            return v[k];
        }
    }
    v[k]
}
fn rank_heap_sort(v: &mut [f32]) {
    let sift = |v: &mut [f32], mut root: usize, n: usize| loop {
        let mut child = 2 * root + 1;
        if child >= n {
            return;
        }
        if child + 1 < n && v[child] < v[child + 1] {
            child += 1;
        }
        if v[root] >= v[child] {
            return;
        }
        v.swap(root, child);
        root = child;
    };
    let len = v.len();
    for root in (0..len / 2).rev() {
        sift(v, root, len);
    }
    for end in (1..len).rev() {
        v.swap(0, end);
        sift(v, 0, end);
    }
}
