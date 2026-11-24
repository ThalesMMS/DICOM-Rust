use super::recursive_gaussian::Complex128;
use super::volume::{Ctx, FilterError, Volume, ctx_err, dims_v, voxel_count};
use crate::fmath;
use crate::numeric::fft;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct N4Progress {
    pub level: i64,
    pub levels: i64,
    pub iteration: i64,
    pub convergence: f64,
}
#[derive(Default)]
pub struct N4Options<'a> {
    pub mask: Option<Vec<bool>>,
    pub shrink_factor: i64,
    pub iterations: Vec<i64>,
    pub convergence_threshold: f64,
    pub control_points: i64,
    pub histogram_bins: i64,
    pub bias_field_fwhm: f64,
    pub wiener_noise: f64,
    pub workers: i64,
    pub progress: Option<&'a mut dyn FnMut(N4Progress)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct N4Level {
    pub iterations: i64,
    pub convergence: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct N4Result {
    pub corrected: Volume,
    pub log_bias_field: Volume,
    pub levels: Vec<N4Level>,
    pub shrunken_dims: [i64; 3],
    pub samples: i64,
}
const ORDER: usize = 3;
struct Normalized {
    shrink: usize,
    iterations: Vec<i64>,
    threshold: f64,
    control_points: usize,
    bins: usize,
    fwhm: f64,
    noise: f64,
}
fn normalized(o: &N4Options<'_>, n: usize) -> Result<Normalized, FilterError> {
    let shrink = if o.shrink_factor == 0 {
        4
    } else {
        o.shrink_factor
    };
    let iterations = if o.iterations.is_empty() {
        vec![50, 50, 50, 50]
    } else {
        o.iterations.clone()
    };
    let threshold = if o.convergence_threshold == 0.0 {
        0.001
    } else {
        o.convergence_threshold
    };
    let cp = if o.control_points == 0 {
        4
    } else {
        o.control_points
    };
    let bins = if o.histogram_bins == 0 {
        200
    } else {
        o.histogram_bins
    };
    let fwhm = if o.bias_field_fwhm == 0.0 {
        0.15
    } else {
        o.bias_field_fwhm
    };
    let noise = if o.wiener_noise == 0.0 {
        0.01
    } else {
        o.wiener_noise
    };
    if shrink < 1 {
        return Err(FilterError::invalid(format!("N4 shrink factor {shrink}")));
    }
    if cp < ORDER as i64 + 1 {
        return Err(FilterError::invalid(format!(
            "N4 needs at least {} control points, not {cp}",
            ORDER + 1
        )));
    }
    if bins < 2 {
        return Err(FilterError::invalid(format!("N4 histogram of {bins} bins")));
    }
    if !(threshold > 0.0) || !(fwhm > 0.0) || !(noise > 0.0) {
        return Err(FilterError::invalid(
            "N4 convergence threshold, FWHM and Wiener noise must be positive",
        ));
    }
    if o.workers < 0 {
        return Err(FilterError::invalid(format!("N4 workers {}", o.workers)));
    }
    if let Some(m) = &o.mask
        && m.len() != n
    {
        return Err(FilterError::invalid(format!(
            "N4 mask of {} voxels for {n}",
            m.len()
        )));
    }
    for it in &iterations {
        if *it < 1 {
            return Err(FilterError::invalid(format!("N4 level of {it} iterations")));
        }
    }
    Ok(Normalized {
        shrink: shrink as usize,
        iterations,
        threshold,
        control_points: cp as usize,
        bins: bins as usize,
        fwhm,
        noise,
    })
}
pub fn n4_bias_field_correction(
    ctx: Ctx<'_>,
    input: Option<&Volume>,
    opts: N4Options<'_>,
) -> Result<N4Result, FilterError> {
    let mut opts = opts;
    let input = validate(ctx, input)?;
    let o = normalized(&opts, input.data.len())?;
    let grid = shrink(ctx, input, opts.mask.as_deref(), o.shrink)?;
    if grid.samples.is_empty() {
        return Err(FilterError::invalid("no positive voxel in the N4 mask"));
    }
    let mut result_levels = Vec::new();
    let log_input: Vec<f64> = grid.values.iter().map(|v| fmath::log(*v)).collect();
    ctx_err(ctx)?;
    let mut log_uncorrected = log_input.clone();
    let n = grid.samples.len();
    let mut log_bias = vec![0f64; n];
    let mut residual = vec![0f64; n];
    let mut sharpened = vec![0f64; n];
    let axes = grid.axes();
    let mut lattice: Option<Lattice> = None;
    let levels = o.iterations.len();
    for (level, max_iterations) in o.iterations.iter().copied().enumerate() {
        let mut convergence = f64::MAX;
        let mut iterations = 0;
        while iterations < max_iterations && convergence > o.threshold {
            ctx_err(ctx)?;
            iterations += 1;
            sharpen(&log_uncorrected, &mut sharpened, o.bins, o.fwhm, o.noise);
            for k in 0..n {
                residual[k] = log_uncorrected[k] - sharpened[k];
            }
            let size = match &lattice {
                Some(l) => l.n,
                None => [o.control_points; 3],
            };
            let phi = fit_bspline(size, &axes, &grid.samples, &residual);
            match &mut lattice {
                None => lattice = Some(phi),
                Some(l) => {
                    for (a, b) in l.phi.iter_mut().zip(&phi.phi) {
                        *a += b;
                    }
                }
            }
            let Some(l) = &lattice else {
                break;
            };
            let new_bias = l.evaluate_at(&axes, &grid.samples);
            convergence = convergence_of(&log_bias, &new_bias);
            log_bias = new_bias;
            for k in 0..n {
                log_uncorrected[k] = log_input[k] - log_bias[k];
            }
            ctx_err(ctx)?;
            if let Some(p) = opts.progress.as_mut() {
                p(N4Progress {
                    level: level as i64,
                    levels: levels as i64,
                    iteration: iterations,
                    convergence,
                });
            }
        }
        result_levels.push(N4Level {
            iterations,
            convergence,
        });
        if level < levels - 1 {
            ctx_err(ctx)?;
            if let Some(l) = lattice.take() {
                lattice = Some(l.refine(grid.dims));
            }
        }
    }
    ctx_err(ctx)?;
    let mut log_field = vec![0f32; input.data.len()];
    let mut corrected = vec![0f32; input.data.len()];
    let axes = grid.full_axes(input.dims);
    if let Some(l) = &lattice {
        l.evaluate_to(&axes, &mut |i, log_b| {
            log_field[i] = log_b as f32;
            corrected[i] = (f64::from(input.data[i]) / fmath::exp(log_b)) as f32;
        });
    }
    ctx_err(ctx)?;
    Ok(N4Result {
        corrected: Volume {
            dims: input.dims,
            spacing_mm: input.spacing_mm,
            data: corrected,
        },
        log_bias_field: Volume {
            dims: input.dims,
            spacing_mm: input.spacing_mm,
            data: log_field,
        },
        levels: result_levels,
        shrunken_dims: grid.dims.map(|d| d as i64),
        samples: n as i64,
    })
}
struct Grid {
    dims: [usize; 3],
    centres: [Vec<f64>; 3],
    samples: Vec<usize>,
    values: Vec<f64>,
}
fn shrink(
    ctx: Ctx<'_>,
    input: &Volume,
    mask: Option<&[bool]>,
    factor: usize,
) -> Result<Grid, FilterError> {
    let d = input.dims.map(|v| v as usize);
    let mut g = Grid {
        dims: [0; 3],
        centres: Default::default(),
        samples: Vec::new(),
        values: Vec::new(),
    };
    for axis in 0..3 {
        let n = d[axis];
        g.dims[axis] = n.div_ceil(factor);
        g.centres[axis] = (0..g.dims[axis])
            .map(|i| {
                let length = factor.min(n - i * factor);
                (i * factor) as f64 + (length as f64 - 1.0) / 2.0
            })
            .collect();
    }
    let total = g.dims[0] * g.dims[1] * g.dims[2];
    let mut sums = vec![0f64; total];
    let mut counts = vec![0usize; total];
    for z in 0..d[2] {
        ctx_err(ctx)?;
        for y in 0..d[1] {
            let row = (z * d[1] + y) * d[0];
            let block = ((z / factor) * g.dims[1] + y / factor) * g.dims[0];
            for x in 0..d[0] {
                let i = row + x;
                let value = input.data[i];
                if value > 0.0 && mask.is_none_or(|m| m[i]) {
                    sums[block + x / factor] += f64::from(value);
                    counts[block + x / factor] += 1;
                }
            }
        }
    }
    for (i, count) in counts.iter().copied().enumerate() {
        if count > 0 {
            g.samples.push(i);
            g.values.push(sums[i] / count as f64);
        }
    }
    ctx_err(ctx)?;
    Ok(g)
}
#[derive(Clone, Default)]
struct AxisSpline {
    positions: Vec<f64>,
    step: f64,
}
impl Grid {
    fn axes(&self) -> [AxisSpline; 3] {
        let mut axes: [AxisSpline; 3] = Default::default();
        for (axis, a) in axes.iter_mut().enumerate() {
            let n = self.dims[axis];
            a.positions = vec![0.0; n];
            if n > 1 {
                a.step = 1.0 / (n - 1) as f64;
                for (i, p) in a.positions.iter_mut().enumerate() {
                    *p = i as f64 / (n - 1) as f64;
                }
            }
        }
        axes
    }
    fn full_axes(&self, dims: [i64; 3]) -> [AxisSpline; 3] {
        let mut axes: [AxisSpline; 3] = Default::default();
        for (axis, a) in axes.iter_mut().enumerate() {
            let centres = &self.centres[axis];
            a.positions = vec![0.0; dims[axis] as usize];
            if centres.len() < 2 {
                continue;
            }
            let (lo, hi) = (centres[0], centres[centres.len() - 1]);
            a.step = 1.0 / (centres.len() - 1) as f64;
            for (i, p) in a.positions.iter_mut().enumerate() {
                *p = fmath::min(1.0, fmath::max(0.0, (i as f64 - lo) / (hi - lo)));
            }
        }
        axes
    }
}
struct Lattice {
    n: [usize; 3],
    phi: Vec<f64>,
}
fn cubic_span(s: f64, step: f64, n: usize) -> (usize, [f64; 4]) {
    let spans = (n - ORDER) as f64;
    let mut p = s * spans;
    let epsilon = spans * step * 1e-3;
    if (p - spans).abs() <= epsilon {
        p = spans - epsilon;
    }
    let below = f64::from_bits(spans.to_bits() - 1);
    p = fmath::max(0.0, fmath::min(p, below));
    let span = p as usize;
    let t = p - span as f64;
    let u = 1.0 - t;
    (
        span,
        [
            u * u * u / 6.0,
            (3.0 * t * t * t - 6.0 * t * t + 4.0) / 6.0,
            (-3.0 * t * t * t + 3.0 * t * t + 3.0 * t + 1.0) / 6.0,
            t * t * t / 6.0,
        ],
    )
}
struct Weights {
    span: Vec<usize>,
    weights: Vec<[f64; 4]>,
}
fn weights_for(axis: &AxisSpline, n: usize) -> Weights {
    let (span, weights) = axis
        .positions
        .iter()
        .map(|s| cubic_span(*s, axis.step, n))
        .unzip();
    Weights { span, weights }
}
const FIT_BLOCK: usize = 4096;
const MAX_FIT_BLOCKS: usize = 16;
fn fit_bspline(
    size: [usize; 3],
    axes: &[AxisSpline; 3],
    samples: &[usize],
    values: &[f64],
) -> Lattice {
    let w = [
        weights_for(&axes[0], size[0]),
        weights_for(&axes[1], size[1]),
        weights_for(&axes[2], size[2]),
    ];
    let (nx, ny) = (axes[0].positions.len(), axes[1].positions.len());
    let total = size[0] * size[1] * size[2];
    let blocks = MAX_FIT_BLOCKS.min(1.max(samples.len() / FIT_BLOCK));
    let mut deltas = Vec::with_capacity(blocks);
    let mut omegas = Vec::with_capacity(blocks);
    for block in 0..blocks {
        let (mut delta, mut omega) = (vec![0f64; total], vec![0f64; total]);
        for k in block * samples.len() / blocks..(block + 1) * samples.len() / blocks {
            let i = samples[k];
            let (x, y, z) = (i % nx, (i / nx) % ny, i / (nx * ny));
            let (bx, by, bz) = (w[0].weights[x], w[1].weights[y], w[2].weights[z]);
            let (mut sx, mut sy, mut sz) = (0.0, 0.0, 0.0);
            for j in 0..4 {
                sx += bx[j] * bx[j];
                sy += by[j] * by[j];
                sz += bz[j] * bz[j];
            }
            let scale = values[k] / (sx * sy * sz);
            for c in 0..4 {
                let zi = w[2].span[z] + c;
                if zi >= size[2] {
                    continue;
                }
                for b in 0..4 {
                    let yi = w[1].span[y] + b;
                    if yi >= size[1] {
                        continue;
                    }
                    let bzy = bz[c] * by[b];
                    let row = (zi * size[1] + yi) * size[0];
                    for a in 0..4 {
                        let xi = w[0].span[x] + a;
                        if xi >= size[0] {
                            continue;
                        }
                        let bb = bzy * bx[a];
                        let b2 = bb * bb;
                        omega[row + xi] += b2;
                        delta[row + xi] += scale * b2 * bb;
                    }
                }
            }
        }
        deltas.push(delta);
        omegas.push(omega);
    }
    let mut lattice = Lattice {
        n: size,
        phi: vec![0.0; total],
    };
    for i in 0..total {
        let (mut delta, mut omega) = (0.0, 0.0);
        for block in 0..blocks {
            delta += deltas[block][i];
            omega += omegas[block][i];
        }
        if omega.abs() > 0.1 * 2.220446049250313e-16 {
            let phi = delta / omega;
            if phi.is_finite() {
                lattice.phi[i] = phi;
            }
        }
    }
    lattice
}
impl Lattice {
    fn evaluate_to(&self, axes: &[AxisSpline; 3], emit: &mut dyn FnMut(usize, f64)) {
        let w = [
            weights_for(&axes[0], self.n[0]),
            weights_for(&axes[1], self.n[1]),
            weights_for(&axes[2], self.n[2]),
        ];
        let (nx, ny, nz) = (
            axes[0].positions.len(),
            axes[1].positions.len(),
            axes[2].positions.len(),
        );
        let mut plane = vec![0f64; self.n[0] * self.n[1]];
        let mut line = vec![0f64; self.n[0]];
        for z in 0..nz {
            plane.iter_mut().for_each(|v| *v = 0.0);
            for c in 0..4 {
                let zi = w[2].span[z] + c;
                if zi >= self.n[2] {
                    continue;
                }
                let weight = w[2].weights[z][c];
                let base = zi * self.n[0] * self.n[1];
                for (i, p) in plane.iter_mut().enumerate() {
                    *p += weight * self.phi[base + i];
                }
            }
            for y in 0..ny {
                line.iter_mut().for_each(|v| *v = 0.0);
                for b in 0..4 {
                    let yi = w[1].span[y] + b;
                    if yi >= self.n[1] {
                        continue;
                    }
                    let weight = w[1].weights[y][b];
                    for (i, l) in line.iter_mut().enumerate() {
                        *l += weight * plane[yi * self.n[0] + i];
                    }
                }
                let row = (z * ny + y) * nx;
                for x in 0..nx {
                    let mut v = 0.0;
                    for a in 0..4 {
                        let xi = w[0].span[x] + a;
                        if xi < self.n[0] {
                            v += w[0].weights[x][a] * line[xi];
                        }
                    }
                    emit(row + x, v);
                }
            }
        }
    }
    fn evaluate_at(&self, axes: &[AxisSpline; 3], samples: &[usize]) -> Vec<f64> {
        let (nx, ny, nz) = (
            axes[0].positions.len(),
            axes[1].positions.len(),
            axes[2].positions.len(),
        );
        let mut out = vec![0f64; samples.len()];
        let sorted = samples.windows(2).all(|p| p[1] > p[0]);
        if sorted && samples.len() > nx * ny * nz / 16 {
            let mut k = 0;
            self.evaluate_to(axes, &mut |i, v| {
                if k < samples.len() && samples[k] == i {
                    out[k] = v;
                    k += 1;
                }
            });
            return out;
        }
        let w = [
            weights_for(&axes[0], self.n[0]),
            weights_for(&axes[1], self.n[1]),
            weights_for(&axes[2], self.n[2]),
        ];
        for (k, o) in out.iter_mut().enumerate() {
            let i = samples[k];
            let (x, y, z) = (i % nx, (i / nx) % ny, i / (nx * ny));
            let mut plane = [[0f64; 4]; 4];
            for c in 0..4 {
                let zi = w[2].span[z] + c;
                if zi >= self.n[2] {
                    continue;
                }
                for b in 0..4 {
                    let yi = w[1].span[y] + b;
                    if yi >= self.n[1] {
                        continue;
                    }
                    for a in 0..4 {
                        let xi = w[0].span[x] + a;
                        if xi < self.n[0] {
                            plane[b][a] += w[2].weights[z][c]
                                * self.phi[(zi * self.n[1] + yi) * self.n[0] + xi];
                        }
                    }
                }
            }
            let mut line = [0f64; 4];
            for b in 0..4 {
                for a in 0..4 {
                    line[a] += w[1].weights[y][b] * plane[b][a];
                }
            }
            for a in 0..4 {
                *o += w[0].weights[x][a] * line[a];
            }
        }
        out
    }
    fn refine(self, samples: [usize; 3]) -> Lattice {
        let mut out = self;
        for axis in 0..3 {
            if 2 * out.n[axis] - ORDER <= (ORDER + 1).max(samples[axis] / 2) {
                out = out.refine_axis(axis);
            }
        }
        out
    }
    fn refine_axis(&self, axis: usize) -> Lattice {
        let n = self.n[axis];
        let mut size = self.n;
        size[axis] = 2 * n - ORDER;
        let mut out = Lattice {
            n: size,
            phi: vec![0.0; size[0] * size[1] * size[2]],
        };
        let stride = [1, self.n[0], self.n[0] * self.n[1]];
        let out_stride = [1, size[0], size[0] * size[1]];
        for z in 0..size[2] {
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let mut index = [x, y, z];
                    let r = index[axis];
                    index[axis] = r / 2;
                    let source = index[0] * stride[0] + index[1] * stride[1] + index[2] * stride[2];
                    let at = |offset: usize| self.phi[source + offset * stride[axis]];
                    let v = if r % 2 == 0 {
                        (at(0) + at(1)) / 2.0
                    } else {
                        (at(0) + 6.0 * at(1) + at(2)) / 8.0
                    };
                    out.phi[x * out_stride[0] + y * out_stride[1] + z * out_stride[2]] = v;
                }
            }
        }
        out
    }
}
fn to_fft(v: &[Complex128]) -> Vec<fft::Complex> {
    v.iter().map(|c| fft::Complex::new(c.re, c.im)).collect()
}
fn from_fft(v: &[fft::Complex]) -> Vec<Complex128> {
    v.iter().map(|c| Complex128::new(c.re, c.im)).collect()
}
fn transform(v: &mut Vec<Complex128>, inverse: bool) {
    let mut f = to_fft(v);
    fft::transform(&mut f, inverse);
    *v = from_fft(&f);
}
const LN2_OVER_PI: f64 = 0.220_635_600_152_651_6;
fn sharpen(values: &[f64], out: &mut [f64], bins: usize, fwhm: f64, noise: f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        lo = fmath::min(lo, *v);
        hi = fmath::max(hi, *v);
    }
    if hi <= lo {
        out.copy_from_slice(values);
        return;
    }
    let slope = (hi - lo) / (bins - 1) as f64;
    let mut histogram = vec![0f64; bins];
    for v in values {
        let c = (v - lo) / slope;
        let i = c.floor() as i64;
        let offset = c - i as f64;
        if offset == 0.0 {
            histogram[i as usize] += 1.0;
        } else if i < bins as i64 - 1 {
            histogram[i as usize] += 1.0 - offset;
            histogram[i as usize + 1] += offset;
        }
    }
    let padded = fmath::f64_to_i64(
        fmath::pow(
            2.0,
            (fmath::log(bins as f64) / fmath::log(2.0)).ceil() + 1.0,
        ) + 0.5,
    ) as usize;
    let padding = fmath::f64_to_i64(0.5 * (padded - bins) as f64) as usize;
    let mut vv = vec![Complex128::default(); padded];
    for (n, h) in histogram.iter().enumerate() {
        vv[n + padding] = Complex128::new(*h, 0.0);
    }
    transform(&mut vv, false);
    let scaled = fwhm / slope;
    let exp_factor = 4.0 * std::f64::consts::LN_2 / (scaled * scaled);
    let scale_factor = 2.0 * LN2_OVER_PI.sqrt() / scaled;
    let mut ff = vec![Complex128::default(); padded];
    ff[0] = Complex128::new(scale_factor, 0.0);
    let half = padded / 2;
    for n in 1..=half {
        let value = Complex128::new(
            scale_factor * fmath::exp(-((n * n) as f64) * exp_factor),
            0.0,
        );
        ff[n] = value;
        ff[padded - n] = value;
    }
    if padded.is_multiple_of(2) {
        ff[half] = Complex128::new(
            scale_factor * fmath::exp(0.25 * -((padded * padded) as f64) * exp_factor),
            0.0,
        );
    }
    transform(&mut ff, false);
    let mut u = vec![Complex128::default(); padded];
    for n in 0..padded {
        let c = Complex128::new(ff[n].re, -ff[n].im);
        let g = c.div(c.mul(ff[n]).add(Complex128::new(noise, 0.0)));
        u[n] = vv[n].mul(Complex128::new(g.re, 0.0));
    }
    transform(&mut u, true);
    let scale = 1.0 / padded as f64;
    for x in u.iter_mut() {
        *x = Complex128::new(fmath::max(x.re * scale, 0.0), 0.0);
    }
    let mut numerator = vec![Complex128::default(); padded];
    let mut denominator = vec![Complex128::default(); padded];
    for n in 0..padded {
        numerator[n] = Complex128::new((lo + (n as f64 - padding as f64) * slope) * u[n].re, 0.0);
        denominator[n] = u[n];
    }
    for series in [&mut numerator, &mut denominator] {
        transform(series, false);
        for (n, s) in series.iter_mut().enumerate() {
            *s = s.mul(ff[n]);
        }
        transform(series, true);
    }
    let mut e = vec![0f64; bins];
    for (n, en) in e.iter_mut().enumerate() {
        let d = denominator[n + padding].re;
        if d != 0.0 {
            *en = numerator[n + padding].re / d;
        }
    }
    for (k, v) in values.iter().enumerate() {
        let c = (v - lo) / slope;
        let i = c.floor() as i64;
        if i < bins as i64 - 1 {
            let i = i as usize;
            out[k] = e[i] + (e[i + 1] - e[i]) * (c - i as f64);
        } else {
            out[k] = e[bins - 1];
        }
    }
}
fn convergence_of(before: &[f64], after: &[f64]) -> f64 {
    let (mut mu, mut sigma, mut n) = (0.0, 0.0, 0.0);
    for k in 0..before.len() {
        let pixel = fmath::exp(before[k] - after[k]);
        n += 1.0;
        if n > 1.0 {
            sigma += (pixel - mu) * (pixel - mu) * (n - 1.0) / n;
        }
        mu = mu * (1.0 - 1.0 / n) + pixel / n;
    }
    (sigma / (n - 1.0)).sqrt() / mu
}
fn validate<'a>(ctx: Ctx<'_>, input: Option<&'a Volume>) -> Result<&'a Volume, FilterError> {
    ctx_err(ctx)?;
    let Some(input) = input else {
        return Err(FilterError::invalid("nil volume"));
    };
    match voxel_count(input.dims) {
        Some(n) if input.data.len() as i64 == n => {}
        _ => {
            return Err(FilterError::invalid(format!(
                "{} samples for dimensions {}",
                input.data.len(),
                dims_v(input.dims)
            )));
        }
    }
    for (axis, s) in input.spacing_mm.iter().enumerate() {
        if input.dims[axis] > 1 && !(*s > 0.0 && *s <= f64::MAX) {
            return Err(FilterError::invalid(format!(
                "spacing {} mm along axis {axis}",
                float_v(*s)
            )));
        }
    }
    if let Some(i) = input.data.iter().position(|x| !x.is_finite()) {
        return Err(FilterError::invalid(format!(
            "non-finite sample at index {i}"
        )));
    }
    ctx_err(ctx)?;
    Ok(input)
}
