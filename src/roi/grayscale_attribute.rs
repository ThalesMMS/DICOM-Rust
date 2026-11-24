#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::grayscale_reconstruction::dims_v;
use super::{Cancellation, Error, ctx_err};
use crate::imagefilter::Volume;
pub const ERR_INVALID_GRAYSCALE_ATTRIBUTE: &str = "roi: invalid grayscale attribute morphology";
pub const ERR_GRAYSCALE_ATTRIBUTE_LIMIT: &str = "roi: grayscale attribute resource limit";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AttributeDimension(pub u8);
pub const ATTRIBUTE_AUTO: AttributeDimension = AttributeDimension(0);
pub const ATTRIBUTE_2D: AttributeDimension = AttributeDimension(1);
pub const ATTRIBUTE_3D: AttributeDimension = AttributeDimension(2);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AttributeSizeUnit(pub u8);
pub const ATTRIBUTE_SAMPLES: AttributeSizeUnit = AttributeSizeUnit(0);
pub const ATTRIBUTE_SQUARE_MM: AttributeSizeUnit = AttributeSizeUnit(1);
pub const ATTRIBUTE_CUBIC_MM: AttributeSizeUnit = AttributeSizeUnit(2);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrayscaleAttributeOptions {
    pub dimension: AttributeDimension,
    pub connectivity: i64,
    pub unit: AttributeSizeUnit,
    pub max_voxels: i64,
    pub max_bytes: i64,
}
pub fn grayscale_attribute_open(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    threshold: f64,
    opts: GrayscaleAttributeOptions,
) -> Result<Volume, Error> {
    grayscale_attribute(ctx, src, threshold, opts, false)
}
pub fn grayscale_attribute_close(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    threshold: f64,
    opts: GrayscaleAttributeOptions,
) -> Result<Volume, Error> {
    grayscale_attribute(ctx, src, threshold, opts, true)
}
const ATTRIBUTE_BYTES_PER_SAMPLE: i64 = 24;
const ATTRIBUTE_FIXED_BYTES: i64 = 1024;
fn poll(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
fn invalid(detail: impl std::fmt::Display) -> Error {
    Error::wrap(ERR_INVALID_GRAYSCALE_ATTRIBUTE, detail)
}
fn grayscale_attribute(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    threshold: f64,
    mut opts: GrayscaleAttributeOptions,
    close: bool,
) -> Result<Volume, Error> {
    poll(ctx)?;
    if threshold < 0.0
        || threshold.is_nan()
        || threshold.is_infinite()
        || opts.max_voxels < 0
        || opts.max_bytes < 0
    {
        return Err(invalid("input, threshold or limits"));
    }
    let mut n: i64 = 1;
    for d in src.dims {
        if d <= 0 || d > i32::MAX as i64 / n {
            return Err(invalid(format_args!("dimensions {}", dims_v(src.dims))));
        }
        n *= d;
    }
    if src.data.len() as i64 != n {
        return Err(invalid("data length"));
    }
    if opts.max_voxels == 0 {
        opts.max_voxels = 64 << 20;
    }
    if opts.max_bytes == 0 {
        opts.max_bytes = 1 << 30;
    }
    if n > opts.max_voxels
        || n > (opts.max_bytes - ATTRIBUTE_FIXED_BYTES) / ATTRIBUTE_BYTES_PER_SAMPLE
        || opts.max_bytes < ATTRIBUTE_FIXED_BYTES
    {
        return Err(Error::wrap(
            ERR_GRAYSCALE_ATTRIBUTE_LIMIT,
            format_args!(
                "{} samples need {} array/workspace bytes",
                n,
                n * ATTRIBUTE_BYTES_PER_SAMPLE + ATTRIBUTE_FIXED_BYTES
            ),
        ));
    }
    let mut dim = opts.dimension;
    if dim == ATTRIBUTE_AUTO {
        dim = ATTRIBUTE_3D;
        if src.dims[2] == 1 {
            dim = ATTRIBUTE_2D;
        }
    }
    if (dim != ATTRIBUTE_2D && dim != ATTRIBUTE_3D) || (dim == ATTRIBUTE_2D && src.dims[2] != 1) {
        return Err(invalid("dimension"));
    }
    let mut conn = opts.connectivity;
    if conn == 0 {
        conn = 6;
        if dim == ATTRIBUTE_2D {
            conn = 4;
        }
    }
    if (dim == ATTRIBUTE_2D && conn != 4 && conn != 8)
        || (dim == ATTRIBUTE_3D && conn != 6 && conn != 18 && conn != 26)
    {
        return Err(invalid(format_args!("connectivity {conn}")));
    }
    if opts.unit.0 > ATTRIBUTE_CUBIC_MM.0
        || (opts.unit == ATTRIBUTE_SQUARE_MM && dim != ATTRIBUTE_2D)
        || (opts.unit == ATTRIBUTE_CUBIC_MM && dim != ATTRIBUTE_3D)
    {
        return Err(invalid("unit/dimension"));
    }
    let mut measure = 1.0f64;
    for (axis, &spacing) in src.spacing_mm.iter().enumerate() {
        let physical = opts.unit != ATTRIBUTE_SAMPLES && (dim == ATTRIBUTE_3D || axis < 2);
        let required = src.dims[axis] > 1 || physical;
        if required && (!(spacing > 0.0) || spacing.is_infinite() || spacing.is_nan()) {
            return Err(invalid(format_args!("spacing axis {axis}")));
        }
        if physical {
            measure *= spacing;
        }
    }
    if !(measure > 0.0) || measure.is_infinite() || measure.is_nan() {
        return Err(invalid("physical sample measure"));
    }
    for (i, &value) in src.data.iter().enumerate() {
        if i & 4095 == 0 {
            poll(ctx)?;
        }
        if value.is_nan() || value.is_infinite() {
            return Err(invalid(format_args!("nonfinite sample {i}")));
        }
    }
    let n = n as usize;
    let mut out = Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data: vec![0.0f32; n],
    };
    if threshold == 0.0 {
        for (i, &v) in src.data.iter().enumerate() {
            if i & 4095 == 0 {
                poll(ctx)?;
            }
            out.data[i] = v;
        }
        poll(ctx)?;
        return Ok(out);
    }
    let mut order = vec![0i32; n];
    let mut scratch = vec![0i32; n];
    let mut parent = vec![0i32; n];
    let mut union = vec![0i32; n];
    let mut count = vec![0u32; n];
    for i in 0..n {
        if i & 4095 == 0 {
            poll(ctx)?;
        }
        order[i] = i as i32;
        union[i] = -1;
    }
    attribute_sort(ctx, &mut order, &mut scratch, &src.data, close)?;
    let mut offsets = [[0i64; 3]; 26];
    let mut no = 0usize;
    for dz in -1i64..=1 {
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let axes = [dx, dy, dz].iter().filter(|&&d| d != 0).count();
                if axes == 0
                    || (dim == ATTRIBUTE_2D && dz != 0)
                    || ((conn == 4 || conn == 6) && axes > 1)
                    || (conn == 18 && axes > 2)
                {
                    continue;
                }
                offsets[no] = [dx, dy, dz];
                no += 1;
            }
        }
    }
    let mut visits = 0u64;
    let (nx, ny, nz) = (src.dims[0], src.dims[1], src.dims[2]);
    for (j, &p) in order.iter().enumerate() {
        if j & 1023 == 0 {
            poll(ctx)?;
        }
        let pu = p as usize;
        parent[pu] = p;
        union[pu] = p;
        count[pu] = 1;
        let pi = p as i64;
        let (x, y, z) = (pi % nx, (pi / nx) % ny, pi / (nx * ny));
        for delta in &offsets[..no] {
            let (qx, qy, qz) = (x + delta[0], y + delta[1], z + delta[2]);
            if qx < 0 || qx >= nx || qy < 0 || qy >= ny || qz < 0 || qz >= nz {
                continue;
            }
            let q = ((qz * ny + qy) * nx + qx) as usize;
            if union[q] < 0 {
                continue;
            }
            let mut index = q as i32;
            let mut root = index;
            while union[root as usize] != root {
                visits += 1;
                if visits & 4095 == 0 {
                    poll(ctx)?;
                }
                root = union[root as usize];
            }
            while index != root {
                let next = union[index as usize];
                union[index as usize] = root;
                index = next;
            }
            if root != p {
                parent[root as usize] = p;
                union[root as usize] = p;
                count[pu] = count[pu].wrapping_add(count[root as usize]);
            }
        }
    }
    for j in (0..n).rev() {
        if j & 4095 == 0 {
            poll(ctx)?;
        }
        let p = order[j] as usize;
        let mut q = parent[p] as usize;
        if src.data[q] == src.data[parent[q] as usize] {
            parent[p] = parent[q];
            q = parent[p] as usize;
        }
        let keep = count[p] as f64 * measure >= threshold;
        if p == q || keep {
            out.data[p] = src.data[p];
        } else {
            out.data[p] = out.data[q];
        }
    }
    poll(ctx)?;
    Ok(out)
}
fn attribute_sort(
    ctx: Option<&dyn Cancellation>,
    order: &mut [i32],
    scratch: &mut [i32],
    values: &[f32],
    close: bool,
) -> Result<(), Error> {
    let n = order.len();
    let mut steps = 0u64;
    let before = |a: i32, b: i32| -> bool {
        let (va, vb) = (values[a as usize], values[b as usize]);
        if va == vb {
            return a < b;
        }
        if close { va < vb } else { va > vb }
    };
    let mut width = 1usize;
    while width < n {
        let mut begin = 0usize;
        while begin < n {
            let mid = begin + width.min(n - begin);
            let end = mid + width.min(n - mid);
            let (mut a, mut b) = (begin, mid);
            for slot in scratch.iter_mut().take(end).skip(begin) {
                steps += 1;
                if steps & 4095 == 0 {
                    poll(ctx)?;
                }
                if a < mid && (b >= end || before(order[a], order[b])) {
                    *slot = order[a];
                    a += 1;
                } else {
                    *slot = order[b];
                    b += 1;
                }
            }
            begin = end;
        }
        for i in 0..n {
            if i & 4095 == 0 {
                poll(ctx)?;
            }
            order[i] = scratch[i];
        }
        if width > n / 2 {
            break;
        }
        width *= 2;
    }
    poll(ctx)
}
