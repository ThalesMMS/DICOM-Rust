use super::slab::{
    SlabFilter, SlabKernel, SlabView, clamp_axis, radius_samples, worker_bound, worker_count,
};
use super::volume::{Ctx, FilterError, Volume, ctx_err};
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MedianOptions {
    pub radius_mm: [f64; 3],
    pub max_kernel_samples: i64,
    pub workers: i64,
}
pub fn median_radius_samples(
    opts: &MedianOptions,
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> Result<[i64; 3], FilterError> {
    radius_samples("median", opts.radius_mm, dims, spacing_mm)
}
#[doc(hidden)]
pub fn median_radius_samples_partial(
    opts: &MedianOptions,
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> ([i64; 3], Result<(), FilterError>) {
    let (r, e) = super::slab::radius_samples_partial("median", opts.radius_mm, dims, spacing_mm);
    (r, e.map_or(Ok(()), Err))
}
pub fn median(ctx: Ctx<'_>, src: &Volume, opts: &MedianOptions) -> Result<Volume, FilterError> {
    src.validate()?;
    let mut k = MedianKernel::new(src.dims, src.spacing_mm, opts)?;
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
struct MedianKernel {
    dims: [usize; 3],
    radius: [usize; 3],
    workers: i64,
}
const MEDIAN_HISTOGRAM_BINS: usize = 1 << 16;
impl MedianKernel {
    fn new(
        dims: [i64; 3],
        spacing_mm: [f64; 3],
        opts: &MedianOptions,
    ) -> Result<MedianKernel, FilterError> {
        let radius = median_radius_samples(opts, dims, spacing_mm)?;
        let mut limit = opts.max_kernel_samples;
        if limit == 0 {
            limit = 1 << 16;
        }
        if limit < 0 {
            return Err(FilterError::invalid(format!("median kernel bound {limit}")));
        }
        let mut samples: i64 = 1;
        for r in radius {
            samples = samples.wrapping_mul(2 * r + 1);
            if samples > limit {
                return Err(FilterError::invalid(format!(
                    "the median box exceeds {limit} samples"
                )));
            }
        }
        Ok(MedianKernel {
            dims: [dims[0] as usize, dims[1] as usize, dims[2] as usize],
            radius: radius.map(|r| r as usize),
            workers: opts.workers,
        })
    }
}
impl SlabFilter for MedianOptions {
    fn slab_kernel(
        &self,
        dims: [i64; 3],
        spacing_mm: [f64; 3],
    ) -> Result<Box<dyn SlabKernel>, FilterError> {
        Ok(Box::new(MedianKernel::new(dims, spacing_mm, self)?))
    }
}
impl SlabKernel for MedianKernel {
    fn halo(&self) -> i64 {
        self.radius[2] as i64
    }
    fn scratch_bytes(&self, slices: i64) -> i64 {
        let r = self.radius.map(|r| r as i64);
        let column_samples = (2 * r[1] + 1) * (2 * r[2] + 1);
        let bx = (2 * r[0] + 1) * column_samples * 4;
        let hist = ((MEDIAN_HISTOGRAM_BINS + (MEDIAN_HISTOGRAM_BINS >> 8)) * 4) as i64;
        let per_worker = column_samples * 8 + bx.max(hist);
        worker_bound(self.workers).min(1.max(slices * self.dims[1] as i64)) * per_worker
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
        let lines = (z1 - z0) * ny;
        let _ = worker_count(self.workers, lines as i64);
        let (lo, span, integral) = integer_span(input.data);
        let use_histogram = integral && span <= MEDIAN_HISTOGRAM_BINS;
        let [rx, ry, rz] = self.radius;
        let column_samples = (2 * ry + 1) * (2 * rz + 1);
        let box_samples = (2 * rx + 1) * column_samples;
        let rank = box_samples / 2;
        let mut rows = vec![0usize; column_samples];
        let mut hist: Option<MedianHistogram> = None;
        let mut bx = Vec::new();
        for line in 0..lines {
            ctx_err(ctx)?;
            let (y, z) = (line % ny, z0 + line / ny);
            let mut i = 0;
            for dz in -(rz as i64)..=rz as i64 {
                let cz = clamp_axis(z as i64 + dz, self.dims[2]);
                for dy in -(ry as i64)..=ry as i64 {
                    rows[i] = input.row(clamp_axis(y as i64 + dy, ny), cz);
                    i += 1;
                }
            }
            let out = &mut dst[line * nx..(line + 1) * nx];
            if use_histogram {
                let h = hist.get_or_insert_with(|| MedianHistogram::new(span));
                h.line(input.data, &rows, out, rx, lo, rank);
                continue;
            }
            if bx.len() != box_samples {
                bx = vec![0f32; box_samples];
            }
            for (x, o) in out.iter_mut().enumerate() {
                let mut n = 0;
                for dx in -(rx as i64)..=rx as i64 {
                    let cx = clamp_axis(x as i64 + dx, nx);
                    for r in &rows {
                        bx[n] = input.data[r.wrapping_add(cx)];
                        n += 1;
                    }
                }
                *o = select_rank(&mut bx, rank);
            }
        }
        ctx_err(ctx)
    }
}
pub(crate) fn integer_span(data: &[f32]) -> (f32, usize, bool) {
    if data.is_empty() {
        return (0.0, 0, false);
    }
    let (mut lo, mut hi) = (data[0], data[0]);
    for v in data {
        let v = *v;
        if v != f64::from(v).trunc() as f32 || v >= (1 << 24) as f32 || v <= -((1 << 24) as f32) {
            return (0.0, 0, false);
        }
        lo = min_nan32(lo, v);
        hi = max_nan32(hi, v);
    }
    (lo, (hi - lo) as i64 as usize + 1, true)
}
pub(crate) fn min_nan32(a: f32, b: f32) -> f32 {
    min_nan64(f64::from(a), f64::from(b)) as f32
}
pub(crate) fn max_nan32(a: f32, b: f32) -> f32 {
    crate::fmath::max(f64::from(a), f64::from(b)) as f32
}
fn min_nan64(a: f64, b: f64) -> f64 {
    crate::fmath::min(a, b)
}
pub(super) struct MedianHistogram {
    pub(super) fine: Vec<i32>,
    pub(super) coarse: Vec<i32>,
    pub(super) med: usize,
    pub(super) below: i64,
}
impl MedianHistogram {
    pub(super) fn new(span: usize) -> MedianHistogram {
        let coarse = span.div_ceil(256);
        MedianHistogram {
            fine: vec![0; coarse << 8],
            coarse: vec![0; coarse],
            med: 0,
            below: 0,
        }
    }
    fn column(&mut self, data: &[f32], rows: &[usize], cx: usize, lo: f32, delta: i32) {
        for r in rows {
            let b = (data[r.wrapping_add(cx)] - lo) as i64 as usize;
            self.fine[b] += delta;
            self.coarse[b >> 8] += delta;
            if b < self.med {
                self.below += i64::from(delta);
            }
        }
    }
    pub(super) fn median(&mut self, rank: i64) -> usize {
        while self.below > rank {
            if self.med & 255 == 0 {
                let b = (self.med >> 8) - 1;
                if self.below - i64::from(self.coarse[b]) > rank {
                    self.below -= i64::from(self.coarse[b]);
                    self.med -= 256;
                    continue;
                }
            }
            self.med -= 1;
            self.below -= i64::from(self.fine[self.med]);
        }
        loop {
            if self.med & 255 == 0 {
                let c = i64::from(self.coarse[self.med >> 8]);
                if self.below + c <= rank {
                    self.below += c;
                    self.med += 256;
                    continue;
                }
            }
            let f = i64::from(self.fine[self.med]);
            if self.below + f > rank {
                return self.med;
            }
            self.below += f;
            self.med += 1;
        }
    }
    fn line(
        &mut self,
        data: &[f32],
        rows: &[usize],
        out: &mut [f32],
        rx: usize,
        lo: f32,
        rank: usize,
    ) {
        let nx = out.len();
        let rx = rx as i64;
        for dx in -rx..=rx {
            self.column(data, rows, clamp_axis(dx, nx), lo, 1);
        }
        for x in 0..nx {
            out[x] = lo + self.median(rank as i64) as f32;
            if x + 1 < nx {
                self.column(data, rows, clamp_axis(x as i64 - rx, nx), lo, -1);
                self.column(data, rows, clamp_axis(x as i64 + rx + 1, nx), lo, 1);
            }
        }
        for dx in -rx..=rx {
            self.column(data, rows, clamp_axis(nx as i64 - 1 + dx, nx), lo, -1);
        }
        self.med = 0;
        self.below = 0;
    }
}
pub(crate) fn select_rank(v: &mut [f32], rank: usize) -> f32 {
    let (mut lo, mut hi) = (0i64, v.len() as i64 - 1);
    let r = rank as i64;
    while lo < hi {
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
        if r <= j {
            hi = j;
        } else if r >= i {
            lo = i;
        } else {
            return v[rank];
        }
    }
    v[rank]
}
