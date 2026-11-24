use super::volume::{Ctx, FilterError, Volume, ctx_err, validate_shape};
use crate::fmath;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex128 {
    pub re: f64,
    pub im: f64,
}
impl Complex128 {
    pub const fn new(re: f64, im: f64) -> Complex128 {
        Complex128 { re, im }
    }
    pub(crate) fn add(self, b: Complex128) -> Complex128 {
        Complex128::new(self.re + b.re, self.im + b.im)
    }
    pub(crate) fn sub(self, b: Complex128) -> Complex128 {
        Complex128::new(self.re - b.re, self.im - b.im)
    }
    pub(crate) fn mul(self, b: Complex128) -> Complex128 {
        Complex128::new(
            self.re * b.re - self.im * b.im,
            self.re * b.im + self.im * b.re,
        )
    }
    pub(crate) fn div(self, m: Complex128) -> Complex128 {
        let n = self;
        let (mut e, mut f);
        if m.re.abs() >= m.im.abs() {
            let ratio = m.im / m.re;
            let denom = m.re + ratio * m.im;
            e = (n.re + n.im * ratio) / denom;
            f = (n.im - n.re * ratio) / denom;
        } else {
            let ratio = m.re / m.im;
            let denom = m.im + ratio * m.re;
            e = (n.re * ratio + n.im) / denom;
            f = (n.im * ratio - n.re) / denom;
        }
        if e.is_nan() && f.is_nan() {
            let (mut a, mut b) = (n.re, n.im);
            let (mut c, mut d) = (m.re, m.im);
            let inf2one = |x: f64| {
                if x.is_infinite() {
                    1f64.copysign(x)
                } else {
                    0f64.copysign(x)
                }
            };
            if m.re == 0.0 && m.im == 0.0 && (!a.is_nan() || !b.is_nan()) {
                e = f64::INFINITY.copysign(c) * a;
                f = f64::INFINITY.copysign(c) * b;
            } else if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
                a = inf2one(a);
                b = inf2one(b);
                e = f64::INFINITY * (a * c + b * d);
                f = f64::INFINITY * (b * c - a * d);
            } else if (c.is_infinite() || d.is_infinite()) && a.is_finite() && b.is_finite() {
                c = inf2one(c);
                d = inf2one(d);
                e = 0.0 * (a * c + b * d);
                f = 0.0 * (b * c - a * d);
            }
        }
        Complex128::new(e, f)
    }
    pub(crate) fn exp(self) -> Complex128 {
        let (re, im) = (self.re, self.im);
        if re.is_infinite() {
            if re > 0.0 && im == 0.0 {
                return self;
            }
            if im.is_infinite() || im.is_nan() {
                if re < 0.0 {
                    return Complex128::new(0.0, 0f64.copysign(im));
                }
                return Complex128::new(f64::INFINITY, f64::NAN);
            }
        } else if re.is_nan() && im == 0.0 {
            return Complex128::new(f64::NAN, im);
        }
        let r = fmath::exp(re);
        let (s, c) = if im == 0.0 {
            (im, 1.0)
        } else if im.is_nan() || im.is_infinite() {
            (f64::NAN, f64::NAN)
        } else {
            (fmath::sin(im), fmath::cos(im))
        };
        Complex128::new(r * c, r * s)
    }
    pub(crate) fn finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}
const ONE: Complex128 = Complex128::new(1.0, 0.0);
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RecursiveGaussianOptions {
    pub sigma_mm: [f64; 3],
    pub normalize_across_scale: bool,
    pub workers: i64,
}
pub fn isotropic_sigma(mm: f64) -> [f64; 3] {
    [mm, mm, mm]
}
#[derive(Debug, Clone, PartialEq)]
pub struct Hessian {
    pub xx: Volume,
    pub xy: Volume,
    pub xz: Volume,
    pub yy: Volume,
    pub yz: Volume,
    pub zz: Volume,
}
fn volume_like(src: &Volume, data: Vec<f32>) -> Volume {
    Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data,
    }
}
pub fn smooth_recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: RecursiveGaussianOptions,
) -> Result<Volume, FilterError> {
    recursive_gaussian(ctx, src, [0, 0, 0], opts)
}
pub fn recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    orders: [i64; 3],
    opts: RecursiveGaussianOptions,
) -> Result<Volume, FilterError> {
    let mut out = None;
    run_recursive_gaussian(ctx, src, &[orders], opts, &mut |_, data| {
        out = Some(data);
        Ok(())
    })?;
    ctx_err(ctx)?;
    Ok(volume_like(src, out.unwrap_or_default()))
}
pub fn gradient_recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: RecursiveGaussianOptions,
) -> Result<[Volume; 3], FilterError> {
    let mut out: [Vec<f32>; 3] = Default::default();
    run_recursive_gaussian(
        ctx,
        src,
        &[[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        opts,
        &mut |i, data| {
            out[i] = data;
            Ok(())
        },
    )?;
    ctx_err(ctx)?;
    Ok(out.map(|data| volume_like(src, data)))
}
pub fn gradient_magnitude_recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: RecursiveGaussianOptions,
) -> Result<Volume, FilterError> {
    let mut acc: Option<Vec<f32>> = None;
    run_recursive_gaussian(
        ctx,
        src,
        &[[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        opts,
        &mut |_, mut data| {
            ctx_err(ctx)?;
            match &mut acc {
                None => {
                    for v in data.iter_mut() {
                        *v = *v * *v;
                    }
                    acc = Some(data);
                }
                Some(acc) => {
                    for (a, v) in acc.iter_mut().zip(&data) {
                        *a += *v * *v;
                    }
                }
            }
            Ok(())
        },
    )?;
    let mut acc = acc.unwrap_or_default();
    ctx_err(ctx)?;
    for v in acc.iter_mut() {
        *v = f64::from(*v).sqrt() as f32;
    }
    ctx_err(ctx)?;
    Ok(volume_like(src, acc))
}
pub fn laplacian_recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: RecursiveGaussianOptions,
) -> Result<Volume, FilterError> {
    let mut acc: Option<Vec<f32>> = None;
    run_recursive_gaussian(
        ctx,
        src,
        &[[2, 0, 0], [0, 2, 0], [0, 0, 2]],
        opts,
        &mut |_, data| {
            match &mut acc {
                None => acc = Some(data),
                Some(acc) => {
                    ctx_err(ctx)?;
                    for (a, v) in acc.iter_mut().zip(&data) {
                        *a += *v;
                    }
                }
            }
            Ok(())
        },
    )?;
    ctx_err(ctx)?;
    Ok(volume_like(src, acc.unwrap_or_default()))
}
pub fn hessian_recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: RecursiveGaussianOptions,
) -> Result<Hessian, FilterError> {
    let mut out: [Vec<f32>; 6] = Default::default();
    let specs = [
        [2, 0, 0],
        [1, 1, 0],
        [1, 0, 1],
        [0, 2, 0],
        [0, 1, 1],
        [0, 0, 2],
    ];
    run_recursive_gaussian(ctx, src, &specs, opts, &mut |i, data| {
        out[i] = data;
        Ok(())
    })?;
    ctx_err(ctx)?;
    let [xx, xy, xz, yy, yz, zz] = out.map(|data| volume_like(src, data));
    Ok(Hessian {
        xx,
        xy,
        xz,
        yy,
        yz,
        zz,
    })
}
struct GaussianPlan<'a> {
    src: &'a Volume,
    n: usize,
    kernels: [[Option<RecursiveKernel>; 3]; 3],
}
fn run_recursive_gaussian(
    ctx: Ctx<'_>,
    src: &Volume,
    specs: &[[i64; 3]],
    opts: RecursiveGaussianOptions,
    emit: &mut dyn FnMut(usize, Vec<f32>) -> Result<(), FilterError>,
) -> Result<(), FilterError> {
    let p = new_gaussian_plan(ctx, src, specs, opts)?;
    ctx_err(ctx)?;
    let mut done = vec![false; specs.len()];
    for i in 0..specs.len() {
        if done[i] {
            continue;
        }
        let spec = specs[i];
        if p.is_zero(spec) {
            done[i] = true;
            emit(i, vec![0.0; p.n])?;
            continue;
        }
        let z_owned = p.pass(ctx, &src.data, 2, spec[2])?;
        let z: &[f32] = z_owned.as_deref().unwrap_or(&src.data);
        for j in i..specs.len() {
            if done[j] || specs[j][2] != spec[2] || p.is_zero(specs[j]) {
                continue;
            }
            let y_owned = p.pass(ctx, z, 1, specs[j][1])?;
            let y: &[f32] = y_owned.as_deref().unwrap_or(z);
            for k in j..specs.len() {
                if done[k]
                    || specs[k][2] != spec[2]
                    || specs[k][1] != specs[j][1]
                    || p.is_zero(specs[k])
                {
                    continue;
                }
                let x = match p.pass(ctx, y, 0, specs[k][0])? {
                    Some(x) => x,
                    None => y.to_vec(),
                };
                done[k] = true;
                emit(k, x)?;
            }
        }
    }
    Ok(())
}
fn new_gaussian_plan<'a>(
    ctx: Ctx<'_>,
    src: &'a Volume,
    specs: &[[i64; 3]],
    opts: RecursiveGaussianOptions,
) -> Result<GaussianPlan<'a>, FilterError> {
    validate_gaussian_source(ctx, src)?;
    let mut p = GaussianPlan {
        src,
        n: src.data.len(),
        kernels: Default::default(),
    };
    for (axis, sigma) in opts.sigma_mm.iter().enumerate() {
        if !(*sigma >= 0.0 && *sigma <= f64::MAX) {
            return Err(FilterError::invalid(format!(
                "sigma {} mm along axis {axis}",
                float_v(*sigma)
            )));
        }
    }
    for spec in specs {
        for (axis, order) in spec.iter().copied().enumerate() {
            if !(0..=2).contains(&order) {
                return Err(FilterError::invalid(format!(
                    "derivative order {order} along axis {axis} (want 0, 1 or 2)"
                )));
            }
            let sigma = opts.sigma_mm[axis];
            if src.dims[axis] == 1 || (order == 0 && sigma == 0.0) {
                continue;
            }
            if sigma == 0.0 {
                return Err(FilterError::invalid(format!(
                    "order-{order} derivative along axis {axis} needs a positive sigma"
                )));
            }
            if p.kernels[axis][order as usize].is_some() {
                continue;
            }
            let mut scale = fmath::pow(src.spacing_mm[axis], -(order as f64));
            if opts.normalize_across_scale {
                scale *= fmath::pow(sigma, order as f64);
            }
            let k = RecursiveKernel::new(sigma / src.spacing_mm[axis], order, scale)
                .map_err(|e| FilterError::invalid(format!("axis {axis}: {e}")))?;
            p.kernels[axis][order as usize] = Some(k);
        }
    }
    Ok(p)
}
pub(crate) const LANE_WIDTH: usize = 32;
impl GaussianPlan<'_> {
    fn is_zero(&self, spec: [i64; 3]) -> bool {
        spec.iter()
            .enumerate()
            .any(|(axis, order)| *order > 0 && self.src.dims[axis] == 1)
    }
    fn pass(
        &self,
        ctx: Ctx<'_>,
        input: &[f32],
        axis: usize,
        order: i64,
    ) -> Result<Option<Vec<f32>>, FilterError> {
        let Some(k) = &self.kernels[axis][order as usize] else {
            return Ok(None);
        };
        let mut out = vec![0f32; self.n];
        let dims = self.src.dims;
        let (nx, ny, nz) = (dims[0] as usize, dims[1] as usize, dims[2] as usize);
        let (length, step, lane_stride, lanes, groups, group_stride) = match axis {
            0 => (nx, 1, nx, ny * nz, 1, 0),
            1 => (ny, nx, 1, nx, nz, nx * ny),
            _ => (nz, nx * ny, 1, nx * ny, 1, 0),
        };
        let chunks = lanes.div_ceil(LANE_WIDTH);
        let count = groups * chunks;
        let mut tmp = vec![0f64; length * LANE_WIDTH.min(lanes)];
        let mut state = vec![0f64; 4 * LANE_WIDTH];
        for item in 0..count {
            ctx_err(ctx)?;
            let (g, c) = (item / chunks, item % chunks);
            let first = c * LANE_WIDTH;
            let lane_count = LANE_WIDTH.min(lanes - first);
            let base = g * group_stride + first * lane_stride;
            k.filter_lanes_window(
                &mut out,
                LaneWindow {
                    base,
                    step,
                    lane_stride,
                    lo: 0,
                    hi: length,
                },
                input,
                base,
                step,
                lane_stride,
                length,
                lane_count,
                &mut tmp,
                &mut state,
            );
        }
        Ok(Some(out))
    }
}
#[derive(Clone, Copy)]
struct IirFit {
    lambda: [f64; 2],
    omega: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
}
const DERICHE_GAUSSIAN: IirFit = IirFit {
    lambda: [-1.783, -1.723],
    omega: [0.6318, 1.997],
    a: [1.680, -0.6803],
    b: [3.735, -0.2598],
};
const FW_LAMBDA: [f64; 2] = [-1.3932, -1.3732];
const FW_OMEGA: [f64; 2] = [0.6681, 2.0787];
const FW_GAUSSIAN: IirFit = IirFit {
    lambda: FW_LAMBDA,
    omega: FW_OMEGA,
    a: [1.3530, -0.3531],
    b: [1.8151, 0.0902],
};
const FW_FIRST: IirFit = IirFit {
    lambda: FW_LAMBDA,
    omega: FW_OMEGA,
    a: [-0.6724, 0.6724],
    b: [-3.4327, 0.6100],
};
const FW_SECOND: IirFit = IirFit {
    lambda: FW_LAMBDA,
    omega: FW_OMEGA,
    a: [-1.3563, 0.3446],
    b: [5.2318, -2.2355],
};
impl IirFit {
    fn gains(&self) -> [Complex128; 2] {
        [
            Complex128::new(self.a[0], -self.b[0]),
            Complex128::new(self.a[1], -self.b[1]),
        ]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct RecursiveKernel {
    pub(crate) z: [Complex128; 2],
    pub(crate) c: [Complex128; 2],
    pub(crate) d: [Complex128; 2],
    pub(crate) c_init: [Complex128; 2],
    pub(crate) d_init: [Complex128; 2],
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct LaneWindow {
    pub(crate) base: usize,
    pub(crate) step: usize,
    pub(crate) lane_stride: usize,
    pub(crate) lo: usize,
    pub(crate) hi: usize,
}
impl RecursiveKernel {
    pub(crate) fn new(sigma: f64, order: i64, scale: f64) -> Result<RecursiveKernel, String> {
        if !(sigma > 0.0) || sigma.is_infinite() {
            return Err(format!("sigma {} samples", float_v(sigma)));
        }
        if !(0..=2).contains(&order) {
            return Err(format!("order {order}"));
        }
        let fit = [DERICHE_GAUSSIAN, FW_FIRST, FW_SECOND][order as usize];
        let mut k = RecursiveKernel::default();
        for i in 0..2 {
            k.z[i] = Complex128::new(fit.lambda[i], fit.omega[i])
                .div(Complex128::new(sigma, 0.0))
                .exp();
        }
        let kz = k.z;
        let moment = |g: [Complex128; 2], m: i64| -> f64 {
            let mut s = Complex128::default();
            for (i, z) in kz.iter().copied().enumerate() {
                let u = ONE.sub(z);
                s = s.add(match m {
                    0 => g[i].div(u),
                    1 => g[i].mul(z).div(u.mul(u)),
                    _ => g[i].mul(z).mul(ONE.add(z)).div(u.mul(u).mul(u)),
                });
            }
            s.re
        };
        let origin = |g: [Complex128; 2]| g[0].add(g[1]).re;
        let sum = |g: [Complex128; 2]| 2.0 * moment(g, 0) - origin(g);
        let mut g = [Complex128::default(); 2];
        let mut sign = ONE;
        let norm = match order {
            0 => {
                g = DERICHE_GAUSSIAN.gains();
                sum(g)
            }
            1 => {
                g = FW_FIRST.gains();
                sign = Complex128::new(-1.0, 0.0);
                -2.0 * moment(g, 1)
            }
            _ => {
                let (g2, g0) = (FW_SECOND.gains(), FW_GAUSSIAN.gains());
                let beta = -sum(g2) / sum(g0);
                for i in 0..2 {
                    g[i] = g2[i].add(Complex128::new(beta, 0.0).mul(g0[i]));
                }
                moment(g, 2)
            }
        };
        if norm == 0.0 || norm.is_nan() || norm.is_infinite() {
            return Err(format!(
                "degenerate kernel for sigma {} samples",
                float_v(sigma)
            ));
        }
        let factor = Complex128::new(scale / norm, 0.0);
        for (i, z) in kz.iter().copied().enumerate() {
            k.c[i] = g[i].mul(factor);
            k.d[i] = sign.mul(k.c[i]);
            k.c_init[i] = k.c[i].div(ONE.sub(z));
            k.d_init[i] = k.d[i].mul(z).div(ONE.sub(z));
        }
        Ok(k)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn filter_lanes_window(
        &self,
        dst: &mut [f32],
        w: LaneWindow,
        src: &[f32],
        base: usize,
        step: usize,
        lane_stride: usize,
        length: usize,
        count: usize,
        tmp: &mut [f64],
        state: &mut [f64],
    ) {
        let (z1r, z1i, z2r, z2i) = (self.z[0].re, self.z[0].im, self.z[1].re, self.z[1].im);
        let (c1r, c1i, c2r, c2i) = (self.c[0].re, self.c[0].im, self.c[1].re, self.c[1].im);
        let st = &mut state[..4 * count];
        let (lo, hi) = (w.lo, w.hi);
        let tmp = &mut tmp[..(hi - lo) * count];
        for l in 0..count {
            let x = Complex128::new(f64::from(src[base + l * lane_stride]), 0.0);
            let (y1, y2) = (self.c_init[0].mul(x), self.c_init[1].mul(x));
            st[4 * l] = y1.re;
            st[4 * l + 1] = y1.im;
            st[4 * l + 2] = y2.re;
            st[4 * l + 3] = y2.im;
        }
        for j in 0..hi {
            let row = base + j * step;
            for l in 0..count {
                let x = f64::from(src[row + l * lane_stride]);
                let s = &mut st[4 * l..4 * l + 4];
                let r1 = c1r * x + z1r * s[0] - z1i * s[1];
                let i1 = c1i * x + z1r * s[1] + z1i * s[0];
                let r2 = c2r * x + z2r * s[2] - z2i * s[3];
                let i2 = c2i * x + z2r * s[3] + z2i * s[2];
                s[0] = r1;
                s[1] = i1;
                s[2] = r2;
                s[3] = i2;
                if j >= lo {
                    tmp[(j - lo) * count + l] = r1 + r2;
                }
            }
        }
        let (d1r, d1i, d2r, d2i) = (self.d[0].re, self.d[0].im, self.d[1].re, self.d[1].im);
        let last = base + (length - 1) * step;
        let mut next_original = [0f64; LANE_WIDTH];
        for l in 0..count {
            next_original[l] = f64::from(src[last + l * lane_stride]);
            let x = Complex128::new(next_original[l], 0.0);
            let (w1, w2) = (self.d_init[0].mul(x), self.d_init[1].mul(x));
            st[4 * l] = w1.re;
            st[4 * l + 1] = w1.im;
            st[4 * l + 2] = w2.re;
            st[4 * l + 3] = w2.im;
            if length - 1 < hi {
                dst[w.base + l * w.lane_stride + (length - 1 - lo) * w.step] =
                    (tmp[(length - 1 - lo) * count + l] + w1.re + w2.re) as f32;
            }
        }
        let mut j = length - 1;
        while j > lo {
            j -= 1;
            let current = base + j * step;
            for l in 0..count {
                let x = next_original[l];
                next_original[l] = f64::from(src[current + l * lane_stride]);
                let s = &mut st[4 * l..4 * l + 4];
                let (u1r, u1i) = (d1r * x + s[0], d1i * x + s[1]);
                let (u2r, u2i) = (d2r * x + s[2], d2i * x + s[3]);
                let (r1, i1) = (z1r * u1r - z1i * u1i, z1r * u1i + z1i * u1r);
                let (r2, i2) = (z2r * u2r - z2i * u2i, z2r * u2i + z2i * u2r);
                s[0] = r1;
                s[1] = i1;
                s[2] = r2;
                s[3] = i2;
                if j < hi {
                    dst[w.base + (j - lo) * w.step + l * w.lane_stride] =
                        (tmp[(j - lo) * count + l] + r1 + r2) as f32;
                }
            }
        }
    }
}
pub(crate) fn validate_gaussian_source(ctx: Ctx<'_>, src: &Volume) -> Result<(), FilterError> {
    validate_shape(src)?;
    for (i, v) in src.data.iter().enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        if !v.is_finite() {
            return Err(FilterError::invalid(format!(
                "non-finite sample at index {i}"
            )));
        }
    }
    ctx_err(ctx)
}
