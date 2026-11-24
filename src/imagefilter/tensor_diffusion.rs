use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
use crate::numtext::format_float;
use sha2::{Digest, Sha256};
fn bad(s: &str) -> FilterError {
    FilterError::new(
        ErrorKind::TensorDiffusionInput,
        format!("imagefilter: invalid tensor diffusion input: {s}"),
    )
}
fn limit(detail: String) -> FilterError {
    FilterError::new(
        ErrorKind::TensorDiffusionLimit,
        format!("imagefilter: tensor diffusion resource limit: {detail}"),
    )
}
pub const COHERENCE_ENHANCING: &str = "CED";
pub const EDGE_ENHANCING: &str = "EED";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TensorDiffusionLimits {
    pub max_voxels: i64,
    pub max_scratch_bytes: i64,
    pub max_working_bytes: i64,
    pub max_stencil_visits: i64,
    pub max_iterations: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TensorDiffusionOptions {
    pub method: String,
    pub sigma_mm: [f64; 3],
    pub rho_mm: [f64; 3],
    pub alpha: f64,
    pub threshold: f64,
    pub exponent: i64,
    pub iterations: i64,
    pub time_step: f64,
    pub workers: i64,
    pub source_reference: String,
    pub limits: TensorDiffusionLimits,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TensorDiffusionMetadata {
    pub derived: bool,
    pub method: String,
    pub domain: String,
    pub boundary: String,
    pub discretization: String,
    pub source_sha256: String,
    pub source_reference: String,
    pub dims: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub sigma_mm: [f64; 3],
    pub rho_mm: [f64; 3],
    pub alpha: f64,
    pub threshold: f64,
    pub time_step: f64,
    pub stability_limit: f64,
    pub total_time: f64,
    pub exponent: i64,
    pub iterations_completed: i64,
    pub workers_requested: i64,
    pub scratch_bytes: i64,
    pub working_bytes: i64,
    pub stencil_visit_bound: i64,
    pub initial_mean: f64,
    pub final_mean: f64,
    pub initial_l2_squared: f64,
    pub final_l2_squared: f64,
    pub residual_rms: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct TensorDiffusionResult {
    pub volume: Volume,
    pub metadata: TensorDiffusionMetadata,
}
#[derive(Default)]
struct Plan {
    n: usize,
    iterations: i64,
    exponent: i64,
    active: Vec<usize>,
    strides: [usize; 3],
    inverse: [f64; 3],
    dt: f64,
    stability: f64,
    radii: [[usize; 3]; 2],
    kernels: [[Vec<f64>; 3]; 2],
    scratch: i64,
    working: i64,
    visits: i64,
}
pub fn tensor_anisotropic_diffusion(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    opts: &TensorDiffusionOptions,
) -> Result<TensorDiffusionResult, FilterError> {
    let mut p = admit(ctx, src, opts)?;
    let Some(src) = src else {
        return Err(bad("nil volume"));
    };
    for kind in 0..2 {
        for &axis in &p.active.clone() {
            let r = p.radii[kind][axis];
            if r == 0 {
                continue;
            }
            let sigma = if kind == 1 {
                opts.rho_mm[axis]
            } else {
                opts.sigma_mm[axis]
            };
            let mut kernel = vec![0f64; 2 * r + 1];
            let mut sum = 0.0;
            for j in -(r as i64)..=r as i64 {
                let x = j as f64 * src.spacing_mm[axis] / sigma;
                let k = fmath::exp(-0.5 * x * x);
                kernel[(j + r as i64) as usize] = k;
                sum += k;
            }
            for k in kernel.iter_mut() {
                *k /= sum;
            }
            p.kernels[kind][axis] = kernel;
        }
    }
    let n = p.n;
    let dims = src.dims.map(|d| d as usize);
    let mut current = vec![0f64; n];
    let mut next = vec![0f64; n];
    let mut smooth = vec![0f64; n];
    let mut scratch = vec![0f64; n];
    let mut tensor: [Vec<f64>; 6] = Default::default();
    for t in tensor.iter_mut() {
        *t = vec![0f64; n];
    }
    let mut h = Sha256::new();
    let (mut initial_mean, mut initial_l2) = (0.0, 0.0);
    for (i, value) in src.data.iter().copied().enumerate() {
        current[i] = f64::from(value);
        initial_mean += current[i];
        initial_l2 += current[i] * current[i];
        h.update(value.to_bits().to_le_bytes());
    }
    ctx_err(ctx)?;
    for _ in 0..p.iterations {
        smooth.copy_from_slice(&current);
        p.smooth(ctx, dims, &mut smooth, &mut scratch, 0)?;
        for i in 0..n {
            let g = p.gradient(dims, &smooth, i);
            for a in 0..3 {
                for b in a..3 {
                    tensor[component(a, b)][i] = g[a] * g[b];
                }
            }
        }
        ctx_err(ctx)?;
        for t in tensor.iter_mut() {
            p.smooth(ctx, dims, t, &mut scratch, 1)?;
        }
        for i in 0..n {
            let mut j = [0f64; 6];
            for (k, jk) in j.iter_mut().enumerate() {
                *jk = tensor[k][i];
            }
            let d = diffusivity(
                j,
                &p.active,
                &opts.method,
                opts.alpha,
                opts.threshold,
                p.exponent,
            );
            for (k, dk) in d.iter().enumerate() {
                tensor[k][i] = *dk;
            }
        }
        ctx_err(ctx)?;
        for i in 0..n {
            next[i] = current[i] + p.dt * p.update(dims, &current, &tensor, i);
        }
        ctx_err(ctx)?;
        std::mem::swap(&mut current, &mut next);
    }
    let mut data = vec![0f32; n];
    let (mut final_mean, mut final_l2, mut residual) = (0.0, 0.0, 0.0);
    for (i, value) in current.iter().copied().enumerate() {
        if value.is_nan() || value.is_infinite() || value.abs() > f64::from(f32::MAX) {
            return Err(FilterError::new(
                ErrorKind::TensorDiffusionNumeric,
                format!("imagefilter: unsupported tensor diffusion arithmetic: output index {i}"),
            ));
        }
        data[i] = value as f32;
        final_mean += value;
        final_l2 += value * value;
        let d = value - f64::from(src.data[i]);
        residual += d * d;
    }
    ctx_err(ctx)?;
    let sha: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(TensorDiffusionResult {
        volume: Volume {
            dims: src.dims,
            spacing_mm: src.spacing_mm,
            data,
        },
        metadata: TensorDiffusionMetadata {
            derived: true,
            method: opts.method.clone(),
            domain: "derived-scalar-intensity-estimate".into(),
            boundary: "natural-absent-differences; replicated-Gaussian".into(),
            discretization: "orthant-average-variational".into(),
            source_sha256: sha,
            source_reference: opts.source_reference.clone(),
            dims: src.dims,
            spacing_mm: src.spacing_mm,
            sigma_mm: opts.sigma_mm,
            rho_mm: opts.rho_mm,
            alpha: opts.alpha,
            threshold: opts.threshold,
            exponent: p.exponent,
            time_step: p.dt,
            stability_limit: p.stability,
            total_time: p.dt * p.iterations as f64,
            iterations_completed: p.iterations,
            workers_requested: opts.workers,
            scratch_bytes: p.scratch,
            working_bytes: p.working,
            stencil_visit_bound: p.visits,
            initial_mean: initial_mean / n as f64,
            final_mean: final_mean / n as f64,
            initial_l2_squared: initial_l2,
            final_l2_squared: final_l2,
            residual_rms: (residual / n as f64).sqrt(),
        },
    })
}
fn check(v: i64, def: i64, hard: i64, need: i64, label: &str) -> Result<(), FilterError> {
    if v < 0 || v > hard {
        return Err(bad(label));
    }
    let v = if v == 0 { def } else { v };
    if need > v {
        return Err(limit(format!("{label} requires {need} > {v}")));
    }
    Ok(())
}
fn admit(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    o: &TensorDiffusionOptions,
) -> Result<Plan, FilterError> {
    ctx_err(ctx)?;
    let mut p = Plan::default();
    let Some(src) = src else {
        return Err(bad("nil volume"));
    };
    let n = match voxel_count(src.dims) {
        Some(n) if n == src.data.len() as i64 => n,
        _ => return Err(bad("dimensions/data")),
    };
    p.n = n as usize;
    if o.method != COHERENCE_ENHANCING && o.method != EDGE_ENHANCING {
        return Err(bad("method must be CED or EED"));
    }
    if !(o.alpha >= 1e-6 && o.alpha <= 1.0) || !(o.threshold >= 1e-24 && o.threshold <= 1e24) {
        return Err(bad("alpha/threshold"));
    }
    p.iterations = if o.iterations == 0 { 5 } else { o.iterations };
    p.exponent = if o.exponent == 0 { 2 } else { o.exponent };
    if p.iterations < 1
        || p.iterations > 1000
        || p.exponent < 1
        || p.exponent > 8
        || o.workers < 0
        || o.workers > 64
    {
        return Err(bad("iterations/exponent/workers"));
    }
    if o.source_reference.len() > 4096 {
        return Err(bad("source reference exceeds 4096 bytes"));
    }
    check(o.limits.max_voxels, 1 << 20, 1 << 24, n, "voxels")?;
    check(
        o.limits.max_iterations,
        128,
        1000,
        p.iterations,
        "iterations",
    )?;
    let d = src.dims.map(|v| v as usize);
    p.strides = [1, d[0], d[0] * d[1]];
    let mut inv_sum = 0.0;
    let (mut kernel_bytes, mut widths) = ([0i64; 2], [0i64; 2]);
    for (axis, h) in src.spacing_mm.iter().copied().enumerate() {
        if src.dims[axis] > 1 {
            if !(h >= 1e-4 && h <= 1e4) {
                return Err(bad("active spacing outside [1e-4,1e4] mm"));
            }
            p.active.push(axis);
            p.inverse[axis] = 1.0 / h;
            inv_sum += 1.0 / (h * h);
        } else if !(h >= 0.0 && h <= f64::MAX) {
            return Err(bad("inactive spacing"));
        }
        for (kind, scale) in [o.sigma_mm[axis], o.rho_mm[axis]].into_iter().enumerate() {
            if !(scale >= 0.0 && scale <= 1e4) {
                return Err(bad("Gaussian scale outside [0,1e4] mm"));
            }
            if src.dims[axis] == 1 || scale == 0.0 {
                continue;
            }
            let radius = (3.0 * scale / h).ceil();
            if radius > 64.0 {
                return Err(limit(format!(
                    "Gaussian axis {axis} needs radius {} > 64",
                    format_float(radius, 'g', -1)
                )));
            }
            p.radii[kind][axis] = radius as usize;
            widths[kind] += 2 * radius as i64 + 1;
            kernel_bytes[kind] += 8 * (2 * radius as i64 + 1);
        }
    }
    if p.active.len() < 2 {
        return Err(bad("requires two or three active axes"));
    }
    p.stability = 1.0 / (2.0 * inv_sum);
    p.dt = o.time_step;
    if p.dt == 0.0 {
        p.dt = 0.9 * p.stability;
    }
    if !(p.dt > 0.0 && p.dt <= p.stability) {
        return Err(bad("time step exceeds physical stability limit"));
    }
    p.scratch = 80 * n + kernel_bytes[0] + kernel_bytes[1];
    p.working = p.scratch + 4 * n + o.source_reference.len() as i64;
    p.visits = n * (p.iterations * (widths[0] + 6 * widths[1] + 128) + 8);
    check(
        o.limits.max_scratch_bytes,
        128 << 20,
        2 << 30,
        p.scratch,
        "scratch bytes",
    )?;
    check(
        o.limits.max_working_bytes,
        160 << 20,
        2 << 30,
        p.working,
        "working bytes",
    )?;
    check(
        o.limits.max_stencil_visits,
        2_000_000_000,
        50_000_000_000,
        p.visits,
        "stencil visits",
    )?;
    for v in &src.data {
        if v.is_nan() || v.is_infinite() {
            return Err(bad("nonfinite input sample"));
        }
        if f64::from(*v).abs() > 1e30 {
            return Err(FilterError::new(
                ErrorKind::TensorDiffusionNumeric,
                "imagefilter: unsupported tensor diffusion arithmetic: input magnitude exceeds 1e30",
            ));
        }
    }
    ctx_err(ctx)?;
    Ok(p)
}
fn component(a: usize, b: usize) -> usize {
    let (a, b) = if a > b { (b, a) } else { (a, b) };
    [[0, 1, 2], [1, 3, 4], [2, 4, 5]][a][b]
}
impl Plan {
    fn smooth(
        &self,
        ctx: Ctx<'_>,
        dims: [usize; 3],
        data: &mut [f64],
        scratch: &mut [f64],
        kind: usize,
    ) -> Result<(), FilterError> {
        for &axis in &self.active {
            let kernel = &self.kernels[kind][axis];
            if kernel.is_empty() {
                continue;
            }
            let (stride, r) = (self.strides[axis], kernel.len() / 2);
            for i in 0..self.n {
                let c = (i / stride) % dims[axis];
                let mut sum = 0.0;
                for (j, w) in kernel.iter().enumerate() {
                    let neighbor = (c + j).saturating_sub(r).min(dims[axis] - 1);
                    let idx = (i as i64 + (neighbor as i64 - c as i64) * stride as i64) as usize;
                    sum += w * data[idx];
                }
                scratch[i] = sum;
            }
            ctx_err(ctx)?;
            data.copy_from_slice(scratch);
        }
        ctx_err(ctx)
    }
    fn gradient(&self, dims: [usize; 3], data: &[f64], i: usize) -> [f64; 3] {
        let mut g = [0f64; 3];
        for &a in &self.active {
            let c = (i / self.strides[a]) % dims[a];
            let (mut lo, mut hi) = (i, i);
            if c > 0 {
                lo -= self.strides[a];
            }
            if c + 1 < dims[a] {
                hi += self.strides[a];
            }
            g[a] = (data[hi] - data[lo]) * 0.5 * self.inverse[a];
        }
        g
    }
    fn update(&self, dims: [usize; 3], u: &[f64], d: &[Vec<f64>; 6], i: usize) -> f64 {
        let mut delta = 0.0;
        for &a in &self.active {
            let (stride, ih) = (self.strides[a], self.inverse[a]);
            let c = (i / stride) % dims[a];
            let comp = component(a, a);
            let cross = |j: usize| -> f64 {
                let g = self.gradient(dims, u, j);
                let mut value = 0.0;
                for &b in &self.active {
                    if b != a {
                        value += d[component(a, b)][j] * g[b];
                    }
                }
                value
            };
            if c > 0 {
                let j = i - stride;
                delta += 0.5 * (d[comp][i] + d[comp][j]) * (u[j] - u[i]) * ih * ih;
                delta -= 0.5 * ih * cross(j);
            } else {
                delta += 0.5 * ih * cross(i);
            }
            if c + 1 < dims[a] {
                let j = i + stride;
                delta += 0.5 * (d[comp][i] + d[comp][j]) * (u[j] - u[i]) * ih * ih;
                delta += 0.5 * ih * cross(j);
            } else {
                delta -= 0.5 * ih * cross(i);
            }
        }
        delta
    }
}
fn diffusivity(
    j: [f64; 6],
    active: &[usize],
    method: &str,
    alpha: f64,
    threshold: f64,
    exponent: i64,
) -> [f64; 6] {
    let transfer = |gap: f64| -> f64 {
        let mut e = 0.0;
        if gap > 0.0 {
            let r = threshold / gap;
            let mut x = 1.0;
            for _ in 0..exponent {
                x *= r;
            }
            e = fmath::exp(-x);
        }
        if method == COHERENCE_ENHANCING {
            return alpha + (1.0 - alpha) * e;
        }
        1.0 - (1.0 - alpha) * e
    };
    let mut d = [0f64; 6];
    if alpha == 1.0 {
        for &a in active {
            d[component(a, a)] = 1.0;
        }
        return d;
    }
    if active.len() == 2 {
        let (a, b) = (active[0], active[1]);
        let (x, y, z) = (j[component(a, a)], j[component(a, b)], j[component(b, b)]);
        let gap = fmath::hypot(x - z, 2.0 * y);
        if gap == 0.0 {
            let v = transfer(0.0);
            d[component(a, a)] = v;
            d[component(b, b)] = v;
            return d;
        }
        let (mut lo, mut hi) = (transfer(gap), transfer(0.0));
        if method == EDGE_ENHANCING {
            std::mem::swap(&mut lo, &mut hi);
        }
        let (average, difference) = (0.5 * (hi + lo), 0.5 * (hi - lo));
        d[component(a, a)] = average + difference * (x - z) / gap;
        d[component(b, b)] = average - difference * (x - z) / gap;
        d[component(a, b)] = difference * 2.0 * y / gap;
        return d;
    }
    let (values, vectors) = eigen3(j);
    for (k, mu) in values.iter().copied().enumerate() {
        let gap = if method == EDGE_ENHANCING {
            mu - values[0]
        } else {
            values[2] - mu
        };
        let weight = transfer(gap);
        for a in 0..3 {
            for b in a..3 {
                d[component(a, b)] += weight * vectors[a][k] * vectors[b][k];
            }
        }
    }
    d
}
fn eigen3(j: [f64; 6]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut a = [[j[0], j[1], j[2]], [j[1], j[3], j[4]], [j[2], j[4], j[5]]];
    let mut vectors = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let scale = fmath::max(j[0].abs() + j[3].abs() + j[5].abs(), 1e-300);
    for _ in 0..16 {
        let mut changed = false;
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            if a[p][q].abs() <= 1e-15 * scale {
                continue;
            }
            changed = true;
            let tau = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = 1f64.copysign(tau) / (tau.abs() + fmath::hypot(1.0, tau));
            let c = 1.0 / fmath::hypot(1.0, t);
            let s = t * c;
            let (app, aqq, apq) = (a[p][p], a[q][q], a[p][q]);
            a[p][p] = app - t * apq;
            a[q][q] = aqq + t * apq;
            a[p][q] = 0.0;
            a[q][p] = 0.0;
            for r in 0..3 {
                if r != p && r != q {
                    let (x, y) = (a[r][p], a[r][q]);
                    a[r][p] = c * x - s * y;
                    a[p][r] = a[r][p];
                    a[r][q] = s * x + c * y;
                    a[q][r] = a[r][q];
                }
                let (x, y) = (vectors[r][p], vectors[r][q]);
                vectors[r][p] = c * x - s * y;
                vectors[r][q] = s * x + c * y;
            }
        }
        if !changed {
            break;
        }
    }
    let mut values = [0f64; 3];
    for k in 0..3 {
        values[k] = fmath::max(0.0, a[k][k]);
    }
    for k in 0..2 {
        for l in k + 1..3 {
            if values[l] < values[k] {
                values.swap(l, k);
                for row in vectors.iter_mut() {
                    row.swap(l, k);
                }
            }
        }
    }
    (values, vectors)
}
