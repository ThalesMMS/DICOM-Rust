use super::recursive_gaussian::{LANE_WIDTH, LaneWindow, RecursiveKernel};
use super::volume::{Ctx, FilterError, Volume, ctx_err};
use crate::fmath;
use crate::numeric::linalg;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VesselnessMeasure(pub u8);
impl VesselnessMeasure {
    pub const FRANGI: VesselnessMeasure = VesselnessMeasure(0);
    pub const SATO: VesselnessMeasure = VesselnessMeasure(1);
    pub fn as_str(self) -> &'static str {
        match self.0 {
            0 => "frangi",
            1 => "sato",
            _ => "unknown",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HessianStructure(pub u8);
impl HessianStructure {
    pub const TUBE: HessianStructure = HessianStructure(0);
    pub const BLOB: HessianStructure = HessianStructure(1);
    pub const PLATE: HessianStructure = HessianStructure(2);
    fn object_dimension(self) -> usize {
        match self {
            HessianStructure::BLOB => 0,
            HessianStructure::PLATE => 2,
            _ => 1,
        }
    }
}
pub const DEFAULT_VESSELNESS_SIGMA_STEPS: i64 = 4;
pub const DEFAULT_VESSELNESS_WORKING_BYTES: i64 = 256 << 20;
const VESSELNESS_MARGIN_SIGMAS: f64 = 6.0;
const VESSELNESS_SLAB_BUFFERS: usize = 7;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VesselnessOptions {
    pub measure: VesselnessMeasure,
    pub structure: HessianStructure,
    pub dark_objects: bool,
    pub sigmas_mm: Vec<f64>,
    pub sigma_min_mm: f64,
    pub sigma_max_mm: f64,
    pub sigma_steps: i64,
    pub equispaced_sigmas: bool,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub scale_by_largest_eigenvalue: bool,
    pub alpha1: f64,
    pub alpha2: f64,
    pub scales_output: bool,
    pub max_working_bytes: i64,
    pub workers: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct VesselnessResult {
    pub response: Volume,
    pub scale_mm: Option<Volume>,
    pub sigmas_mm: Vec<f64>,
    pub gammas: Option<Vec<f64>>,
    pub slab_slices: i64,
    pub working_bytes: i64,
}
pub fn vesselness_sigmas(
    min_mm: f64,
    max_mm: f64,
    steps: i64,
    equispaced: bool,
) -> Result<Vec<f64>, FilterError> {
    if !(min_mm > 0.0) || min_mm.is_infinite() || !(max_mm >= min_mm) || max_mm.is_infinite() {
        return Err(FilterError::invalid(format!(
            "sigma range {}–{} mm",
            float_v(min_mm),
            float_v(max_mm)
        )));
    }
    if steps < 1 {
        return Err(FilterError::invalid(format!("{steps} sigma steps")));
    }
    if steps == 1 || max_mm == min_mm {
        return Ok(vec![min_mm]);
    }
    let n = steps as usize;
    let mut sigmas = vec![0.0; n];
    for (i, s) in sigmas.iter_mut().enumerate() {
        if equispaced {
            *s = min_mm + (max_mm - min_mm) * i as f64 / (steps - 1) as f64;
        } else {
            let step = (fmath::log(max_mm) - fmath::log(min_mm)) / (steps - 1) as f64;
            *s = fmath::exp(fmath::log(min_mm) + step * i as f64);
        }
    }
    sigmas[n - 1] = max_mm;
    Ok(sigmas)
}
pub fn plan_vesselness_slab_slices(dims: [i64; 3], working_bytes: i64) -> Result<i64, FilterError> {
    if dims[0] <= 0 || dims[1] <= 0 || dims[2] <= 0 || working_bytes < 0 {
        return Err(FilterError::invalid("invalid vesselness slab metadata"));
    }
    if dims[0] > i64::MAX / dims[1] / VESSELNESS_SLAB_BUFFERS as i64 / 4 {
        return Err(FilterError::invalid("vesselness slab byte count overflows"));
    }
    let working_bytes = if working_bytes == 0 {
        DEFAULT_VESSELNESS_WORKING_BYTES
    } else {
        working_bytes
    };
    let slice_bytes = dims[0] * dims[1] * VESSELNESS_SLAB_BUFFERS as i64 * 4;
    Ok(1.max(dims[2].min(working_bytes / slice_bytes)))
}
pub fn vesselness(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: &VesselnessOptions,
) -> Result<VesselnessResult, FilterError> {
    src.validate()?;
    let mut p = VesselPlan::new(ctx, src, opts)?;
    ctx_err(ctx)?;
    p.allocate();
    let n = src.data.len();
    let mut res = VesselnessResult {
        response: Volume {
            dims: src.dims,
            spacing_mm: src.spacing_mm,
            data: vec![0.0; n],
        },
        scale_mm: opts.scales_output.then(|| Volume {
            dims: src.dims,
            spacing_mm: src.spacing_mm,
            data: vec![0.0; n],
        }),
        sigmas_mm: p.sigmas.clone(),
        gammas: (p.measure == VesselnessMeasure::FRANGI).then(|| vec![0.0; p.sigmas.len()]),
        slab_slices: p.slab as i64,
        working_bytes: p.working_bytes,
    };
    let sigmas = p.sigmas.clone();
    for (i, sigma) in sigmas.iter().copied().enumerate() {
        p.set_scale(sigma)?;
        let mut gamma = p.gamma;
        let slabs = p.nz.div_ceil(p.slab);
        if p.measure == VesselnessMeasure::FRANGI && p.auto_gamma {
            if slabs > 1 {
                let mut max_norm = 0.0;
                let mut z0 = 0;
                while z0 < p.nz {
                    let z1 = p.nz.min(z0 + p.slab);
                    let h = p.hessian_slab(z0, z1)?;
                    let m = p.max_norm(&h, (z1 - z0) * p.nxy)?;
                    max_norm = fmath::max(max_norm, m);
                    z0 += p.slab;
                }
                gamma = max_norm / 2.0;
            } else {
                let h = p.hessian_slab(0, p.nz)?;
                let m = p.max_norm(&h, p.nz * p.nxy)?;
                gamma = m / 2.0;
                p.measure_slab(&h, 0, p.nz, gamma, sigma, &mut res)?;
                if let Some(g) = &mut res.gammas {
                    g[i] = gamma;
                }
                continue;
            }
        }
        if let Some(g) = &mut res.gammas {
            g[i] = fmath::max(gamma, 0.0);
        }
        let mut z0 = 0;
        while z0 < p.nz {
            let z1 = p.nz.min(z0 + p.slab);
            let h = p.hessian_slab(z0, z1)?;
            p.measure_slab(&h, z0, z1, gamma, sigma, &mut res)?;
            z0 += p.slab;
        }
    }
    ctx_err(ctx)?;
    Ok(res)
}
type Comp = Option<usize>;
#[derive(Clone, Copy)]
struct HessianSlab {
    xx: Comp,
    xy: Comp,
    xz: Comp,
    yy: Comp,
    yz: Comp,
    zz: Comp,
    n: usize,
}
struct VesselPlan<'a> {
    ctx: Ctx<'a>,
    src: &'a Volume,
    nx: usize,
    ny: usize,
    nz: usize,
    nxy: usize,
    sigmas: Vec<f64>,
    measure: VesselnessMeasure,
    object_dim: usize,
    image_dim: usize,
    dark: bool,
    alpha: f64,
    beta: f64,
    gamma: f64,
    auto_gamma: bool,
    scale_by_l3: bool,
    alpha1: f64,
    alpha2: f64,
    slab: usize,
    max_line: usize,
    working_bytes: i64,
    bufs: Vec<Vec<f32>>,
    tmp: Vec<f64>,
    state: Vec<f64>,
    kernels: [[Option<RecursiveKernel>; 3]; 3],
    margin: usize,
}
fn weight(name: &str, v: f64, def: f64) -> Result<f64, FilterError> {
    if v == 0.0 {
        return Ok(def);
    }
    if !(v > 0.0) || v.is_infinite() {
        return Err(FilterError::invalid(format!("{name} {}", float_v(v))));
    }
    Ok(v)
}
impl<'a> VesselPlan<'a> {
    fn new(
        ctx: Ctx<'a>,
        src: &'a Volume,
        opts: &VesselnessOptions,
    ) -> Result<VesselPlan<'a>, FilterError> {
        let (nx, ny, nz) = (
            src.dims[0] as usize,
            src.dims[1] as usize,
            src.dims[2] as usize,
        );
        let mut p = VesselPlan {
            ctx,
            src,
            nx,
            ny,
            nz,
            nxy: nx * ny,
            sigmas: Vec::new(),
            measure: opts.measure,
            object_dim: 0,
            image_dim: if nz == 1 { 2 } else { 3 },
            dark: opts.dark_objects,
            alpha: 0.0,
            beta: 0.0,
            gamma: 0.0,
            auto_gamma: false,
            scale_by_l3: opts.scale_by_largest_eigenvalue,
            alpha1: 0.0,
            alpha2: 0.0,
            slab: 0,
            max_line: 0,
            working_bytes: 0,
            bufs: Vec::new(),
            tmp: Vec::new(),
            state: Vec::new(),
            kernels: Default::default(),
            margin: 0,
        };
        match opts.measure {
            VesselnessMeasure::FRANGI => {
                if opts.structure.0 > HessianStructure::PLATE.0 {
                    return Err(FilterError::invalid(format!(
                        "Hessian structure {}",
                        opts.structure.0
                    )));
                }
                p.object_dim = opts.structure.object_dimension();
                if p.object_dim >= p.image_dim {
                    return Err(FilterError::invalid("plate objectness needs a 3-D volume"));
                }
                p.alpha = weight("alpha", opts.alpha, 0.5)?;
                p.beta = weight("beta", opts.beta, 0.5)?;
                if opts.gamma.is_nan() || opts.gamma.is_infinite() {
                    return Err(FilterError::invalid(format!(
                        "gamma {}",
                        float_v(opts.gamma)
                    )));
                }
                p.gamma = opts.gamma;
                p.auto_gamma = opts.gamma == 0.0;
            }
            VesselnessMeasure::SATO => {
                if p.image_dim != 3 {
                    return Err(FilterError::invalid(
                        "Sato's line measure needs a 3-D volume",
                    ));
                }
                p.alpha1 = weight("alpha1", opts.alpha1, 0.5)?;
                p.alpha2 = weight("alpha2", opts.alpha2, 2.0)?;
            }
            m => {
                return Err(FilterError::invalid(format!("vesselness measure {}", m.0)));
            }
        }
        if !opts.sigmas_mm.is_empty() {
            for s in &opts.sigmas_mm {
                if !(*s > 0.0) || s.is_infinite() {
                    return Err(FilterError::invalid(format!("sigma {} mm", float_v(*s))));
                }
            }
            p.sigmas = opts.sigmas_mm.clone();
        } else {
            let mut max_mm = opts.sigma_max_mm;
            let mut steps = opts.sigma_steps;
            if max_mm == 0.0 {
                max_mm = opts.sigma_min_mm;
            }
            if steps == 0 {
                steps = 1;
                if max_mm > opts.sigma_min_mm {
                    steps = DEFAULT_VESSELNESS_SIGMA_STEPS;
                }
            }
            p.sigmas = vesselness_sigmas(opts.sigma_min_mm, max_mm, steps, opts.equispaced_sigmas)?;
        }
        let workers = crate::render::parallel::workers(opts.workers);
        p.max_line = nx.max(ny).max(nz);
        let scratch_bytes = workers
            .wrapping_mul((p.max_line * LANE_WIDTH + 4 * LANE_WIDTH) as i64)
            .wrapping_mul(8);
        let mut budget = opts.max_working_bytes;
        if budget < 0 {
            return Err(FilterError::invalid(format!(
                "working bound {budget} bytes"
            )));
        }
        if budget == 0 {
            budget = DEFAULT_VESSELNESS_WORKING_BYTES;
        }
        p.slab = plan_vesselness_slab_slices(src.dims, budget)? as usize;
        let slice_bytes = VESSELNESS_SLAB_BUFFERS as i64 * p.nxy as i64 * 4;
        p.working_bytes = (p.slab as i64)
            .wrapping_mul(slice_bytes)
            .wrapping_add(scratch_bytes);
        Ok(p)
    }
    fn allocate(&mut self) {
        let n = self.slab * self.nxy;
        self.bufs = (0..VESSELNESS_SLAB_BUFFERS)
            .map(|_| vec![0f32; n])
            .collect();
        self.tmp = vec![0f64; self.max_line * LANE_WIDTH];
        self.state = vec![0f64; 4 * LANE_WIDTH];
    }
    fn set_scale(&mut self, sigma: f64) -> Result<(), FilterError> {
        self.kernels = Default::default();
        for axis in 0..3 {
            if self.src.dims[axis] == 1 {
                continue;
            }
            let samples = sigma / self.src.spacing_mm[axis];
            for order in 0..=2i64 {
                let k = RecursiveKernel::new(samples, order, fmath::pow(samples, order as f64))
                    .map_err(|e| FilterError::invalid(format!("axis {axis}: {e}")))?;
                self.kernels[axis][order as usize] = Some(k);
            }
        }
        self.margin = 0;
        if self.nz > 1 {
            self.margin = fmath::f64_to_i64(
                (VESSELNESS_MARGIN_SIGMAS * sigma / self.src.spacing_mm[2]).ceil(),
            ) as usize;
        }
        Ok(())
    }
    fn z_pass(
        &mut self,
        dst: usize,
        order: usize,
        z0: usize,
        z1: usize,
    ) -> Result<(), FilterError> {
        let Some(k) = self.kernels[2][order] else {
            return Ok(());
        };
        let zlo = z0.saturating_sub(self.margin);
        let zhi = self.nz.min(z1.saturating_add(self.margin));
        let length = zhi - zlo;
        let chunks = self.nxy.div_ceil(LANE_WIDTH);
        let mut out = std::mem::take(&mut self.bufs[dst]);
        for c in 0..chunks {
            ctx_err(self.ctx)?;
            let first = c * LANE_WIDTH;
            let count = LANE_WIDTH.min(self.nxy - first);
            k.filter_lanes_window(
                &mut out,
                LaneWindow {
                    base: first,
                    step: self.nxy,
                    lane_stride: 1,
                    lo: z0 - zlo,
                    hi: z1 - zlo,
                },
                &self.src.data,
                zlo * self.nxy + first,
                self.nxy,
                1,
                length,
                count,
                &mut self.tmp,
                &mut self.state,
            );
        }
        self.bufs[dst] = out;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn plane_pass(
        ctx: Ctx<'_>,
        nx: usize,
        ny: usize,
        out: &mut [f32],
        input: &[f32],
        k: &RecursiveKernel,
        axis: usize,
        slices: usize,
        tmp: &mut [f64],
        state: &mut [f64],
    ) -> Result<(), FilterError> {
        let nxy = nx * ny;
        let (length, step, lane_stride, lanes, groups, group_stride) = if axis == 0 {
            (nx, 1, nx, ny * slices, 1, 0)
        } else {
            (ny, nx, 1, nx, slices, nxy)
        };
        let chunks = lanes.div_ceil(LANE_WIDTH);
        for item in 0..groups * chunks {
            ctx_err(ctx)?;
            let (g, c) = (item / chunks, item % chunks);
            let first = c * LANE_WIDTH;
            let count = LANE_WIDTH.min(lanes - first);
            let base = g * group_stride + first * lane_stride;
            k.filter_lanes_window(
                out,
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
                count,
                tmp,
                state,
            );
        }
        Ok(())
    }
    fn component(
        &mut self,
        dst: usize,
        zsrc: Comp,
        oy: usize,
        ox: usize,
        slices: usize,
    ) -> Result<Comp, FilterError> {
        let Some(zsrc) = zsrc else {
            return Ok(None);
        };
        const TMP: usize = 3;
        let n = slices * self.nxy;
        let mut y = zsrc;
        if let Some(k) = self.kernels[1][oy] {
            let mut out = std::mem::take(&mut self.bufs[TMP]);
            Self::plane_pass(
                self.ctx,
                self.nx,
                self.ny,
                &mut out[..n],
                &self.bufs[zsrc][..n],
                &k,
                1,
                slices,
                &mut self.tmp,
                &mut self.state,
            )?;
            self.bufs[TMP] = out;
            y = TMP;
        } else if oy > 0 {
            return Ok(None);
        }
        if let Some(k) = self.kernels[0][ox] {
            let mut out = std::mem::take(&mut self.bufs[dst]);
            let r = if y == dst {
                let input = out[..n].to_vec();
                Self::plane_pass(
                    self.ctx,
                    self.nx,
                    self.ny,
                    &mut out[..n],
                    &input,
                    &k,
                    0,
                    slices,
                    &mut self.tmp,
                    &mut self.state,
                )
            } else {
                Self::plane_pass(
                    self.ctx,
                    self.nx,
                    self.ny,
                    &mut out[..n],
                    &self.bufs[y][..n],
                    &k,
                    0,
                    slices,
                    &mut self.tmp,
                    &mut self.state,
                )
            };
            self.bufs[dst] = out;
            r?;
        } else if ox > 0 {
            return Ok(None);
        } else if y != dst {
            let (a, b) = if y < dst {
                let (l, r) = self.bufs.split_at_mut(dst);
                (&l[y], &mut r[0])
            } else {
                let (l, r) = self.bufs.split_at_mut(y);
                (&r[0], &mut l[dst])
            };
            b[..n].copy_from_slice(&a[..n]);
        }
        Ok(Some(dst))
    }
    fn hessian_slab(&mut self, z0: usize, z1: usize) -> Result<HessianSlab, FilterError> {
        let slices = z1 - z0;
        let n = slices * self.nxy;
        let mut zd: [Comp; 3] = [None; 3];
        if self.nz == 1 {
            self.bufs[0][..n].copy_from_slice(&self.src.data[..n]);
            zd[0] = Some(0);
        } else {
            for (order, z) in zd.iter_mut().enumerate() {
                self.z_pass(order, order, z0, z1)?;
                *z = Some(order);
            }
        }
        let zz = self.component(4, zd[2], 0, 0, slices)?;
        let xz = self.component(2, zd[1], 0, 1, slices)?;
        let yz = self.component(5, zd[1], 1, 0, slices)?;
        let xx = self.component(1, zd[0], 0, 2, slices)?;
        let xy = self.component(6, zd[0], 1, 1, slices)?;
        let yy = self.component(0, zd[0], 2, 0, slices)?;
        ctx_err(self.ctx)?;
        Ok(HessianSlab {
            xx,
            xy,
            xz,
            yy,
            yz,
            zz,
            n,
        })
    }
    fn at(&self, h: &HessianSlab, i: usize) -> [f64; 6] {
        let get = |c: Comp| c.map_or(0.0, |b| f64::from(self.bufs[b][i]));
        [
            get(h.xx),
            get(h.xy),
            get(h.xz),
            get(h.yy),
            get(h.yz),
            get(h.zz),
        ]
    }
    fn max_norm(&self, h: &HessianSlab, n: usize) -> Result<f64, FilterError> {
        const CHUNK: usize = 1 << 14;
        let _ = h.n;
        let mut best = 0.0;
        for c in 0..n.div_ceil(CHUNK) {
            ctx_err(self.ctx)?;
            let (lo, hi) = (c * CHUNK, n.min((c + 1) * CHUNK));
            let mut part = 0.0;
            for i in lo..hi {
                let [xx, xy, xz, yy, yz, zz] = self.at(h, i);
                part = fmath::max(
                    part,
                    xx * xx + yy * yy + zz * zz + 2.0 * (xy * xy + xz * xz + yz * yz),
                );
            }
            best = fmath::max(best, part);
        }
        Ok(best.sqrt())
    }
    fn measure_slab(
        &self,
        h: &HessianSlab,
        z0: usize,
        z1: usize,
        gamma: f64,
        sigma: f64,
        res: &mut VesselnessResult,
    ) -> Result<(), FilterError> {
        const CHUNK: usize = 1 << 14;
        let n = (z1 - z0) * self.nxy;
        let offset = z0 * self.nxy;
        let out = &mut res.response.data[offset..offset + n];
        let mut scales = res
            .scale_mm
            .as_mut()
            .map(|s| &mut s.data[offset..offset + n]);
        for c in 0..n.div_ceil(CHUNK) {
            ctx_err(self.ctx)?;
            let (lo, hi) = (c * CHUNK, n.min((c + 1) * CHUNK));
            for i in lo..hi {
                let mut e = self.at(h, i);
                if self.dark {
                    for v in e.iter_mut() {
                        *v = -*v;
                    }
                }
                let [xx, xy, xz, yy, yz, zz] = e;
                let v = self.voxel_measure(xx, xy, xz, yy, yz, zz, gamma);
                let f = v as f32;
                if f > out[i] {
                    out[i] = f;
                    if let Some(s) = scales.as_deref_mut() {
                        s[i] = sigma as f32;
                    }
                }
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn voxel_measure(
        &self,
        xx: f64,
        xy: f64,
        xz: f64,
        yy: f64,
        yz: f64,
        zz: f64,
        gamma: f64,
    ) -> f64 {
        if self.measure == VesselnessMeasure::SATO {
            let e = linalg::symmetric_eigenvalues3(xx, xy, xz, yy, yz, zz);
            return sato_line_measure(e, self.alpha1, self.alpha2);
        }
        if self.object_dim <= 1 && xx + yy + zz > 0.0 {
            return 0.0;
        }
        let mut sorted = [0f64; 3];
        if self.image_dim == 2 {
            let e = linalg::symmetric_eigenvalues2(xx, xy, yy);
            sorted[0] = e[0];
            sorted[1] = e[1];
            if sorted[0].abs() > sorted[1].abs() {
                sorted.swap(0, 1);
            }
        } else {
            sorted = sort_by_magnitude(linalg::symmetric_eigenvalues3(xx, xy, xz, yy, yz, zz));
        }
        let mut abs = [0f64; 3];
        for i in 0..self.image_dim {
            abs[i] = sorted[i].abs();
        }
        objectness(
            &sorted[..self.image_dim],
            &abs[..self.image_dim],
            self.object_dim,
            self.alpha,
            self.beta,
            gamma,
            self.scale_by_l3,
        )
    }
}
pub(crate) fn objectness(
    sorted: &[f64],
    abs: &[f64],
    m: usize,
    alpha: f64,
    beta: f64,
    gamma: f64,
    scale_by_largest: bool,
) -> f64 {
    let n = sorted.len();
    if sorted[m..].iter().any(|v| *v > 0.0) {
        return 0.0;
    }
    let mut v = 1.0;
    if m + 1 < n {
        let mut den = 1.0;
        for a in &abs[m + 1..n] {
            den *= a;
        }
        if !(den > 0.0) {
            return 0.0;
        }
        let r_a = abs[m] / root(den, n - m - 1);
        v *= 1.0 - fmath::exp(-0.5 * r_a * r_a / (alpha * alpha));
    }
    if m > 0 {
        let mut den = 1.0;
        for a in &abs[m..n] {
            den *= a;
        }
        if !(den > 0.0) {
            return 0.0;
        }
        let r_b = abs[m - 1] / root(den, n - m);
        v *= fmath::exp(-0.5 * r_b * r_b / (beta * beta));
    }
    if gamma > 0.0 {
        let mut s2 = 0.0;
        for a in abs {
            s2 += a * a;
        }
        v *= 1.0 - fmath::exp(-0.5 * s2 / (gamma * gamma));
    }
    if scale_by_largest {
        v *= abs[n - 1];
    }
    v
}
fn root(x: f64, k: usize) -> f64 {
    match k {
        1 => x,
        2 => x.sqrt(),
        _ => fmath::cbrt(x),
    }
}
fn sato_line_measure(e: [f64; 3], alpha1: f64, alpha2: f64) -> f64 {
    let normalize = fmath::min(-e[1], -e[0]);
    if !(normalize > 0.0) {
        return 0.0;
    }
    let a = if e[2] <= 0.0 { alpha1 } else { alpha2 };
    let r = e[2] / (a * normalize);
    fmath::exp(-0.5 * r * r) * normalize
}
fn sort_by_magnitude(mut e: [f64; 3]) -> [f64; 3] {
    if e[0].abs() > e[1].abs() {
        e.swap(0, 1);
    }
    if e[1].abs() > e[2].abs() {
        e.swap(1, 2);
    }
    if e[0].abs() > e[1].abs() {
        e.swap(0, 1);
    }
    e
}
