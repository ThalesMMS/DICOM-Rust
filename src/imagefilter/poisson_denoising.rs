use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
use sha2::{Digest, Sha256};
fn bad(s: &str) -> FilterError {
    FilterError::new(
        ErrorKind::PoissonDenoisingInput,
        format!("imagefilter: invalid Poisson denoising input: {s}"),
    )
}
fn numeric() -> FilterError {
    FilterError::new(
        ErrorKind::PoissonDenoisingNumeric,
        "imagefilter: unsupported Poisson denoising arithmetic",
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoissonDenoisingLimits {
    pub max_voxels: i64,
    pub max_scratch_bytes: i64,
    pub max_working_bytes: i64,
    pub max_neighbor_visits: i64,
    pub max_iterations: i64,
    pub max_source_references: i64,
    pub max_reference_bytes: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PoissonDenoisingOptions {
    pub count_scale: f64,
    pub lambda: f64,
    pub iterations: i64,
    pub relative_change: f64,
    pub source_references: Vec<String>,
    pub limits: PoissonDenoisingLimits,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PoissonDenoisingMetadata {
    pub derived: bool,
    pub method: String,
    pub noise_model: String,
    pub domain: String,
    pub boundary: String,
    pub stopping_reason: String,
    pub source_sha256: String,
    pub source_references: Vec<String>,
    pub count_scale: f64,
    pub lambda: f64,
    pub relative_change: f64,
    pub iterations_requested: i64,
    pub iterations_completed: i64,
    pub fractional_count_samples: i64,
    pub spacing_mm: [f64; 3],
    pub dims: [i64; 3],
    pub scratch_bytes: i64,
    pub working_bytes: i64,
    pub neighbor_visit_bound: i64,
    pub initial_objective: f64,
    pub final_objective: f64,
    pub final_poisson_deviance: f64,
    pub final_penalty: f64,
    pub observed_mean_counts: f64,
    pub estimated_mean_counts: f64,
    pub maximum_change_counts: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PoissonDenoisingResult {
    pub volume: Volume,
    pub metadata: PoissonDenoisingMetadata,
}
#[derive(Default)]
struct Admission {
    n: usize,
    iterations: i64,
    refs_bytes: i64,
    weights: [f64; 3],
    scratch: i64,
    working: i64,
    visits: i64,
}
pub fn denoise_poisson(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    opts: &PoissonDenoisingOptions,
) -> Result<PoissonDenoisingResult, FilterError> {
    let a = admit(ctx, src, opts)?;
    let Some(src) = src else {
        return Err(bad("nil volume"));
    };
    let mut x = vec![0f64; a.n];
    let (mut sum, mut max_count, mut max_source) = (0.0, 0.0, 0.0);
    let mut fractional = 0;
    let mut h = Sha256::new();
    for (i, v) in src.data.iter().copied().enumerate() {
        x[i] = f64::from(v) * opts.count_scale;
        if x[i] != x[i].trunc() {
            fractional += 1;
        }
        sum += x[i];
        max_count = fmath::max(max_count, x[i]);
        max_source = fmath::max(max_source, f64::from(v));
        h.update(v.to_bits().to_le_bytes());
    }
    ctx_err(ctx)?;
    let mut initial = 0.0;
    if opts.lambda != 0.0 {
        initial = objective(ctx, src, &x, opts.count_scale, opts.lambda, a.weights)?.0;
    }
    let mut completed = 0;
    let mut reason = "iteration-limit";
    let mut change = 0.0;
    if opts.lambda == 0.0 {
        reason = "identity";
    } else if max_count == 0.0 {
        reason = "zero-counts";
    } else {
        for _ in 0..a.iterations {
            change = 0.0;
            for i in 0..x.len() {
                if i & 255 == 0 {
                    ctx_err(ctx)?;
                }
                let (neighbors, degree) = neighbors_of(src.dims, &x, i, a.weights);
                let y = f64::from(src.data[i]) * opts.count_scale;
                let next = coordinate(y, opts.lambda * degree, opts.lambda * neighbors);
                if !(next >= 0.0) || next.is_infinite() || (y > 0.0 && next == 0.0) {
                    return Err(numeric());
                }
                change = fmath::max(change, (next - x[i]).abs());
                x[i] = next;
            }
            completed += 1;
            if opts.relative_change > 0.0 && change / max_count <= opts.relative_change {
                reason = "relative-change";
                break;
            }
        }
    }
    let (mut fin, mut deviance, mut penalty) = (0.0, 0.0, 0.0);
    if opts.lambda != 0.0 {
        (fin, deviance, penalty) =
            objective(ctx, src, &x, opts.count_scale, opts.lambda, a.weights)?;
    }
    let mut data = vec![0f32; a.n];
    let mut estimated = 0.0;
    for (i, value) in x.iter().copied().enumerate() {
        estimated += value;
        if opts.lambda == 0.0 {
            data[i] = src.data[i];
            continue;
        }
        let mut physical = value / opts.count_scale;
        if physical > max_source && physical <= max_source * (1.0 + 1e-14) {
            physical = max_source;
        }
        if physical > f64::from(f32::MAX) || !(physical >= 0.0) || physical.is_infinite() {
            return Err(numeric());
        }
        data[i] = physical as f32;
    }
    ctx_err(ctx)?;
    let sha: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(PoissonDenoisingResult {
        volume: Volume {
            dims: src.dims,
            spacing_mm: src.spacing_mm,
            data,
        },
        metadata: PoissonDenoisingMetadata {
            derived: true,
            method: "Poisson-quadratic-MAP-coordinate".into(),
            noise_model: "independent-Poisson".into(),
            domain: "nonnegative-count-intensity-estimate".into(),
            boundary: "natural-axial-no-flux".into(),
            stopping_reason: reason.into(),
            source_sha256: sha,
            source_references: opts.source_references.clone(),
            count_scale: opts.count_scale,
            lambda: opts.lambda,
            relative_change: opts.relative_change,
            iterations_requested: a.iterations,
            iterations_completed: completed,
            fractional_count_samples: fractional,
            dims: src.dims,
            spacing_mm: src.spacing_mm,
            scratch_bytes: a.scratch,
            working_bytes: a.working,
            neighbor_visit_bound: a.visits,
            initial_objective: initial,
            final_objective: fin,
            final_poisson_deviance: deviance,
            final_penalty: penalty,
            observed_mean_counts: sum / a.n as f64,
            estimated_mean_counts: estimated / a.n as f64,
            maximum_change_counts: change,
        },
    })
}
fn check(v: i64, def: i64, hard: i64, need: i64, label: &str) -> Result<(), FilterError> {
    if v < 0 || v > hard {
        return Err(bad(label));
    }
    let v = if v == 0 { def } else { v };
    if need > v {
        return Err(FilterError::new(
            ErrorKind::PoissonDenoisingLimit,
            format!("imagefilter: Poisson denoising resource limit: {label} requires {need} > {v}"),
        ));
    }
    Ok(())
}
fn admit(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    o: &PoissonDenoisingOptions,
) -> Result<Admission, FilterError> {
    ctx_err(ctx)?;
    let mut a = Admission::default();
    let Some(src) = src else {
        return Err(bad("nil volume"));
    };
    let n = match voxel_count(src.dims) {
        Some(n) if n == src.data.len() as i64 => n,
        _ => return Err(bad("dimensions/data")),
    };
    a.n = n as usize;
    if !(o.count_scale >= 1e-12 && o.count_scale <= 1e12) {
        return Err(bad("count scale"));
    }
    if !(o.lambda == 0.0 || (o.lambda >= 1e-12 && o.lambda <= 1e6)) {
        return Err(bad("lambda"));
    }
    if !(o.relative_change >= 0.0 && o.relative_change <= 1.0) {
        return Err(bad("relative change"));
    }
    a.iterations = if o.iterations == 0 { 20 } else { o.iterations };
    if a.iterations < 1 || a.iterations > 4096 {
        return Err(bad("iterations"));
    }
    check(o.limits.max_voxels, 1 << 20, 1 << 24, n, "voxels")?;
    check(
        o.limits.max_iterations,
        128,
        4096,
        a.iterations,
        "iterations",
    )?;
    if o.source_references.is_empty() {
        return Err(bad("source references required"));
    }
    check(
        o.limits.max_source_references,
        32,
        1024,
        o.source_references.len() as i64,
        "references",
    )?;
    for s in &o.source_references {
        if s.is_empty() {
            return Err(bad("empty source reference"));
        }
        if s.len() as i64 > (1 << 20) - a.refs_bytes {
            return Err(FilterError::new(
                ErrorKind::PoissonDenoisingLimit,
                "imagefilter: Poisson denoising resource limit: reference bytes",
            ));
        }
        a.refs_bytes += s.len() as i64;
    }
    check(
        o.limits.max_reference_bytes,
        4096,
        1 << 20,
        a.refs_bytes,
        "reference bytes",
    )?;
    a.scratch = 8 * n;
    a.working = 12 * n + a.refs_bytes + 16 * o.source_references.len() as i64;
    a.visits = n * (6 * a.iterations + 24);
    check(
        o.limits.max_scratch_bytes,
        16 << 20,
        128 << 20,
        a.scratch,
        "scratch bytes",
    )?;
    check(
        o.limits.max_working_bytes,
        32 << 20,
        256 << 20,
        a.working,
        "working bytes",
    )?;
    check(
        o.limits.max_neighbor_visits,
        100_000_000,
        5_000_000_000,
        a.visits,
        "neighbor visits",
    )?;
    for (axis, s) in src.spacing_mm.iter().copied().enumerate() {
        if src.dims[axis] > 1 {
            let w = 1.0 / (s * s);
            if !(s > 0.0 && w >= 1e-12 && w <= 1e12) {
                return Err(bad("active spacing outside physical weight range"));
            }
            a.weights[axis] = w;
        } else if !(s >= 0.0 && s <= f64::MAX) {
            return Err(bad("inactive spacing"));
        }
    }
    for v in &src.data {
        if !(*v >= 0.0) || v.is_infinite() {
            return Err(bad("nonfinite/negative samples"));
        }
    }
    ctx_err(ctx)?;
    Ok(a)
}
fn coordinate(y: f64, a: f64, neighbor_sum: f64) -> f64 {
    if a == 0.0 {
        return y;
    }
    let b = neighbor_sum - 1.0;
    let d = fmath::hypot(b, 2.0 * a.sqrt() * y.sqrt());
    if b >= 0.0 {
        return (b + d) / (2.0 * a);
    }
    2.0 * y / (d - b)
}
fn neighbors_of(d: [i64; 3], x: &[f64], i: usize, w: [f64; 3]) -> (f64, f64) {
    let d = d.map(|v| v as usize);
    let strides = [1, d[0], d[0] * d[1]];
    let coords = [i % d[0], (i / d[0]) % d[1], i / (d[0] * d[1])];
    let (mut sum, mut degree) = (0.0, 0.0);
    for axis in 0..3 {
        if coords[axis] > 0 {
            sum += w[axis] * x[i - strides[axis]];
            degree += w[axis];
        }
        if coords[axis] + 1 < d[axis] {
            sum += w[axis] * x[i + strides[axis]];
            degree += w[axis];
        }
    }
    (sum, degree)
}
fn objective(
    ctx: Ctx<'_>,
    src: &Volume,
    x: &[f64],
    scale: f64,
    lambda: f64,
    w: [f64; 3],
) -> Result<(f64, f64, f64), FilterError> {
    let d = src.dims.map(|v| v as usize);
    let strides = [1, d[0], d[0] * d[1]];
    let (mut likelihood, mut penalty) = (0.0, 0.0);
    for (i, v) in x.iter().copied().enumerate() {
        if i & 255 == 0 {
            ctx_err(ctx)?;
        }
        let y = f64::from(src.data[i]) * scale;
        let mut term = v;
        if y > 0.0 {
            if !(v > 0.0) {
                return Err(numeric());
            }
            let mut logratio = fmath::log(y) - fmath::log(v);
            if (y - v).abs() < v / 2.0 {
                logratio = fmath::log1p((y - v) / v);
            }
            term = v - y + y * logratio;
            if term < 0.0 && term >= -1e-14 * fmath::max(y, v) {
                term = 0.0;
            }
        }
        if term < 0.0 || term.is_infinite() || term.is_nan() {
            return Err(numeric());
        }
        likelihood += term;
        let coords = [i % d[0], (i / d[0]) % d[1], i / (d[0] * d[1])];
        for axis in 0..3 {
            if coords[axis] + 1 < d[axis] {
                let delta = v - x[i + strides[axis]];
                penalty += lambda * 0.5 * w[axis] * delta * delta;
            }
        }
    }
    let total = likelihood + penalty;
    let deviance = 2.0 * likelihood;
    if !(total >= 0.0) || total.is_infinite() {
        return Err(numeric());
    }
    Ok((total, deviance, penalty))
}
