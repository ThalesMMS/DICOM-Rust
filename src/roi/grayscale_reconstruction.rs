#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::connected_components::{
    CONNECTIVITY_6, CONNECTIVITY_18, CONNECTIVITY_26, Connectivity3D,
};
use super::distance3d::VolumeMask;
use super::{Cancellation, Error, ctx_err};
use crate::imagefilter::Volume;
use crate::textfmt::float_v;
pub const ERR_INVALID_MORPHOLOGY: &str = "roi: invalid grayscale morphology request";
pub const ERR_MORPHOLOGY_TOO_LARGE: &str = "roi: grayscale morphology exceeds the voxel budget";
pub const DEFAULT_MAX_MORPHOLOGY_VOXELS: i64 = 1 << 26;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrayscaleOptions {
    pub connectivity: Connectivity3D,
    pub max_voxels: i64,
}
impl GrayscaleOptions {
    pub(crate) fn normalize(mut self) -> Result<GrayscaleOptions, Error> {
        if self.connectivity.0 == 0 {
            self.connectivity = CONNECTIVITY_6;
        }
        self.connectivity.validate()?;
        if self.max_voxels == 0 {
            self.max_voxels = DEFAULT_MAX_MORPHOLOGY_VOXELS;
        } else if self.max_voxels < 0 {
            return Err(Error::wrap(
                ERR_INVALID_MORPHOLOGY,
                format_args!("negative voxel budget {}", self.max_voxels),
            ));
        }
        Ok(self)
    }
}
#[inline]
pub(crate) fn min32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}
#[inline]
pub(crate) fn max32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}
pub(crate) fn dims_v(d: [i64; 3]) -> String {
    format!("[{} {} {}]", d[0], d[1], d[2])
}
pub(crate) fn check_morphology_volume(
    v: &Volume,
    max_voxels: i64,
    name: &str,
) -> Result<(), Error> {
    if let Err(e) = v.validate() {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!("{name}: {e}"),
        ));
    }
    let n = v.data.len() as i64;
    if n > max_voxels || n > i32::MAX as i64 {
        return Err(Error::wrap(
            ERR_MORPHOLOGY_TOO_LARGE,
            format_args!("{name} has {n} voxels, budget {max_voxels}"),
        ));
    }
    Ok(())
}
fn check_image(src: &Volume, opts: GrayscaleOptions) -> Result<(), Error> {
    let opts = opts.normalize()?;
    check_morphology_volume(src, opts.max_voxels, "image")
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct NbOffset {
    pub dx: i64,
    pub dy: i64,
    pub dz: i64,
    pub step: i64,
}
#[derive(Debug, Clone)]
pub(crate) struct Neighbourhood {
    pub dims: [i64; 3],
    pub all: Vec<NbOffset>,
    pub prev: Vec<NbOffset>,
    pub next: Vec<NbOffset>,
}
fn connectivity_order(c: Connectivity3D) -> i64 {
    match c {
        CONNECTIVITY_18 => 2,
        CONNECTIVITY_26 => 3,
        _ => 1,
    }
}
impl Neighbourhood {
    pub(crate) fn new(dims: [i64; 3], c: Connectivity3D) -> Neighbourhood {
        let mut n = Neighbourhood {
            dims,
            all: Vec::new(),
            prev: Vec::new(),
            next: Vec::new(),
        };
        for dz in -1i64..=1 {
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let order = dx.abs() + dy.abs() + dz.abs();
                    if order == 0
                        || order > connectivity_order(c)
                        || dims[0] == 1 && dx != 0
                        || dims[1] == 1 && dy != 0
                        || dims[2] == 1 && dz != 0
                    {
                        continue;
                    }
                    let o = NbOffset {
                        dx,
                        dy,
                        dz,
                        step: (dz * dims[1] + dy) * dims[0] + dx,
                    };
                    n.all.push(o);
                    if o.step < 0 {
                        n.prev.push(o);
                    } else {
                        n.next.push(o);
                    }
                }
            }
        }
        n
    }
    #[inline]
    pub(crate) fn coords(&self, i: usize) -> (i64, i64, i64) {
        let i = i as i64;
        let plane = self.dims[0] * self.dims[1];
        let z = i / plane;
        let r = i - z * plane;
        let y = r / self.dims[0];
        (r - y * self.dims[0], y, z)
    }
    #[inline]
    pub(crate) fn inside(&self, x: i64, y: i64, z: i64, o: NbOffset) -> bool {
        ((x + o.dx) as u64) < self.dims[0] as u64
            && ((y + o.dy) as u64) < self.dims[1] as u64
            && ((z + o.dz) as u64) < self.dims[2] as u64
    }
}
#[inline]
fn at(i: usize, step: i64) -> usize {
    (i as i64 + step) as usize
}
pub fn reconstruction_by_dilation(
    ctx: Option<&dyn Cancellation>,
    marker: &Volume,
    mask: &Volume,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    reconstruction(ctx, marker, mask, opts, false)
}
pub fn reconstruction_by_erosion(
    ctx: Option<&dyn Cancellation>,
    marker: &Volume,
    mask: &Volume,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    reconstruction(ctx, marker, mask, opts, true)
}
fn reconstruction(
    ctx: Option<&dyn Cancellation>,
    marker: &Volume,
    mask: &Volume,
    opts: GrayscaleOptions,
    erosion: bool,
) -> Result<Volume, Error> {
    let opts = opts.normalize()?;
    check_morphology_volume(marker, opts.max_voxels, "marker")?;
    check_morphology_volume(mask, opts.max_voxels, "mask")?;
    if marker.dims != mask.dims {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "marker dimensions {} differ from mask {}",
                dims_v(marker.dims),
                dims_v(mask.dims)
            ),
        ));
    }
    let mut out = marker.clone();
    let negated: Vec<f32>;
    let limit: &[f32] = if erosion {
        negated = mask.data.iter().map(|v| -v).collect();
        for v in out.data.iter_mut() {
            *v = -*v;
        }
        &negated
    } else {
        &mask.data
    };
    for (i, &v) in out.data.iter().enumerate() {
        if v > limit[i] {
            let side = if erosion { "below" } else { "above" };
            return Err(Error::wrap(
                ERR_INVALID_MORPHOLOGY,
                format_args!("marker {side} mask at sample {i}"),
            ));
        }
    }
    let nb = Neighbourhood::new(out.dims, opts.connectivity);
    reconstruct_by_dilation(ctx, &mut out.data, limit, &nb)?;
    if erosion {
        for v in out.data.iter_mut() {
            *v = -*v;
        }
    }
    Ok(out)
}
fn reconstruct_by_dilation(
    ctx: Option<&dyn Cancellation>,
    j: &mut [f32],
    mask: &[f32],
    nb: &Neighbourhood,
) -> Result<(), Error> {
    let (nx, ny, nz) = (nb.dims[0], nb.dims[1], nb.dims[2]);
    for z in 0..nz {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        for y in 0..ny {
            let base = ((z * ny + y) * nx) as usize;
            for x in 0..nx {
                let i = base + x as usize;
                let mut v = j[i];
                for &o in &nb.prev {
                    if nb.inside(x, y, z, o) && j[at(i, o.step)] > v {
                        v = j[at(i, o.step)];
                    }
                }
                j[i] = min32(v, mask[i]);
            }
        }
    }
    let mut queue: Vec<i32> = Vec::new();
    for z in (0..nz).rev() {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        for y in (0..ny).rev() {
            let base = ((z * ny + y) * nx) as usize;
            for x in (0..nx).rev() {
                let i = base + x as usize;
                let mut v = j[i];
                for &o in &nb.next {
                    if nb.inside(x, y, z, o) && j[at(i, o.step)] > v {
                        v = j[at(i, o.step)];
                    }
                }
                v = min32(v, mask[i]);
                j[i] = v;
                for &o in &nb.next {
                    if nb.inside(x, y, z, o) {
                        let q = at(i, o.step);
                        if j[q] < v && j[q] < mask[q] {
                            queue.push(i as i32);
                            break;
                        }
                    }
                }
            }
        }
    }
    let mut head = 0usize;
    let mut steps = 0u64;
    while head < queue.len() {
        if steps & 0xffff == 0 {
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            if head > queue.len() / 2 {
                queue.drain(..head);
                head = 0;
            }
        }
        let p = queue[head] as usize;
        head += 1;
        let v = j[p];
        let (x, y, z) = nb.coords(p);
        for &o in &nb.all {
            if !nb.inside(x, y, z, o) {
                continue;
            }
            let q = at(p, o.step);
            if j[q] < v && j[q] < mask[q] {
                j[q] = min32(v, mask[q]);
                queue.push(q as i32);
            }
        }
        steps += 1;
    }
    Ok(())
}
pub fn regional_maxima(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    opts: GrayscaleOptions,
) -> Result<VolumeMask, Error> {
    regional_extrema(ctx, src, opts, false)
}
pub fn regional_minima(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    opts: GrayscaleOptions,
) -> Result<VolumeMask, Error> {
    regional_extrema(ctx, src, opts, true)
}
fn regional_extrema(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    opts: GrayscaleOptions,
    minima: bool,
) -> Result<VolumeMask, Error> {
    let opts = opts.normalize()?;
    check_morphology_volume(src, opts.max_voxels, "image")?;
    let nb = Neighbourhood::new(src.dims, opts.connectivity);
    let data = &src.data;
    let beyond = |a: f32, b: f32| if minima { a < b } else { a > b };
    let mut excluded = vec![false; data.len()];
    let mut stack: Vec<i32> = Vec::new();
    for i in 0..data.len() {
        if i & 0xffff == 0
            && let Some(e) = ctx_err(ctx)
        {
            return Err(e);
        }
        if excluded[i] {
            continue;
        }
        let v = data[i];
        let (x, y, z) = nb.coords(i);
        let mut higher = false;
        for &o in &nb.all {
            if nb.inside(x, y, z, o) && beyond(data[at(i, o.step)], v) {
                higher = true;
                break;
            }
        }
        if !higher {
            continue;
        }
        excluded[i] = true;
        stack.clear();
        stack.push(i as i32);
        while let Some(p) = stack.pop() {
            let p = p as usize;
            let (px, py, pz) = nb.coords(p);
            for &o in &nb.all {
                if !nb.inside(px, py, pz, o) {
                    continue;
                }
                let q = at(p, o.step);
                if !excluded[q] && data[q] == v {
                    excluded[q] = true;
                    stack.push(q as i32);
                }
            }
        }
    }
    let mut out = VolumeMask::new(src.dims[0], src.dims[1], src.dims[2], src.spacing_mm);
    for (i, e) in excluded.iter().enumerate() {
        out.voxels[i] = !e;
    }
    Ok(out)
}
fn check_height(h: f64) -> Result<(), Error> {
    if !(h >= 0.0) || h.is_infinite() || h > f32::MAX as f64 {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!("height {}", float_v(h)),
        ));
    }
    Ok(())
}
fn shifted(src: &Volume, h: f64) -> Volume {
    Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data: src.data.iter().map(|&v| (v as f64 + h) as f32).collect(),
    }
}
pub fn h_maxima(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    h: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    check_height(h)?;
    check_image(src, opts)?;
    reconstruction_by_dilation(ctx, &shifted(src, -h), src, opts)
}
pub fn h_minima(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    h: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    check_height(h)?;
    check_image(src, opts)?;
    reconstruction_by_erosion(ctx, &shifted(src, h), src, opts)
}
pub fn h_convex(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    h: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    let mut hmax = h_maxima(ctx, src, h, opts)?;
    for (i, &v) in src.data.iter().enumerate() {
        hmax.data[i] = v - hmax.data[i];
    }
    Ok(hmax)
}
pub fn h_concave(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    h: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    let mut hmin = h_minima(ctx, src, h, opts)?;
    for (i, &v) in src.data.iter().enumerate() {
        hmin.data[i] -= v;
    }
    Ok(hmin)
}
pub fn extended_maxima(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    h: f64,
    opts: GrayscaleOptions,
) -> Result<VolumeMask, Error> {
    let hmax = h_maxima(ctx, src, h, opts)?;
    regional_maxima(ctx, &hmax, opts)
}
pub fn extended_minima(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    h: f64,
    opts: GrayscaleOptions,
) -> Result<VolumeMask, Error> {
    let hmin = h_minima(ctx, src, h, opts)?;
    regional_minima(ctx, &hmin, opts)
}
pub fn grayscale_fill_holes(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    check_image(src, opts)?;
    let mut peak = f32::NEG_INFINITY;
    for &v in &src.data {
        peak = max32(peak, v);
    }
    let mut marker = Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data: vec![0.0f32; src.data.len()],
    };
    let (nx, ny, nz) = (src.dims[0], src.dims[1], src.dims[2]);
    let border = |i: i64, n: i64| n > 1 && (i == 0 || i == n - 1);
    let mut i = 0usize;
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                marker.data[i] = if border(x, nx) || border(y, ny) || border(z, nz) {
                    src.data[i]
                } else {
                    peak
                };
                i += 1;
            }
        }
    }
    reconstruction_by_erosion(ctx, &marker, src, opts)
}
