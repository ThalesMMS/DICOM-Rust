use super::edge_preserving::{BilateralKernel, BilateralOptions};
use super::median::{MedianOptions, max_nan32, median_radius_samples, min_nan32, select_rank};
use super::nlm::{NLM_CUTOFF, NlmKernel, NonLocalMeansOptions, radius_or_default};
use super::slab::{DEFAULT_SLAB_WORKING_BYTES, SlabView, clamp_axis, worker_bound};
use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    pub origin: [i64; 3],
    pub size: [i64; 3],
}
pub struct RegionalIo<'a> {
    pub src: Option<&'a dyn RegionReader>,
    pub dst: Option<&'a mut dyn RegionWriter>,
}
pub trait RegionReader {
    fn read_region(&self, ctx: Ctx<'_>, r: Region, dst: &mut [f32]) -> Result<(), FilterError>;
}
pub trait RegionWriter {
    fn write_region(&mut self, ctx: Ctx<'_>, r: Region, src: &[f32]) -> Result<(), FilterError>;
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegionalFilter {
    Median(MedianOptions),
    Bilateral(BilateralOptions),
    NonLocalMeans(NonLocalMeansOptions),
    NilMedian,
    NilBilateral,
    NilNonLocalMeans,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegionalOptions {
    pub max_working_bytes: i64,
    pub tile_size: [i64; 3],
    pub max_read_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegionalResult {
    pub tile_size: [i64; 3],
    pub halo: [i64; 3],
    pub working_bytes: i64,
    pub planned_read_bytes: i64,
    pub read_bytes: i64,
    pub tiles: i64,
}
fn invalid(detail: &str) -> FilterError {
    FilterError::invalid(detail)
}
fn budget_error(required: i64, available: i64) -> FilterError {
    FilterError::new(
        ErrorKind::RegionalBudget,
        format!("imagefilter: regional working budget: need {required} bytes, have {available}"),
    )
}
#[derive(Default, Clone)]
struct NlmParams {
    dims: [i64; 3],
    patch: [i64; 3],
    search: [i64; 3],
    step: [i64; 3],
    block_len: i64,
    inv_h2: f64,
    cutoff: f64,
}
#[derive(Clone)]
struct RegionalKernel {
    kind: i32,
    radius: [i64; 3],
    workers: i64,
    block_len: i64,
    taps: i64,
    nlm: NlmParams,
}
fn checked_product(values: &[i64]) -> Result<i64, FilterError> {
    let mut n: i64 = 1;
    for &v in values {
        if v < 0 || (v != 0 && n > i64::MAX / v) {
            return Err(invalid("regional arithmetic overflow"));
        }
        n *= v;
    }
    Ok(n)
}
fn odd_widths(r: [i64; 3]) -> [i64; 3] {
    r.map(|v| v.wrapping_mul(2).wrapping_add(1))
}
fn radii_valid(r: [i64; 3]) -> bool {
    r.iter().all(|v| *v >= 0 && *v <= (i64::MAX - 1) / 2)
}
fn regional_params(
    f: &RegionalFilter,
    dims: [i64; 3],
    spacing: [f64; 3],
) -> Result<RegionalKernel, FilterError> {
    let mut k = RegionalKernel {
        kind: 0,
        radius: [0; 3],
        workers: 0,
        block_len: 0,
        taps: 0,
        nlm: NlmParams::default(),
    };
    match f {
        RegionalFilter::NilMedian => return Err(invalid("nil median options")),
        RegionalFilter::NilBilateral => return Err(invalid("nil bilateral options")),
        RegionalFilter::NilNonLocalMeans => return Err(invalid("nil NLM options")),
        RegionalFilter::Other => {
            return Err(invalid("filter is not regional median, bilateral or NLM"));
        }
        RegionalFilter::Median(o) => {
            k.kind = 1;
            k.workers = o.workers;
            k.radius = median_radius_samples(o, dims, spacing)?;
            if !radii_valid(k.radius) {
                return Err(invalid("median radius overflow"));
            }
            let limit = if o.max_kernel_samples == 0 {
                1 << 16
            } else {
                o.max_kernel_samples
            };
            let n = checked_product(&odd_widths(k.radius));
            match n {
                Ok(n) if limit >= 0 && n <= limit => k.block_len = n,
                _ => return Err(invalid("median box bound")),
            }
        }
        RegionalFilter::Bilateral(o) => {
            k.kind = 2;
            k.workers = o.workers;
            if !(o.range_sigma > 0.0) || o.range_sigma.is_infinite() {
                return Err(invalid("range sigma"));
            }
            let limit = if o.max_kernel_samples == 0 {
                1 << 16
            } else {
                o.max_kernel_samples
            };
            if limit < 1 {
                return Err(invalid("bilateral bound"));
            }
            for (a, s) in o.sigma_mm.iter().copied().enumerate() {
                if !(s >= 0.0) || s.is_infinite() {
                    return Err(invalid("bilateral sigma"));
                }
                if s > 0.0 && dims[a] > 1 {
                    let r = (2.5 * s / spacing[a]).ceil();
                    if r.is_infinite() || r > 4_611_686_018_427_387_904.0 {
                        return Err(invalid("bilateral radius overflow"));
                    }
                    k.radius[a] = fmath::f64_to_i64(r);
                }
            }
            let n = checked_product(&odd_widths(k.radius));
            match n {
                Ok(n) if n <= limit => k.taps = n,
                _ => return Err(invalid("bilateral kernel bound")),
            }
        }
        RegionalFilter::NonLocalMeans(o) => {
            k.kind = 3;
            k.workers = o.workers;
            if !(o.noise_sigma > 0.0) || o.noise_sigma.is_infinite() {
                return Err(invalid("noise sigma"));
            }
            let beta = if o.beta == 0.0 { 1.0 } else { o.beta };
            if !(beta > 0.0) || beta.is_infinite() {
                return Err(invalid("beta"));
            }
            let block_step = if o.block_step == 0 { 2 } else { o.block_step };
            if block_step < 0 {
                return Err(invalid("block step"));
            }
            let max_search = if o.max_search_samples == 0 {
                1 << 16
            } else {
                o.max_search_samples
            };
            k.nlm.dims = dims;
            k.nlm.patch = radius_or_default("patch", o.patch_radius_mm, 1, dims, spacing)?;
            k.nlm.search = radius_or_default("search", o.search_radius_mm, 5, dims, spacing)?;
            if !radii_valid(k.nlm.patch) || !radii_valid(k.nlm.search) {
                return Err(invalid("NLM radius overflow"));
            }
            let s = k.nlm.search;
            match checked_product(&odd_widths(s)) {
                Ok(n) if max_search >= 1 && n <= max_search => {}
                _ => return Err(invalid("NLM search bound")),
            }
            let p = k.nlm.patch;
            let n = checked_product(&odd_widths(p)).map_err(|_| invalid("NLM patch overflow"))?;
            k.block_len = n;
            k.nlm.block_len = n;
            for a in 0..3 {
                let r = p[a];
                k.nlm.step[a] = block_step.min(2 * r + 1);
                if r > (i64::MAX - s[a]) / 2 {
                    return Err(invalid("halo overflow"));
                }
                k.radius[a] = 2 * r + s[a];
            }
            let h2 = 2.0 * beta * o.noise_sigma * o.noise_sigma;
            if !(h2 > 0.0) || h2.is_infinite() {
                return Err(invalid("noise scale"));
            }
            k.nlm.inv_h2 = 1.0 / h2;
            k.nlm.cutoff = NLM_CUTOFF * h2;
        }
    }
    k.workers = worker_bound(k.workers);
    Ok(k)
}
fn regional_valid(r: Region, dims: [i64; 3]) -> bool {
    (0..3).all(|a| r.origin[a] >= 0 && r.size[a] > 0 && r.origin[a] <= dims[a] - r.size[a])
}
fn regional_window(r: Region, dims: [i64; 3], halo: [i64; 3]) -> Region {
    let mut w = Region::default();
    for a in 0..3 {
        let n = dims[a];
        w.origin[a] = 0.max(r.origin[a] - halo[a].min(n));
        let end = r.origin[a] + r.size[a];
        w.size[a] = n.min(end + halo[a].min(n - end)) - w.origin[a];
    }
    w
}
fn regional_extent(n: i64, size: i64, halo: i64) -> i64 {
    if halo >= (n - size + 1) / 2 {
        return n;
    }
    size + 2 * halo
}
fn worker_count(workers: i64, lines: i64) -> i64 {
    1.max(workers.min(lines))
}
fn cost(k: &RegionalKernel, dims: [i64; 3], tile: [i64; 3]) -> Result<i64, FilterError> {
    let mut window = [0i64; 3];
    for a in 0..3 {
        let n = dims[a];
        let h = k.radius[a].min(n);
        window[a] = n;
        if tile[a] <= n - 2 * h.min(n / 2) {
            window[a] = n.min(tile[a] + 2 * h);
        }
    }
    let ni = checked_product(&[window[0], window[1], window[2], 4])?;
    let no = checked_product(&[tile[0], tile[1], tile[2], 4])?;
    let mut max_lines = tile[1] * tile[2];
    if k.kind == 3 {
        max_lines = max_lines.max(regional_extent(dims[1], tile[1], k.nlm.patch[1]));
    }
    let workers = worker_count(k.workers, max_lines);
    let scratch = match k.kind {
        1 => checked_product(&[k.block_len, 4, workers])?,
        2 => checked_product(&[k.taps, 64])?,
        _ => {
            let mut centres = [0i64; 3];
            for a in 0..3 {
                centres[a] = regional_extent(dims[a], tile[a], k.nlm.patch[a]);
            }
            let estimates = checked_product(&[centres[0], centres[1], centres[2], k.block_len, 4])?;
            let per_worker = checked_product(&[workers, k.block_len, 24])?;
            let offsets = checked_product(&[2 * k.nlm.patch[1] + 1, 2 * k.nlm.patch[2] + 1, 16])?;
            let metadata = checked_product(&[centres[0] + centres[1] + centres[2], 8])?;
            let mut scratch = estimates;
            for extra in [per_worker, offsets, metadata] {
                if scratch > i64::MAX - extra {
                    return Err(invalid("scratch overflow"));
                }
                scratch += extra;
            }
            scratch
        }
    };
    let metadata = checked_product(&[workers, 256])?;
    let mut total: i64 = 0;
    for part in [ni, no, scratch, 4096, metadata] {
        if total > i64::MAX - part {
            return Err(invalid("working overflow"));
        }
        total += part;
    }
    Ok(total)
}
pub fn plan_regions(
    f: &RegionalFilter,
    dims: [i64; 3],
    spacing: [f64; 3],
    requested: Region,
    opts: RegionalOptions,
) -> Result<RegionalResult, (RegionalResult, FilterError)> {
    let mut res = RegionalResult::default();
    plan_impl(f, dims, spacing, requested, opts, &mut res)
        .map(|_| res)
        .map_err(|e| (res, e))
}
fn plan_impl(
    f: &RegionalFilter,
    dims: [i64; 3],
    spacing: [f64; 3],
    requested: Region,
    opts: RegionalOptions,
    res: &mut RegionalResult,
) -> Result<RegionalKernel, FilterError> {
    if voxel_count(dims).is_none() || !regional_valid(requested, dims) {
        return Err(invalid("regional dimensions/request"));
    }
    for (a, s) in spacing.iter().enumerate() {
        if dims[a] > 1 && !(*s > 0.0 && *s <= f64::MAX) {
            return Err(invalid("spacing"));
        }
    }
    let k = regional_params(f, dims, spacing)?;
    res.halo = k.radius;
    for (axis, halo) in k.radius.iter().enumerate() {
        if dims[axis] - 1 > i64::MAX - halo {
            return Err(invalid(&format!(
                "global coordinate plus halo overflows axis {axis}"
            )));
        }
    }
    let budget = if opts.max_working_bytes == 0 {
        DEFAULT_SLAB_WORKING_BYTES
    } else {
        opts.max_working_bytes
    };
    if budget < 0 || opts.max_read_bytes < 0 {
        return Err(invalid("regional bounds"));
    }
    let minimum = cost(&k, dims, [1, 1, 1])?;
    if minimum > budget {
        return Err(budget_error(minimum, budget));
    }
    let mut tile = opts.tile_size;
    for a in 0..3 {
        let n = requested.size[a];
        if tile[a] < 0 {
            return Err(invalid("tile size"));
        }
        if tile[a] == 0 {
            tile[a] = n.min([64, 64, 16][a]);
        }
        tile[a] = tile[a].min(n);
    }
    loop {
        let n = cost(&k, dims, tile)?;
        if n <= budget {
            res.working_bytes = n;
            break;
        }
        let mut axis = 0;
        for a in 1..3 {
            if tile[a] > tile[axis] {
                axis = a;
            }
        }
        tile[axis] = (tile[axis] + 1) / 2;
    }
    res.tile_size = tile;
    let mut tiles: i64 = 1;
    let mut win = [0i64; 3];
    for a in 0..3 {
        let n = requested.size[a];
        let c = (n - 1) / tile[a] + 1;
        if tiles > i64::MAX / c {
            return Err(invalid("tile count"));
        }
        tiles *= c;
        win[a] = regional_extent(dims[a], tile[a], k.radius[a]);
    }
    let mut bytes = checked_product(&[tiles, win[0], win[1], win[2], 4])?;
    if k.kind == 1 {
        match checked_product(&[dims[0], dims[1], dims[2], 4]) {
            Ok(full) if bytes <= i64::MAX - full => bytes += full,
            _ => return Err(invalid("classification read overflow")),
        }
    }
    res.planned_read_bytes = bytes;
    if opts.max_read_bytes > 0 && bytes > opts.max_read_bytes {
        return Err(FilterError::new(
            ErrorKind::RegionalBudget,
            format!("imagefilter: regional working budget: read bound needs {bytes} bytes"),
        ));
    }
    Ok(k)
}
impl Volume {
    fn region_check(&self, r: Region, len: usize) -> Result<(), FilterError> {
        match voxel_count(r.size) {
            Some(n) if regional_valid(r, self.dims) && len as i64 == n => {}
            _ => return Err(invalid("region buffer")),
        }
        match voxel_count(self.dims) {
            Some(total) if self.data.len() as i64 == total => Ok(()),
            _ => Err(invalid("volume buffer")),
        }
    }
}
impl RegionReader for Volume {
    fn read_region(&self, ctx: Ctx<'_>, r: Region, dst: &mut [f32]) -> Result<(), FilterError> {
        self.region_check(r, dst.len())?;
        let s = r.size.map(|v| v as usize);
        for z in 0..s[2] {
            for y in 0..s[1] {
                ctx_err(ctx)?;
                let i = self.index(r.origin[0], r.origin[1] + y as i64, r.origin[2] + z as i64)
                    as usize;
                let j = (z * s[1] + y) * s[0];
                dst[j..j + s[0]].copy_from_slice(&self.data[i..i + s[0]]);
            }
        }
        Ok(())
    }
}
impl RegionWriter for Volume {
    fn write_region(&mut self, ctx: Ctx<'_>, r: Region, src: &[f32]) -> Result<(), FilterError> {
        self.region_check(r, src.len())?;
        let s = r.size.map(|v| v as usize);
        for z in 0..s[2] {
            for y in 0..s[1] {
                ctx_err(ctx)?;
                let i = self.index(r.origin[0], r.origin[1] + y as i64, r.origin[2] + z as i64)
                    as usize;
                let j = (z * s[1] + y) * s[0];
                self.data[i..i + s[0]].copy_from_slice(&src[j..j + s[0]]);
            }
        }
        Ok(())
    }
}
fn freeze_workers(f: &RegionalFilter) -> RegionalFilter {
    match *f {
        RegionalFilter::Median(mut o) => {
            o.workers = worker_bound(o.workers);
            RegionalFilter::Median(o)
        }
        RegionalFilter::Bilateral(mut o) => {
            o.workers = worker_bound(o.workers);
            RegionalFilter::Bilateral(o)
        }
        RegionalFilter::NonLocalMeans(mut o) => {
            o.workers = worker_bound(o.workers);
            RegionalFilter::NonLocalMeans(o)
        }
        other => other,
    }
}
pub fn filter_regions(
    ctx: Ctx<'_>,
    f: &RegionalFilter,
    io: RegionalIo<'_>,
    dims: [i64; 3],
    spacing: [f64; 3],
    requested: Region,
    opts: RegionalOptions,
) -> (RegionalResult, Result<(), FilterError>) {
    let mut res = RegionalResult::default();
    let (Some(src), Some(dst)) = (io.src, io.dst) else {
        return (res, Err(invalid("nil regional IO")));
    };
    let f = freeze_workers(f);
    let k = match plan_impl(&f, dims, spacing, requested, opts, &mut res) {
        Ok(k) => k,
        Err(e) => return (res, Err(e)),
    };
    let r = run_regions(ctx, &f, &k, src, dims, spacing, requested, dst, &mut res);
    (res, r)
}
#[allow(clippy::too_many_arguments)]
fn run_regions(
    ctx: Ctx<'_>,
    f: &RegionalFilter,
    k: &RegionalKernel,
    src: &dyn RegionReader,
    dims: [i64; 3],
    spacing: [f64; 3],
    requested: Region,
    dst: &mut dyn RegionWriter,
    res: &mut RegionalResult,
) -> Result<(), FilterError> {
    ctx_err(ctx)?;
    let mut max_win = [0i64; 3];
    for a in 0..3 {
        max_win[a] = regional_extent(dims[a], res.tile_size[a], k.radius[a]);
    }
    let ni = voxel_count(max_win).unwrap_or(0) as usize;
    let no = voxel_count(res.tile_size).unwrap_or(0) as usize;
    let mut input = vec![0f32; ni];
    let mut output = vec![0f32; no];
    let read = |r: Region, buf: &mut [f32], res: &mut RegionalResult| -> Result<(), FilterError> {
        ctx_err(ctx)?;
        src.read_region(ctx, r, buf)?;
        res.read_bytes += buf.len() as i64 * 4;
        if buf.iter().any(|v| !v.is_finite()) {
            return Err(invalid("nonfinite regional sample"));
        }
        ctx_err(ctx)
    };
    let mut integral = true;
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    if k.kind == 1 {
        let mut z = 0;
        while z < dims[2] {
            let dz = max_win[2].min(dims[2] - z);
            let mut y = 0;
            while y < dims[1] {
                let dy = max_win[1].min(dims[1] - y);
                let mut x = 0;
                while x < dims[0] {
                    let dx = max_win[0].min(dims[0] - x);
                    let r = Region {
                        origin: [x, y, z],
                        size: [dx, dy, dz],
                    };
                    let n = (dx * dy * dz) as usize;
                    read(r, &mut input[..n], res)?;
                    for v in &input[..n] {
                        let v = *v;
                        integral = integral
                            && v == f64::from(v).trunc() as f32
                            && v < 16_777_216.0
                            && v > -16_777_216.0;
                        lo = min_nan32(lo, v);
                        hi = max_nan32(hi, v);
                    }
                    x += dx;
                }
                y += dy;
            }
            z += dz;
        }
        integral = integral && f64::from(hi) - f64::from(lo) + 1.0 <= 65536.0;
    }
    let bilateral = match f {
        RegionalFilter::Bilateral(o) if k.kind == 2 => {
            Some(BilateralKernel::new(dims, spacing, o)?)
        }
        _ => None,
    };
    let mut nlm = NlmState::default();
    let ud = dims.map(|d| d as usize);
    let mut bx = vec![0f32; k.block_len.max(0) as usize];
    let end = [
        requested.origin[0] + requested.size[0],
        requested.origin[1] + requested.size[1],
        requested.origin[2] + requested.size[2],
    ];
    let mut z = requested.origin[2];
    while z < end[2] {
        let tz = res.tile_size[2].min(end[2] - z);
        let mut y = requested.origin[1];
        while y < end[1] {
            let ty = res.tile_size[1].min(end[1] - y);
            let mut x = requested.origin[0];
            while x < end[0] {
                let tx = res.tile_size[0].min(end[0] - x);
                let tile = Region {
                    origin: [x, y, z],
                    size: [tx, ty, tz],
                };
                let w = regional_window(tile, dims, k.radius);
                let n = (w.size[0] * w.size[1] * w.size[2]) as usize;
                read(w, &mut input[..n], res)?;
                let view = SlabView {
                    data: &input[..n],
                    dims: ud,
                    lo: 0,
                    hi: 0,
                    region_origin: w.origin.map(|v| v as usize),
                    region_size: w.size.map(|v| v as usize),
                };
                let m = (tx * ty * tz) as usize;
                let part = &mut output[..m];
                if k.kind == 3 {
                    regional_nlm(ctx, k, &mut nlm, tile, &view, part)?;
                } else {
                    let (tx, ty) = (tx as usize, ty as usize);
                    for line in 0..ty * tz as usize {
                        ctx_err(ctx)?;
                        let gy = y as usize + line % ty;
                        let gz = z as usize + line / ty;
                        for ix in 0..tx {
                            let gx = x as usize + ix;
                            if k.kind == 1 {
                                let mut j = 0;
                                let r = k.radius;
                                for dx in -r[0]..=r[0] {
                                    for dz in -r[2]..=r[2] {
                                        for dy in -r[1]..=r[1] {
                                            bx[j] = view.at(
                                                clamp_axis(gx as i64 + dx, ud[0]),
                                                clamp_axis(gy as i64 + dy, ud[1]),
                                                clamp_axis(gz as i64 + dz, ud[2]),
                                            );
                                            j += 1;
                                        }
                                    }
                                }
                                let mut v = select_rank(&mut bx, k.block_len as usize / 2);
                                if integral {
                                    v = lo + (v - lo) as i64 as f32;
                                }
                                part[line * tx + ix] = v;
                            } else if let Some(b) = &bilateral {
                                let centre = f64::from(view.at(gx, gy, gz));
                                let (mut sum, mut weights) = (0.0, 0.0);
                                for t in &b.taps {
                                    let (sx, sy, sz) = (
                                        (gx as i64).wrapping_add(t.dx),
                                        (gy as i64).wrapping_add(t.dy),
                                        (gz as i64).wrapping_add(t.dz),
                                    );
                                    if sx < 0
                                        || sy < 0
                                        || sz < 0
                                        || sx >= dims[0]
                                        || sy >= dims[1]
                                        || sz >= dims[2]
                                    {
                                        continue;
                                    }
                                    let v =
                                        f64::from(view.at(sx as usize, sy as usize, sz as usize));
                                    let d = v - centre;
                                    let weight = t.w * fmath::exp(d * d * b.range_scale);
                                    sum += weight * v;
                                    weights += weight;
                                }
                                part[line * tx + ix] = (sum / weights) as f32;
                            }
                        }
                    }
                    ctx_err(ctx)?;
                }
                ctx_err(ctx)?;
                dst.write_region(ctx, tile, part)?;
                res.tiles += 1;
                x += tx;
            }
            y += ty;
        }
        z += tz;
    }
    ctx_err(ctx)
}
#[derive(Default)]
struct NlmState {
    kernel: Option<NlmKernel>,
    planes: Vec<f32>,
}
fn regional_centres(n: i64, r: i64, step: i64, start: i64, size: i64) -> Vec<usize> {
    let lo = 0.max(start - r);
    let end = start + size - 1;
    let hi = end + r.min(n - 1 - end);
    let mut first = lo;
    let rem = lo % step;
    if rem != 0 {
        let inc = step - rem;
        if inc > n - 1 - lo {
            first = n - 1;
        } else {
            first = lo + inc;
        }
    }
    let mut out = Vec::new();
    let mut c = first;
    while c < n - 1 && c <= hi {
        out.push(c as usize);
        if step > n - 1 - c {
            break;
        }
        c += step;
    }
    if n - 1 >= lo && n - 1 <= hi {
        out.push((n - 1) as usize);
    }
    out
}
fn regional_nlm(
    ctx: Ctx<'_>,
    p: &RegionalKernel,
    state: &mut NlmState,
    tile: Region,
    input: &SlabView<'_>,
    out: &mut [f32],
) -> Result<(), FilterError> {
    let k = state.kernel.get_or_insert_with(|| NlmKernel {
        dims: p.nlm.dims.map(|d| d as usize),
        patch: p.nlm.patch.map(|d| d as usize),
        search: p.nlm.search.map(|d| d as usize),
        cutoff: p.nlm.cutoff,
        inv_h2: p.nlm.inv_h2,
        workers: p.workers,
        centres: Default::default(),
        cover_lo: Default::default(),
        cover_hi: Default::default(),
        block_len: p.nlm.block_len as usize,
        row_offsets: Vec::new(),
        ring: 0,
        planes: Vec::new(),
        patch_buf: Vec::new(),
        other: Vec::new(),
        patch64: Vec::new(),
        acc: Vec::new(),
    });
    for a in 0..3 {
        k.centres[a] = regional_centres(
            p.nlm.dims[a],
            p.nlm.patch[a],
            p.nlm.step[a],
            tile.origin[a],
            tile.size[a],
        );
    }
    let (nbx, nby, nbz) = (k.centres[0].len(), k.centres[1].len(), k.centres[2].len());
    let bl = k.block_len;
    let plane_len = nbx * nby * bl;
    if state.planes.len() < plane_len * nbz {
        state.planes = vec![0f32; plane_len * nbz];
    }
    let planes = &mut state.planes[..plane_len * nbz];
    k.row_offsets.clear();
    let pa = k.patch.map(|v| v as i64);
    let rs = input.region_size.map(|v| v as i64);
    for dz in -pa[2]..=pa[2] {
        for dy in -pa[1]..=pa[1] {
            k.row_offsets.push((dz * rs[1] + dy) * rs[0]);
        }
    }
    let centres_z = k.centres[2].clone();
    let centres_y = k.centres[1].clone();
    for (iz, cz) in centres_z.iter().copied().enumerate() {
        let plane = &mut planes[iz * plane_len..(iz + 1) * plane_len];
        for (iy, cy) in centres_y.iter().copied().enumerate() {
            ctx_err(ctx)?;
            k.block_row(
                input,
                &mut plane[iy * nbx * bl..(iy + 1) * nbx * bl],
                cy,
                cz,
            );
        }
        ctx_err(ctx)?;
    }
    let (bx, by) = (2 * k.patch[0] + 1, 2 * k.patch[1] + 1);
    let s = tile.size.map(|v| v as usize);
    let o = tile.origin.map(|v| v as usize);
    for line in 0..s[1] * s[2] {
        ctx_err(ctx)?;
        let y = o[1] + line % s[1];
        let z = o[2] + line / s[1];
        for ix in 0..s[0] {
            let x = o[0] + ix;
            let mut sum = 0.0;
            let mut count = 0usize;
            for (iz, cz) in k.centres[2].iter().copied().enumerate() {
                if z.abs_diff(cz) > k.patch[2] {
                    continue;
                }
                for (iy, cy) in k.centres[1].iter().copied().enumerate() {
                    if y.abs_diff(cy) > k.patch[1] {
                        continue;
                    }
                    for (jx, cx) in k.centres[0].iter().copied().enumerate() {
                        if x.abs_diff(cx) > k.patch[0] {
                            continue;
                        }
                        let offset = ((iz * nby + iy) * nbx + jx) * bl
                            + ((z + k.patch[2] - cz) * by + y + k.patch[1] - cy) * bx
                            + x
                            + k.patch[0]
                            - cx;
                        sum += f64::from(planes[offset]);
                        count += 1;
                    }
                }
            }
            out[line * s[0] + ix] = (sum / count as f64) as f32;
        }
    }
    ctx_err(ctx)
}
pub(super) fn regional_params_pub(
    f: &RegionalFilter,
    dims: [i64; 3],
    spacing: [f64; 3],
) -> Result<[i64; 3], FilterError> {
    regional_params(f, dims, spacing).map(|k| k.radius)
}
pub(super) fn regional_valid_pub(r: Region, dims: [i64; 3]) -> bool {
    regional_valid(r, dims)
}
pub(super) fn regional_window_pub(r: Region, dims: [i64; 3], halo: [i64; 3]) -> Region {
    regional_window(r, dims, halo)
}
pub(super) fn worker_frozen(f: &RegionalFilter) -> RegionalFilter {
    freeze_workers(f)
}
pub(super) fn checked_product_pub(values: &[i64]) -> Result<i64, FilterError> {
    checked_product(values)
}
