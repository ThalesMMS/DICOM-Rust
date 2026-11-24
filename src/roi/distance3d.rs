#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::pdqsort::sort_float64s;
use super::roimask::RasterMask;
use super::segmentation::Segmentation3D;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::textfmt::float_v;
pub const ERR_INVALID_VOLUME_MASK: &str = "roi: invalid volume mask";
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeMask {
    pub columns: i64,
    pub rows: i64,
    pub slices: i64,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    pub voxels: Vec<bool>,
}
impl VolumeMask {
    pub fn new(columns: i64, rows: i64, slices: i64, spacing: [f64; 3]) -> VolumeMask {
        let axis =
            |n: i64, step: f64| -> Vec<f64> { (0..n.max(0)).map(|i| i as f64 * step).collect() };
        let voxels = columns.wrapping_mul(rows).wrapping_mul(slices).max(0) as usize;
        VolumeMask {
            columns,
            rows,
            slices,
            x: axis(columns, spacing[0]),
            y: axis(rows, spacing[1]),
            z: axis(slices, spacing[2]),
            voxels: vec![false; voxels],
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.validate_context(None)
    }
    pub(crate) fn validate_context(&self, ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
        if self.columns <= 0
            || self.rows <= 0
            || self.slices <= 0
            || self.x.len() as i64 != self.columns
            || self.y.len() as i64 != self.rows
            || self.z.len() as i64 != self.slices
            || self.voxels.len() as i64
                != self
                    .columns
                    .wrapping_mul(self.rows)
                    .wrapping_mul(self.slices)
        {
            return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
        }
        for axis in [&self.x, &self.y, &self.z] {
            for (i, &v) in axis.iter().enumerate() {
                if i % 4096 == 0
                    && let Some(err) = ctx_err(ctx)
                {
                    return Err(ctx_err(ctx).unwrap_or(err));
                }
                if !v.is_finite() || i > 0 && v <= axis[i - 1] {
                    return Err(Error::wrap(
                        ERR_INVALID_VOLUME_MASK,
                        "coordinates must be finite and ascending",
                    ));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn index(&self, x: i64, y: i64, z: i64) -> usize {
        ((z * self.rows + y) * self.columns + x) as usize
    }
    pub fn count(&self) -> i64 {
        self.voxels.iter().filter(|v| **v).count() as i64
    }
    pub(crate) fn voxel_volume(&self, i: usize) -> f64 {
        let (columns, rows) = (self.columns as usize, self.rows as usize);
        let (x, y, z) = (i % columns, i / columns % rows, i / (columns * rows));
        axis_extent(&self.x, x) * axis_extent(&self.y, y) * axis_extent(&self.z, z)
    }
}
pub(crate) fn axis_extent(c: &[f64], i: usize) -> f64 {
    if c.len() == 1 {
        1.0
    } else if i == 0 {
        c[1] - c[0]
    } else if i == c.len() - 1 {
        c[i] - c[i - 1]
    } else {
        (c[i + 1] - c[i - 1]) / 2.0
    }
}
pub(crate) fn ctx_check(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}
pub(crate) fn ctx_poll(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(err) => Err(ctx_err(ctx).unwrap_or(err)),
        None => Ok(()),
    }
}
impl VolumeMask {
    pub fn distance_transform(
        &self,
        ctx: Option<&dyn Cancellation>,
        target: bool,
    ) -> Result<Vec<f64>, Error> {
        ctx_check(ctx)?;
        self.validate_context(ctx)?;
        let mut d = vec![0.0f64; self.voxels.len()];
        for (i, &v) in self.voxels.iter().enumerate() {
            if i % 4096 == 0 {
                ctx_poll(ctx)?;
            }
            d[i] = if v == target { 0.0 } else { f64::INFINITY };
        }
        let (nx, ny, nz) = (
            self.columns as usize,
            self.rows as usize,
            self.slices as usize,
        );
        for pass in 0..3 {
            let (n, lines, coords): (usize, usize, &[f64]) = match pass {
                0 => (nx, ny * nz, &self.x),
                1 => (ny, nx * nz, &self.y),
                _ => (nz, nx * ny, &self.z),
            };
            let at = |line: usize, k: usize| -> usize {
                match pass {
                    0 => line * nx + k,
                    1 => (line / nx * ny + k) * nx + line % nx,
                    _ => k * nx * ny + line,
                }
            };
            let mut f = vec![0.0f64; n];
            let mut out = vec![0.0f64; n];
            let mut hull = vec![0usize; n];
            let mut bounds = vec![0.0f64; n + 1];
            for line in 0..lines {
                ctx_check(ctx)?;
                for (k, value) in f.iter_mut().enumerate() {
                    if k % 4096 == 0 {
                        ctx_check(ctx)?;
                    }
                    *value = d[at(line, k)];
                }
                squared_distance_1d_context(ctx, &mut out, &f, coords, &mut hull, &mut bounds)?;
                for (k, value) in out.iter().enumerate() {
                    if k % 4096 == 0 {
                        ctx_check(ctx)?;
                    }
                    d[at(line, k)] = *value;
                }
            }
            ctx_check(ctx)?;
        }
        for (i, v) in d.iter_mut().enumerate() {
            if i % 4096 == 0 {
                ctx_poll(ctx)?;
            }
            *v = v.sqrt();
        }
        Ok(d)
    }
    pub fn expand(
        &self,
        ctx: Option<&dyn Cancellation>,
        margin_mm: f64,
    ) -> Result<VolumeMask, Error> {
        check_margin(margin_mm)?;
        let d = self.distance_transform(ctx, true)?;
        let mut out = self.clone();
        for (voxel, v) in out.voxels.iter_mut().zip(&d) {
            *voxel = *v <= margin_mm;
        }
        Ok(out)
    }
    pub fn shrink(
        &self,
        ctx: Option<&dyn Cancellation>,
        margin_mm: f64,
    ) -> Result<VolumeMask, Error> {
        check_margin(margin_mm)?;
        let d = self.distance_transform(ctx, false)?;
        let mut out = self.clone();
        for (i, v) in d.iter().enumerate() {
            out.voxels[i] = self.voxels[i] && *v > margin_mm;
        }
        Ok(out)
    }
    pub fn open(
        &self,
        ctx: Option<&dyn Cancellation>,
        radius_mm: f64,
    ) -> Result<VolumeMask, Error> {
        self.shrink(ctx, radius_mm)?.expand(ctx, radius_mm)
    }
    pub fn close(
        &self,
        ctx: Option<&dyn Cancellation>,
        radius_mm: f64,
    ) -> Result<VolumeMask, Error> {
        self.expand(ctx, radius_mm)?.shrink(ctx, radius_mm)
    }
    pub fn fill_holes(&self, ctx: Option<&dyn Cancellation>) -> Result<VolumeMask, Error> {
        ctx_check(ctx)?;
        self.validate_context(ctx)?;
        let (nx, ny, nz) = (self.columns, self.rows, self.slices);
        let mut outside = vec![false; self.voxels.len()];
        let mut stack: Vec<usize> = Vec::with_capacity(1024);
        let voxels = &self.voxels;
        let index = |x: i64, y: i64, z: i64| ((z * ny + y) * nx + x) as usize;
        let push = |outside: &mut Vec<bool>, stack: &mut Vec<usize>, x: i64, y: i64, z: i64| {
            let i = index(x, y, z);
            if !voxels[i] && !outside[i] {
                outside[i] = true;
                stack.push(i);
            }
        };
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    if index(x, y, z) % 4096 == 0 {
                        ctx_poll(ctx)?;
                    }
                    if x == 0 || y == 0 || z == 0 || x == nx - 1 || y == ny - 1 || z == nz - 1 {
                        push(&mut outside, &mut stack, x, y, z);
                    }
                }
            }
        }
        let mut steps = 0u64;
        while let Some(&i) = stack.last() {
            if steps.is_multiple_of(65536) {
                ctx_poll(ctx)?;
            }
            steps += 1;
            stack.pop();
            let i = i as i64;
            let (x, y, z) = (i % nx, i / nx % ny, i / (nx * ny));
            if x > 0 {
                push(&mut outside, &mut stack, x - 1, y, z);
            }
            if x < nx - 1 {
                push(&mut outside, &mut stack, x + 1, y, z);
            }
            if y > 0 {
                push(&mut outside, &mut stack, x, y - 1, z);
            }
            if y < ny - 1 {
                push(&mut outside, &mut stack, x, y + 1, z);
            }
            if z > 0 {
                push(&mut outside, &mut stack, x, y, z - 1);
            }
            if z < nz - 1 {
                push(&mut outside, &mut stack, x, y, z + 1);
            }
        }
        let mut out = self.clone();
        for (i, voxel) in out.voxels.iter_mut().enumerate() {
            if i % 4096 == 0 {
                ctx_poll(ctx)?;
            }
            *voxel = !outside[i];
        }
        ctx_check(ctx)?;
        Ok(out)
    }
    fn surface(&self, ctx: Option<&dyn Cancellation>) -> Result<VolumeMask, Error> {
        ctx_check(ctx)?;
        let mut out = self.clone();
        let (nx, ny, nz) = (
            self.columns as usize,
            self.rows as usize,
            self.slices as usize,
        );
        let v = &self.voxels;
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let i = (z * ny + y) * nx + x;
                    if i % 4096 == 0 {
                        ctx_poll(ctx)?;
                    }
                    if !v[i] {
                        continue;
                    }
                    let interior = x > 0
                        && x < nx - 1
                        && y > 0
                        && y < ny - 1
                        && z > 0
                        && z < nz - 1
                        && v[i - 1]
                        && v[i + 1]
                        && v[i - nx]
                        && v[i + nx]
                        && v[i - nx * ny]
                        && v[i + nx * ny];
                    out.voxels[i] = !interior;
                }
            }
        }
        ctx_check(ctx)?;
        Ok(out)
    }
}
fn check_margin(margin_mm: f64) -> Result<(), Error> {
    if !(margin_mm >= 0.0) || margin_mm.is_infinite() {
        return Err(Error::wrap(
            ERR_INVALID_VOLUME_MASK,
            format_args!("margin {} mm", float_v(margin_mm)),
        ));
    }
    Ok(())
}
pub(crate) fn squared_distance_1d_context(
    ctx: Option<&dyn Cancellation>,
    out: &mut [f64],
    f: &[f64],
    c: &[f64],
    hull: &mut [usize],
    bounds: &mut [f64],
) -> Result<(), Error> {
    let n = f.len();
    let mut k: isize = -1;
    let mut inner_steps = 0u64;
    for p in 0..n {
        if p % 4096 == 0 {
            ctx_poll(ctx)?;
        }
        if f[p] == f64::INFINITY {
            continue;
        }
        while k >= 0 {
            inner_steps += 1;
            if inner_steps.is_multiple_of(4096) {
                ctx_poll(ctx)?;
            }
            let q = hull[k as usize];
            let s = ((f[p] + c[p] * c[p]) - (f[q] + c[q] * c[q])) / (2.0 * (c[p] - c[q]));
            if s <= bounds[k as usize] {
                k -= 1;
                continue;
            }
            k += 1;
            hull[k as usize] = p;
            bounds[k as usize] = s;
            break;
        }
        if k < 0 {
            k = 0;
            hull[0] = p;
            bounds[0] = f64::NEG_INFINITY;
        }
        bounds[k as usize + 1] = f64::INFINITY;
    }
    if k < 0 {
        for (q, value) in out.iter_mut().enumerate() {
            if q % 4096 == 0 {
                ctx_poll(ctx)?;
            }
            *value = f64::INFINITY;
        }
        return ctx_check(ctx);
    }
    let mut j = 0usize;
    for q in 0..n {
        if q % 4096 == 0 {
            ctx_poll(ctx)?;
        }
        while bounds[j + 1] < c[q] {
            inner_steps += 1;
            if inner_steps.is_multiple_of(4096) {
                ctx_poll(ctx)?;
            }
            j += 1;
        }
        let p = hull[j];
        let dx = c[q] - c[p];
        out[q] = dx * dx + f[p];
    }
    ctx_check(ctx)
}
impl Segmentation3D {
    pub fn volume_mask(&self) -> (Option<VolumeMask>, Option<Error>) {
        let geometry_slices = self.geometry.slices.as_deref().unwrap_or(&[]);
        if self.columns <= 0 || self.rows <= 0 || geometry_slices.is_empty() {
            return (None, Some(Error::sentinel(ERR_INVALID_VOLUME_MASK)));
        }
        let slices = geometry_slices.len();
        let first = &geometry_slices[0];
        let mut m = VolumeMask::new(
            self.columns,
            self.rows,
            slices as i64,
            [first.col_spacing, first.row_spacing, 1.0],
        );
        if let Some(positions) = self.geometry.positions.as_deref()
            && positions.len() == slices
        {
            m.z.copy_from_slice(positions);
        }
        for z in 0..slices as i64 {
            let Some(mask) = self.mask_at(z) else {
                continue;
            };
            mask.for_each_pixel(|x, y| {
                if x < self.columns && y < self.rows {
                    let i = m.index(x, y, z);
                    m.voxels[i] = true;
                }
            });
        }
        let err = m.validate().err();
        (Some(m), err)
    }
    pub fn set_volume_mask(&mut self, m: &VolumeMask) -> Result<(), Error> {
        m.validate()?;
        if m.columns != self.columns || m.rows != self.rows {
            return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
        }
        for z in 0..m.slices {
            let mut mask = RasterMask::new(m.columns, m.rows);
            for y in 0..m.rows {
                let mut start = -1i64;
                for x in 0..=m.columns {
                    let on = x < m.columns && m.voxels[m.index(x, y, z)];
                    if on && start < 0 {
                        start = x;
                    } else if !on && start >= 0 {
                        mask.set_run(y, start, x);
                        start = -1;
                    }
                }
            }
            self.set_mask(z, Some(mask));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OverlapMetrics {
    pub dice: f64,
    pub jaccard: f64,
    pub volume_a: f64,
    pub volume_b: f64,
    pub hausdorff_mm: f64,
    pub hd95_mm: f64,
    pub mean_surface_distance_mm: f64,
}
pub const ERR_EMPTY_MASK: &str = "roi: empty mask";
pub fn compare_masks(
    ctx: Option<&dyn Cancellation>,
    a: &VolumeMask,
    b: &VolumeMask,
) -> (OverlapMetrics, Option<Error>) {
    let mut m = OverlapMetrics::default();
    match compare_masks_into(ctx, a, b, &mut m) {
        Ok(()) => (m, None),
        Err(err) if err.is(ERR_EMPTY_MASK) => (m, Some(err)),
        Err(err) => (OverlapMetrics::default(), Some(err)),
    }
}
fn compare_masks_into(
    ctx: Option<&dyn Cancellation>,
    a: &VolumeMask,
    b: &VolumeMask,
    m: &mut OverlapMetrics,
) -> Result<(), Error> {
    ctx_check(ctx)?;
    a.validate_context(ctx)?;
    b.validate_context(ctx)?;
    if a.columns != b.columns
        || a.rows != b.rows
        || a.slices != b.slices
        || !equal_axes(&a.x, &b.x)
        || !equal_axes(&a.y, &b.y)
        || !equal_axes(&a.z, &b.z)
    {
        return Err(Error::wrap(
            ERR_INVALID_VOLUME_MASK,
            "masks on different grids",
        ));
    }
    let (mut both, mut either, mut count_a, mut count_b) = (0i64, 0i64, 0i64, 0i64);
    for i in 0..a.voxels.len() {
        if i % 4096 == 0 {
            ctx_poll(ctx)?;
        }
        let (va, vb) = (a.voxels[i], b.voxels[i]);
        if va {
            count_a += 1;
            m.volume_a += a.voxel_volume(i);
        }
        if vb {
            count_b += 1;
            m.volume_b += a.voxel_volume(i);
        }
        if va && vb {
            both += 1;
        }
        if va || vb {
            either += 1;
        }
    }
    ctx_check(ctx)?;
    if count_a == 0 || count_b == 0 {
        return Err(Error::sentinel(ERR_EMPTY_MASK));
    }
    m.dice = 2.0 * both as f64 / (count_a + count_b) as f64;
    m.jaccard = both as f64 / either as f64;
    let surface_a = a.surface(ctx)?;
    let surface_b = b.surface(ctx)?;
    let to_a = surface_a.distance_transform(ctx, true)?;
    let to_b = surface_b.distance_transform(ctx, true)?;
    let (mut ab, mut ba) = (Vec::new(), Vec::new());
    for i in 0..surface_a.voxels.len() {
        if i % 4096 == 0 {
            ctx_poll(ctx)?;
        }
        if surface_a.voxels[i] {
            ab.push(to_b[i]);
        }
        if surface_b.voxels[i] {
            ba.push(to_a[i]);
        }
    }
    let mut sum = 0.0f64;
    for direction in [&ab, &ba] {
        for (i, &d) in direction.iter().enumerate() {
            if i % 4096 == 0 {
                ctx_poll(ctx)?;
            }
            sum += d;
            m.hausdorff_mm = fmath::max(m.hausdorff_mm, d);
        }
    }
    m.mean_surface_distance_mm = sum / (ab.len() + ba.len()) as f64;
    let ab95 = percentile_in_place(&mut ab, 0.95);
    ctx_check(ctx)?;
    m.hd95_mm = fmath::max(ab95, percentile_in_place(&mut ba, 0.95));
    ctx_check(ctx)?;
    Ok(())
}
fn equal_axes(a: &[f64], b: &[f64]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if (a[i] - b[i]).abs() > 1e-9 * fmath::max(1.0, a[i].abs()) {
            return false;
        }
    }
    true
}
fn percentile_in_place(values: &mut [f64], p: f64) -> f64 {
    sort_float64s(values);
    let rank = fmath::f64_to_i64((p * values.len() as f64).ceil()) - 1;
    values[rank.min(values.len() as i64 - 1).max(0) as usize]
}
