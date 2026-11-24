use super::recursive_gaussian::{
    LANE_WIDTH, LaneWindow, RecursiveGaussianOptions, RecursiveKernel, smooth_recursive_gaussian,
};
use super::slab::{DEFAULT_SLAB_WORKING_BYTES, SlabFilter, SlabKernel, SlabView, worker_bound};
use super::volume::{Ctx, FilterError, Volume, ctx_err};
use crate::fmath;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnisotropicDiffusionOptions {
    pub iterations: i64,
    pub time_step: f64,
    pub conductance: f64,
    pub workers: i64,
    pub max_working_bytes: i64,
}
pub fn gradient_anisotropic_diffusion(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: AnisotropicDiffusionOptions,
) -> Result<Volume, FilterError> {
    anisotropic_diffusion(ctx, src, opts, diffusion_line, 1.0)
}
pub fn curvature_anisotropic_diffusion(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: AnisotropicDiffusionOptions,
) -> Result<Volume, FilterError> {
    anisotropic_diffusion(ctx, src, opts, curvature_diffusion_line, 0.5)
}
type DiffusionLine = fn(&Volume, &mut [f32], usize, usize, &[usize], [f64; 3], f64, f64);
fn anisotropic_diffusion(
    ctx: Ctx<'_>,
    src: &Volume,
    mut opts: AnisotropicDiffusionOptions,
    update: DiffusionLine,
    stability: f64,
) -> Result<Volume, FilterError> {
    src.validate()?;
    if opts.iterations == 0 {
        opts.iterations = 5;
    }
    if opts.conductance == 0.0 {
        opts.conductance = 1.0;
    }
    if opts.iterations < 0
        || opts.iterations > 1000
        || !(opts.conductance > 0.0)
        || opts.conductance.is_infinite()
    {
        return Err(FilterError::invalid(format!(
            "iterations {}, conductance {}",
            opts.iterations,
            float_v(opts.conductance)
        )));
    }
    let mut axes = Vec::new();
    let mut inv_h2 = [0f64; 3];
    let mut limit = 0.0;
    for axis in 0..3 {
        if src.dims[axis] > 1 {
            axes.push(axis);
            inv_h2[axis] = 1.0 / (src.spacing_mm[axis] * src.spacing_mm[axis]);
            limit += inv_h2[axis];
        }
    }
    let mut out = src.clone();
    if axes.is_empty() {
        return Ok(out);
    }
    let stable = stability / (2.0 * limit);
    if opts.time_step == 0.0 {
        opts.time_step = 0.9 * stable;
    }
    if !(opts.time_step > 0.0) || opts.time_step > stable * (1.0 + 1e-9) {
        return Err(FilterError::invalid(format!(
            "time step {} mm² exceeds the stable {} mm²",
            float_v(opts.time_step),
            float_v(stable)
        )));
    }
    if opts.max_working_bytes < 0 {
        return Err(FilterError::invalid(format!(
            "working bound {} bytes",
            opts.max_working_bytes
        )));
    }
    if opts.max_working_bytes == 0 {
        opts.max_working_bytes = DEFAULT_SLAB_WORKING_BYTES;
    }
    let (ny, nz) = (src.dims[1] as usize, src.dims[2] as usize);
    let nxy = src.dims[0] as usize * ny;
    let mut slab = nz;
    if out.data.len() as i64 * 4 > opts.max_working_bytes {
        slab = 1i64.max((nz as i64).min(opts.max_working_bytes / (2 * nxy as i64 * 4))) as usize;
    }
    let mut bufs: [Vec<f32>; 2] = [vec![0f32; slab * nxy], Vec::new()];
    if slab < nz {
        bufs[1] = vec![0f32; slab * nxy];
    }
    for _ in 0..opts.iterations {
        let mut mean_square = average_gradient_squared(ctx, &out, &axes, false)?;
        if mean_square == 0.0 {
            mean_square = average_gradient_squared(ctx, &out, &axes, true)?;
            if mean_square == 0.0 {
                break;
            }
        }
        let k = 2.0 * opts.conductance * opts.conductance * mean_square;
        if slab == nz {
            let mut next = std::mem::take(&mut bufs[0]);
            for line in 0..ny * nz {
                ctx_err(ctx)?;
                update(&out, &mut next, 0, line, &axes, inv_h2, k, opts.time_step);
            }
            ctx_err(ctx)?;
            bufs[0] = std::mem::replace(&mut out.data, next);
            continue;
        }
        let mut pending: Option<usize> = None;
        let (mut s, mut z0) = (0usize, 0usize);
        while z0 < nz {
            let z1 = nz.min(z0 + slab);
            let mut next = std::mem::take(&mut bufs[s % 2]);
            for line in 0..(z1 - z0) * ny {
                if let Err(e) = ctx_err(ctx) {
                    bufs[s % 2] = next;
                    return Err(e);
                }
                update(
                    &out,
                    &mut next,
                    z0 * nxy,
                    z0 * ny + line,
                    &axes,
                    inv_h2,
                    k,
                    opts.time_step,
                );
            }
            bufs[s % 2] = next;
            ctx_err(ctx)?;
            if let Some(p) = pending {
                let prev = &bufs[(s + 1) % 2];
                out.data[p * nxy..z0 * nxy].copy_from_slice(&prev[..(z0 - p) * nxy]);
            }
            pending = Some(z0);
            s += 1;
            z0 += slab;
        }
        let p = pending.unwrap_or(0);
        let last = &bufs[((nz - 1) / slab) % 2];
        out.data[p * nxy..].copy_from_slice(&last[..(nz - p) * nxy]);
    }
    Ok(out)
}
fn average_gradient_squared(
    ctx: Ctx<'_>,
    v: &Volume,
    axes: &[usize],
    boundary_only: bool,
) -> Result<f64, FilterError> {
    let (nx, ny, nz) = (v.dims[0] as usize, v.dims[1] as usize, v.dims[2] as usize);
    let strides = [1, nx, nx * ny];
    let dims = [nx, ny, nz];
    let (mut sum, mut count) = (0.0, 0usize);
    for z in 0..nz {
        for y in 0..ny {
            ctx_err(ctx)?;
            for x in 0..nx {
                let c = [x, y, z];
                let mut interior = true;
                for &axis in axes {
                    if dims[axis] > 2 && (c[axis] == 0 || c[axis] == dims[axis] - 1) {
                        interior = false;
                    }
                }
                if interior == boundary_only {
                    continue;
                }
                let i = (z * ny + y) * nx + x;
                let mut g = 0.0;
                for &axis in axes {
                    let low = i - c[axis].min(1) * strides[axis];
                    let high = i + (dims[axis] - 1 - c[axis]).min(1) * strides[axis];
                    let d = (f64::from(v.data[high]) - f64::from(v.data[low]))
                        / (2.0 * v.spacing_mm[axis]);
                    g += d * d;
                }
                sum += g;
                count += 1;
            }
        }
    }
    if boundary_only {
        count = v.data.len();
    }
    if count == 0 {
        ctx_err(ctx)?;
        return Ok(0.0);
    }
    let r = sum / count as f64;
    ctx_err(ctx)?;
    Ok(r)
}
fn at_clamped(v: &Volume, c: [i64; 3]) -> f64 {
    let mut c = c;
    for (axis, ca) in c.iter_mut().enumerate() {
        if *ca < 0 {
            *ca = 0;
        } else if *ca >= v.dims[axis] {
            *ca = v.dims[axis] - 1;
        }
    }
    f64::from(v.data[((c[2] * v.dims[1] + c[1]) * v.dims[0] + c[0]) as usize])
}
#[allow(clippy::too_many_arguments)]
fn diffusion_line(
    v: &Volume,
    next: &mut [f32],
    offset: usize,
    line: usize,
    axes: &[usize],
    inv_h2: [f64; 3],
    k: f64,
    dt: f64,
) {
    let (nx, ny) = (v.dims[0] as usize, v.dims[1] as usize);
    let (y, z) = (line % ny, line / ny);
    let strides = [1i64, nx as i64, (nx * ny) as i64];
    let data = &v.data;
    for x in 0..nx {
        let c = [x as i64, y as i64, z as i64];
        let i = (z * ny + y) * nx + x;
        let centre = f64::from(data[i]);
        let mut delta = 0.0;
        for &axis in axes {
            let h = v.spacing_mm[axis];
            for side in [1i64, -1] {
                let na = c[axis] + side;
                if na < 0 || na >= v.dims[axis] {
                    continue;
                }
                let neighbour = f64::from(data[(i as i64 + side * strides[axis]) as usize]);
                let normal = (neighbour - centre) / h;
                let mut g2 = normal * normal;
                for &other in axes {
                    if other == axis {
                        continue;
                    }
                    let ho = v.spacing_mm[other];
                    let (mut p, mut m) = (c, c);
                    p[other] += 1;
                    m[other] -= 1;
                    let here = (at_clamped(v, p) - at_clamped(v, m)) / (2.0 * ho);
                    p[axis] += side;
                    m[axis] += side;
                    let there = (at_clamped(v, p) - at_clamped(v, m)) / (2.0 * ho);
                    let t = (here + there) / 2.0;
                    g2 += t * t;
                }
                delta += fmath::exp(-g2 / k) * (neighbour - centre) * inv_h2[axis];
            }
        }
        next[i - offset] = (centre + dt * delta) as f32;
    }
}
fn sqr(v: f64) -> f64 {
    v * v
}
#[allow(clippy::too_many_arguments)]
fn curvature_diffusion_line(
    v: &Volume,
    next: &mut [f32],
    offset: usize,
    line: usize,
    axes: &[usize],
    _inv_h2: [f64; 3],
    k: f64,
    dt: f64,
) {
    const MIN_NORM: f64 = 1e-10;
    let (nx, ny) = (v.dims[0] as usize, v.dims[1] as usize);
    let (y, z) = (line % ny, line / ny);
    let data = &v.data;
    let centred = |c: [i64; 3], axis: usize| -> f64 {
        let (mut p, mut m) = (c, c);
        p[axis] += 1;
        m[axis] -= 1;
        (at_clamped(v, p) - at_clamped(v, m)) / (2.0 * v.spacing_mm[axis])
    };
    for x in 0..nx {
        let c = [x as i64, y as i64, z as i64];
        let i = (z * ny + y) * nx + x;
        let centre = f64::from(data[i]);
        let (mut forward, mut backward, mut mid) = ([0f64; 3], [0f64; 3], [0f64; 3]);
        for &axis in axes {
            let (mut p, mut m) = (c, c);
            p[axis] += 1;
            m[axis] -= 1;
            forward[axis] = (at_clamped(v, p) - centre) / v.spacing_mm[axis];
            backward[axis] = (centre - at_clamped(v, m)) / v.spacing_mm[axis];
            mid[axis] = centred(c, axis);
        }
        let mut speed = 0.0;
        for &axis in axes {
            let mut g2f = forward[axis] * forward[axis];
            let mut g2b = backward[axis] * backward[axis];
            for &other in axes {
                if other == axis {
                    continue;
                }
                let (mut p, mut m) = (c, c);
                p[axis] += 1;
                m[axis] -= 1;
                let aug = mid[other] + centred(p, other);
                let dim = mid[other] + centred(m, other);
                g2f += 0.25 * aug * aug;
                g2b += 0.25 * dim * dim;
            }
            let nf = forward[axis] / (MIN_NORM + g2f).sqrt() * fmath::exp(-g2f / k);
            let nb = backward[axis] / (MIN_NORM + g2b).sqrt() * fmath::exp(-g2b / k);
            speed += (nf - nb) / v.spacing_mm[axis];
        }
        let mut propagation = 0.0;
        for &axis in axes {
            if speed > 0.0 {
                propagation +=
                    sqr(fmath::min(backward[axis], 0.0)) + sqr(fmath::max(forward[axis], 0.0));
            } else {
                propagation +=
                    sqr(fmath::max(backward[axis], 0.0)) + sqr(fmath::min(forward[axis], 0.0));
            }
        }
        next[i - offset] = (centre + dt * propagation.sqrt() * speed) as f32;
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BilateralOptions {
    pub sigma_mm: [f64; 3],
    pub range_sigma: f64,
    pub max_kernel_samples: i64,
    pub workers: i64,
}
pub fn bilateral(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: &BilateralOptions,
) -> Result<Volume, FilterError> {
    src.validate()?;
    let mut k = BilateralKernel::new(src.dims, src.spacing_mm, opts)?;
    let mut out = vec![0f32; src.data.len()];
    k.run(
        ctx,
        &SlabView::whole(src),
        0,
        src.dims[2] as usize,
        &mut out,
    )?;
    Ok(Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data: out,
    })
}
pub(super) struct BilateralTap {
    pub(super) dx: i64,
    pub(super) dy: i64,
    pub(super) dz: i64,
    pub(super) w: f64,
}
pub(super) struct BilateralKernel {
    pub(super) dims: [usize; 3],
    pub(super) radius: [i64; 3],
    pub(super) taps: Vec<BilateralTap>,
    pub(super) range_scale: f64,
}
impl BilateralKernel {
    pub(super) fn new(
        dims: [i64; 3],
        spacing_mm: [f64; 3],
        opts: &BilateralOptions,
    ) -> Result<BilateralKernel, FilterError> {
        if !(opts.range_sigma > 0.0) || opts.range_sigma.is_infinite() {
            return Err(FilterError::invalid(format!(
                "range sigma {}",
                float_v(opts.range_sigma)
            )));
        }
        let max_samples = if opts.max_kernel_samples == 0 {
            1 << 16
        } else {
            opts.max_kernel_samples
        };
        let mut k = BilateralKernel {
            dims: [dims[0] as usize, dims[1] as usize, dims[2] as usize],
            radius: [0; 3],
            taps: Vec::new(),
            range_scale: -1.0 / (2.0 * opts.range_sigma * opts.range_sigma),
        };
        let mut samples: i64 = 1;
        for axis in 0..3 {
            let s = opts.sigma_mm[axis];
            if s < 0.0 || s.is_nan() || s.is_infinite() {
                return Err(FilterError::invalid(format!("sigma {} mm", float_v(s))));
            }
            if s > 0.0 && dims[axis] > 1 {
                k.radius[axis] = fmath::f64_to_i64((2.5 * s / spacing_mm[axis]).ceil());
            }
            samples = samples.wrapping_mul(k.radius[axis].wrapping_mul(2).wrapping_add(1));
            if samples > max_samples {
                return Err(FilterError::invalid(format!(
                    "the bilateral kernel exceeds {max_samples} samples"
                )));
            }
        }
        let r = k.radius;
        let mut dz = r[2].wrapping_neg();
        while dz <= r[2] {
            let mut dy = r[1].wrapping_neg();
            while dy <= r[1] {
                let mut dx = r[0].wrapping_neg();
                while dx <= r[0] {
                    let mut d2 = 0.0;
                    for (axis, d) in [dx, dy, dz].into_iter().enumerate() {
                        if d != 0 {
                            let rr = d as f64 * spacing_mm[axis] / opts.sigma_mm[axis];
                            d2 += rr * rr;
                        }
                    }
                    if d2 <= 2.5 * 2.5 {
                        k.taps.push(BilateralTap {
                            dx,
                            dy,
                            dz,
                            w: fmath::exp(-d2 / 2.0),
                        });
                    }
                    if dx == i64::MAX {
                        break;
                    }
                    dx += 1;
                }
                if dy == i64::MAX {
                    break;
                }
                dy += 1;
            }
            if dz == i64::MAX {
                break;
            }
            dz += 1;
        }
        Ok(k)
    }
}
impl SlabFilter for BilateralOptions {
    fn slab_kernel(
        &self,
        dims: [i64; 3],
        spacing_mm: [f64; 3],
    ) -> Result<Box<dyn SlabKernel>, FilterError> {
        Ok(Box::new(BilateralKernel::new(dims, spacing_mm, self)?))
    }
}
impl SlabKernel for BilateralKernel {
    fn halo(&self) -> i64 {
        self.radius[2]
    }
    fn scratch_bytes(&self, _slices: i64) -> i64 {
        0
    }
    fn run(
        &mut self,
        ctx: Ctx<'_>,
        input: &SlabView<'_>,
        z0: usize,
        z1: usize,
        dst: &mut [f32],
    ) -> Result<(), FilterError> {
        let [nx, ny, nz] = self.dims.map(|d| d as i64);
        let lines = (z1 - z0) * ny as usize;
        for line in 0..lines {
            ctx_err(ctx)?;
            let (y, z) = (
                (line % ny as usize) as i64,
                (z0 + line / ny as usize) as i64,
            );
            let centre_row = input.row(y as usize, z as usize);
            for x in 0..nx {
                let centre = f64::from(input.data[centre_row.wrapping_add(x as usize)]);
                let (mut sum, mut weights) = (0.0, 0.0);
                for t in &self.taps {
                    let (sx, sy, sz) = (
                        x.wrapping_add(t.dx),
                        y.wrapping_add(t.dy),
                        z.wrapping_add(t.dz),
                    );
                    if sx < 0 || sy < 0 || sz < 0 || sx >= nx || sy >= ny || sz >= nz {
                        continue;
                    }
                    let v = f64::from(input.at(sx as usize, sy as usize, sz as usize));
                    let d = v - centre;
                    let w = t.w * fmath::exp(d * d * self.range_scale);
                    sum += w * v;
                    weights += w;
                }
                dst[line * nx as usize + x as usize] = (sum / weights) as f32;
            }
        }
        ctx_err(ctx)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UnsharpMaskOptions {
    pub sigma_mm: [f64; 3],
    pub amount: f64,
    pub threshold: f64,
    pub workers: i64,
}
impl UnsharpMaskOptions {
    fn validated(&self) -> Result<UnsharpMaskOptions, FilterError> {
        let mut o = *self;
        if o.amount == 0.0 {
            o.amount = 0.5;
        }
        if o.amount.is_nan() || o.amount.is_infinite() || o.threshold < 0.0 || o.threshold.is_nan()
        {
            return Err(FilterError::invalid(format!(
                "amount {}, threshold {}",
                float_v(o.amount),
                float_v(o.threshold)
            )));
        }
        Ok(o)
    }
    fn sharpen(&self, src: &[f32], blurred: Option<&[f32]>, dst: &mut [f32]) {
        for (i, v) in src.iter().copied().enumerate() {
            let b = match blurred {
                Some(b) => b[i],
                None => dst[i],
            };
            let detail = f64::from(v) - f64::from(b);
            if self.threshold > 0.0 && detail.abs() <= self.threshold {
                dst[i] = v;
                continue;
            }
            dst[i] = (f64::from(v) + self.amount * detail) as f32;
        }
    }
}
pub fn unsharp_mask(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: &UnsharpMaskOptions,
) -> Result<Volume, FilterError> {
    let o = opts.validated()?;
    let mut blurred = smooth_recursive_gaussian(
        ctx,
        src,
        RecursiveGaussianOptions {
            sigma_mm: o.sigma_mm,
            normalize_across_scale: false,
            workers: o.workers,
        },
    )?;
    o.sharpen(&src.data, None, &mut blurred.data);
    Ok(blurred)
}
struct UnsharpKernel {
    opts: UnsharpMaskOptions,
    dims: [usize; 3],
    kernels: [Option<RecursiveKernel>; 2],
    workers: i64,
    y_pass: Vec<f32>,
    blurred: Vec<f32>,
    tmp: Vec<f64>,
    state: Vec<f64>,
}
impl SlabFilter for UnsharpMaskOptions {
    fn slab_kernel(
        &self,
        dims: [i64; 3],
        spacing_mm: [f64; 3],
    ) -> Result<Box<dyn SlabKernel>, FilterError> {
        let o = self.validated()?;
        if dims[2] > 1 && o.sigma_mm[2] != 0.0 {
            return Err(FilterError::invalid(
                "UnsharpMask streams only in plane (a zero z sigma): its recursive Gaussian along z reads the whole column",
            ));
        }
        let mut k = UnsharpKernel {
            opts: o,
            dims: [dims[0] as usize, dims[1] as usize, dims[2] as usize],
            kernels: [None, None],
            workers: worker_bound(o.workers),
            y_pass: Vec::new(),
            blurred: Vec::new(),
            tmp: Vec::new(),
            state: Vec::new(),
        };
        for (axis, sigma) in o.sigma_mm.iter().copied().enumerate() {
            if !(sigma >= 0.0 && sigma <= f64::MAX) {
                return Err(FilterError::invalid(format!(
                    "sigma {} mm along axis {axis}",
                    float_v(sigma)
                )));
            }
            if axis == 2 || sigma == 0.0 || dims[axis] == 1 {
                continue;
            }
            let rk = RecursiveKernel::new(sigma / spacing_mm[axis], 0, 1.0)
                .map_err(|e| FilterError::invalid(format!("axis {axis}: {e}")))?;
            k.kernels[axis] = Some(rk);
        }
        Ok(Box::new(k))
    }
}
impl UnsharpKernel {
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &mut self,
        ctx: Ctx<'_>,
        rk: &RecursiveKernel,
        dst: &mut [f32],
        src: &[f32],
        length: usize,
        step: usize,
        lane_stride: usize,
        lanes: usize,
        groups: usize,
        group_stride: usize,
    ) -> Result<(), FilterError> {
        let chunks = lanes.div_ceil(LANE_WIDTH);
        for item in 0..groups * chunks {
            ctx_err(ctx)?;
            if self.tmp.len() < length * LANE_WIDTH {
                self.tmp = vec![0.0; length * LANE_WIDTH];
                self.state = vec![0.0; 4 * LANE_WIDTH];
            }
            let (g, c) = (item / chunks, item % chunks);
            let first = c * LANE_WIDTH;
            let count = LANE_WIDTH.min(lanes - first);
            let base = g * group_stride + first * lane_stride;
            rk.filter_lanes_window(
                dst,
                LaneWindow {
                    base,
                    step,
                    lane_stride,
                    lo: 0,
                    hi: length,
                },
                src,
                base,
                step,
                lane_stride,
                length,
                count,
                &mut self.tmp,
                &mut self.state,
            );
        }
        ctx_err(ctx)
    }
}
impl SlabKernel for UnsharpKernel {
    fn halo(&self) -> i64 {
        0
    }
    fn scratch_bytes(&self, slices: i64) -> i64 {
        let n = slices * (self.dims[0] * self.dims[1]) as i64;
        let line = ((self.dims[0].max(self.dims[1]) * LANE_WIDTH + 4 * LANE_WIDTH) * 8) as i64;
        2 * n * 4 + self.workers * line
    }
    fn run(
        &mut self,
        ctx: Ctx<'_>,
        input: &SlabView<'_>,
        z0: usize,
        z1: usize,
        dst: &mut [f32],
    ) -> Result<(), FilterError> {
        let (nx, ny) = (self.dims[0], self.dims[1]);
        let slices = z1 - z0;
        let n = slices * nx * ny;
        let src = &input.data[(z0 - input.lo) * nx * ny..(z1 - input.lo) * nx * ny];
        if self.y_pass.len() < n {
            self.y_pass = vec![0.0; n];
            self.blurred = vec![0.0; n];
        }
        let mut y_pass = std::mem::take(&mut self.y_pass);
        let mut blurred = std::mem::take(&mut self.blurred);
        let mut used_y = false;
        let mut r = Ok(());
        if let Some(rk) = self.kernels[1] {
            r = self.pass(
                ctx,
                &rk,
                &mut y_pass[..n],
                src,
                ny,
                nx,
                1,
                nx,
                slices,
                nx * ny,
            );
            used_y = true;
        }
        let mut used_blur = false;
        if r.is_ok()
            && let Some(rk) = self.kernels[0]
        {
            let y: &[f32] = if used_y { &y_pass[..n] } else { src };
            r = self.pass(ctx, &rk, &mut blurred[..n], y, nx, 1, nx, ny * slices, 1, 0);
            used_blur = true;
        }
        if r.is_ok() {
            let b: &[f32] = if used_blur {
                &blurred[..n]
            } else if used_y {
                &y_pass[..n]
            } else {
                src
            };
            self.opts.sharpen(src, Some(b), dst);
        }
        self.y_pass = y_pass;
        self.blurred = blurred;
        r?;
        ctx_err(ctx)
    }
}
