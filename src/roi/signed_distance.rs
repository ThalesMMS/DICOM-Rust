#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::distance3d::{ERR_INVALID_VOLUME_MASK, VolumeMask, ctx_check};
use super::roimask::RasterMask;
use super::{Cancellation, Error};
pub const ERR_SIGNED_DISTANCE_LIMIT: &str = "roi: signed distance resource limit";
pub const DEFAULT_SIGNED_DISTANCE_MAX_VOXELS: i64 = 1 << 26;
pub const DEFAULT_SIGNED_DISTANCE_MAX_BYTES: i64 = 1 << 30;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SignedDistanceOptions {
    pub inside_is_positive: bool,
    pub squared_distance: bool,
    pub max_voxels: i64,
    pub max_bytes: i64,
}
impl VolumeMask {
    pub fn signed_distance_transform(
        &self,
        ctx: Option<&dyn Cancellation>,
        opts: SignedDistanceOptions,
    ) -> Result<Vec<f64>, Error> {
        ctx_check(ctx)?;
        let n = signed_distance_admission(self.columns, self.rows, self.slices, false, opts)?;
        if self.voxels.len() as i64 != n
            || self.x.len() as i64 != self.columns
            || self.y.len() as i64 != self.rows
            || self.z.len() as i64 != self.slices
        {
            return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
        }
        signed_distance(ctx, self, opts)
    }
}
impl RasterMask {
    pub fn signed_distance_transform(
        &self,
        ctx: Option<&dyn Cancellation>,
        column_spacing: f64,
        row_spacing: f64,
        opts: SignedDistanceOptions,
    ) -> Result<Vec<f64>, Error> {
        ctx_check(ctx)?;
        if !(column_spacing > 0.0)
            || !(row_spacing > 0.0)
            || column_spacing.is_infinite()
            || row_spacing.is_infinite()
        {
            return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
        }
        let n = signed_distance_admission(self.columns, self.rows, 1, true, opts)?;
        if self.runs.len() as i64 != self.rows {
            return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
        }
        for (y, runs) in self.runs.iter().enumerate() {
            if y & 255 == 0 {
                ctx_check(ctx)?;
            }
            if runs.len() as i64 > self.columns {
                return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
            }
            let mut end = 0i64;
            for run in runs {
                if run.start < end
                    || run.start < 0
                    || run.end <= run.start
                    || run.end > self.columns
                {
                    return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
                }
                end = run.end;
            }
        }
        let mut volume = VolumeMask {
            columns: self.columns,
            rows: self.rows,
            slices: 1,
            x: vec![0.0; self.columns as usize],
            y: vec![0.0; self.rows as usize],
            z: vec![0.0],
            voxels: vec![false; n as usize],
        };
        for (x, value) in volume.x.iter_mut().enumerate() {
            *value = x as f64 * column_spacing;
        }
        for y in 0..self.rows as usize {
            volume.y[y] = y as f64 * row_spacing;
            if y & 255 == 0 {
                ctx_check(ctx)?;
            }
            for run in &self.runs[y] {
                for x in run.start..run.end {
                    if x & 4095 == 0 {
                        ctx_check(ctx)?;
                    }
                    volume.voxels[y * self.columns as usize + x as usize] = true;
                }
            }
        }
        signed_distance(ctx, &volume, opts)
    }
}
fn signed_distance_admission(
    nx: i64,
    ny: i64,
    nz: i64,
    raster: bool,
    opts: SignedDistanceOptions,
) -> Result<i64, Error> {
    if nx <= 0 || ny <= 0 || nz <= 0 || opts.max_voxels < 0 || opts.max_bytes < 0 {
        return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
    }
    let (mut limit, mut bytes) = (opts.max_voxels, opts.max_bytes);
    if limit == 0 {
        limit = DEFAULT_SIGNED_DISTANCE_MAX_VOXELS;
    }
    if bytes == 0 {
        bytes = DEFAULT_SIGNED_DISTANCE_MAX_BYTES;
    }
    if nx > limit || ny > limit / nx || nz > limit / (nx * ny) {
        return Err(Error::wrap(
            ERR_SIGNED_DISTANCE_LIMIT,
            format_args!("voxel count exceeds {limit}"),
        ));
    }
    let n = nx * ny * nz;
    let mut per_voxel = 9i64;
    let mut axis_copies = 1i64;
    if raster {
        per_voxel += 1;
        axis_copies += 1;
    }
    let mut remaining = bytes;
    let mut reserve = |count: i64, width: i64| -> bool {
        if count > remaining / width {
            return false;
        }
        remaining -= count * width;
        true
    };
    if !reserve(n, per_voxel)
        || !reserve(nx, 8 * axis_copies + 512)
        || !reserve(ny, 8 * axis_copies + 512)
        || !reserve(nz, 8 * axis_copies + 512)
        || !reserve(1, 8192)
    {
        return Err(Error::wrap(
            ERR_SIGNED_DISTANCE_LIMIT,
            format_args!("managed array reservation exceeds {bytes} bytes"),
        ));
    }
    Ok(n)
}
fn signed_distance(
    ctx: Option<&dyn Cancellation>,
    source: &VolumeMask,
    opts: SignedDistanceOptions,
) -> Result<Vec<f64>, Error> {
    let x = signed_distance_axis(ctx, &source.x)?;
    let y = signed_distance_axis(ctx, &source.y)?;
    let z = signed_distance_axis(ctx, &source.z)?;
    let mut boundary = VolumeMask {
        columns: source.columns,
        rows: source.rows,
        slices: source.slices,
        x,
        y,
        z,
        voxels: vec![false; source.voxels.len()],
    };
    let (nx, ny, nz) = (source.columns, source.rows, source.slices);
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                let index = ((z * ny + y) * nx + x) as usize;
                if index & 4095 == 0 {
                    ctx_check(ctx)?;
                }
                if !source.voxels[index] {
                    continue;
                }
                let mut zz = (z - 1).max(0);
                while zz <= (nz - 1).min(z + 1) && !boundary.voxels[index] {
                    let mut yy = (y - 1).max(0);
                    while yy <= (ny - 1).min(y + 1) && !boundary.voxels[index] {
                        for xx in (x - 1).max(0)..=(nx - 1).min(x + 1) {
                            if !source.voxels[((zz * ny + yy) * nx + xx) as usize] {
                                boundary.voxels[index] = true;
                                break;
                            }
                        }
                        yy += 1;
                    }
                    zz += 1;
                }
            }
        }
    }
    let mut out = boundary.distance_transform(ctx, true)?;
    for (i, &inside) in source.voxels.iter().enumerate() {
        if i & 4095 == 0 {
            ctx_check(ctx)?;
        }
        if opts.squared_distance {
            out[i] *= out[i];
        }
        if inside != opts.inside_is_positive {
            out[i] = -out[i];
        }
    }
    ctx_check(ctx)?;
    Ok(out)
}
pub(crate) fn signed_distance_axis(
    ctx: Option<&dyn Cancellation>,
    axis: &[f64],
) -> Result<Vec<f64>, Error> {
    for (i, &value) in axis.iter().enumerate() {
        if i & 4095 == 0 {
            ctx_check(ctx)?;
        }
        if !value.is_finite() || i > 0 && value <= axis[i - 1] {
            return Err(Error::wrap(
                ERR_INVALID_VOLUME_MASK,
                "finite ascending axes required",
            ));
        }
    }
    let span = axis[axis.len() - 1] - axis[0];
    if span.is_infinite() || span > (f64::MAX / 64.0).sqrt() {
        return Err(Error::wrap(
            ERR_INVALID_VOLUME_MASK,
            "squared coordinate span is not representable",
        ));
    }
    let mut out = vec![0.0f64; axis.len()];
    for (i, &value) in axis.iter().enumerate() {
        out[i] = value - axis[0];
        if i > 0 {
            let gap = out[i] - out[i - 1];
            if gap <= 0.0 || gap * gap == 0.0 {
                return Err(Error::wrap(
                    ERR_INVALID_VOLUME_MASK,
                    "squared coordinate spacing is not representable",
                ));
            }
        }
    }
    ctx_check(ctx)?;
    Ok(out)
}
