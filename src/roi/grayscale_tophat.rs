#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::grayscale_reconstruction::{
    ERR_INVALID_MORPHOLOGY, GrayscaleOptions, check_morphology_volume, max32, min32,
};
use super::resample_grid::for_each_line;
use super::{Cancellation, Error};
use crate::fmath;
use crate::imagefilter::Volume;
use crate::textfmt::float_v;
const MAX_BALL_ROWS: i64 = 1 << 14;
#[derive(Debug, Clone, Copy)]
struct BallRow {
    dy: i64,
    dz: i64,
    half: i64,
}
fn ball_rows(dims: [i64; 3], spacing: [f64; 3], r: f64) -> Result<Vec<BallRow>, Error> {
    if !(r >= 0.0) || r.is_infinite() {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!("radius {} mm", float_v(r)),
        ));
    }
    let extent = |axis: usize| -> i64 {
        if dims[axis] == 1 {
            return 0;
        }
        fmath::f64_to_i64((r / spacing[axis] + 1e-9).floor()).min(dims[axis] - 1)
    };
    let (ry, rz) = (extent(1), extent(2));
    let span = |e: i64| e.wrapping_mul(2).wrapping_add(1);
    if span(ry).wrapping_mul(span(rz)) > MAX_BALL_ROWS {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "a {} mm ball spans more than {} rows at this spacing",
                float_v(r),
                MAX_BALL_ROWS
            ),
        ));
    }
    let mut rows = Vec::new();
    for dz in rz.wrapping_neg()..=rz {
        for dy in ry.wrapping_neg()..=ry {
            let mut rest = r * r;
            if dims[1] > 1 {
                rest -= fmath::pow(dy as f64 * spacing[1], 2.0);
            }
            if dims[2] > 1 {
                rest -= fmath::pow(dz as f64 * spacing[2], 2.0);
            }
            if rest < -1e-9 * r * r {
                continue;
            }
            let mut half = 0;
            if dims[0] > 1 {
                let clamped = if rest > 0.0 { rest } else { 0.0 };
                half = fmath::f64_to_i64((clamped.sqrt() / spacing[0] + 1e-9).floor())
                    .min(dims[0] - 1);
            }
            rows.push(BallRow { dy, dz, half });
        }
    }
    Ok(rows)
}
pub fn grayscale_erode(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    ball_filter(ctx, src, radius_mm, opts, false)
}
pub fn grayscale_dilate(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    ball_filter(ctx, src, radius_mm, opts, true)
}
pub fn grayscale_open(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    let eroded = grayscale_erode(ctx, src, radius_mm, opts)?;
    grayscale_dilate(ctx, &eroded, radius_mm, opts)
}
pub fn grayscale_close(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    let dilated = grayscale_dilate(ctx, src, radius_mm, opts)?;
    grayscale_erode(ctx, &dilated, radius_mm, opts)
}
pub fn white_top_hat(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    let mut opened = grayscale_open(ctx, src, radius_mm, opts)?;
    for (i, &v) in src.data.iter().enumerate() {
        opened.data[i] = v - opened.data[i];
    }
    Ok(opened)
}
pub fn black_top_hat(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
) -> Result<Volume, Error> {
    let mut closed = grayscale_close(ctx, src, radius_mm, opts)?;
    for (i, &v) in src.data.iter().enumerate() {
        closed.data[i] -= v;
    }
    Ok(closed)
}
fn ball_filter(
    ctx: Option<&dyn Cancellation>,
    src: &Volume,
    radius_mm: f64,
    opts: GrayscaleOptions,
    dilate: bool,
) -> Result<Volume, Error> {
    let opts = opts.normalize()?;
    check_morphology_volume(src, opts.max_voxels, "image")?;
    let rows = ball_rows(src.dims, src.spacing_mm, radius_mm)?;
    let (nx, ny, nz) = (src.dims[0], src.dims[1], src.dims[2]);
    let mut out = Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data: vec![0.0f32; src.data.len()],
    };
    let fill = if dilate {
        f32::NEG_INFINITY
    } else {
        f32::INFINITY
    };
    let w = nx as usize;
    let mut run = vec![0.0f32; w];
    let mut deque = vec![0usize; w];
    let data = &mut out.data;
    for_each_line(ctx, ny * nz, |line| {
        let (y, z) = (line % ny, line / ny);
        let acc = &mut data[line as usize * w..(line as usize + 1) * w];
        acc.fill(fill);
        for row in &rows {
            let (sy, sz) = (y + row.dy, z + row.dz);
            if sy < 0 || sy >= ny || sz < 0 || sz >= nz {
                continue;
            }
            let start = (sz * ny + sy) as usize * w;
            let source = &src.data[start..start + w];
            running_extremum(&mut run, source, row.half, &mut deque, dilate);
            if dilate {
                for (a, &v) in acc.iter_mut().zip(run.iter()) {
                    *a = max32(*a, v);
                }
            } else {
                for (a, &v) in acc.iter_mut().zip(run.iter()) {
                    *a = min32(*a, v);
                }
            }
        }
    })?;
    Ok(out)
}
fn running_extremum(dst: &mut [f32], src: &[f32], half: i64, deque: &mut [usize], high: bool) {
    let n = src.len() as i64;
    let (mut head, mut tail, mut next) = (0usize, 0usize, 0i64);
    for x in 0..n {
        while next < n && next <= x.wrapping_add(half) {
            let v = src[next as usize];
            while tail > head
                && (!high && src[deque[tail - 1]] >= v || high && src[deque[tail - 1]] <= v)
            {
                tail -= 1;
            }
            deque[tail] = next as usize;
            tail += 1;
            next += 1;
        }
        while (deque[head] as i64) < x.wrapping_sub(half) {
            head += 1;
        }
        dst[x as usize] = src[deque[head]];
    }
}
