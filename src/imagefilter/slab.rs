use super::volume::{Ctx, FilterError, Volume, ctx_err, dims_v, voxel_count};
use crate::fmath;
use crate::textfmt::float_v;
pub const DEFAULT_SLAB_WORKING_BYTES: i64 = 256 << 20;
pub trait SliceReader {
    fn read_slices(
        &self,
        ctx: Ctx<'_>,
        z0: i64,
        z1: i64,
        dst: &mut [f32],
    ) -> Result<(), FilterError>;
}
pub trait SliceWriter {
    fn write_slices(&mut self, ctx: Ctx<'_>, z0: i64, src: &[f32]) -> Result<(), FilterError>;
}
impl SliceReader for Volume {
    fn read_slices(
        &self,
        _ctx: Ctx<'_>,
        z0: i64,
        z1: i64,
        dst: &mut [f32],
    ) -> Result<(), FilterError> {
        let nxy = self.dims[0].wrapping_mul(self.dims[1]);
        if z0 < 0 || z1 > self.dims[2] || z0 > z1 || dst.len() as i64 != (z1 - z0).wrapping_mul(nxy)
        {
            return Err(FilterError::invalid(format!(
                "slices [{z0}, {z1}) into {} samples of a {} volume",
                dst.len(),
                dims_v(self.dims)
            )));
        }
        let start = (z0 * nxy) as usize;
        dst.copy_from_slice(&self.data[start..start + dst.len()]);
        Ok(())
    }
}
impl SliceWriter for Volume {
    fn write_slices(&mut self, _ctx: Ctx<'_>, z0: i64, src: &[f32]) -> Result<(), FilterError> {
        let nxy = self.dims[0].wrapping_mul(self.dims[1]);
        let n = src.len() as i64;
        if nxy == 0 || n % nxy != 0 || z0 < 0 || z0 + n / nxy > self.dims[2] {
            return Err(FilterError::invalid(format!(
                "{} samples at slice {z0} of a {} volume",
                src.len(),
                dims_v(self.dims)
            )));
        }
        let start = (z0 * nxy) as usize;
        self.data[start..start + src.len()].copy_from_slice(src);
        Ok(())
    }
}
pub trait SlabKernel {
    fn halo(&self) -> i64;
    fn scratch_bytes(&self, slices: i64) -> i64;
    fn run(
        &mut self,
        ctx: Ctx<'_>,
        input: &SlabView<'_>,
        z0: usize,
        z1: usize,
        dst: &mut [f32],
    ) -> Result<(), FilterError>;
}
pub trait SlabFilter {
    fn slab_kernel(
        &self,
        dims: [i64; 3],
        spacing_mm: [f64; 3],
    ) -> Result<Box<dyn SlabKernel>, FilterError>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlabOptions {
    pub max_working_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlabResult {
    pub slab_slices: i64,
    pub halo_slices: i64,
    pub slabs: i64,
    pub working_bytes: i64,
}
pub fn filter_slabs(
    ctx: Ctx<'_>,
    f: Option<&dyn SlabFilter>,
    src: Option<&dyn SliceReader>,
    dims: [i64; 3],
    spacing_mm: [f64; 3],
    dst: Option<&mut dyn SliceWriter>,
    opts: SlabOptions,
) -> (SlabResult, Result<(), FilterError>) {
    let mut res = SlabResult::default();
    let (Some(f), Some(src), Some(dst)) = (f, src, dst) else {
        return (
            res,
            Err(FilterError::invalid("nil filter, source or destination")),
        );
    };
    if voxel_count(dims).is_none() {
        return (
            res,
            Err(FilterError::invalid(format!("dimensions {}", dims_v(dims)))),
        );
    }
    for (axis, s) in spacing_mm.iter().enumerate() {
        if dims[axis] > 1 && !(*s > 0.0 && *s <= f64::MAX) {
            return (
                res,
                Err(FilterError::invalid(format!(
                    "spacing {} mm along axis {axis}",
                    float_v(*s)
                ))),
            );
        }
    }
    let mut k = match f.slab_kernel(dims, spacing_mm) {
        Ok(k) => k,
        Err(e) => return (res, Err(e)),
    };
    let mut budget = opts.max_working_bytes;
    if budget < 0 {
        return (
            res,
            Err(FilterError::invalid(format!(
                "working bound {budget} bytes"
            ))),
        );
    }
    if budget == 0 {
        budget = DEFAULT_SLAB_WORKING_BYTES;
    }
    let (nz, nxy) = (dims[2], dims[0] * dims[1]);
    let halo = k.halo().min(nz);
    let slice_bytes = nxy * 4;
    let cost = |k: &dyn SlabKernel, slices: i64| -> i64 {
        let window = nz.min(slices + 2 * halo);
        (window + slices)
            .wrapping_mul(slice_bytes)
            .wrapping_add(k.scratch_bytes(slices))
    };
    let mut slab = 1;
    let (mut lo, mut hi) = (1i64, nz);
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if cost(k.as_ref(), mid) <= budget {
            slab = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    res.slab_slices = slab;
    res.halo_slices = halo;
    res.working_bytes = cost(k.as_ref(), slab);
    if let Err(e) = ctx_err(ctx) {
        return (res, Err(e));
    }
    let nxy = nxy as usize;
    let (nz, slab, halo) = (nz as usize, slab as usize, halo as usize);
    let mut window = vec![0f32; nz.min(slab + 2 * halo) * nxy];
    let mut out = vec![0f32; slab * nxy];
    let (mut held0, mut held1) = (0usize, 0usize);
    let mut z0 = 0;
    while z0 < nz {
        let z1 = nz.min(z0 + slab);
        let (lo, hi) = (z0.saturating_sub(halo), nz.min(z1 + halo));
        if lo >= held0 && lo < held1 {
            window.copy_within((lo - held0) * nxy..(held1 - held0) * nxy, 0);
        } else {
            held1 = lo;
        }
        if hi > held1
            && let Err(e) = src.read_slices(
                ctx,
                held1 as i64,
                hi as i64,
                &mut window[(held1 - lo) * nxy..(hi - lo) * nxy],
            )
        {
            return (res, Err(e));
        }
        held0 = lo;
        held1 = hi;
        let view = SlabView::new(&window[..(hi - lo) * nxy], dims, lo, hi);
        let part = &mut out[..(z1 - z0) * nxy];
        if let Err(e) = k.run(ctx, &view, z0, z1, part) {
            return (res, Err(e));
        }
        if let Err(e) = dst.write_slices(ctx, z0 as i64, part) {
            return (res, Err(e));
        }
        res.slabs += 1;
        z0 += slab;
    }
    (res, ctx_err(ctx))
}
pub(crate) fn worker_bound(requested: i64) -> i64 {
    crate::render::parallel::workers(requested)
}
pub(crate) fn worker_count(requested: i64, lines: i64) -> i64 {
    1.max(worker_bound(requested).min(lines))
}
#[derive(Debug, Clone, Copy)]
pub struct SlabView<'a> {
    pub(crate) data: &'a [f32],
    pub(crate) dims: [usize; 3],
    pub(crate) lo: usize,
    #[allow(dead_code)]
    pub(crate) hi: usize,
    pub(crate) region_origin: [usize; 3],
    pub(crate) region_size: [usize; 3],
}
impl<'a> SlabView<'a> {
    pub(crate) fn new(data: &'a [f32], dims: [i64; 3], lo: usize, hi: usize) -> SlabView<'a> {
        SlabView {
            data,
            dims: [dims[0] as usize, dims[1] as usize, dims[2] as usize],
            lo,
            hi,
            region_origin: [0; 3],
            region_size: [0; 3],
        }
    }
    pub(crate) fn whole(v: &'a Volume) -> SlabView<'a> {
        SlabView::new(&v.data, v.dims, 0, v.dims[2] as usize)
    }
    pub(crate) fn row(&self, y: usize, z: usize) -> usize {
        if self.region_size[0] != 0 {
            let (o, s) = (self.region_origin, self.region_size);
            return z
                .wrapping_sub(o[2])
                .wrapping_mul(s[1])
                .wrapping_add(y)
                .wrapping_sub(o[1])
                .wrapping_mul(s[0])
                .wrapping_sub(o[0]);
        }
        ((z - self.lo) * self.dims[1] + y) * self.dims[0]
    }
    pub(crate) fn at(&self, x: usize, y: usize, z: usize) -> f32 {
        self.data[self.row(y, z).wrapping_add(x)]
    }
}
pub(crate) fn clamp_axis(c: i64, n: usize) -> usize {
    if c < 0 {
        0
    } else if c as usize >= n {
        n - 1
    } else {
        c as usize
    }
}
pub(crate) fn radius_samples(
    name: &str,
    radius_mm: [f64; 3],
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> Result<[i64; 3], FilterError> {
    let (r, e) = radius_samples_partial(name, radius_mm, dims, spacing_mm);
    match e {
        Some(e) => Err(e),
        None => Ok(r),
    }
}
pub(crate) fn radius_samples_partial(
    name: &str,
    radius_mm: [f64; 3],
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> ([i64; 3], Option<FilterError>) {
    let mut r = [0i64; 3];
    for (axis, mm) in radius_mm.iter().copied().enumerate() {
        if !(mm >= 0.0) || mm.is_infinite() {
            let e = FilterError::invalid(format!(
                "{name} radius {} mm along axis {axis}",
                float_v(mm)
            ));
            return (r, Some(e));
        }
        if dims[axis] == 1 || mm == 0.0 {
            continue;
        }
        let mut samples = (mm / spacing_mm[axis] + 0.5).floor();
        if samples > dims[axis] as f64 {
            samples = dims[axis] as f64;
        }
        r[axis] = fmath::f64_to_i64(samples);
    }
    (r, None)
}
