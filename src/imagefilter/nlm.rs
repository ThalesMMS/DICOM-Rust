use super::slab::{SlabFilter, SlabKernel, SlabView, clamp_axis, radius_samples, worker_bound};
use super::volume::{Ctx, FilterError, Volume, ctx_err};
use crate::fmath;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NonLocalMeansOptions {
    pub noise_sigma: f64,
    pub beta: f64,
    pub patch_radius_mm: [f64; 3],
    pub search_radius_mm: [f64; 3],
    pub block_step: i64,
    pub max_search_samples: i64,
    pub workers: i64,
}
pub fn non_local_means(
    ctx: Ctx<'_>,
    src: &Volume,
    opts: &NonLocalMeansOptions,
) -> Result<Volume, FilterError> {
    src.validate()?;
    let mut k = NlmKernel::new(src.dims, src.spacing_mm, opts)?;
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
pub(super) const NLM_CUTOFF: f64 = 18.0;
pub(super) struct NlmKernel {
    pub(super) dims: [usize; 3],
    pub(super) patch: [usize; 3],
    pub(super) search: [usize; 3],
    pub(super) cutoff: f64,
    pub(super) inv_h2: f64,
    pub(super) workers: i64,
    pub(super) centres: [Vec<usize>; 3],
    pub(super) cover_lo: [Vec<usize>; 3],
    pub(super) cover_hi: [Vec<usize>; 3],
    pub(super) block_len: usize,
    pub(super) row_offsets: Vec<i64>,
    pub(super) ring: usize,
    pub(super) planes: Vec<Vec<f32>>,
    pub(super) patch_buf: Vec<f32>,
    pub(super) other: Vec<f32>,
    pub(super) patch64: Vec<f64>,
    pub(super) acc: Vec<f64>,
}
pub(super) fn radius_or_default(
    name: &str,
    mm: [f64; 3],
    def: i64,
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> Result<[i64; 3], FilterError> {
    if mm.iter().all(|v| *v == 0.0) {
        let mut r = [0i64; 3];
        for (axis, ra) in r.iter_mut().enumerate() {
            if dims[axis] > 1 {
                *ra = def.min(dims[axis]);
            }
        }
        return Ok(r);
    }
    radius_samples(name, mm, dims, spacing_mm)
}
fn block_cover(n: usize, radius: usize, step: usize) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let mut centres = Vec::new();
    let mut c = 0;
    loop {
        if c + 1 >= n {
            centres.push(n - 1);
            break;
        }
        centres.push(c);
        c += step;
    }
    let (mut lo, mut hi) = (vec![0; n], vec![0; n]);
    let mut first = 0;
    for p in 0..n {
        while centres[first] + radius < p {
            first += 1;
        }
        let mut last = first;
        while last + 1 < centres.len() && centres[last + 1] <= p + radius {
            last += 1;
        }
        lo[p] = first;
        hi[p] = last;
    }
    (centres, lo, hi)
}
impl NlmKernel {
    fn new(
        dims: [i64; 3],
        spacing_mm: [f64; 3],
        opts: &NonLocalMeansOptions,
    ) -> Result<NlmKernel, FilterError> {
        if !(opts.noise_sigma > 0.0) || opts.noise_sigma.is_infinite() {
            return Err(FilterError::invalid(format!(
                "noise sigma {}",
                float_v(opts.noise_sigma)
            )));
        }
        let beta = if opts.beta == 0.0 { 1.0 } else { opts.beta };
        if !(beta > 0.0) || beta.is_infinite() {
            return Err(FilterError::invalid(format!("beta {}", float_v(beta))));
        }
        let block_step = if opts.block_step == 0 {
            2
        } else {
            opts.block_step
        };
        if block_step < 0 {
            return Err(FilterError::invalid(format!("block step {block_step}")));
        }
        let max_search = if opts.max_search_samples == 0 {
            1 << 16
        } else {
            opts.max_search_samples
        };
        let patch = radius_or_default("patch", opts.patch_radius_mm, 1, dims, spacing_mm)?;
        let search = radius_or_default("search", opts.search_radius_mm, 5, dims, spacing_mm)?;
        let udims = [dims[0] as usize, dims[1] as usize, dims[2] as usize];
        let mut k = NlmKernel {
            dims: udims,
            patch: patch.map(|r| r as usize),
            search: search.map(|r| r as usize),
            cutoff: 0.0,
            inv_h2: 0.0,
            workers: opts.workers,
            centres: Default::default(),
            cover_lo: Default::default(),
            cover_hi: Default::default(),
            block_len: 0,
            row_offsets: Vec::new(),
            ring: 0,
            planes: Vec::new(),
            patch_buf: Vec::new(),
            other: Vec::new(),
            patch64: Vec::new(),
            acc: Vec::new(),
        };
        let (mut search_samples, mut block_len) = (1i64, 1i64);
        for axis in 0..3 {
            search_samples = search_samples.wrapping_mul(2 * search[axis] + 1);
            block_len = block_len.wrapping_mul(2 * patch[axis] + 1);
            if search_samples > max_search {
                return Err(FilterError::invalid(format!(
                    "the search window exceeds {max_search} samples"
                )));
            }
            let step = block_step.min(2 * patch[axis] + 1) as usize;
            let (c, lo, hi) = block_cover(udims[axis], k.patch[axis], step);
            k.centres[axis] = c;
            k.cover_lo[axis] = lo;
            k.cover_hi[axis] = hi;
        }
        k.block_len = block_len as usize;
        for dz in -patch[2]..=patch[2] {
            for dy in -patch[1]..=patch[1] {
                k.row_offsets.push((dz * dims[1] + dy) * dims[0]);
            }
        }
        let h2 = 2.0 * beta * opts.noise_sigma * opts.noise_sigma;
        if !(h2 > 0.0) || h2.is_infinite() {
            return Err(FilterError::invalid(format!(
                "noise sigma {} and beta {}",
                float_v(opts.noise_sigma),
                float_v(beta)
            )));
        }
        k.inv_h2 = 1.0 / h2;
        k.cutoff = NLM_CUTOFF * h2;
        for z in 0..k.cover_lo[2].len() {
            k.ring = k.ring.max(k.cover_hi[2][z] - k.cover_lo[2][z] + 1);
        }
        Ok(k)
    }
    fn average(&self, out: &mut [f32], y: usize, z: usize) {
        let nbx = self.centres[0].len();
        let (bx, by) = (2 * self.patch[0] + 1, 2 * self.patch[1] + 1);
        let (z_lo, z_hi) = (self.cover_lo[2][z], self.cover_hi[2][z]);
        let (y_lo, y_hi) = (self.cover_lo[1][y], self.cover_hi[1][y]);
        for (x, o) in out.iter_mut().enumerate() {
            let (x_lo, x_hi) = (self.cover_lo[0][x], self.cover_hi[0][x]);
            let mut sum = 0.0;
            for kz in z_lo..=z_hi {
                let plane = &self.planes[kz % self.ring];
                let oz = z + self.patch[2] - self.centres[2][kz];
                for ky in y_lo..=y_hi {
                    let oy = y + self.patch[1] - self.centres[1][ky];
                    let row = ky * nbx;
                    for kx in x_lo..=x_hi {
                        let ox = x + self.patch[0] - self.centres[0][kx];
                        sum += f64::from(
                            plane[(row + kx) * self.block_len + (oz * by + oy) * bx + ox],
                        );
                    }
                }
            }
            let count = (z_hi - z_lo + 1) * (y_hi - y_lo + 1) * (x_hi - x_lo + 1);
            *o = (sum / count as f64) as f32;
        }
    }
    fn gather(&self, input: &SlabView<'_>, dst: &mut [f32], cx: usize, cy: usize, cz: usize) {
        let [nx, ny, nz] = self.dims;
        let [rx, ry, rz] = self.patch.map(|r| r as i64);
        let mut i = 0;
        for dz in -rz..=rz {
            let z = clamp_axis(cz as i64 + dz, nz);
            for dy in -ry..=ry {
                let row = input.row(clamp_axis(cy as i64 + dy, ny), z);
                for dx in -rx..=rx {
                    dst[i] = input.data[row.wrapping_add(clamp_axis(cx as i64 + dx, nx))];
                    i += 1;
                }
            }
        }
    }
    pub(super) fn block_row(
        &mut self,
        input: &SlabView<'_>,
        plane: &mut [f32],
        cy: usize,
        cz: usize,
    ) {
        let [nx, ny, nz] = self.dims;
        let bl = self.block_len;
        if self.patch_buf.len() != bl {
            self.patch_buf = vec![0.0; bl];
            self.patch64 = vec![0.0; bl];
            self.other = vec![0.0; bl];
            self.acc = vec![0.0; bl];
        }
        let [rx, ry, rz] = self.patch;
        let [sx, sy, sz] = self.search.map(|s| s as i64);
        let width = 2 * rx + 1;
        let limit = self.cutoff * bl as f64;
        let mut patch_buf = std::mem::take(&mut self.patch_buf);
        let mut other = std::mem::take(&mut self.other);
        for kx in 0..self.centres[0].len() {
            let cx = self.centres[0][kx];
            self.gather(input, &mut patch_buf, cx, cy, cz);
            for (i, v) in patch_buf.iter().enumerate() {
                self.patch64[i] = f64::from(*v);
                self.acc[i] = 0.0;
            }
            let (mut weights, mut best) = (0.0f64, 0.0f64);
            let (cx, cy, cz) = (cx as i64, cy as i64, cz as i64);
            for oz in (-sz).max(-cz)..=sz.min(nz as i64 - 1 - cz) {
                let qz = cz + oz;
                for oy in (-sy).max(-cy)..=sy.min(ny as i64 - 1 - cy) {
                    let qy = cy + oy;
                    for ox in (-sx).max(-cx)..=sx.min(nx as i64 - 1 - cx) {
                        if ox == 0 && oy == 0 && oz == 0 {
                            continue;
                        }
                        let qx = cx + ox;
                        let (rxi, ryi, rzi) = (rx as i64, ry as i64, rz as i64);
                        let interior = qx >= rxi
                            && qx + rxi < nx as i64
                            && qy >= ryi
                            && qy + ryi < ny as i64
                            && qz >= rzi
                            && qz + rzi < nz as i64;
                        let mut base = 0usize;
                        if interior {
                            base = input
                                .row(qy as usize, qz as usize)
                                .wrapping_add((qx - rxi) as usize);
                        } else {
                            self.gather(input, &mut other, qx as usize, qy as usize, qz as usize);
                        }
                        let seg = |r: usize, off: i64| -> &[f32] {
                            if interior {
                                let s = base.wrapping_add(off as usize);
                                &input.data[s..s + width]
                            } else {
                                &other[r * width..(r + 1) * width]
                            }
                        };
                        let mut d = 0.0;
                        for (r, off) in self.row_offsets.iter().enumerate() {
                            let p = &self.patch64[r * width..(r + 1) * width];
                            for (j, v) in seg(r, *off).iter().enumerate() {
                                let diff = p[j] - f64::from(*v);
                                d += diff * diff;
                            }
                            if d > limit {
                                break;
                            }
                        }
                        if d > limit {
                            continue;
                        }
                        let w = fmath::exp(-d / bl as f64 * self.inv_h2);
                        best = fmath::max(best, w);
                        weights += w;
                        for (r, off) in self.row_offsets.iter().enumerate() {
                            let acc = &mut self.acc[r * width..(r + 1) * width];
                            for (j, v) in seg(r, *off).iter().enumerate() {
                                acc[j] += w * f64::from(*v);
                            }
                        }
                    }
                }
            }
            if best == 0.0 {
                best = 1.0;
            }
            weights += best;
            let block = &mut plane[kx * bl..(kx + 1) * bl];
            for (i, v) in self.patch64.iter().enumerate() {
                block[i] = ((self.acc[i] + best * v) / weights) as f32;
            }
        }
        self.patch_buf = patch_buf;
        self.other = other;
    }
}
impl SlabFilter for NonLocalMeansOptions {
    fn slab_kernel(
        &self,
        dims: [i64; 3],
        spacing_mm: [f64; 3],
    ) -> Result<Box<dyn SlabKernel>, FilterError> {
        Ok(Box::new(NlmKernel::new(dims, spacing_mm, self)?))
    }
}
impl SlabKernel for NlmKernel {
    fn halo(&self) -> i64 {
        (2 * self.patch[2] + self.search[2]) as i64
    }
    fn scratch_bytes(&self, _slices: i64) -> i64 {
        let plane =
            self.centres[0].len() as i64 * self.centres[1].len() as i64 * self.block_len as i64 * 4;
        let per_worker = self.block_len as i64 * (4 + 4 + 8);
        self.ring as i64 * plane
            + worker_bound(self.workers).min(self.centres[1].len() as i64) * per_worker
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
        let (nbx, nby) = (self.centres[0].len(), self.centres[1].len());
        let plane_len = nbx * nby * self.block_len;
        if self.planes.len() != self.ring {
            self.planes = (0..self.ring).map(|_| vec![0f32; plane_len]).collect();
        }
        let mut next = z0;
        for kz in self.cover_lo[2][z0]..=self.cover_hi[2][z1 - 1] {
            let mut plane = std::mem::take(&mut self.planes[kz % self.ring]);
            let cz = self.centres[2][kz];
            let span = nbx * self.block_len;
            for ky in 0..nby {
                if let Err(e) = ctx_err(ctx) {
                    self.planes[kz % self.ring] = plane;
                    return Err(e);
                }
                let cy = self.centres[1][ky];
                self.block_row(input, &mut plane[ky * span..(ky + 1) * span], cy, cz);
            }
            self.planes[kz % self.ring] = plane;
            ctx_err(ctx)?;
            while next < z1 && self.cover_hi[2][next] == kz {
                let z = next;
                let out = &mut dst[(z - z0) * nx * ny..(z - z0 + 1) * nx * ny];
                for y in 0..ny {
                    ctx_err(ctx)?;
                    self.average(&mut out[y * nx..(y + 1) * nx], y, z);
                }
                next += 1;
            }
        }
        ctx_err(ctx)
    }
}
