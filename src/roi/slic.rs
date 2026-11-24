use super::{Cancellation, Error, ctx_err};
use crate::fmath;
pub const ERR_SLIC_INPUT: &str = "roi: invalid SLIC input";
pub const ERR_SLIC_LIMIT: &str = "roi: SLIC resource limit";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SlicConnectivity(pub u8);
pub const SLIC_FACE_CONNECTIVITY: SlicConnectivity = SlicConnectivity(0);
pub const SLIC_FULL_CONNECTIVITY: SlicConnectivity = SlicConnectivity(1);
#[derive(Debug, Clone, Copy, Default)]
pub struct SlicGrid<'a> {
    pub size: [i64; 3],
    pub spacing: [f64; 3],
    pub values: &'a [f64],
    pub available: Option<&'a [bool]>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SlicOptions {
    pub target_step_mm: f64,
    pub compactness: f64,
    pub intensity_scale: f64,
    pub iterations: i64,
    pub connectivity: SlicConnectivity,
    pub reject_non_finite: bool,
    pub max_voxels: i64,
    pub max_clusters: i64,
    pub max_bytes: i64,
    pub max_work: i64,
    pub max_iterations: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlicResult {
    pub size: [i64; 3],
    pub spacing: [f64; 3],
    pub labels: Vec<u32>,
    pub options: SlicOptions,
    pub seed_step_voxels: [i64; 3],
    pub seed_step_mm: [f64; 3],
    pub initial_clusters: i64,
    pub clusters: i64,
    pub iterations: i64,
    pub available_voxels: i64,
    pub unavailable_voxels: i64,
    pub converged: bool,
    pub iteration_limit_reached: bool,
    pub connectivity_splits: i64,
    pub objective: f64,
    pub reserved_bytes: i64,
    pub work: i64,
}
#[derive(Debug, Clone, Copy, Default)]
struct SlicCenter {
    x: f64,
    y: f64,
    z: f64,
    value: f64,
}
struct SlicRun<'a> {
    ctx: Option<&'a dyn Cancellation>,
    work: i64,
    limit: i64,
}
impl SlicRun<'_> {
    fn tick(&mut self) -> Result<(), Error> {
        if self.work >= self.limit {
            return Err(Error::wrap(ERR_SLIC_LIMIT, "work"));
        }
        self.work += 1;
        if self.work & 1023 == 0
            && let Some(err) = ctx_err(self.ctx)
        {
            return Err(err);
        }
        Ok(())
    }
}
fn input_error() -> Error {
    Error::sentinel(ERR_SLIC_INPUT)
}
fn slic_options(mut o: SlicOptions) -> Result<SlicOptions, Error> {
    if !o.target_step_mm.is_finite()
        || o.target_step_mm <= 0.0
        || !o.intensity_scale.is_finite()
        || o.intensity_scale <= 0.0
        || !o.compactness.is_finite()
        || o.compactness < 0.0
        || o.iterations <= 0
        || o.connectivity.0 > SLIC_FULL_CONNECTIVITY.0
    {
        return Err(input_error());
    }
    const DEFAULTS: [i64; 4] = [16 << 20, 65536, 1 << 30, 100_000_000];
    let limits = [
        &mut o.max_voxels,
        &mut o.max_clusters,
        &mut o.max_bytes,
        &mut o.max_work,
    ];
    for (p, default) in limits.into_iter().zip(DEFAULTS) {
        if *p < 0 {
            return Err(input_error());
        }
        if *p == 0 {
            *p = default;
        }
    }
    if o.max_iterations < 0 {
        return Err(input_error());
    }
    if o.max_iterations == 0 {
        o.max_iterations = 100;
    }
    if o.iterations > o.max_iterations {
        return Err(Error::wrap(ERR_SLIC_LIMIT, "iterations"));
    }
    if o.max_clusters > i64::from(u32::MAX) {
        return Err(input_error());
    }
    Ok(o)
}
fn slic_index(s: [i64; 3], x: i64, y: i64, z: i64) -> i64 {
    (z * s[1] + y) * s[0] + x
}
fn slic_coords(s: [i64; 3], i: i64) -> [i64; 3] {
    [i % s[0], (i / s[0]) % s[1], i / (s[0] * s[1])]
}
pub fn slic(
    ctx: Option<&dyn Cancellation>,
    g: &SlicGrid<'_>,
    o: SlicOptions,
) -> Result<SlicResult, Error> {
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    let o = slic_options(o)?;
    let magnitude_limit = (f64::MAX / 64.0).sqrt();
    let mut n = 1i64;
    let (mut step, mut cells) = ([0i64; 3], [0i64; 3]);
    for a in 0..3 {
        let d = g.size[a];
        if d <= 0 || !g.spacing[a].is_finite() || g.spacing[a] <= 0.0 {
            return Err(input_error());
        }
        if d > o.max_voxels / n {
            return Err(Error::wrap(ERR_SLIC_LIMIT, "voxels"));
        }
        n *= d;
        let ratio = o.target_step_mm / g.spacing[a];
        if ratio >= d as f64 || ratio == f64::INFINITY {
            step[a] = d;
        } else {
            step[a] = fmath::f64_to_i64((ratio + 0.5).floor());
            if step[a] < 1 {
                step[a] = 1;
            }
        }
        cells[a] = 1 + (d - 1) / step[a];
        let bound = d as f64 * g.spacing[a] / o.target_step_mm;
        if !bound.is_finite()
            || bound.abs() > magnitude_limit
            || !(bound * o.compactness).is_finite()
            || (bound * o.compactness).abs() > magnitude_limit
        {
            return Err(input_error());
        }
    }
    if g.values.len() as i64 != n || g.available.is_some_and(|a| a.len() as i64 != n) {
        return Err(input_error());
    }
    let mut k = 1i64;
    for c in cells {
        if c > o.max_clusters / k {
            return Err(Error::wrap(ERR_SLIC_LIMIT, "initial clusters"));
        }
        k *= c;
    }
    if n > (i64::MAX - 32768) / 64 || k > (i64::MAX - 32768 - 64 * n) / 256 {
        return Err(Error::sentinel(ERR_SLIC_LIMIT));
    }
    let reserved = 32768 + 64 * n + 256 * k;
    if reserved > o.max_bytes {
        return Err(Error::wrap(ERR_SLIC_LIMIT, "bytes"));
    }
    let mut run = SlicRun {
        ctx,
        work: 0,
        limit: o.max_work,
    };
    let available = |i: usize| g.available.is_none_or(|a| a[i]);
    let mut avail = 0i64;
    for (i, &v) in g.values.iter().enumerate() {
        run.tick()?;
        if !available(i) {
            continue;
        }
        if !v.is_finite() {
            if o.reject_non_finite {
                return Err(input_error());
            }
            continue;
        }
        let normalized = v / o.intensity_scale;
        if !normalized.is_finite() || normalized.abs() > magnitude_limit {
            return Err(input_error());
        }
        avail += 1;
    }
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    let mut result = SlicResult {
        size: g.size,
        spacing: g.spacing,
        options: o,
        available_voxels: avail,
        unavailable_voxels: n - avail,
        reserved_bytes: reserved,
        seed_step_voxels: step,
        ..SlicResult::default()
    };
    for a in 0..3 {
        result.seed_step_mm[a] = step[a] as f64 * g.spacing[a];
    }
    let total = n as usize;
    let mut valid = vec![false; total];
    for (i, &v) in g.values.iter().enumerate() {
        run.tick()?;
        valid[i] = available(i) && v.is_finite();
    }
    let mut labels = vec![0u32; total];
    let mut dist = vec![0.0f64; total];
    if avail == 0 {
        result.labels = labels;
        result.converged = true;
        result.work = run.work;
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        return Ok(result);
    }
    let mut centers: Vec<SlicCenter> = Vec::with_capacity(k as usize);
    let mut z = 0i64;
    while z < g.size[2] {
        let mut y = 0i64;
        while y < g.size[1] {
            let mut x = 0i64;
            while x < g.size[0] {
                let end = [
                    x + step[0].min(g.size[0] - x),
                    y + step[1].min(g.size[1] - y),
                    z + step[2].min(g.size[2] - z),
                ];
                let center = [
                    (x as f64 + end[0] as f64 - 1.0) / 2.0,
                    (y as f64 + end[1] as f64 - 1.0) / 2.0,
                    (z as f64 + end[2] as f64 - 1.0) / 2.0,
                ];
                let mut best = f64::INFINITY;
                let mut chosen = -1i64;
                for zz in z..end[2] {
                    for yy in y..end[1] {
                        for xx in x..end[0] {
                            run.tick()?;
                            let i = slic_index(g.size, xx, yy, zz);
                            if !valid[i as usize] {
                                continue;
                            }
                            let mut d = 0.0f64;
                            for (a, c) in [xx, yy, zz].into_iter().enumerate() {
                                let v = (c as f64 - center[a]) * g.spacing[a] / o.target_step_mm;
                                d += v * v;
                            }
                            if d < best {
                                best = d;
                                chosen = i;
                            }
                        }
                    }
                }
                if chosen >= 0 {
                    let c = slic_coords(g.size, chosen);
                    centers.push(SlicCenter {
                        x: c[0] as f64,
                        y: c[1] as f64,
                        z: c[2] as f64,
                        value: g.values[chosen as usize] / o.intensity_scale,
                    });
                }
                x += step[0];
            }
            y += step[1];
        }
        z += step[2];
    }
    result.initial_clusters = centers.len() as i64;
    let score = |i: usize, c: &SlicCenter| -> f64 {
        let coord = slic_coords(g.size, i as i64);
        let v = g.values[i] / o.intensity_scale - c.value;
        let mut d = v * v;
        for (a, p) in [c.x, c.y, c.z].into_iter().enumerate() {
            let delta = (coord[a] as f64 - p) * g.spacing[a] / o.target_step_mm * o.compactness;
            d += delta * delta;
        }
        d
    };
    let mut previous = vec![0u32; total];
    let mut sums = vec![SlicCenter::default(); centers.len()];
    let mut counts = vec![0i64; centers.len()];
    for iteration in 0..o.iterations {
        for i in 0..total {
            run.tick()?;
            previous[i] = labels[i];
        }
        for i in 0..total {
            run.tick()?;
            dist[i] = f64::INFINITY;
            labels[i] = 0;
        }
        for (ci, c) in centers.iter().enumerate() {
            let cc = [c.x, c.y, c.z];
            let (mut lo, mut hi) = ([0i64; 3], [0i64; 3]);
            for a in 0..3 {
                lo[a] = (fmath::f64_to_i64(cc[a].floor()).wrapping_sub(2 * step[a])).max(0);
                hi[a] =
                    (fmath::f64_to_i64(cc[a].ceil()).wrapping_add(2 * step[a])).min(g.size[a] - 1);
            }
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    for x in lo[0]..=hi[0] {
                        run.tick()?;
                        let i = slic_index(g.size, x, y, z) as usize;
                        if !valid[i] {
                            continue;
                        }
                        let d = score(i, c);
                        if !d.is_finite() {
                            return Err(input_error());
                        }
                        if d < dist[i] {
                            dist[i] = d;
                            labels[i] = (ci + 1) as u32;
                        }
                    }
                }
            }
        }
        for i in 0..total {
            if !valid[i] || labels[i] != 0 {
                continue;
            }
            for (ci, c) in centers.iter().enumerate() {
                run.tick()?;
                let d = score(i, c);
                if !d.is_finite() {
                    return Err(input_error());
                }
                if d < dist[i] {
                    dist[i] = d;
                    labels[i] = (ci + 1) as u32;
                }
            }
        }
        for i in 0..sums.len() {
            run.tick()?;
            sums[i] = SlicCenter::default();
            counts[i] = 0;
        }
        let mut changed = false;
        let mut objective = 0.0f64;
        let mut observed = 0i64;
        for i in 0..total {
            let id = labels[i];
            run.tick()?;
            if !valid[i] {
                continue;
            }
            if id == 0 {
                return Err(input_error());
            }
            if id != previous[i] {
                changed = true;
            }
            let ci = id as usize - 1;
            counts[ci] += 1;
            let c = slic_coords(g.size, i as i64);
            let mut a = sums[ci];
            let weight = 1.0 / counts[ci] as f64;
            a.x += (c[0] as f64 - a.x) * weight;
            a.y += (c[1] as f64 - a.y) * weight;
            a.z += (c[2] as f64 - a.z) * weight;
            a.value += (g.values[i] / o.intensity_scale - a.value) * weight;
            sums[ci] = a;
            observed += 1;
            objective += (dist[i] - objective) / observed as f64;
        }
        for (i, c) in sums.iter().enumerate() {
            if counts[i] > 0 {
                centers[i] = *c;
            }
        }
        result.iterations = iteration + 1;
        result.objective = objective;
        if !changed {
            result.converged = true;
            break;
        }
    }
    result.iteration_limit_reached = !result.converged;
    let mut connected = vec![0u32; total];
    let mut queue: Vec<usize> = Vec::with_capacity(total);
    let mut pieces = vec![0i64; centers.len()];
    let mut next = 0u32;
    for start in 0..total {
        let old = labels[start];
        run.tick()?;
        if old == 0 || connected[start] != 0 {
            continue;
        }
        if i64::from(next) >= o.max_clusters {
            return Err(Error::wrap(ERR_SLIC_LIMIT, "connected clusters"));
        }
        next += 1;
        pieces[old as usize - 1] += 1;
        connected[start] = next;
        queue.clear();
        queue.push(start);
        let mut head = 0;
        while head < queue.len() {
            let c = slic_coords(g.size, queue[head] as i64);
            for dz in -1i64..=1 {
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        run.tick()?;
                        let norm = dx.abs() + dy.abs() + dz.abs();
                        if norm == 0 || (o.connectivity == SLIC_FACE_CONNECTIVITY && norm != 1) {
                            continue;
                        }
                        let (x, y, z) = (c[0] + dx, c[1] + dy, c[2] + dz);
                        if x < 0
                            || y < 0
                            || z < 0
                            || x >= g.size[0]
                            || y >= g.size[1]
                            || z >= g.size[2]
                        {
                            continue;
                        }
                        let j = slic_index(g.size, x, y, z) as usize;
                        if labels[j] == old && connected[j] == 0 {
                            connected[j] = next;
                            queue.push(j);
                        }
                    }
                }
            }
            head += 1;
        }
    }
    for &p in &pieces {
        if p > 1 {
            result.connectivity_splits += p - 1;
        }
    }
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    result.labels = connected;
    result.clusters = i64::from(next);
    result.work = run.work;
    Ok(result)
}
